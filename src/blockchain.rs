use crate::block::Block;
use crate::constants::{
    DIFFICULTY_ADJUST_INTERVAL, MAX_DIFFICULTY, MIN_DIFFICULTY, TARGET_BLOCK_TIME_SEC,
};
use crate::emission::{calculate_reward, EmissionConfig};
use crate::state::{State, DEV_COINBASE_ADDR};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Blockchain {
    /// Линейное представление основной цепи (от genesis до tip)
    pub chain: Vec<Block>,

    /// Текущая целевая сложность для новых блоков
    pub difficulty: u32,

    /// Общий выпуск монет по основной цепи
    pub total_supply: u64,

    /// Активный AI-публичный ключ для проверки EnergyProof
    pub active_ai_pubkey: Option<Vec<u8>>,

    /// Состояние производителей энергии по основной цепи
    pub producer_state: HashMap<String, ProducerState>,

    /// Все известные блоки по их хешу
    pub blocks_by_hash: HashMap<String, Block>,

    /// Дерево: для каждого хеша родителя список детей
    pub children: HashMap<String, Vec<String>>,

    /// Накопленная сложность (chainwork) по хешу блока
    pub chainwork_by_hash: HashMap<String, u128>,

    /// Хеш головы основной цепи
    pub tip_hash: String,

    /// Аккаунтное состояние по основной цепи
    pub state: State,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ProducerState {
    pub last_seq: u64,
    pub last_ts: u128,
}

impl Blockchain {
    pub fn new_with_genesis(ai_pubkey: Option<Vec<u8>>) -> Self {
        let mut blocks_by_hash = HashMap::new();
        let mut chainwork_by_hash = HashMap::new();
        let children = HashMap::new();

        let genesis = Block::genesis();
        let genesis_hash = genesis.hash.clone();

        let work = block_work(&genesis);
        chainwork_by_hash.insert(genesis_hash.clone(), work);
        blocks_by_hash.insert(genesis_hash.clone(), genesis.clone());

        // В dev-режиме coinbase адрес = "alice"
        let state = State::new(DEV_COINBASE_ADDR.to_string());

        Blockchain {
            chain: vec![genesis],
            difficulty: crate::constants::INITIAL_DIFFICULTY,
            total_supply: 0,
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

    /// Добавление блока с fork-choice по chainwork.
    /// Проверка state выполняется при reorg и при восстановлении цепи.
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

        if self.blocks_by_hash.contains_key(&block.hash) {
            return true;
        }

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
            }
        }

        let last_seen_ts_opt: Option<u128> = block
            .energy_proof
            .as_ref()
            .and_then(|ep| self.producer_state.get(&ep.producer_id))
            .map(|ps| ps.last_ts);

        let cfg = EmissionConfig::default();

        let reward_and_ts = calculate_reward(
            self.total_supply,
            block.energy_proof.as_ref(),
            self.active_ai_pubkey.as_deref(),
            last_seen_ts_opt,
            &cfg,
        );

        let (reward, _last_ts_opt) = match reward_and_ts {
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

        let current_tip_work = *self
            .chainwork_by_hash
            .get(&self.tip_hash)
            .unwrap_or(&0u128);

        if my_work > current_tip_work {
            // Новый лучший tip по chainwork — пересобираем основную цепь
            if let Err(e) = self.rebuild_main_chain_to(&block_hash) {
                println!("Reorg error: {}", e);
                return false;
            }
        }

        true
    }

    /// Пересобираем основную цепь (chain, total_supply, producer_state, difficulty, state) до заданного tip.
    fn rebuild_main_chain_to(&mut self, new_tip_hash: &str) -> Result<(), String> {
        let mut path: Vec<String> = Vec::new();
        let mut cur = new_tip_hash.to_string();

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

        self.chain.clear();
        self.total_supply = 0;
        self.producer_state.clear();

        // сбрасываем state от genesis: новый State с тем же coinbase
        let coinbase_addr = self.state.coinbase.clone();
        self.state = State::new(coinbase_addr);

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

            // применяем награду и транзакции к state
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

            self.chain.push(b);
        }

        self.tip_hash = new_tip_hash.to_string();
        self.adjust_difficulty();

        let new_height = self.chain.len();

        // Логируем только настоящие реорги (смена ветки),
        // а не обычное продолжение цепи на один блок.
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
        let len = self.chain.len() as u64;
        if len <= 1 {
            return;
        }
        if len % DIFFICULTY_ADJUST_INTERVAL != 0 {
            return;
        }

        let window_size = DIFFICULTY_ADJUST_INTERVAL as usize;
        if self.chain.len() < window_size + 1 {
            return;
        }

        let last_idx = self.chain.len() - 1;
        let first_idx = last_idx - window_size;

        let first_block = &self.chain[first_idx];
        let last_block = &self.chain[last_idx];

        let time_span_ms = if last_block.timestamp > first_block.timestamp {
            last_block.timestamp - first_block.timestamp
        } else {
            1
        };

        let target_block_ms = (TARGET_BLOCK_TIME_SEC as u128) * 1000;
        let avg_ms_per_block = time_span_ms / (window_size as u128);
        let ratio = avg_ms_per_block as f64 / target_block_ms as f64;

        let mut new_diff = self.difficulty;

        if ratio < 0.5 {
            if new_diff < MAX_DIFFICULTY.saturating_sub(1) {
                new_diff += 2;
            } else if new_diff < MAX_DIFFICULTY {
                new_diff += 1;
            }
        } else if ratio < 0.9 {
            if new_diff < MAX_DIFFICULTY {
                new_diff += 1;
            }
        } else if ratio > 2.0 {
            if new_diff > MIN_DIFFICULTY + 1 {
                new_diff -= 2;
            } else if new_diff > MIN_DIFFICULTY {
                new_diff -= 1;
            }
        } else if ratio > 1.1 {
            if new_diff > MIN_DIFFICULTY {
                new_diff -= 1;
            }
        }

        if new_diff != self.difficulty {
            println!(
                "Difficulty adjusted: {} -> {} (avg_ms_per_block={} target={} ratio={:.3})",
                self.difficulty, new_diff, avg_ms_per_block, target_block_ms, ratio
            );
            self.difficulty = new_diff;
        }
    }

    pub fn save_state(&self, path: &Path) -> Result<(), String> {
        let tmp = format!("{}.tmp", path.to_string_lossy());
        let serialized = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        fs::write(&tmp, &serialized).map_err(|e| e.to_string())?;
        fs::rename(&tmp, path).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn load_state(path: &Path) -> Result<Self, String> {
        let data = fs::read_to_string(path).map_err(|e| e.to_string())?;
        let mut bc: Blockchain = serde_json::from_str(&data).map_err(|e| e.to_string())?;

        if bc.chain.is_empty() {
            return Err("loaded blockchain has empty chain".to_string());
        }

        let tip = bc.chain.last().unwrap();
        bc.tip_hash = tip.hash.clone();

        if bc.blocks_by_hash.is_empty() {
            bc.blocks_by_hash = HashMap::new();
            bc.chainwork_by_hash = HashMap::new();
            bc.children = HashMap::new();

            for b in &bc.chain {
                let h = b.hash.clone();
                let work = block_work(b);
                bc.blocks_by_hash.insert(h.clone(), b.clone());
                let parent = b.previous_hash.clone();
                bc.children.entry(parent).or_insert_with(Vec::new).push(h.clone());

                let parent_work = bc
                    .chainwork_by_hash
                    .get(&b.previous_hash)
                    .copied()
                    .unwrap_or(0);
                bc.chainwork_by_hash
                    .insert(h.clone(), parent_work.saturating_add(work));
            }
        }

        // Если state пустой (старый снапшот), инициализируем его по текущей цепи
        if bc.state.balances.is_empty() {
            let coinbase = DEV_COINBASE_ADDR.to_string();
            bc.state = State::new(coinbase);
            let mut total_supply = 0u64;
            let coinbase_addr = bc.state.coinbase.clone();
            for b in &bc.chain {
                let cfg = EmissionConfig::default();
                let reward_and_ts = calculate_reward(
                    total_supply,
                    b.energy_proof.as_ref(),
                    bc.active_ai_pubkey.as_deref(),
                    None,
                    &cfg,
                );
                if let Ok((reward, _)) = reward_and_ts {
                    total_supply = total_supply.saturating_add(reward);
                    if reward > 0 {
                        bc.state.credit(&coinbase_addr, reward);
                    }
                    let _ = bc.state.apply_txs_atomic(&b.transactions);
                }
            }
            bc.total_supply = total_supply;
        }

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