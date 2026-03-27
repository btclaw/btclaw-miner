/// NEXUS Indexer Server v3.2 — 扫链 + REST API + Transfer + Batch Transfer + Reorg 检测
///
/// v3.2 Changes:
///   - Reorg 检测：每轮扫描前检查最近 6 个已扫区块 hash
///   - 自动回滚：reorg 发生时回滚 mints/transfers/余额到分叉点
///   - block_hashes 持久化：nexus_block_hashes.json
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
use nexus_reactor::proof::{verify_proof, read_raw_block_via_rpc};

/// v3.3: 此高度起强制要求完整全节点证明，之前的 mint 跳过验证
const PROOF_REQUIRED_FROM: u32 = 941950;

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
    batch_index: u32,
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
//  Block Hash 记录（v3.2 Reorg 检测用）
// ═══════════════════════════════════════════

const BLOCK_HASHES_FILE: &str = "nexus_block_hashes.json";

fn save_block_hashes(hashes: &HashMap<u32, String>) {
    if let Ok(j) = serde_json::to_string_pretty(hashes) {
        std::fs::write(BLOCK_HASHES_FILE, j).ok();
    }
}

fn load_block_hashes() -> HashMap<u32, String> {
    std::fs::read_to_string(BLOCK_HASHES_FILE).ok()
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
            health: r#"{"status":"starting","protocol":"NEXUS","version":"3.2"}"#.to_string(),
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
    block_hashes: Mutex<HashMap<u32, String>>,
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

    let mut status_val = serde_json::to_value(&indexer.status()).unwrap();
    status_val["scan_height"] = serde_json::json!(scan_h);
    status_val["total_transfers"] = serde_json::json!(transfers.len());
    let status_json = serde_json::to_string(&status_val).unwrap_or_default();

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

    let health_json = serde_json::to_string(&serde_json::json!({
        "status": "ok", "protocol": "NEXUS", "version": "3.2",
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
fn extract_inscription_body(script_hex: &str) -> Option<String> {
    let bytes: Vec<u8> = (0..script_hex.len() / 2)
        .filter_map(|i| u8::from_str_radix(&script_hex[i * 2..i * 2 + 2], 16).ok())
        .collect();

    // 找 OP_IF (0x63)
    let if_idx = bytes.iter().position(|&b| b == 0x63)?;
    let mut pos = if_idx + 1;
    let mut push_count = 0u32;
    let mut in_body = false;
    let mut body = Vec::new();

    while pos < bytes.len() {
        let b = bytes[pos];
        if b == 0x68 { break; } // OP_ENDIF

        // OP_0 after 3+ pushes = body separator
        if b == 0x00 && push_count >= 3 && !in_body {
            in_body = true;
            pos += 1;
            continue;
        }

        // 解析 push 操作
        let (data_start, data_len) = if b >= 0x01 && b <= 0x4b {
            // OP_PUSHBYTES_1..75: 直接 push
            (pos + 1, b as usize)
        } else if b == 0x4c && pos + 1 < bytes.len() {
            // OP_PUSHDATA1: 1字节长度
            (pos + 2, bytes[pos + 1] as usize)
        } else if b == 0x4d && pos + 2 < bytes.len() {
            // OP_PUSHDATA2: 2字节 LE 长度
            let len = u16::from_le_bytes([bytes[pos + 1], bytes[pos + 2]]) as usize;
            (pos + 3, len)
        } else if b == 0x4e && pos + 4 < bytes.len() {
            // OP_PUSHDATA4: 4字节 LE 长度
            let len = u32::from_le_bytes([bytes[pos + 1], bytes[pos + 2], bytes[pos + 3], bytes[pos + 4]]) as usize;
            (pos + 5, len)
        } else {
            pos += 1;
            continue;
        };

        if data_start + data_len > bytes.len() { break; }

        if in_body {
            body.extend_from_slice(&bytes[data_start..data_start + data_len]);
        }

        pos = data_start + data_len;
        push_count += 1;
    }

    if body.is_empty() { return None; }
    String::from_utf8(body).ok()
}

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
                        } else { continue; };
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
                if i == 0 { minter_address = addr.to_string(); }
                if addr == FEE_ADDRESS && sats >= MINT_FEE_SATS { fee_output_valid = true; }
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
                // 提取公钥（不变）
                if script_hex.len() >= 66 {
                    let pk_start = if &script_hex[..2] == "20" { 2 } else { 0 };
                    if script_hex.len() >= pk_start + 64 {
                        tx_pubkey = Some(script_hex[pk_start..pk_start + 64].to_string());
                    }
                }
                // v3.3: 提取铭文 body（支持多 chunk，兼容旧单 chunk）
                if let Some(body_str) = extract_inscription_body(script_hex) {
                    if body_str.contains("\"nexus\"") && body_str.contains("\"mint\"") {
                        witness_json = Some(body_str);
                    }
                }
            }
        }
    }

    witness_json.as_ref()?;

    Some(CandidateTx {
        txid, block_height, tx_index_in_block: tx_index, minter_address,
        witness_json, opreturn_bytes: opreturn_data, proof: None, fee_output_valid, tx_pubkey,
    })
}

// ═══════════════════════════════════════════
//  交易解析 — TRANSFER (单笔)
// ═══════════════════════════════════════════

struct PendingTransfer {
    txid: String, sender: String, recipient: String,
    amount: u64, block_height: u32, batch_index: u32,
}

fn parse_transfer(tx: &serde_json::Value, block_height: u32) -> Option<PendingTransfer> {
    let txid = tx["txid"].as_str()?.to_string();
    let vouts = tx["vout"].as_array()?;
    let mut transfer_amount: Option<u64> = None;

    for vout in vouts.iter() {
        let script_type = vout["scriptPubKey"]["type"].as_str().unwrap_or("");
        let hex_str = vout["scriptPubKey"]["hex"].as_str().unwrap_or("");
        if script_type == "nulldata" {
            if let Ok(script_bytes) = hex::decode(hex_str) {
                if script_bytes.len() > 4 && script_bytes[0] == 0x6a {
                    let data_start = if script_bytes[1] <= 75 { 2 }
                        else if script_bytes[1] == 0x4c && script_bytes.len() > 3 { 3 }
                        else { continue; };
                    if data_start < script_bytes.len() {
                        if let Ok(text) = std::str::from_utf8(&script_bytes[data_start..]) {
                            if text.starts_with("NXS:TRANSFER:") {
                                let parts: Vec<&str> = text.splitn(4, ':').collect();
                                if parts.len() >= 3 {
                                    if let Ok(amt) = parts[2].parse::<u64>() {
                                        if amt > 0 { transfer_amount = Some(amt); }
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
    let recipient = vouts.get(1)?["scriptPubKey"]["address"].as_str()?.to_string();
    if !recipient.starts_with("bc1p") && !recipient.starts_with("bc1q") { return None; }

    let vin0 = tx["vin"].as_array()?.first()?;
    let sender = vin0["prevout"]["scriptPubKey"]["address"].as_str()
        .map(|s| s.to_string()).unwrap_or_default();
    if sender.is_empty() || sender == recipient { return None; }

    Some(PendingTransfer { txid, sender, recipient, amount, block_height, batch_index: 0 })
}

// ═══════════════════════════════════════════
//  交易解析 — BATCH TRANSFER (v3.1)
// ═══════════════════════════════════════════

fn parse_batch_transfer(tx: &serde_json::Value, block_height: u32) -> Vec<PendingTransfer> {
    let txid = match tx["txid"].as_str() { Some(s) => s.to_string(), None => return vec![] };
    let vouts = match tx["vout"].as_array() { Some(v) => v, None => return vec![] };
    let vins = match tx["vin"].as_array() { Some(v) => v, None => return vec![] };

    let mut amounts: Vec<u64> = Vec::new();
    for vout in vouts.iter() {
        let script_type = vout["scriptPubKey"]["type"].as_str().unwrap_or("");
        let hex_str = vout["scriptPubKey"]["hex"].as_str().unwrap_or("");
        if script_type == "nulldata" {
            if let Ok(script_bytes) = hex::decode(hex_str) {
                if script_bytes.len() > 4 && script_bytes[0] == 0x6a {
                    let data_start = if script_bytes[1] <= 75 { 2 }
                        else if script_bytes[1] == 0x4c && script_bytes.len() > 3 { 3 }
                        else { continue; };
                    if data_start < script_bytes.len() {
                        if let Ok(text) = std::str::from_utf8(&script_bytes[data_start..]) {
                            if text.starts_with("NXS:BATCH:") {
                                for part in text[10..].split(',') {
                                    if let Ok(amt) = part.trim().parse::<u64>() {
                                        if amt > 0 { amounts.push(amt); }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    if amounts.is_empty() { return vec![]; }

    let n = amounts.len();
    let recipient = match vouts.get(n) {
        Some(vout) => match vout["scriptPubKey"]["address"].as_str() {
            Some(addr) if addr.starts_with("bc1p") || addr.starts_with("bc1q") => addr.to_string(),
            _ => return vec![],
        },
        None => return vec![],
    };

    let mut results = Vec::new();
    for i in 0..n {
        let sender = match vins.get(i) {
            Some(vin) => match vin["prevout"]["scriptPubKey"]["address"].as_str() {
                Some(addr) if !addr.is_empty() && addr != recipient => addr.to_string(),
                _ => { eprintln!("[scan] ❌ BATCH parse failed: vin[{}] invalid, tx {}", i, &txid[..12]); return vec![]; }
            },
            None => { eprintln!("[scan] ❌ BATCH parse failed: vin[{}] missing, tx {}", i, &txid[..12]); return vec![]; }
        };
        results.push(PendingTransfer {
            txid: txid.clone(), sender, recipient: recipient.clone(),
            amount: amounts[i], block_height, batch_index: i as u32,
        });
    }
    results
}

// ═══════════════════════════════════════════
//  区块扫描 — MINT + TRANSFER + BATCH + Reorg 检测
// ═══════════════════════════════════════════

fn scan_blocks(state: &AppState) {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build().unwrap();

    let chain_height = match get_block_count(&client, &state.rpc_url, &state.rpc_user, &state.rpc_pass) {
        Ok(h) => h,
        Err(e) => { eprintln!("[scan] getblockcount failed: {}", e); return; }
    };

    // ── v3.2: Reorg 检测 — 检查最近 6 个已扫区块的 hash ──
    let reorg_at: Option<u32> = {
        let block_hashes = state.block_hashes.lock().unwrap();
        let current_scan = *state.scan_height.lock().unwrap();
        let mut found: Option<u32> = None;

        let check_from = if current_scan > 6 { current_scan - 6 } else { GENESIS_BLOCK };
        for h in (check_from..=current_scan).rev() {
            if let Some(saved_hash) = block_hashes.get(&h) {
                match get_block_hash(&client, &state.rpc_url, &state.rpc_user, &state.rpc_pass, h) {
                    Ok(chain_hash) => {
                        if chain_hash != *saved_hash {
                            found = Some(h);
                        } else {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        }
        found
    };

    if let Some(reorg_height) = reorg_at {
        eprintln!("[scan] ⚠️ REORG detected at block {}! Rolling back...", reorg_height);

        let mut transfers = state.transfers.lock().unwrap();
        let mut indexer = state.indexer.lock().unwrap();

        // 回滚 transfers
        let removed_transfers: Vec<TransferRecord> = transfers.iter()
            .filter(|t| t.block_height >= reorg_height).cloned().collect();
        for t in &removed_transfers {
            *indexer.balances.entry(t.from.clone()).or_insert(0) += t.amount;
            *indexer.balances.entry(t.to.clone()).or_insert(0) = indexer.balances
                .get(&t.to).copied().unwrap_or(0).saturating_sub(t.amount);
            if indexer.balances.get(&t.to) == Some(&0) { indexer.balances.remove(&t.to); }
            eprintln!("[scan] ↩️ Reverted transfer {} NXS: {} → {} at block {}",
                t.amount, &t.from[..16.min(t.from.len())], &t.to[..16.min(t.to.len())], t.block_height);
        }
        transfers.retain(|t| t.block_height < reorg_height);

        // 回滚 mints
        let removed_mints: Vec<MintRecord> = indexer.mints.iter()
            .filter(|m| m.block_height >= reorg_height).cloned().collect();
        for m in &removed_mints {
            *indexer.balances.entry(m.address.clone()).or_insert(0) = indexer.balances
                .get(&m.address).copied().unwrap_or(0).saturating_sub(m.amount as u64);
            if indexer.balances.get(&m.address) == Some(&0) { indexer.balances.remove(&m.address); }
            indexer.used_proofs.remove(&m.proof_hash);
            indexer.minted = indexer.minted.saturating_sub(m.amount as u64);
            if indexer.next_seq > 0 { indexer.next_seq -= 1; }
            eprintln!("[scan] ↩️ Reverted mint #{} at block {}", m.seq, m.block_height);
        }
        indexer.mints.retain(|m| m.block_height < reorg_height);

        // 回滚 block_hashes + scan_height
        {
            let mut block_hashes = state.block_hashes.lock().unwrap();
            block_hashes.retain(|&h, _| h < reorg_height);
            save_block_hashes(&block_hashes);
        }
        *state.scan_height.lock().unwrap() = reorg_height - 1;
        save_state(&indexer, reorg_height - 1);
        save_transfers(&transfers);

        eprintln!("[scan] ✅ Rollback complete. Resuming from block {}", reorg_height);
        refresh_cache(state);
        return;
    }

    // ── 正常扫描 ──
    let current_scan = *state.scan_height.lock().unwrap();
    let start = if current_scan == 0 { GENESIS_BLOCK } else { current_scan + 1 };
    let end = chain_height.min(start + 100);

    if start > chain_height { refresh_cache(state); return; }

    let mut found_mints = 0u32;
    let mut found_transfers = 0u32;

    for height in start..=end {
        let hash = match get_block_hash(&client, &state.rpc_url, &state.rpc_user, &state.rpc_pass, height) {
            Ok(h) => h, Err(_) => continue,
        };

        // 记录区块 hash
        {
            let mut block_hashes = state.block_hashes.lock().unwrap();
            block_hashes.insert(height, hash.clone());
            if block_hashes.len() > 20 {
                let min_keep = if height > 20 { height - 20 } else { 0 };
                block_hashes.retain(|&h, _| h >= min_keep);
            }
        }

        let block = match get_block(&client, &state.rpc_url, &state.rpc_user, &state.rpc_pass, &hash) {
            Ok(b) => b, Err(_) => continue,
        };
        let txs = match block["tx"].as_array() { Some(t) => t, None => continue };

        // ── MINT ──
        let mut mint_candidates = Vec::new();
        for (tx_idx, tx) in txs.iter().enumerate() {
            if let Some(candidate) = parse_candidate(tx, height, tx_idx as u32) {
                mint_candidates.push(candidate);
            }
        }
        if !mint_candidates.is_empty() {
            let mut indexer = state.indexer.lock().unwrap();
            let get_raw = |h: u32| -> Result<Vec<u8>, String> {
                read_raw_block_via_rpc(&state.rpc_url, &state.rpc_user, &state.rpc_pass, h)
            };
            for candidate in mint_candidates {
                match light_validate(&indexer, &candidate, &get_raw) {
                    Ok(record) => {
                        println!("[scan] ✅ MINT #{} at block {} tx {}", record.seq, height, &record.txid[..12]);
                        indexer.confirm(record);
                        found_mints += 1;
                    }
                    Err(e) => { eprintln!("[scan] ❌ Invalid MINT at block {}: {}", height, e); }
                }
            }
        }

        // ── TRANSFER + BATCH ──
        let mut transfer_candidates: Vec<PendingTransfer> = Vec::new();
        for tx in txs.iter() {
            if let Some(pending) = parse_transfer(tx, height) {
                transfer_candidates.push(pending);
            } else {
                let batch = parse_batch_transfer(tx, height);
                if !batch.is_empty() { transfer_candidates.extend(batch); }
            }
        }
        if !transfer_candidates.is_empty() {
            let mut indexer = state.indexer.lock().unwrap();
            let mut transfers = state.transfers.lock().unwrap();
            for pending in transfer_candidates {
                if transfers.iter().any(|t| t.txid == pending.txid && t.batch_index == pending.batch_index) { continue; }
                let sender_balance = indexer.balances.get(&pending.sender).copied().unwrap_or(0);
                if sender_balance < pending.amount {
                    eprintln!("[scan] ❌ TRANSFER failed at block {}: {} has {} NXS, needs {}",
                        height, &pending.sender[..16], sender_balance, pending.amount);
                    continue;
                }
                *indexer.balances.entry(pending.sender.clone()).or_insert(0) -= pending.amount;
                *indexer.balances.entry(pending.recipient.clone()).or_insert(0) += pending.amount;
                if indexer.balances.get(&pending.sender) == Some(&0) { indexer.balances.remove(&pending.sender); }

                let record = TransferRecord {
                    txid: pending.txid.clone(), from: pending.sender.clone(), to: pending.recipient.clone(),
                    amount: pending.amount, block_height: pending.block_height, batch_index: pending.batch_index,
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
    { let transfers = state.transfers.lock().unwrap(); save_transfers(&transfers); }
    { let block_hashes = state.block_hashes.lock().unwrap(); save_block_hashes(&block_hashes); }

    refresh_cache(state);

    if found_mints > 0 || found_transfers > 0 {
        println!("[scan] Scanned {} → {}, mints: {}, transfers: {}", start, end, found_mints, found_transfers);
    }
}

fn light_validate(
    indexer: &Indexer,
    tx: &CandidateTx,
    get_raw_block: &dyn Fn(u32) -> Result<Vec<u8>, String>,
) -> Result<MintRecord, String> {
    if indexer.minted >= MAX_SUPPLY { return Err("supply exhausted".into()); }

    let witness_json = tx.witness_json.as_ref().ok_or("no witness")?;
    let wit: WitnessPayload = serde_json::from_str(witness_json)
        .map_err(|e| format!("JSON parse: {}", e))?;
    if wit.p != "nexus" || wit.op != "mint" || wit.amt != MINT_AMOUNT {
        return Err("field mismatch".into());
    }
    if wit.pk.is_empty() { return Err("missing pk".into()); }
    if let Some(ref tx_pk) = tx.tx_pubkey {
        if wit.pk != *tx_pk { return Err(format!("pk mismatch: {} vs {}", &wit.pk[..16], &tx_pk[..16])); }
    }

    let opr_bytes = tx.opreturn_bytes.as_ref().ok_or("no opreturn")?;
    let opr = OpReturnData::from_bytes(opr_bytes).ok_or("bad opreturn")?;
    if opr.magic != "NXS" || opr.op != "MINT" { return Err("bad magic/op".into()); }

    verify_interlock(witness_json, opr_bytes)?;
    if !tx.fee_output_valid { return Err("fee invalid".into()); }
    if indexer.used_proofs.contains_key(&wit.fnp) { return Err("proof already used".into()); }

    // ═══ v3.3 规则6: 全节点证明独立验证（941950+ 强制）═══
    if tx.block_height >= PROOF_REQUIRED_FROM {
        let proof = wit.proof.as_ref()
            .ok_or(format!("missing proof data (required from block {})", PROOF_REQUIRED_FROM))?;
        // 6a. 预检查（轻量，防 DoS）
        if proof.round1_heights.len() != CHALLENGES_PER_ROUND
            || proof.round2_heights.len() != CHALLENGES_PER_ROUND {
            return Err("proof heights count mismatch".into());
        }
        if proof.round2_ts.saturating_sub(proof.round1_ts) > MAX_ROUND_GAP_SECS {
            return Err(format!("proof time gap {}s > {}s limit",
                proof.round2_ts - proof.round1_ts, MAX_ROUND_GAP_SECS));
        }
        if proof.combined.len() != 64 || proof.pubkey.len() != 66 {
            return Err("proof field length invalid".into());
        }
        // 6b. fnp 必须等于 proof.combined
        if proof.combined != wit.fnp {
            return Err(format!("proof.combined != fnp: {} vs {}", &proof.combined[..16], &wit.fnp[..16]));
        }
        // 6c. 完整密码学验证 — indexer 用自己的全节点重新计算两轮哈希
        verify_proof(proof, get_raw_block)?;
        println!("    [proof] ✅ Full node proof verified for block {}", proof.block_height);
    }

    Ok(MintRecord {
        seq: indexer.next_seq, txid: tx.txid.clone(), address: tx.minter_address.clone(),
        amount: MINT_AMOUNT, block_height: tx.block_height, proof_hash: wit.fnp.clone(),
    })
}

const GENESIS_BLOCK: u32 = 941890;

// ═══════════════════════════════════════════
//  HTTP
// ═══════════════════════════════════════════

fn cached_json_response(body: &str) -> HttpResponse {
    HttpResponse::Ok()
        .insert_header(("Content-Type", "application/json"))
        .insert_header(("Cache-Control", "public, max-age=5, stale-while-revalidate=30"))
        .body(body.to_string())
}

async fn api_status(data: web::Data<Arc<AppState>>) -> HttpResponse { cached_json_response(&data.cache.read().unwrap().status) }
async fn api_holders(data: web::Data<Arc<AppState>>) -> HttpResponse { cached_json_response(&data.cache.read().unwrap().holders) }
async fn api_mints_recent(data: web::Data<Arc<AppState>>) -> HttpResponse { cached_json_response(&data.cache.read().unwrap().mints_recent) }
async fn api_health(data: web::Data<Arc<AppState>>) -> HttpResponse { cached_json_response(&data.cache.read().unwrap().health) }

async fn api_balance(data: web::Data<Arc<AppState>>, path: web::Path<String>) -> HttpResponse {
    let addr = path.into_inner();
    let indexer = data.indexer.lock().unwrap();
    let balance = indexer.balances.get(&addr).copied().unwrap_or(0);
    cached_json_response(&serde_json::to_string(&serde_json::json!({"address": addr, "balance": balance})).unwrap())
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
struct PageQuery { page: Option<u32>, limit: Option<u32> }

async fn api_mints(data: web::Data<Arc<AppState>>, query: web::Query<PageQuery>) -> HttpResponse {
    let page = query.page.unwrap_or(1).max(1);
    let limit = query.limit.unwrap_or(20).min(100);
    let indexer = data.indexer.lock().unwrap();
    let total = indexer.mints.len() as u32;
    let start = ((page - 1) * limit) as usize;
    let end = (start + limit as usize).min(indexer.mints.len());
    let mints: Vec<_> = if start < indexer.mints.len() { indexer.mints[start..end].to_vec() } else { vec![] };
    HttpResponse::Ok().json(serde_json::json!({"page": page, "limit": limit, "total": total, "total_pages": (total + limit - 1) / limit, "mints": mints}))
}

async fn api_tx(data: web::Data<Arc<AppState>>, path: web::Path<String>) -> HttpResponse {
    let txid = path.into_inner();
    let indexer = data.indexer.lock().unwrap();
    match indexer.mints.iter().find(|m| m.txid == txid) {
        Some(m) => HttpResponse::Ok().json(m),
        None => HttpResponse::NotFound().json(serde_json::json!({"error": "not found"})),
    }
}

async fn api_mints_by_address(data: web::Data<Arc<AppState>>, path: web::Path<String>) -> HttpResponse {
    let addr = path.into_inner();
    let indexer = data.indexer.lock().unwrap();
    let mut mints: Vec<serde_json::Value> = indexer.mints.iter()
        .filter(|m| m.address == addr)
        .map(|m| serde_json::json!({"seq": m.seq, "address": m.address, "amount": m.amount, "reveal_txid": m.txid, "block_height": m.block_height}))
        .collect();
    mints.reverse();
    let balance = indexer.balances.get(&addr).copied().unwrap_or(0);
    cached_json_response(&serde_json::to_string(&serde_json::json!({"address": addr, "balance": balance, "mint_count": mints.len(), "mints": mints})).unwrap())
}

async fn api_mint_by_tx(data: web::Data<Arc<AppState>>, path: web::Path<String>) -> HttpResponse {
    let txid = path.into_inner();
    let indexer = data.indexer.lock().unwrap();
    match indexer.mints.iter().find(|m| m.txid == txid) {
        Some(m) => HttpResponse::Ok().json(serde_json::json!({"mints": [{"seq": m.seq, "address": m.address, "amount": m.amount, "reveal_txid": m.txid, "block_height": m.block_height}], "total": 1})),
        None => HttpResponse::NotFound().json(serde_json::json!({"error": "not found"})),
    }
}

async fn api_transfers_by_address(data: web::Data<Arc<AppState>>, path: web::Path<String>) -> HttpResponse {
    let addr = path.into_inner();
    let transfers = data.transfers.lock().unwrap();
    let records: Vec<serde_json::Value> = transfers.iter()
        .filter(|t| t.from == addr || t.to == addr)
        .map(|t| serde_json::json!({"txid": t.txid, "from": t.from, "to": t.to, "amount": t.amount, "block_height": t.block_height, "batch_index": t.batch_index, "type": if t.from == addr { "sent" } else { "received" }}))
        .collect();
    let mut records_rev = records;
    records_rev.reverse();
    cached_json_response(&serde_json::to_string(&serde_json::json!({"address": addr, "transfers": records_rev, "total": records_rev.len()})).unwrap())
}

// ═══════════════════════════════════════════
//  主函数
// ═══════════════════════════════════════════

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    println!();
    println!("  ╔════════════════════════════════════════╗");
    println!("  ║  NEXUS Indexer v3.2                    ║");
    println!("  ║  + Transfer Support (NXS:TRANSFER)     ║");
    println!("  ║  + Batch Transfer (NXS:BATCH)          ║");
    println!("  ║  + Reorg Detection & Auto-Rollback     ║");
    println!("  ║  + Response Cache + CF Edge            ║");
    println!("  ╚════════════════════════════════════════╝");
    println!();

    let config = node_detect::NexusConfig::load();
    let rpc_url = format!("http://127.0.0.1:8332");
    let rpc_user = config.rpc_user.clone();
    let rpc_pass = config.rpc_pass.clone();

    let (indexer, scan_height) = load_state();
    let transfers = load_transfers();
    let block_hashes = load_block_hashes();
    println!("  Loaded state: {} mints, {} transfers, {} block hashes, scan height: {}",
        indexer.mints.len(), transfers.len(), block_hashes.len(), scan_height);

    let state = Arc::new(AppState {
        indexer: Mutex::new(indexer),
        scan_height: Mutex::new(scan_height),
        cache: RwLock::new(ResponseCache::empty()),
        transfers: Mutex::new(transfers),
        block_hashes: Mutex::new(block_hashes),
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
            .route("/api/status",   web::get().to(api_status))
            .route("/api/holders",  web::get().to(api_holders))
            .route("/api/mints/recent", web::get().to(api_mints_recent))
            .route("/api/health",   web::get().to(api_health))
            .route("/api/balance/{addr}", web::get().to(api_balance))
            .route("/api/mint/{seq}",     web::get().to(api_mint_by_seq))
            .route("/api/mints",          web::get().to(api_mints))
            .route("/api/tx/{txid}",      web::get().to(api_tx))
            .route("/api/mints/address/{addr}", web::get().to(api_mints_by_address))
            .route("/api/mint/tx/{txid}",       web::get().to(api_mint_by_tx))
            .route("/api/transfers/address/{addr}", web::get().to(api_transfers_by_address))
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
