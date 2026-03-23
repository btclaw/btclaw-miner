/// NEXUS 双层互锁交易构造
///
/// Witness层(铭文) ←→ OP_RETURN层 互相包含对方的SHA256
///
/// OP_RETURN格式 (v2 可读ASCII):
///   NXS:1:w=<16 hex>:p=<16 hex>
///   约43字节，区块浏览器直接可读

use sha2::{Sha256, Digest};
use serde::{Serialize, Deserialize};
use crate::constants::*;
use crate::proof::TwoRoundProof;

// ═══════════════════════════════════════════
//  数据结构
// ═══════════════════════════════════════════

/// Witness层铭文JSON
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WitnessPayload {
    pub p: String,        // "nexus"
    pub op: String,       // "mint"
    pub amt: u64,         // 500
    pub pk: String,       // minter x-only pubkey (64 hex)
    pub fnp: String,      // combined proof hash (full 64 hex)
    pub opr: String,      // SHA256(OP_RETURN bytes) (full 64 hex)
}

/// OP_RETURN数据 (ASCII可读格式)
///
/// 格式: NXS:1:w=<witness_hash前16hex>:p=<proof_hash前16hex>
/// 例如: NXS:1:w=e2fa8baedf7b7a13:p=ea3af2ee3ac4bdd1
///
/// 完整hash在Witness JSON铭文层中，OP_RETURN做可读标识
#[derive(Debug, Clone)]
pub struct OpReturnData {
    pub magic: String,              // "NXS"
    pub version: u8,                // 1
    pub witness_hash_short: String, // 前16 hex字符 (8字节)
    pub proof_hash_short: String,   // 前16 hex字符 (8字节)
    // 用于互锁验证的完整hash（不写入OP_RETURN，仅内部使用）
    pub witness_hash_full: [u8; 32],
    pub proof_hash_full: [u8; 32],
}

impl OpReturnData {
    /// 序列化为ASCII可读格式
    /// 输出: NXS:1:w=abcdef0123456789:p=fedcba9876543210
    pub fn to_bytes(&self) -> Vec<u8> {
        let text = format!("NXS:{}:w={}:p={}",
            self.version,
            self.witness_hash_short,
            self.proof_hash_short,
        );
        text.into_bytes()
    }

    /// 从ASCII文本解析
    pub fn from_bytes(d: &[u8]) -> Option<Self> {
        let text = std::str::from_utf8(d).ok()?;
        if !text.starts_with("NXS:") { return None; }

        let parts: Vec<&str> = text.split(':').collect();
        if parts.len() < 4 { return None; }

        // parts[0] = "NXS"
        // parts[1] = version (e.g. "1")
        // parts[2] = "w=<16hex>"
        // parts[3] = "p=<16hex>"

        let version: u8 = parts[1].parse().ok()?;
        let wit_hash = parts[2].strip_prefix("w=")?;
        let proof_hash = parts[3].strip_prefix("p=")?;

        if wit_hash.len() != 16 || proof_hash.len() != 16 { return None; }

        Some(Self {
            magic: "NXS".into(),
            version,
            witness_hash_short: wit_hash.to_string(),
            proof_hash_short: proof_hash.to_string(),
            witness_hash_full: [0u8; 32], // 解析时无完整hash
            proof_hash_full: [0u8; 32],
        })
    }
}

// ═══════════════════════════════════════════
//  互锁构造
// ═══════════════════════════════════════════

/// 构造结果
#[derive(Debug, Clone)]
pub struct InterlockResult {
    pub witness_json: String,
    pub witness_hash: [u8; 32],
    pub opreturn_bytes: Vec<u8>,
    pub opreturn_hash: [u8; 32],
}

/// 构造双层互锁数据
///
/// 单向确定方案 (不需要迭代):
/// 1. 先构造witness(opr=""), 算其hash → 写入opreturn
/// 2. 构造完整opreturn, 算其hash → 写入witness.opr
/// 验证时: 将witness.opr替换为""再算hash, 与opreturn中的前缀比对
pub fn build_interlock(proof: &TwoRoundProof, pubkey_hex: &str) -> Result<InterlockResult, String> {
    let proof_bytes: [u8; 32] = hex::decode(&proof.combined)
        .map_err(|e| e.to_string())?.try_into().map_err(|_| "len")?;

    // Step 1: witness with empty opr
    let wit_core = WitnessPayload {
        p: "nexus".into(),
        op: "mint".into(),
        amt: MINT_AMOUNT,
        pk: pubkey_hex.to_string(),
        fnp: proof.combined.clone(),
        opr: String::new(),
    };
    let wit_core_json = serde_json::to_string(&wit_core).map_err(|e| e.to_string())?;
    let wit_core_hash: [u8; 32] = Sha256::digest(wit_core_json.as_bytes()).into();

    // Step 2: opreturn with witness_core_hash (ASCII可读格式)
    let opr = OpReturnData {
        magic: "NXS".into(),
        version: VERSION,
        witness_hash_short: hex::encode(&wit_core_hash[..8]), // 前8字节 = 16 hex
        proof_hash_short: hex::encode(&proof_bytes[..8]),      // 前8字节 = 16 hex
        witness_hash_full: wit_core_hash,
        proof_hash_full: proof_bytes,
    };
    let opr_bytes = opr.to_bytes();
    let opr_hash: [u8; 32] = Sha256::digest(&opr_bytes).into();

    // Step 3: final witness with opr hash
    let wit_final = WitnessPayload {
        p: "nexus".into(),
        op: "mint".into(),
        amt: MINT_AMOUNT,
        pk: pubkey_hex.to_string(),
        fnp: proof.combined.clone(),
        opr: hex::encode(opr_hash),
    };
    let wit_final_json = serde_json::to_string(&wit_final).map_err(|e| e.to_string())?;
    let wit_final_hash: [u8; 32] = Sha256::digest(wit_final_json.as_bytes()).into();

    Ok(InterlockResult {
        witness_json: wit_final_json,
        witness_hash: wit_final_hash,
        opreturn_bytes: opr_bytes,
        opreturn_hash: opr_hash,
    })
}

// ═══════════════════════════════════════════
//  互锁验证 (Indexer用)
// ═══════════════════════════════════════════

pub fn verify_interlock(witness_json: &str, opreturn_bytes: &[u8]) -> Result<(), String> {
    let wit: WitnessPayload = serde_json::from_str(witness_json)
        .map_err(|e| format!("witness JSON无效: {}", e))?;
    let opr = OpReturnData::from_bytes(opreturn_bytes)
        .ok_or("OP_RETURN格式无效")?;

    // 验证 witness.opr → opreturn (正向)
    let opr_hash: [u8; 32] = Sha256::digest(opreturn_bytes).into();
    if wit.opr != hex::encode(opr_hash) {
        return Err("witness→opreturn hash不匹配".into());
    }

    // 验证 opreturn.witness_hash_short → witness_core (反向, 前缀匹配)
    let wit_core = WitnessPayload {
        p: wit.p.clone(),
        op: wit.op.clone(),
        amt: wit.amt,
        pk: wit.pk.clone(),
        fnp: wit.fnp.clone(),
        opr: String::new(),
    };
    let wit_core_json = serde_json::to_string(&wit_core)
        .map_err(|e| format!("序列化失败: {}", e))?;
    let wit_core_hash: [u8; 32] = Sha256::digest(wit_core_json.as_bytes()).into();
    let wit_core_hash_short = hex::encode(&wit_core_hash[..8]);
    if opr.witness_hash_short != wit_core_hash_short {
        return Err("opreturn→witness hash前缀不匹配".into());
    }

    // proof hash前缀验证
    let fnp_bytes = hex::decode(&wit.fnp).map_err(|e| e.to_string())?;
    let proof_short = hex::encode(&fnp_bytes[..8]);
    if opr.proof_hash_short != proof_short {
        return Err("proof hash前缀不匹配".into());
    }

    // 字段一致性
    if wit.p != "nexus" { return Err("协议标识错误".into()); }
    if wit.op != "mint" { return Err("操作类型错误".into()); }
    if wit.amt != MINT_AMOUNT { return Err(format!("金额错误: {}", wit.amt)); }

    Ok(())
}

// ═══════════════════════════════════════════
//  Bitcoin脚本构造辅助
// ═══════════════════════════════════════════

/// 构造OP_RETURN脚本
pub fn build_opreturn_script(data: &[u8]) -> Vec<u8> {
    let mut s = Vec::new();
    s.push(0x6a); // OP_RETURN
    push_data(&mut s, data);
    s
}

fn push_data(s: &mut Vec<u8>, d: &[u8]) {
    let len = d.len();
    if len <= 75 { s.push(len as u8); }
    else if len <= 255 { s.push(0x4c); s.push(len as u8); }
    else if len <= 65535 { s.push(0x4d); s.extend_from_slice(&(len as u16).to_le_bytes()); }
    else { s.push(0x4e); s.extend_from_slice(&(len as u32).to_le_bytes()); }
    s.extend_from_slice(d);
}

// ═══════════════════════════════════════════
//  测试
// ═══════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_proof() -> TwoRoundProof {
        TwoRoundProof {
            round1_hash: hex::encode([0xAA; 32]),
            round1_ts: 1000,
            round1_heights: vec![1;10],
            round2_hash: hex::encode([0xBB; 32]),
            round2_ts: 1005,
            round2_heights: vec![2;10],
            combined: hex::encode([0xCC; 32]),
            block_hash: hex::encode([0xDD; 32]),
            block_height: 941523,
            pubkey: hex::encode([0x02; 33]),
        }
    }

    #[test]
    fn interlock_builds_and_verifies() {
        let result = build_interlock(&mock_proof(), &hex::encode([0x02u8; 32])).unwrap();
        assert!(verify_interlock(&result.witness_json, &result.opreturn_bytes).is_ok());
    }

    #[test]
    fn interlock_detects_tamper() {
        let result = build_interlock(&mock_proof(), &hex::encode([0x02u8; 32])).unwrap();
        let mut bad = result.opreturn_bytes.clone();
        *bad.last_mut().unwrap() ^= 0xFF;
        assert!(verify_interlock(&result.witness_json, &bad).is_err());
    }

    #[test]
    fn opreturn_ascii_readable() {
        let opr = OpReturnData {
            magic: "NXS".into(),
            version: 1,
            witness_hash_short: "e2fa8baedf7b7a13".into(),
            proof_hash_short: "ea3af2ee3ac4bdd1".into(),
            witness_hash_full: [0; 32],
            proof_hash_full: [0; 32],
        };
        let bytes = opr.to_bytes();
        let text = String::from_utf8(bytes.clone()).unwrap();
        assert_eq!(text, "NXS:1:w=e2fa8baedf7b7a13:p=ea3af2ee3ac4bdd1");
        assert!(text.len() < 80); // within OP_RETURN limit

        // roundtrip
        let parsed = OpReturnData::from_bytes(&bytes).unwrap();
        assert_eq!(parsed.version, 1);
        assert_eq!(parsed.witness_hash_short, "e2fa8baedf7b7a13");
        assert_eq!(parsed.proof_hash_short, "ea3af2ee3ac4bdd1");
    }

    #[test]
    fn opreturn_rejects_invalid() {
        assert!(OpReturnData::from_bytes(b"BTC:1:w=abc:p=def").is_none());
        assert!(OpReturnData::from_bytes(b"NXS").is_none());
    }
}
