use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use k256::ecdsa::signature::Signer;
use k256::ecdsa::SigningKey;

use crate::block::Block;
use crate::blockchain::Blockchain;
use crate::energy::EnergyProof;
use crate::mempool::Mempool;
use crate::transaction::Transaction;

/// Майнер: строит новый блок.
/// Если external_proof_opt задан, используем его.
/// Иначе (dev-режим) можем сгенерировать EnergyProof локально при наличии sk_opt.
pub fn mine_block(
    chain: &Blockchain,
    selected_txs: Vec<Transaction>,
    sk_opt: Option<&SigningKey>,
    external_proof_opt: Option<EnergyProof>,
) -> Block {
    let index = chain.chain.len() as u64;
    let previous_hash = chain.last_hash();
    let difficulty = chain.difficulty;

    let transactions = selected_txs;

    // Логика выбора EnergyProof:
    // 1) если передан внешний валидный EnergyProof — просто используем его;
    // 2) иначе, в dev-режиме, можем сгенерировать "фиктивный" proof с помощью sk_opt.
    let energy_proof: Option<EnergyProof> = if let Some(ext) = external_proof_opt {
        println!(
            "Miner: using external EnergyProof producer_id={} seq={} kwh={} ai_score={}",
            ext.producer_id, ext.sequence, ext.kwh, ext.ai_score
        );
        Some(ext)
    } else {
        if sk_opt.is_some() && chain.active_ai_pubkey.is_some() {
            println!("Miner: no external EnergyProof, using dev internal proof");
        } else {
            println!("Miner: no EnergyProof (external or internal) will be attached");
        }

        sk_opt.and_then(|sk| {
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
        })
    };

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

/// Потокобезопасная версия с внешним EnergyProof:
/// - под локами берёт chain и tx из mempool,
/// - под теми же локами читает последний external EnergyProof (если есть),
/// - затем майнит блок без дальнейших локов.
pub fn mine_block_threadsafe_with_proof(
    chain_arc: &Arc<Mutex<Blockchain>>,
    mempool_arc: &Arc<Mutex<Mempool>>,
    sk_opt: Option<&SigningKey>,
    external_proof_arc: &Arc<Mutex<Option<EnergyProof>>>,
) -> Block {
    let (chain_snapshot, selected_txs, external_proof_opt) = {
        let chain_guard = chain_arc.lock().unwrap();
        let mempool_guard = mempool_arc.lock().unwrap();
        let proof_guard = external_proof_arc.lock().unwrap();

        let max_txs = 100usize;
        let txs = mempool_guard.select_for_block(max_txs);

        let proof_clone = proof_guard.clone();

        (chain_guard.clone(), txs, proof_clone)
    };

    mine_block(&chain_snapshot, selected_txs, sk_opt, external_proof_opt)
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