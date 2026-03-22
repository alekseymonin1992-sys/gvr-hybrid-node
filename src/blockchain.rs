use crate::block::Block;
use crate::constants::{
    DIFFICULTY_ADJUST_INTERVAL, MAX_DIFFICULTY, MIN_DIFFICULTY, TARGET_BLOCK_TIME_SEC,
};
use crate::emission::{calculate_reward, EmissionConfig};
use crate::state::State;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Состояние производителя энергии
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ProducerState {
    pub last_seq: u64,
    pub last_ts: u128,
}

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

impl Blockchain {
    /// Создаём новую цепь с генезис‑блоком.
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

        // Если у генезиса есть reward — начислим его coinbase'у и учтём в total_supply
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

    /// Хеш текущего tip'а.
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

        // Уже видели этот блок — считаем, что ок.
        if self.blocks_by_hash.contains_key(&block.hash) {
            return true;
        }

        // Анти‑replay по EnergyProof (sequence/timestamp по producer_id)
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

        // Добавляем блок в "глобальный" граф
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

        // Если новая ветка тяжелее — перестраиваем основную цепь до этого блока.
        if my_work > current_tip_work {
            if let Err(e) = self.rebuild_main_chain_to(&block_hash) {
                println!("Reorg error: {}", e);
                return false;
            }
        }

        true
    }

    /// Пересобираем основную цепь до заданного tip.
    fn rebuild_main_chain_to(&mut self, new_tip_hash: &str) -> Result<(), String> {
        // Восстанавливаем путь от new_tip к genesis по previous_hash
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

        // Сбрасываем основной view
        self.chain.clear();
        self.total_supply = 0;
        self.producer_state.clear();

        // Сбрасываем state, сохранив текущий coinbase адрес.
        let coinbase_addr = self.state.coinbase.clone();
        self.state = State::new(coinbase_addr);

        // Проходим по всем блокам пути, восстанавливая state/total_supply/producer_state.
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

            // Применяем транзакции к state
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

            // Обновляем producer_state при наличии EnergyProof
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

    /// Авто‑регулировка сложности раз в DIFFICULTY_ADJUST_INTERVAL блоков.
    fn adjust_difficulty(&mut self) {
        // Нужна минимальная высота: genesis + интервал
        if self.chain.len() < (DIFFICULTY_ADJUST_INTERVAL as usize + 1) {
            return;
        }

        let height = self.chain.len() as u64;
        if height % DIFFICULTY_ADJUST_INTERVAL != 0 {
            // Меняем сложность только на кратких INTERVAL высотах
            return;
        }

        let last_index = self.chain.len() - 1;
        let first_index = last_index - DIFFICULTY_ADJUST_INTERVAL as usize;

        let first_block = &self.chain[first_index];
        let last_block = &self.chain[last_index];

        let actual_time_ms: u128 = last_block
            .timestamp
            .saturating_sub(first_block.timestamp);
        // привели к u128
        let target_time_ms: u128 =
            (TARGET_BLOCK_TIME_SEC as u128 * 1000u128) * (DIFFICULTY_ADJUST_INTERVAL as u128);

        if actual_time_ms == 0 {
            return;
        }

        if actual_time_ms < target_time_ms {
            // Блоки идут быстрее таргета — повышаем сложность
            if self.difficulty < MAX_DIFFICULTY {
                self.difficulty += 1;
                println!(
                    "Difficulty adjust: ↑ to {} (actual {} ms, target {} ms over {} blocks)",
                    self.difficulty, actual_time_ms, target_time_ms, DIFFICULTY_ADJUST_INTERVAL
                );
            }
        } else if actual_time_ms > target_time_ms {
            // Блоки идут медленнее таргета — понижаем сложность
            if self.difficulty > MIN_DIFFICULTY {
                self.difficulty -= 1;
                println!(
                    "Difficulty adjust: ↓ to {} (actual {} ms, target {} ms over {} blocks)",
                    self.difficulty, actual_time_ms, target_time_ms, DIFFICULTY_ADJUST_INTERVAL
                );
            }
        }
    }

    /// Сохранение состояния блокчейна в файл (JSON).
    pub fn save_state(&self, path: &Path) -> anyhow::Result<()> {
        let tmp_path = path.with_extension("tmp");
        let data = serde_json::to_vec_pretty(self)?;
        fs::write(&tmp_path, &data)?;
        fs::rename(&tmp_path, path)?;
        Ok(())
    }

    /// Загрузка состояния блокчейна из файла.
    pub fn load_state(path: &Path) -> anyhow::Result<Blockchain> {
        let data = fs::read(path)?;
        let mut bc: Blockchain = serde_json::from_slice(&data)?;

        // Полная реконструкция индексных структур из chain.
        let mut blocks_by_hash = HashMap::new();
        let mut children: HashMap<String, Vec<String>> = HashMap::new();
        let mut chainwork_by_hash = HashMap::new();

        if bc.chain.is_empty() {
            anyhow::bail!("loaded chain is empty");
        }

        // Обрабатываем генезис
        let genesis = bc.chain[0].clone();
        let genesis_hash = genesis.hash.clone();
        let work0 = block_work(&genesis);
        blocks_by_hash.insert(genesis_hash.clone(), genesis);
        chainwork_by_hash.insert(genesis_hash.clone(), work0);

        // Остальные блоки по основной цепи
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

/// Работа блока (chainwork) по его сложности.
fn block_work(b: &Block) -> u128 {
    if b.difficulty >= 63 {
        u128::MAX
    } else {
        1u128 << b.difficulty
    }
}