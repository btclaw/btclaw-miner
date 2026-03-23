/// NEXUS 全节点证明模块
///
/// 三道防线:
/// 1. 磁盘验证 — 直接读blk*.dat，验证>500GB
/// 2. 两轮挑战 — 15秒窗口，本地~100ms，API~5-15s
/// 3. 深层切片 — 需要解析区块体结构
///
/// Bitcoin Core 30.x+ 支持: 自动检测 obfuscation key (XOR加密)

use sha2::{Sha256, Digest};
use serde::{Serialize, Deserialize};
use std::path::Path;
use std::io::{Read, Seek, SeekFrom};
use crate::constants::*;

// ═══════════════════════════════════════════
//  数据结构
// ═══════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TwoRoundProof {
    pub round1_hash: String,
    pub round1_ts: u64,
    pub round1_heights: Vec<u32>,
    pub round2_hash: String,
    pub round2_ts: u64,
    pub round2_heights: Vec<u32>,
    pub combined: String,
    pub block_hash: String,
    pub block_height: u32,
    pub pubkey: String,
}

// ═══════════════════════════════════════════
//  Obfuscation Key 检测 (Bitcoin Core 30.x+)
// ═══════════════════════════════════════════

/// 检测blk文件的XOR obfuscation key
///
/// Bitcoin Core 30.x 开始对blk*.dat文件使用8字节循环XOR加密
/// 旧版本 (< 30.0) 不加密，key全为0
///
/// 检测方法: 读blk00000.dat前4字节 XOR 已知mainnet magic = key前4字节
/// 然后读前8字节 XOR 期望的 magic+size = 完整8字节key
fn detect_obfuscation_key(blocks_dir: &Path) -> [u8; 8] {
    let blk0 = blocks_dir.join("blk00000.dat");
    let mut key = [0u8; 8];

    let mut f = match std::fs::File::open(&blk0) {
        Ok(f) => f,
        Err(_) => return key, // 无法读取，假设无加密
    };

    let mut buf = [0u8; 8];
    if f.read_exact(&mut buf).is_err() {
        return key;
    }

    // 前4字节 XOR mainnet magic
    let magic = BTC_MAINNET_MAGIC;
    key[0] = buf[0] ^ magic[0];
    key[1] = buf[1] ^ magic[1];
    key[2] = buf[2] ^ magic[2];
    key[3] = buf[3] ^ magic[3];

    // 如果前4字节key全为0，说明没加密（旧版Bitcoin Core）
    if key[0] == 0 && key[1] == 0 && key[2] == 0 && key[3] == 0 {
        return [0u8; 8];
    }

    // 后4字节: buf[4..8] XOR 后应该是block size (little-endian)
    // 第一个区块(genesis)的size是285字节 = 0x0000011D
    // 但更可靠的方式：用前4字节key的模式推导完整8字节
    // Bitcoin Core 的 obfuscation key 存在 LevelDB 里，我们这里从文件名提取
    // 简化方式：假设key是周期性的，尝试从debug.log或直接穷举

    // 尝试读LevelDB的obfuscation key
    let blocks_parent = blocks_dir.parent().unwrap_or(Path::new("/"));
    if let Some(full_key) = read_obfuscation_from_leveldb(blocks_parent) {
        return full_key;
    }

    // 备用：用已知genesis block size推导后4字节
    // genesis block raw size = 285 bytes = 0x1D010000 in LE
    let expected_size: [u8; 4] = [0x1D, 0x01, 0x00, 0x00];
    key[4] = buf[4] ^ expected_size[0];
    key[5] = buf[5] ^ expected_size[1];
    key[6] = buf[6] ^ expected_size[2];
    key[7] = buf[7] ^ expected_size[3];

    key
}

/// 从Bitcoin Core的LevelDB读取obfuscation key
fn read_obfuscation_from_leveldb(datadir: &Path) -> Option<[u8; 8]> {
    // 尝试从debug.log提取
    let log_path = datadir.join("debug.log");
    if let Ok(content) = std::fs::read_to_string(&log_path) {
        // 搜索 "Using obfuscation key for blocksdir *.dat files"
        for line in content.lines().rev() {
            if line.contains("obfuscation key for blocksdir") || line.contains("obfuscation key for /") {
                // 格式: ... obfuscation key ... : 'HEXHEXHEXHEX'
                if let Some(start) = line.rfind('\'') {
                    let after = &line[start + 1..];
                    if let Some(end) = after.find('\'') {
                        let hex_key = &after[..end];
                        if hex_key.len() == 16 {
                            if let Ok(bytes) = hex::decode(hex_key) {
                                if bytes.len() == 8 {
                                    let mut key = [0u8; 8];
                                    key.copy_from_slice(&bytes);
                                    return Some(key);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

/// 对数据应用XOR解密（8字节key循环）
fn xor_decrypt(data: &mut [u8], key: &[u8; 8], offset: u64) {
    if key == &[0u8; 8] { return; } // 无加密
    for (i, byte) in data.iter_mut().enumerate() {
        let key_idx = ((offset as usize + i) % 8) as usize;
        *byte ^= key[key_idx];
    }
}

/// 检查key是否为全零（未加密）
fn has_obfuscation(key: &[u8; 8]) -> bool {
    key != &[0u8; 8]
}

// ═══════════════════════════════════════════
//  第一道防线: 磁盘验证
// ═══════════════════════════════════════════

/// 验证本地是完整的BTC Full Archive Node
pub fn verify_full_node(datadir: &str) -> Result<(), String> {
    let blocks_dir = Path::new(datadir).join("blocks");

    if !blocks_dir.exists() {
        return Err(format!("{:?}/blocks 不存在", datadir));
    }

    // 扫描blk文件
    let mut total_size: u64 = 0;
    let mut file_count: u32 = 0;

    for entry in std::fs::read_dir(&blocks_dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with("blk") && name.ends_with(".dat") {
            total_size += entry.metadata().map_err(|e| e.to_string())?.len();
            file_count += 1;
        }
    }

    if total_size < MIN_BLOCKS_DIR_SIZE {
        return Err(format!(
            "区块数据 {:.1}GB 不足 {:.0}GB — 不是Full Archive Node",
            total_size as f64 / 1e9,
            MIN_BLOCKS_DIR_SIZE as f64 / 1e9
        ));
    }

    if file_count < MIN_BLK_FILE_COUNT {
        return Err(format!(
            "blk文件数 {} < {} — 可能是pruned节点",
            file_count, MIN_BLK_FILE_COUNT
        ));
    }

    // 验证早期文件存在
    #[cfg(not(feature = "regtest"))]
    let check_range = 0..10;
    #[cfg(feature = "regtest")]
    let check_range = 0..1;

    for i in check_range {
        let path = blocks_dir.join(format!("blk{:05}.dat", i));
        if !path.exists() {
            return Err(format!("缺少 blk{:05}.dat — pruned节点", i));
        }
    }

    // 检测obfuscation key
    let obf_key = detect_obfuscation_key(&blocks_dir);
    if has_obfuscation(&obf_key) {
        eprintln!("  [info] Detected obfuscation key: {}", hex::encode(obf_key));
    }

    // 验证blk00000.dat magic (带XOR解密)
    let mut f = std::fs::File::open(blocks_dir.join("blk00000.dat"))
        .map_err(|e| e.to_string())?;
    let mut magic = [0u8; 4];
    f.read_exact(&mut magic).map_err(|e| e.to_string())?;

    // XOR解密
    xor_decrypt(&mut magic, &obf_key, 0);

    if magic != BTC_MAINNET_MAGIC {
        return Err(format!(
            "blk00000.dat magic {:02X?} != mainnet {:02X?}",
            magic, BTC_MAINNET_MAGIC
        ));
    }

    Ok(())
}

/// 从blk文件直接读取指定位置的区块原始字节（支持obfuscation）
pub fn read_raw_block_from_disk(
    datadir: &str,
    file_number: u32,
    file_offset: u64,
) -> Result<Vec<u8>, String> {
    let blocks_dir = Path::new(datadir).join("blocks");
    let path = blocks_dir.join(format!("blk{:05}.dat", file_number));

    // 检测key
    let obf_key = detect_obfuscation_key(&blocks_dir);

    let mut f = std::fs::File::open(&path)
        .map_err(|e| format!("打开 {:?} 失败: {}", path, e))?;

    f.seek(SeekFrom::Start(file_offset))
        .map_err(|e| format!("seek失败: {}", e))?;

    // 读取magic + size
    let mut header = [0u8; 8];
    f.read_exact(&mut header).map_err(|e| format!("读header失败: {}", e))?;

    // XOR解密
    xor_decrypt(&mut header, &obf_key, file_offset);

    if header[0..4] != BTC_MAINNET_MAGIC {
        return Err("区块magic不匹配".into());
    }

    let block_size = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);

    let mut block = vec![0u8; block_size as usize];
    f.read_exact(&mut block).map_err(|e| format!("读区块数据失败: {}", e))?;

    // XOR解密区块数据
    xor_decrypt(&mut block, &obf_key, file_offset + 8);

    Ok(block)
}

/// 通过RPC获取raw block (主要使用方式)
pub fn read_raw_block_via_rpc(
    rpc_url: &str,
    rpc_user: &str,
    rpc_pass: &str,
    height: u32,
) -> Result<Vec<u8>, String> {
    let client = reqwest::blocking::Client::new();

    let hash_resp = rpc_call(&client, rpc_url, rpc_user, rpc_pass,
        "getblockhash", &[serde_json::json!(height)])?;
    let hash = hash_resp.as_str().ok_or("getblockhash非字符串")?;

    let raw_resp = rpc_call(&client, rpc_url, rpc_user, rpc_pass,
        "getblock", &[serde_json::json!(hash), serde_json::json!(0)])?;
    let raw_hex = raw_resp.as_str().ok_or("getblock非字符串")?;

    hex::decode(raw_hex).map_err(|e| format!("hex解码失败: {}", e))
}

fn rpc_call(
    client: &reqwest::blocking::Client,
    url: &str, user: &str, pass: &str,
    method: &str, params: &[serde_json::Value],
) -> Result<serde_json::Value, String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0", "id": "nexus",
        "method": method, "params": params,
    });
    let resp: serde_json::Value = client.post(url)
        .basic_auth(user, Some(pass))
        .json(&body).send().map_err(|e| e.to_string())?
        .json().map_err(|e| e.to_string())?;

    if let Some(err) = resp.get("error") {
        if !err.is_null() {
            return Err(format!("RPC error: {}", err));
        }
    }
    resp.get("result").cloned().ok_or("无result字段".into())
}

// ═══════════════════════════════════════════
//  第二道防线: 两轮挑战
// ═══════════════════════════════════════════

/// 生成两轮全节点证明
pub fn generate_proof(
    block_hash: &[u8; 32],
    block_hash_hex: &str,
    block_height: u32,
    pubkey: &[u8; 33],
    get_raw_block: &dyn Fn(u32) -> Result<Vec<u8>, String>,
) -> Result<TwoRoundProof, String> {
    let pubkey_hex = hex::encode(pubkey);

    // ── Round 1 ──
    let t1 = now();
    let seed1 = make_seed(block_hash, pubkey, b"r1");
    let heights1 = derive_heights(&seed1, block_height);
    let hash1 = compute_round(&seed1, &heights1, pubkey, get_raw_block)?;

    // ── Round 2 ──
    let hash1_bytes: [u8; 32] = hex::decode(&hash1)
        .map_err(|e| e.to_string())?.try_into().map_err(|_| "len")?;
    let seed2 = make_seed(&hash1_bytes, pubkey, b"r2");
    let heights2 = derive_heights(&seed2, block_height);
    let hash2 = compute_round(&seed2, &heights2, pubkey, get_raw_block)?;
    let t2 = now();

    if t2 - t1 > MAX_ROUND_GAP_SECS {
        return Err(format!(
            "两轮耗时{}秒 > {}秒限制。请确保数据在SSD上。",
            t2 - t1, MAX_ROUND_GAP_SECS
        ));
    }

    let combined = sha256_two(
        &hex::decode(&hash1).unwrap(),
        &hex::decode(&hash2).unwrap(),
    );

    Ok(TwoRoundProof {
        round1_hash: hash1, round1_ts: t1, round1_heights: heights1,
        round2_hash: hash2, round2_ts: t2, round2_heights: heights2,
        combined: hex::encode(combined),
        block_hash: block_hash_hex.into(),
        block_height, pubkey: pubkey_hex,
    })
}

/// 验证两轮证明 (Indexer端)
pub fn verify_proof(
    proof: &TwoRoundProof,
    get_raw_block: &dyn Fn(u32) -> Result<Vec<u8>, String>,
) -> Result<(), String> {
    let gap = proof.round2_ts.saturating_sub(proof.round1_ts);
    if gap > MAX_ROUND_GAP_SECS {
        return Err(format!("时间差{}s > {}s", gap, MAX_ROUND_GAP_SECS));
    }

    let pubkey: [u8; 33] = hex::decode(&proof.pubkey)
        .map_err(|e| e.to_string())?.try_into().map_err(|_| "pk len")?;
    let bh: [u8; 32] = hex::decode(&proof.block_hash)
        .map_err(|e| e.to_string())?.try_into().map_err(|_| "bh len")?;

    let seed1 = make_seed(&bh, &pubkey, b"r1");
    let exp_h1 = derive_heights(&seed1, proof.block_height);
    if exp_h1 != proof.round1_heights { return Err("R1 heights篡改".into()); }
    let exp_hash1 = compute_round(&seed1, &exp_h1, &pubkey, get_raw_block)?;
    if exp_hash1 != proof.round1_hash { return Err("R1 hash不匹配".into()); }

    let h1b: [u8; 32] = hex::decode(&proof.round1_hash).unwrap().try_into().unwrap();
    let seed2 = make_seed(&h1b, &pubkey, b"r2");
    let exp_h2 = derive_heights(&seed2, proof.block_height);
    if exp_h2 != proof.round2_heights { return Err("R2 heights篡改".into()); }
    let exp_hash2 = compute_round(&seed2, &exp_h2, &pubkey, get_raw_block)?;
    if exp_hash2 != proof.round2_hash { return Err("R2 hash不匹配".into()); }

    let exp_combined = hex::encode(sha256_two(
        &hex::decode(&proof.round1_hash).unwrap(),
        &hex::decode(&proof.round2_hash).unwrap(),
    ));
    if exp_combined != proof.combined { return Err("combined不匹配".into()); }

    Ok(())
}

// ═══════════════════════════════════════════
//  内部函数
// ═══════════════════════════════════════════

fn make_seed(a: &[u8; 32], b: &[u8; 33], domain: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(a); h.update(b); h.update(domain);
    h.finalize().into()
}

fn derive_heights(seed: &[u8; 32], max: u32) -> Vec<u32> {
    let mut out = Vec::with_capacity(CHALLENGES_PER_ROUND);
    let mut seen = std::collections::HashSet::new();
    let mut ctr = 0u32;
    while out.len() < CHALLENGES_PER_ROUND {
        let h = sha256_two(seed, &ctr.to_le_bytes());
        let val = (u32::from_le_bytes([h[0], h[1], h[2], h[3]])
            % max.saturating_sub(1)) + 1;
        if seen.insert(val) { out.push(val); }
        ctr += 1;
    }
    out
}

fn extract_slice(raw: &[u8], seed: &[u8; 32], height: u32) -> Result<[u8; 32], String> {
    if raw.len() < 113 {
        return Err(format!("区块{}太小: {}B", height, raw.len()));
    }
    let body = &raw[80..];

    let off_h = sha256_two(seed, &height.to_le_bytes());
    let off = u32::from_le_bytes([off_h[0], off_h[1], off_h[2], off_h[3]]) as usize
        % (body.len().saturating_sub(SLICE_SIZE).max(1));

    let end = (off + SLICE_SIZE).min(body.len());
    let mut slice = [0u8; 32];
    slice[..end - off].copy_from_slice(&body[off..end]);
    Ok(slice)
}

fn compute_round(
    seed: &[u8; 32],
    heights: &[u32],
    pubkey: &[u8; 33],
    get_raw_block: &dyn Fn(u32) -> Result<Vec<u8>, String>,
) -> Result<String, String> {
    let mut pre = Vec::with_capacity(CHALLENGES_PER_ROUND * SLICE_SIZE + 33 + 32);
    for &h in heights {
        let raw = get_raw_block(h)?;
        let slice = extract_slice(&raw, seed, h)?;
        pre.extend_from_slice(&slice);
    }
    pre.extend_from_slice(pubkey);
    pre.extend_from_slice(seed);
    Ok(hex::encode::<[u8; 32]>(Sha256::digest(&pre).into()))
}

fn sha256_two(a: &[u8], b: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(a); h.update(b);
    h.finalize().into()
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
}

// ═══════════════════════════════════════════
//  测试
// ═══════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_block(height: u32) -> Vec<u8> {
        let mut data = vec![0u8; 1000];
        for (i, b) in data.iter_mut().enumerate() {
            *b = ((height as usize * 7 + i * 13 + 42) % 256) as u8;
        }
        data
    }

    #[test]
    fn proof_roundtrip() {
        let bh = [0xAB; 32];
        let pk = [0x02; 33];
        let getter = |h: u32| -> Result<Vec<u8>, String> { Ok(mock_block(h)) };
        let proof = generate_proof(&bh, &hex::encode(bh), 10000, &pk, &getter).unwrap();
        assert!(verify_proof(&proof, &getter).is_ok());
    }

    #[test]
    fn proof_detects_tamper() {
        let bh = [0xAB; 32];
        let pk = [0x02; 33];
        let real = |h: u32| -> Result<Vec<u8>, String> { Ok(mock_block(h)) };
        let proof = generate_proof(&bh, &hex::encode(bh), 10000, &pk, &real).unwrap();
        let fake = |h: u32| -> Result<Vec<u8>, String> {
            let mut d = mock_block(h);
            d[100] ^= 0xFF;
            Ok(d)
        };
        assert!(verify_proof(&proof, &fake).is_err());
    }

    #[test]
    fn heights_no_duplicates() {
        let seed = [0xFF; 32];
        let heights = derive_heights(&seed, 900000);
        let set: std::collections::HashSet<_> = heights.iter().collect();
        assert_eq!(set.len(), heights.len());
    }

    #[test]
    fn heights_in_range() {
        let seed = [0x42; 32];
        let max = 941523;
        for &h in &derive_heights(&seed, max) {
            assert!(h >= 1 && h < max);
        }
    }

    #[test]
    fn xor_decrypt_roundtrip() {
        let key: [u8; 8] = [0x8e, 0x5d, 0xcd, 0xa8, 0xa6, 0x43, 0xbf, 0xc1];
        let original = vec![0xF9, 0xBE, 0xB4, 0xD9, 0x1D, 0x01, 0x00, 0x00];
        let mut encrypted = original.clone();
        xor_decrypt(&mut encrypted, &key, 0); // encrypt
        assert_ne!(encrypted, original);
        xor_decrypt(&mut encrypted, &key, 0); // decrypt back
        assert_eq!(encrypted, original);
    }
}
