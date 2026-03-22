use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};

use crate::accounts::SignedTransfer;
use crate::blockchain::Blockchain;
use crate::mempool::Mempool;
use crate::p2p::{
    ban_peer, get_peer_states_arc, get_peers_arc, request_blocks_from_locators, request_status,
    unban_peer, PeerState,
};
use crate::transaction::Transaction;

#[derive(Clone)]
struct RpcState {
    blockchain: Arc<Mutex<Blockchain>>,
    mempool: Arc<Mutex<Mempool>>,
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

/// Запуск HTTP RPC-сервера на axum.
pub fn start_rpc(
    bind_addr: &str,
    blockchain: Arc<Mutex<Blockchain>>,
    mempool: Arc<Mutex<Mempool>>,
) {
    let addr_s = bind_addr.to_string();
    println!("RPC (axum) starting on http://{}", addr_s);

    let state = RpcState {
        blockchain,
        mempool,
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

/// Собрать список PeerInfo из глобального p2p::GLOBAL_PEER_STATES.
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

/// Упрощённая копия build_locators для вызова из RPC.
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