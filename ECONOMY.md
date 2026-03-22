# GVR Economy (Tokenomics)

Документ описывает экономическую модель GVR Hybrid Chain в текущей реализации.

- Максимальная эмиссия: **21 000 000 GVR**
- Три фазы эмиссии:
  - Phase1 — классический PoW
  - Phase2 — гибрид PoW + EnergyProof
  - Phase3 — «зелёный хвост»: основная награда за энергию

Все параметры и формулы ниже соответствуют текущему коду (`constants.rs`, `emission.rs`, `state.rs` и др.).

---

## 1. Основные параметры

**Базовые константы (из `constants.rs`):**

- `MAX_SUPPLY = 21_000_000`  
  Максимальное количество GVR, которое когда-либо будет создано.

- `BASE_REWARD = 50`  
  Базовая награда за блок в Phase1.

- `PHASE3_POW_REWARD = 1`  
  Награда за PoW в Phase3 (хвостовая, символическая).

- `MIN_SIGNED_FEE = 1`  
  Минимальная комиссия за `Signed`-транзакцию (на уровне протокола и mempool).

**Пороги фаз по суммарной эмиссии (`total_supply`):**

- `PHASE1_SUPPLY_LIMIT = 9_000_000` GVR  
  Phase1: `0 .. 9_000_000`.

- `PHASE2_SUPPLY_LIMIT = 15_000_000` GVR  
  Phase2: `9_000_000 .. 15_000_000`.

- Phase3: `15_000_000 .. 21_000_000` GVR (`MAX_SUPPLY`).

**Параметры энергии и AI:**

- `MIN_AI_SCORE = 0.8`  
  Минимальный допустимый `ai_score` для EnergyProof.

- `ENERGY_FACTOR = 0.01`  
  Коэффициент влияния энергии в Phase2.

- `BASE_GVR_PER_KWH = 10.0`  
  Базовая доходность энергии в Phase3 (условные GVR за kWh * score).

---

## 2. Фазы эмиссии

### 2.1. Phase1 — чистый PoW

**Диапазон:**

- `0 <= total_supply < PHASE1_SUPPLY_LIMIT = 9_000_000`

**Правило награды:**

```text
reward = BASE_REWARD = 50 GVR
(пока total_supply < MAX_SUPPLY)