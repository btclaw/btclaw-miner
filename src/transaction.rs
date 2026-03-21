/// NEXUS 双层互锁交易构造
///
/// Witness层(铭文) ←→ OP_RETURN层 互相包含对方的SHA256

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
    pub fnp: String,      // combined proof hash
    pub opr: String,      // SHA256(OP_RETURN bytes)
}

/// OP_RETURN二进制结构 (共71字节)
/// "NXS"(3) + version(1) + seq(4) + witness_hash(32) + proof(32) - 1 = 72B
#[derive(Debug, Clone)]
pub struct OpReturnData {
    pub magic: [u8; 3],            // "NXS"
    pub version: u8,               // 0x01
    pub witness_hash: [u8; 32],    // SHA256(witness JSON)
    pub proof_hash: [u8; 32],      // combined proof
}

impl OpReturnData {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(68);
        b.extend_from_slice(&self.magic);          // 3
        b.push(self.version);                       // 1
        b.extend_from_slice(&self.witness_hash);    // 32
        b.extend_from_slice(&self.proof_hash);      // 32
        b                                           // total: 68
    }

    pub fn from_bytes(d: &[u8]) -> Option<Self> {
        if d.len() < 68 { return None; }
        if &d[0..3] != b"NXS" { return None; }
        Some(Self {
            magic: [d[0], d[1], d[2]],
            version: d[3],
            witness_hash: d[4..36].try_into().ok()?,
            proof_hash: d[36..68].try_into().ok()?,
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
/// 鸡生蛋问题的解法: 迭代求固定点
/// - witness.opr = SHA256(opreturn)
/// - opreturn.witness_hash = SHA256(witness_json)
/// 迭代2-3次必然收敛
pub fn build_interlock(proof: &TwoRoundProof) -> Result<InterlockResult, String> {
    let proof_bytes: [u8; 32] = hex::decode(&proof.combined)
        .map_err(|e| e.to_string())?.try_into().map_err(|_| "len")?;

    // 单向确定方案 (不需要迭代):
    // 1. 先构造witness(opr=""), 算其hash → 写入opreturn.witness_hash
    // 2. 构造完整opreturn, 算其hash → 写入witness.opr
    // 验证时: 验证者将witness.opr替换为""再算hash, 与opreturn.witness_hash比对

    // Step 1: witness with empty opr
    let wit_core = WitnessPayload {
        p: "nexus".into(),
        op: "mint".into(),
        amt: MINT_AMOUNT,
        fnp: proof.combined.clone(),
        opr: String::new(),
    };
    let wit_core_json = serde_json::to_string(&wit_core).map_err(|e| e.to_string())?;
    let wit_core_hash: [u8; 32] = Sha256::digest(wit_core_json.as_bytes()).into();

    // Step 2: opreturn with witness_core_hash
    let opr = OpReturnData {
        magic: *MAGIC,
        version: VERSION,
        witness_hash: wit_core_hash,
        proof_hash: proof_bytes,
    };
    let opr_bytes = opr.to_bytes();
    let opr_hash: [u8; 32] = Sha256::digest(&opr_bytes).into();

    // Step 3: final witness with opr hash
    let wit_final = WitnessPayload {
        p: "nexus".into(),
        op: "mint".into(),
        amt: MINT_AMOUNT,
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

    // 验证 opreturn.witness_hash → witness_core (反向)
    // witness_core = witness with opr="" 
    let wit_core = WitnessPayload {
        p: wit.p.clone(),
        op: wit.op.clone(),
        amt: wit.amt,
        fnp: wit.fnp.clone(),
        opr: String::new(),
    };
    let wit_core_json = serde_json::to_string(&wit_core)
        .map_err(|e| format!("序列化失败: {}", e))?;
    let wit_core_hash: [u8; 32] = Sha256::digest(wit_core_json.as_bytes()).into();
    if opr.witness_hash != wit_core_hash {
        return Err("opreturn→witness hash不匹配".into());
    }

    // 字段一致性

    let fnp_bytes = hex::decode(&wit.fnp).map_err(|e| e.to_string())?;
    if fnp_bytes != opr.proof_hash { return Err("proof不一致".into()); }

    if wit.p != "nexus" { return Err("协议标识错误".into()); }
    if wit.op != "mint" { return Err("操作类型错误".into()); }
    if wit.amt != MINT_AMOUNT { return Err(format!("金额错误: {}", wit.amt)); }

    Ok(())
}

// ═══════════════════════════════════════════
//  Bitcoin脚本构造辅助
// ═══════════════════════════════════════════

/// 构造Witness铭文envelope字节
pub fn build_inscription_script(payload_json: &str) -> Vec<u8> {
    let mut s = Vec::new();
    s.push(0x00); // OP_FALSE
    s.push(0x63); // OP_IF
    push_data(&mut s, b"nexus");
    s.push(0x01); s.push(0x01); // content-type tag
    push_data(&mut s, b"application/nexus-mint");
    s.push(0x01); s.push(0x00); // body separator
    push_data(&mut s, payload_json.as_bytes());
    s.push(0x68); // OP_ENDIF
    s
}

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
        let result = build_interlock(&mock_proof()).unwrap();
        assert!(verify_interlock(&result.witness_json, &result.opreturn_bytes).is_ok());
    }

    #[test]
    fn interlock_detects_tamper() {
        let result = build_interlock(&mock_proof()).unwrap();
        let mut bad = result.opreturn_bytes.clone();
        *bad.last_mut().unwrap() ^= 0xFF;
        assert!(verify_interlock(&result.witness_json, &bad).is_err());
    }

    #[test]
    fn opreturn_roundtrip() {
        let opr = OpReturnData {
            magic: *b"NXS", version: 1,
            witness_hash: [0xAA; 32], proof_hash: [0xBB; 32],
        };
        let bytes = opr.to_bytes();
        let parsed = OpReturnData::from_bytes(&bytes).unwrap();
        assert_eq!(parsed.witness_hash, [0xAA; 32]);
    }
}
