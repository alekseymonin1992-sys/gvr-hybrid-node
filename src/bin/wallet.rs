use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;

use clap::{Parser, Subcommand};
use k256::ecdsa::SigningKey;
use k256::ecdsa::VerifyingKey;
use rand::thread_rng;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use gvr_hybrid_node::accounts::sign_transfer;

/// Папка, где хранятся кошельки (приватные ключи)
const WALLET_DIR: &str = "wallets";

#[derive(Parser, Debug)]
#[command(author, version, about = "GVR Wallet CLI", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Создать новый кошелёк (приватный ключ + адрес)
    New {
        /// Имя кошелька (будет использоваться как "from" адрес в state)
        #[arg(long)]
        name: String,
    },

    /// Показать данные кошелька (адрес и публичный ключ)
    Show {
        /// Имя кошелька
        #[arg(long)]
        name: String,
    },

    /// Отправить подписанную транзакцию через RPC /tx
    Send {
        /// RPC адрес ноды
        #[arg(long, default_value = "127.0.0.1:8080")]
        rpc: String,

        /// Имя кошелька-отправителя (должен быть создан через `new`)
        #[arg(long)]
        from_wallet: String,

        /// Адрес получателя (строка, как в state)
        #[arg(long)]
        to: String,

        /// Сумма перевода
        #[arg(long)]
        amount: u64,

        /// Комиссия за транзакцию (уходит майнеру)
        #[arg(long, default_value_t = 1)]
        fee: u64,
    },
}

/// DTO для /tx (тот же, что в client.rs)
#[derive(Debug, Serialize)]
struct SignedTransferDto {
    from: String,
    to: String,
    amount: u64,
    nonce: u64,
    pubkey_sec1: Vec<u8>,
    signature: Vec<u8>,
    fee: u64,
}

#[derive(Debug, Deserialize)]
struct StatusResponse {
    height: u64,
    tip: String,

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

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let res = match cli.command {
        Commands::New { name } => cmd_new_wallet(&name),
        Commands::Show { name } => cmd_show_wallet(&name),
        Commands::Send {
            rpc,
            from_wallet,
            to,
            amount,
            fee,
        } => cmd_send(&rpc, &from_wallet, &to, amount, fee).await,
    };

    if let Err(e) = res {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

/// Создать новый кошелёк: приватный ключ + адрес == name
fn cmd_new_wallet(name: &str) -> Result<(), String> {
    fs::create_dir_all(WALLET_DIR)
        .map_err(|e| format!("failed to create wallet dir: {}", e))?;

    let path = wallet_path(name);
    if path.exists() {
        return Err(format!(
            "wallet '{}' already exists at {}",
            name,
            path.display()
        ));
    }

    // Генерируем случайный приватный ключ
    let mut rng = thread_rng();
    let sk = SigningKey::random(&mut rng);
    let bytes = sk.to_bytes();

    let mut f = fs::File::create(&path)
        .map_err(|e| format!("failed to create wallet file: {}", e))?;
    f.write_all(&bytes)
        .map_err(|e| format!("failed to write wallet file: {}", e))?;

    // Адрес в нашей модели — это просто строка name (используется в state как addr)
    // Публичный ключ SEC1:
    let vk = VerifyingKey::from(&sk);
    let pub_sec1 = vk.to_encoded_point(false).as_bytes().to_vec();

    println!("Created wallet '{}':", name);
    println!("  file: {}", path.display());
    println!("  address (state addr): {}", name);
    println!("  pubkey_sec1 (hex): {}", hex::encode(pub_sec1));

    Ok(())
}

/// Показать данные кошелька
fn cmd_show_wallet(name: &str) -> Result<(), String> {
    let sk = load_wallet_sk(name)?;
    let vk = VerifyingKey::from(&sk);
    let pub_sec1 = vk.to_encoded_point(false).as_bytes().to_vec();

    println!("Wallet '{}':", name);
    println!("  file: {}", wallet_path(name).display());
    println!("  address (state addr): {}", name);
    println!("  pubkey_sec1 (hex): {}", hex::encode(pub_sec1));

    Ok(())
}

/// Отправить транзакцию: from_wallet -> to, amount, fee
async fn cmd_send(
    rpc_addr: &str,
    from_wallet: &str,
    to: &str,
    amount: u64,
    fee: u64,
) -> Result<(), String> {
    let sk = load_wallet_sk(from_wallet)?;

    let client = Client::new();
    let base_url = format!("http://{}", rpc_addr);

    // 1) Получаем nonce для from_wallet через /nonce?addr=...
    #[derive(Debug, Deserialize)]
    struct NonceResp {
        nonce: u64,
    }

    let nonce_url = format!("{}/nonce?addr={}", base_url, from_wallet);
    let nonce_resp = client
        .get(&nonce_url)
        .send()
        .await
        .map_err(|e| format!("nonce request error: {}", e))?;

    let nonce_status = nonce_resp.status();
    let nonce_body = nonce_resp.text().await.unwrap_or_default();

    if !nonce_status.is_success() {
        return Err(format!(
            "nonce request failed: status={} body={}",
            nonce_status, nonce_body
        ));
    }

    let nonce_parsed: NonceResp = serde_json::from_str(&nonce_body)
        .map_err(|e| format!("nonce json parse error: {}", e))?;
    let nonce = nonce_parsed.nonce;

    // 2) Для информации берём общий статус
    let status_url = format!("{}/status", base_url);
    let status = client
        .get(&status_url)
        .send()
        .await
        .map_err(|e| format!("status request error: {}", e))?
        .json::<StatusResponse>()
        .await
        .map_err(|e| format!("status json parse error: {}", e))?;

    println!(
        "Using nonce={} for from={} (height={}, tip={}), fee={}",
        nonce, from_wallet, status.height, status.tip, fee
    );

    // Адресом в state будет from_wallet (строка)
    let st = sign_transfer(&sk, from_wallet, to, amount, fee, nonce);
    let dto = SignedTransferDto {
        from: st.from.clone(),
        to: st.to.clone(),
        amount: st.amount,
        nonce: st.nonce,
        pubkey_sec1: st.pubkey_sec1.clone(),
        signature: st.signature.clone(),
        fee: st.fee,
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

fn wallet_path(name: &str) -> PathBuf {
    let mut p = PathBuf::from(WALLET_DIR);
    p.push(format!("{}.key", name));
    p
}

fn load_wallet_sk(name: &str) -> Result<SigningKey, String> {
    let path = wallet_path(name);
    let mut data = Vec::new();
    let mut f = fs::File::open(&path)
        .map_err(|e| format!("failed to open wallet file {}: {}", path.display(), e))?;
    f.read_to_end(&mut data)
        .map_err(|e| format!("failed to read wallet file: {}", e))?;
    if data.len() != 32 {
        return Err("wallet key file has invalid length".into());
    }
    let arr: [u8; 32] = data
        .try_into()
        .map_err(|_| "invalid key length in wallet file".to_string())?;
    let sk = SigningKey::from_bytes(&arr)
        .map_err(|e| format!("invalid key bytes: {}", e))?;
    Ok(sk)
}