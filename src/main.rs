use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use clap::Parser;
use k256::ecdsa::{SigningKey, VerifyingKey};
use rand::Rng;

use gvr_hybrid_node::accounts::sign_transfer;
use gvr_hybrid_node::blockchain::Blockchain;
use gvr_hybrid_node::constants;
use gvr_hybrid_node::mempool::Mempool;
use gvr_hybrid_node::mine;
use gvr_hybrid_node::p2p;
use gvr_hybrid_node::rpc;
use gvr_hybrid_node::transaction::Transaction;

const DEV_MINER_KEY_FILE: &str = "dev_key.bin";

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(long, default_value = "127.0.0.1:4000")]
    p2p_addr: String,

    #[arg(long)]
    peers: Option<String>,

    #[arg(long)]
    no_mine: bool,

    #[arg(long)]
    dev_gen_tx: bool,

    #[arg(long, default_value = "127.0.0.1:8080")]
    rpc_addr: String,

    /// Адрес, который будет получать награду за блок (coinbase)
    #[arg(long, default_value = "alice")]
    coinbase_addr: String,

    /// Путь к приватному AI-ключу (ECDSA k256), если хотим сами подписывать EnergyProof
    #[arg(long)]
    ai_key_file: Option<String>,

    /// Путь к файлу с публичным AI-ключом (SEC1), если только проверяем EnergyProof
    #[arg(long)]
    ai_pubkey_file: Option<String>,
}

fn save_sk(path: &Path, sk: &SigningKey) -> anyhow::Result<()> {
    let bytes = sk.to_bytes();
    let mut f = fs::File::create(path)?;
    f.write_all(&bytes)?;
    Ok(())
}

fn load_sk(path: &Path) -> anyhow::Result<SigningKey> {
    let mut data = Vec::new();
    let mut f = fs::File::open(path)?;
    f.read_to_end(&mut data)?;
    if data.len() != 32 {
        anyhow::bail!("key file has invalid length");
    }
    let arr: [u8; 32] = data
        .try_into()
        .map_err(|_| anyhow::anyhow!("invalid key length"))?;
    let sk = SigningKey::from_bytes(&arr)
        .map_err(|e| anyhow::anyhow!(format!("invalid key bytes: {}", e)))?;
    Ok(sk)
}

/// Ключ майнера (используется для coinbase и подписания dev-транзакций).
fn ensure_dev_miner_key() -> SigningKey {
    let path = Path::new(DEV_MINER_KEY_FILE);
    if path.exists() {
        match load_sk(path) {
            Ok(sk) => {
                println!("🔐 Loaded miner key from {}", DEV_MINER_KEY_FILE);
                return sk;
            }
            Err(e) => {
                println!(
                    "⚠ Failed to load miner key: {}, will generate new one",
                    e
                );
            }
        }
    }

    let mut seed = [0u8; 32];
    rand::thread_rng().fill(&mut seed);
    let sk = SigningKey::from_bytes(&seed).expect("create signing key");
    if let Err(e) = save_sk(path, &sk) {
        println!("⚠ Failed to save miner key: {}", e);
    } else {
        println!("🔐 Saved new miner key to {}", DEV_MINER_KEY_FILE);
    }
    sk
}

/// Инициализация AI-ключа:
/// 1) --ai-key-file: приватный ключ (подписываем + pubkey в active_ai_pubkey);
/// 2) --ai-pubkey-file: только паблик, нода проверяет EnergyProof, но не майнит их сама;
/// 3) ничего не задано: dev-режим — используем miner key как AI-ключ.
fn load_ai_key_from_args(
    args: &Args,
    miner_sk: &SigningKey,
) -> anyhow::Result<(Option<SigningKey>, Vec<u8>)> {
    // 1. Приватный AI-ключ
    if let Some(path_str) = &args.ai_key_file {
        let path = Path::new(path_str);
        let sk = load_sk(path)?;
        let vk = VerifyingKey::from(&sk);
        let pub_sec1 = vk.to_encoded_point(false).as_bytes().to_vec();
        println!("🤖 Loaded AI private key from {}", path_str);
        return Ok((Some(sk), pub_sec1));
    }

    // 2. Только паблик (SEC1)
    if let Some(path_str) = &args.ai_pubkey_file {
        let mut data = Vec::new();
        let mut f = fs::File::open(path_str)?;
        f.read_to_end(&mut data)?;
        println!("🤖 Loaded AI public key (SEC1) from {}", path_str);
        return Ok((None, data));
    }

    // 3. Dev-режим — miner key == AI key
    let vk = VerifyingKey::from(miner_sk);
    let pub_sec1 = vk.to_encoded_point(false).as_bytes().to_vec();
    println!("🤖 Dev mode: using miner key as AI key");
    Ok((Some(miner_sk.clone()), pub_sec1))
}

fn parse_peers(peers_arg: &Option<String>) -> Vec<String> {
    match peers_arg {
        None => Vec::new(),
        Some(s) => {
            if s.trim().is_empty() {
                Vec::new()
            } else {
                s.split(',')
                    .map(|p| p.trim().to_string())
                    .filter(|p| !p.is_empty())
                    .collect()
            }
        }
    }
}

/// Снимок статуса, чтобы печатать без долгих локов.
#[derive(Clone, Debug, Default)]
struct SharedStatus {
    height: usize,
    difficulty: u32,
    total_supply: u64,
    bal_alice: u64,
    bal_bob: u64,
    nonce_alice: u64,
}

fn main() {
    let args = Args::parse();

    println!("🌱 Starting GVR Hybrid Node");

    // Ключ майнера (coinbase, dev-транзакции)
    let miner_sk = ensure_dev_miner_key();

    // AI-ключ (подписывает EnergyProof) + активный AI pubkey
    let (ai_sk_opt, ai_pub_sec1) = match load_ai_key_from_args(&args, &miner_sk) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("❌ Failed to init AI key: {}", e);
            std::process::exit(1);
        }
    };

    let coinbase_addr = args.coinbase_addr.clone();

    let chain_state = if Path::new(constants::SNAPSHOT_FILE).exists() {
        match Blockchain::load_state(Path::new(constants::SNAPSHOT_FILE)) {
            Ok(mut bc) => {
                println!("♻ Loaded chain state from {}", constants::SNAPSHOT_FILE);
                bc.active_ai_pubkey = Some(ai_pub_sec1.clone());
                if bc.state.coinbase.is_empty() {
                    bc.state.coinbase = coinbase_addr.clone();
                }
                bc
            }
            Err(e) => {
                println!("⚠ Failed to load snapshot: {}, creating new chain", e);
                Blockchain::new_with_genesis(
                    Some(ai_pub_sec1.clone()),
                    coinbase_addr.clone(),
                )
            }
        }
    } else {
        Blockchain::new_with_genesis(Some(ai_pub_sec1.clone()), coinbase_addr.clone())
    };

    let blockchain: Arc<Mutex<Blockchain>> = Arc::new(Mutex::new(chain_state));
    let mempool: Arc<Mutex<Mempool>> = Arc::new(Mutex::new(Mempool::new()));

    let peers_vec = parse_peers(&args.peers);

    // P2P server
    {
        let bc_for_p2p = Arc::clone(&blockchain);
        let mp_for_p2p = Arc::clone(&mempool);
        let p2p_addr = args.p2p_addr.clone();
        let peers_for_thread = peers_vec.clone();
        thread::spawn(move || {
            p2p::start_p2p(&p2p_addr, peers_for_thread, bc_for_p2p, mp_for_p2p);
        });
    }

    // RPC server
    {
        let bc_for_rpc = Arc::clone(&blockchain);
        let mp_for_rpc = Arc::clone(&mempool);
        let rpc_addr = args.rpc_addr.clone();
        rpc::start_rpc(&rpc_addr, bc_for_rpc, mp_for_rpc);
    }

    // Miner
    if !args.no_mine {
        let miner_chain: Arc<Mutex<Blockchain>> = Arc::clone(&blockchain);
        let miner_mempool: Arc<Mutex<Mempool>> = Arc::clone(&mempool);
        let ai_sk_clone = ai_sk_opt.clone();
        let peers_for_broadcast = peers_vec.clone();

        thread::spawn(move || loop {
            // потокобезопасный майнер (без долгих локов)
            let block = mine::mine_block_threadsafe(
                &miner_chain,
                &miner_mempool,
                ai_sk_clone.as_ref(),
            );

            {
                let mut chain = miner_chain.lock().unwrap();
                let included_txs = block.transactions.clone();
                if chain.add_block(block.clone()) {
                    println!(
                        "✅ Block accepted idx={} reward={}",
                        block.index, block.reward
                    );
                    {
                        let mut mp = miner_mempool.lock().unwrap();
                        mp.remove_included(&included_txs);
                    }
                    if !peers_for_broadcast.is_empty() {
                        p2p::broadcast_block(&block, &peers_for_broadcast);
                    }
                } else {
                    println!("❌ Local block rejected idx={}", block.index);
                }
            }

            thread::sleep(Duration::from_millis(100));
        });
    } else {
        println!("⏸ Mining disabled on this node (--no-mine)");
    }

    // Dev-генерация подписанных транзакций alice -> bob
    if !args.no_mine && args.dev_gen_tx {
        println!("🧪 Dev tx generation enabled (--dev-gen-tx)");
        let mp_for_txs: Arc<Mutex<Mempool>> = Arc::clone(&mempool);
        let peers_for_tx = peers_vec.clone();
        let bc_for_tx: Arc<Mutex<Blockchain>> = Arc::clone(&blockchain);
        let miner_sk_for_tx = miner_sk.clone();

        thread::spawn(move || loop {
            thread::sleep(Duration::from_secs(5));

            let nonce = {
                let bc = bc_for_tx.lock().unwrap();
                bc.state.nonce_of("alice")
            };

            // Для dev-транзакций задаём фиксированную fee, например 1 GVR
            let fee: u64 = 1;

            let st = sign_transfer(&miner_sk_for_tx, "alice", "bob", 1, fee, nonce);
            let tx = Transaction::signed(st);

            {
                let mut mp = mp_for_txs.lock().unwrap();
                let h = mp.add_tx(tx.clone());
                if h.is_empty() {
                    println!("❌ Dev signed tx rejected by mempool");
                    continue;
                }
            }

            if !peers_for_tx.is_empty() {
                p2p::broadcast_tx(&tx, &peers_for_tx);
            }

            println!(
                "🧾 Local signed dev tx created (nonce={}, fee={}) and (maybe) broadcast",
                nonce, fee
            );
        });
    } else if args.dev_gen_tx && args.no_mine {
        println!("⚠ --dev-gen-tx ignored because --no-mine is set");
    }

    // Ctrl+C handler
    {
        let bc: Arc<Mutex<Blockchain>> = Arc::clone(&blockchain);
        ctrlc::set_handler(move || {
            println!("🛑 Received Ctrl+C, saving state...");

            gvr_hybrid_node::p2p::signal_shutdown();
            std::thread::sleep(std::time::Duration::from_millis(200));

            let guard = bc.lock().unwrap();
            if let Err(e) = guard.save_state(Path::new(constants::SNAPSHOT_FILE)) {
                println!("⚠ Failed to save snapshot: {}", e);
            } else {
                println!("💾 Snapshot saved to {}", constants::SNAPSHOT_FILE);
            }
            std::process::exit(0);
        })
        .expect("Error setting Ctrl-C handler");
    }

    // Shared status
    let shared_status = Arc::new(Mutex::new(SharedStatus::default()));
    let status_chain = Arc::clone(&blockchain);
    let status_shared = Arc::clone(&shared_status);

    // Status loop in separate thread
    thread::spawn(move || {
        println!("DEBUG: entering status loop");
        loop {
            {
                let chain = status_chain.lock().unwrap();
                let state = &chain.state;
                let mut snap = status_shared.lock().unwrap();

                snap.height = chain.chain.len();
                snap.difficulty = chain.difficulty;
                snap.total_supply = chain.total_supply;
                snap.bal_alice = state.balance_of("alice");
                snap.bal_bob = state.balance_of("bob");
                snap.nonce_alice = state.nonce_of("alice");
            }

            {
                let snap = status_shared.lock().unwrap();
                println!("================ NETWORK STATUS ================");
                println!("Height: {}", snap.height);
                println!("Difficulty: {}", snap.difficulty);
                println!(
                    "Total Supply: {} / {}",
                    snap.total_supply,
                    constants::MAX_SUPPLY
                );
                let progress =
                    (snap.total_supply as f64 / constants::MAX_SUPPLY as f64) * 100.0;
                println!("Emission Progress: {:.4} %", progress);
                println!("Balances:");
                println!("  alice: {}", snap.bal_alice);
                println!("  bob  : {}", snap.bal_bob);
                println!("Nonces:");
                println!("  alice: {}", snap.nonce_alice);
                println!("================================================\n");
            }

            thread::sleep(Duration::from_secs(10));
        }
    });

    loop {
        thread::sleep(Duration::from_secs(60));
    }
}