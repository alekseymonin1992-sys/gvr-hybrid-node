#!/usr/bin/env python3
# fix_and_sign_phase3.py
# Usage:
#  python fix_and_sign_phase3.py --in state_test_phase3.json --out state_test_phase3.fixed.json
# Optional: --privkey <hex32> to use your private key; otherwise script generates a new key.
# Requires: ecdsa (pip install ecdsa)

import argparse, json, hashlib, struct, time
from pathlib import Path
from ecdsa import SigningKey, SECP256k1
from ecdsa.util import sigencode_der

def u16_be(x): return int(x).to_bytes(2,'big')
def u64_be(x): return int(x).to_bytes(8,'big')
def u128_be(i):
    hi=(i>>64)&((1<<64)-1); lo=i&((1<<64)-1)
    return hi.to_bytes(8,'big')+lo.to_bytes(8,'big')
def f64_be(x): return struct.pack('>d', float(x))

def canonical_energy_bytes(ep):
    b=bytearray()
    pid=ep['producer_id'].encode('utf-8')
    b+=u16_be(len(pid)); b+=pid
    b+=u64_be(int(ep['sequence']))
    b+=f64_be(float(ep['kwh']))
    b+=u128_be(int(ep['timestamp']))
    b+=f64_be(float(ep['ai_score']))
    if ep.get('proof_id') is None:
        b+=u16_be(0)
    else:
        idb=str(ep['proof_id']).encode('utf-8'); b+=u16_be(len(idb)); b+=idb
    return bytes(b)

def calc_hash(block):
    h=hashlib.sha256()
    h.update(str(int(block['index'])).encode())
    h.update(str(block['previous_hash']).encode())
    h.update(str(int(block['timestamp'])).encode())
    for tx in block.get('transactions',[]):
        if 'sender' in tx and 'receiver' in tx and 'amount' in tx:
            h.update(tx['sender'].encode()); h.update(tx['receiver'].encode()); h.update(str(int(tx['amount'])).encode())
        elif 'new_ai_pubkey_sec1' in tx and 'proposer' in tx and 'signature' in tx:
            h.update(tx['proposer'].encode())
            nap = tx['new_ai_pubkey_sec1']; nap_bytes = bytes(nap) if isinstance(nap,list) else bytes.fromhex(nap)
            sig = tx['signature']; sig_bytes = bytes(sig) if isinstance(sig,list) else bytes.fromhex(sig)
            import binascii
            h.update(binascii.hexlify(nap_bytes)); h.update(binascii.hexlify(sig_bytes))
        else:
            h.update(json.dumps(tx, sort_keys=True, ensure_ascii=False).encode())
    h.update(str(int(block.get('nonce',0))).encode())
    h.update(str(int(block.get('difficulty',0))).encode())
    ep=block.get('energy_proof')
    if ep is None:
        h.update(b'no_proof')
    else:
        h.update(canonical_energy_bytes(ep))
    h.update(str(int(block.get('reward',0))).encode())
    return h.hexdigest()

def find_nonce(block, difficulty, max_tries=10_000_000):
    prefix='0'*difficulty
    nonce=int(block.get('nonce',0))
    for _ in range(max_tries):
        block['nonce']=nonce
        hh=calc_hash(block)
        if hh.startswith(prefix):
            return nonce, hh
        nonce+=1
    raise RuntimeError('nonce not found')

def sec1_from_sk(sk):
    vk=sk.get_verifying_key()
    return b'\x04'+vk.to_string()

def sign_ep(ep, sk):
    digest=hashlib.sha256(canonical_energy_bytes(ep)).digest()
    sig=sk.sign_digest_deterministic(digest, sigencode=sigencode_der)
    return list(sig)

def main():
    p=argparse.ArgumentParser()
    p.add_argument('--in', dest='infile', default='state_test_phase3.json')
    p.add_argument('--out', dest='outfile', default='state_test_phase3.fixed.json')
    p.add_argument('--privkey', dest='priv', default=None)
    args=p.parse_args()

    infile=Path(args.infile); outfile=Path(args.outfile)
    if not infile.exists(): print("Input missing", infile); return
    data=json.loads(infile.read_text(encoding='utf-8'))

    if args.priv:
        sk=SigningKey.from_string(bytes.fromhex(args.priv), curve=SECP256k1)
    else:
        sk=SigningKey.generate(curve=SECP256k1)

    pubsec1=sec1_from_sk(sk)
    data['active_ai_pubkey']=list(pubsec1)

    chain=data.get('chain',[])
    for i,blk in enumerate(chain):
        ep=blk.get('energy_proof')
        if ep is not None:
            # ensure timestamp/fields are ints/floats
            ep['timestamp']=int(ep['timestamp'])
            ep['kwh']=float(ep['kwh'])
            ep['ai_score']=float(ep['ai_score'])
            # sign (DER) and write
            siglist=sign_ep(ep, sk)
            ep['ai_signature']=siglist
            blk['energy_proof']=ep
        # now find nonce & hash (use block fields as-is)
        blk['previous_hash']=chain[i-1]['hash'] if i>0 else "0"
        # ensure ints
        blk['index']=int(blk['index']); blk['timestamp']=int(blk['timestamp']); blk['difficulty']=int(blk.get('difficulty',3))
        nonce, hh = find_nonce(blk, blk['difficulty'])
        blk['nonce']=nonce; blk['hash']=hh
        print(f"Block {i}: nonce={nonce} hash={hh}")

    # fix total_supply
    total=sum(int(b.get('reward',0)) for b in chain)
    data['total_supply']=total

    outfile.write_text(json.dumps(data, ensure_ascii=False, indent=2), encoding='utf-8')
    print("Wrote", outfile)

if __name__=='__main__':
    main()