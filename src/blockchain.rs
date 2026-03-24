use crate::block::Block;
use crate::constants::{
    DIFFICULTY_ADJUST_INTERVAL, MAX_DIFFICULTY, MIN_DIFFICULTY, TARGET_BLOCK_TIME_SEC,
    MAX_REORG_DEPTH, MIN_PROOF_INTERVAL_MS,
};
use crate::emission::{calculate_reward, EmissionConfig};
use crate::state::State;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

use k256::ecdsa::{VerifyingKey, Signature, signature::Verifier};
use sha2::{Sha256, Digest};

/// Состояние производителя энергии
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ProducerState {
    pub last_seq: u64,
    pub last_ts: u128,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Blockchain {
    pub chain: Vec<Block>,
    pub difficulty: u32,
    pub total_supply: u64,
    pub active_ai_pubkey: Option<Vec<u8>>,
    pub producer_state: HashMap<String, ProducerState>,
    pub blocks_by_hash: HashMap<String, Block>,
    pub children: HashMap<String, Vec<String>>,
    pub chainwork_by_hash: HashMap<String, u128>,
    pub tip_hash: String,
    pub state: State,
}

impl Blockchain {
    pub fn new_with_genesis(ai_pubkey: Option<Vec<u8>>, coinbase_addr: String) -> Self {
        let mut blocks_by_hash = HashMap::new();
        let mut chainwork_by_hash = HashMap::new();
        let children = HashMap::new();

        let genesis = Block::genesis();
        let genesis_hash = genesis.hash.clone();

        let work = block_work(&genesis);
        chainwork_by_hash.insert(genesis_hash.clone(), work);
        blocks_by_hash.insert(genesis_hash.clone(), genesis.clone());

        let mut state = State::new(coinbase_addr.clone());

        let mut total_supply = 0u64;
        if genesis.reward > 0 {
            state.credit(&coinbase_addr, genesis.reward);
            total_supply = genesis.reward;
        }

        Blockchain {
            chain: vec![genesis],
            difficulty: crate::constants::INITIAL_DIFFICULTY,
            total_supply,
            active_ai_pubkey: ai_pubkey,
            producer_state: HashMap::new(),
            blocks_by_hash,
            children,
            chainwork_by_hash,
            tip_hash: genesis_hash,
            state,
        }
    }

    pub fn last_hash(&self) -> String {
        self.tip_hash.clone()
    }

    /// Добавление нового блока.
    ///
    /// ВАЖНО:
    /// - Всегда сначала валидируем блок и вставляем его в индексы (blocks_by_hash, chainwork).
    /// - Затем, если новая ветка даёт больше chainwork:
    ///   * либо делаем reorg через `rebuild_main_chain_to`;
    ///   * либо, если это простое продолжение tip’а, детерминированно применяем
    ///     reward + tx к состоянию (state/total_supply/producer_state/chain).
    pub fn add_block(&mut self, mut block: Block) -> bool {
        let parent_hash = block.previous_hash.clone();
        if !self.blocks_by_hash.contains_key(&parent_hash) {
            println!(
                "Reject block: unknown parent {} for block idx={} hash={}",
                parent_hash, block.index, block.hash
            );
            return false;
        }

        let calc_hash = block.calculate_hash();
        if block.hash != calc_hash {
            println!(
                "Reject block: hash mismatch (provided {} vs calculated {})",
                block.hash, calc_hash
            );
            return false;
        }

        if !block
            .hash
            .starts_with(&"0".repeat(block.difficulty as usize))
        {
            println!(
                "Reject block: insufficient difficulty in hash {}",
                block.hash
            );
            return false;
        }

        // Если уже видели этот блок — считаем его принятым (idempotent).
        if self.blocks_by_hash.contains_key(&block.hash) {
            return true;
        }

        // Анти‑replay и анти‑спам по EnergyProof
        if let Some(ep) = &block.energy_proof {
            if let Some(ps) = self.producer_state.get(&ep.producer_id) {
                if ep.timestamp <= ps.last_ts {
                    println!(
                        "Reject block: replay detected (timestamp <= last_seen) for producer {}",
                        ep.producer_id
                    );
                    return false;
                }
                if ep.sequence <= ps.last_seq {
                    println!(
                        "Reject block: replay detected (sequence <= last_seq) for producer {}",
                        ep.producer_id
                    );
                    return false;
                }
                let dt = ep.timestamp.saturating_sub(ps.last_ts);
                if dt < MIN_PROOF_INTERVAL_MS {
                    println!(
                        "Reject block: producer {} submits proofs too frequently (dt={} ms < MIN_PROOF_INTERVAL_MS={})",
                        ep.producer_id, dt, MIN_PROOF_INTERVAL_MS
                    );
                    return false;
                }
            }
        }

        // Исторический last_seen_ts для данного producer (если он уже был в main‑chain)
        let last_seen_ts_opt: Option<u128> = block
            .energy_proof
            .as_ref()
            .and_then(|ep| self.producer_state.get(&ep.producer_id))
            .map(|ps| ps.last_ts);

        // Расчёт награды по текущему total_supply и энергопруфу
        let cfg = EmissionConfig::default();

        let reward_and_ts = calculate_reward(
            self.total_supply,
            block.energy_proof.as_ref(),
            self.active_ai_pubkey.as_deref(),
            last_seen_ts_opt,
            &cfg,
        );

        let (reward, last_ts_from_calc_opt) = match reward_and_ts {
            Ok((r, ts_opt)) => (r, ts_opt),
            Err(err_msg) => {
                println!("Reject block: emission calc error: {}", err_msg);
                return false;
            }
        };

        block.reward = reward;

        let phase = crate::emission::current_phase(self.total_supply);
        println!(
            "Emission: phase={:?} total_supply_before={} new_reward={}",
            phase, self.total_supply, reward
        );

        let block_hash = block.hash.clone();

        // Вставляем блок в индексы (он может стать частью альтернативной ветки)
        self.blocks_by_hash.insert(block_hash.clone(), block.clone());
        self.children
            .entry(parent_hash.clone())
            .or_insert_with(Vec::new)
            .push(block_hash.clone());

        let parent_work = *self
            .chainwork_by_hash
            .get(&parent_hash)
            .unwrap_or(&0u128);
        let my_work = parent_work.saturating_add(block_work(&block));
        self.chainwork_by_hash.insert(block_hash.clone(), my_work);

        // Сравниваем с текущим tip’ом по chainwork
        let current_tip_work = *self
            .chainwork_by_hash
            .get(&self.tip_hash)
            .unwrap_or(&0u128);

        let old_tip_index = self.chain.last().map(|b| b.index).unwrap_or(0);
        let new_block_index = block.index;

        if my_work > current_tip_work {
            // Возможная смена лучшей ветки
            if old_tip_index > new_block_index
                && old_tip_index.saturating_sub(new_block_index) > MAX_REORG_DEPTH
            {
                println!(
                    "Reject block: reorg depth too large (old_tip_index={} new_block_index={} max={})",
                    old_tip_index, new_block_index, MAX_REORG_DEPTH
                );
                return false;
            }

            // Если это НЕ прямое продолжение текущего tip’а — делаем полноценный reorg
            if parent_hash != self.tip_hash {
                if let Err(e) = self.rebuild_main_chain_to(&block_hash) {
                    println!("Reorg error: {}", e);
                    return false;
                }
            } else {
                // Простое расширение текущей main‑chain:
                //
                // 1) Увеличиваем total_supply на reward
                // 2) Начисляем reward на coinbase
                // 3) Применяем txs к state (атомарно)
                // 4) Обновляем producer_state по EnergyProof
                // 5) Применяем RotateAIKey‑txs
                // 6) Добавляем блок в chain
                // 7) Обновляем tip_hash и, при необходимости, difficulty

                if reward > 0 {
                    self.total_supply = self.total_supply.saturating_add(reward);
                    let coinbase = self.state.coinbase.clone();
                    self.state.credit(&coinbase, reward);
                }

                // Применяем транзакции к состоянию
                match self.state.apply_txs_atomic(&block.transactions) {
                    Ok(next_state) => {
                        self.state = next_state;
                    }
                    Err(e) => {
                        println!(
                            "Reject block: state transition error for block idx={} hash={}: {}",
                            block.index, block.hash, e
                        );
                        // Откатывать индексы blocks_by_hash/children/chainwork не пробуем — в худшем случае этот
                        // блок останется в альтернативной ветке, но не в main‑chain (chain).
                        return false;
                    }
                }

                // Обновление producer_state (если calculate_reward вернул timestamp)
                if let (Some(ts), Some(ep)) = (last_ts_from_calc_opt, &block.energy_proof) {
                    self.producer_state.insert(
                        ep.producer_id.clone(),
                        ProducerState {
                            last_seq: ep.sequence,
                            last_ts: ts,
                        },
                    );
                }

                // Применяем RotateAIKey‑txs
                if let Err(e) = self.apply_rotate_ai_key_txs(&block) {
                    println!("RotateAIKey error in block idx={}: {}", block.index, e);
                    // Ошибка ротации не должна ломать весь блок, но логируем.
                }

                // Наконец, блок становится частью main‑chain
                self.chain.push(block.clone());
                self.tip_hash = block_hash.clone();
                self.adjust_difficulty();
            }
        }

        if let Some(ep) = &block.energy_proof {
            println!(
                "Block idx={} hash={} EnergyProof: producer_id={} seq={} kwh={} ai_score={} ts={}",
                block.index,
                block.hash,
                ep.producer_id,
                ep.sequence,
                ep.kwh,
                ep.ai_score,
                ep.timestamp,
            );
        } else {
            println!(
                "Block idx={} hash={} has no EnergyProof",
                block.index,
                block.hash
            );
        }

        true
    }

    fn apply_rotate_ai_key_txs(&mut self, block: &Block) -> Result<(), String> {
        use crate::transaction::Transaction;

        for tx in &block.transactions {
            if let Transaction::RotateAIKey {
                new_ai_pubkey_sec1,
                proposer,
                signature,
            } = tx
            {
                let current_ai_pub = match &self.active_ai_pubkey {
                    Some(p) => p.clone(),
                    None => {
                        return Err("no active_ai_pubkey set, cannot rotate".into());
                    }
                };

                let _new_vk = VerifyingKey::from_sec1_bytes(new_ai_pubkey_sec1)
                    .map_err(|e| format!("invalid new_ai_pubkey_sec1: {}", e))?;

                let mut hasher = Sha256::new();
                hasher.update(proposer.as_bytes());
                hasher.update(&[0u8]);
                hasher.update(new_ai_pubkey_sec1);
                let msg_hash = hasher.finalize();

                let sig = Signature::from_der(signature)
                    .map_err(|_| "invalid RotateAIKey signature DER".to_string())?;

                let vk = VerifyingKey::from_sec1_bytes(&current_ai_pub)
                    .map_err(|e| format!("invalid current active_ai_pubkey: {}", e))?;

                vk.verify(msg_hash.as_slice(), &sig)
                    .map_err(|_| "RotateAIKey signature verify failed".to_string())?;

                self.active_ai_pubkey = Some(new_ai_pubkey_sec1.clone());
                println!(
                    "RotateAIKey applied in block idx={} proposer={} (active_ai_pubkey updated)",
                    block.index, proposer
                );
            }
        }

        Ok(())
    }

    fn rebuild_main_chain_to(&mut self, new_tip_hash: &str) -> Result<(), String> {
        let mut path: Vec<String> = Vec::new();
        let mut cur = new_tip_hash.to_string();

        // Строим путь от нового tip’а до генезиса
        loop {
            path.push(cur.clone());
            let b = self
                .blocks_by_hash
                .get(&cur)
                .ok_or_else(|| format!("missing block {} in blocks_by_hash", cur))?;

            if b.index == 0 {
                break;
            }
            cur = b.previous_hash.clone();
        }

        path.reverse();

        let old_tip = self.tip_hash.clone();
        let old_height = self.chain.len();

        // Очищаем main‑chain и состояние
        self.chain.clear();
        self.total_supply = 0;
        self.producer_state.clear();

        let coinbase_addr = self.state.coinbase.clone();
        self.state = State::new(coinbase_addr);

        // Пересобираем main‑chain по пути
        for h in &path {
            let mut b = self
                .blocks_by_hash
                .get(h)
                .cloned()
                .ok_or_else(|| format!("missing block {} on rebuild", h))?;

            let last_seen_ts_opt: Option<u128> = b
                .energy_proof
                .as_ref()
                .and_then(|ep| self.producer_state.get(&ep.producer_id))
                .map(|ps| ps.last_ts);

            let cfg = EmissionConfig::default();

            let reward_and_ts = calculate_reward(
                self.total_supply,
                b.energy_proof.as_ref(),
                self.active_ai_pubkey.as_deref(),
                last_seen_ts_opt,
                &cfg,
            );

            let (reward, last_ts_opt) = match reward_and_ts {
                Ok((r, ts_opt)) => (r, ts_opt),
                Err(err_msg) => {
                    return Err(format!(
                        "emission calc error on rebuild for block {}: {}",
                        h, err_msg
                    ))
                }
            };

            b.reward = reward;
            self.total_supply = self.total_supply.saturating_add(reward);

            if reward > 0 {
                let coinbase = self.state.coinbase.clone();
                self.state.credit(&coinbase, reward);
            }

            match self.state.apply_txs_atomic(&b.transactions) {
                Ok(next_state) => {
                    self.state = next_state;
                }
                Err(e) => {
                    return Err(format!(
                        "state transition error on rebuild for block {}: {}",
                        h, e
                    ));
                }
            }

            if let Some(ts) = last_ts_opt {
                if let Some(ep) = &b.energy_proof {
                    self.producer_state.insert(
                        ep.producer_id.clone(),
                        ProducerState {
                            last_seq: ep.sequence,
                            last_ts: ts,
                        },
                    );
                }
            }

            if let Err(e) = self.apply_rotate_ai_key_txs(&b) {
                return Err(format!(
                    "RotateAIKey apply error on rebuild for block {}: {}",
                    h, e
                ));
            }

            self.chain.push(b);
        }

        self.tip_hash = new_tip_hash.to_string();
        self.adjust_difficulty();

        let new_height = self.chain.len();

        let is_simple_extension = {
            if let Some(new_tip_block) = self.blocks_by_hash.get(&self.tip_hash) {
                new_tip_block.previous_hash == old_tip
            } else {
                false
            }
        };

        if !is_simple_extension {
            println!(
                "Reorg: old_tip={} (height={}) -> new_tip={} (height={})",
                old_tip, old_height, self.tip_hash, new_height
            );
        }

        Ok(())
    }

    fn adjust_difficulty(&mut self) {
        if self.chain.len() < (DIFFICULTY_ADJUST_INTERVAL as usize + 1) {
            return;
        }

        let height = self.chain.len() as u64;
        if height % DIFFICULTY_ADJUST_INTERVAL != 0 {
            return;
        }

        let last_index = self.chain.len() - 1;
        let first_index = last_index - DIFFICULTY_ADJUST_INTERVAL as usize;

        let first_block = &self.chain[first_index];
        let last_block = &self.chain[last_index];

        let actual_time_ms: u128 = last_block
            .timestamp
            .saturating_sub(first_block.timestamp);
        let target_time_ms: u128 =
            (TARGET_BLOCK_TIME_SEC as u128 * 1000u128) * (DIFFICULTY_ADJUST_INTERVAL as u128);

        if actual_time_ms == 0 {
            return;
        }

        if actual_time_ms < target_time_ms {
            if self.difficulty < MAX_DIFFICULTY {
                self.difficulty += 1;
                println!(
                    "Difficulty adjust: ↑ to {} (actual {} ms, target {} ms over {} blocks)",
                    self.difficulty, actual_time_ms, target_time_ms, DIFFICULTY_ADJUST_INTERVAL
                );
            }
        } else if actual_time_ms > target_time_ms {
            if self.difficulty > MIN_DIFFICULTY {
                self.difficulty -= 1;
                println!(
                    "Difficulty adjust: ↓ to {} (actual {} ms, target {} ms over {} blocks)",
                    self.difficulty, actual_time_ms, target_time_ms, DIFFICULTY_ADJUST_INTERVAL
                );
            }
        }
    }

    pub fn save_state(&self, path: &Path) -> anyhow::Result<()> {
        let tmp_path = path.with_extension("tmp");
        let data = serde_json::to_vec_pretty(self)?;
        fs::write(&tmp_path, &data)?;
        fs::rename(&tmp_path, path)?;
        Ok(())
    }

    pub fn load_state(path: &Path) -> anyhow::Result<Blockchain> {
        let data = fs::read(path)?;
        let mut bc: Blockchain = serde_json::from_slice(&data)?;

        let mut blocks_by_hash = HashMap::new();
        let mut children: HashMap<String, Vec<String>> = HashMap::new();
        let mut chainwork_by_hash = HashMap::new();

        if bc.chain.is_empty() {
            anyhow::bail!("loaded chain is empty");
        }

        let genesis = bc.chain[0].clone();
        let genesis_hash = genesis.hash.clone();
        let work0 = block_work(&genesis);
        blocks_by_hash.insert(genesis_hash.clone(), genesis);
        chainwork_by_hash.insert(genesis_hash.clone(), work0);

        let mut prev_hash = genesis_hash.clone();
        for b in bc.chain.iter().skip(1).cloned() {
            let h = b.hash.clone();
            blocks_by_hash.insert(h.clone(), b.clone());
            children.entry(prev_hash.clone()).or_default().push(h.clone());

            let parent_work = *chainwork_by_hash.get(&prev_hash).unwrap_or(&0);
            let my_work = parent_work.saturating_add(block_work(&b));
            chainwork_by_hash.insert(h.clone(), my_work);

            prev_hash = h;
        }

        bc.blocks_by_hash = blocks_by_hash;
        bc.children = children;
        bc.chainwork_by_hash = chainwork_by_hash;
        bc.tip_hash = prev_hash;

        Ok(bc)
    }
}

fn block_work(b: &Block) -> u128 {
    if b.difficulty >= 63 {
        u128::MAX
    } else {
        1u128 << b.difficulty
    }
}