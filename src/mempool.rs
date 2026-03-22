use std::collections::{HashMap, HashSet};

use crate::hashing::hash;
use crate::transaction::Transaction;

#[derive(Debug, Default)]
pub struct Mempool {
    /// tx_hash (hex) -> Transaction
    pub txs: HashMap<String, Transaction>,
    /// Порядок добавления
    pub order: Vec<String>,
}

impl Mempool {
    pub fn new() -> Self {
        Mempool {
            txs: HashMap::new(),
            order: Vec::new(),
        }
    }

    /// Добавить транзакцию в mempool. Возвращает её хеш (hex).
    /// Signed-транзакции проверяем по подписи и минимальной комиссии,
    /// остальные — принимаем как есть.
    pub fn add_tx(&mut self, tx: Transaction) -> String {
        match &tx {
            Transaction::Signed(st) => {
                // 1) проверяем подпись
                match st.verify() {
                    Ok(true) => {}
                    Ok(false) => {
                        println!("Mempool: reject Signed tx — signature invalid");
                        return String::new();
                    }
                    Err(e) => {
                        println!("Mempool: reject Signed tx — verify error: {}", e);
                        return String::new();
                    }
                }

                // 2) проверяем минимальную комиссию
                if st.fee < crate::constants::MIN_SIGNED_FEE {
                    println!(
                        "Mempool: reject Signed tx — fee {} below MIN_SIGNED_FEE={}",
                        st.fee,
                        crate::constants::MIN_SIGNED_FEE
                    );
                    return String::new();
                }
            }
            _ => {
                // Transfer / RotateAIKey — пока без подписи и без fee
            }
        }

        let h = tx_hash_hex(&tx);
        if !self.txs.contains_key(&h) {
            self.txs.insert(h.clone(), tx);
            self.order.push(h.clone());
        }
        h
    }

    /// Удалить транзакции, которые уже попали в блок.
    pub fn remove_included(&mut self, included: &[Transaction]) {
        let mut to_remove: HashSet<String> = HashSet::new();
        for tx in included {
            let h = tx_hash_hex(tx);
            to_remove.insert(h);
        }

        self.order.retain(|h| !to_remove.contains(h));
        for h in &to_remove {
            self.txs.remove(h);
        }
    }

    /// Выбрать до max_count транзакций для включения в новый блок.
    /// Приоритет: сначала Signed с большим fee, затем остальные в порядке добавления.
    pub fn select_for_block(&self, max_count: usize) -> Vec<Transaction> {
        let mut signed: Vec<(u64, String)> = Vec::new();
        let mut other: Vec<String> = Vec::new();

        for h in &self.order {
            if let Some(tx) = self.txs.get(h) {
                match tx {
                    Transaction::Signed(st) => {
                        signed.push((st.fee, h.clone()));
                    }
                    _ => {
                        other.push(h.clone());
                    }
                }
            }
        }

        // сортируем Signed по fee (по убыванию)
        signed.sort_by(|a, b| b.0.cmp(&a.0));

        let mut out = Vec::new();

        for (_fee, h) in signed.into_iter() {
            if out.len() >= max_count {
                break;
            }
            if let Some(tx) = self.txs.get(&h) {
                out.push(tx.clone());
            }
        }

        if out.len() < max_count {
            for h in other {
                if out.len() >= max_count {
                    break;
                }
                if let Some(tx) = self.txs.get(&h) {
                    out.push(tx.clone());
                }
            }
        }

        out
    }
}

/// Хеш транзакции (SHA256 от JSON-сериализации).
pub fn tx_hash_hex(tx: &Transaction) -> String {
    let data = serde_json::to_vec(tx).unwrap_or_default();
    let h = hash(&data);
    hex::encode(h)
}