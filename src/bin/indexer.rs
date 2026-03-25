/// NEXUS Indexer Server v2.9.2 — 扫链 + REST API + 响应缓存
///
/// 优化：
///   - 响应缓存层：HTTP 处理器不再锁 Indexer Mutex，直接读预序列化 JSON
///   - RwLock 缓存：多个 HTTP 请求并发读取，零阻塞
///   - Cache-Control：CF 边缘缓存 5 秒，减少回源
///   - 扫描线程：每轮扫描后更新缓存，HTTP 永不等待扫描完成
///
/// API端点:
///   GET /api/status              — 铸造进度、供应量、持有者数
///   GET /api/balance/:addr       — 地址NXS余额
///   GET /api/mint/:seq           — 按序号查铸造记录
///   GET /api/mints?page=1&limit=20 — 分页铸造列表
///   GET /api/mints/recent        — 最近铸造记录（前端用，最新20条）
///   GET /api/mints/address/:addr — 按地址查铸造记录
///   GET /api/holders             — 持有者排行（前100）
///   GET /api/tx/:txid            — 按txid查铸造记录
///   GET /api/health              — 健康检查

use std::sync::{Arc, Mutex, RwLock};
use std::collections::HashMap;
use actix_web::{web, App, HttpServer, HttpResponse, middleware};
use serde::{Serialize, Deserialize};

use nexus_reactor::constants::*;
use nexus_reactor::indexer::*;
use nexus_reactor::transaction::*;
use nexus_reactor::node_detect;

// ═══════════════════════════════════════════
//  响应缓存 — HTTP处理器只读这里，不锁Indexer
// ═══════════════════════════════════════════

/// 预序列化的 JSON 响应，由扫描线程定期更新
struct ResponseCache {
    status: String,
    holders: String,
    mints_recent: String,
    health: String,
}

impl ResponseCache {
    fn empty() -> Self {
        Self {
            status: "{}".to_string(),
            holders: r#"{"holders":[],"total_holders":0}"#.to_string(),
            mints_recent: r#"{"mints":[],"total":0}"#.to_string(),
            health: r#"{"status":"starting","protocol":"NEXUS","version":"2.9.2"}"#.to_string(),
        }
    }
}

// ═══════════════════════════════════════════
//  状态
// ═══════════════════════════════════════════

struct AppState {
    indexer: Mutex<Indexer>,
    scan_height: Mutex<u32>,
    /// 预序列化响应缓存 — RwLock 允许多个 HTTP 并发读
    cache: RwLock<ResponseCache>,
    rpc_url: String,
    rpc_user: String,
    rpc_pass: String,
}

// ═══════════════════════════════════════════
//  缓存刷新 — 扫描线程调用
// ═══════════════════════════════════════════

/// 锁 indexer 一次，序列化所有高频响应，写入 RwLock 缓存
/// HTTP 处理器直接读缓存，永远不等待扫描
fn refresh_cache(state: &AppState) {
    let indexer = state.indexer.lock().unwrap();
    let scan_h = *state.scan_height.lock().unwrap();

    // status
    let mut status_val = serde_json::to_value(&indexer.status()).unwrap();
    status_val["scan_height"] = serde_json::json!(scan_h);
    let status_json = serde_json::to_string(&status_val).unwrap_or_default();

    // holders (top 100)
    let mut holders_list: Vec<serde_json::Value> = indexer.balances.iter()
        .map(|(addr, &bal)| {
            let mint_count = indexer.mints.iter().filter(|m| m.address == *addr).count();
            serde_json::json!({"address": addr, "balance": bal, "mint_count": mint_count})
        })
        .collect();
    holders_list.sort_by(|a, b| b["balance"].as_u64().cmp(&a["balance"].as_u64()));
    holders_list.truncate(100);
    let holders_json = serde_json::to_string(&serde_json::json!({
        "total_holders": indexer.balances.len(),
        "holders": holders_list,
    })).unwrap_or_default();

    // mints/recent (top 20, newest first)
    let total = indexer.mints.len();
    let start = if total > 20 { total - 20 } else { 0 };
    let mut recent: Vec<_> = indexer.mints[start..].to_vec();
    recent.reverse();
    let mints_list: Vec<serde_json::Value> = recent.iter().map(|m| {
        serde_json::json!({
            "seq": m.seq, "address": m.address, "amount": m.amount,
            "reveal_txid": m.txid, "block_height": m.block_height,
        })
    }).collect();
    let mints_json = serde_json::to_string(&serde_json::json!({
        "mints": mints_list, "total": total,
    })).unwrap_or_default();

    // health
    let health_json = serde_json::to_string(&serde_json::json!({
        "status": "ok", "protocol": "NEXUS", "version": "2.9.2", "scan_height": scan_h,
    })).unwrap_or_default();

    // 写入缓存（write lock 很短，只是赋值字符串）
    let mut cache = state.cache.write().unwrap();
    cache.status = status_json;
    cache.holders = holders_json;
    cache.mints_recent = mints_json;
    cache.health = health_json;
}

// ═══════════════════════════════════════════
//  持久化
// ═══════════════════════════════════════════

const STATE_FILE: &str = "nexus_indexer_state.json";
const SCAN_FILE: &str = "nexus_indexer_scan.json";

fn save_state(indexer: &Indexer, scan_height: u32) {
    if let Ok(j) = serde_json::to_string_pretty(indexer) {
        std::fs::write(STATE_FILE, j).ok();
    }
    let scan = serde_json::json!({"scan_height": scan_height});
    std::fs::write(SCAN_FILE, scan.to_string()).ok();
}

fn load_state() -> (Indexer, u32) {
    let indexer = std::fs::read_to_string(STATE_FILE).ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(Indexer::new);

    let scan_height = std::fs::read_to_string(SCAN_FILE).ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v["scan_height"].as_u64())
        .unwrap_or(0) as u32;

    (indexer, scan_height)
}

// ═══════════════════════════════════════════
//  RPC辅助
// ═══════════════════════════════════════════

fn rpc_json(
    client: &reqwest::blocking::Client,
    url: &str, user: &str, pass: &str,
    method: &str, params: &[serde_json::Value],
) -> Result<serde_json::Value, String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0", "id": "indexer",
        "method": method, "params": params,
    });
    let resp: serde_json::Value = client.post(url)
        .basic_auth(user, Some(pass))
        .json(&body).send().map_err(|e| e.to_string())?
        .json().map_err(|e| e.to_string())?;
    if let Some(e) = resp.get("error") {
        if !e.is_null() { return Err(format!("RPC {}: {}", method, e)); }
    }
    resp.get("result").cloned().ok_or(format!("RPC {}: no result", method))
}

fn get_block_count(client: &reqwest::blocking::Client, url: &str, user: &str, pass: &str) -> Result<u32, String> {
    rpc_json(client, url, user, pass, "getblockcount", &[])
        .and_then(|v| v.as_u64().map(|n| n as u32).ok_or("bad blockcount".into()))
}

fn get_block_hash(client: &reqwest::blocking::Client, url: &str, user: &str, pass: &str, height: u32) -> Result<String, String> {
    rpc_json(client, url, user, pass, "getblockhash", &[serde_json::json!(height)])
        .and_then(|v| v.as_str().map(|s| s.to_string()).ok_or("bad hash".into()))
}

fn get_block(client: &reqwest::blocking::Client, url: &str, user: &str, pass: &str, hash: &str) -> Result<serde_json::Value, String> {
    rpc_json(client, url, user, pass, "getblock", &[serde_json::json!(hash), serde_json::json!(2)])
}

fn get_raw_tx(client: &reqwest::blocking::Client, url: &str, user: &str, pass: &str, txid: &str) -> Result<serde_json::Value, String> {
    rpc_json(client, url, user, pass, "getrawtransaction", &[serde_json::json!(txid), serde_json::json!(true)])
}

// ═══════════════════════════════════════════
//  交易解析
// ═══════════════════════════════════════════

fn parse_candidate(tx: &serde_json::Value, block_height: u32, tx_index: u32) -> Option<CandidateTx> {
    let txid = tx["txid"].as_str()?.to_string();
    let vouts = tx["vout"].as_array()?;

    let mut opreturn_data: Option<Vec<u8>> = None;
    let mut fee_output_valid = false;
    let mut minter_address = String::new();

    for (i, vout) in vouts.iter().enumerate() {
        let script_type = vout["scriptPubKey"]["type"].as_str().unwrap_or("");
        let hex_str = vout["scriptPubKey"]["hex"].as_str().unwrap_or("");

        match script_type {
            "nulldata" => {
                if let Ok(script_bytes) = hex::decode(hex_str) {
                    if script_bytes.len() > 4 && script_bytes[0] == 0x6a {
                        let data_start = if script_bytes.len() > 2 && script_bytes[1] <= 75 {
                            2
                        } else if script_bytes.len() > 3 && script_bytes[1] == 0x4c {
                            3
                        } else {
                            continue;
                        };
                        let data = &script_bytes[data_start..];
                        if let Ok(text) = std::str::from_utf8(data) {
                            if text.starts_with("NXS:") {
                                opreturn_data = Some(data.to_vec());
                            }
                        }
                    }
                }
            }
            "witness_v1_taproot" => {
                let addr = vout["scriptPubKey"]["address"].as_str().unwrap_or("");
                let sats = (vout["value"].as_f64().unwrap_or(0.0) * 1e8) as u64;

                if i == 0 {
                    minter_address = addr.to_string();
                }
                if addr == FEE_ADDRESS && sats >= MINT_FEE_SATS {
                    fee_output_valid = true;
                }
            }
            _ => {}
        }
    }

    opreturn_data.as_ref()?;

    let mut witness_json: Option<String> = None;
    let mut tx_pubkey: Option<String> = None;

    if let Some(vin0) = tx["vin"].as_array().and_then(|a| a.first()) {
        if let Some(witness) = vin0["txinwitness"].as_array() {
            if witness.len() >= 2 {
                let script_hex = witness[1].as_str().unwrap_or("");
                if script_hex.len() >= 66 {
                    let pk_start = if &script_hex[..2] == "20" { 2 } else { 0 };
                    if script_hex.len() >= pk_start + 64 {
                        tx_pubkey = Some(script_hex[pk_start..pk_start + 64].to_string());
                    }
                }

                if let Some(idx) = script_hex.find("7b22") {
                    let json_hex = &script_hex[idx..];
                    let mut depth: i32 = 0;
                    let mut end = 0;
                    let bytes: Vec<u8> = (0..json_hex.len() / 2)
                        .filter_map(|i| u8::from_str_radix(&json_hex[i * 2..i * 2 + 2], 16).ok())
                        .collect();
                    for (i, &b) in bytes.iter().enumerate() {
                        if b == b'{' { depth += 1; }
                        if b == b'}' {
                            depth -= 1;
                            if depth == 0 { end = i + 1; break; }
                        }
                    }
                    if end > 0 {
                        if let Ok(json_str) = String::from_utf8(bytes[..end].to_vec()) {
                            if json_str.contains("\"nexus\"") && json_str.contains("\"mint\"") {
                                witness_json = Some(json_str);
                            }
                        }
                    }
                }
            }
        }
    }

    witness_json.as_ref()?;

    Some(CandidateTx {
        txid,
        block_height,
        tx_index_in_block: tx_index,
        minter_address,
        witness_json,
        opreturn_bytes: opreturn_data,
        proof: None,
        fee_output_valid,
        tx_pubkey,
    })
}

// ═══════════════════════════════════════════
//  区块扫描
// ═══════════════════════════════════════════

fn scan_blocks(state: &AppState) {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build().unwrap();

    let chain_height = match get_block_count(&client, &state.rpc_url, &state.rpc_user, &state.rpc_pass) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("[scan] getblockcount failed: {}", e);
            return;
        }
    };

    let current_scan = *state.scan_height.lock().unwrap();

    let start = if current_scan == 0 { GENESIS_BLOCK } else { current_scan + 1 };
    let end = chain_height.min(start + 100);

    if start > chain_height {
        // 即使没有新区块，也刷新缓存（保持 scan_height 更新）
        refresh_cache(state);
        return;
    }

    let mut found = 0u32;

    for height in start..=end {
        let hash = match get_block_hash(&client, &state.rpc_url, &state.rpc_user, &state.rpc_pass, height) {
            Ok(h) => h,
            Err(_) => continue,
        };

        let block = match get_block(&client, &state.rpc_url, &state.rpc_user, &state.rpc_pass, &hash) {
            Ok(b) => b,
            Err(_) => continue,
        };

        let txs = match block["tx"].as_array() {
            Some(t) => t,
            None => continue,
        };

        let mut candidates = Vec::new();
        for (tx_idx, tx) in txs.iter().enumerate() {
            if let Some(candidate) = parse_candidate(tx, height, tx_idx as u32) {
                candidates.push(candidate);
            }
        }

        if !candidates.is_empty() {
            let mut indexer = state.indexer.lock().unwrap();
            for candidate in candidates {
                match light_validate(&indexer, &candidate) {
                    Ok(record) => {
                        println!("[scan] ✅ MINT #{} at block {} tx {}",
                            record.seq, height, &record.txid[..12]);
                        indexer.confirm(record);
                        found += 1;
                    }
                    Err(e) => {
                        eprintln!("[scan] ❌ Invalid NEXUS tx at block {}: {}", height, e);
                    }
                }
            }
        }

        *state.scan_height.lock().unwrap() = height;
    }

    // 保存状态
    let indexer = state.indexer.lock().unwrap();
    let scan_h = *state.scan_height.lock().unwrap();
    save_state(&indexer, scan_h);
    drop(indexer); // 显式释放锁，再刷新缓存

    // ★ 关键：扫描完成后刷新缓存，HTTP 处理器从缓存读取
    refresh_cache(state);

    if found > 0 {
        println!("[scan] Scanned {} → {}, found {} mints", start, end, found);
    }
}

fn light_validate(indexer: &Indexer, tx: &CandidateTx) -> Result<MintRecord, String> {
    if indexer.minted >= MAX_SUPPLY {
        return Err("supply exhausted".into());
    }

    let witness_json = tx.witness_json.as_ref().ok_or("no witness")?;
    let wit: WitnessPayload = serde_json::from_str(witness_json)
        .map_err(|e| format!("JSON parse: {}", e))?;
    if wit.p != "nexus" || wit.op != "mint" || wit.amt != MINT_AMOUNT {
        return Err("field mismatch".into());
    }

    if wit.pk.is_empty() {
        return Err("missing pk".into());
    }

    if let Some(ref tx_pk) = tx.tx_pubkey {
        if wit.pk != *tx_pk {
            return Err(format!("pk mismatch: {} vs {}", &wit.pk[..16], &tx_pk[..16]));
        }
    }

    let opr_bytes = tx.opreturn_bytes.as_ref().ok_or("no opreturn")?;
    let opr = OpReturnData::from_bytes(opr_bytes).ok_or("bad opreturn")?;
    if opr.magic != "NXS" || opr.op != "MINT" { return Err("bad magic/op".into()); }

    verify_interlock(witness_json, opr_bytes)?;

    if !tx.fee_output_valid {
        return Err("fee invalid".into());
    }

    if indexer.used_proofs.contains_key(&wit.fnp) {
        return Err("proof already used".into());
    }

    Ok(MintRecord {
        seq: indexer.next_seq,
        txid: tx.txid.clone(),
        address: tx.minter_address.clone(),
        amount: MINT_AMOUNT,
        block_height: tx.block_height,
        proof_hash: wit.fnp.clone(),
    })
}

const GENESIS_BLOCK: u32 = 941890;

// ═══════════════════════════════════════════
//  HTTP 响应辅助 — 带 Cache-Control
// ═══════════════════════════════════════════

/// 返回 JSON 字符串 + Cache-Control 头
/// max-age=5: CF 边缘缓存 5 秒（减少回源 80%+）
/// stale-while-revalidate=30: 缓存过期后 30 秒内先返回旧数据
fn cached_json_response(body: &str) -> HttpResponse {
    HttpResponse::Ok()
        .insert_header(("Content-Type", "application/json"))
        .insert_header(("Cache-Control", "public, max-age=5, stale-while-revalidate=30"))
        .body(body.to_string())
}

// ═══════════════════════════════════════════
//  HTTP API — 高频端点从缓存读取（零锁等待）
// ═══════════════════════════════════════════

/// GET /api/status — 从缓存读取，不锁 Indexer
async fn api_status(data: web::Data<Arc<AppState>>) -> HttpResponse {
    let cache = data.cache.read().unwrap();
    cached_json_response(&cache.status)
}

/// GET /api/holders — 从缓存读取
async fn api_holders(data: web::Data<Arc<AppState>>) -> HttpResponse {
    let cache = data.cache.read().unwrap();
    cached_json_response(&cache.holders)
}

/// GET /api/mints/recent — 从缓存读取
async fn api_mints_recent(data: web::Data<Arc<AppState>>) -> HttpResponse {
    let cache = data.cache.read().unwrap();
    cached_json_response(&cache.mints_recent)
}

/// GET /api/health — 从缓存读取
async fn api_health(data: web::Data<Arc<AppState>>) -> HttpResponse {
    let cache = data.cache.read().unwrap();
    cached_json_response(&cache.health)
}

// ═══════════════════════════════════════════
//  HTTP API — 低频端点（仍需锁 Indexer，但请求量小）
// ═══════════════════════════════════════════

async fn api_balance(data: web::Data<Arc<AppState>>, path: web::Path<String>) -> HttpResponse {
    let addr = path.into_inner();
    let indexer = data.indexer.lock().unwrap();
    let balance = indexer.balances.get(&addr).copied().unwrap_or(0);
    cached_json_response(&serde_json::to_string(&serde_json::json!({
        "address": addr, "balance": balance,
    })).unwrap())
}

async fn api_mint_by_seq(data: web::Data<Arc<AppState>>, path: web::Path<u32>) -> HttpResponse {
    let seq = path.into_inner();
    let indexer = data.indexer.lock().unwrap();
    match indexer.mints.iter().find(|m| m.seq == seq) {
        Some(m) => HttpResponse::Ok().json(m),
        None => HttpResponse::NotFound().json(serde_json::json!({"error": "not found"})),
    }
}

#[derive(Deserialize)]
struct PageQuery {
    page: Option<u32>,
    limit: Option<u32>,
}

async fn api_mints(data: web::Data<Arc<AppState>>, query: web::Query<PageQuery>) -> HttpResponse {
    let page = query.page.unwrap_or(1).max(1);
    let limit = query.limit.unwrap_or(20).min(100);
    let indexer = data.indexer.lock().unwrap();

    let total = indexer.mints.len() as u32;
    let start = ((page - 1) * limit) as usize;
    let end = (start + limit as usize).min(indexer.mints.len());

    let mints: Vec<_> = if start < indexer.mints.len() {
        indexer.mints[start..end].to_vec()
    } else {
        vec![]
    };

    HttpResponse::Ok().json(serde_json::json!({
        "page": page, "limit": limit, "total": total,
        "total_pages": (total + limit - 1) / limit,
        "mints": mints,
    }))
}

async fn api_tx(data: web::Data<Arc<AppState>>, path: web::Path<String>) -> HttpResponse {
    let txid = path.into_inner();
    let indexer = data.indexer.lock().unwrap();
    match indexer.mints.iter().find(|m| m.txid == txid) {
        Some(m) => HttpResponse::Ok().json(m),
        None => HttpResponse::NotFound().json(serde_json::json!({"error": "not found"})),
    }
}

/// GET /api/mints/address/{addr}
async fn api_mints_by_address(data: web::Data<Arc<AppState>>, path: web::Path<String>) -> HttpResponse {
    let addr = path.into_inner();
    let indexer = data.indexer.lock().unwrap();

    let mut mints: Vec<serde_json::Value> = indexer.mints.iter()
        .filter(|m| m.address == addr)
        .map(|m| serde_json::json!({
            "seq": m.seq, "address": m.address, "amount": m.amount,
            "reveal_txid": m.txid, "block_height": m.block_height,
        }))
        .collect();
    mints.reverse();

    let balance = indexer.balances.get(&addr).copied().unwrap_or(0);

    cached_json_response(&serde_json::to_string(&serde_json::json!({
        "address": addr, "balance": balance,
        "mint_count": mints.len(), "mints": mints,
    })).unwrap())
}

/// GET /api/mint/tx/{txid}
async fn api_mint_by_tx(data: web::Data<Arc<AppState>>, path: web::Path<String>) -> HttpResponse {
    let txid = path.into_inner();
    let indexer = data.indexer.lock().unwrap();
    match indexer.mints.iter().find(|m| m.txid == txid) {
        Some(m) => {
            HttpResponse::Ok().json(serde_json::json!({
                "mints": [{"seq": m.seq, "address": m.address, "amount": m.amount,
                    "reveal_txid": m.txid, "block_height": m.block_height}],
                "total": 1,
            }))
        }
        None => HttpResponse::NotFound().json(serde_json::json!({"error": "not found"})),
    }
}

// ═══════════════════════════════════════════
//  主函数
// ═══════════════════════════════════════════

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    println!();
    println!("  ╔════════════════════════════════════════╗");
    println!("  ║  NEXUS Indexer v2.9.2                  ║");
    println!("  ║  + Response Cache Layer                ║");
    println!("  ║  + Cache-Control for CF Edge           ║");
    println!("  ╚════════════════════════════════════════╝");
    println!();

    let config = node_detect::NexusConfig::load();
    let rpc_url = format!("http://127.0.0.1:8332");
    let rpc_user = config.rpc_user.clone();
    let rpc_pass = config.rpc_pass.clone();

    let (indexer, scan_height) = load_state();
    println!("  Loaded state: {} mints, scan height: {}", indexer.mints.len(), scan_height);

    let state = Arc::new(AppState {
        indexer: Mutex::new(indexer),
        scan_height: Mutex::new(scan_height),
        cache: RwLock::new(ResponseCache::empty()),
        rpc_url: rpc_url.clone(),
        rpc_user: rpc_user.clone(),
        rpc_pass: rpc_pass.clone(),
    });

    // 启动时立即填充缓存
    refresh_cache(&state);
    println!("  [cache] Initial cache populated");

    // 后台扫描线程
    let scan_state = state.clone();
    std::thread::spawn(move || {
        println!("  [scanner] Starting from block {}...", GENESIS_BLOCK);
        loop {
            scan_blocks(&scan_state);
            std::thread::sleep(std::time::Duration::from_secs(30));
        }
    });

    let http_state = state.clone();
    println!("  [http] Listening on http://0.0.0.0:3000");
    println!("  [http] Cache-Control: max-age=5, stale-while-revalidate=30");
    println!();

    HttpServer::new(move || {
        let cors = actix_cors::Cors::permissive();
        App::new()
            .wrap(cors)
            .wrap(middleware::Logger::new("%a %r %s %Dms"))
            // ── 高频端点（从缓存读取，零锁） ──
            .route("/api/status",   web::get().to(api_status))
            .route("/api/holders",  web::get().to(api_holders))
            .route("/api/mints/recent", web::get().to(api_mints_recent))
            .route("/api/health",   web::get().to(api_health))
            // ── 低频端点（仍需锁，但请求少） ──
            .route("/api/balance/{addr}", web::get().to(api_balance))
            .route("/api/mint/{seq}",     web::get().to(api_mint_by_seq))
            .route("/api/mints",          web::get().to(api_mints))
            .route("/api/tx/{txid}",      web::get().to(api_tx))
            .route("/api/mints/address/{addr}", web::get().to(api_mints_by_address))
            .route("/api/mint/tx/{txid}",       web::get().to(api_mint_by_tx))
            // ── 兼容旧路由 ──
            .route("/status",   web::get().to(api_status))
            .route("/holders",  web::get().to(api_holders))
            .route("/health",   web::get().to(api_health))
            .route("/balance/{addr}", web::get().to(api_balance))
            .route("/mint/{seq}",     web::get().to(api_mint_by_seq))
            .route("/mints",          web::get().to(api_mints))
            .route("/tx/{txid}",      web::get().to(api_tx))
            .app_data(web::Data::new(http_state.clone()))
    })
    .bind("0.0.0.0:3000")?
    .workers(4)  // ★ 4个工作线程，扛更多并发
    .run()
    .await
}
