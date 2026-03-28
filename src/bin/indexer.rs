use std::sync::{Arc, RwLock};
use actix_web::{web, App, HttpServer, HttpResponse, middleware};
use serde::Deserialize;

use nexus_reactor::constants::*;
use nexus_reactor::indexer::*;
use nexus_reactor::transaction::*;
use nexus_reactor::db::{NexusDb, TransferRecord};
use nexus_reactor::node_detect;
use nexus_reactor::proof::{verify_proof, read_raw_block_via_rpc};

/// Blocks before this height skip full proof verification
const PROOF_REQUIRED_FROM: u32 = 941950;

/// First block to scan for NEXUS transactions
const GENESIS_BLOCK: u32 = 941890;

/// SQLite database file path
const DB_PATH: &str = "nexus_indexer.db";

/// Legacy JSON file paths (for migration)
const STATE_FILE: &str = "nexus_indexer_state.json";
const TRANSFERS_FILE: &str = "nexus_transfers.json";
const SCAN_FILE: &str = "nexus_indexer_scan.json";
const BLOCK_HASHES_FILE: &str = "nexus_block_hashes.json";

// ═══════════════════════════════════════════
//  Response Cache (5-second TTL)
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
            status: "{}".into(),
            holders: r#"{"holders":[],"total_holders":0}"#.into(),
            mints_recent: r#"{"mints":[],"total":0}"#.into(),
            health: r#"{"status":"starting","protocol":"NEXUS","version":"4.0"}"#.into(),
        }
    }
}

// ═══════════════════════════════════════════
//  Application State
// ═══════════════════════════════════════════

struct AppState {
    db: Arc<NexusDb>,
    indexer: Indexer,
    cache: RwLock<ResponseCache>,
    rpc_url: String,
    rpc_user: String,
    rpc_pass: String,
}

// ═══════════════════════════════════════════
//  Cache Refresh — reads from SQLite
// ═══════════════════════════════════════════

fn refresh_cache(state: &AppState) {
    let db = &state.db;
    let scan_h = db.get_scan_height();
    let transfer_count = db.get_transfer_count();

    // Status
    let status = state.indexer.status();
    let mut status_val = serde_json::to_value(&status).unwrap();
    status_val["scan_height"] = serde_json::json!(scan_h);
    status_val["total_transfers"] = serde_json::json!(transfer_count);
    let status_json = serde_json::to_string(&status_val).unwrap_or_default();

    // Holders (top 100)
    let holders = db.get_holders(100);
    let holders_list: Vec<serde_json::Value> = holders.iter().map(|(addr, bal)| {
        let mint_count = db.get_mint_count_by_address(addr);
        serde_json::json!({"address": addr, "balance": bal, "mint_count": mint_count})
    }).collect();
    let holders_json = serde_json::to_string(&serde_json::json!({
        "total_holders": holders_list.len(),
        "holders": holders_list,
    })).unwrap_or_default();

    // Recent mints (last 20)
    let recent = db.get_mints_recent(20);
    let total = db.get_mint_count();
    let mints_list: Vec<serde_json::Value> = recent.iter().map(|m| {
        serde_json::json!({
            "seq": m.seq, "address": m.address, "amount": m.amount,
            "reveal_txid": m.txid, "block_height": m.block_height,
        })
    }).collect();
    let mints_json = serde_json::to_string(&serde_json::json!({
        "mints": mints_list, "total": total,
    })).unwrap_or_default();

    // Health
    let health_json = serde_json::to_string(&serde_json::json!({
        "status": "ok", "protocol": "NEXUS", "version": "4.0",
        "scan_height": scan_h, "total_transfers": transfer_count,
    })).unwrap_or_default();

    let mut cache = state.cache.write().unwrap();
    cache.status = status_json;
    cache.holders = holders_json;
    cache.mints_recent = mints_json;
    cache.health = health_json;
}

// ═══════════════════════════════════════════
//  Bitcoin RPC Helpers
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

// ═══════════════════════════════════════════
//  Transaction Parsing — MINT
// ═══════════════════════════════════════════

fn extract_inscription_body(script_hex: &str) -> Option<String> {
    let bytes: Vec<u8> = (0..script_hex.len() / 2)
        .filter_map(|i| u8::from_str_radix(&script_hex[i * 2..i * 2 + 2], 16).ok())
        .collect();

    let if_idx = bytes.iter().position(|&b| b == 0x63)?;
    let mut pos = if_idx + 1;
    let mut push_count = 0u32;
    let mut in_body = false;
    let mut body = Vec::new();

    while pos < bytes.len() {
        let b = bytes[pos];
        if b == 0x68 { break; }

        if b == 0x00 && push_count >= 3 && !in_body {
            in_body = true;
            pos += 1;
            continue;
        }

        let (data_start, data_len) = if b >= 0x01 && b <= 0x4b {
            (pos + 1, b as usize)
        } else if b == 0x4c && pos + 1 < bytes.len() {
            (pos + 2, bytes[pos + 1] as usize)
        } else if b == 0x4d && pos + 2 < bytes.len() {
            let len = u16::from_le_bytes([bytes[pos + 1], bytes[pos + 2]]) as usize;
            (pos + 3, len)
        } else if b == 0x4e && pos + 4 < bytes.len() {
            let len = u32::from_le_bytes([bytes[pos + 1], bytes[pos + 2], bytes[pos + 3], bytes[pos + 4]]) as usize;
            (pos + 5, len)
        } else {
            pos += 1;
            continue;
        };

        if data_start + data_len > bytes.len() { break; }
        if in_body { body.extend_from_slice(&bytes[data_start..data_start + data_len]); }
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
                        let data_start = if script_bytes.len() > 2 && script_bytes[1] <= 75 { 2 }
                            else if script_bytes.len() > 3 && script_bytes[1] == 0x4c { 3 }
                            else { continue; };
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
                if script_hex.len() >= 66 {
                    let pk_start = if &script_hex[..2] == "20" { 2 } else { 0 };
                    if script_hex.len() >= pk_start + 64 {
                        tx_pubkey = Some(script_hex[pk_start..pk_start + 64].to_string());
                    }
                }
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
//  Transaction Parsing — TRANSFER
// ═══════════════════════════════════════════

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
//  Transaction Parsing — BATCH TRANSFER
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
                _ => { eprintln!("[scan] BATCH vin[{}] invalid, tx {}", i, &txid[..12]); return vec![]; }
            },
            None => { eprintln!("[scan] BATCH vin[{}] missing, tx {}", i, &txid[..12]); return vec![]; }
        };
        results.push(PendingTransfer {
            txid: txid.clone(), sender, recipient: recipient.clone(),
            amount: amounts[i], block_height, batch_index: i as u32,
        });
    }
    results
}

// ═══════════════════════════════════════════
//  Mint Validation (scanner version)
// ═══════════════════════════════════════════

fn light_validate(
    db: &NexusDb,
    tx: &CandidateTx,
    get_raw_block: &dyn Fn(u32) -> Result<Vec<u8>, String>,
) -> Result<MintRecord, String> {
    let minted = db.get_minted();
    let next_seq = db.get_next_seq();

    if minted >= MAX_SUPPLY { return Err("supply exhausted".into()); }

    let witness_json = tx.witness_json.as_ref().ok_or("no witness")?;
    let wit: WitnessPayload = serde_json::from_str(witness_json)
        .map_err(|e| format!("JSON parse: {}", e))?;
    if wit.p != "nexus" || wit.op != "mint" || wit.amt != MINT_AMOUNT {
        return Err("field mismatch".into());
    }
    if wit.pk.is_empty() { return Err("missing pk".into()); }
    if let Some(ref tx_pk) = tx.tx_pubkey {
        if wit.pk != *tx_pk {
            return Err(format!("pk mismatch: {} vs {}", &wit.pk[..16], &tx_pk[..16]));
        }
    }

    let opr_bytes = tx.opreturn_bytes.as_ref().ok_or("no opreturn")?;
    let opr = OpReturnData::from_bytes(opr_bytes).ok_or("bad opreturn")?;
    if opr.magic != "NXS" || opr.op != "MINT" { return Err("bad magic/op".into()); }

    verify_interlock(witness_json, opr_bytes)?;
    if !tx.fee_output_valid { return Err("fee invalid".into()); }

    // Replay protection — check DB
    if db.has_proof(&wit.fnp) { return Err("proof already used".into()); }

    // Full node proof verification (required from block 941950+)
    if tx.block_height >= PROOF_REQUIRED_FROM {
        let proof = wit.proof.as_ref()
            .ok_or(format!("missing proof (required from block {})", PROOF_REQUIRED_FROM))?;

        if proof.round1_heights.len() != CHALLENGES_PER_ROUND
            || proof.round2_heights.len() != CHALLENGES_PER_ROUND {
            return Err("proof heights count mismatch".into());
        }
        if proof.round2_ts.saturating_sub(proof.round1_ts) > MAX_ROUND_GAP_SECS {
            return Err(format!("proof time gap {}s exceeds {}s limit",
                proof.round2_ts - proof.round1_ts, MAX_ROUND_GAP_SECS));
        }
        if proof.combined.len() != 64 || proof.pubkey.len() != 66 {
            return Err("proof field length invalid".into());
        }
        if proof.combined != wit.fnp {
            return Err(format!("proof.combined != fnp"));
        }

        verify_proof(proof, get_raw_block)?;
        println!("    [proof] ✅ Verified for block {}", proof.block_height);
    }

    Ok(MintRecord {
        seq: next_seq,
        txid: tx.txid.clone(),
        address: tx.minter_address.clone(),
        amount: MINT_AMOUNT,
        block_height: tx.block_height,
        proof_hash: wit.fnp.clone(),
    })
}

// ═══════════════════════════════════════════
//  Block Scanner — writes directly to SQLite
// ═══════════════════════════════════════════

fn scan_blocks(state: &AppState) {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build().unwrap();

    let db = &state.db;

    let chain_height = match get_block_count(&client, &state.rpc_url, &state.rpc_user, &state.rpc_pass) {
        Ok(h) => h,
        Err(e) => { eprintln!("[scan] getblockcount failed: {}", e); return; }
    };

    // ── Reorg Detection ──
    let current_scan = db.get_scan_height();
    let check_from = if current_scan > 6 { current_scan - 6 } else { GENESIS_BLOCK };
    let mut reorg_at: Option<u32> = None;

    for h in (check_from..=current_scan).rev() {
        if let Some(saved_hash) = db.get_block_hash(h) {
            match get_block_hash(&client, &state.rpc_url, &state.rpc_user, &state.rpc_pass, h) {
                Ok(chain_hash) => {
                    if chain_hash != saved_hash { reorg_at = Some(h); }
                    else { break; }
                }
                Err(_) => break,
            }
        }
    }

    if let Some(reorg_height) = reorg_at {
        eprintln!("[scan] ⚠️ REORG at block {}! Rolling back...", reorg_height);
        let (rolled_mints, rolled_transfers) = db.rollback_to_height(reorg_height);
        for m in &rolled_mints {
            eprintln!("[scan] ↩️ Reverted mint #{} at block {}", m.seq, m.block_height);
        }
        for t in &rolled_transfers {
            eprintln!("[scan] ↩️ Reverted transfer {} NXS at block {}", t.amount, t.block_height);
        }
        eprintln!("[scan] ✅ Rollback complete, resuming from block {}", reorg_height);
        refresh_cache(state);
        return;
    }

    // ── Normal Scan ──
    let start = if current_scan == 0 { GENESIS_BLOCK } else { current_scan + 1 };
    let end = chain_height.min(start + 100);

    if start > chain_height { refresh_cache(state); return; }

    let mut found_mints = 0u32;
    let mut found_transfers = 0u32;

    for height in start..=end {
        let hash = match get_block_hash(&client, &state.rpc_url, &state.rpc_user, &state.rpc_pass, height) {
            Ok(h) => h, Err(_) => continue,
        };

        // Record block hash (keep last 20)
        db.set_block_hash(height, &hash);
        if height > 20 { db.cleanup_block_hashes(height - 20); }

        let block = match get_block(&client, &state.rpc_url, &state.rpc_user, &state.rpc_pass, &hash) {
            Ok(b) => b, Err(_) => continue,
        };
        let txs = match block["tx"].as_array() { Some(t) => t, None => continue };

        // ── Process MINTs ──
        for (tx_idx, tx) in txs.iter().enumerate() {
            if let Some(candidate) = parse_candidate(tx, height, tx_idx as u32) {
                let get_raw = |h: u32| -> Result<Vec<u8>, String> {
                    read_raw_block_via_rpc(&state.rpc_url, &state.rpc_user, &state.rpc_pass, h)
                };
                match light_validate(db, &candidate, &get_raw) {
                    Ok(record) => {
                        println!("[scan] ✅ MINT #{} at block {} tx {}",
                            record.seq, height, &record.txid[..12]);
                        state.indexer.confirm(record);
                        found_mints += 1;
                    }
                    Err(e) => { eprintln!("[scan] ❌ Invalid MINT at block {}: {}", height, e); }
                }
            }
        }

        // ── Process TRANSFERs ──
        let mut transfer_candidates: Vec<PendingTransfer> = Vec::new();
        for tx in txs.iter() {
            if let Some(pending) = parse_transfer(tx, height) {
                transfer_candidates.push(pending);
            } else {
                let batch = parse_batch_transfer(tx, height);
                if !batch.is_empty() { transfer_candidates.extend(batch); }
            }
        }

        for pending in transfer_candidates {
            // Dedup check
            if db.has_transfer(&pending.txid, pending.batch_index) { continue; }

            let sender_balance = db.get_balance(&pending.sender);
            if sender_balance < pending.amount {
                eprintln!("[scan] ❌ TRANSFER failed at block {}: {} has {} NXS, needs {}",
                    height, &pending.sender[..16.min(pending.sender.len())],
                    sender_balance, pending.amount);
                continue;
            }

            // Update balances
            db.sub_balance(&pending.sender, pending.amount);
            db.add_balance(&pending.recipient, pending.amount);

            // Record transfer
            let record = TransferRecord {
                txid: pending.txid.clone(),
                from: pending.sender.clone(),
                to: pending.recipient.clone(),
                amount: pending.amount,
                block_height: pending.block_height,
                batch_index: pending.batch_index,
            };
            db.add_transfer(&record);

            println!("[scan] ✅ TRANSFER {} NXS: {} → {} at block {} [batch#{}]",
                pending.amount,
                &pending.sender[..16.min(pending.sender.len())],
                &pending.recipient[..16.min(pending.recipient.len())],
                height, pending.batch_index);
            found_transfers += 1;
        }

        db.set_scan_height(height);
    }

    refresh_cache(state);

    if found_mints > 0 || found_transfers > 0 {
        println!("[scan] Scanned {} → {}, mints: {}, transfers: {}", start, end, found_mints, found_transfers);
    }
}

// ═══════════════════════════════════════════
//  HTTP API Handlers
// ═══════════════════════════════════════════

fn json_response(body: &str) -> HttpResponse {
    HttpResponse::Ok()
        .insert_header(("Content-Type", "application/json"))
        .insert_header(("Cache-Control", "public, max-age=5, stale-while-revalidate=30"))
        .body(body.to_string())
}

async fn api_status(data: web::Data<Arc<AppState>>) -> HttpResponse {
    json_response(&data.cache.read().unwrap().status)
}

async fn api_holders(data: web::Data<Arc<AppState>>) -> HttpResponse {
    json_response(&data.cache.read().unwrap().holders)
}

async fn api_mints_recent(data: web::Data<Arc<AppState>>) -> HttpResponse {
    json_response(&data.cache.read().unwrap().mints_recent)
}

async fn api_health(data: web::Data<Arc<AppState>>) -> HttpResponse {
    json_response(&data.cache.read().unwrap().health)
}

async fn api_balance(data: web::Data<Arc<AppState>>, path: web::Path<String>) -> HttpResponse {
    let addr = path.into_inner();
    let balance = data.db.get_balance(&addr);
    json_response(&serde_json::to_string(
        &serde_json::json!({"address": addr, "balance": balance})
    ).unwrap())
}

async fn api_mint_by_seq(data: web::Data<Arc<AppState>>, path: web::Path<u32>) -> HttpResponse {
    let seq = path.into_inner();
    match data.db.get_mint_by_seq(seq) {
        Some(m) => HttpResponse::Ok().json(m),
        None => HttpResponse::NotFound().json(serde_json::json!({"error": "not found"})),
    }
}

#[derive(Deserialize)]
struct PageQuery { page: Option<u32>, limit: Option<u32> }

async fn api_mints(data: web::Data<Arc<AppState>>, query: web::Query<PageQuery>) -> HttpResponse {
    let page = query.page.unwrap_or(1).max(1);
    let limit = query.limit.unwrap_or(20).min(100);
    let (mints, total) = data.db.get_mints_page(page, limit);
    HttpResponse::Ok().json(serde_json::json!({
        "page": page, "limit": limit, "total": total,
        "total_pages": (total + limit - 1) / limit, "mints": mints
    }))
}

async fn api_tx(data: web::Data<Arc<AppState>>, path: web::Path<String>) -> HttpResponse {
    let txid = path.into_inner();
    match data.db.get_mint_by_txid(&txid) {
        Some(m) => HttpResponse::Ok().json(m),
        None => HttpResponse::NotFound().json(serde_json::json!({"error": "not found"})),
    }
}

async fn api_mints_by_address(data: web::Data<Arc<AppState>>, path: web::Path<String>) -> HttpResponse {
    let addr = path.into_inner();
    let mints = data.db.get_mints_by_address(&addr);
    let balance = data.db.get_balance(&addr);
    let mints_list: Vec<serde_json::Value> = mints.iter().map(|m| {
        serde_json::json!({
            "seq": m.seq, "address": m.address, "amount": m.amount,
            "reveal_txid": m.txid, "block_height": m.block_height
        })
    }).collect();
    json_response(&serde_json::to_string(&serde_json::json!({
        "address": addr, "balance": balance,
        "mint_count": mints_list.len(), "mints": mints_list
    })).unwrap())
}

async fn api_mint_by_tx(data: web::Data<Arc<AppState>>, path: web::Path<String>) -> HttpResponse {
    let txid = path.into_inner();
    match data.db.get_mint_by_txid(&txid) {
        Some(m) => HttpResponse::Ok().json(serde_json::json!({
            "mints": [{"seq": m.seq, "address": m.address, "amount": m.amount,
                       "reveal_txid": m.txid, "block_height": m.block_height}],
            "total": 1
        })),
        None => HttpResponse::NotFound().json(serde_json::json!({"error": "not found"})),
    }
}

async fn api_transfers_by_address(data: web::Data<Arc<AppState>>, path: web::Path<String>) -> HttpResponse {
    let addr = path.into_inner();
    let transfers = data.db.get_transfers_by_address(&addr);
    let records: Vec<serde_json::Value> = transfers.iter().map(|t| {
        serde_json::json!({
            "txid": t.txid, "from": t.from, "to": t.to,
            "amount": t.amount, "block_height": t.block_height,
            "batch_index": t.batch_index,
            "type": if t.from == addr { "sent" } else { "received" }
        })
    }).collect();
    json_response(&serde_json::to_string(&serde_json::json!({
        "address": addr, "transfers": records, "total": records.len()
    })).unwrap())
}

// ═══════════════════════════════════════════
//  Main
// ═══════════════════════════════════════════

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    println!();
    println!("  ╔════════════════════════════════════════╗");
    println!("  ║  NEXUS Indexer v4.0                    ║");
    println!("  ║  SQLite backend · Zero-restart edits   ║");
    println!("  ║  Transfer + Batch + Reorg Detection    ║");
    println!("  ╚════════════════════════════════════════╝");
    println!();

    // Initialize database
    let db = Arc::new(NexusDb::open(DB_PATH)
        .expect("Failed to open database"));

    // Auto-migrate from JSON if this is the first run
    db.migrate_from_json(STATE_FILE, TRANSFERS_FILE, SCAN_FILE, BLOCK_HASHES_FILE);

    let scan_height = db.get_scan_height();
    let mint_count = db.get_mint_count();
    let transfer_count = db.get_transfer_count();
    let holder_count = db.get_holder_count();
    println!("  Database: {}", DB_PATH);
    println!("  State: {} mints, {} transfers, {} holders, scan height: {}",
        mint_count, transfer_count, holder_count, scan_height);

    // Load RPC config
    let config = node_detect::NexusConfig::load();
    let rpc_url = "http://127.0.0.1:8332".to_string();
    let rpc_user = config.rpc_user.clone();
    let rpc_pass = config.rpc_pass.clone();

    // Build application state
    let indexer = Indexer::new(db.clone());
    let state = Arc::new(AppState {
        db: db.clone(),
        indexer,
        cache: RwLock::new(ResponseCache::empty()),
        rpc_url,
        rpc_user,
        rpc_pass,
    });

    refresh_cache(&state);
    println!("  [cache] Ready");

    // Start scanner thread
    let scan_state = state.clone();
    std::thread::spawn(move || {
        println!("  [scanner] Starting from block {}...", GENESIS_BLOCK);
        loop {
            let before = scan_state.db.get_scan_height();
            scan_blocks(&scan_state);
            let after = scan_state.db.get_scan_height();
            if after - before >= 100 {
                println!("[scan] Catching up... height: {}", after);
                continue;
            }
            std::thread::sleep(std::time::Duration::from_secs(30));
        }
    });

    // Start HTTP server
    let http_state = state.clone();
    println!("  [http] Listening on http://0.0.0.0:3000");
    println!();

    HttpServer::new(move || {
        let cors = actix_cors::Cors::permissive();
        App::new()
            .wrap(cors)
            .wrap(middleware::Logger::new("%a %r %s %Dms"))
            .route("/api/status",                   web::get().to(api_status))
            .route("/api/holders",                  web::get().to(api_holders))
            .route("/api/mints/recent",             web::get().to(api_mints_recent))
            .route("/api/health",                   web::get().to(api_health))
            .route("/api/balance/{addr}",           web::get().to(api_balance))
            .route("/api/mint/{seq}",               web::get().to(api_mint_by_seq))
            .route("/api/mints",                    web::get().to(api_mints))
            .route("/api/tx/{txid}",                web::get().to(api_tx))
            .route("/api/mints/address/{addr}",     web::get().to(api_mints_by_address))
            .route("/api/mint/tx/{txid}",           web::get().to(api_mint_by_tx))
            .route("/api/transfers/address/{addr}", web::get().to(api_transfers_by_address))
            // Short aliases
            .route("/status",          web::get().to(api_status))
            .route("/holders",         web::get().to(api_holders))
            .route("/health",          web::get().to(api_health))
            .route("/balance/{addr}",  web::get().to(api_balance))
            .route("/mint/{seq}",      web::get().to(api_mint_by_seq))
            .route("/mints",           web::get().to(api_mints))
            .route("/tx/{txid}",       web::get().to(api_tx))
            .app_data(web::Data::new(http_state.clone()))
    })
    .bind("0.0.0.0:3000")?
    .workers(4)
    .run()
    .await
}
