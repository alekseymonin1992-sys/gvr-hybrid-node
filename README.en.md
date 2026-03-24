```markdown
# GVR Hybrid Node

GVR is an experimental hybrid cryptocurrency written in Rust with a three‑phase monetary policy:

- **Phase1** – classic Proof‑of‑Work (fixed block reward).
- **Phase2** – hybrid PoW + EnergyProof (AI‑validated energy).
- **Phase3** – “green tail”: main reward comes from energy, with a small PoW tail.

The project includes:

- a full node (`gvr_hybrid_node`),
- HTTP RPC API (based on `axum`),
- P2P network,
- CLI RPC client (`gvr-client`),
- CLI wallet (`gvr-wallet`),
- P2P client (`gvr-p2p-client`),
- utilities for AI keys and EnergyProof (`gvr-ai-keygen`, `gvr-energy-client`, `gvr-ai-rotate`),
- Windows distribution (`dist` folder with `.exe` and `.bat`).

- Full description of the **economy**: [ECONOMY.md](ECONOMY.md).  
- Technical **protocol specification**: [PROTO.md](PROTO.md).

[Русский README](README.md)

---

## 1. Features

- Hybrid consensus: PoW + EnergyProof.
- Three‑phase emission with a hard cap of `21 000 000 GVR`.
- Signed transfers (`SignedTransfer`) with fees and nonces.
- Built‑in P2P protocol with locator‑based sync and peer banning.
- HTTP RPC API for integration with apps and frontends.
- Separate CLI wallet and clients for transactions and EnergyProof.

---

## 2. Build from source

You need Rust (stable) and Cargo.

```bash
git clone https://github.com/alekseymonin1992-sys/gvr-hybrid-node.git
cd gvr-hybrid-node
cargo build --release
```

Binaries (in `target/release`):

- `gvr-node` – main node,
- `gvr-client` – simple RPC client,
- `gvr-wallet` – CLI wallet,
- `gvr-p2p-client` – P2P client,
- `gvr-ai-keygen` – AI key generator,
- `gvr-energy-client` – EnergyProof client (RPC),
- `gvr-ai-rotate` – AI public key rotation tool.

---

## 3. Quick start

### 3.1. Generate AI key

The AI key is used to sign and verify EnergyProof.

```bash
target/release/gvr-ai-keygen
```

This creates:

- `ai_key.bin` – private AI key (k256 ECDSA),
- `ai_pubkey.bin` – public AI key (SEC1, uncompressed).

### 3.2. Run a node

Example: single node with mining and RPC enabled:

```bash
target/release/gvr-node \
  --p2p_addr 127.0.0.1:4000 \
  --rpc_addr 127.0.0.1:8080 \
  --coinbase_addr alice \
  --ai-key-file ai_key.bin
```

- `--coinbase_addr` – address that receives block rewards and transaction fees.
- On graceful shutdown the node stores a state snapshot in `state.json`.

### 3.3. Create a wallet

```bash
target/release/gvr-wallet new --name alice
```

This prints:

- path to the private key file (`wallets/alice.key`),
- address in the state (string `alice`),
- public key in SEC1 format (hex).

Show wallet details:

```bash
target/release/gvr-wallet show --name alice
```

### 3.4. Send a transaction

Send 10 GVR from wallet `alice` to address `bob`:

```bash
target/release/gvr-wallet send \
  --rpc 127.0.0.1:8080 \
  --from_wallet alice \
  --to bob \
  --amount 10 \
  --fee 1
```

The wallet:

- fetches the current `nonce` via `/nonce?addr=...`,
- signs the transfer with the wallet’s private key,
- posts a DTO to the node’s `/tx` endpoint.

The node validates the signature and fee, adds the tx to the mempool and then into a block.

### 3.5. Submit EnergyProof

Submit energy production data to the node:

```bash
target/release/gvr-energy-client \
  --rpc 127.0.0.1:8080 \
  --producer_id my_station_1 \
  --sequence 1 \
  --kwh 123.45 \
  --ai_score 0.92
```

The client:

- constructs an `EnergyProof`,
- signs it with the AI private key (`ai_key.bin`),
- verifies the signature locally,
- sends a DTO to `/energy_proof`.

The node:

- validates the fields and signature,
- if valid, stores the proof and uses it when computing the reward for subsequent blocks (Phase2/Phase3).

---

## 4. Multi‑node network example

### 4.1. Node #1

```bash
target/release/gvr-node \
  --p2p_addr 127.0.0.1:4000 \
  --rpc_addr 127.0.0.1:8080 \
  --coinbase_addr alice \
  --ai-key-file ai_key.bin
```

### 4.2. Node #2

```bash
target/release/gvr-node \
  --p2p_addr 127.0.0.1:4001 \
  --rpc_addr 127.0.0.1:8081 \
  --coinbase_addr bob \
  --ai-pubkey-file ai_pubkey.bin \
  --peers 127.0.0.1:4000
```

Sync Node #2 with Node #1 via RPC:

```bash
curl "http://127.0.0.1:8081/sync?peer=127.0.0.1:4000"
```

### 4.3. Status and diagnostics

Node status:

```bash
curl http://127.0.0.1:8080/status
```

Balance of an address:

```bash
curl "http://127.0.0.1:8080/balance?addr=alice"
```

Nonce of an address:

```bash
curl "http://127.0.0.1:8080/nonce?addr=alice"
```

Known peers:

```bash
curl http://127.0.0.1:8080/peers
```

---

## 5. Project status

The project is currently **Draft / Experimental**:

- protocol rules, emission parameters and the economic model may change;
- breaking changes to block format and RPC API are possible.

For canonical and up‑to‑date details:

- protocol: [PROTO.md](PROTO.md),
- economy: 