use std::fs;
use std::io::{Read, Write};
use std::path::Path;

use clap::Parser;
use k256::ecdsa::{signature::Signer, SigningKey, VerifyingKey};
use rand::thread_rng;
use serde::Serialize;

use sha2::{Sha256, Digest};

/// Файл со старым AI-приватным ключом (текущий активный в сети)
const OLD_AI_KEY_FILE: &str = "ai_key.bin";

#[derive(Parser, Debug)]
#[command(author, version, about = "GVR AI key rotation client", long_about = None)]
struct Cli {
    /// RPC адрес ноды (пока используется только для отображения, tx отправляем через P2P)
    #[arg(long, default_value = "127.0.0.1:8080")]
    rpc: String,

    /// Имя "proposer" для RotateAIKey (например, admin-addr или alice)
    #[arg(long, default_value = "ai-admin")]
    proposer: String,

    /// Куда сохранить НОВЫЙ приватный AI-ключ
    #[arg(long, default_value = "ai_key_new.bin")]
    out_key: String,

    /// Не отправлять транзакцию, только вывести JSON и записать новый ключ
    #[arg(long)]
    dry_run: bool,
}

/// DTO для RotateAIKey в формате P2P Transaction
#[derive(Debug, Serialize)]
#[serde(tag = "type", content = "payload")]
enum TxDto {
    RotateAIKey {
        new_ai_pubkey_sec1: Vec<u8>,
        proposer: String,
        signature: Vec<u8>,
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let rpc_url = format!("http://{}", cli.rpc);
    println!("Using RPC (for info): {}", rpc_url);

    // 1. Загружаем СТАРЫЙ AI-приватный ключ
    let old_sk = match load_ai_key(Path::new(OLD_AI_KEY_FILE)) {
        Ok(sk) => sk,
        Err(e) => {
            eprintln!("Failed to load old AI key {}: {}", OLD_AI_KEY_FILE, e);
            std::process::exit(1);
        }
    };

    let old_vk = VerifyingKey::from(&old_sk);
    let old_pub_sec1 = old_vk.to_encoded_point(false).as_bytes().to_vec();
    println!("Old AI pubkey_sec1 (hex): {}", hex::encode(&old_pub_sec1));

    // 2. Генерируем НОВЫЙ AI-ключ
    let mut rng = thread_rng();
    let new_sk = SigningKey::random(&mut rng);
    let new_bytes = new_sk.to_bytes();

    let new_vk = VerifyingKey::from(&new_sk);
    let new_pub_sec1 = new_vk.to_encoded_point(false).as_bytes().to_vec();
    println!("New AI pubkey_sec1 (hex): {}", hex::encode(&new_pub_sec1));

    // 3. Сохраняем новый приватный ключ в out_key
    if let Err(e) = save_ai_key(Path::new(&cli.out_key), &new_bytes) {
        eprintln!("Failed to save new AI key to {}: {}", cli.out_key, e);
        std::process::exit(1);
    }
    println!("New AI private key saved to {}", cli.out_key);

    // 4. Формируем сообщение для подписи RotateAIKey:
    // hash(proposer || 0x00 || new_ai_pubkey_sec1)
    let mut hasher = Sha256::new();
    hasher.update(cli.proposer.as_bytes());
    hasher.update(&[0u8]);
    hasher.update(&new_pub_sec1);
    let msg_hash = hasher.finalize();

    let sig: k256::ecdsa::Signature = old_sk.sign(msg_hash.as_slice());
    let sig_der = sig.to_der().as_bytes().to_vec();

    // 5. Собираем DTO транзакции RotateAIKey
    let tx = TxDto::RotateAIKey {
        new_ai_pubkey_sec1: new_pub_sec1.clone(),
        proposer: cli.proposer.clone(),
        signature: sig_der.clone(),
    };

    let tx_json = serde_json::to_string_pretty(&tx).unwrap();
    println!("RotateAIKey Tx DTO (for P2P):\n{}\n", tx_json);

    if cli.dry_run {
        println!("dry_run: not sending to node");
        return;
    }

    println!("NOTE: RotateAIKey Tx сформирована. Отправку в сеть нужно сделать через P2P (аналогично p2p_client).");
    println!("Сейчас gvr-ai-rotate не шлёт её автоматически, только подготавливает JSON и новый ключ.");
}

fn load_ai_key(path: &Path) -> anyhow::Result<SigningKey> {
    let mut data = Vec::new();
    let mut f = fs::File::open(path)?;
    f.read_to_end(&mut data)?;
    if data.len() != 32 {
        anyhow::bail!("AI key file has invalid length");
    }
    let arr: [u8; 32] = data
        .try_into()
        .map_err(|_| anyhow::anyhow!("invalid key length"))?;
    let sk = SigningKey::from_bytes(&arr)
        .map_err(|e| anyhow::anyhow!(format!("invalid key bytes: {}", e)))?;
    Ok(sk)
}

fn save_ai_key(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let mut f = fs::File::create(path)?;
    f.write_all(bytes)?;
    Ok(())
}