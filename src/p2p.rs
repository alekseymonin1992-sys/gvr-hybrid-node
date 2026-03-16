use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use k256::ecdsa::{signature::Signer, signature::Verifier, Signature, SigningKey, VerifyingKey};
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::block::Block;
use crate::blockchain::Blockchain;
use crate::hashing::hash as sha256_bytes;
use crate::mempool::Mempool;
use crate::transaction::Transaction;

const MAX_PEERS: usize = 512;
const MAX_INBOUND_CONN: usize = 128;

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type", content = "payload")]
enum P2pMessage {
    Hello {
        node_id: String,
        height: u64,
        last_hash: String,
        pubkey_sec1: Vec<u8>,
        signature: Vec<u8>,
    },

    Block(Block),
    Tx(Transaction),

    Ping,
    GetStatus,
    Status { height: u64, last_hash: String },

    GetBlocks { from_index: u64, max: u64 },
    GetBlocksFromLocators { locators: Vec<String>, max: u64 },
    Blocks(Vec<Block>),

    InvTx { tx_hashes: Vec<String> },
    InvBlock { block_hashes: Vec<String> },
    GetDataTx { tx_hashes: Vec<String> },
    GetDataBlock { block_hashes: Vec<String> },
    GetMempool,
    MempoolInv { tx_hashes: Vec<String> },

    GetPeers,
    Peers(Vec<String>),
}

static SHUTDOWN: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Serialize)]
pub struct PeerState {
    pub last_error_ts: u128,
    pub error_count: u32,
    pub banned_until: u128,
    pub last_contact_ts: u128,
}

impl PeerState {
    fn new(now: u128) -> Self {
        PeerState {
            last_error_ts: now,
            error_count: 0,
            banned_until: 0,
            last_contact_ts: 0,
        }
    }
}

#[derive(Debug, Clone)]
struct InboundRate {
    last_msg_ts: u128,
    msg_count: u32,
}

impl InboundRate {
    fn new(now: u128) -> Self {
        InboundRate {
            last_msg_ts: now,
            msg_count: 0,
        }
    }
}

#[derive(Debug)]
struct SeenCache {
    max_entries: usize,
    order: VecDeque<Vec<u8>>,
    map: HashMap<Vec<u8>, ()>,
}

impl SeenCache {
    fn new(max_entries: usize) -> Self {
        SeenCache {
            max_entries,
            order: VecDeque::new(),
            map: HashMap::new(),
        }
    }

    fn check_and_insert(&mut self, key: &[u8]) -> bool {
        if self.map.contains_key(key) {
            return true;
        }
        let k = key.to_vec();
        self.map.insert(k.clone(), ());
        self.order.push_back(k);
        if self.order.len() > self.max_entries {
            if let Some(old) = self.order.pop_front() {
                self.map.remove(&old);
            }
        }
        false
    }
}

#[derive(Clone)]
struct P2pContext {
    peers: Arc<Mutex<Vec<String>>>,
    peer_states: Arc<Mutex<HashMap<String, PeerState>>>,
    blockchain: Arc<Mutex<Blockchain>>,
    node_id: String,
    p2p_sk: SigningKey,
}

static mut GLOBAL_PEER_STATES: Option<Arc<Mutex<HashMap<String, PeerState>>>> = None;
static mut GLOBAL_PEERS: Option<Arc<Mutex<Vec<String>>>> = None;

pub fn signal_shutdown() {
    SHUTDOWN.store(true, Ordering::SeqCst);
}

#[allow(static_mut_refs)]
pub fn get_peer_states_arc() -> Option<Arc<Mutex<HashMap<String, PeerState>>>> {
    unsafe { GLOBAL_PEER_STATES.clone() }
}

#[allow(static_mut_refs)]
pub fn get_peers_arc() -> Option<Arc<Mutex<Vec<String>>>> {
    unsafe { GLOBAL_PEERS.clone() }
}

pub fn get_peers_list() -> Vec<String> {
    if let Some(arc) = get_peers_arc() {
        let guard = arc.lock().unwrap();
        guard.clone()
    } else {
        Vec::new()
    }
}

#[allow(static_mut_refs)]
pub fn ban_peer(addr: &str, until_ts: u128) {
    if let Some(arc) = unsafe { GLOBAL_PEER_STATES.clone() } {
        let mut map = arc.lock().unwrap();
        let now = now_ms();
        let st = map.entry(addr.to_string()).or_insert_with(|| PeerState::new(now));
        st.banned_until = until_ts;
    }
}

#[allow(static_mut_refs)]
pub fn unban_peer(addr: &str) {
    if let Some(arc) = unsafe { GLOBAL_PEER_STATES.clone() } {
        let mut map = arc.lock().unwrap();
        if let Some(st) = map.get_mut(addr) {
            st.banned_until = 0;
            st.error_count = 0;
        }
    }
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u128
}

fn read_exact_len(stream: &mut TcpStream, len: usize) -> std::io::Result<Vec<u8>> {
    let mut buf = vec![0u8; len];
    let mut read = 0usize;
    while read < len {
        match stream.read(&mut buf[read..]) {
            Ok(0) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "peer closed",
                ))
            }
            Ok(n) => read += n,
            Err(e) => return Err(e),
        }
    }
    Ok(buf)
}

fn recv_message(stream: &mut TcpStream) -> Option<P2pMessage> {
    let mut lenb = [0u8; 4];
    if stream.read_exact(&mut lenb).is_err() {
        return None;
    }
    let len = u32::from_be_bytes(lenb) as usize;
    if len == 0 || len > 10_000_000 {
        return None;
    }
    let body = match read_exact_len(stream, len) {
        Ok(b) => b,
        Err(_) => return None,
    };
    serde_json::from_slice::<P2pMessage>(&body).ok()
}

fn send_message(stream: &mut TcpStream, msg: &P2pMessage) -> std::io::Result<()> {
    let payload = serde_json::to_vec(msg)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
    let len = (payload.len() as u32).to_be_bytes();
    stream.write_all(&len)?;
    stream.write_all(&payload)?;
    stream.flush()?;
    Ok(())
}

fn hello_canonical_bytes(node_id: &str, height: u64, last_hash: &str, pubkey_sec1: &[u8]) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(node_id.as_bytes());
    buf.push(0u8);
    buf.extend_from_slice(&height.to_be_bytes());
    buf.push(0u8);
    buf.extend_from_slice(last_hash.as_bytes());
    buf.push(0u8);
    buf.extend_from_slice(pubkey_sec1);
    buf
}

fn hash_hello(node_id: &str, height: u64, last_hash: &str, pubkey_sec1: &[u8]) -> [u8; 32] {
    let data = hello_canonical_bytes(node_id, height, last_hash, pubkey_sec1);
    let mut hasher = Sha256::new();
    hasher.update(&data);
    let out = hasher.finalize();
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&out);
    arr
}

fn send_message_to_peer_with_hello(
    addr: &str,
    msg: &P2pMessage,
    ctx: &P2pContext,
) -> std::io::Result<()> {
    let now = now_ms();
    {
        let mut map = ctx.peer_states.lock().unwrap();
        let st = map.entry(addr.to_string()).or_insert_with(|| PeerState::new(now));
        if st.banned_until > now {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "peer banned",
            ));
        }
        if now.saturating_sub(st.last_contact_ts) < 3_000 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::WouldBlock,
                "backoff",
            ));
        }
        st.last_contact_ts = now;
    }

    if let Ok(mut stream) = TcpStream::connect(addr) {
        let (height, last_hash) = {
            let bc = ctx.blockchain.lock().unwrap();
            (bc.chain.len() as u64, bc.last_hash())
        };

        let p2p_pub = VerifyingKey::from(&ctx.p2p_sk);
        let pubkey_sec1 = p2p_pub.to_encoded_point(false).as_bytes().to_vec();

        let h = hash_hello(&ctx.node_id, height, &last_hash, &pubkey_sec1);
        let sig: Signature = ctx.p2p_sk.sign(&h);

        let hello = P2pMessage::Hello {
            node_id: ctx.node_id.clone(),
            height,
            last_hash,
            pubkey_sec1,
            signature: sig.to_der().as_bytes().to_vec(),
        };

        send_message(&mut stream, &hello)?;
        send_message(&mut stream, msg)?;
        Ok(())
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "connect failed",
        ))
    }
}

pub fn start_p2p(
    addr: &str,
    peers: Vec<String>,
    blockchain: Arc<Mutex<Blockchain>>,
    mempool: Arc<Mutex<Mempool>>,
) {
    let listener = TcpListener::bind(addr).expect("p2p bind failed");
    listener
        .set_nonblocking(true)
        .expect("set_nonblocking failed");
    println!("P2P listening on {}", addr);

    let p2p_sk = {
        let mut rng = rand::thread_rng();
        let mut seed = [0u8; 32];
        rng.fill(&mut seed);
        SigningKey::from_bytes(&seed).expect("create p2p signing key")
    };

    let my_node_id = {
        let mut rng = rand::thread_rng();
        format!("node-{:016x}", rng.gen::<u64>())
    };
    println!("P2P node_id={}", my_node_id);

    let peer_states: Arc<Mutex<HashMap<String, PeerState>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let peers_arc: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(peers.clone()));
    let inbound_rates: Arc<Mutex<HashMap<String, InboundRate>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let inbound_conn_count: Arc<Mutex<usize>> = Arc::new(Mutex::new(0usize));

    let seen_blocks = Arc::new(Mutex::new(SeenCache::new(10_000)));
    let seen_txs = Arc::new(Mutex::new(SeenCache::new(50_000)));

    unsafe {
        GLOBAL_PEER_STATES = Some(peer_states.clone());
        GLOBAL_PEERS = Some(peers_arc.clone());
    }

    let ctx_out = P2pContext {
        peers: Arc::clone(&peers_arc),
        peer_states: Arc::clone(&peer_states),
        blockchain: Arc::clone(&blockchain),
        node_id: my_node_id.clone(),
        p2p_sk: p2p_sk.clone(),
    };

    let bc_in = Arc::clone(&blockchain);
    let mp_in = Arc::clone(&mempool);
    let my_node_id_in = my_node_id.clone();
    let peers_for_incoming = Arc::clone(&peers_arc);
    let inbound_rates_in = Arc::clone(&inbound_rates);
    let inbound_conn_count_in = Arc::clone(&inbound_conn_count);
    let seen_blocks_in = Arc::clone(&seen_blocks);
    let seen_txs_in = Arc::clone(&seen_txs);

    thread::spawn(move || {
        loop {
            if SHUTDOWN.load(Ordering::SeqCst) {
                println!("P2P: incoming thread shutting down");
                break;
            }

            match listener.accept() {
                Ok((s, _)) => {
                    {
                        let mut cnt = inbound_conn_count_in.lock().unwrap();
                        if *cnt >= MAX_INBOUND_CONN {
                            println!("P2P: too many inbound connections, dropping");
                            drop(s);
                            continue;
                        }
                        *cnt += 1;
                    }

                    let peer_addr_opt: Option<SocketAddr> = s.peer_addr().ok();
                    if let Some(peer_addr) = peer_addr_opt {
                        let addr_str = peer_addr.to_string();

                        {
                            let mut list = peers_for_incoming.lock().unwrap();
                            if !list.contains(&addr_str) {
                                if list.len() < MAX_PEERS {
                                    println!("P2P: discovered incoming peer {}", addr_str);
                                    list.push(addr_str.clone());
                                } else {
                                    println!(
                                        "P2P: peer list full ({} entries), ignoring {}",
                                        list.len(),
                                        addr_str
                                    );
                                }
                            }
                        }

                        {
                            let now = now_ms();
                            let mut rates = inbound_rates_in.lock().unwrap();
                            rates
                                .entry(addr_str.clone())
                                .or_insert_with(|| InboundRate::new(now));
                        }
                    }

                    let bc = Arc::clone(&bc_in);
                    let mp = Arc::clone(&mp_in);
                    let my_id = my_node_id_in.clone();
                    let inbound_rates_conn = Arc::clone(&inbound_rates_in);
                    let inbound_conn_count_conn = Arc::clone(&inbound_conn_count_in);
                    let seen_blocks_conn = Arc::clone(&seen_blocks_in);
                    let seen_txs_conn = Arc::clone(&seen_txs_in);

                    thread::spawn(move || {
                        handle_connection(
                            s,
                            bc,
                            mp,
                            &my_id,
                            inbound_rates_conn,
                            inbound_conn_count_conn,
                            seen_blocks_conn,
                            seen_txs_conn,
                        );
                    });
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(50));
                }
                Err(e) => {
                    println!("P2P accept error: {}", e);
                    thread::sleep(Duration::from_millis(100));
                }
            }
        }
    });

    thread::spawn(move || loop {
        if SHUTDOWN.load(Ordering::SeqCst) {
            println!("P2P: outgoing thread shutting down");
            break;
        }

        let peers_snapshot = {
            let guard = ctx_out.peers.lock().unwrap();
            guard.clone()
        };

        for peer in &peers_snapshot {
            if SHUTDOWN.load(Ordering::SeqCst) {
                break;
            }

            let _ = send_message_to_peer_with_hello(peer, &P2pMessage::Ping, &ctx_out);

            match request_status(peer) {
                Ok((height, _last_hash)) => {
                    let (local_height, locators) = {
                        let bc = ctx_out.blockchain.lock().unwrap();
                        let h = bc.chain.len() as u64;
                        let locs = build_locators(&bc, 32);
                        (h, locs)
                    };
                    if height > local_height {
                        println!(
                            "P2P: peer {} has higher height {} (local {}), requesting blocks by locators",
                            peer, height, local_height
                        );
                        if let Ok(blocks) =
                            request_blocks_from_locators(peer, &locators, 500)
                        {
                            let mut bc = ctx_out.blockchain.lock().unwrap();
                            for b in blocks {
                                let _ = bc.add_block(b);
                            }
                        }
                    }
                }
                Err(e) => {
                    let now = now_ms();
                    let mut map = ctx_out.peer_states.lock().unwrap();
                    let st = map.entry(peer.to_string()).or_insert_with(|| PeerState::new(now));
                    st.error_count = st.error_count.saturating_add(1);
                    st.last_error_ts = now;

                    if st.error_count >= 5 {
                        st.banned_until = now + 60_000;
                        println!(
                            "P2P: peer {} banned for 60s due to repeated errors (last: {})",
                            peer, e
                        );
                        st.error_count = 0;
                    }
                }
            }

            let _ = send_simple(peer, &P2pMessage::GetPeers);
            let _ = send_simple(peer, &P2pMessage::GetMempool);

            thread::sleep(Duration::from_millis(200));
        }
        thread::sleep(Duration::from_secs(10));
    });
}

fn send_simple(addr: &str, msg: &P2pMessage) -> std::io::Result<()> {
    if let Ok(mut stream) = TcpStream::connect(addr) {
        send_message(&mut stream, msg)?;
        Ok(())
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "connect failed",
        ))
    }
}

fn handle_connection(
    mut stream: TcpStream,
    blockchain: Arc<Mutex<Blockchain>>,
    mempool: Arc<Mutex<Mempool>>,
    my_node_id: &str,
    inbound_rates: Arc<Mutex<HashMap<String, InboundRate>>>,
    inbound_conn_count: Arc<Mutex<usize>>,
    seen_blocks: Arc<Mutex<SeenCache>>,
    seen_txs: Arc<Mutex<SeenCache>>,
) {
    let peer_addr_str = stream
        .peer_addr()
        .map(|a| a.to_string())
        .unwrap_or_else(|_| "<unknown>".to_string());

    if let Some(first_msg) = recv_message(&mut stream) {
        {
            let now = now_ms();
            let mut rates = inbound_rates.lock().unwrap();
            let r = rates
                .entry(peer_addr_str.clone())
                .or_insert_with(|| InboundRate::new(now));
            if now.saturating_sub(r.last_msg_ts) > 5_000 {
                r.last_msg_ts = now;
                r.msg_count = 0;
            }
            r.msg_count = r.msg_count.saturating_add(1);
        }

        match first_msg {
            P2pMessage::Hello {
                node_id,
                height,
                last_hash,
                pubkey_sec1,
                signature,
            } => {
                let vk = match VerifyingKey::from_sec1_bytes(&pubkey_sec1) {
                    Ok(v) => v,
                    Err(e) => {
                        println!(
                            "P2P: incoming hello from {} has invalid pubkey: {}",
                            node_id, e
                        );
                        drop_connection(inbound_conn_count);
                        return;
                    }
                };

                let h = hash_hello(&node_id, height, &last_hash, &pubkey_sec1);
                let sig = match Signature::from_der(&signature) {
                    Ok(s) => s,
                    Err(_) => {
                        println!(
                            "P2P: incoming hello from {} has invalid signature format",
                            node_id
                        );
                        drop_connection(inbound_conn_count);
                        return;
                    }
                };

                let sig_ok = vk.verify(&h, &sig).is_ok();

                if !sig_ok {
                    println!(
                        "P2P: incoming hello from {} failed signature verification (height={}, tip={})",
                        node_id, height, last_hash
                    );
                    drop_connection(inbound_conn_count);
                    return;
                }

                println!(
                    "P2P: incoming hello from {} (height={} tip={}) [signature OK]",
                    node_id, height, last_hash
                );

                let _ = my_node_id;
            }
            other => {
                handle_msg(
                    other,
                    &mut stream,
                    &blockchain,
                    &mempool,
                    my_node_id,
                    &peer_addr_str,
                    &inbound_rates,
                    &seen_blocks,
                    &seen_txs,
                );
                drop_connection(inbound_conn_count);
                return;
            }
        }
    } else {
        drop_connection(inbound_conn_count);
        return;
    }

    while !SHUTDOWN.load(Ordering::SeqCst) {
        if let Some(msg) = recv_message(&mut stream) {
            {
                let now = now_ms();
                let mut rates = inbound_rates.lock().unwrap();
                let r = rates
                    .entry(peer_addr_str.clone())
                    .or_insert_with(|| InboundRate::new(now));
                if now.saturating_sub(r.last_msg_ts) > 5_000 {
                    r.last_msg_ts = now;
                    r.msg_count = 0;
                }
                r.msg_count = r.msg_count.saturating_add(1);
                if r.msg_count > 20_000 {
                    println!(
                        "P2P: inbound peer {} exceeded per-conn msg limit, closing",
                        peer_addr_str
                    );
                    break;
                }
            }

            handle_msg(
                msg,
                &mut stream,
                &blockchain,
                &mempool,
                my_node_id,
                &peer_addr_str,
                &inbound_rates,
                &seen_blocks,
                &seen_txs,
            );
        } else {
            break;
        }
    }

    drop_connection(inbound_conn_count);
}

fn drop_connection(inbound_conn_count: Arc<Mutex<usize>>) {
    let mut cnt = inbound_conn_count.lock().unwrap();
    *cnt = cnt.saturating_sub(1);
}

fn handle_msg(
    msg: P2pMessage,
    stream: &mut TcpStream,
    blockchain: &Arc<Mutex<Blockchain>>,
    mempool: &Arc<Mutex<Mempool>>,
    _my_node_id: &str,
    peer_addr: &str,
    _inbound_rates: &Arc<Mutex<HashMap<String, InboundRate>>>,
    seen_blocks: &Arc<Mutex<SeenCache>>,
    seen_txs: &Arc<Mutex<SeenCache>>,
) {
    match msg {
        P2pMessage::Ping => {}

        P2pMessage::Block(b) => {
            if b.transactions.len() > 10_000 {
                println!(
                    "P2P: received block idx={} with too many txs ({}), rejecting",
                    b.index,
                    b.transactions.len()
                );
                return;
            }

            {
                let mut seen = seen_blocks.lock().unwrap();
                let key = b.hash.as_bytes();
                if seen.check_and_insert(key) {
                    return;
                }
            }

            println!("P2P: received block idx={} hash={}", b.index, b.hash);
            let mut bc = blockchain.lock().unwrap();
            let accepted = bc.add_block(b.clone());
            if accepted {
                println!("P2P: block accepted idx={}", b.index);
                let peers = get_peers_list();
                if !peers.is_empty() {
                    let inv = P2pMessage::InvBlock {
                        block_hashes: vec![b.hash.clone()],
                    };
                    for p in peers {
                        if p == *peer_addr {
                            continue;
                        }
                        let _ = send_simple(&p, &inv);
                    }
                }
            } else {
                println!("P2P: block rejected idx={}", b.index);
            }
        }

        P2pMessage::Tx(tx) => {
            let tx_bytes = match serde_json::to_vec(&tx) {
                Ok(b) => b,
                Err(_) => return,
            };
            let h = sha256_bytes(&tx_bytes);
            {
                let mut seen = seen_txs.lock().unwrap();
                if seen.check_and_insert(&h) {
                    return;
                }
            }

            println!("P2P: received tx");
            let mut mp = mempool.lock().unwrap();
            mp.add_tx(tx.clone());

            let tx_hash_hex = hex::encode(h);
            let peers = get_peers_list();
            if !peers.is_empty() {
                let inv = P2pMessage::InvTx {
                    tx_hashes: vec![tx_hash_hex],
                };
                for p in peers {
                    if p == *peer_addr {
                        continue;
                    }
                    let _ = send_simple(&p, &inv);
                }
            }
        }

        P2pMessage::GetStatus => {
            let bc = blockchain.lock().unwrap();
            let height = bc.chain.len() as u64;
            let last_hash = bc.last_hash();
            let reply = P2pMessage::Status { height, last_hash };
            if let Err(e) = send_message(stream, &reply) {
                println!("P2P: failed to send Status: {}", e);
            }
        }

        P2pMessage::Status { .. } => {}

        P2pMessage::GetBlocks { from_index, max } => {
            let max = max.min(1000);
            let bc = blockchain.lock().unwrap();
            let start = from_index as usize;
            let end = (from_index + max) as usize;

            let len = bc.chain.len();
            if start >= len {
                let reply = P2pMessage::Blocks(Vec::new());
                if let Err(e) = send_message(stream, &reply) {
                    println!("P2P: failed to send Blocks: {}", e);
                }
                return;
            }

            let actual_end = end.min(len);
            let blocks: Vec<Block> = bc.chain[start..actual_end].to_vec();
            let reply = P2pMessage::Blocks(blocks);
            if let Err(e) = send_message(stream, &reply) {
                println!("P2P: failed to send Blocks: {}", e);
            }
        }

        P2pMessage::GetBlocksFromLocators { locators, max } => {
            let max = max.min(1000);
            let bc = blockchain.lock().unwrap();
            let start_index_opt = find_common_ancestor_index(&bc, &locators);

            if let Some(start_idx) = start_index_opt {
                let start = (start_idx + 1) as usize;
                let len = bc.chain.len();
                if start >= len {
                    let reply = P2pMessage::Blocks(Vec::new());
                    if let Err(e) = send_message(stream, &reply) {
                        println!("P2P: failed to send Blocks: {}", e);
                    }
                    return;
                }
                let end = ((start_idx + 1) as u64 + max) as usize;
                let actual_end = end.min(len);
                let blocks: Vec<Block> = bc.chain[start..actual_end].to_vec();
                let reply = P2pMessage::Blocks(blocks);
                if let Err(e) = send_message(stream, &reply) {
                    println!("P2P: failed to send Blocks: {}", e);
                }
            } else {
                let reply = P2pMessage::Blocks(Vec::new());
                if let Err(e) = send_message(stream, &reply) {
                    println!("P2P: failed to send Blocks (no common ancestor): {}", e);
                }
            }
        }

        P2pMessage::Blocks(_) => {}

        P2pMessage::InvTx { tx_hashes } => {
            let mut missing = Vec::new();
            {
                let mp = mempool.lock().unwrap();
                for h in &tx_hashes {
                    if !mp.txs.contains_key(h) {
                        missing.push(h.clone());
                    }
                }
            }
            if !missing.is_empty() {
                let req = P2pMessage::GetDataTx { tx_hashes: missing };
                let _ = send_message(stream, &req);
            }
        }

        P2pMessage::InvBlock { block_hashes } => {
            let mut missing = Vec::new();
            {
                let bc = blockchain.lock().unwrap();
                for h in &block_hashes {
                    if !bc.blocks_by_hash.contains_key(h) {
                        missing.push(h.clone());
                    }
                }
            }
            if !missing.is_empty() {
                let req = P2pMessage::GetDataBlock {
                    block_hashes: missing,
                };
                let _ = send_message(stream, &req);
            }
        }

        P2pMessage::GetDataTx { tx_hashes } => {
            let mp = mempool.lock().unwrap();
            for h in tx_hashes {
                if let Some(tx) = mp.txs.get(&h) {
                    let msg = P2pMessage::Tx(tx.clone());
                    if let Err(e) = send_message(stream, &msg) {
                        println!("P2P: failed to send Tx {}: {}", h, e);
                    }
                }
            }
        }

        P2pMessage::GetDataBlock { block_hashes } => {
            let bc = blockchain.lock().unwrap();
            for h in block_hashes {
                if let Some(b) = bc.blocks_by_hash.get(&h) {
                    let msg = P2pMessage::Block(b.clone());
                    if let Err(e) = send_message(stream, &msg) {
                        println!("P2P: failed to send Block {}: {}", h, e);
                    }
                }
            }
        }

        P2pMessage::GetMempool => {
            let mp = mempool.lock().unwrap();
            let hashes: Vec<String> = mp.txs.keys().cloned().collect();
            let msg = P2pMessage::MempoolInv { tx_hashes: hashes };
            if let Err(e) = send_message(stream, &msg) {
                println!("P2P: failed to send MempoolInv: {}", e);
            }
        }

        P2pMessage::MempoolInv { tx_hashes } => {
            let mut missing = Vec::new();
            {
                let mp = mempool.lock().unwrap();
                for h in &tx_hashes {
                    if !mp.txs.contains_key(h) {
                        missing.push(h.clone());
                    }
                }
            }
            if !missing.is_empty() {
                let req = P2pMessage::GetDataTx { tx_hashes: missing };
                let _ = send_message(stream, &req);
            }
        }

        P2pMessage::GetPeers => {
            let peers = get_peers_list();
            let msg = P2pMessage::Peers(peers);
            if let Err(e) = send_message(stream, &msg) {
                println!("P2P: failed to send Peers: {}", e);
            }
        }

        P2pMessage::Peers(list) => {
            if let Some(arc) = get_peers_arc() {
                let mut peers = arc.lock().unwrap();
                for p in list {
                    if p == *peer_addr {
                        continue;
                    }
                    if !peers.contains(&p) {
                        if peers.len() < MAX_PEERS {
                            println!("P2P: learned about peer {}", p);
                            peers.push(p);
                        } else {
                            println!(
                                "P2P: peer list full ({} entries), ignoring {}",
                                peers.len(),
                                p
                            );
                        }
                    }
                }
            }
        }

        P2pMessage::Hello { .. } => {}
    }
}

pub fn request_status(addr: &str) -> std::io::Result<(u64, String)> {
    if let Ok(mut stream) = TcpStream::connect(addr) {
        let msg = P2pMessage::GetStatus;
        send_message(&mut stream, &msg)?;
        if let Some(P2pMessage::Status { height, last_hash }) = recv_message(&mut stream) {
            Ok((height, last_hash))
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "invalid Status reply",
            ))
        }
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "connect failed",
        ))
    }
}

pub fn request_blocks_from_locators(
    addr: &str,
    locators: &[String],
    max: u64,
) -> std::io::Result<Vec<Block>> {
    if let Ok(mut stream) = TcpStream::connect(addr) {
        let msg = P2pMessage::GetBlocksFromLocators {
            locators: locators.to_vec(),
            max,
        };
        send_message(&mut stream, &msg)?;
        if let Some(P2pMessage::Blocks(blocks)) = recv_message(&mut stream) {
            Ok(blocks)
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "invalid Blocks reply",
            ))
        }
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "connect failed",
        ))
    }
}

fn build_locators(bc: &Blockchain, max_count: usize) -> Vec<String> {
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

fn find_common_ancestor_index(bc: &Blockchain, locators: &[String]) -> Option<u64> {
    for h in locators {
        if let Some(idx) = bc.chain.iter().position(|b| &b.hash == h) {
            return Some(idx as u64);
        }
    }
    None
}

pub fn broadcast_block(block: &Block, peers: &[String]) {
    let msg = P2pMessage::Block(block.clone());
    for peer in peers.iter() {
        let peer = peer.clone();
        let msg = msg.clone();
        thread::spawn(move || {
            if let Ok(mut stream) = TcpStream::connect(&peer) {
                let _ = send_message(&mut stream, &msg);
            }
        });
    }
}

pub fn broadcast_tx(tx: &Transaction, peers: &[String]) {
    let msg = P2pMessage::Tx(tx.clone());
    for peer in peers.iter() {
        let peer = peer.clone();
        let msg = msg.clone();
        thread::spawn(move || {
            if let Ok(mut stream) = TcpStream::connect(&peer) {
                let _ = send_message(&mut stream, &msg);
            }
        });
    }
}