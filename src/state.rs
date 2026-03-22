use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::transaction::Transaction;
use crate::constants::MIN_SIGNED_FEE;

/// Адрес по умолчанию для dev-режима (может быть переопределён через CLI).
pub const DEV_COINBASE_ADDR: &str = "alice";

/// Простое аккаунтное состояние: адрес -> баланс, адрес -> nonce.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct State {
    pub balances: HashMap<String, u64>,
    /// Nonce (sequence) для каждого адреса, для SignedTransfer.
    pub nonces: HashMap<String, u64>,
    /// Адрес, на который зачисляется награда за блок (coinbase).
    pub coinbase: String,
}

impl State {
    pub fn new(coinbase: String) -> Self {
        State {
            balances: HashMap::new(),
            nonces: HashMap::new(),
            coinbase,
        }
    }

    /// Получить баланс адреса.
    pub fn balance_of(&self, addr: &str) -> u64 {
        *self.balances.get(addr).unwrap_or(&0u64)
    }

    /// Текущий ожидаемый nonce для адреса (по умолчанию 0).
    pub fn nonce_of(&self, addr: &str) -> u64 {
        *self.nonces.get(addr).unwrap_or(&0u64)
    }

    /// Зачислить сумму на адрес (safe-saturating).
    pub fn credit(&mut self, addr: &str, amount: u64) {
        let entry = self.balances.entry(addr.to_string()).or_insert(0);
        *entry = entry.saturating_add(amount);
    }

    /// Списать сумму с адреса. Возвращает Err, если не хватает средств.
    pub fn debit(&mut self, addr: &str, amount: u64) -> Result<(), String> {
        let entry = self.balances.entry(addr.to_string()).or_insert(0);
        if *entry < amount {
            return Err(format!(
                "insufficient funds: addr={} have={} need={}",
                addr, *entry, amount
            ));
        }
        *entry -= amount;
        Ok(())
    }

    /// Установить следующий nonce для адреса.
    fn bump_nonce(&mut self, addr: &str) {
        let entry = self.nonces.entry(addr.to_string()).or_insert(0);
        *entry = entry.saturating_add(1);
    }

    /// Применить одну транзакцию к state.
    pub fn apply_tx(&mut self, tx: &Transaction) -> Result<(), String> {
        match tx {
            Transaction::Transfer {
                sender,
                receiver,
                amount,
            } => {
                if *amount == 0 {
                    return Err("zero-amount transfer".to_string());
                }
                self.debit(sender, *amount)?;
                self.credit(receiver, *amount);
                Ok(())
            }
            Transaction::RotateAIKey { .. } => {
                // экономику RotateAIKey пока не трогаем
                Ok(())
            }
            Transaction::Signed(st) => {
                if st.amount == 0 {
                    return Err("zero-amount signed transfer".to_string());
                }

                // Минимальная комиссия на уровне протокола
                if st.fee < MIN_SIGNED_FEE {
                    return Err(format!(
                        "fee below minimum: got={} min={}",
                        st.fee, MIN_SIGNED_FEE
                    ));
                }

                // проверяем nonce: ожидаемое значение должно совпасть
                let expected_nonce = self.nonce_of(&st.from);
                if st.nonce != expected_nonce {
                    return Err(format!(
                        "invalid nonce for {}: got={} expected={}",
                        st.from, st.nonce, expected_nonce
                    ));
                }
                // списываем amount + fee с отправителя
                let total_debit = st.amount.saturating_add(st.fee);
                self.debit(&st.from, total_debit)?;
                // зачисляем amount получателю
                self.credit(&st.to, st.amount);
                // комиссия уходит на coinbase-адрес
                if st.fee > 0 {
                    let coinbase = self.coinbase.clone();
                    self.credit(&coinbase, st.fee);
                }
                // увеличиваем nonce
                self.bump_nonce(&st.from);
                Ok(())
            }
        }
    }

    /// Применить набор транзакций к state в порядке.
    /// Если какая-то невалидна, всё откатываем (используем временную копию).
    pub fn apply_txs_atomic(&self, txs: &[Transaction]) -> Result<State, String> {
        let mut next = self.clone();
        for tx in txs {
            next.apply_tx(tx)?;
        }
        Ok(next)
    }
}