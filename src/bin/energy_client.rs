use std::fs;
use std::io::Read;
use std::path::Path;

use clap::Parser;
use k256::ecdsa::{signature::Signer, SigningKey, VerifyingKey};
use reqwest::Client;
use serde::Serialize;

use gvr_hybrid_node::energy::EnergyProof;

/// Файл с приватным AI-ключом (тот же, что у ноды)
const AI_KEY_FILE: &str = "ai_key.bin";

#[derive(Parser, Debug)]
#[command(author, version, about = "GVR EnergyProof client", long_about = None)]
struct Cli {
    /// RPC адрес ноды
    #[arg(long, default_value = "127.0.0.1:8080")]
    rpc: String,

    /// producer_id для EnergyProof
    #[arg(long, default_value = "dev_producer")]
    producer_id: String,

    /// sequence (можно указывать вручную, нода дополнительно проверит монотонность)
    #[arg(long, default_value_t = 1)]
    sequence: u64,

    /// kWh за интервал
    #[arg(long, default_value_t = 100.0)]
    kwh: f64,

    /// AI-оценка качества (0.0..1.0)
    #[arg(long, default_value_t = 0.9)]
    ai_score: f64,

    /// Не использовать текущее время, а задать timestamp вручную (мс от UNIX_EPOCH)
    #[arg(long)]
    timestamp: Option<u128>,

    /// Не отправлять в ноду, а просто вывести JSON на экран
    #[arg(long)]
    dry_run: bool,
}

#[derive(Debug, Serialize)]
struct EnergyProofDto {
    producer_id: String,
    sequence: u64,
    kwh: f64,
    timestamp: u128,
    ai_score: f64,
    ai_signature: Vec<u8>,
    proof_id: Option<String>,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let rpc_url = format!("http://{}", cli.rpc);
    println!("Using RPC: {}", rpc_url);

    // 1. Загружаем AI-приватный ключ
    let sk = match load_ai_key() {
        Ok(sk) => sk,
        Err(e) => {
            eprintln!("Failed to load AI key: {}", e);
            std::process::exit(1);
        }
    };

    let vk = VerifyingKey::from(&sk);
    let pub_sec1 = vk.to_encoded_point(false).as_bytes().to_vec();
    // ИСПРАВЛЕНО: используем ссылку, чтобы не "съесть" pub_sec1
    println!("AI pubkey_sec1 (hex): {}", hex::encode(&pub_sec1));

    // 2. Формируем EnergyProof (без подписи)
    let ts = cli.timestamp.unwrap_or_else(current_time_ms);

    let mut proof = EnergyProof {
        producer_id: cli.producer_id.clone(),
        sequence: cli.sequence,
        kwh: cli.kwh,
        timestamp: ts,
        ai_score: cli.ai_score,
        ai_signature: Vec::new(),
        proof_id: None,
    };

    // 3. Подписываем
    let hash = proof.hash_for_signing();
    let sig: k256::ecdsa::Signature = sk.sign(&hash);
    let sig_der = sig.to_der().as_bytes().to_vec();
    proof.ai_signature = sig_der.clone();

    // 4. Проверяем локально поля и подпись (для себя)
    if let Err(e) = proof.validate_fields(gvr_hybrid_node::constants::MIN_AI_SCORE) {
        eprintln!("Local proof validation failed: {}", e);
        std::process::exit(1);
    }
    let sig_ok = proof.verify_signature(&pub_sec1).unwrap_or(false);
    if !sig_ok {
        eprintln!("Local signature verification FAILED (something is wrong)");
        std::process::exit(1);
    }

    // 5. Собираем DTO для /energy_proof
    let dto = EnergyProofDto {
        producer_id: proof.producer_id.clone(),
        sequence: proof.sequence,
        kwh: proof.kwh,
        timestamp: proof.timestamp,
        ai_score: proof.ai_score,
        ai_signature: proof.ai_signature.clone(),
        proof_id: proof.proof_id.clone(),
    };

    let dto_json = serde_json::to_string_pretty(&dto).unwrap();
    println!("EnergyProof DTO:\n{}\n", dto_json);

    if cli.dry_run {
        println!("dry_run: not sending to node");
        return;
    }

    // 6. Отправляем в ноду
    let client = Client::new();
    let url = format!("{}/energy_proof", rpc_url);
    println!("POST {}", url);

    let resp = match client.post(&url).json(&dto).send().await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Request failed: {}", e);
            std::process::exit(1);
        }
    };

    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    println!("Status: {}", status);
    println!("Body  : {}", body);

    if !status.is_success() {
        std::process::exit(1);
    }
}

fn load_ai_key() -> anyhow::Result<SigningKey> {
    let path = Path::new(AI_KEY_FILE);
    let mut data = Vec::new();
    let mut f = fs::File::open(path)?;
    f.read_to_end(&mut data)?;
    if data.len() != 32 {
        anyhow::bail!("ai_key.bin has invalid length");
    }
    let arr: [u8; 32] = data
        .try_into()
        .map_err(|_| anyhow::anyhow!("invalid key length"))?;
    let sk = SigningKey::from_bytes(&arr)
        .map_err(|e| anyhow::anyhow!(format!("invalid key bytes: {}", e)))?;
    Ok(sk)
}

fn current_time_ms() -> u128 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    now.as_millis() as u128
}