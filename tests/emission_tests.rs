use chrono::Utc;
use gvr_hybrid_node::emission::{calculate_reward, EmissionConfig};
use gvr_hybrid_node::energy::EnergyProof;
use gvr_hybrid_node::constants::{
    PHASE1_SUPPLY_LIMIT, PHASE2_SUPPLY_LIMIT, MIN_AI_SCORE, SCALE,
    PHASE3_POW_REWARD,
};
use k256::ecdsa::{signature::Signer, SigningKey, VerifyingKey};
use rand::rngs::StdRng;
use rand::SeedableRng;

#[test]
fn phase1_reward_fixed() {
    let cfg = EmissionConfig::default();
    let (r, ts): (u64, Option<u128>) =
        calculate_reward(0, None, None, None, &cfg).expect("calc failed");
    assert_eq!(r, cfg.base_reward);
    assert!(ts.is_none());
}

#[test]
fn phase2_reward_with_proof() {
    let cfg = EmissionConfig::default();

    let mut rng = StdRng::from_seed([11u8; 32]);
    let sk = SigningKey::random(&mut rng);
    let vk = VerifyingKey::from(&sk);
    let pubsec1 = vk.to_encoded_point(false).as_bytes().to_vec();

    let mut ep = EnergyProof {
        producer_id: "p3".to_string(),
        sequence: 2,
        kwh: 10.0,
        timestamp: Utc::now().timestamp_millis() as u128,
        ai_score: 0.9,
        ai_signature: vec![],
        proof_id: None,
    };

    let hash = ep.hash_for_signing();
    let sig: k256::ecdsa::Signature = sk.sign(&hash);
    ep.ai_signature = sig.to_der().as_bytes().to_vec();

    // В Phase2 total_supply должен быть в диапазоне [PHASE1_SUPPLY_LIMIT .. PHASE2_SUPPLY_LIMIT).
    let total = PHASE1_SUPPLY_LIMIT; // начало Phase2
    let (reward, ts_opt) =
        calculate_reward(total, Some(&ep), Some(&pubsec1), None, &cfg).expect("calc failed");

    // В Phase2 всегда есть PoW-часть = base_reward / 2.
    assert!(
        reward >= cfg.base_reward / 2,
        "Phase2 reward must be at least pow_reward (base_reward/2), got {}",
        reward
    );
    // При наличии валидного EnergyProof функция возвращает Some(timestamp).
    assert!(ts_opt.is_some());
}

#[test]
fn phase3_reward_expected() {
    let cfg = EmissionConfig::default();

    let mut rng = StdRng::from_seed([13u8; 32]);
    let sk = SigningKey::random(&mut rng);
    let vk = VerifyingKey::from(&sk);
    let pubsec1 = vk.to_encoded_point(false).as_bytes().to_vec();

    let mut ep = EnergyProof {
        producer_id: "p4".to_string(),
        sequence: 3,
        kwh: 5.0,
        timestamp: Utc::now().timestamp_millis() as u128,
        ai_score: 0.95,
        ai_signature: vec![],
        proof_id: None,
    };

    let hash = ep.hash_for_signing();
    let sig: k256::ecdsa::Signature = sk.sign(&hash);
    ep.ai_signature = sig.to_der().as_bytes().to_vec();

    // В Phase3 total_supply должен быть >= PHASE2_SUPPLY_LIMIT.
    let total = PHASE2_SUPPLY_LIMIT + 1;
    let (reward, ts_opt) =
        calculate_reward(total, Some(&ep), Some(&pubsec1), None, &cfg).expect("calc failed");

    // Посчитаем ожидаемое значение по той же логике, что и в emission.rs (Phase3).
    // FP-константы:
    let kwh_fp = ep.kwh_fp();
    let score_fp = ep.ai_score_fp();
    let base_per_kwh_fp = (cfg.base_gvr_per_kwh * SCALE as f64) as u128;

    let mut reward_fp = 0u128;
    if kwh_fp != 0 && score_fp != 0 {
        reward_fp = kwh_fp
            .saturating_mul(base_per_kwh_fp)
            .saturating_mul(score_fp)
            / (SCALE.saturating_mul(SCALE));
    }

    let energy_reward_u = (reward_fp / SCALE) as u64;
    let expected_total = PHASE3_POW_REWARD.saturating_add(energy_reward_u);

    assert_eq!(
        reward, expected_total,
        "Phase3 reward mismatch: expected {} (pow {} + energy {}), got {}",
        expected_total, PHASE3_POW_REWARD, energy_reward_u, reward
    );
    assert!(ts_opt.is_some());
}