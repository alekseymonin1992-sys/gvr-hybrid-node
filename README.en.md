# GVR Hybrid Node

GVR is an experimental hybrid cryptocurrency written in Rust with a **three‑phase emission model**:

- **Phase1** – classic PoW (fixed block reward).
- **Phase2** – hybrid PoW + EnergyProof (AI‑signed proof of energy).
- **Phase3** – "green tail": main reward for energy, tiny PoW tail.

The project includes:

- full node (`gvr_hybrid_node`),
- RPC API (based on `axum`),
- P2P network,
- CLI RPC client (`client`),
- CLI wallet (`wallet`),
- a Windows distribution folder `dist` with `.exe` and `.bat` files.

Full economics description: see [ECONOMY.md](ECONOMY.md) (Russian).

---

## Build from source

Requires:

- Rust (stable) + Cargo  
  <https://www.rust-lang.org/tools/install>
- Git

Clone and build:

```bash
git clone https://github.com/alekseymonin1992-sys/gvr-hybrid-node.git
cd gvr-hybrid-node
cargo build --release