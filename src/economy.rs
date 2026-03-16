pub const MAX_SUPPLY: u64 = 21_000_000;
pub const INITIAL_REWARD: u64 = 10;
pub const HALVING_INTERVAL: u64 = 210_000;

use crate::block::EnergyProof;

pub fn block_reward(total_supply: u64, energy_proof: Option<&EnergyProof>) -> u64 {
    if total_supply < 1_000_000 {
        // Фаза 1 — PoW
        INITIAL_REWARD
    } else if total_supply < 3_000_000 {
        // Фаза 2 — гибрид PoW + Energy
        let pow_reward = INITIAL_REWARD / 2;
        let energy_reward = energy_proof.map_or(0, |ep| ep.energy_kwh / 10);
        pow_reward + energy_reward
    } else {
        // Фаза 3 — только Green
        energy_proof.map_or(0, |ep| ep.energy_kwh / 10)
    }
}