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
    pub seq: u32,         // 铸造序号 (由Reactor从Indexer获取)
    pub amt: u64,         // 500_00000000
    pub fnp: String,      // combined proof hash
    pub opr: String,      // SHA256(OP_RETURN bytes) ← 互锁
}

/// OP_RETURN二进制结构 (共71字节)
/// "NXS"(3) + version(1) + seq(4) + witness_hash(32) + proof(32) - 1 = 72B
#[derive(Debug, Clone)]
pub struct OpReturnData {
    pub magic: [u8; 3],            // "NXS"
    pub version: u8,               // 0x01
    pub seq: u32,                  // 铸造序号
    pub witness_hash: [u8; 32],    // SHA256(witness JSON) ← 互锁
    pub proof_hash: [u8; 32],      // combined proof
}

impl OpReturnData {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(72);
        b.extend_from_slice(&self.magic);
        b.push(self.version);
        b.extend_from_slice(&self.seq.to_le_bytes());
        b.extend_from_slice(&self.witness_hash);
        b.extend_from_slice(&self.proof_hash);
        b
    }

    pub fn from_bytes(d: &[u8]) -> Option<Self> {
        if d.len() < 72 { return None; }
        if &d[0..3] != b"NXS" { return None; }
        Some(Self {
            magic: [d[0], d[1], d[2]],
            version: d[3],
            seq: u32::from_le_bytes([d[4], d[5], d[6], d[7]]),
            witness_hash: d[8..40].try_into().ok()?,
            proof_hash: d[40..72].try_into().ok()?,
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
pub fn build_interlock(seq: u32, proof: &TwoRoundProof) -> Result<InterlockResult, String> {
    let proof_bytes: [u8; 32] = hex::decode(&proof.combined)
        .map_err(|e| e.to_string())?.try_into().map_err(|_| "len")?;

    let mut opr = OpReturnData {
        magic: *MAGIC,
        version: VERSION,
        seq,
        witness_hash: [0u8; 32], // 占位
        proof_hash: proof_bytes,
    };

    let mut wit = WitnessPayload {
        p: "nexus".into(),
        op: "mint".into(),
        seq,
        amt: MINT_AMOUNT,
        fnp: proof.combined.clone(),
        opr: hex::encode([0u8; 32]), // 占位
    };

    // 迭代求固定点 (通常2次收敛)
    for _ in 0..10 {
        let opr_bytes = opr.to_bytes();
        let opr_hash: [u8; 32] = Sha256::digest(&opr_bytes).into();
        wit.opr = hex::encode(opr_hash);

        let wit_json = serde_json::to_string(&wit).map_err(|e| e.to_string())?;
        let wit_hash: [u8; 32] = Sha256::digest(wit_json.as_bytes()).into();

        if opr.witness_hash == wit_hash {
            return Ok(InterlockResult {
                witness_json: wit_json,
                witness_hash: wit_hash,
                opreturn_bytes: opr_bytes,
                opreturn_hash: opr_hash,
            });
        }
        opr.witness_hash = wit_hash;
    }

    Err("互锁未收敛".into())
}

// ═══════════════════════════════════════════
//  互锁验证 (Indexer用)
// ═══════════════════════════════════════════

pub fn verify_interlock(witness_json: &str, opreturn_bytes: &[u8]) -> Result<(), String> {
    let wit: WitnessPayload = serde_json::from_str(witness_json)
        .map_err(|e| format!("witness JSON无效: {}", e))?;
    let opr = OpReturnData::from_bytes(opreturn_bytes)
        .ok_or("OP_RETURN格式无效")?;

    // 验证 witness → opreturn
    let opr_hash: [u8; 32] = Sha256::digest(opreturn_bytes).into();
    if wit.opr != hex::encode(opr_hash) {
        return Err("witness→opreturn hash不匹配".into());
    }

    // 验证 opreturn → witness
    let wit_hash: [u8; 32] = Sha256::digest(witness_json.as_bytes()).into();
    if opr.witness_hash != wit_hash {
        return Err("opreturn→witness hash不匹配".into());
    }

    // 字段一致性
    if wit.seq != opr.seq { return Err("seq不一致".into()); }

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
        let result = build_interlock(1, &mock_proof()).unwrap();
        assert!(verify_interlock(&result.witness_json, &result.opreturn_bytes).is_ok());
    }

    #[test]
    fn interlock_detects_tamper() {
        let result = build_interlock(1, &mock_proof()).unwrap();
        let mut bad = result.opreturn_bytes.clone();
        *bad.last_mut().unwrap() ^= 0xFF;
        assert!(verify_interlock(&result.witness_json, &bad).is_err());
    }

    #[test]
    fn opreturn_roundtrip() {
        let opr = OpReturnData {
            magic: *b"NXS", version: 1, seq: 42000,
            witness_hash: [0xAA; 32], proof_hash: [0xBB; 32],
        };
        let bytes = opr.to_bytes();
        let parsed = OpReturnData::from_bytes(&bytes).unwrap();
        assert_eq!(parsed.seq, 42000);
        assert_eq!(parsed.witness_hash, [0xAA; 32]);
    }
}
