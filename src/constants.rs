// Начальная сложность PoW
pub const INITIAL_DIFFICULTY: u32 = 3;
pub const DIFFICULTY_ADJUST_INTERVAL: u64 = 10;

// Целевое время блока ~90 секунд (боевой режим)
pub const TARGET_BLOCK_TIME_SEC: u64 = 90;

// Границы сложности для авто‑регулировки
pub const MIN_DIFFICULTY: u32 = 1;
pub const MAX_DIFFICULTY: u32 = 32;

// Максимальная глубина reorg (в блоках) — защита от очень глубоких форков
pub const MAX_REORG_DEPTH: u64 = 100;

// Минимальный объём энергии для учёта (если где‑то понадобится)
pub const MIN_ENERGY_KWH: u64 = 20;

// Общий максимум эмиссии (боевой)
pub const MAX_SUPPLY: u64 = 21_000_000;

// Базовая награда в Phase1
pub const BASE_REWARD: u64 = 50;

// Малая PoW‑награда в Phase3 (хвостовая поддержка PoW)
pub const PHASE3_POW_REWARD: u64 = 1;

// МИНИМАЛЬНАЯ КОМИССИЯ ЗА Signed‑ТРАНЗАКЦИЮ
pub const MIN_SIGNED_FEE: u64 = 1;

// Fixed-point scale: 1 GVR = 1 * 1e8 units
pub const SCALE: u128 = 100_000_000u128;

// Параметры для EnergyProof
pub const MIN_AI_SCORE: f64 = 0.8;
// Для боевой сети можно сделать аккуратный коэффициент
pub const ENERGY_FACTOR: f64 = 0.01;
pub const BASE_GVR_PER_KWH: f64 = 10.0;

// FP‑константы
pub const BASE_REWARD_FP: u128 = (BASE_REWARD as u128) * SCALE;
pub const ENERGY_FACTOR_FP: u128 = (ENERGY_FACTOR * SCALE as f64) as u128;
pub const BASE_GVR_PER_KWH_FP: u128 =
    (BASE_GVR_PER_KWH * SCALE as f64) as u128;

// Допустимое расхождение по времени (ms) для EnergyProof
pub const ALLOWED_TIMESTAMP_SKEW_MS: u128 = 5 * 60 * 1000;

// Максимальный kWh в одном EnergyProof (анти-абуз).
// Для боевой сети подставить реалистичный максимум (например, 10_000 MWh = 10_000_000 kWh).
pub const MAX_KWH_PER_PROOF: f64 = 10_000_000.0;

// Минимальный интервал между двумя EnergyProof одного producer (по времени proof'а), мс.
// В боевой сети, например, 10 минут: один proof на станцию раз в 10 минут.
pub const MIN_PROOF_INTERVAL_MS: u128 = 10 * 60 * 1000;

// Persistence
pub const SNAPSHOT_INTERVAL: usize = 10;
pub const SNAPSHOT_FILE: &str = "state.json";
pub const SNAPSHOT_TMP: &str = "state.json.tmp";

// Эмиссионные фазы (боевые пороги)
// Phase1:    0          .. 9_000_000   GVR
// Phase2:    9_000_000  .. 15_000_000  GVR
// Phase3:    15_000_000 .. 21_000_000  GVR
pub const PHASE1_SUPPLY_LIMIT: u64 = 9_000_000;
pub const PHASE2_SUPPLY_LIMIT: u64 = 15_000_000;