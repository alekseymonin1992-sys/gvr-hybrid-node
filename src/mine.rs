use std::time::{SystemTime, UNIX_EPOCH};

use k256::ecdsa::signature::Signer;
use k256::ecdsa::SigningKey;

use crate::block::Block;
use crate::blockchain::Blockchain;
use crate::energy::EnergyProof;
use crate::mempool::Mempool;

/// Dev-майнер: строит новый блок,
/// - включает транзакции из mempool (до max_txs),
/// - добавляет EnergyProof, если есть ключ.
pub fn mine_block(
    chain: &Blockchain,
    mempool: &Mempool,
    sk_opt: Option<&SigningKey>,
) -> Block {
    let index = chain.chain.len() as u64;
    let previous_hash = chain.last_hash();
    let difficulty = chain.difficulty;

    // включаем транзакции из mempool
    let max_txs = 100usize;
    let transactions = mempool.select_for_block(max_txs);

    let energy_proof = sk_opt.and_then(|sk| {
        if chain.active_ai_pubkey.is_none() {
            return None;
        }

        let producer_id = "dev_producer".to_string();
        let sequence = next_sequence_for_producer(chain, &producer_id);
        let kwh = 100.0_f64;
        let ai_score = 0.9_f64;
        let timestamp = current_time_ms();

        let mut proof = EnergyProof {
            producer_id: producer_id.clone(),
            sequence,
            kwh,
            timestamp,
            ai_score,
            ai_signature: Vec::new(),
            proof_id: None,
        };

        let msg_hash = proof.hash_for_signing();
        let sig: k256::ecdsa::Signature = sk.sign(&msg_hash);
        proof.ai_signature = sig.to_der().as_bytes().to_vec();

        Some(proof)
    });

    let reward = 0u64;

    Block::new(
        index,
        previous_hash,
        transactions,
        difficulty,
        energy_proof,
        reward,
    )
}

fn next_sequence_for_producer(chain: &Blockchain, producer_id: &str) -> u64 {
    if let Some(ps) = chain.producer_state.get(producer_id) {
        ps.last_seq.saturating_add(1)
    } else {
        1
    }
}

fn current_time_ms() -> u128 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    now.as_millis() as u128
}