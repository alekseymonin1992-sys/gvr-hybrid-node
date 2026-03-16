#!/usr/bin/env python3
"""
validate_state.py

Validator for state.json snapshots produced by the GVR hybrid node.

Checks performed:
- JSON parse
- chain indexing and previous_hash linkage
- recomputes each block hash using same algorithm as Block::calculate_hash (SHA256(hex))
- verifies block.hash matches computed and checks leading zeros per-block difficulty (or global difficulty)
- verifies EnergyProof ECDSA signature (DER, secp256k1) if active_ai_pubkey present
- verifies total_supply equals sum of block rewards
- checks producer_state timestamps/seq non-decreasing
- prints summary and detailed errors

Usage:
    pip install -r requirements.txt
    python validate_state.py state.json

Outputs exit code 0 on OK, 1 on errors.

Note: This validator replicates canonical_bytes and hashing used in Rust code:
 - For transactions: transfers include sender, receiver, amount; RotateAIKey includes proposer and hex(new_ai_pubkey_sec1) and hex(signature)
 - If energy_proof is present, uses canonical_bytes: producer_id length (u16 BE) + bytes, sequence(u64 BE), kwh(f64 BE), timestamp(u128 BE), ai_score(f64 BE), proof_id length(u16 BE) + bytes (or 0).
"""

import sys
import json
import struct
import binascii
from typing import Optional

# For ECDSA secp256k1 verification
try:
    from ecdsa import VerifyingKey, util, der
    from ecdsa.curves import SECP256k1
except Exception as e:
    print("Please install 'ecdsa' package (pip install ecdsa). Error:", e)
    sys.exit(2)

import hashlib

def u16_be(x: int) -> bytes:
    return struct.pack(">H", x & 0xFFFF)

def u64_be(x: int) -> bytes:
    return struct.pack(">Q", x & 0xFFFFFFFFFFFFFFFF)

def u128_be(x: int) -> bytes:
    # pack as two u64 big-endian: high then low
    high = (x >> 64) & 0xFFFFFFFFFFFFFFFF
    low = x & 0xFFFFFFFFFFFFFFFF
    return struct.pack(">QQ", high, low)

def f64_be(x: float) -> bytes:
    return struct.pack(">d", x)

def canonical_energy_bytes(ep: dict) -> bytes:
    buf = bytearray()
    pid = ep.get("producer_id", "").encode("utf-8")
    buf.extend(u16_be(len(pid)))
    buf.extend(pid)
    seq = int(ep.get("sequence", 0))
    buf.extend(u64_be(seq))
    # kwh: float
    kwh = float(ep.get("kwh", 0.0))
    buf.extend(f64_be(kwh))
    # timestamp: we expect integer (u128)
    ts = int(ep.get("timestamp", 0))
    buf.extend(u128_be(ts))
    # ai_score: float
    ai = float(ep.get("ai_score", 0.0))
    buf.extend(f64_be(ai))
    # proof_id
    if ep.get("proof_id") is None:
        buf.extend(u16_be(0))
    else:
        pidb = str(ep.get("proof_id")).encode("utf-8")
        buf.extend(u16_be(len(pidb)))
        buf.extend(pidb)
    return bytes(buf)

def calculate_block_hash(block: dict) -> str:
    # Should mirror Block::calculate_hash in Rust
    h = hashlib.sha256()
    # index
    h.update(str(int(block.get("index", 0))).encode("utf-8"))
    # previous_hash
    h.update(str(block.get("previous_hash", "")).encode("utf-8"))
    # timestamp
    h.update(str(int(block.get("timestamp", 0))).encode("utf-8"))

    # transactions
    txs = block.get("transactions", [])
    for tx in txs:
        # detect type by keys
        if "sender" in tx and "receiver" in tx and "amount" in tx:
            # Transfer
            h.update(tx.get("sender", "").encode("utf-8"))
            h.update(tx.get("receiver", "").encode("utf-8"))
            h.update(str(int(tx.get("amount", 0))).encode("utf-8"))
        elif "new_ai_pubkey_sec1" in tx and "proposer" in tx and "signature" in tx:
            # RotateAIKey
            proposer = tx.get("proposer", "")
            newkey = tx.get("new_ai_pubkey_sec1", [])
            sig = tx.get("signature", [])
            # newkey and sig are arrays of bytes in JSON (or maybe hex strings)
            if isinstance(newkey, list):
                newhex = binascii.hexlify(bytes(newkey)).decode()
            elif isinstance(newkey, str):
                # if hex string
                try:
                    newhex = newkey
                except:
                    newhex = str(newkey)
            else:
                newhex = str(newkey)
            if isinstance(sig, list):
                sighex = binascii.hexlify(bytes(sig)).decode()
            elif isinstance(sig, str):
                sighex = sig
            else:
                sighex = str(sig)
            h.update(proposer.encode("utf-8"))
            h.update(newhex.encode("utf-8"))
            h.update(sighex.encode("utf-8"))
        else:
            # Unknown tx format: include JSON encoding to be safe
            h.update(json.dumps(tx, sort_keys=True).encode("utf-8"))

    # nonce and difficulty
    h.update(str(int(block.get("nonce", 0))).encode("utf-8"))
    h.update(str(int(block.get("difficulty", 0))).encode("utf-8"))

    # energy_proof
    ep = block.get("energy_proof", None)
    if ep is None:
        h.update(b"no_proof")
    else:
        cb = canonical_energy_bytes(ep)
        h.update(cb)

    # reward
    h.update(str(int(block.get("reward", 0))).encode("utf-8"))

    return h.hexdigest()

def verify_ecdsa_der_signature(ai_pubkey_sec1: bytes, signature_der: bytes, msg32: bytes) -> bool:
    """
    ai_pubkey_sec1: SEC1 encoded public key bytes (uncompressed usually starts with 0x04)
    signature_der: DER-encoded ECDSA signature bytes
    msg32: 32-byte message (sha256 hash)
    """
    try:
        # ecdsa library expects VerifyingKey.from_string with compressed/uncompressed?
        # Use from_string with 'uncompressed' - remove leading 0x04 if present, and get the raw x||y
        if ai_pubkey_sec1[0] == 4 and len(ai_pubkey_sec1) in (65, 65):
            raw = ai_pubkey_sec1[1:]
        elif len(ai_pubkey_sec1) in (64, 64):
            raw = ai_pubkey_sec1
        else:
            # try hex
            try:
                raw = binascii.unhexlify(ai_pubkey_sec1)
            except:
                raw = ai_pubkey_sec1

        vk = VerifyingKey.from_string(raw, curve=SECP256k1)
        # verify_digest expects the raw r||s signature in bytes or der decode
        # ecdsa VerifyingKey.verify(signature, data, hashfunc=...) expects the signature in DER by default if using verify_digest?
        # We'll decode DER to (r,s) and re-encode to raw signature for verify_digest
        try:
            # try verify_digest with DER: VerifyingKey.verify_digest(signature, digest)
            # But signature must be in raw (r||s). Let's decode DER:
            rs = der.remove_sequence(signature_der)
            # remove_sequence returns tuple (r_bytes, s_bytes) if using internal; but easiest: use util.sigdecode_der later.
        except Exception:
            pass

        # Using verify_digest with sigdecode_der:
        ok = vk.verify_digest(signature_der, msg32, sigdecode=util.sigdecode_der)
        return ok
    except Exception as e:
        # fallback: try verifying assuming signature_der is raw r||s
        try:
            ok = vk.verify_digest(signature_der, msg32, sigdecode=util.sigdecode_string)
            return ok
        except Exception:
            # final fallback failed
            return False

def check_state(state: dict, path: str):
    errors = []
    warnings = []
    chain = state.get("chain", [])
    global_difficulty = int(state.get("difficulty", 0))
    active_ai_pubkey = state.get("active_ai_pubkey", None)
    if active_ai_pubkey is not None:
        if isinstance(active_ai_pubkey, list):
            try:
                ai_pub_bytes = bytes(active_ai_pubkey)
            except Exception:
                ai_pub_bytes = None
        elif isinstance(active_ai_pubkey, str):
            try:
                ai_pub_bytes = binascii.unhexlify(active_ai_pubkey)
            except:
                ai_pub_bytes = active_ai_pubkey.encode("utf-8")
        else:
            ai_pub_bytes = None
    else:
        ai_pub_bytes = None

    # iterate blocks
    prev_hash_expected = None
    total_rewards = 0
    for i, blk in enumerate(chain):
        idx = int(blk.get("index", -1))
        if idx != i:
            errors.append(f"Block index mismatch at position {i}: block.index={idx}")
        prev_hash = blk.get("previous_hash", "")
        if i == 0:
            # genesis: previous_hash usually "0"
            if prev_hash not in ("0", "", None):
                warnings.append(f"Genesis previous_hash is not '0' (got {prev_hash})")
        else:
            # check linkage
            if prev_hash != (chain[i-1].get("hash") if i-1 < len(chain) else None):
                errors.append(f"Block {i} previous_hash does not match hash of block {i-1}")
        # recompute hash
        computed = calculate_block_hash(blk)
        given = blk.get("hash", "")
        if computed != given:
            errors.append(f"Block {i} hash mismatch: computed {computed} but block.hash={given}")
        # difficulty check: per-block if present else global
        block_diff = int(blk.get("difficulty", global_difficulty if global_difficulty is not None else 0))
        if block_diff > 0:
            # check leading zeros
            if not given.startswith("0" * block_diff):
                errors.append(f"Block {i} hash does not satisfy difficulty {block_diff}: hash={given}")
        # energy proof signature check if available and ai pubkey configured
        ep = blk.get("energy_proof", None)
        if ep:
            # verify signature present
            sig = ep.get("ai_signature", None)
            if sig is None:
                errors.append(f"Block {i} has energy_proof but missing ai_signature")
            else:
                # signature may be list of ints or hex string or bytes
                if isinstance(sig, list):
                    sig_bytes = bytes(sig)
                elif isinstance(sig, str):
                    try:
                        sig_bytes = binascii.unhexlify(sig)
                    except:
                        sig_bytes = sig.encode("utf-8")
                else:
                    sig_bytes = sig
                # compute hash_for_signing (sha256 of canonical bytes)
                cb = canonical_energy_bytes(ep)
                h = hashlib.sha256(cb).digest()
                if ai_pub_bytes:
                    ok = verify_ecdsa_der_signature(ai_pub_bytes, sig_bytes, h)
                    if not ok:
                        errors.append(f"Block {i} energy proof signature invalid")
                else:
                    warnings.append(f"Block {i} has energy_proof but state.active_ai_pubkey not configured; skipping signature check")
        # accumulate reward
        try:
            r = int(blk.get("reward", 0))
        except:
            r = 0
        total_rewards += r

    # compare total_supply
    state_total = int(state.get("total_supply", 0))
    if total_rewards != state_total:
        errors.append(f"total_supply mismatch: sum(rewards)={total_rewards} but state.total_supply={state_total}")

    # check producer_state monotonicity
    ps = state.get("producer_state", {})
    for pid, info in ps.items():
        last_seq = int(info.get("last_seq", 0))
        last_ts = int(info.get("last_ts", 0))
        # check that there's at least one block with matching producer and timestamp >= last_ts
        found = False
        for blk in chain:
            ep = blk.get("energy_proof")
            if ep and ep.get("producer_id") == pid:
                if int(ep.get("timestamp", 0)) >= last_ts and int(ep.get("sequence", 0)) >= last_seq:
                    found = True
                    break
        if not found:
            warnings.append(f"producer_state for {pid} points to seq={last_seq}, ts={last_ts} but no matching proof found in chain")

    return errors, warnings

def main(argv):
    if len(argv) < 2:
        print("Usage: python validate_state.py state.json")
        return 2
    path = argv[1]
    try:
        with open(path, "r", encoding="utf-8") as f:
            state = json.load(f)
    except Exception as e:
        print("Failed to read/parse JSON:", e)
        return 2

    errors, warnings = check_state(state, path)
    print("Validation result for", path)
    if errors:
        print("ERRORS:")
        for e in errors:
            print("  -", e)
    else:
        print("No errors found.")

    if warnings:
        print("WARNINGS:")
        for w in warnings:
            print("  -", w)
    else:
        print("No warnings.")

    if errors:
        return 1
    return 0

if __name__ == "__main__":
    rc = main(sys.argv)
    sys.exit(rc)