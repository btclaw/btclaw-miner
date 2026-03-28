use serde::{Serialize, Deserialize};
use std::sync::Arc;
use crate::constants::*;
use crate::proof::{TwoRoundProof, verify_proof};
use crate::transaction::*;
use crate::db::NexusDb;

// ═══════════════════════════════════════════
//  状态 — 所有数据存储在 SQLite 中
// ═══════════════════════════════════════════

pub struct Indexer {
    pub db: Arc<NexusDb>,
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
    pub fn new(db: Arc<NexusDb>) -> Self {
        Self { db }
    }

    pub fn status(&self) -> Status {
        let minted = self.db.get_minted();
        let next_seq = self.db.get_next_seq();
        Status {
            total_supply: MAX_SUPPLY,
            minted,
            remaining: MAX_SUPPLY.saturating_sub(minted),
            next_seq,
            total_mints: TOTAL_MINTS,
            mints_remaining: TOTAL_MINTS.saturating_sub(next_seq.saturating_sub(1)),
            mint_amount_per_tx: MINT_AMOUNT,
            mint_fee_sats: MINT_FEE_SATS,
            complete: minted >= MAX_SUPPLY,
            holders: self.db.get_holder_count(),
        }
    }

    /// 验证一笔交易是否为有效的NEXUS铸造
    pub fn validate(
        &self,
        tx: &CandidateTx,
        get_raw_block: &dyn Fn(u32) -> Result<Vec<u8>, String>,
    ) -> Result<MintRecord, String> {

        let minted = self.db.get_minted();
        let next_seq = self.db.get_next_seq();

        // ═══ 规则7: 总量检查 ═══
        if minted >= MAX_SUPPLY {
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
        if opr.magic != "NXS" { return Err("魔术数错误".into()); }
        if opr.op != "MINT" { return Err(format!("操作类型错误: {}", opr.op)); }
        if opr.amt != MINT_AMOUNT { return Err(format!("OP_RETURN铸造量错误: {}", opr.amt)); }

        // ═══ 规则3: 铸造费 (轻量，前置防DoS) ═══
        if !tx.fee_output_valid {
            return Err(format!(
                "铸造费无效: 需向 {} 支付 {} sats",
                FEE_ADDRESS, MINT_FEE_SATS
            ));
        }

        // ═══ 规则4: 双层互锁 ═══
        verify_interlock(witness_json, opr_bytes)?;

        // ═══ 规则5: 身份绑定 — pk必须匹配交易签名公钥 ═══
        if wit.pk.is_empty() {
            return Err("缺少公钥字段pk".into());
        }
        if let Some(ref tx_pubkey) = tx.tx_pubkey {
            if wit.pk != *tx_pubkey {
                return Err(format!(
                    "公钥不匹配: JSON pk={} vs tx pubkey={}",
                    wit.pk, tx_pubkey
                ));
            }
        }

        // ═══ 规则6: 全节点证明 (最昂贵，放最后) ═══
        let proof = tx.proof.as_ref()
            .ok_or("缺少全节点证明")?;
        // 6a. 轻量预检查 (防DoS)
        if proof.round1_heights.len() != CHALLENGES_PER_ROUND
            || proof.round2_heights.len() != CHALLENGES_PER_ROUND {
            return Err("proof轮次数量异常".into());
        }
        if proof.round2_ts.saturating_sub(proof.round1_ts) > MAX_ROUND_GAP_SECS {
            return Err(format!("proof时间差{}s超限", proof.round2_ts - proof.round1_ts));
        }
        if proof.combined.len() != 64 || proof.pubkey.len() != 66 {
            return Err("proof字段长度异常".into());
        }
        // 6b. 防重放 — 从数据库查询
        if self.db.has_proof(&proof.combined) {
            return Err("该全节点证明已被使用 (重放攻击)".into());
        }
        // 6c. 完整验证
        verify_proof(proof, get_raw_block)?;

        // ═══ 全部通过 ═══
        Ok(MintRecord {
            seq: next_seq,
            txid: tx.txid.clone(),
            address: tx.minter_address.clone(),
            amount: MINT_AMOUNT,
            block_height: tx.block_height,
            proof_hash: proof.combined.clone(),
        })
    }

    /// 确认铸造 (写入数据库)
    pub fn confirm(&self, record: MintRecord) {
        // 写入铸造记录
        self.db.add_mint(&record);

        // 更新余额
        self.db.add_balance(&record.address, record.amount);

        // 记录已用证明
        self.db.add_proof(&record.proof_hash);

        // 更新 minted 和 next_seq
        let minted = self.db.get_minted();
        self.db.set_minted(minted + record.amount);
        self.db.set_next_seq(record.seq + 1);
    }

    /// 处理一个区块中的所有NEXUS交易
    /// 按交易在区块中的位置排序，依次验证和确认
    pub fn process_block(
        &self,
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
    pub tx_pubkey: Option<String>,  // Taproot witness提取的公钥
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
