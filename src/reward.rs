use crate::energy_proof::EnergyProof;

pub const MIN_AI_SCORE: f64 = 0.8;

pub fn calculate_reward(base_reward: f64, proof: &EnergyProof, max_reward: f64) -> f64 {
    if proof.ai_score < MIN_AI_SCORE { return 0.0; }
    let reward = base_reward * proof.kwh * proof.ai_score;
    reward.min(max_reward)
}