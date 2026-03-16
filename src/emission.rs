use crate::constants::*;
use crate::energy::EnergyProof;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct EmissionConfig {
    pub max_supply: u64,
    pub base_reward: u64,
    pub min_ai_score: f64,
    pub energy_factor: f64,
    pub base_gvr_per_kwh: f64,
    pub allowed_skew_ms: u128,
}

impl Default for EmissionConfig {
    fn default() -> Self {
        EmissionConfig {
            max_supply: MAX_SUPPLY,
            base_reward: BASE_REWARD,
            min_ai_score: MIN_AI_SCORE,
            energy_factor: ENERGY_FACTOR,
            base_gvr_per_kwh: BASE_GVR_PER_KWH,
            allowed_skew_ms: ALLOWED_TIMESTAMP_SKEW_MS,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum EmissionPhase {
    Phase1,
    Phase2,
    Phase3,
}

pub fn current_phase(total_supply: u64) -> EmissionPhase {
    if total_supply < PHASE1_SUPPLY_LIMIT {
        EmissionPhase::Phase1
    } else if total_supply < PHASE2_SUPPLY_LIMIT {
        EmissionPhase::Phase2
    } else {
        EmissionPhase::Phase3
    }
}
pub fn calculate_reward(
    total_supply: u64,
    energy_proof: Option<&EnergyProof>,
    ai_pubkey_opt: Option<&[u8]>,
    _last_seen_timestamp_opt: Option<u128>,
    cfg: &EmissionConfig,
) -> Result<(u64, Option<u128>), String> {
    let phase = current_phase(total_supply);

    match phase {
        EmissionPhase::Phase1 => {
            let mut reward = cfg.base_reward;
            if total_supply >= cfg.max_supply {
                return Ok((0, None));
            }
            if total_supply + reward > cfg.max_supply {
                reward = cfg.max_supply - total_supply;
            }
            Ok((reward, None))
        }
        EmissionPhase::Phase2 => {
            let proof = energy_proof.ok_or_else(|| "EnergyProof required in Phase2".to_string())?;
            proof.validate_fields(cfg.min_ai_score)?;

            let now_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|e| format!("time error: {}", e))?
                .as_millis() as u128;

            if proof.timestamp > now_ms + cfg.allowed_skew_ms {
                return Err("EnergyProof timestamp too far in future".into());
            }

            let ai_pub = ai_pubkey_opt.ok_or_else(|| "AI public key not configured on node".to_string())?;
            let ok = proof
                .verify_signature(ai_pub)
                .map_err(|e| format!("signature parse/verify error: {}", e))?;
            if !ok {
                return Err("AI signature invalid".into());
            }

            let kwh_fp = proof.kwh_fp();
            let score_fp = proof.ai_score_fp();

            let term = if kwh_fp == 0 || score_fp == 0 {
                0u128
            } else {
                (kwh_fp.saturating_mul(score_fp)) / SCALE
            };

            let energy_factor_fp = (cfg.energy_factor * SCALE as f64) as u128;
            let bonus_fp = if term == 0 {
                0u128
            } else {
                term.saturating_mul(energy_factor_fp) / SCALE
            };
            let multiplier_fp = SCALE.saturating_add(bonus_fp);
            let reward_fp = BASE_REWARD_FP.saturating_mul(multiplier_fp) / SCALE;
            let mut reward_u = (reward_fp / SCALE) as u64;

            if total_supply >= cfg.max_supply {
                return Ok((0, None));
            }
            if total_supply + reward_u > cfg.max_supply {
                reward_u = cfg.max_supply - total_supply;
            }

            Ok((reward_u, Some(proof.timestamp)))
        }
        EmissionPhase::Phase3 => {
            let proof = energy_proof.ok_or_else(|| "EnergyProof required in Phase3".to_string())?;
            proof.validate_fields(cfg.min_ai_score)?;

            let now_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|e| format!("time error: {}", e))?
                .as_millis() as u128;

            if proof.timestamp > now_ms + cfg.allowed_skew_ms {
                return Err("EnergyProof timestamp too far in future".into());
            }

            let ai_pub = ai_pubkey_opt.ok_or_else(|| "AI public key not configured on node".to_string())?;
            let ok = proof
                .verify_signature(ai_pub)
                .map_err(|e| format!("signature parse/verify error: {}", e))?;
            if !ok {
                return Err("AI signature invalid".into());
            }

            let kwh_fp = proof.kwh_fp();
            let score_fp = proof.ai_score_fp();
            let base_per_kwh_fp = (cfg.base_gvr_per_kwh * SCALE as f64) as u128;

            let mut reward_fp = 0u128;
            if kwh_fp != 0 && score_fp != 0 {
                reward_fp = kwh_fp
                    .saturating_mul(base_per_kwh_fp)
                    .saturating_mul(score_fp)
                    / (SCALE.saturating_mul(SCALE));
            }

            let mut reward_u = (reward_fp / SCALE) as u64;

            if total_supply >= cfg.max_supply {
                return Ok((0, None));
            }
            if total_supply + reward_u > cfg.max_supply {
                reward_u = cfg.max_supply - total_supply;
            }

            Ok((reward_u, Some(proof.timestamp)))
        }
    }
}