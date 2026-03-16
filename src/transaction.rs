use serde::{Deserialize, Serialize};

use crate::accounts::SignedTransfer;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Transaction {
    /// Старый простой трансфер (без подписи) — оставляем для совместимости.
    Transfer {
        sender: String,
        receiver: String,
        amount: u64,
    },
    /// Поворот AI-ключа (как было).
    RotateAIKey {
        new_ai_pubkey_sec1: Vec<u8>,
        proposer: String,
        signature: Vec<u8>,
    },
    /// Новый подписанный перевод.
    Signed(SignedTransfer),
}

impl Transaction {
    pub fn transfer(sender: String, receiver: String, amount: u64) -> Self {
        Transaction::Transfer {
            sender,
            receiver,
            amount,
        }
    }

    pub fn rotate_ai_key(
        new_ai_pubkey_sec1: Vec<u8>,
        proposer: String,
        signature: Vec<u8>,
    ) -> Self {
        Transaction::RotateAIKey {
            new_ai_pubkey_sec1,
            proposer,
            signature,
        }
    }

    pub fn signed(st: SignedTransfer) -> Self {
        Transaction::Signed(st)
    }
}