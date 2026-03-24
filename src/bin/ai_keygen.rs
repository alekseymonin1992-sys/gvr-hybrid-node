use std::fs::File;
use std::io::Write;
use std::path::Path;

use k256::ecdsa::{SigningKey, VerifyingKey};
use rand::thread_rng;

fn main() {
    let mut rng = thread_rng();
    let sk = SigningKey::random(&mut rng);
    let bytes = sk.to_bytes();

    // Сохраняем приватный ключ
    let priv_path = Path::new("ai_key.bin");
    let mut f = File::create(priv_path).expect("create ai_key.bin");
    f.write_all(&bytes).expect("write ai_key.bin");

    // Публичный ключ SEC1 (uncompressed)
    let vk = VerifyingKey::from(&sk);
    let pub_sec1 = vk.to_encoded_point(false).as_bytes().to_vec();

    // Сохраняем паблик как сырой SEC1 в ai_pubkey.bin
    let pub_path = Path::new("ai_pubkey.bin");
    let mut f2 = File::create(pub_path).expect("create ai_pubkey.bin");
    f2.write_all(&pub_sec1).expect("write ai_pubkey.bin");

    println!("AI key generated:");
    println!("  private: {} (32 bytes)", priv_path.display());
    println!("  public : {} (SEC1 uncompressed)", pub_path.display());
    println!("  pubkey_sec1 (hex): {}", hex::encode(pub_sec1));
}