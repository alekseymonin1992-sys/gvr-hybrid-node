# GVR Hybrid Node

GVR — экспериментальная гибридная криптовалюта на Rust с трёхфазной экономикой:

- **Phase1** — классический PoW (фиксированная награда).
- **Phase2** — гибрид PoW + EnergyProof (AI-подтверждённая энергия).
- **Phase3** — "зелёный хвост": основная награда за энергию, маленький PoW‑хвост.

Проект включает:

- полноценную ноду (`gvr_hybrid_node`),
- RPC API (`axum`),
- P2P-сеть,
- CLI-клиент (`client`),
- CLI-кошелёк (`wallet`),
- сборку для Windows в виде папки `dist` с `.exe` и `.bat`.

Полное описание экономики: см. [ECONOMY.md](ECONOMY.md).

---

## Сборка из исходников

Требуется установленный Rust (stable) и Cargo.

```bash
git clone https://github.com/alekseymonin1992-sys/gvr-hybrid-node.git
cd gvr-hybrid-node
cargo build --release