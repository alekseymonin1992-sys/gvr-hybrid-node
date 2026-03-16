use std::fs;
use std::io::Read;
use std::path::Path;

use clap::{Parser, Subcommand};
use k256::ecdsa::SigningKey;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use gvr_hybrid_node::accounts::sign_transfer;

/// Путь к dev-ключу, такой же, как у ноды.
const DEV_KEY_FILE: &str = "dev_key.bin";

#[derive(Parser, Debug)]
#[command(author, version, about = "GVR client CLI", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Отправить подписанную транзакцию через RPC /tx (nonce берётся из /status)
    SendTx {
        #[arg(long, default_value = "127.0.0.1:8080")]
        rpc: String,

        #[arg(long, default_value = "alice")]
        from: String,

        #[arg(long)]
        to: String,

        #[arg(long)]
        amount: u64,
    },

    /// Отправить подписанную транзакцию через RPC /tx с явным nonce (без /status)
    SendTxRaw {
        #[arg(long, default_value = "127.0.0.1:8080")]
        rpc: String,

        #[arg(long, default_value = "alice")]
        from: String,

        #[arg(long)]
        to: String,

        #[arg(long)]
        amount: u64,

        #[arg(long)]
        nonce: u64,
    },

    /// Показать список пиров через RPC /peers
    Peers {
        #[arg(long, default_value = "127.0.0.1:8080")]
        rpc: String,
    },

    /// Показать общий статус ноды (/status)
    Status {
        #[arg(long, default_value = "127.0.0.1:8080")]
        rpc: String,
    },

    /// Показать баланс выбранного адреса (/balance)
    Balance {
        #[arg(long, default_value = "127.0.0.1:8080")]
        rpc: String,

        #[arg(long, default_value = "alice")]
        addr: String,
    },

    /// Показать nonce выбранного адреса (/nonce)
    Nonce {
        #[arg(long, default_value = "127.0.0.1:8080")]
        rpc: String,

        #[arg(long, default_value = "alice")]
        addr: String,
    },
}

#[derive(Debug, Deserialize)]
struct StatusResponse {
    height: u64,
    tip: String,
    alice_nonce: u64,

    #[allow(dead_code)]
    difficulty: u32,
    #[allow(dead_code)]
    total_supply: u64,
    #[allow(dead_code)]
    alice_balance: u64,
    #[allow(dead_code)]
    bob_balance: u64,
    #[allow(dead_code)]
    phase: String,
}

/// DTO для /tx
#[derive(Debug, Serialize)]
struct SignedTransferDto {
    from: String,
    to: String,
    amount: u64,
    nonce: u64,
    pubkey_sec1: Vec<u8>,
    signature: Vec<u8>,
}

/// DTO для /peers
#[derive(Debug, Deserialize)]
struct PeersResponse {
    peers: Vec<PeerInfo>,
}

#[derive(Debug, Deserialize)]
struct PeerInfo {
    addr: String,
    last_error_ts: u128,
    error_count: u32,
    banned_until: u128,
    last_contact_ts: u128,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let http_client = Client::new();

    let res = match cli.command {
        Commands::SendTx { rpc, from, to, amount } => {
            cmd_send_tx(&http_client, &rpc, &from, &to, amount).await
        }
        Commands::SendTxRaw { rpc, from, to, amount, nonce } => {
            cmd_send_tx_raw(&http_client, &rpc, &from, &to, amount, nonce).await
        }
        Commands::Peers { rpc } => cmd_peers(&http_client, &rpc).await,
        Commands::Status { rpc } => cmd_status(&http_client, &rpc).await,
        Commands::Balance { rpc, addr } => cmd_balance(&http_client, &rpc, &addr).await,
        Commands::Nonce { rpc, addr } => cmd_nonce(&http_client, &rpc, &addr).await,
    };

    if let Err(e) = res {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

async fn cmd_send_tx(
    client: &Client,
    rpc_addr: &str,
    from: &str,
    to: &str,
    amount: u64,
) -> Result<(), String> {
    let sk = load_dev_key().map_err(|e| format!("failed to load dev key: {}", e))?;

    let base_url = format!("http://{}", rpc_addr);

    let status_url = format!("{}/status", base_url);
    let status = client
        .get(&status_url)
        .send()
        .await
        .map_err(|e| format!("status request error: {}", e))?
        .json::<StatusResponse>()
        .await
        .map_err(|e| format!("status json parse error: {}", e))?;

    let nonce = if from == "alice" {
        status.alice_nonce
    } else {
        0
    };

    println!(
        "Using nonce={} for from={} (height={}, tip={})",
        nonce, from, status.height, status.tip
    );

    let st = sign_transfer(&sk, from, to, amount, nonce);
    let dto = SignedTransferDto {
        from: st.from.clone(),
        to: st.to.clone(),
        amount: st.amount,
        nonce: st.nonce,
        pubkey_sec1: st.pubkey_sec1.clone(),
        signature: st.signature.clone(),
    };

    let tx_url = format!("{}/tx", base_url);
    let resp = client
        .post(&tx_url)
        .json(&dto)
        .send()
        .await
        .map_err(|e| format!("tx send error: {}", e))?;

    let status_code = resp.status();
    let body_text = resp.text().await.unwrap_or_default();

    if !status_code.is_success() {
        return Err(format!(
            "tx rejected by node, status={} body={}",
            status_code, body_text
        ));
    }

    println!("TX accepted by node: {}", body_text);
    Ok(())
}

async fn cmd_send_tx_raw(
    client: &Client,
    rpc_addr: &str,
    from: &str,
    to: &str,
    amount: u64,
    nonce: u64,
) -> Result<(), String> {
    let sk = load_dev_key().map_err(|e| format!("failed to load dev key: {}", e))?;

    let base_url = format!("http://{}", rpc_addr);

    println!(
        "Sending raw tx: from={} to={} amount={} nonce={} (without /status)",
        from, to, amount, nonce
    );

    let st = sign_transfer(&sk, from, to, amount, nonce);
    let dto = SignedTransferDto {
        from: st.from.clone(),
        to: st.to.clone(),
        amount: st.amount,
        nonce: st.nonce,
        pubkey_sec1: st.pubkey_sec1.clone(),
        signature: st.signature.clone(),
    };

    let tx_url = format!("{}/tx", base_url);
    let resp = client
        .post(&tx_url)
        .json(&dto)
        .send()
        .await
        .map_err(|e| format!("tx send error: {}", e))?;

    let status_code = resp.status();
    let body_text = resp.text().await.unwrap_or_default();

    if !status_code.is_success() {
        return Err(format!(
            "tx rejected by node, status={} body={}",
            status_code, body_text
        ));
    }

    println!("TX accepted by node: {}", body_text);
    Ok(())
}

async fn cmd_peers(client: &Client, rpc_addr: &str) -> Result<(), String> {
    let base_url = format!("http://{}", rpc_addr);
    let url = format!("{}/peers", base_url);

    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("peers request error: {}", e))?;

    let status = resp.status();
    let body_text = resp.text().await.unwrap_or_default();

    if !status.is_success() {
        return Err(format!(
            "peers request failed: status={} body={}",
            status, body_text
        ));
    }

    let peers: PeersResponse =
        serde_json::from_str(&body_text).map_err(|e| format!("peers json parse error: {}", e))?;

    if peers.peers.is_empty() {
        println!("No peers known by this node.");
        return Ok(());
    }

    println!("Peers:");
    for p in peers.peers {
        println!(
            "  {} | errors={} last_error_ts={} banned_until={} last_contact_ts={}",
            p.addr, p.error_count, p.last_error_ts, p.banned_until, p.last_contact_ts
        );
    }

    Ok(())
}

async fn cmd_status(client: &Client, rpc_addr: &str) -> Result<(), String> {
    let url = format!("http://{}/status", rpc_addr);
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("status request error: {}", e))?;
    let text = resp.text().await.unwrap_or_default();
    println!("{}", text);
    Ok(())
}

async fn cmd_balance(client: &Client, rpc_addr: &str, addr: &str) -> Result<(), String> {
    let url = format!("http://{}/balance?addr={}", rpc_addr, addr);
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("balance request error: {}", e))?;
    let text = resp.text().await.unwrap_or_default();
    println!("{}", text);
    Ok(())
}

async fn cmd_nonce(client: &Client, rpc_addr: &str, addr: &str) -> Result<(), String> {
    let url = format!("http://{}/nonce?addr={}", rpc_addr, addr);
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("nonce request error: {}", e))?;
    let text = resp.text().await.unwrap_or_default();
    println!("{}", text);
    Ok(())
}

fn load_dev_key() -> anyhow::Result<SigningKey> {
    let path = Path::new(DEV_KEY_FILE);
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