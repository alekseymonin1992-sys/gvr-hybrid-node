use std::fs;
use std::io::Read;
use std::net::TcpStream;
use std::path::Path;
use std::io::Write;

use clap::{Parser, Subcommand};
use k256::ecdsa::SigningKey;
use serde::{Serialize, Deserialize};

use gvr_hybrid_node::accounts::sign_transfer;
use gvr_hybrid_node::transaction::Transaction;

/// Путь к dev-ключу, такой же, как у ноды.
const DEV_KEY_FILE: &str = "dev_key.bin";

#[derive(Parser, Debug)]
#[command(author, version, about = "GVR P2P client", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Отправить Signed-транзакцию (как раньше)
    SendSigned {
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
    },

    /// Отправить RotateAIKey-транзакцию через P2P.
    /// Ожидает JSON файл с полями new_ai_pubkey_sec1/proposer/signature,
    /// таким как выводит gvr-ai-rotate --dry-run.
    RotateAi {
        /// P2P адрес ноды, например 127.0.0.1:4000
        #[arg(long, default_value = "127.0.0.1:4000")]
        p2p: String,

        /// Путь к JSON-файлу с RotateAIKey Tx DTO (вывод gvr-ai-rotate)
        #[arg(long)]
        json: String,
    },
}

/// Упрощённая копия P2pMessage для Tx, чтобы сформировать JSON так же, как в p2p.rs
#[derive(Serialize, Debug)]
#[serde(tag = "type", content = "payload")]
enum P2pMessage {
    Tx(Transaction),
}

/// DTO для чтения RotateAIKey JSON из файла
#[derive(Debug, Deserialize)]
#[serde(tag = "type", content = "payload")]
enum RotateAiFileDto {
    RotateAIKey {
        new_ai_pubkey_sec1: Vec<u8>,
        proposer: String,
        signature: Vec<u8>,
    },
}

fn main() -> Result<(), String> {
    let cli = Cli::parse();

    match cli.command {
        Commands::SendSigned {
            p2p,
            from,
            to,
            amount,
            nonce,
            fee,
        } => cmd_send_signed(&p2p, &from, &to, amount, nonce, fee),

        Commands::RotateAi { p2p, json } => cmd_rotate_ai(&p2p, &json),
    }
}

fn cmd_send_signed(
    p2p_addr: &str,
    from: &str,
    to: &str,
    amount: u64,
    nonce: u64,
    fee: u64,
) -> Result<(), String> {
    let sk = load_dev_key().map_err(|e| format!("failed to load dev key: {}", e))?;
    println!(
        "Sending P2P signed tx: from={} to={} amount={} nonce={} fee={} via {}",
        from, to, amount, nonce, fee, p2p_addr
    );

    let st = sign_transfer(&sk, from, to, amount, fee, nonce);
    let tx = Transaction::signed(st);

    let msg = P2pMessage::Tx(tx);
    send_p2p_message(p2p_addr, &msg)
}

fn cmd_rotate_ai(p2p_addr: &str, json_path: &str) -> Result<(), String> {
    println!(
        "Sending P2P RotateAIKey tx from JSON {} via {}",
        json_path, p2p_addr
    );

    // Читаем JSON-файл, который выдал gvr-ai-rotate --dry-run
    let mut data = String::new();
    fs::File::open(json_path)
        .map_err(|e| format!("failed to open JSON file {}: {}", json_path, e))?
        .read_to_string(&mut data)
        .map_err(|e| format!("failed to read JSON file: {}", e))?;

    let dto: RotateAiFileDto = serde_json::from_str(&data)
        .map_err(|e| format!("failed to parse RotateAIKey JSON: {}", e))?;

    let tx = match dto {
        RotateAiFileDto::RotateAIKey {
            new_ai_pubkey_sec1,
            proposer,
            signature,
        } => Transaction::rotate_ai_key(new_ai_pubkey_sec1, proposer, signature),
    };

    let msg = P2pMessage::Tx(tx);
    send_p2p_message(p2p_addr, &msg)
}

fn send_p2p_message(p2p_addr: &str, msg: &P2pMessage) -> Result<(), String> {
    let payload = serde_json::to_vec(msg).map_err(|e| format!("json encode error: {}", e))?;
    let len = (payload.len() as u32).to_be_bytes();

    let mut stream = TcpStream::connect(p2p_addr)
        .map_err(|e| format!("connect to {} failed: {}", p2p_addr, e))?;

    stream
        .write_all(&len)
        .and_then(|_| stream.write_all(&payload))
        .map_err(|e| format!("send error: {}", e))?;

    println!("P2P message sent");
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