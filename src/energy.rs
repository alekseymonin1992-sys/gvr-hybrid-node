use k256::ecdsa::{signature::Verifier, Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::convert::TryInto;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::constants::{SCALE, MAX_KWH_PER_PROOF};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct EnergyProof {
    pub producer_id: String,
    pub sequence: u64,
    pub kwh: f64,
    pub timestamp: u128,
    pub ai_score: f64,
    pub ai_signature: Vec<u8>,
    pub proof_id: Option<String>,
}

impl EnergyProof {
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::new();

        let pid_bytes = self.producer_id.as_bytes();
        let len: u16 = pid_bytes.len().try_into().unwrap_or(u16::MAX);
        buf.extend(&len.to_be_bytes());
        buf.extend(pid_bytes);

        buf.extend(&self.sequence.to_be_bytes());
        buf.extend(&self.kwh.to_be_bytes());
        buf.extend(&self.timestamp.to_be_bytes());
        buf.extend(&self.ai_score.to_be_bytes());

        match &self.proof_id {
            Some(id) => {
                let idb = id.as_bytes();
                let idlen: u16 = idb.len().try_into().unwrap_or(u16::MAX);
                buf.extend(&idlen.to_be_bytes());
                buf.extend(idb);
            }
            None => {
                buf.extend(&0u16.to_be_bytes());
            }
        }

        buf
    }

    pub fn hash_for_signing(&self) -> [u8; 32] {
        let data = self.canonical_bytes();
        let mut hasher = Sha256::new();
        hasher.update(&data);
        let result = hasher.finalize();
        result.as_slice().try_into().expect("sha256 output len")
    }

    pub fn verify_signature(&self, ai_pubkey_sec1: &[u8]) -> Result<bool, String> {
        // Construct verifying key
        let vk = VerifyingKey::from_sec1_bytes(ai_pubkey_sec1)
            .map_err(|e| format!("invalid ai_pubkey: {}", e))?;

        // Prefer DER-encoded signature
        let sig = Signature::from_der(&self.ai_signature)
            .map_err(|_| "invalid signature format (DER expected)".to_string())?;

        let hash = self.hash_for_signing();

        match vk.verify(&hash, &sig) {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    /// Строгая проверка полей EnergyProof.
    /// Для боевой сети:
    /// - kwh > 0 и <= MAX_KWH_PER_PROOF
    /// - ai_score >= min_ai_score
    /// - timestamp не слишком в будущем.
    pub fn validate_fields(&self, min_ai_score: f64) -> Result<(), String> {
        if !(self.kwh > 0.0) {
            return Err("kwh must be > 0".into());
        }
        if self.kwh > MAX_KWH_PER_PROOF {
            return Err(format!(
                "kwh too large: got={} max={}",
                self.kwh, MAX_KWH_PER_PROOF
            ));
        }

        if !(self.ai_score >= min_ai_score) {
            return Err("ai_score below minimum".into());
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| format!("time error: {}", e))?;
        let now_ms = (now.as_millis()) as u128;
        const MAX_FUTURE_SKEW_MS: u128 = 5 * 60 * 1000;
        if self.timestamp > now_ms + MAX_FUTURE_SKEW_MS {
            return Err("timestamp too far in future".into());
        }

        Ok(())
    }

    pub fn kwh_fp(&self) -> u128 {
        if !self.kwh.is_finite() || self.kwh <= 0.0 {
            return 0;
        }
        let v = (self.kwh * SCALE as f64).round();
        if v < 0.0 {
            0
        } else {
            v as u128
        }
    }

    pub fn ai_score_fp(&self) -> u128 {
        if !self.ai_score.is_finite() || self.ai_score < 0.0 {
            return 0;
        }
        let v = (self.ai_score * SCALE as f64).round();
        if v < 0.0 {
            0
        } else {
            v as u128
        }
    }
}