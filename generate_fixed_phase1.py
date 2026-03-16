#!/usr/bin/env python3
# Генератор корректного state_test_phase1.json (genesis + 3 блока, PoW-only)
# Требует: Python 3 (не нужны дополнительные пакеты)

import json
import hashlib
import time
from pathlib import Path
import struct

def u128_be(i: int) -> bytes:
    hi = (i >> 64) & ((1 << 64) - 1)
    lo = i & ((1 << 64) - 1)
    return hi.to_bytes(8,'big') + lo.to_bytes(8,'big')

def canonical_energy_bytes(ep):
    b = bytearray()
    pid = ep['producer_id'].encode('utf-8')
    b += len(pid).to_bytes(2,'big')
    b += pid
    b += int(ep['sequence']).to_bytes(8,'big')
    b += struct.pack('>d', float(ep['kwh']))
    b += u128_be(int(ep['timestamp']))
    b += struct.pack('>d', float(ep['ai_score']))
    if ep.get('proof_id') is None:
        b += (0).to_bytes(2,'big')
    else:
        idb = str(ep['proof_id']).encode('utf-8')
        b += len(idb).to_bytes(2,'big') + idb
    return bytes(b)

def calc_hash(block):
    h = hashlib.sha256()
    h.update(str(int(block['index'])).encode())
    h.update(str(block['previous_hash']).encode())
    h.update(str(int(block['timestamp'])).encode())
    for tx in block.get('transactions', []):
        # Transactions not used here
        if 'sender' in tx and 'receiver' in tx and 'amount' in tx:
            h.update(tx['sender'].encode())
            h.update(tx['receiver'].encode())
            h.update(str(int(tx['amount'])).encode())
        else:
            h.update(json.dumps(tx, sort_keys=True).encode())
    h.update(str(int(block.get('nonce',0))).encode())
    h.update(str(int(block.get('difficulty',0))).encode())
    ep = block.get('energy_proof')
    if ep is None:
        h.update(b'no_proof')
    else:
        h.update(canonical_energy_bytes(ep))
    h.update(str(int(block.get('reward',0))).encode())
    return h.hexdigest()

def find_nonce_for_block(block, difficulty, max_tries=10_000_000):
    target = '0' * difficulty
    nonce = int(block.get('nonce', 0))
    for i in range(max_tries):
        block['nonce'] = nonce
        hh = calc_hash(block)
        if hh.startswith(target):
            return nonce, hh
        nonce += 1
    raise RuntimeError('nonce not found within max_tries')

def main():
    out = {}
    chain = []
    now_ms = int(time.time() * 1000)

    # Create genesis and find nonce for difficulty 2
    difficulty_genesis = 2
    g = {
        "index": 0,
        "previous_hash": "0",
        "timestamp": now_ms,
        "transactions": [],
        "nonce": 0,
        "difficulty": difficulty_genesis,
        "energy_proof": None,
        "reward": 0,
    }
    n, hh = find_nonce_for_block(g, difficulty_genesis)
    g['nonce'] = n
    g['hash'] = hh
    chain.append(g)
    print("Genesis nonce", n, "hash", hh)

    # Create 3 PoW blocks (difficulty 3)
    difficulty = 3
    rewards = [500, 500, 0]
    for i in range(1, 4):
        blk = {
            "index": i,
            "previous_hash": chain[-1]['hash'],
            "timestamp": now_ms + i*1000,
            "transactions": [],
            "nonce": 0,
            "difficulty": difficulty,
            "energy_proof": None,
            "reward": rewards[i-1],
        }
        nonce, hh = find_nonce_for_block(blk, difficulty)
        blk['nonce'] = nonce
        blk['hash'] = hh
        chain.append(blk)
        print(f"Block {i}: nonce={nonce} hash={hh}")

    total_supply = sum(b['reward'] for b in chain)
    out['chain'] = chain
    out['difficulty'] = difficulty
    out['total_supply'] = total_supply
    out['active_ai_pubkey'] = None
    out['producer_state'] = {}

    p = Path('state_test_phase1.json')
    p.write_text(json.dumps(out, ensure_ascii=False, indent=2), encoding='utf-8')
    print("Wrote", p.resolve(), "total_supply=", total_supply)

if __name__ == '__main__':
    main()