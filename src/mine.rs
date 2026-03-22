use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use k256::ecdsa::signature::Signer;
use k256::ecdsa::SigningKey;

use crate::block::Block;
use crate::blockchain::Blockchain;
use crate::energy::EnergyProof;
use crate::mempool::Mempool;
use crate::transaction::Transaction;

/// Dev-майнер: строит новый блок,
/// - получает уже выбранные транзакции,
/// - добавляет EnergyProof, если есть ключ.
pub fn mine_block(
    chain: &Blockchain,
    selected_txs: Vec<Transaction>,
    sk_opt: Option<&SigningKey>,
) -> Block {
    let index = chain.chain.len() as u64;
    let previous_hash = chain.last_hash();
    let difficulty = chain.difficulty;

    let transactions = selected_txs;

    let energy_proof = sk_opt.and_then(|sk| {
        // Если на ноде не настроен active_ai_pubkey — не делаем EnergyProof.
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

/// Потокобезопасная версия:
/// - под локами берёт срез состояния блокчейна и выбирает транзакции из mempool,
/// - затем майнит блок без дальнейших локов.
pub fn mine_block_threadsafe(
    chain_arc: &Arc<Mutex<Blockchain>>,
    mempool_arc: &Arc<Mutex<Mempool>>,
    sk_opt: Option<&SigningKey>,
) -> Block {
    // 1. Под локами берём ссылку на chain и выбираем tx из mempool
    let (chain_snapshot, selected_txs) = {
        let chain_guard = chain_arc.lock().unwrap();
        let mempool_guard = mempool_arc.lock().unwrap();

        let max_txs = 100usize;
        let txs = mempool_guard.select_for_block(max_txs);

        // Клонируем только сам Blockchain (он у тебя Clone) и список tx.
        (chain_guard.clone(), txs)
    };

    // 2. Майним блок на копии chain и уже выбранных транзакциях
    mine_block(&chain_snapshot, selected_txs, sk_opt)
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