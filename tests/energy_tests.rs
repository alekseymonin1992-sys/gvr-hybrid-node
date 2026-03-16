use chrono::Utc;
use gvr_hybrid_node::energy::EnergyProof;
use k256::ecdsa::{signature::Signer, SigningKey, VerifyingKey};
// use k256::elliptic_curve::sec1::ToEncodedPoint; // removed
use rand::rngs::StdRng;
use rand::SeedableRng;

#[test]
fn signature_verify_ok() {
    let mut rng = StdRng::from_seed([42u8; 32]);
    let sk = SigningKey::random(&mut rng);
    let vk = VerifyingKey::from(&sk);
    let pubsec1 = vk.to_encoded_point(false).as_bytes().to_vec();

    let mut ep = EnergyProof {
        producer_id: "test-producer".to_string(),
        sequence: 1,
        kwh: 12.34,
        timestamp: Utc::now().timestamp_millis() as u128,
        ai_score: 0.9,
        ai_signature: vec![],
        proof_id: None,
    };

    let hash = ep.hash_for_signing();
    let sig: k256::ecdsa::Signature = sk.sign(&hash);
    ep.ai_signature = sig.to_der().as_bytes().to_vec();

    let ok = ep.verify_signature(&pubsec1).expect("verify call failed");
    assert!(ok, "signature should verify");
}

#[test]
fn signature_verify_fail_wrong_key() {
    let mut rng = StdRng::from_seed([7u8; 32]);
    let sk1 = SigningKey::random(&mut rng);
    let vk1 = VerifyingKey::from(&sk1);
    let pub1 = vk1.to_encoded_point(false).as_bytes().to_vec();

    let mut rng2 = StdRng::from_seed([9u8; 32]);
    let sk2 = SigningKey::random(&mut rng2);

    let mut ep = EnergyProof {
        producer_id: "test-producer-2".to_string(),
        sequence: 1,
        kwh: 5.0,
        timestamp: Utc::now().timestamp_millis() as u128,
        ai_score: 0.85,
        ai_signature: vec![],
        proof_id: None,
    };

    let hash = ep.hash_for_signing();
    let sig: k256::ecdsa::Signature = sk2.sign(&hash);
    ep.ai_signature = sig.to_der().as_bytes().to_vec();

    let ok = ep.verify_signature(&pub1).expect("verify call failed");
    assert!(!ok, "signature with wrong key must not verify");
}