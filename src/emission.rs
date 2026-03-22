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

/// Основная функция расчёта награды за блок.
/// Возвращает (reward, last_seen_timestamp_for_producer)
pub fn calculate_reward(
    total_supply: u64,
    energy_proof: Option<&EnergyProof>,
    ai_pubkey_opt: Option<&[u8]>,
    _last_seen_timestamp_opt: Option<u128>,
    cfg: &EmissionConfig,
) -> Result<(u64, Option<u128>), String> {
    let phase = current_phase(total_supply);

    match phase {
        // Phase1: классический фиксированный BASE_REWARD до порога.
        EmissionPhase::Phase1 => {
            let mut reward = cfg.base_reward;
            if total_supply >= cfg.max_supply {
                return Ok((0, None));
            }
            if total_supply + reward > cfg.max_supply {
                reward = cfg.max_supply - total_supply;
            }

            // Лёгкий лог для Phase1
            println!(
                "Phase1 reward: base_reward={} total_supply_before={}",
                reward, total_supply
            );

            Ok((reward, None))
        }

        // Phase2: гибридная модель.
        // Даже без EnergyProof блок получает базовую PoW-награду (pow_reward),
        // а при наличии валидного EnergyProof добавляется energy_bonus.
        EmissionPhase::Phase2 => {
            // Базовая часть за PoW (половина BASE_REWARD)
            let mut pow_reward: u64 = cfg.base_reward / 2; // 25 при BASE_REWARD=50

            if total_supply >= cfg.max_supply {
                return Ok((0, None));
            }
            if total_supply + pow_reward > cfg.max_supply {
                pow_reward = cfg.max_supply - total_supply;
            }

            // Если EnergyProof нет — отдаём только pow_reward.
            let proof = match energy_proof {
                None => {
                    println!(
                        "Phase2 reward: pow_reward={} energy_bonus=0 total_reward={} (no EnergyProof, total_supply_before={})",
                        pow_reward,
                        pow_reward,
                        total_supply
                    );
                    return Ok((pow_reward, None));
                }
                Some(p) => p,
            };

            // Есть EnergyProof — считаем бонус.
            proof.validate_fields(cfg.min_ai_score)?;

            let now_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|e| format!("time error: {}", e))?
                .as_millis() as u128;

            if proof.timestamp > now_ms + cfg.allowed_skew_ms {
                return Err("EnergyProof timestamp too far in future".into());
            }

            let ai_pub = ai_pubkey_opt
                .ok_or_else(|| "AI public key not configured on node".to_string())?;
            let ok = proof
                .verify_signature(ai_pub)
                .map_err(|e| format!("signature parse/verify error: {}", e))?;
            if !ok {
                return Err("AI signature invalid".into());
            }

            let kwh_fp = proof.kwh_fp();
            let score_fp = proof.ai_score_fp();

            // term ≈ kwh * score в FP
            let term = if kwh_fp == 0 || score_fp == 0 {
                0u128
            } else {
                (kwh_fp.saturating_mul(score_fp)) / SCALE
            };

            let energy_factor_fp = (cfg.energy_factor * SCALE as f64) as u128;

            // bonus_fp ~ term * ENERGY_FACTOR
            let bonus_fp = if term == 0 {
                0u128
            } else {
                term.saturating_mul(energy_factor_fp) / SCALE
            };

            // energy_bonus_fp ~ BASE_REWARD * bonus
            let energy_bonus_fp =
                BASE_REWARD_FP.saturating_mul(bonus_fp) / SCALE;

            let energy_bonus_u = (energy_bonus_fp / SCALE) as u64;

            // Общая награда = pow_reward + energy_bonus.
            let mut total_reward = pow_reward.saturating_add(energy_bonus_u);

            if total_supply >= cfg.max_supply {
                return Ok((0, None));
            }
            if total_supply + total_reward > cfg.max_supply {
                total_reward = cfg.max_supply - total_supply;
            }

            println!(
                "Phase2 reward: pow_reward={} energy_bonus={} total_reward={} (kwh={} ai_score={} total_supply_before={})",
                pow_reward,
                energy_bonus_u,
                total_reward,
                proof.kwh,
                proof.ai_score,
                total_supply
            );

            Ok((total_reward, Some(proof.timestamp)))
        }

        // Phase3: "зелёная" модель с маленьким PoW-хвостом.
        // Небольшой фиксированный pow_reward + основная часть от энергии.
        EmissionPhase::Phase3 => {
            let mut pow_reward = PHASE3_POW_REWARD;

            if total_supply >= cfg.max_supply {
                return Ok((0, None));
            }
            if total_supply + pow_reward > cfg.max_supply {
                pow_reward = cfg.max_supply - total_supply;
            }

            let proof = match energy_proof {
                None => {
                    // Если нет EnergyProof, отдаём только крошечную PoW-награду.
                    println!(
                        "Phase3 reward: pow_reward={} energy_reward=0 total_reward={} (no EnergyProof, total_supply_before={})",
                        pow_reward,
                        pow_reward,
                        total_supply
                    );
                    return Ok((pow_reward, None));
                }
                Some(p) => p,
            };

            proof.validate_fields(cfg.min_ai_score)?;

            let now_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|e| format!("time error: {}", e))?
                .as_millis() as u128;

            if proof.timestamp > now_ms + cfg.allowed_skew_ms {
                return Err("EnergyProof timestamp too far in future".into());
            }

            let ai_pub = ai_pubkey_opt
                .ok_or_else(|| "AI public key not configured on node".to_string())?;
            let ok = proof
                .verify_signature(ai_pub)
                .map_err(|e| format!("signature parse/verify error: {}", e))?;
            if !ok {
                return Err("AI signature invalid".into());
            }

            let kwh_fp = proof.kwh_fp();
            let score_fp = proof.ai_score_fp();
            let base_per_kwh_fp = (cfg.base_gvr_per_kwh * SCALE as f64) as u128;

            // reward_fp ~ kwh * base_per_kwh * score
            let mut reward_fp = 0u128;
            if kwh_fp != 0 && score_fp != 0 {
                reward_fp = kwh_fp
                    .saturating_mul(base_per_kwh_fp)
                    .saturating_mul(score_fp)
                    / (SCALE.saturating_mul(SCALE));
            }

            let energy_reward_u = (reward_fp / SCALE) as u64;

            // Общая награда = маленькая PoW + энергетическая.
            let mut total_reward = pow_reward.saturating_add(energy_reward_u);

            if total_supply >= cfg.max_supply {
                return Ok((0, None));
            }
            if total_supply + total_reward > cfg.max_supply {
                total_reward = cfg.max_supply - total_supply;
            }

            println!(
                "Phase3 reward: pow_reward={} energy_reward={} total_reward={} (kwh={} ai_score={} total_supply_before={})",
                pow_reward,
                energy_reward_u,
                total_reward,
                proof.kwh,
                proof.ai_score,
                total_supply
            );

            Ok((total_reward, Some(proof.timestamp)))
        }
    }
}