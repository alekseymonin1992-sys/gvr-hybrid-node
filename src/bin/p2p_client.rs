use std::fs;
use std::io::Read;
use std::net::TcpStream;
use std::path::Path;
use std::io::Write;

use clap::Parser;
use k256::ecdsa::SigningKey;
use serde::Serialize;

use gvr_hybrid_node::accounts::sign_transfer;
use gvr_hybrid_node::transaction::Transaction;

/// Путь к dev-ключу, такой же, как у ноды.
const DEV_KEY_FILE: &str = "dev_key.bin";

#[derive(Parser, Debug)]
#[command(author, version, about = "GVR P2P client", long_about = None)]
struct Cli {
    /// P2P адрес ноды, например 127.0.0.1:4000
    #[arg(long, default_value = "127.0.0.1:4000")]
    p2p: String,

    /// from-адрес
    #[arg(long, default_value = "alice")]
    from: String,

    /// to-адрес
    #[arg(long, default_value = "bob")]
    to: String,

    /// сумма перевода
    #[arg(long, default_value_t = 1)]
    amount: u64,

    /// nonce
    #[arg(long)]
    nonce: u64,

    /// комиссия за транзакцию
    #[arg(long, default_value_t = 0)]
    fee: u64,
}

// Упрощённая копия P2pMessage для Tx, чтобы сформировать JSON так же, как в p2p.rs
#[derive(Serialize, Debug)]
#[serde(tag = "type", content = "payload")]
enum P2pMessage {
    Tx(Transaction),
}

fn main() -> Result<(), String> {
    let cli = Cli::parse();

    let sk = load_dev_key().map_err(|e| format!("failed to load dev key: {}", e))?;
    println!(
        "Sending P2P tx: from={} to={} amount={} nonce={} fee={} via {}",
        cli.from, cli.to, cli.amount, cli.nonce, cli.fee, cli.p2p
    );

    // Подписываем перевод
    let st = sign_transfer(&sk, &cli.from, &cli.to, cli.amount, cli.fee, cli.nonce);
    let tx = Transaction::signed(st);

    // Упаковываем в P2pMessage::Tx
    let msg = P2pMessage::Tx(tx);

    // Сериализуем в JSON и отправляем с length-prefixed протоколом (как в p2p.rs)
    let payload = serde_json::to_vec(&msg).map_err(|e| format!("json encode error: {}", e))?;
    let len = (payload.len() as u32).to_be_bytes();

    let mut stream = TcpStream::connect(&cli.p2p)
        .map_err(|e| format!("connect to {} failed: {}", cli.p2p, e))?;

    // Отправляем длину и сам payload
    stream
        .write_all(&len)
        .and_then(|_| stream.write_all(&payload))
        .map_err(|e| format!("send error: {}", e))?;

    println!("P2P Tx sent");

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