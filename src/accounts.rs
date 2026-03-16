use k256::ecdsa::{signature::Signer, signature::Verifier, Signature, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Аккаунт в системе: публичный ключ и адрес (упрощенный).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Account {
    /// SEC1-encoded uncompressed public key
    pub pubkey_sec1: Vec<u8>,
    /// Человеко-читаемый адрес (для state), например "alice"
    pub address: String,
}

impl Account {
    /// Создать аккаунт из SigningKey и адреса (имени).
    pub fn from_signing_key(sk: &SigningKey, address: String) -> Self {
        let vk = VerifyingKey::from(sk);
        let pub_sec1 = vk.to_encoded_point(false).as_bytes().to_vec();
        Account {
            pubkey_sec1: pub_sec1,
            address,
        }
    }

    /// Получить VerifyingKey
    pub fn verifying_key(&self) -> Result<VerifyingKey, String> {
        VerifyingKey::from_sec1_bytes(&self.pubkey_sec1)
            .map_err(|e| format!("invalid pubkey: {}", e))
    }
}

/// Подписанный перевод: отправитель, получатель, сумма, nonce и подпись.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SignedTransfer {
    pub from: String,   // адрес отправителя (должен совпадать с account.address)
    pub to: String,     // адрес получателя
    pub amount: u64,
    pub nonce: u64,     // sequence для защиты от replay / упорядочивания
    pub pubkey_sec1: Vec<u8>,   // публичный ключ отправителя
    pub signature: Vec<u8>,     // DER-подпись
}

impl SignedTransfer {
    /// Canonical bytes для хеширования/подписи.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend(self.from.as_bytes());
        buf.push(0u8);
        buf.extend(self.to.as_bytes());
        buf.push(0u8);
        buf.extend(self.amount.to_le_bytes());
        buf.extend(self.nonce.to_le_bytes());
        buf
    }

    pub fn hash_for_signing(&self) -> [u8; 32] {
        let data = self.canonical_bytes();
        let mut hasher = Sha256::new();
        hasher.update(&data);
        let out = hasher.finalize();
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&out);
        arr
    }

    /// Проверить подпись.
    pub fn verify(&self) -> Result<bool, String> {
        let vk = VerifyingKey::from_sec1_bytes(&self.pubkey_sec1)
            .map_err(|e| format!("invalid pubkey: {}", e))?;

        let sig = Signature::from_der(&self.signature)
            .map_err(|_| "invalid signature format (DER expected)".to_string())?;

        let msg = self.hash_for_signing();
        match vk.verify(&msg, &sig) {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }
}

/// Подписать перевод при помощи приватного ключа.
pub fn sign_transfer(
    sk: &SigningKey,
    from_addr: &str,
    to_addr: &str,
    amount: u64,
    nonce: u64,
) -> SignedTransfer {
    let vk = VerifyingKey::from(sk);
    let pub_sec1 = vk.to_encoded_point(false).as_bytes().to_vec();

    let base = SignedTransfer {
        from: from_addr.to_string(),
        to: to_addr.to_string(),
        amount,
        nonce,
        pubkey_sec1: pub_sec1,
        signature: Vec::new(),
    };

    let msg = base.hash_for_signing();
    let sig: Signature = sk.sign(&msg);

    SignedTransfer {
        signature: sig.to_der().as_bytes().to_vec(),
        ..base
    }
}