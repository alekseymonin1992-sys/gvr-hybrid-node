use crate::energy::EnergyProof;
use crate::transaction::Transaction;
use chrono;
use hex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Block {
    pub index: u64,
    pub previous_hash: String,
    pub timestamp: u128,
    pub transactions: Vec<Transaction>,
    pub nonce: u64,
    pub difficulty: u32,

    pub energy_proof: Option<EnergyProof>,
    pub reward: u64,

    pub hash: String,
}

impl Block {
    pub fn new(
        index: u64,
        previous_hash: String,
        transactions: Vec<Transaction>,
        difficulty: u32,
        energy_proof: Option<EnergyProof>,
        reward: u64,
    ) -> Self {
        let timestamp = chrono::Utc::now().timestamp_millis() as u128;

        let mut block = Block {
            index,
            previous_hash,
            timestamp,
            transactions,
            nonce: 0,
            difficulty,
            energy_proof,
            reward,
            hash: String::new(),
        };

        block.mine();
        block
    }

    fn mine(&mut self) {
        loop {
            let hash = self.calculate_hash();
            if hash.starts_with(&"0".repeat(self.difficulty as usize)) {
                self.hash = hash;
                break;
            }
            self.nonce += 1;
        }
    }

    pub fn calculate_hash(&self) -> String {
        let mut hasher = Sha256::new();

        hasher.update(self.index.to_string());
        hasher.update(&self.previous_hash);
        hasher.update(self.timestamp.to_string());

        for tx in &self.transactions {
            match tx {
                Transaction::Transfer {
                    sender,
                    receiver,
                    amount,
                } => {
                    hasher.update(sender);
                    hasher.update(receiver);
                    hasher.update(amount.to_string());
                }
                Transaction::RotateAIKey {
                    new_ai_pubkey_sec1,
                    proposer,
                    signature,
                } => {
                    hasher.update(proposer);
                    hasher.update(hex::encode(new_ai_pubkey_sec1));
                    hasher.update(hex::encode(signature));
                }
                Transaction::Signed(st) => {
                    hasher.update(&st.from);
                    hasher.update(&st.to);
                    hasher.update(st.amount.to_string());
                    // fee не включаем в хэш блока, можно добавить позже
                    hasher.update(hex::encode(&st.pubkey_sec1));
                    hasher.update(hex::encode(&st.signature));
                }
            }
        }

        hasher.update(self.nonce.to_string());
        hasher.update(self.difficulty.to_string());

        if let Some(proof) = &self.energy_proof {
            let cb = proof.canonical_bytes();
            hasher.update(cb);
        } else {
            hasher.update("no_proof");
        }

        hex::encode(hasher.finalize())
    }

    pub fn genesis() -> Self {
        let index = 0u64;
        let previous_hash = "0".to_string();
        let timestamp = 1_700_000_000_000u128; // фикс
        let transactions = Vec::new();
        let nonce = 0u64;
        let difficulty = 1u32;
        let energy_proof = None;
        let reward = 0u64;

        let mut block = Block {
            index,
            previous_hash,
            timestamp,
            transactions,
            nonce,
            difficulty,
            energy_proof,
            reward,
            hash: String::new(),
        };

        block.hash = block.calculate_hash();
        block
    }
}