Этот документ описывает протокол блокчейна GVR Hybrid: модель эмиссии, консенсус, типы транзакций, формат блока, P2P‑уровень, RPC‑API и основные лимиты. Документ предназначен как официальная спецификация сети и монеты.

---

## 1. Обзор сети

GVR Hybrid — это гибридная сеть, которая сочетает:

- **Proof‑of‑Work (PoW)** — классический механизм с регулируемой сложностью;
- **EnergyProof** — криптографически подписанные доказательства выработки энергии, влияющие на эмиссию монеты;
- **Эмиссионные фазы** — три этапа, в которых вес PoW и энерго‑доказательств меняется.

Основные цели протокола:

1. Обеспечить надёжный, PoW‑поддержанный консенсус и безопасность.
2. Привязать большую часть эмиссии к реальной выработке энергии.
3. Ограничить максимальное предложение монеты фиксированным капом.
4. Поддерживать простую и прозрачную модель аккаунтов и транзакций.

---

## 2. Базовые параметры сети

Все численные параметры определены в модуле `constants.rs`. Ключевые:

### 2.1. Эмиссия и фазы

- **Максимальное предложение (MAX_SUPPLY):** `21_000_000` GVR.
- **Базовая награда в Phase1 (BASE_REWARD):** `50` GVR.
- **Малая PoW‑награда в Phase3 (PHASE3_POW_REWARD):** `1` GVR.

**Разбиение на фазы по общей эмиссии (`total_supply`):**

- **Phase1:** `0 .. PHASE1_SUPPLY_LIMIT`  
  `PHASE1_SUPPLY_LIMIT = 9_000_000`
- **Phase2:** `PHASE1_SUPPLY_LIMIT .. PHASE2_SUPPLY_LIMIT`  
  `PHASE2_SUPPLY_LIMIT = 15_000_000`
- **Phase3:** `PHASE2_SUPPLY_LIMIT .. MAX_SUPPLY`  
  `MAX_SUPPLY = 21_000_000`

Текущая фаза определяется функцией:

```rust
pub fn current_phase(total_supply: u64) -> EmissionPhase;
```

### 2.2. Proof‑of‑Work

- **Начальная сложность:** `INITIAL_DIFFICULTY = 3`.  
  Хэш блока должен начинаться с `difficulty` нулевых шестнадцатеричных символов.
- **Целевое время блока (TARGET_BLOCK_TIME_SEC):** `90` секунд.
- **Интервал пересчёта сложности (DIFFICULTY_ADJUST_INTERVAL):** `10` блоков.
- **Минимальная и максимальная сложность:**
  - `MIN_DIFFICULTY = 1`
  - `MAX_DIFFICULTY = 32`

Сложность пересчитывается только на высотах, кратных `DIFFICULTY_ADJUST_INTERVAL`, исходя из фактического времени генерации последних N блоков.

### 2.3. EnergyProof и энергетические параметры

- **Минимальный AI‑score (MIN_AI_SCORE):** `0.8`.
- **Базовое количество GVR за 1 kWh (BASE_GVR_PER_KWH):** `10.0`.
- **Энергетический коэффициент (ENERGY_FACTOR):** `0.01` (используется в Phase2).
- **Максимальный объём энергии в одном EnergyProof (MAX_KWH_PER_PROOF):** `10_000_000.0` kWh.
- **Допустимое расхождение по времени (ALLOWED_TIMESTAMP_SKEW_MS):**
  `5 * 60 * 1000` мс.
- **Минимальный интервал между EnergyProof одного производителя
  (MIN_PROOF_INTERVAL_MS):** `10 * 60 * 1000` мс.

### 2.4. Signed‑транзакции и комиссии

- **Минимальная комиссия за Signed‑транзакцию (MIN_SIGNED_FEE):** `1` GVR.

### 2.5. Fixed‑point масштаб

Внутренние расчёты по энергии и эмиссии используют fixed‑point:

- `SCALE = 100_000_000` (1 GVR = 1e8 ед.).
- Числа типа `*_FP` хранятся в этом масштабе.

---

## 3. Модель консенсуса

GVR Hybrid использует:

1. **Локальное PoW‑майнинг условие:**  
   Блок валиден, если его `hash` имеет достаточное количество начальных нулей
   в шестнадцатеричном представлении в соответствии с `difficulty`.

2. **Цепь с наибольшей “chainwork”:**  
   Для каждого блока определяется условная работа:

   ```rust
   fn block_work(b: &Block) -> u128 {
       if b.difficulty >= 63 {
           u128::MAX
       } else {
           1u128 << b.difficulty
       }
   }
   ```

   **Совокупная работа цепи** до блока с хэшем `H` — это сумма `block_work`
   всех блоков от генезис‑блока до `H`. Главной считается ветвь с максимальным chainwork.

3. **Ограничение глубины reorg:**  
   `MAX_REORG_DEPTH = 100`.  
   Если новая потенциальная лучшая ветка короче текущего tip’а более чем на 100 блоков,
   такая реорганизация отклоняется.

4. **Автоматическая настройка сложности:**  
   Функция `Blockchain::adjust_difficulty()` сравнивает фактическое время генерации последних N блоков
   с целевым временем `TARGET_BLOCK_TIME_SEC * DIFFICULTY_ADJUST_INTERVAL`:

   - если блоки находились существенно **быстрее** цели — сложность увеличивается на 1 (но не выше `MAX_DIFFICULTY`);
   - если **медленнее** — уменьшается на 1 (но не ниже `MIN_DIFFICULTY`).

5. **Перестроение main‑chain (rebuild_main_chain_to):**  
   При появлении блока на альтернативной ветке, который даёт большую chainwork,
   нода:

   - находит путь от генезис‑блока к новому tip’у,
   - детерминированно пересчитывает:
     - эмиссию и `total_supply`,
     - account‑state (`State`),
     - `producer_state` по EnergyProof,
     - `active_ai_pubkey` (применяя `RotateAIKey`‑транзакции),
   - пересобирает массив `chain` в соответствии с новой основной ветвью.

---

## 4. Структура блока

Структура блока (`block.rs`):

```rust
pub struct Block {
    pub index: u64,
    pub previous_hash: String,
    pub timestamp: u128,
    pub transactions: Vec<Transaction>,
    pub nonce: u64,
    pub difficulty: u32,

    pub energy_proof: Option<EnergyProof>,
    pub reward: u64,

    pub hash: String,
}
```

### 4.1. Поле `hash` и PoW‑условие

`Block::calculate_hash()` вычисляет SHA256 от конкатенации:

1. `index` (как строка),
2. `previous_hash`,
3. `timestamp` (как строка),
4. сериализованные поля транзакций (по типу, без JSON),
5. `nonce` (как строка),
6. `difficulty` (как строка),
7. либо `energy_proof.canonical_bytes()`, либо строка `"no_proof"`.

`Block::mine()` увеличивает `nonce` до тех пор, пока:

```text
hash.starts_with("0".repeat(difficulty as usize))
```

Валидность блока проверяется в `Blockchain::add_block()`:

- хэш блока должен совпадать с пересчитанным `calculate_hash()`;
- PoW‑условие по `difficulty` должно выполняться;
- предыдущий хэш (`previous_hash`) должен ссылаться на известный блок.

### 4.2. Блок‑генезис

`Block::genesis()` создаёт предопределённый блок:

- `index = 0`,
- `previous_hash = "0"`,
- фиксированный `timestamp`,
- пустой список транзакций,
- `difficulty = 1`,
- `energy_proof = None`,
- `reward = 0`,
- `hash` вычисляется без майнинга (единственный “особый” блок).

---

## 5. Эмиссия и расчёт награды

Основная функция эмиссии:  

```rust
pub fn calculate_reward(
    total_supply: u64,
    energy_proof: Option<&EnergyProof>,
    ai_pubkey_opt: Option<&[u8]>,
    last_seen_timestamp_opt: Option<u128>,
    cfg: &EmissionConfig,
) -> Result<(u64, Option<u128>), String>
```

Возвращает:

- `reward`: награда за блок (в GVR),
- `Option<u128>` — timestamp, который следует сохранить как последнюю метку времени для данного производителя.

### 5.1. Ограничение MAX_SUPPLY

Во всех фазах перед начислением награды проверяется:

- если `total_supply >= max_supply`, награда = `0`;
- если `total_supply + reward > max_supply`, награда урезается до `max_supply - total_supply`.

Суммарная эмиссия строго не превышает `MAX_SUPPLY`.

### 5.2. Phase1: классический PoW

Условия:

- `total_supply < PHASE1_SUPPLY_LIMIT`.

Награда:

- фиксированная `BASE_REWARD` (пока не достигнут `MAX_SUPPLY`).

EnergyProof в Phase1 не используется, поле `energy_proof` может быть пустым или заполненным — на награду это не влияет.

### 5.3. Phase2: гибрид PoW + EnergyProof

Условия:

- `PHASE1_SUPPLY_LIMIT <= total_supply < PHASE2_SUPPLY_LIMIT`.

Награда состоит из:

1. **PoW‑часть (pow_reward):**

   - `pow_reward = BASE_REWARD / 2` (при `BASE_REWARD = 50` это `25 GVR`);
   - даже при отсутствии EnergyProof блок получает `pow_reward`.

2. **Энергетический бонус (energy_bonus_u):**  
   Присутствует только при наличии валидного `EnergyProof`.

Требования к EnergyProof:

- `EnergyProof::validate_fields(min_ai_score)`:
  - `kwh > 0 && kwh <= MAX_KWH_PER_PROOF`;
  - `ai_score >= MIN_AI_SCORE`;
  - timestamp не слишком далеко в будущем.
- `proof.timestamp` не должен быть больше `now_ms + allowed_skew_ms`.
- Должен быть задан `ai_pubkey_opt` (активный публичный AI‑ключ).
- `verify_signature(ai_pubkey)` должен вернуть `true`.

Бонус вычисляется в fixed‑point:

- `kwh_fp = kwh * SCALE`,
- `score_fp = ai_score * SCALE`,
- `term ≈ kwh * score` в FP: `term = (kwh_fp * score_fp) / SCALE`,
- `energy_factor_fp = (cfg.energy_factor * SCALE as f64) as u128`,
- `bonus_fp = term * energy_factor_fp / SCALE`,
- `energy_bonus_fp = BASE_REWARD_FP * bonus_fp / SCALE`,
- `energy_bonus_u = (energy_bonus_fp / SCALE) as u64`.

Итоговая награда:

```text
total_reward = pow_reward + energy_bonus_u
```

С учётом `MAX_SUPPLY`.

При отсутствии EnergyProof:

```text
total_reward = pow_reward
```

и `last_seen_timestamp_opt` = `None`.

### 5.4. Phase3: “зелёный хвост”

Условия:

- `PHASE2_SUPPLY_LIMIT <= total_supply < MAX_SUPPLY`.

Награда:

1. Небольшая **PoW‑хвостовая** часть:

   - `pow_reward = PHASE3_POW_REWARD` (по умолчанию `1 GVR`).

2. Основная часть — **энергетическая награда**:

   Требования к EnergyProof те же, что в Phase2.

Расчёт:

- `kwh_fp = kwh * SCALE`,
- `score_fp = ai_score * SCALE`,
- `base_per_kwh_fp = (cfg.base_gvr_per_kwh * SCALE as f64) as u128`,
- `reward_fp ≈ kwh * base_gvr_per_kwh * score`:

  ```text
  reward_fp =
      kwh_fp * base_per_kwh_fp * score_fp / (SCALE * SCALE)
  ```

- `energy_reward_u = (reward_fp / SCALE) as u64`.

Итог:

```text
total_reward = pow_reward + energy_reward_u
```

С учётом `MAX_SUPPLY`.  
Если EnergyProof отсутствует или невалиден — блок получает только `pow_reward`.

---

## 6. EnergyProof

### 6.1. Структура

```rust
pub struct EnergyProof {
    pub producer_id: String,
    pub sequence: u64,
    pub kwh: f64,
    pub timestamp: u128,
    pub ai_score: f64,
    pub ai_signature: Vec<u8>, // DER-encoded ECDSA (k256)
    pub proof_id: Option<String>,
}
```

### 6.2. Canonical bytes и хэш для подписи

```rust
pub fn canonical_bytes(&self) -> Vec<u8> {
    // producer_id (len + bytes, u16 BE + bytes)
    // sequence (u64, BE)
    // kwh (f64, BE)
    // timestamp (u128, BE)
    // ai_score (f64, BE)
    // proof_id (u16 len + bytes) или 0
}
```

Хэш для подписи:

```rust
pub fn hash_for_signing(&self) -> [u8; 32] {
    Sha256(canonical_bytes)
}
```

### 6.3. Подпись и проверка

EnergyProof подписывается AI‑ключом (k256 ECDSA):

- приватный ключ: `ai_key.bin`,
- публичный ключ в SEC1‑формате (uncompressed): `ai_pubkey.bin`,
  хранится в `Blockchain::active_ai_pubkey`.

Проверка:

```rust
pub fn verify_signature(&self, ai_pubkey_sec1: &[u8]) -> Result<bool, String>;
```

- `ai_pubkey_sec1` — сырой SEC1‑паблик,
- `ai_signature` — DER‑подпись,
- сообщение — `hash_for_signing()`.

### 6.4. Валидация полей

```rust
pub fn validate_fields(&self, min_ai_score: f64) -> Result<(), String>
```

Проверяет:

- `kwh > 0` и `kwh <= MAX_KWH_PER_PROOF`,
- `ai_score >= min_ai_score`,
- timestamp не больше `now_ms + MAX_FUTURE_SKEW_MS` (5 минут).

---

## 7. Replay‑защита для EnergyProof

На уровне блокчейна ведётся состояние производителей:

```rust
pub struct ProducerState {
    pub last_seq: u64,
    pub last_ts: u128,
}
```

`Blockchain::producer_state: HashMap<String, ProducerState>`  
ключ — `producer_id`.

При добавлении блока с EnergyProof в `Blockchain::add_block()`:

1. Если для данного `producer_id` есть `ps`:

   - `ep.timestamp` должен быть строго больше `ps.last_ts`,
   - `ep.sequence` должен быть строго больше `ps.last_seq`,
   - разница `dt = ep.timestamp - ps.last_ts` должна быть ≥ `MIN_PROOF_INTERVAL_MS`.

2. В случае нарушения — блок отклоняется как replay или спам.

3. После успешного добавления блока и пересчёта награды, при наличии `last_ts_opt` от `calculate_reward`, обновляется:

```rust
producer_state[producer_id] = ProducerState {
    last_seq: ep.sequence,
    last_ts: last_ts_opt,
};
```

Таким образом:

- один и тот же `(producer_id, sequence)` не может быть использован повторно;
- EnergyProof не может штамповаться слишком часто по времени.

---

## 8. Типы транзакций

Перечисление `Transaction`:

```rust
pub enum Transaction {
    /// Простой трансфер без подписи
    Transfer {
        sender: String,
        receiver: String,
        amount: u64,
    },

    /// Поворот AI-ключа (ротация публичного ключа, которым подписываются EnergyProof)
    RotateAIKey {
        new_ai_pubkey_sec1: Vec<u8>,
        proposer: String,
        signature: Vec<u8>,
    },

    /// Подписанный перевод с комиссией и nonce
    Signed(SignedTransfer),
}
```

### 8.1. Простой Transfer

Семантика:

- списать `amount` с `sender`,
- зачислить `amount` на `receiver`,
- без комиссии и без подписи.

Оставлен для совместимости и простых сценариев, не рекомендуется для публичной сети.

### 8.2. RotateAIKey

Используется для смены активного AI‑публичного ключа, который проверяет подписи EnergyProof.

Поля:

- `new_ai_pubkey_sec1`: новый AI‑паблик в SEC1 (uncompressed),
- `proposer`: строковой идентификатор (например, “ai-admin”),
- `signature`: DER‑подпись старым активным AI‑ключом.

Сообщение для подписи:

```text
hash( proposer || 0x00 || new_ai_pubkey_sec1 )
```

Ротация выполняется в `Blockchain::apply_rotate_ai_key_txs()`:

1. Из `self.active_ai_pubkey` извлекается текущий паблик.
2. Переход разрешён только если:

   - `new_ai_pubkey_sec1` корректен как SEC1‑ключ;
   - ECDSA‑подпись в `signature` валидна для сообщения выше и текущего активного AI‑паблика.

3. При успехе:

```rust
self.active_ai_pubkey = Some(new_ai_pubkey_sec1.clone());
```

Балансы не затрагиваются.

### 8.3. SignedTransfer

Определён в `accounts.rs`:

```rust
pub struct SignedTransfer {
    pub from: String,
    pub to: String,
    pub amount: u64,
    pub fee: u64,
    pub nonce: u64,
    pub pubkey_sec1: Vec<u8>,
    pub signature: Vec<u8>,
}
```

#### 8.3.1. Canonical bytes и хэш для подписи

```rust
buf.extend(self.from.as_bytes());
buf.push(0u8);
buf.extend(self.to.as_bytes());
buf.push(0u8);
buf.extend(self.amount.to_le_bytes());
buf.extend(self.fee.to_le_bytes());
buf.extend(self.nonce.to_le_bytes());
```

`hash_for_signing()` = SHA256 от этих canonical bytes.

#### 8.3.2. Проверка подписи

```rust
pub fn verify(&self) -> Result<bool, String>;
```

- `VerifyingKey::from_sec1_bytes(&self.pubkey_sec1)`,
- `Signature::from_der(&self.signature)`,
- `vk.verify(hash_for_signing(), &sig)`.

Mempool (`Mempool::add_tx`) для `Transaction::Signed`:

1. Проверяет подпись `st.verify()`:
   - если невалидна — транзакция отклоняется.
2. Проверяет `st.fee >= MIN_SIGNED_FEE`:
   - иначе отклоняет.

#### 8.3.3. Семантика на уровне состояния (`State::apply_tx`)

При применении `Transaction::Signed(st)`:

1. `st.amount > 0`, иначе ошибка.
2. `st.fee >= MIN_SIGNED_FEE`, иначе ошибка.
3. Проверяется nonce:

   - `expected_nonce = state.nonce_of(&st.from)`,
   - если `st.nonce != expected_nonce` — ошибка.

4. Списывается `amount + fee` со счёта `from` (проверка баланса).
5. Зачисляется `amount` на `to`.
6. Комиссия `fee` зачисляется на coinbase‑адрес `State::coinbase`.
7. Nonce отправителя увеличивается на 1.

Функция `apply_txs_atomic()` применяет набор транзакций к state атомарно.

---

## 9. Аккаунтная модель и состояние

Состояние хранится в структуре `State`:

```rust
pub struct State {
    pub balances: HashMap<String, u64>,
    pub nonces: HashMap<String, u64>,
    pub coinbase: String,
}
```

- **Адрес** — произвольная строка (например, `alice`, `bob`, `alekseymonin1992`).
- `balances` — балансы аккаунтов.
- `nonces` — nonce‑ы для Signed‑транзакций.
- `coinbase` — адрес, куда зачисляются награды и комиссии.

---

## 10. P2P‑уровень

P2P‑протокол реализован поверх TCP, с собственным бинарно‑JSON протоколом сообщений.

### 10.1. Общая схема

- Каждый P2P‑узел слушает TCP‑адрес (`--p2p_addr`, по умолчанию `127.0.0.1:4000`).
- Подключения двунаправленные: узлы как принимают, так и инициируют соединения.
- Входящий поток соединений ограничен (`MAX_INBOUND_CONN = 128`).
- Список известных пиров ограничен (`MAX_PEERS = 512`).

Сообщения сериализуются через JSON (`serde`), с префиксом длины:

1. 4 байта: длина JSON‑payload (u32 BE),
2. JSON‑payload.

### 10.2. Формат сообщений

Перечисление `P2pMessage`:

```rust
#[serde(tag = "type", content = "payload")]
enum P2pMessage {
    Hello {
        node_id: String,
        height: u64,
        last_hash: String,
        pubkey_sec1: Vec<u8>,
        signature: Vec<u8>,
    },

    Block(Block),
    Tx(Transaction),

    Ping,
    GetStatus,
    Status { height: u64, last_hash: String },

    GetBlocks { from_index: u64, max: u64 },
    GetBlocksFromLocators { locators: Vec<String>, max: u64 },
    Blocks(Vec<Block>),

    InvTx { tx_hashes: Vec<String> },
    InvBlock { block_hashes: Vec<String> },
    GetDataTx { tx_hashes: Vec<String> },
    GetDataBlock { block_hashes: Vec<String> },
    GetMempool,
    MempoolInv { tx_hashes: Vec<String> },

    GetPeers,
    Peers(Vec<String>),
}
```

#### 10.2.1. Hello‑handshake

`Hello` используется для идентификации узла и проверки подписи.

Поля:

- `node_id`: строковой ID узла (`node-xxxxxxxxxxxxxx`),
- `height`: высота цепи на отправителе,
- `last_hash`: tip‑hash,
- `pubkey_sec1`: публичный P2P‑ключ узла,
- `signature`: DER‑подпись P2P‑ключом.

Сообщение для подписи:

```rust
fn hello_canonical_bytes(node_id: &str, height: u64, last_hash: &str, pubkey_sec1: &[u8]) -> Vec<u8>;
fn hash_hello(...) -> [u8; 32] = Sha256(hello_canonical_bytes);
```

Подпись:

- `Signature = p2p_sk.sign(hash_hello(...))`,
- проверяется через `VerifyingKey::from_sec1_bytes(pubkey_sec1)`.

Если подпись невалидна — соединение закрывается.

#### 10.2.2. Синхронизация блоков

- `GetStatus` / `Status` — узнать высоту и tip‑hash пира.
- `GetBlocks { from_index, max }` — запрос линейного диапазона блоков по индексу.
- `Blocks(Vec<Block>)` — ответ с последовательностью блоков.

Оптимизированный вариант:

- `GetBlocksFromLocators { locators, max }`:

  - `locators` — список известных хэшей по экспоненциальной схеме (recent → genesis),
  - узел находит общий предок и возвращает блоки после него (до `max` штук).

#### 10.2.3. Распространение блоков и транзакций

Механизм похож на Bitcoin:

- при получении нового блока/транзакции узел:
  - проверяет по `SeenCache`, не видел ли уже этот объект;
  - если нет — добавляет, пытается применить (`add_block()` или `mempool.add_tx()`),
  - формирует `InvBlock` или `InvTx` и рассылает пиру список новых хэшей.

Пир, получив `InvBlock`/`InvTx`, может запросить конкретные объекты через:

- `GetDataBlock { block_hashes }` или
- `GetDataTx { tx_hashes }`.

Mempool синхронизируется через:

- `GetMempool` → `MempoolInv { tx_hashes }`.

#### 10.2.4. Пиры и бан

Узлы ведут структуру `PeerState`:

```rust
pub struct PeerState {
    pub last_error_ts: u128,
    pub error_count: u32,
    pub banned_until: u128,
    pub last_contact_ts: u128,
}
```

- Если пир даёт ошибки слишком часто (`error_count >= 5`), он может быть временно забанен (`banned_until = now + 60_000`).
- Также есть простые rate‑limit’ы по количеству сообщений с одного соединения.

---

## 11. RPC‑API

HTTP‑RPC реализован на базе `axum`. По умолчанию нода слушает:

- `--rpc_addr 127.0.0.1:8080`

Базовые эндпоинты:

### 11.1. `GET /status`

Возвращает общий статус ноды.

**Пример ответа:**

```json
{
  "height": 123,
  "tip": "abcd1234...",
  "difficulty": 5,
  "total_supply": 100000,
  "alice_balance": 500,
  "bob_balance": 1000,
  "alice_nonce": 3,
  "phase": "Phase1"
}
```

### 11.2. `POST /tx`

Принимает подписанный перевод (`SignedTransferDto`).

**Тело запроса:**

```json
{
  "from": "alice",
  "to": "bob",
  "amount": 10,
  "nonce": 0,
  "pubkey_sec1": "<bytes...>",
  "signature": "<bytes...>",
  "fee": 1
}
```

`pubkey_sec1` и `signature` — массивы байт (на стороне Rust — `Vec<u8>`).

Если транзакция принята в mempool:

```json
{ "tx_hash": "..." }
```

Если отклонена — HTTP 400 с текстом ошибки.

### 11.3. `POST /energy_proof`

Принимает `EnergyProofDto`:

```json
{
  "producer_id": "dev_producer",
  "sequence": 1,
  "kwh": 100.0,
  "timestamp": 1700000000000,
  "ai_score": 0.9,
  "ai_signature": "<bytes...>",
  "proof_id": null
}
```

Поведение:

- простой rate‑limit (не чаще 1 раза в 1000 мс);
- проверяются поля `EnergyProof` и подпись по `active_ai_pubkey` ноды;
- при успехе EnergyProof сохраняется в `last_energy_proof` и будет использован майнером в следующем блоке (до появления нового proof’а).

Успешный ответ:

```json
{
  "status": "ok",
  "producer_id": "dev_producer",
  "sequence": 1
}
```

### 11.4. `GET /balance?addr=...`

Возвращает баланс адреса:

```json
{
  "addr": "alice",
  "balance": 123
}
```

### 11.5. `GET /nonce?addr=...`

Возвращает текущий `nonce` адреса:

```json
{
  "addr": "alice",
  "nonce": 3
}
```

### 11.6. `GET /peers`

Возвращает список известных пиров и их состояние (для отладки):

```json
{
  "peers": [
    {
      "addr": "127.0.0.1:4000",
      "last_error_ts": 0,
      "error_count": 0,
      "banned_until": 0,
      "last_contact_ts": 1700000000000
    }
  ]
}
```

### 11.7. `GET or POST /sync?peer=IP:PORT`

Запускает фоновую синхронизацию с указанным P2P‑пиром.

Пример:

```bash
curl "http://127.0.0.1:8081/sync?peer=127.0.0.1:4000"
```

Ответ:

```text
sync started
```

### 11.8. `POST /ban?peer=IP:PORT[&duration_ms=...]`

Ручной бан пира через RPC.

Пример:

```bash
curl -X POST "http://127.0.0.1:8080/ban?peer=1.2.3.4:4000&duration_ms=60000"
```

Ответ:

```json
{
  "status": "banned",
  "peer": "1.2.3.4:4000",
  "until": 1700000000000
}
```

### 11.9. `POST /unban?peer=IP:PORT`

Снять бан:

```bash
curl -X POST "http://127.0.0.1:8080/unban?peer=1.2.3.4:4000"
```

---

## 12. How to Run (коротко)

### 12.1. Сборка

```bash
cargo build --release
```

Бинарники:

- `gvr-node` — основная нода,
- `gvr-client` — простой RPC‑клиент,
- `gvr-wallet` — кошелёк,
- `gvr-p2p-client` — P2P‑клиент,
- `gvr-ai-keygen` — генерация AI‑ключа,
- `gvr-energy-client` — клиент для `/energy_proof`,
- `gvr-ai-rotate` — инструмент ротации AI‑ключа.

### 12.2. Генерация AI‑ключа

```bash
target/release/gvr-ai-keygen
```

Создаёт:

- `ai_key.bin` (приватный AI‑ключ),
- `ai_pubkey.bin` (публичный SEC1‑ключ).

### 12.3. Запуск первой ноды

```bash
target/release/gvr-node \
  --p2p_addr 127.0.0.1:4000 \
  --rpc_addr 127.0.0.1:8080 \
  --coinbase_addr alice \
  --ai-key-file ai_key.bin
```

### 12.4. Создание кошелька

```bash
target/release/gvr-wallet new --name alekseymonin1992
```

Проверить:

```bash
target/release/gvr-wallet show --name alekseymonin1992
```

### 12.5. Отправка транзакции

```bash
target/release/gvr-wallet send \
  --rpc 127.0.0.1:8080 \
  --from_wallet alekseymonin1992 \
  --to bob \
  --amount 10 \
  --fee 1
```

### 12.6. Отправка EnergyProof

```bash
target/release/gvr-energy-client \
  --rpc 127.0.0.1:8080 \
  --producer_id my_station_1 \
  --sequence 1 \
  --kwh 123.45 \
  --ai_score 0.92
```

### 12.7. Вторая нода и синхронизация

```bash
target/release/gvr-node \
  --p2p_addr 127.0.0.1:4001 \
  --rpc_addr 127.0.0.1:8081 \
  --coinbase_addr bob \
  --ai-pubkey-file ai_pubkey.bin \
  --peers 127.0.0.1:4000
```

Синхронизация:

```bash
curl "http://127.0.0.1:8081/sync?peer=127.0.0.1:4000"
```

---

## 13. Репозиторий и обновление протокола

Этот документ (`PROTO.md`) должен версионироваться вместе с кодом.  
При любом изменении протокола (эмиссия, форматы сообщений, типы транзакций и т.д.) необходимо обновлять этот файл и повышать версию в заголовке.
```