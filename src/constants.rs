pub const INITIAL_DIFFICULTY: u32 = 3;
pub const DIFFICULTY_ADJUST_INTERVAL: u64 = 10;

// Целевое время блока ~90 секунд
pub const TARGET_BLOCK_TIME_SEC: u64 = 90;

// Границы сложности для авто‑регулировки
pub const MIN_DIFFICULTY: u32 = 1;
pub const MAX_DIFFICULTY: u32 = 32;

pub const MIN_ENERGY_KWH: u64 = 20;

// Emission params
pub const MAX_SUPPLY: u64 = 21_000_000;
pub const BASE_REWARD: u64 = 50; // Phase1 fixed

// Fixed-point scale: 1 GVR = 1 * 1e8 units
pub const SCALE: u128 = 100_000_000u128;

pub const MIN_AI_SCORE: f64 = 0.8;
pub const ENERGY_FACTOR: f64 = 0.01;
pub const BASE_GVR_PER_KWH: f64 = 10.0;

pub const BASE_REWARD_FP: u128 = (BASE_REWARD as u128) * SCALE;
pub const ENERGY_FACTOR_FP: u128 = (ENERGY_FACTOR * SCALE as f64) as u128;
pub const BASE_GVR_PER_KWH_FP: u128 =
    (BASE_GVR_PER_KWH * SCALE as f64) as u128;

pub const ALLOWED_TIMESTAMP_SKEW_MS: u128 = 5 * 60 * 1000;

// Persistence
pub const SNAPSHOT_INTERVAL: usize = 10;
pub const SNAPSHOT_FILE: &str = "state.json";
pub const SNAPSHOT_TMP: &str = "state.json.tmp";

// Эмиссионные фазы (dev/testnet пороги)
pub const PHASE1_SUPPLY_LIMIT: u64 = 1_000;
pub const PHASE2_SUPPLY_LIMIT: u64 = 2_000;