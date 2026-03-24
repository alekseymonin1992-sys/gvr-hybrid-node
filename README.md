```markdown
# GVR Hybrid Node

GVR — экспериментальная гибридная криптовалюта на Rust с трёхфазной экономикой:

- **Phase1** — классический PoW (фиксированная награда).
- **Phase2** — гибрид PoW + EnergyProof (AI‑подтверждённая энергия).
- **Phase3** — "зелёный хвост": основная награда за энергию, маленький PoW‑хвост.

Проект включает:

- полноценную ноду (`gvr_hybrid_node`),
- RPC API (на базе `axum`),
- P2P‑сеть,
- CLI‑клиент (`gvr-client`),
- CLI‑кошелёк (`gvr-wallet`),
- P2P‑клиент (`gvr-p2p-client`),
- утилиты для AI‑ключей и EnergyProof (`gvr-ai-keygen`, `gvr-energy-client`, `gvr-ai-rotate`),
- сборку для Windows в виде папки `dist` с `.exe` и `.bat`.

- Полное описание **экономики**: [ECONOMY.md](ECONOMY.md).  
- Техническая спецификация **протокола**: [PROTO.md](PROTO.md).

[English README](README.en.md)

---

## 1. Возможности

- Гибридный консенсус: PoW + EnergyProof.
- Трёхфазная эмиссия с жёстким лимитом `21 000 000 GVR`.
- Поддержка подписанных транзакций (`SignedTransfer`) с комиссиями и nonce.
- Встроенный P2P‑протокол с синхронизацией по locators и баном пиров.
- HTTP RPC API для интеграции с приложениями и фронтендами.
- Отдельный CLI‑кошелёк и клиенты для работы с транзакциями и EnergyProof.

---

## 2. Сборка из исходников

Требуется установленный Rust (stable) и Cargo.

```bash
git clone https://github.com/alekseymonin1992-sys/gvr-hybrid-node.git
cd gvr-hybrid-node
cargo build --release
```

Соберутся бинарники (в `target/release`):

- `gvr-node` — основная нода,
- `gvr-client` — простой RPC‑клиент,
- `gvr-wallet` — CLI‑кошелёк,
- `gvr-p2p-client` — P2P‑клиент,
- `gvr-ai-keygen` — генерация AI‑ключа,
- `gvr-energy-client` — отправка EnergyProof через RPC,
- `gvr-ai-rotate` — ротация AI‑публичного ключа.

---

## 3. Быстрый старт

### 3.1. Генерация AI‑ключа

AI‑ключ используется для подписи и проверки EnergyProof.

```bash
target/release/gvr-ai-keygen
```

Создаст:

- `ai_key.bin` — приватный AI‑ключ (ECDSA k256),
- `ai_pubkey.bin` — публичный AI‑ключ (SEC1, uncompressed).

### 3.2. Запуск ноды

Пример запуска одной ноды с майнингом и RPC:

```bash
target/release/gvr-node \
  --p2p_addr 127.0.0.1:4000 \
  --rpc_addr 127.0.0.1:8080 \
  --coinbase_addr alice \
  --ai-key-file ai_key.bin
```

- `--coinbase_addr` — адрес, на который идут награды за блоки и комиссии.
- При корректном завершении нода сохраняет снимок состояния в `state.json`.

### 3.3. Создание кошелька

```bash
target/release/gvr-wallet new --name alekseymonin1992
```

Выведет:

- путь к файлу приватного ключа (`wallets/alekseymonin1992.key`),
- адрес в сети (строка `alekseymonin1992`),
- публичный ключ в формате SEC1 (hex).

Проверить кошелёк:

```bash
target/release/gvr-wallet show --name alekseymonin1992
```

### 3.4. Отправка транзакции

Перевести 10 GVR с кошелька `alekseymonin1992` на адрес `bob`:

```bash
target/release/gvr-wallet send \
  --rpc 127.0.0.1:8080 \
  --from_wallet alekseymonin1992 \
  --to bob \
  --amount 10 \
  --fee 1
```

Кошелёк:

- сам запросит `nonce` через `/nonce?addr=...`,
- подпишет транзакцию ECDSA‑ключом,
- отправит DTO в `/tx` на ноду.

### 3.5. Отправка EnergyProof

Отправить EnergyProof (выработка энергии) на ноду:

```bash
target/release/gvr-energy-client \
  --rpc 127.0.0.1:8080 \
  --producer_id my_station_1 \
  --sequence 1 \
  --kwh 123.45 \
  --ai_score 0.92
```

Клиент:

- собирает структуру `EnergyProof`,
- подписывает её AI‑ключом (`ai_key.bin`),
- проверяет подпись локально,
- отправляет DTO на `/energy_proof`.

Нода:

- валидирует поля и подпись,
- при успехе сохраняет proof и использует его при майнинге следующих блоков (Phase2/Phase3).

---

## 4. Работа в сети из нескольких нод

### 4.1. Нода №1

```bash
target/release/gvr-node \
  --p2p_addr 127.0.0.1:4000 \
  --rpc_addr 127.0.0.1:8080 \
  --coinbase_addr alice \
  --ai-key-file ai_key.bin
```

### 4.2. Нода №2

```bash
target/release/gvr-node \
  --p2p_addr 127.0.0.1:4001 \
  --rpc_addr 127.0.0.1:8081 \
  --coinbase_addr bob \
  --ai-pubkey-file ai_pubkey.bin \
  --peers 127.0.0.1:4000
```

Синхронизация Ноды №2 с Нодой №1 через RPC:

```bash
curl "http://127.0.0.1:8081/sync?peer=127.0.0.1:4000"
```

### 4.3. Диагностика и статус

Статус ноды:

```bash
curl http://127.0.0.1:8080/status
```

Баланс адреса:

```bash
curl "http://127.0.0.1:8080/balance?addr=alice"
```

Nonce адреса:

```bash
curl "http://127.0.0.1:8080/nonce?addr=alice"
```

Список пиров:

```bash
curl http://127.0.0.1:8080/peers
```

---

## 5. Статус проекта

Проект в статусе **Draft / Experimental**:

- протокол, параметры эмиссии и экономическая модель могут меняться;
- возможны несовместимые изменения формата блоков и RPC API.

Для детальной информации:

- протокол: [PROTO.md](PROTO.md),
- экономика: 