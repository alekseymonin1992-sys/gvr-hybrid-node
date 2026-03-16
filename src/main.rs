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

const DEV_KEY_FILE: &str = "dev_key.bin";

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// p2p listen address, e.g. 127.0.0.1:4000
    #[arg(long, default_value = "127.0.0.1:4000")]
    p2p_addr: String,

    /// comma-separated list of peers to connect to, e.g. 127.0.0.1:4001,127.0.0.1:4002
    #[arg(long)]
    peers: Option<String>,

    /// if set, node will not mine blocks, only validate and sync
    #[arg(long)]
    no_mine: bool,

    /// if set, node will periodically generate dev tx alice->bob (for testing)
    #[arg(long)]
    dev_gen_tx: bool,

    /// HTTP RPC bind address, e.g. 127.0.0.1:8080
    #[arg(long, default_value = "127.0.0.1:8080")]
    rpc_addr: String,
}

fn save_dev_key(path: &Path, sk: &SigningKey) -> anyhow::Result<()> {
    let bytes = sk.to_bytes();
    let mut f = fs::File::create(path)?;
    f.write_all(&bytes)?;
    Ok(())
}

fn load_dev_key(path: &Path) -> anyhow::Result<SigningKey> {
    let mut data = Vec::new();
    let mut f = fs::File::open(path)?;
    f.read_to_end(&mut data)?;
    if data.len() != 32 {
        anyhow::bail!("dev_key.bin has invalid length");
    }
    let arr: [u8; 32] = data
        .try_into()
        .map_err(|_| anyhow::anyhow!("invalid key length"))?;
    let sk = SigningKey::from_bytes(&arr)
        .map_err(|e| anyhow::anyhow!(format!("invalid key bytes: {}", e)))?;
    Ok(sk)
}

fn ensure_dev_key() -> SigningKey {
    let path = Path::new(DEV_KEY_FILE);
    if path.exists() {
        match load_dev_key(path) {
            Ok(sk) => {
                println!("🔐 Loaded dev key from {}", DEV_KEY_FILE);
                return sk;
            }
            Err(e) => {
                println!("⚠ Failed to load dev key: {}, will generate new one", e);
            }
        }
    }

    let mut seed = [0u8; 32];
    rand::thread_rng().fill(&mut seed);
    let sk = SigningKey::from_bytes(&seed).expect("create signing key");
    if let Err(e) = save_dev_key(path, &sk) {
        println!("⚠ Failed to save dev key: {}", e);
    } else {
        println!("🔐 Saved new dev key to {}", DEV_KEY_FILE);
    }
    sk
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

fn main() {
    let args = Args::parse();

    println!("🌱 Starting GVR Hybrid Node (dev mode)");

    let dev_sk = ensure_dev_key();
    let dev_vk = VerifyingKey::from(&dev_sk);
    let ai_pub_sec1 = dev_vk.to_encoded_point(false).as_bytes().to_vec();

    let chain_state = if Path::new(constants::SNAPSHOT_FILE).exists() {
        match Blockchain::load_state(Path::new(constants::SNAPSHOT_FILE)) {
            Ok(mut bc) => {
                println!("♻ Loaded chain state from {}", constants::SNAPSHOT_FILE);
                bc.active_ai_pubkey = Some(ai_pub_sec1.clone());
                bc
            }
            Err(e) => {
                println!("⚠ Failed to load snapshot: {}, creating new chain", e);
                Blockchain::new_with_genesis(Some(ai_pub_sec1.clone()))
            }
        }
    } else {
        Blockchain::new_with_genesis(Some(ai_pub_sec1.clone()))
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
        let dev_sk_clone = dev_sk.clone();
        let peers_for_broadcast = peers_vec.clone();

        thread::spawn(move || loop {
            let block = {
                let chain = miner_chain.lock().unwrap();
                let mp = miner_mempool.lock().unwrap();
                mine::mine_block(&chain, &mp, Some(&dev_sk_clone))
            };

            {
                let mut chain = miner_chain.lock().unwrap();
                let included_txs = block.transactions.clone();
                if chain.add_block(block.clone()) {
                    println!("✅ Block accepted idx={} reward={}", block.index, block.reward);
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
        let dev_sk_for_tx = dev_sk.clone();

        thread::spawn(move || loop {
            thread::sleep(Duration::from_secs(5));

            let nonce = {
                let bc = bc_for_tx.lock().unwrap();
                bc.state.nonce_of("alice")
            };

            let st = sign_transfer(&dev_sk_for_tx, "alice", "bob", 1, nonce);
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
                "🧾 Local signed dev tx created (nonce={}) and (maybe) broadcast",
                nonce
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

            // Сигнализируем P2P-тредам, что надо завершаться
            gvr_hybrid_node::p2p::signal_shutdown();

            // Даём им чуть-чуть времени корректно выйти
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

    // Status loop
    loop {
        thread::sleep(Duration::from_secs(3));
        let chain = blockchain.lock().unwrap();
        let state = &chain.state;
        let bal_alice = state.balance_of("alice");
        let bal_bob = state.balance_of("bob");
        let nonce_alice = state.nonce_of("alice");

        println!("================ NETWORK STATUS ================");
        println!("Height: {}", chain.chain.len());
        println!("Difficulty: {}", chain.difficulty);
        println!(
            "Total Supply: {} / {}",
            chain.total_supply,
            constants::MAX_SUPPLY
        );
        let progress = (chain.total_supply as f64 / constants::MAX_SUPPLY as f64) * 100.0;
        println!("Emission Progress: {:.4} %", progress);
        println!("Balances:");
        println!("  alice: {}", bal_alice);
        println!("  bob  : {}", bal_bob);
        println!("Nonces:");
        println!("  alice: {}", nonce_alice);
        println!("================================================\n");
    }
}