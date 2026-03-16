# GVR v2 — гибридная криптовалюта (PoW + EnergyProof)

GVR — экспериментальная гибридная криптовалюта:

- **PoW + EnergyProof + AI‑оценка** (гибридная эмиссия по фазам);
- **аккаунтная модель** (балансы + nonce), удобная для смарт‑логики;
- **P2P‑сеть** в духе биткоина:
  - постоянные TCP‑соединения,
  - hello с подписью p2p‑ключом,
  - chainwork и авто‑сложность,
  - инвентарь блоков и транзакций;
- **RPC на axum** и **CLI‑клиент** (`client`), похожий на `bitcoin-cli`.

Проект/каталог: `gvr_v2`  
Основной бинарь ноды: `gvr_hybrid_node`  
CLI‑клиент: `client`  

---

## 1. Архитектура (коротко)

Основные модули:

- `block.rs` — структура блока, хэш, genesis.
- `blockchain.rs` — хранение цепочки, chainwork, reorg, авто‑сложность, интеграция эмиссии и state.
- `state.rs` — аккаунтное состояние: балансы + nonce, применение транзакций.
- `transaction.rs` + `accounts.rs` — типы транзакций (в т.ч. `Signed`), подписи и проверка подписи.
- `mempool.rs` — mempool с проверкой подписи для `Transaction::Signed`.
- `mine.rs` — майнинг:
  - выбирает транзакции из mempool,
  - создаёт блок,
  - формирует `EnergyProof` и подписывает его AI‑ключом.
- `emission.rs` + `energy.rs` + `constants.rs` — гибридная эмиссия:
  - Phase1/Phase2/Phase3;
  - награда зависит от kWh и ai_score в Phase2/3.
- `p2p.rs` — P2P:
  - Hello с подписью p2p‑ключом,
  - sync по локаторам (`GetBlocksFromLocators`),
  - `InvTx`/`InvBlock`/`MempoolInv`,
  - banlist и backoff.
- `rpc.rs` — HTTP‑RPC на базе axum:
  - `/status`, `/tx`, `/peers`, `/balance`, `/nonce`, `/sync`, `/ban`, `/unban`.
- `src/bin/client.rs` — CLI‑клиент (`client`) для RPC.

---

## 2. Сборка

Требуется установленный Rust (через `rustup`).

В PowerShell (Windows):

```powershell
cd C:\Users\Пользователь\gvr_v2
cargo build