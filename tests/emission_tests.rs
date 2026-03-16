use chrono::Utc;
use gvr_hybrid_node::emission::{calculate_reward, EmissionConfig};
use gvr_hybrid_node::energy::EnergyProof;
use k256::ecdsa::{signature::Signer, SigningKey, VerifyingKey};
// use k256::elliptic_curve::sec1::ToEncodedPoint; // removed
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

    let total = 1_500_000u64; // Phase2
    let (reward, ts_opt) =
        calculate_reward(total, Some(&ep), Some(&pubsec1), None, &cfg).expect("calc failed");
    assert!(reward >= cfg.base_reward);
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

    let total = 4_000_000u64; // Phase3
    let (reward, ts_opt) =
        calculate_reward(total, Some(&ep), Some(&pubsec1), None, &cfg).expect("calc failed");

    let expected_f = ep.kwh * cfg.base_gvr_per_kwh * ep.ai_score;
    let expected = expected_f.floor() as u64;
    assert_eq!(reward, expected);
    assert!(ts_opt.is_some());
}