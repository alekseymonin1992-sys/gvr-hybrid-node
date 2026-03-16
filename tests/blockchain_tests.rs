use chrono::Utc;
use k256::ecdsa::{signature::Signer, SigningKey, VerifyingKey};
// use k256::elliptic_curve::sec1::ToEncodedPoint;  // removed
use rand::rngs::StdRng;
use rand::SeedableRng;

use gvr_hybrid_node::block::Block;
use gvr_hybrid_node::blockchain::Blockchain;
use gvr_hybrid_node::energy::EnergyProof;

#[test]
fn add_block_accepts_valid() {
    let mut rng = StdRng::from_seed([21u8; 32]);
    let sk = SigningKey::random(&mut rng);
    let vk = VerifyingKey::from(&sk);
    let pubsec1 = vk.to_encoded_point(false).as_bytes().to_vec();

    let mut bc = Blockchain::new_with_genesis(Some(pubsec1.clone()));

    let mut ep = EnergyProof {
        producer_id: "pb1".to_string(),
        sequence: 1,
        kwh: 30.0,
        timestamp: Utc::now().timestamp_millis() as u128,
        ai_score: 0.9,
        ai_signature: vec![],
        proof_id: None,
    };
    let hash = ep.hash_for_signing();
    let sig: k256::ecdsa::Signature = sk.sign(&hash);
    ep.ai_signature = sig.to_der().as_bytes().to_vec();

    let idx = bc.chain.len() as u64;
    let blk = Block::new(idx, bc.last_hash(), vec![], bc.difficulty, Some(ep), 0);
    bc.add_block(blk);
    assert!(bc.total_supply > 0);
}

#[test]
fn replay_protection_blocks_replay() {
    let mut rng = StdRng::from_seed([23u8; 32]);
    let sk = SigningKey::random(&mut rng);
    let vk = VerifyingKey::from(&sk);
    let pubsec1 = vk.to_encoded_point(false).as_bytes().to_vec();

    let mut bc = Blockchain::new_with_genesis(Some(pubsec1.clone()));

    // Перемещаемся в Phase2, чтобы реплей‑защита сработала
    bc.total_supply = 1_500_000u64;

    let mut ep1 = EnergyProof {
        producer_id: "rp".to_string(),
        sequence: 5,
        kwh: 25.0,
        timestamp: Utc::now().timestamp_millis() as u128,
        ai_score: 0.9,
        ai_signature: vec![],
        proof_id: None,
    };
    let hash1 = ep1.hash_for_signing();
    let sig1: k256::ecdsa::Signature = sk.sign(&hash1);
    ep1.ai_signature = sig1.to_der().as_bytes().to_vec();

    let blk1 = Block::new(
        bc.chain.len() as u64,
        bc.last_hash(),
        vec![],
        bc.difficulty,
        Some(ep1.clone()),
        0,
    );
    bc.add_block(blk1);
    let prev_supply = bc.total_supply;

    // replay with same proof.sequence should be rejected
    let blk2 = Block::new(
        bc.chain.len() as u64,
        bc.last_hash(),
        vec![],
        bc.difficulty,
        Some(ep1),
        0,
    );
    bc.add_block(blk2);
    assert_eq!(bc.total_supply, prev_supply);
}