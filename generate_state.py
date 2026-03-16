#!/usr/bin/env python3
# generate_state.py
# Generate state JSON compatible with Rust node canonicalization and hashing.
# - canonical_energy_bytes matches EnergyProof::canonical_bytes
# - Block hash calculation matches Block::calculate_hash
# - can sign EnergyProofs with a provided secp256k1 private key (DER signatures)
# - finds nonce to satisfy difficulty (leading zero hex chars)
#
# Usage:
#  python generate_state.py            # generates state_test_phase2.json (example)
#  python generate_state.py --out state.json --privkey <hex32> --count 10 --difficulty 3
#
# Requires: ecdsa (pip install ecdsa)
#
# Note: by default this script produces a small example chain; tune --count/--target as needed.

import argparse
import json
import hashlib
import struct
import time
from pathlib import Path
from ecdsa import SigningKey, SECP256k1
from ecdsa.util import sigencode_der

def u16_be(x: int) -> bytes:
    return int(x).to_bytes(2, 'big')

def u64_be(x: int) -> bytes:
    return int(x).to_bytes(8, 'big')

def u128_be(i: int) -> bytes:
    hi = (i >> 64) & ((1 << 64) - 1)
    lo = i & ((1 << 64) - 1)
    return hi.to_bytes(8, 'big') + lo.to_bytes(8, 'big')

def f64_be(x: float) -> bytes:
    return struct.pack('>d', float(x))

def canonical_energy_bytes(ep: dict) -> bytes:
    # producer_id len (u16) + bytes
    b = bytearray()
    pid_bytes = ep['producer_id'].encode('utf-8')
    b += u16_be(len(pid_bytes))
    b += pid_bytes
    b += u64_be(int(ep['sequence']))
    b += f64_be(float(ep['kwh']))
    b += u128_be(int(ep['timestamp']))
    b += f64_be(float(ep['ai_score']))
    if ep.get('proof_id') is None:
        b += u16_be(0)
    else:
        idb = str(ep['proof_id']).encode('utf-8')
        b += u16_be(len(idb))
        b += idb
    return bytes(b)

def hash_for_signing(ep: dict) -> bytes:
    return hashlib.sha256(canonical_energy_bytes(ep)).digest()

def calc_block_hash(block: dict) -> str:
    h = hashlib.sha256()
    h.update(str(int(block['index'])).encode())
    h.update(str(block['previous_hash']).encode())
    h.update(str(int(block['timestamp'])).encode())

    for tx in block.get('transactions', []):
        # mimic Rust Transaction hashing
        if isinstance(tx, dict) and 'sender' in tx and 'receiver' in tx and 'amount' in tx:
            h.update(tx['sender'].encode())
            h.update(tx['receiver'].encode())
            h.update(str(int(tx['amount'])).encode())
        elif isinstance(tx, dict) and 'new_ai_pubkey_sec1' in tx and 'proposer' in tx and 'signature' in tx:
            h.update(tx['proposer'].encode())
            nap = tx['new_ai_pubkey_sec1']
            nap_bytes = bytes(nap) if isinstance(nap, list) else bytes.fromhex(nap)
            sig = tx['signature']
            sig_bytes = bytes(sig) if isinstance(sig, list) else bytes.fromhex(sig)
            import binascii
            h.update(binascii.hexlify(nap_bytes))
            h.update(binascii.hexlify(sig_bytes))
        else:
            h.update(json.dumps(tx, sort_keys=True, ensure_ascii=False).encode())

    h.update(str(int(block.get('nonce', 0))).encode())
    h.update(str(int(block.get('difficulty', 0))).encode())

    ep = block.get('energy_proof')
    if ep is None:
        h.update(b'no_proof')
    else:
        h.update(canonical_energy_bytes(ep))

    h.update(str(int(block.get('reward', 0))).encode())
    return h.hexdigest()

def find_nonce(block: dict, difficulty: int, max_tries: int = 5_000_000):
    prefix = '0' * difficulty
    nonce = int(block.get('nonce', 0))
    for _ in range(max_tries):
        block['nonce'] = nonce
        hh = calc_block_hash(block)
        if hh.startswith(prefix):
            return nonce, hh
        nonce += 1
    raise RuntimeError("nonce not found within max_tries")

def sec1_from_sk(sk: SigningKey) -> bytes:
    vk = sk.get_verifying_key()
    return b'\x04' + vk.to_string()

def sign_ep(ep: dict, sk: SigningKey) -> list:
    digest = hash_for_signing(ep)
    sig = sk.sign_digest_deterministic(digest, sigencode=sigencode_der)
    return list(sig)

def build_chain(count: int, difficulty: int, sk: SigningKey, start_ts: int, target_total: int = None):
    chain = []
    now = start_ts

    # genesis: find nonce for difficulty 2
    genesis = {
        "index": 0,
        "previous_hash": "0",
        "timestamp": now,
        "transactions": [],
        "nonce": 0,
        "difficulty": 2,
        "energy_proof": None,
        "reward": 0
    }
    gn, gh = find_nonce(genesis, 2)
    genesis['nonce'] = gn
    genesis['hash'] = gh
    chain.append(genesis)

    total = 0
    for i in range(1, count+1):
        now = start_ts + i * 1000
        ep = {
            "producer_id": f"producer-{1000 + (i % 100)}",
            "sequence": 1,
            "kwh": float(50 + (i * 3) % 200),  # synthetic energy
            "timestamp": now,
            "ai_score": 0.9,
            "ai_signature": [],
            "proof_id": None
        }
        if sk is not None:
            ep['ai_signature'] = sign_ep(ep, sk)

        blk = {
            "index": i,
            "previous_hash": chain[-1]['hash'],
            "timestamp": now,
            "transactions": [],
            "nonce": 0,
            "difficulty": difficulty,
            "energy_proof": ep if sk is not None else ep,  # include proof even if unsigned
            "reward": 0
        }

        n, hh = find_nonce(blk, difficulty)
        blk['nonce'] = n
        blk['hash'] = hh

        # set a synthetic reward (not exact emission formula) to allow target_total if requested
        reward = int((ep['kwh'] * ep['ai_score']) * 10)
        blk['reward'] = reward
        total += reward

        chain.append(blk)

        if target_total is not None and total >= target_total:
            break

    return chain, total

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--out', default='state_test_phase2.json')
    parser.add_argument('--count', type=int, default=10, help='number of mined blocks after genesis')
    parser.add_argument('--difficulty', type=int, default=3)
    parser.add_argument('--privkey', default=None, help='hex 32-byte private key to sign proofs')
    parser.add_argument('--target', type=int, default=None, help='target total supply (approx)')
    args = parser.parse_args()

    if args.privkey:
        sk = SigningKey.from_string(bytes.fromhex(args.privkey), curve=SECP256k1)
    else:
        sk = None

    start_ts = int(time.time() * 1000)

    chain, total = build_chain(args.count, args.difficulty, sk, start_ts, target_total=args.target)

    top = {
        "chain": chain,
        "difficulty": args.difficulty,
        "total_supply": total,
        "active_ai_pubkey": list(sec1_from_sk(sk)) if sk is not None else None,
        "producer_state": {f"producer-{1000+(i%100)}": {"last_seq": 1, "last_ts": chain[i+1]['timestamp']} for i in range(len(chain)-1)}
    }

    out = Path(args.out)
    out.write_text(json.dumps(top, ensure_ascii=False, indent=2), encoding='utf-8')
    print("Wrote", out.resolve(), "total_supply=", total)