/// NEXUS Indexer Server v3.1 — 扫链 + REST API + Transfer + Batch Transfer 支持
///
/// v3.1 Changes:
///   - NXS:BATCH:<amt1>,<amt2>,... 批量 Transfer 解析
///   - parse_batch_transfer: 每个 vin[i] → amounts[i], recipient 从 OUTPUT[N]
///   - TransferRecord 新增 batch_index 字段
///   - 去重逻辑按 (txid, batch_index) 组合判断
///
/// v3.0 Changes:
///   - NXS:TRANSFER OP_RETURN 扫描 + 余额更新
///   - TransferRecord 持久化
///   - GET /api/transfers/address/{addr} 端点
///   - balance = minted - sent + received
///
/// API端点:
///   GET /api/status              — 铸造进度、供应量、持有者数
///   GET /api/balance/:addr       — 地址NXS余额
///   GET /api/mint/:seq           — 按序号查铸造记录
///   GET /api/mints?page=1&limit=20 — 分页铸造列表
///   GET /api/mints/recent        — 最近铸造记录
///   GET /api/mints/address/:addr — 按地址查铸造+余额
///   GET /api/holders             — 持有者排行
///   GET /api/tx/:txid            — 按txid查铸造记录
///   GET /api/transfers/address/:addr — 按地址查转账记录
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
//  Transfer 记录
// ═══════════════════════════════════════════

#[derive(Serialize, Deserialize, Clone, Debug)]
struct TransferRecord {
    txid: String,
    from: String,
    to: String,
    amount: u64,
    block_height: u32,
    #[serde(default)]
    batch_index: u32,  // 0=单笔 transfer, 0..N-1=批量中的序号
}

const TRANSFERS_FILE: &str = "nexus_transfers.json";

fn save_transfers(transfers: &[TransferRecord]) {
    if let Ok(j) = serde_json::to_string_pretty(transfers) {
        std::fs::write(TRANSFERS_FILE, j).ok();
    }
}

fn load_transfers() -> Vec<TransferRecord> {
    std::fs::read_to_string(TRANSFERS_FILE).ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

// ═══════════════════════════════════════════
//  响应缓存
// ═══════════════════════════════════════════

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
            health: r#"{"status":"starting","protocol":"NEXUS","version":"3.1"}"#.to_string(),
        }
    }
}

// ═══════════════════════════════════════════
//  状态
// ═══════════════════════════════════════════

struct AppState {
    indexer: Mutex<Indexer>,
    scan_height: Mutex<u32>,
    cache: RwLock<ResponseCache>,
    transfers: Mutex<Vec<TransferRecord>>,
    rpc_url: String,
    rpc_user: String,
    rpc_pass: String,
}

// ═══════════════════════════════════════════
//  缓存刷新
// ═══════════════════════════════════════════

fn refresh_cache(state: &AppState) {
    let indexer = state.indexer.lock().unwrap();
    let scan_h = *state.scan_height.lock().unwrap();
    let transfers = state.transfers.lock().unwrap();

    // status
    let mut status_val = serde_json::to_value(&indexer.status()).unwrap();
    status_val["scan_height"] = serde_json::json!(scan_h);
    status_val["total_transfers"] = serde_json::json!(transfers.len());
    let status_json = serde_json::to_string(&status_val).unwrap_or_default();

    // holders (top 100) — balances already reflect transfers
    let mut holders_list: Vec<serde_json::Value> = indexer.balances.iter()
        .filter(|(_, &bal)| bal > 0)
        .map(|(addr, &bal)| {
            let mint_count = indexer.mints.iter().filter(|m| m.address == *addr).count();
            serde_json::json!({"address": addr, "balance": bal, "mint_count": mint_count})
        })
        .collect();
    holders_list.sort_by(|a, b| b["balance"].as_u64().cmp(&a["balance"].as_u64()));
    holders_list.truncate(100);
    let holders_json = serde_json::to_string(&serde_json::json!({
        "total_holders": holders_list.len(),
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
        "status": "ok", "protocol": "NEXUS", "version": "3.1",
        "scan_height": scan_h, "total_transfers": transfers.len(),
    })).unwrap_or_default();

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
    rpc_json(client, url, user, pass, "getblock", &[serde_json::json!(hash), serde_json::json!(3)])
}

fn get_raw_tx(client: &reqwest::blocking::Client, url: &str, user: &str, pass: &str, txid: &str) -> Result<serde_json::Value, String> {
    rpc_json(client, url, user, pass, "getrawtransaction", &[serde_json::json!(txid), serde_json::json!(true)])
}

// ═══════════════════════════════════════════
//  交易解析 — MINT
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
                            if text.starts_with("NXS:MINT:") {
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
//  交易解析 — TRANSFER (单笔)
// ═══════════════════════════════════════════

/// 解析 NXS:TRANSFER:<amount> 格式的 OP_RETURN
struct PendingTransfer {
    txid: String,
    sender: String,
    recipient: String,
    amount: u64,
    block_height: u32,
    batch_index: u32,
}

fn parse_transfer(tx: &serde_json::Value, block_height: u32) -> Option<PendingTransfer> {
    let txid = tx["txid"].as_str()?.to_string();
    let vouts = tx["vout"].as_array()?;

    // 1. 找 NXS:TRANSFER OP_RETURN 并解析 amount
    let mut transfer_amount: Option<u64> = None;

    for vout in vouts.iter() {
        let script_type = vout["scriptPubKey"]["type"].as_str().unwrap_or("");
        let hex_str = vout["scriptPubKey"]["hex"].as_str().unwrap_or("");

        if script_type == "nulldata" {
            if let Ok(script_bytes) = hex::decode(hex_str) {
                if script_bytes.len() > 4 && script_bytes[0] == 0x6a {
                    let data_start = if script_bytes[1] <= 75 {
                        2
                    } else if script_bytes[1] == 0x4c && script_bytes.len() > 3 {
                        3
                    } else {
                        continue;
                    };
                    if data_start < script_bytes.len() {
                        if let Ok(text) = std::str::from_utf8(&script_bytes[data_start..]) {
                            // 只匹配 NXS:TRANSFER:, 不匹配 NXS:BATCH:
                            if text.starts_with("NXS:TRANSFER:") {
                                // 支持两种格式:
                                // 新: NXS:TRANSFER:500
                                // 旧: NXS:TRANSFER:500:to=bc1p...
                                let parts: Vec<&str> = text.splitn(4, ':').collect();
                                if parts.len() >= 3 {
                                    if let Ok(amt) = parts[2].parse::<u64>() {
                                        if amt > 0 {
                                            transfer_amount = Some(amt);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    let amount = transfer_amount?;

    // 2. 获取 recipient — 从 output[1] 的地址读取（买家 NXS 标记 output）
    let recipient = vouts.get(1)?["scriptPubKey"]["address"].as_str()?.to_string();
    if !recipient.starts_with("bc1p") && !recipient.starts_with("bc1q") {
        return None;
    }

    // 3. 获取 sender — 从 vin[0] 的 prevout 获取
    let vin0 = tx["vin"].as_array()?.first()?;
    let sender = vin0["prevout"]["scriptPubKey"]["address"].as_str()
        .map(|s| s.to_string())
        .unwrap_or_default();

    if sender.is_empty() { return None; }
    if sender == recipient { return None; }

    Some(PendingTransfer {
        txid,
        sender,
        recipient,
        amount,
        block_height,
        batch_index: 0, // 单笔 transfer
    })
}

// ═══════════════════════════════════════════
//  交易解析 — BATCH TRANSFER (新增 v3.1)
// ═══════════════════════════════════════════

/// 解析 NXS:BATCH:<amt1>,<amt2>,... 格式的 OP_RETURN
/// 每个金额对应 vin[i] 的卖家, recipient 从 OUTPUT[N] 读取（N = 金额数量）
fn parse_batch_transfer(tx: &serde_json::Value, block_height: u32) -> Vec<PendingTransfer> {
    let txid = match tx["txid"].as_str() {
        Some(s) => s.to_string(),
        None => return vec![],
    };
    let vouts = match tx["vout"].as_array() {
        Some(v) => v,
        None => return vec![],
    };
    let vins = match tx["vin"].as_array() {
        Some(v) => v,
        None => return vec![],
    };

    // 1. 找 NXS:BATCH OP_RETURN 并解析金额列表
    let mut amounts: Vec<u64> = Vec::new();

    for vout in vouts.iter() {
        let script_type = vout["scriptPubKey"]["type"].as_str().unwrap_or("");
        let hex_str = vout["scriptPubKey"]["hex"].as_str().unwrap_or("");

        if script_type == "nulldata" {
            if let Ok(script_bytes) = hex::decode(hex_str) {
                if script_bytes.len() > 4 && script_bytes[0] == 0x6a {
                    let data_start = if script_bytes[1] <= 75 {
                        2
                    } else if script_bytes[1] == 0x4c && script_bytes.len() > 3 {
                        3
                    } else {
                        continue;
                    };
                    if data_start < script_bytes.len() {
                        if let Ok(text) = std::str::from_utf8(&script_bytes[data_start..]) {
                            if text.starts_with("NXS:BATCH:") {
                                let amounts_str = &text[10..]; // skip "NXS:BATCH:"
                                for part in amounts_str.split(',') {
                                    if let Ok(amt) = part.trim().parse::<u64>() {
                                        if amt > 0 {
                                            amounts.push(amt);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    if amounts.is_empty() {
        return vec![];
    }

    let n = amounts.len();

    // 2. recipient = OUTPUT[N]（N 个卖家 output 之后的第一个 = 买家 marker）
    let recipient = match vouts.get(n) {
        Some(vout) => match vout["scriptPubKey"]["address"].as_str() {
            Some(addr) if addr.starts_with("bc1p") || addr.starts_with("bc1q") => addr.to_string(),
            _ => return vec![],
        },
        None => return vec![],
    };

    // 3. 每个 vin[i] 的 prevout = sender_i
    let mut results = Vec::new();
    for i in 0..n {
        let sender = match vins.get(i) {
            Some(vin) => match vin["prevout"]["scriptPubKey"]["address"].as_str() {
                Some(addr) if !addr.is_empty() && addr != recipient => addr.to_string(),
                _ => {
                    eprintln!("[scan] ❌ BATCH parse failed: vin[{}] has no valid sender address, tx {}",
                        i, &txid[..12]);
                    return vec![]; // 任何一个 sender 无效则整个 batch 无效
                }
            },
            None => {
                eprintln!("[scan] ❌ BATCH parse failed: vin[{}] missing, tx {}", i, &txid[..12]);
                return vec![];
            }
        };

        results.push(PendingTransfer {
            txid: txid.clone(),
            sender,
            recipient: recipient.clone(),
            amount: amounts[i],
            block_height,
            batch_index: i as u32,
        });
    }

    results
}

// ═══════════════════════════════════════════
//  区块扫描 — 同时处理 MINT + TRANSFER + BATCH
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
        refresh_cache(state);
        return;
    }

    let mut found_mints = 0u32;
    let mut found_transfers = 0u32;

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

        // ── 处理 MINT 候选 ──
        let mut mint_candidates = Vec::new();
        for (tx_idx, tx) in txs.iter().enumerate() {
            if let Some(candidate) = parse_candidate(tx, height, tx_idx as u32) {
                mint_candidates.push(candidate);
            }
        }

        if !mint_candidates.is_empty() {
            let mut indexer = state.indexer.lock().unwrap();
            for candidate in mint_candidates {
                match light_validate(&indexer, &candidate) {
                    Ok(record) => {
                        println!("[scan] ✅ MINT #{} at block {} tx {}",
                            record.seq, height, &record.txid[..12]);
                        indexer.confirm(record);
                        found_mints += 1;
                    }
                    Err(e) => {
                        eprintln!("[scan] ❌ Invalid MINT at block {}: {}", height, e);
                    }
                }
            }
        }

        // ── 处理 TRANSFER + BATCH 候选 ──
        let mut transfer_candidates: Vec<PendingTransfer> = Vec::new();
        for tx in txs.iter() {
            // 先尝试单笔 TRANSFER 解析
            if let Some(pending) = parse_transfer(tx, height) {
                transfer_candidates.push(pending);
            } else {
                // 不是单笔 TRANSFER，尝试 BATCH 解析
                let batch = parse_batch_transfer(tx, height);
                if !batch.is_empty() {
                    transfer_candidates.extend(batch);
                }
            }
        }

        if !transfer_candidates.is_empty() {
            let mut indexer = state.indexer.lock().unwrap();
            let mut transfers = state.transfers.lock().unwrap();

            for pending in transfer_candidates {
                // 去重：按 (txid, batch_index) 组合判断（防止重复扫描）
                if transfers.iter().any(|t| t.txid == pending.txid && t.batch_index == pending.batch_index) {
                    continue;
                }

                // 验证 sender 余额
                let sender_balance = indexer.balances.get(&pending.sender).copied().unwrap_or(0);
                if sender_balance < pending.amount {
                    eprintln!("[scan] ❌ TRANSFER failed at block {}: {} has {} NXS, needs {}",
                        height, &pending.sender[..16], sender_balance, pending.amount);
                    continue;
                }

                // 更新余额
                *indexer.balances.entry(pending.sender.clone()).or_insert(0) -= pending.amount;
                *indexer.balances.entry(pending.recipient.clone()).or_insert(0) += pending.amount;

                // 清理零余额
                if indexer.balances.get(&pending.sender) == Some(&0) {
                    indexer.balances.remove(&pending.sender);
                }

                let record = TransferRecord {
                    txid: pending.txid.clone(),
                    from: pending.sender.clone(),
                    to: pending.recipient.clone(),
                    amount: pending.amount,
                    block_height: pending.block_height,
                    batch_index: pending.batch_index,
                };

                println!("[scan] ✅ TRANSFER {} NXS: {} → {} at block {} tx {} [batch#{}]",
                    pending.amount, &pending.sender[..16], &pending.recipient[..16],
                    height, &pending.txid[..12], pending.batch_index);

                transfers.push(record);
                found_transfers += 1;
            }
        }

        *state.scan_height.lock().unwrap() = height;
    }

    // 保存状态
    {
        let indexer = state.indexer.lock().unwrap();
        let scan_h = *state.scan_height.lock().unwrap();
        save_state(&indexer, scan_h);
    }
    {
        let transfers = state.transfers.lock().unwrap();
        save_transfers(&transfers);
    }

    refresh_cache(state);

    if found_mints > 0 || found_transfers > 0 {
        println!("[scan] Scanned {} → {}, mints: {}, transfers: {}",
            start, end, found_mints, found_transfers);
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
//  HTTP 响应辅助
// ═══════════════════════════════════════════

fn cached_json_response(body: &str) -> HttpResponse {
    HttpResponse::Ok()
        .insert_header(("Content-Type", "application/json"))
        .insert_header(("Cache-Control", "public, max-age=5, stale-while-revalidate=30"))
        .body(body.to_string())
}

// ═══════════════════════════════════════════
//  HTTP API — 高频端点
// ═══════════════════════════════════════════

async fn api_status(data: web::Data<Arc<AppState>>) -> HttpResponse {
    let cache = data.cache.read().unwrap();
    cached_json_response(&cache.status)
}

async fn api_holders(data: web::Data<Arc<AppState>>) -> HttpResponse {
    let cache = data.cache.read().unwrap();
    cached_json_response(&cache.holders)
}

async fn api_mints_recent(data: web::Data<Arc<AppState>>) -> HttpResponse {
    let cache = data.cache.read().unwrap();
    cached_json_response(&cache.mints_recent)
}

async fn api_health(data: web::Data<Arc<AppState>>) -> HttpResponse {
    let cache = data.cache.read().unwrap();
    cached_json_response(&cache.health)
}

// ═══════════════════════════════════════════
//  HTTP API — 低频端点
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

/// GET /api/mints/address/{addr} — 包含 transfer 后的真实余额
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

    // balance 已经包含 transfer 的影响
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

/// GET /api/transfers/address/{addr} — 查某地址的转账记录
async fn api_transfers_by_address(data: web::Data<Arc<AppState>>, path: web::Path<String>) -> HttpResponse {
    let addr = path.into_inner();
    let transfers = data.transfers.lock().unwrap();

    let records: Vec<serde_json::Value> = transfers.iter()
        .filter(|t| t.from == addr || t.to == addr)
        .map(|t| serde_json::json!({
            "txid": t.txid,
            "from": t.from,
            "to": t.to,
            "amount": t.amount,
            "block_height": t.block_height,
            "batch_index": t.batch_index,
            "type": if t.from == addr { "sent" } else { "received" },
        }))
        .collect();

    let mut records_rev = records;
    records_rev.reverse();

    cached_json_response(&serde_json::to_string(&serde_json::json!({
        "address": addr,
        "transfers": records_rev,
        "total": records_rev.len(),
    })).unwrap())
}

// ═══════════════════════════════════════════
//  主函数
// ═══════════════════════════════════════════

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    println!();
    println!("  ╔════════════════════════════════════════╗");
    println!("  ║  NEXUS Indexer v3.1                    ║");
    println!("  ║  + Transfer Support (NXS:TRANSFER)     ║");
    println!("  ║  + Batch Transfer (NXS:BATCH)          ║");
    println!("  ║  + Response Cache + CF Edge            ║");
    println!("  ╚════════════════════════════════════════╝");
    println!();

    let config = node_detect::NexusConfig::load();
    let rpc_url = format!("http://127.0.0.1:8332");
    let rpc_user = config.rpc_user.clone();
    let rpc_pass = config.rpc_pass.clone();

    let (indexer, scan_height) = load_state();
    let transfers = load_transfers();
    println!("  Loaded state: {} mints, {} transfers, scan height: {}",
        indexer.mints.len(), transfers.len(), scan_height);

    // 重建余额：先从 indexer 自带的 balances，然后应用 transfers
    // (如果 indexer 的 balances 已经包含 transfer 影响则跳过)

    let state = Arc::new(AppState {
        indexer: Mutex::new(indexer),
        scan_height: Mutex::new(scan_height),
        cache: RwLock::new(ResponseCache::empty()),
        transfers: Mutex::new(transfers),
        rpc_url: rpc_url.clone(),
        rpc_user: rpc_user.clone(),
        rpc_pass: rpc_pass.clone(),
    });

    refresh_cache(&state);
    println!("  [cache] Initial cache populated");

    let scan_state = state.clone();
    std::thread::spawn(move || {
        println!("  [scanner] Starting from block {}...", GENESIS_BLOCK);
        loop {
            let before = *scan_state.scan_height.lock().unwrap();
            scan_blocks(&scan_state);
            let after = *scan_state.scan_height.lock().unwrap();

            if after - before >= 100 {
                println!("[scan] Catching up... height: {}", after);
                continue;
            }
            std::thread::sleep(std::time::Duration::from_secs(30));
        }
    });

    let http_state = state.clone();
    println!("  [http] Listening on http://0.0.0.0:3000");
    println!();

    HttpServer::new(move || {
        let cors = actix_cors::Cors::permissive();
        App::new()
            .wrap(cors)
            .wrap(middleware::Logger::new("%a %r %s %Dms"))
            // ── 高频端点（缓存） ──
            .route("/api/status",   web::get().to(api_status))
            .route("/api/holders",  web::get().to(api_holders))
            .route("/api/mints/recent", web::get().to(api_mints_recent))
            .route("/api/health",   web::get().to(api_health))
            // ── 低频端点 ──
            .route("/api/balance/{addr}", web::get().to(api_balance))
            .route("/api/mint/{seq}",     web::get().to(api_mint_by_seq))
            .route("/api/mints",          web::get().to(api_mints))
            .route("/api/tx/{txid}",      web::get().to(api_tx))
            .route("/api/mints/address/{addr}", web::get().to(api_mints_by_address))
            .route("/api/mint/tx/{txid}",       web::get().to(api_mint_by_tx))
            // ── Transfer 端点 ──
            .route("/api/transfers/address/{addr}", web::get().to(api_transfers_by_address))
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
    .workers(4)
    .run()
    .await
}
