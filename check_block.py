#!/usr/bin/env python3
# check_block.py
# Usage: python check_block.py state_test_phase3.json <block_index>
# Prints stored hash, python-calc hash, canonical energy bytes and signature info for diagnosis.

import sys
import json
import hashlib
import struct
import binascii

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
        b += u16_be(len(idb)); b += idb
    return bytes(b)

def calc_hash(block: dict) -> str:
    h = hashlib.sha256()
    h.update(str(int(block['index'])).encode())
    h.update(str(block['previous_hash']).encode())
    h.update(str(int(block['timestamp'])).encode())
    # transactions: simplified as in generator
    for tx in block.get('transactions', []):
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

def main():
    if len(sys.argv) < 3:
        print("Usage: python check_block.py <state.json> <block_index>")
        return
    path = sys.argv[1]
    idx = int(sys.argv[2])
    with open(path, 'r', encoding='utf-8') as f:
        top = json.load(f)

    chain = top.get('chain', [])
    if idx < 0 or idx >= len(chain):
        print("block index out of range, chain length =", len(chain))
        return

    block = chain[idx]
    print("Block index:", idx)
    print("Stored hash:", block.get('hash'))
    calc = calc_hash(block)
    print("Computed hash:", calc)
    print("Nonce:", block.get('nonce'))
    print("Difficulty:", block.get('difficulty'))
    print("Previous_hash:", block.get('previous_hash'))
    print("Timestamp:", block.get('timestamp'))
    ep = block.get('energy_proof')
    if ep is None:
        print("No energy_proof (no_proof used in hash).")
    else:
        print("--- EnergyProof ---")
        print("producer_id:", ep.get('producer_id'))
        print("sequence:", ep.get('sequence'))
        print("kwh:", ep.get('kwh'))
        print("timestamp:", ep.get('timestamp'))
        print("ai_score:", ep.get('ai_score'))
        sig = ep.get('ai_signature', [])
        print("ai_signature length:", len(sig))
        if isinstance(sig, list):
            print("ai_signature (first 20 bytes):", sig[:20])
        else:
            print("ai_signature (hex, first 40 chars):", binascii.hexlify(sig)[:40])
        cb = canonical_energy_bytes(ep)
        print("canonical_energy_bytes (len):", len(cb))
        print("canonical_energy_bytes (hex start):", binascii.hexlify(cb)[:160].decode())
        print("sha256(canonical_bytes):", hashlib.sha256(cb).hexdigest())
    # print raw bytes used in block hash construction for deeper diff
    raw = bytearray()
    raw.extend(str(int(block['index'])).encode())
    raw.extend(str(block['previous_hash']).encode())
    raw.extend(str(int(block['timestamp'])).encode())
    for tx in block.get('transactions', []):
        raw.extend(json.dumps(tx, sort_keys=True, ensure_ascii=False).encode())
    raw.extend(str(int(block.get('nonce', 0))).encode())
    raw.extend(str(int(block.get('difficulty', 0))).encode())
    if ep is None:
        raw.extend(b'no_proof')
    else:
        raw.extend(canonical_energy_bytes(ep))
    raw.extend(str(int(block.get('reward', 0))).encode())
    print("Raw concat bytes (hex start):", binascii.hexlify(bytes(raw))[:200].decode())
    print("SHA256 of raw bytes (should equal computed hash):", hashlib.sha256(bytes(raw)).hexdigest())

if __name__ == '__main__':
    main()