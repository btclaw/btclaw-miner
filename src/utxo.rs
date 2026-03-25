/// NEXUS UTXO 管理模块
///
/// 五层安全过滤 + 多UTXO合并选择 + 找零/铸造记录追踪
///
/// 运行时产生三个JSON文件:
///   nxs_mints.json   — 铸造产生的 token UTXO (永久锁定, 不可花费)
///   nxs_change.json  — Commit TX 找零 (可复用, 优先用于下次铸造)
///   nxs_locked.json  — 检测到的外部协议资产 (不可花费)

use serde::{Serialize, Deserialize};
use std::collections::HashSet;
use crate::constants::*;

// ═══════════════════════════════════════════
//  数据结构
// ═══════════════════════════════════════════

/// 单条 UTXO 记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UtxoRecord {
    pub txid: String,
    pub vout: u32,
    pub amount: u64,         // satoshis
    #[serde(default)]
    pub confirmations: u64,
    #[serde(default)]
    pub address: String,
}

impl UtxoRecord {
    pub fn key(&self) -> String {
        format!("{}:{}", self.txid, self.vout)
    }
}

/// UTXO 分类结果
#[derive(Debug, Clone, PartialEq)]
pub enum UtxoClass {
    /// 可安全花费
    Spendable,
    /// 锁定: NXS 铸造的 token UTXO
    LockedNxsMint,
    /// 锁定: 疑似外部协议资产 (铭文/符文等)
    LockedExternalAsset,
    /// 锁定: 金额过小, 疑似绑定资产
    LockedDust,
    /// 灰色地带: 默认锁定, 用户可手动解锁
    GrayZone,
    /// 已知找零: 安全可花费
    KnownChange,
}

/// UTXO 管理器
#[derive(Debug)]
pub struct UtxoManager {
    /// 铸造记录 (锁定)
    pub mints: Vec<UtxoRecord>,
    /// 找零记录 (可用)
    pub changes: Vec<UtxoRecord>,
    /// 外部资产锁定记录
    pub locked: Vec<UtxoRecord>,

    // 快速查找集合
    mint_keys: HashSet<String>,
    change_keys: HashSet<String>,
    locked_keys: HashSet<String>,
}

impl UtxoManager {
    // ═══════════════════════════════════════
    //  加载 / 保存
    // ═══════════════════════════════════════

    /// 从本地JSON文件加载, 文件不存在则创建空记录
    pub fn load() -> Self {
        let mints = Self::load_file("nxs_mints.json");
        let changes = Self::load_file("nxs_change.json");
        let locked = Self::load_file("nxs_locked.json");

        let mint_keys: HashSet<String> = mints.iter().map(|r| r.key()).collect();
        let change_keys: HashSet<String> = changes.iter().map(|r| r.key()).collect();
        let locked_keys: HashSet<String> = locked.iter().map(|r| r.key()).collect();

        Self { mints, changes, locked, mint_keys, change_keys, locked_keys }
    }

    fn load_file(path: &str) -> Vec<UtxoRecord> {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// 保存所有记录到本地JSON
    pub fn save(&self) {
        Self::save_file("nxs_mints.json", &self.mints);
        Self::save_file("nxs_change.json", &self.changes);
        Self::save_file("nxs_locked.json", &self.locked);
    }

    fn save_file(path: &str, records: &[UtxoRecord]) {
        if let Ok(json) = serde_json::to_string_pretty(records) {
            let _ = std::fs::write(path, json);
        }
    }

    // ═══════════════════════════════════════
    //  五层 UTXO 分类决策树
    // ═══════════════════════════════════════

    /// 对单个 UTXO 进行安全分类
    ///
    /// 五层过滤 (按优先级):
    ///   1. 本地铸造记录 → 锁定
    ///   2. 金额 ≤ DUST_LIMIT(546) → 锁定 (疑似资产)
    ///   3. 来源交易含协议数据 → 锁定 (需外部调用 classify_with_tx_check)
    ///   4. 本地找零记录 → 可用
    ///   5. 金额 > SAFE_UTXO_THRESHOLD(1000) → 可用, 否则灰色地带
    pub fn classify(&self, utxo: &UtxoRecord) -> UtxoClass {
        let key = utxo.key();

        // 层1: 本地铸造记录
        if self.mint_keys.contains(&key) {
            return UtxoClass::LockedNxsMint;
        }

        // 层2: 金额过滤 (≤546 几乎必定是协议资产)
        if utxo.amount <= DUST_LIMIT {
            return UtxoClass::LockedDust;
        }

        // 层3: 外部资产锁定记录 (之前已检测过的)
        if self.locked_keys.contains(&key) {
            return UtxoClass::LockedExternalAsset;
        }

        // 层4: 本地找零记录 (我们自己产生的, 绝对安全)
        if self.change_keys.contains(&key) {
            return UtxoClass::KnownChange;
        }

        // 层5: 安全金额阈值
        if utxo.amount > SAFE_UTXO_THRESHOLD {
            return UtxoClass::Spendable;
        }

        // 547-1000 sats: 灰色地带
        UtxoClass::GrayZone
    }

    /// 带来源交易检查的分类 (需要 RPC 调用结果)
    ///
    /// has_protocol_data: 来源TX是否包含 OP_RETURN/inscription 等协议数据
    pub fn classify_with_tx_check(
        &mut self,
        utxo: &UtxoRecord,
        has_protocol_data: bool,
    ) -> UtxoClass {
        // 先做基础分类
        let base = self.classify(utxo);

        // 如果基础分类已经确定, 直接返回
        match base {
            UtxoClass::LockedNxsMint |
            UtxoClass::LockedDust |
            UtxoClass::LockedExternalAsset |
            UtxoClass::KnownChange => return base,
            _ => {}
        }

        // 对灰色地带和可花费的 UTXO, 检查来源交易
        if has_protocol_data {
            // 写入锁定记录
            self.add_locked(utxo.clone());
            return UtxoClass::LockedExternalAsset;
        }

        base
    }

    // ═══════════════════════════════════════
    //  UTXO 选择算法
    // ═══════════════════════════════════════

    /// 从 UTXO 列表中选择足够的可用 UTXO 用于 Commit TX
    ///
    /// 返回: (选中的UTXOs, 总金额)
    /// 选择策略:
    ///   1. 优先使用已知找零 (nxs_change.json)
    ///   2. 然后使用大额安全 UTXO
    ///   3. 支持多个 UTXO 合并
    pub fn select_for_commit(
        &self,
        all_utxos: &[UtxoRecord],
        target_sats: u64,
    ) -> Result<(Vec<UtxoRecord>, u64), String> {
        // 分类所有 UTXO
        let mut known_changes: Vec<&UtxoRecord> = Vec::new();
        let mut spendable: Vec<&UtxoRecord> = Vec::new();

        for utxo in all_utxos {
            match self.classify(utxo) {
                UtxoClass::KnownChange => known_changes.push(utxo),
                UtxoClass::Spendable => spendable.push(utxo),
                _ => {} // 锁定的跳过
            }
        }

        // 按金额降序排列 (优先用大的, 减少输入数)
        known_changes.sort_by(|a, b| b.amount.cmp(&a.amount));
        spendable.sort_by(|a, b| b.amount.cmp(&a.amount));

        // 策略1: 尝试找单个足够大的 UTXO (最优, 1输入)
        for utxo in known_changes.iter().chain(spendable.iter()) {
            if utxo.amount >= target_sats {
                return Ok((vec![(*utxo).clone()], utxo.amount));
            }
        }

        // 策略2: 合并多个 UTXO
        let mut selected: Vec<UtxoRecord> = Vec::new();
        let mut total: u64 = 0;

        // 先用找零
        for utxo in &known_changes {
            if total >= target_sats { break; }
            selected.push((*utxo).clone());
            total += utxo.amount;
        }

        // 不够则补充大额安全 UTXO
        if total < target_sats {
            for utxo in &spendable {
                if total >= target_sats { break; }
                selected.push((*utxo).clone());
                total += utxo.amount;
            }
        }

        if total < target_sats {
            let deficit = target_sats - total;
            return Err(format!(
                "可用余额不足: 有 {} sats, 需要 {} sats, 差 {} sats\n\
                 请向铸造地址充值至少 {} sats",
                total, target_sats, deficit, deficit
            ));
        }

        // 限制输入数量 (避免超过 mempool 未确认链限制)
        if selected.len() > MAX_COMMIT_INPUTS {
            return Err(format!(
                "需要合并 {} 个 UTXO, 超过安全上限 {}。\n\
                 请先充值一个大额 UTXO",
                selected.len(), MAX_COMMIT_INPUTS
            ));
        }

        Ok((selected, total))
    }

    // ═══════════════════════════════════════
    //  记录管理
    // ═══════════════════════════════════════

    /// 记录铸造产生的 token UTXO (Reveal TX 的 output[0])
    pub fn record_mint(&mut self, txid: &str, vout: u32, amount: u64) {
        let record = UtxoRecord {
            txid: txid.to_string(),
            vout,
            amount,
            confirmations: 0,
            address: String::new(),
        };
        let key = record.key();
        if !self.mint_keys.contains(&key) {
            self.mint_keys.insert(key);
            self.mints.push(record);
        }
    }

    /// 记录 Commit TX 的找零 UTXO
    pub fn record_change(&mut self, txid: &str, vout: u32, amount: u64) {
        let record = UtxoRecord {
            txid: txid.to_string(),
            vout,
            amount,
            confirmations: 0,
            address: String::new(),
        };
        let key = record.key();
        if !self.change_keys.contains(&key) {
            self.change_keys.insert(key);
            self.changes.push(record);
        }
    }

    /// 添加外部资产锁定
    fn add_locked(&mut self, record: UtxoRecord) {
        let key = record.key();
        if !self.locked_keys.contains(&key) {
            self.locked_keys.insert(key);
            self.locked.push(record);
        }
    }

    /// 清理已花费的找零记录
    /// 调用时机: 铸造前, 对比 listunspent 结果清除不存在的找零
    pub fn cleanup_spent_changes(&mut self, live_utxo_keys: &HashSet<String>) {
        self.changes.retain(|r| live_utxo_keys.contains(&r.key()));
        self.change_keys = self.changes.iter().map(|r| r.key()).collect();
    }

    // ═══════════════════════════════════════
    //  余额预检
    // ═══════════════════════════════════════

    /// 铸造前预检: 计算可用余额, 展示 UTXO 池状态
    pub fn pre_check(&self, all_utxos: &[UtxoRecord], target_sats: u64) -> PreCheckResult {
        let mut available: u64 = 0;
        let mut locked_count: usize = 0;
        let mut locked_sats: u64 = 0;
        let mut spendable_count: usize = 0;
        let mut gray_count: usize = 0;

        for utxo in all_utxos {
            match self.classify(utxo) {
                UtxoClass::KnownChange | UtxoClass::Spendable => {
                    available += utxo.amount;
                    spendable_count += 1;
                }
                UtxoClass::GrayZone => {
                    gray_count += 1;
                }
                _ => {
                    locked_count += 1;
                    locked_sats += utxo.amount;
                }
            }
        }

        PreCheckResult {
            total_utxos: all_utxos.len(),
            spendable_count,
            locked_count,
            locked_sats,
            gray_count,
            available_sats: available,
            target_sats,
            sufficient: available >= target_sats,
            deficit: if available >= target_sats { 0 } else { target_sats - available },
        }
    }
}

/// 预检结果
#[derive(Debug)]
pub struct PreCheckResult {
    pub total_utxos: usize,
    pub spendable_count: usize,
    pub locked_count: usize,
    pub locked_sats: u64,
    pub gray_count: usize,
    pub available_sats: u64,
    pub target_sats: u64,
    pub sufficient: bool,
    pub deficit: u64,
}

impl PreCheckResult {
    /// 打印预检报告
    pub fn print(&self) {
        println!("    ── UTXO Pool Status / UTXO池状态 ──");
        println!("    Total UTXOs:    {}", self.total_utxos);
        println!("    Spendable:      {} ({} sats)", self.spendable_count, self.available_sats);
        println!("    Locked:         {} ({} sats)", self.locked_count, self.locked_sats);
        if self.gray_count > 0 {
            println!("    Gray zone:      {} (默认锁定)", self.gray_count);
        }
        println!("    Need:           {} sats", self.target_sats);
        if self.sufficient {
            println!("    Status:         ✅ 余额充足");
        } else {
            println!("    Status:         ❌ 不足, 需充值 {} sats", self.deficit);
        }
        println!();
    }
}

// ═══════════════════════════════════════════
//  来源交易检查辅助
// ═══════════════════════════════════════════

/// 检查原始交易数据是否包含协议数据 (OP_RETURN / inscription)
///
/// 输入: getrawtransaction 的 JSON 结果
/// 检查项:
///   - vout 中是否有 nulldata 类型 (OP_RETURN)
///   - vin 中的 witness 是否包含 ord envelope 标记
pub fn check_tx_has_protocol_data(tx_json: &serde_json::Value) -> bool {
    // 检查 OP_RETURN 输出
    if let Some(vouts) = tx_json["vout"].as_array() {
        for vout in vouts {
            if vout["scriptPubKey"]["type"].as_str() == Some("nulldata") {
                return true;
            }
        }
    }

    // 检查 witness 中的 inscription envelope
    if let Some(vins) = tx_json["vin"].as_array() {
        for vin in vins {
            if let Some(witness) = vin["txinwitness"].as_array() {
                for item in witness {
                    if let Some(hex_str) = item.as_str() {
                        // ord envelope: OP_FALSE OP_IF "ord" ...
                        // hex: 0063036f726401...
                        if hex_str.contains("0063036f7264") {
                            return true;
                        }
                    }
                }
            }
        }
    }

    false
}

// ═══════════════════════════════════════════
//  测试
// ═══════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn make_utxo(txid: &str, vout: u32, amount: u64) -> UtxoRecord {
        UtxoRecord {
            txid: txid.to_string(),
            vout,
            amount,
            confirmations: 1,
            address: "bc1ptest".to_string(),
        }
    }

    #[test]
    fn classify_locks_dust() {
        let mgr = UtxoManager::load();
        let utxo = make_utxo("aaa", 0, TOKEN_OUTPUT_SATS);
        assert_eq!(mgr.classify(&utxo), UtxoClass::LockedDust);

        let utxo2 = make_utxo("bbb", 0, 546);
        assert_eq!(mgr.classify(&utxo2), UtxoClass::LockedDust);
    }

    #[test]
    fn classify_spendable_large() {
        let mgr = UtxoManager::load();
        let utxo = make_utxo("ccc", 0, 5000);
        assert_eq!(mgr.classify(&utxo), UtxoClass::Spendable);
    }

    #[test]
    fn classify_gray_zone() {
        let mgr = UtxoManager::load();
        let utxo = make_utxo("ddd", 0, 800);
        assert_eq!(mgr.classify(&utxo), UtxoClass::GrayZone);
    }

    #[test]
    fn classify_locks_recorded_mint() {
        let mut mgr = UtxoManager::load();
        mgr.record_mint("eee", 0, TOKEN_OUTPUT_SATS);
        let utxo = make_utxo("eee", 0, TOKEN_OUTPUT_SATS);
        assert_eq!(mgr.classify(&utxo), UtxoClass::LockedNxsMint);
    }

    #[test]
    fn classify_known_change() {
        let mut mgr = UtxoManager::load();
        mgr.record_change("fff", 1, 620);
        let utxo = make_utxo("fff", 1, 620);
        assert_eq!(mgr.classify(&utxo), UtxoClass::KnownChange);
    }

    #[test]
    fn select_single_utxo() {
        let mgr = UtxoManager::load();
        let utxos = vec![make_utxo("aaa", 0, 5000)];
        let (selected, total) = mgr.select_for_commit(&utxos, 1500).unwrap();
        assert_eq!(selected.len(), 1);
        assert_eq!(total, 5000);
    }

    #[test]
    fn select_merges_changes() {
        let mut mgr = UtxoManager::load();
        mgr.record_change("aaa", 0, 620);
        mgr.record_change("bbb", 0, 620);
        let utxos = vec![
            make_utxo("aaa", 0, 620),
            make_utxo("bbb", 0, 620),
        ];
        let (selected, total) = mgr.select_for_commit(&utxos, 1200).unwrap();
        assert_eq!(selected.len(), 2);
        assert_eq!(total, 1240);
    }

    #[test]
    fn select_rejects_insufficient() {
        let mgr = UtxoManager::load();
        let utxos = vec![make_utxo("aaa", 0, 500)];
        assert!(mgr.select_for_commit(&utxos, 1500).is_err());
    }

    #[test]
    fn select_skips_locked() {
        let mut mgr = UtxoManager::load();
        mgr.record_mint("locked", 0, TOKEN_OUTPUT_SATS);
        let utxos = vec![
            make_utxo("locked", 0, TOKEN_OUTPUT_SATS),  // 被锁定
            make_utxo("good", 0, 5000),   // 可用
        ];
        let (selected, _) = mgr.select_for_commit(&utxos, 1500).unwrap();
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].txid, "good");
    }

    #[test]
    fn pre_check_reports_correctly() {
        let mut mgr = UtxoManager::load();
        mgr.record_mint("mint1", 0, TOKEN_OUTPUT_SATS);
        mgr.record_change("change1", 1, 620);
        let utxos = vec![
            make_utxo("mint1", 0, TOKEN_OUTPUT_SATS),     // locked
            make_utxo("change1", 1, 620),   // known change
            make_utxo("big", 0, 5000),      // spendable
            make_utxo("small", 0, 800),     // gray zone
        ];
        let result = mgr.pre_check(&utxos, 1500);
        assert_eq!(result.spendable_count, 2); // change + big
        assert_eq!(result.available_sats, 5620);
        assert_eq!(result.locked_count, 1);
        assert!(result.sufficient);
    }
}
