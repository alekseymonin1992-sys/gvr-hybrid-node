use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::fs;
use std::path::PathBuf;

use axum::{
    extract::{DefaultBodyLimit, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};

use crate::accounts::SignedTransfer;
use crate::blockchain::Blockchain;
use crate::mempool::Mempool;
use crate::energy::EnergyProof;
use crate::p2p::{
    ban_peer, get_peer_states_arc, get_peers_arc, request_blocks_from_locators, request_status,
    unban_peer, PeerState,
};
use crate::transaction::Transaction;

#[derive(Clone)]
struct RpcState {
    blockchain: Arc<Mutex<Blockchain>>,
    mempool: Arc<Mutex<Mempool>>,
    // последний внешний EnergyProof, присланный по RPC
    last_energy_proof: Arc<Mutex<Option<EnergyProof>>>,
    // для простого rate-limit по /energy_proof
    last_ep_ts: Arc<Mutex<u128>>,
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u128
}

/// DTO для приёма SignedTransfer по HTTP.
#[derive(Debug, Deserialize)]
struct SignedTransferDto {
    from: String,
    to: String,
    amount: u64,
    nonce: u64,
    pubkey_sec1: Vec<u8>,
    signature: Vec<u8>,
    fee: u64,
}

/// DTO для приёма EnergyProof по HTTP.
#[derive(Debug, Deserialize)]
struct EnergyProofDto {
    producer_id: String,
    sequence: u64,
    kwh: f64,
    timestamp: u128,
    ai_score: f64,
    ai_signature: Vec<u8>,
    proof_id: Option<String>,
}

/// DTO для вывода одного пира через /peers.
#[derive(Debug, Serialize)]
struct PeerInfo {
    addr: String,
    last_error_ts: u128,
    error_count: u32,
    banned_until: u128,
    last_contact_ts: u128,
}

/// DTO для /status
#[derive(Debug, Serialize)]
struct StatusResponse {
    height: u64,
    tip: String,
    difficulty: u32,
    total_supply: u64,
    alice_balance: u64,
    bob_balance: u64,
    alice_nonce: u64,
    phase: String,
}

#[derive(Debug, Serialize)]
struct P2pDebugInfo {
    peers: Vec<PeerInfo>,
    total_peers: usize,
    banned_peers: usize,
}

/// DTO для /mempool
#[derive(Debug, Serialize)]
struct MempoolTx {
    hash: String,
    kind: String,
}

#[derive(Debug, Serialize)]
struct MempoolResponse {
    txs: Vec<MempoolTx>,
}

/// Запуск HTTP RPC-сервера на axum.
pub fn start_rpc(
    bind_addr: &str,
    blockchain: Arc<Mutex<Blockchain>>,
    mempool: Arc<Mutex<Mempool>>,
    last_energy_proof: Arc<Mutex<Option<EnergyProof>>>,
) {
    let addr_s = bind_addr.to_string();
    println!("RPC (axum) starting on http://{}", addr_s);

    let state = RpcState {
        blockchain,
        mempool,
        last_energy_proof,
        last_ep_ts: Arc::new(Mutex::new(0)),
    };

    thread::spawn(move || {
        let rt = match tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => {
                eprintln!("RPC: failed to build tokio runtime: {}", e);
                return;
            }
        };

        rt.block_on(async move {
            let app = Router::new()
                .route("/tx", post(handle_tx))
                .route("/status", get(handle_status))
                .route("/peers", get(handle_peers))
                .route("/sync", get(handle_sync).post(handle_sync))
                .route("/ban", post(handle_ban))
                .route("/unban", post(handle_unban))
                .route("/balance", get(handle_balance))
                .route("/nonce", get(handle_nonce))
                .route("/p2p_debug", get(handle_p2p_debug))
                .route("/ui", get(handle_ui))
                .route("/energy_proof", post(handle_energy_proof))
                .route("/mempool", get(handle_mempool))
                // Ограничиваем размер тела запросов, чтобы не положить ноду огромными JSON'ами
                .layer(DefaultBodyLimit::max(16 * 1024)) // 16 KB
                .with_state(state);

            let addr: SocketAddr = match addr_s.parse() {
                Ok(a) => a,
                Err(e) => {
                    eprintln!(
                        "RPC: invalid bind addr '{}': {} (expected ip:port)",
                        addr_s, e
                    );
                    return;
                }
            };

            println!("RPC: binding to {}", addr);

            let listener = match tokio::net::TcpListener::bind(addr).await {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("RPC: bind failed on {}: {}", addr, e);
                    return;
                }
            };

            println!("RPC: listening on {}", addr);

            if let Err(e) = axum::serve(listener, app).await {
                eprintln!("RPC server error: {}", e);
            }
        });
    });
}

/// GET /ui — простой HTML UI (static/index.html)
async fn handle_ui() -> Response {
    let path: PathBuf = PathBuf::from("static").join("index.html");

    match fs::read_to_string(&path) {
        Ok(contents) => {
            (
                StatusCode::OK,
                (
                    [("Content-Type", "text/html; charset=utf-8")],
                    contents,
                ),
            )
                .into_response()
        }
        Err(e) => {
            let body = format!("Failed to read UI file {}: {}", path.display(), e);
            (StatusCode::INTERNAL_SERVER_ERROR, body).into_response()
        }
    }
}

/// POST /tx
async fn handle_tx(
    State(st): State<RpcState>,
    Json(dto): Json<SignedTransferDto>,
) -> Response {
    let stx = SignedTransfer {
        from: dto.from,
        to: dto.to,
        amount: dto.amount,
        fee: dto.fee,
        nonce: dto.nonce,
        pubkey_sec1: dto.pubkey_sec1,
        signature: dto.signature,
    };

    let tx = Transaction::signed(stx);

    let tx_hash = {
        let mut mp = st.mempool.lock().unwrap();
        mp.add_tx(tx)
    };

    if tx_hash.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            "tx rejected by mempool".to_string(),
        )
            .into_response();
    }

    println!("RPC: accepted tx via /tx, hash={}", tx_hash);

    let resp = serde_json::json!({ "tx_hash": tx_hash });
    (StatusCode::OK, Json(resp)).into_response()
}

/// POST /energy_proof
///
/// Принимает EnergyProofDto, проверяет поля и подпись по active_ai_pubkey,
/// и если всё ок, сохраняет его в RpcState.last_energy_proof.
/// Есть простой rate-limit: не чаще, чем раз в 1 секунду.
async fn handle_energy_proof(
    State(st): State<RpcState>,
    Json(dto): Json<EnergyProofDto>,
) -> Response {
    // Rate-limit: не чаще, чем раз в 1000 мс
    {
        let now = now_ms();
        let mut last_ts = st.last_ep_ts.lock().unwrap();
        if now.saturating_sub(*last_ts) < 1000 {
            let msg = "too many EnergyProof submissions, slow down".to_string();
            println!("RPC /energy_proof: {}", msg);
            return (StatusCode::TOO_MANY_REQUESTS, msg).into_response();
        }
        *last_ts = now;
    }

    // Собираем EnergyProof из DTO
    let proof = EnergyProof {
        producer_id: dto.producer_id,
        sequence: dto.sequence,
        kwh: dto.kwh,
        timestamp: dto.timestamp,
        ai_score: dto.ai_score,
        ai_signature: dto.ai_signature,
        proof_id: dto.proof_id,
    };

    // Проверка полей (kwh > 0, ai_score >= MIN_AI_SCORE, timestamp не в далёком будущем)
    if let Err(e) = proof.validate_fields(crate::constants::MIN_AI_SCORE) {
        let msg = format!("invalid EnergyProof fields: {}", e);
        println!("RPC /energy_proof: {}", msg);
        return (StatusCode::BAD_REQUEST, msg).into_response();
    }

    // Нужен active_ai_pubkey в блокчейне для проверки подписи.
    let ai_pub_opt = {
        let bc = st.blockchain.lock().unwrap();
        bc.active_ai_pubkey.clone()
    };

    let ai_pub = match ai_pub_opt {
        Some(p) => p,
        None => {
            let msg = "AI public key not configured on this node".to_string();
            println!("RPC /energy_proof: {}", msg);
            return (StatusCode::BAD_REQUEST, msg).into_response();
        }
    };

    // Проверяем подпись
    match proof.verify_signature(&ai_pub) {
        Ok(true) => {
            // Сохраняем последний валидный proof
            {
                let mut slot = st.last_energy_proof.lock().unwrap();
                *slot = Some(proof.clone());
            }
            println!(
                "RPC /energy_proof: accepted proof producer_id={} seq={} kwh={} ai_score={}",
                proof.producer_id, proof.sequence, proof.kwh, proof.ai_score
            );
            let body = serde_json::json!({
                "status": "ok",
                "producer_id": proof.producer_id,
                "sequence": proof.sequence
            });
            (StatusCode::OK, Json(body)).into_response()
        }
        Ok(false) => {
            let msg = "AI signature invalid".to_string();
            println!("RPC /energy_proof: {}", msg);
            (StatusCode::BAD_REQUEST, msg).into_response()
        }
        Err(e) => {
            let msg = format!("verify error: {}", e);
            println!("RPC /energy_proof: {}", msg);
            (StatusCode::BAD_REQUEST, msg).into_response()
        }
    }
}

/// GET /status
async fn handle_status(State(st): State<RpcState>) -> Response {
    let bc = st.blockchain.lock().unwrap();
    let height = bc.chain.len() as u64;
    let tip = bc.tip_hash.clone();
    let total_supply = bc.total_supply;
    let diff = bc.difficulty;
    let bal_alice = bc.state.balance_of("alice");
    let bal_bob = bc.state.balance_of("bob");
    let nonce_alice = bc.state.nonce_of("alice");

    let phase = crate::emission::current_phase(total_supply);
    let phase_str = match phase {
        crate::emission::EmissionPhase::Phase1 => "Phase1",
        crate::emission::EmissionPhase::Phase2 => "Phase2",
        crate::emission::EmissionPhase::Phase3 => "Phase3",
    }
    .to_string();

    let resp = StatusResponse {
        height,
        tip,
        difficulty: diff,
        total_supply,
        alice_balance: bal_alice,
        bob_balance: bal_bob,
        alice_nonce: nonce_alice,
        phase: phase_str,
    };

    (StatusCode::OK, Json(resp)).into_response()
}

/// GET /peers
async fn handle_peers() -> Response {
    let peer_infos = collect_peer_infos();
    let body = serde_json::json!({ "peers": peer_infos });
    (StatusCode::OK, Json(body)).into_response()
}

#[derive(Debug, Deserialize)]
struct SyncQuery {
    peer: Option<String>,
}

/// GET/POST /sync?peer=IP:PORT
async fn handle_sync(
    State(st): State<RpcState>,
    Query(q): Query<SyncQuery>,
) -> Response {
    let peer = match q.peer {
        Some(p) if !p.is_empty() => p,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                "missing peer param, use /sync?peer=IP:PORT".to_string(),
            )
                .into_response();
        }
    };

    let bc_for_sync = Arc::clone(&st.blockchain);

    thread::spawn(move || {
        if let Some(peers_arc) = get_peers_arc() {
            let mut peers = peers_arc.lock().unwrap();
            if !peers.contains(&peer) {
                peers.push(peer.clone());
            }
        }

        for _ in 0..3 {
            let (blocks_opt, _locs) = {
                let (locs, local_h) = {
                    let bc = bc_for_sync.lock().unwrap();
                    let locs = build_locators_for_rpc(&bc, 32);
                    (locs, bc.chain.len() as u64)
                };

                match request_status(&peer) {
                    Ok((h, _)) => {
                        if h > local_h {
                            match request_blocks_from_locators(&peer, &locs, 500) {
                                Ok(blocks) => (Some(blocks), locs),
                                Err(_) => (None, locs),
                            }
                        } else {
                            (None, locs)
                        }
                    }
                    Err(_) => (None, locs),
                }
            };

            if let Some(blocks) = blocks_opt {
                let mut bc = bc_for_sync.lock().unwrap();
                for b in blocks {
                    let _ = bc.add_block(b);
                }
            }

            thread::sleep(Duration::from_millis(500));
        }
    });

    (StatusCode::OK, "sync started".to_string()).into_response()
}

#[derive(Debug, Deserialize)]
struct BanQuery {
    peer: Option<String>,
    duration_ms: Option<u128>,
}

/// POST /ban?peer=IP:PORT[&duration_ms=...]
async fn handle_ban(Query(q): Query<BanQuery>) -> Response {
    let peer = match q.peer {
        Some(p) if !p.is_empty() => p,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                "missing peer param, use ?peer=IP:PORT".to_string(),
            )
                .into_response();
        }
    };

    let dur_ms = q.duration_ms.unwrap_or(60_000);
    let until = now_ms() + dur_ms;
    ban_peer(&peer, until);

    let body = serde_json::json!({
        "status": "banned",
        "peer": peer,
        "until": until
    });

    (StatusCode::OK, Json(body)).into_response()
}

/// POST /unban?peer=IP:PORT
async fn handle_unban(Query(q): Query<BanQuery>) -> Response {
    let peer = match q.peer {
        Some(p) if !p.is_empty() => p,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                "missing peer param, use ?peer=IP:PORT".to_string(),
            )
                .into_response();
        }
    };

    unban_peer(&peer);
    let body = serde_json::json!({
        "status": "unbanned",
        "peer": peer
    });

    (StatusCode::OK, Json(body)).into_response()
}

/// GET /balance?addr=...
#[derive(Debug, Deserialize)]
struct AddrQuery {
    addr: String,
}

#[derive(Debug, Serialize)]
struct BalanceResponse {
    addr: String,
    balance: u64,
}

async fn handle_balance(
    State(st): State<RpcState>,
    Query(q): Query<AddrQuery>,
) -> Response {
    let bc = st.blockchain.lock().unwrap();
    let bal = bc.state.balance_of(&q.addr);
    let resp = BalanceResponse {
        addr: q.addr,
        balance: bal,
    };
    (StatusCode::OK, Json(resp)).into_response()
}

/// GET /nonce?addr=...
#[derive(Debug, Serialize)]
struct NonceResponse {
    addr: String,
    nonce: u64,
}

async fn handle_nonce(
    State(st): State<RpcState>,
    Query(q): Query<AddrQuery>,
) -> Response {
    let bc = st.blockchain.lock().unwrap();
    let nonce = bc.state.nonce_of(&q.addr);
    let resp = NonceResponse {
        addr: q.addr,
        nonce,
    };
    (StatusCode::OK, Json(resp)).into_response()
}

/// GET /p2p_debug
async fn handle_p2p_debug() -> Response {
    let peer_infos = collect_peer_infos();
    let total_peers = peer_infos.len();
    let now = now_ms();
    let banned_peers = peer_infos
        .iter()
        .filter(|p| p.banned_until > now)
        .count();

    let body = P2pDebugInfo {
        peers: peer_infos,
        total_peers,
        banned_peers,
    };

    (StatusCode::OK, Json(body)).into_response()
}

/// GET /mempool — список транзакций в локальном mempool
async fn handle_mempool(State(st): State<RpcState>) -> Response {
    let mp = st.mempool.lock().unwrap();

    let mut out = Vec::new();
    for (hash, tx) in mp.txs.iter() {
        let kind = match tx {
            Transaction::Transfer { .. } => "Transfer",
            Transaction::RotateAIKey { .. } => "RotateAIKey",
            Transaction::Signed(_) => "Signed",
        }
        .to_string();

        out.push(MempoolTx {
            hash: hash.clone(),
            kind,
        });
    }

    let resp = MempoolResponse { txs: out };
    (StatusCode::OK, Json(resp)).into_response()
}

fn collect_peer_infos() -> Vec<PeerInfo> {
    let mut result = Vec::new();

    let arc_opt: Option<Arc<Mutex<HashMap<String, PeerState>>>> = get_peer_states_arc();
    let arc = match arc_opt {
        Some(a) => a,
        None => return result,
    };

    let map_guard = arc.lock().unwrap();
    for (addr, st) in map_guard.iter() {
        result.push(PeerInfo {
            addr: addr.clone(),
            last_error_ts: st.last_error_ts,
            error_count: st.error_count,
            banned_until: st.banned_until,
            last_contact_ts: st.last_contact_ts,
        });
    }

    result
}

fn build_locators_for_rpc(bc: &Blockchain, max_count: usize) -> Vec<String> {
    let mut locators = Vec::new();
    if bc.chain.is_empty() {
        return locators;
    }

    let mut index = bc.chain.len() as i64 - 1;
    let mut step = 1i64;

    while index >= 0 && locators.len() < max_count {
        let h = bc.chain[index as usize].hash.clone();
        locators.push(h);

        if index == 0 {
            break;
        }

        index -= step;
        if index < 0 {
            index = 0;
        }
        if locators.len() > 10 {
            step *= 2;
        }
    }

    if let Some(genesis_hash) = bc.chain.first().map(|b| b.hash.clone()) {
        if !locators.contains(&genesis_hash) {
            locators.push(genesis_hash);
        }
    }

    locators
}