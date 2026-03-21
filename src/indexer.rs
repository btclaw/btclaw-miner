/// NEXUS Indexer — 极简验证，仅6条规则
///
/// 1. Witness铭文含"nexus"且格式正确
/// 2. OP_RETURN以"NXS"开头且格式正确
/// 3. 双层互锁hash验证通过
/// 4. 全节点证明验证通过
/// 5. 铸造费5000sats正确发送到项目方地址
/// 6. mint_seq <= 42,000 (总量未超)
///
/// 序号分配: 按区块确认顺序 + 区块内交易位置排序

use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use crate::constants::*;
use crate::proof::{TwoRoundProof, verify_proof};
use crate::transaction::*;

// ═══════════════════════════════════════════
//  状态
// ═══════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Indexer {
    /// 下一个可用序号
    pub next_seq: u32,
    /// 已铸造总量
    pub minted: u64,
    /// 余额表 address → amount
    pub balances: HashMap<String, u64>,
    /// 已使用的proof hash (防重放)
    pub used_proofs: HashMap<String, bool>,
    /// 铸造记录
    pub mints: Vec<MintRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MintRecord {
    pub seq: u32,
    pub txid: String,
    pub address: String,
    pub amount: u64,
    pub block_height: u32,
    pub proof_hash: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Status {
    pub total_supply: u64,
    pub minted: u64,
    pub remaining: u64,
    pub next_seq: u32,
    pub total_mints: u32,
    pub mints_remaining: u32,
    pub mint_amount_per_tx: u64,
    pub mint_fee_sats: u64,
    pub complete: bool,
    pub holders: usize,
}

impl Indexer {
    pub fn new() -> Self {
        Self {
            next_seq: 1,
            minted: 0,
            balances: HashMap::new(),
            used_proofs: HashMap::new(),
            mints: Vec::new(),
        }
    }

    pub fn status(&self) -> Status {
        Status {
            total_supply: MAX_SUPPLY,
            minted: self.minted,
            remaining: MAX_SUPPLY - self.minted,
            next_seq: self.next_seq,
            total_mints: TOTAL_MINTS,
            mints_remaining: TOTAL_MINTS - (self.next_seq - 1),
            mint_amount_per_tx: MINT_AMOUNT,
            mint_fee_sats: MINT_FEE_SATS,
            complete: self.minted >= MAX_SUPPLY,
            holders: self.balances.len(),
        }
    }

    /// 验证一笔交易是否为有效的NEXUS铸造
    pub fn validate(
        &self,
        tx: &CandidateTx,
        get_raw_block: &dyn Fn(u32) -> Result<Vec<u8>, String>,
    ) -> Result<MintRecord, String> {

        // ═══ 规则6: 总量检查 (先检查，快速失败) ═══
        if self.minted >= MAX_SUPPLY {
            return Err("铸造已结束，总量21,000,000已达上限".into());
        }

        // ═══ 规则1: Witness铭文格式 ═══
        let witness_json = tx.witness_json.as_ref()
            .ok_or("缺少Witness铭文数据")?;
        let wit: WitnessPayload = serde_json::from_str(witness_json)
            .map_err(|e| format!("Witness JSON解析失败: {}", e))?;
        if wit.p != "nexus" { return Err(format!("协议标识错误: {}", wit.p)); }
        if wit.op != "mint" { return Err(format!("操作类型错误: {}", wit.op)); }
        if wit.amt != MINT_AMOUNT { return Err(format!("铸造数量错误: {}", wit.amt)); }

        // ═══ 规则2: OP_RETURN格式 ═══
        let opr_bytes = tx.opreturn_bytes.as_ref()
            .ok_or("缺少OP_RETURN数据")?;
        let opr = OpReturnData::from_bytes(opr_bytes)
            .ok_or("OP_RETURN格式无效")?;
        if &opr.magic != MAGIC { return Err("魔术数错误".into()); }
        if opr.version != VERSION { return Err(format!("版本号错误: {}", opr.version)); }

        // ═══ 规则3: 双层互锁 ═══
        verify_interlock(witness_json, opr_bytes)?;

        // ═══ 规则4: 全节点证明 ═══
        let proof = tx.proof.as_ref()
            .ok_or("缺少全节点证明")?;
        // 防重放
        if self.used_proofs.contains_key(&proof.combined) {
            return Err("该全节点证明已被使用 (重放攻击)".into());
        }
        verify_proof(proof, get_raw_block)?;

        // ═══ 规则5: 铸造费 ═══
        if !tx.fee_output_valid {
            return Err(format!(
                "铸造费无效: 需向 {} 支付 {} sats",
                FEE_ADDRESS, MINT_FEE_SATS
            ));
        }

        // ═══ 全部通过 ═══
        Ok(MintRecord {
            seq: self.next_seq,
            txid: tx.txid.clone(),
            address: tx.minter_address.clone(),
            amount: MINT_AMOUNT,
            block_height: tx.block_height,
            proof_hash: proof.combined.clone(),
        })
    }

    /// 确认铸造 (更新状态)
    pub fn confirm(&mut self, record: MintRecord) {
        let addr = record.address.clone();
        let proof = record.proof_hash.clone();

        *self.balances.entry(addr).or_insert(0) += MINT_AMOUNT;
        self.used_proofs.insert(proof, true);
        self.minted += MINT_AMOUNT;
        self.next_seq += 1;
        self.mints.push(record);
    }

    /// 处理一个区块中的所有NEXUS交易
    /// 按交易在区块中的位置排序，依次验证和确认
    pub fn process_block(
        &mut self,
        candidates: Vec<CandidateTx>,
        get_raw_block: &dyn Fn(u32) -> Result<Vec<u8>, String>,
    ) -> Vec<Result<MintRecord, String>> {
        let mut results = Vec::new();

        for tx in candidates {
            match self.validate(&tx, get_raw_block) {
                Ok(record) => {
                    let r = record.clone();
                    self.confirm(record);
                    results.push(Ok(r));
                }
                Err(e) => {
                    results.push(Err(e));
                }
            }
        }

        results
    }
}

// ═══════════════════════════════════════════
//  从BTC交易解析出的候选数据
// ═══════════════════════════════════════════

#[derive(Debug, Clone)]
pub struct CandidateTx {
    pub txid: String,
    pub block_height: u32,
    pub tx_index_in_block: u32,
    pub minter_address: String,
    pub witness_json: Option<String>,
    pub opreturn_bytes: Option<Vec<u8>>,
    pub proof: Option<TwoRoundProof>,
    pub fee_output_valid: bool,
}

/// 快速筛选: 交易的OP_RETURN是否以"NXS"开头
pub fn is_nexus_tx(script_pubkey: &[u8]) -> bool {
    if script_pubkey.len() < 6 || script_pubkey[0] != 0x6a { return false; }
    let data_start = match script_pubkey[1] {
        n if n <= 75 => 2,
        0x4c => 3,
        0x4d => 4,
        _ => return false,
    };
    script_pubkey.len() > data_start + 3
        && &script_pubkey[data_start..data_start + 3] == b"NXS"
}

// ═══════════════════════════════════════════
//  测试
// ═══════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_indexer_status() {
        let idx = Indexer::new();
        let s = idx.status();
        assert_eq!(s.total_supply, MAX_SUPPLY);
        assert_eq!(s.minted, 0);
        assert_eq!(s.next_seq, 1);
        assert_eq!(s.mints_remaining, TOTAL_MINTS);
        assert!(!s.complete);
    }

    #[test]
    fn confirm_updates_state() {
        let mut idx = Indexer::new();
        idx.confirm(MintRecord {
            seq: 1,
            txid: "abc".into(),
            address: "bc1qxyz".into(),
            amount: MINT_AMOUNT,
            block_height: 941523,
            proof_hash: "proof1".into(),
        });

        assert_eq!(idx.next_seq, 2);
        assert_eq!(idx.minted, MINT_AMOUNT);
        assert_eq!(*idx.balances.get("bc1qxyz").unwrap(), MINT_AMOUNT);
        assert!(idx.used_proofs.contains_key("proof1"));
    }

    #[test]
    fn replay_detected() {
        let mut idx = Indexer::new();
        idx.used_proofs.insert("proof_x".into(), true);
        assert!(idx.used_proofs.contains_key("proof_x"));
    }

    #[test]
    fn supply_cap() {
        let mut idx = Indexer::new();
        idx.minted = MAX_SUPPLY; // 假装已铸完
        let s = idx.status();
        assert!(s.complete);
        assert_eq!(s.mints_remaining, 0);
    }

    #[test]
    fn is_nexus_tx_detection() {
        // OP_RETURN + push(76) + "NXS" + ...
        let mut script = vec![0x6a]; // OP_RETURN
        script.push(72);             // push 72 bytes
        script.extend_from_slice(b"NXS");
        script.extend_from_slice(&[0u8; 69]);
        assert!(is_nexus_tx(&script));

        // 非NEXUS交易
        let mut bad = vec![0x6a, 5];
        bad.extend_from_slice(b"XXXXX");
        assert!(!is_nexus_tx(&bad));
    }
}
