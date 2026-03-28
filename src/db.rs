use rusqlite::{Connection, params};
use std::sync::Mutex;
use std::collections::HashMap;

use crate::indexer::MintRecord;

// ═══════════════════════════════════════════
//  Transfer 记录 (从 bin/indexer.rs 移到这里)
// ═══════════════════════════════════════════

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TransferRecord {
    pub txid: String,
    pub from: String,
    pub to: String,
    pub amount: u64,
    pub block_height: u32,
    #[serde(default)]
    pub batch_index: u32,
}

// ═══════════════════════════════════════════
//  数据库主结构
// ═══════════════════════════════════════════

pub struct NexusDb {
    conn: Mutex<Connection>,
}

impl NexusDb {
    /// 打开（或创建）数据库，初始化表结构
    pub fn open(path: &str) -> Result<Self, String> {
        let conn = Connection::open(path).map_err(|e| format!("DB open failed: {}", e))?;

        // 性能优化
        conn.execute_batch("
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            PRAGMA cache_size = -8000;
            PRAGMA busy_timeout = 5000;
        ").map_err(|e| format!("PRAGMA failed: {}", e))?;

        // 建表
        conn.execute_batch("
            CREATE TABLE IF NOT EXISTS meta (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS balances (
                address TEXT PRIMARY KEY,
                amount  INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS mints (
                seq          INTEGER PRIMARY KEY,
                txid         TEXT NOT NULL,
                address      TEXT NOT NULL,
                amount       INTEGER NOT NULL,
                block_height INTEGER NOT NULL,
                proof_hash   TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_mints_address ON mints(address);
            CREATE INDEX IF NOT EXISTS idx_mints_txid ON mints(txid);
            CREATE INDEX IF NOT EXISTS idx_mints_block ON mints(block_height);

            CREATE TABLE IF NOT EXISTS used_proofs (
                proof_hash TEXT PRIMARY KEY
            );

            CREATE TABLE IF NOT EXISTS transfers (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                txid         TEXT NOT NULL,
                from_addr    TEXT NOT NULL,
                to_addr      TEXT NOT NULL,
                amount       INTEGER NOT NULL,
                block_height INTEGER NOT NULL,
                batch_index  INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_transfers_from ON transfers(from_addr);
            CREATE INDEX IF NOT EXISTS idx_transfers_to ON transfers(to_addr);
            CREATE INDEX IF NOT EXISTS idx_transfers_txid ON transfers(txid);
            CREATE INDEX IF NOT EXISTS idx_transfers_block ON transfers(block_height);

            CREATE TABLE IF NOT EXISTS block_hashes (
                height INTEGER PRIMARY KEY,
                hash   TEXT NOT NULL
            );
        ").map_err(|e| format!("CREATE TABLE failed: {}", e))?;

        // 初始化 meta 默认值
        conn.execute(
            "INSERT OR IGNORE INTO meta (key, value) VALUES ('next_seq', '1')",
            [],
        ).ok();
        conn.execute(
            "INSERT OR IGNORE INTO meta (key, value) VALUES ('minted', '0')",
            [],
        ).ok();
        conn.execute(
            "INSERT OR IGNORE INTO meta (key, value) VALUES ('scan_height', '0')",
            [],
        ).ok();

        Ok(Self { conn: Mutex::new(conn) })
    }

    // ═══════════════════════════════════════
    //  META
    // ═══════════════════════════════════════

    pub fn get_meta(&self, key: &str) -> String {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT value FROM meta WHERE key = ?1",
            params![key],
            |row| row.get(0),
        ).unwrap_or_default()
    }

    pub fn get_meta_u64(&self, key: &str) -> u64 {
        self.get_meta(key).parse().unwrap_or(0)
    }

    pub fn get_meta_u32(&self, key: &str) -> u32 {
        self.get_meta(key).parse().unwrap_or(0)
    }

    pub fn set_meta(&self, key: &str, value: &str) {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES (?1, ?2)",
            params![key, value],
        ).ok();
    }

    pub fn set_meta_u64(&self, key: &str, value: u64) {
        self.set_meta(key, &value.to_string());
    }

    pub fn set_meta_u32(&self, key: &str, value: u32) {
        self.set_meta(key, &value.to_string());
    }

    // ═══════════════════════════════════════
    //  便捷: next_seq / minted / scan_height
    // ═══════════════════════════════════════

    pub fn get_next_seq(&self) -> u32 { self.get_meta_u32("next_seq") }
    pub fn set_next_seq(&self, v: u32) { self.set_meta_u32("next_seq", v); }

    pub fn get_minted(&self) -> u64 { self.get_meta_u64("minted") }
    pub fn set_minted(&self, v: u64) { self.set_meta_u64("minted", v); }

    pub fn get_scan_height(&self) -> u32 { self.get_meta_u32("scan_height") }
    pub fn set_scan_height(&self, v: u32) { self.set_meta_u32("scan_height", v); }

    // ═══════════════════════════════════════
    //  BALANCES
    // ═══════════════════════════════════════

    pub fn get_balance(&self, address: &str) -> u64 {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT amount FROM balances WHERE address = ?1",
            params![address],
            |row| row.get::<_, i64>(0),
        ).unwrap_or(0) as u64
    }

    pub fn set_balance(&self, address: &str, amount: u64) {
        let conn = self.conn.lock().unwrap();
        if amount == 0 {
            conn.execute("DELETE FROM balances WHERE address = ?1", params![address]).ok();
        } else {
            conn.execute(
                "INSERT OR REPLACE INTO balances (address, amount) VALUES (?1, ?2)",
                params![address, amount as i64],
            ).ok();
        }
    }

    pub fn add_balance(&self, address: &str, amount: u64) {
        let current = self.get_balance(address);
        self.set_balance(address, current + amount);
    }

    pub fn sub_balance(&self, address: &str, amount: u64) {
        let current = self.get_balance(address);
        let new_val = current.saturating_sub(amount);
        self.set_balance(address, new_val);
    }

    pub fn get_holder_count(&self) -> usize {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT COUNT(*) FROM balances WHERE amount > 0",
            [],
            |row| row.get::<_, i64>(0),
        ).unwrap_or(0) as usize
    }

    /// 获取持有人排行 (按余额降序, 最多 limit 个)
    pub fn get_holders(&self, limit: u32) -> Vec<(String, u64)> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT address, amount FROM balances WHERE amount > 0 ORDER BY amount DESC LIMIT ?1"
        ).unwrap();
        stmt.query_map(params![limit], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as u64))
        }).unwrap().filter_map(|r| r.ok()).collect()
    }

    /// 获取所有余额 (用于兼容旧代码)
    pub fn get_all_balances(&self) -> HashMap<String, u64> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT address, amount FROM balances WHERE amount > 0"
        ).unwrap();
        stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as u64))
        }).unwrap().filter_map(|r| r.ok()).collect()
    }

    // ═══════════════════════════════════════
    //  MINTS
    // ═══════════════════════════════════════

    pub fn add_mint(&self, record: &MintRecord) {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO mints (seq, txid, address, amount, block_height, proof_hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                record.seq, record.txid, record.address,
                record.amount as i64, record.block_height, record.proof_hash
            ],
        ).ok();
    }

    pub fn get_mint_count(&self) -> usize {
        let conn = self.conn.lock().unwrap();
        conn.query_row("SELECT COUNT(*) FROM mints", [], |row| row.get::<_, i64>(0))
            .unwrap_or(0) as usize
    }

    pub fn get_mints_recent(&self, limit: u32) -> Vec<MintRecord> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT seq, txid, address, amount, block_height, proof_hash
             FROM mints ORDER BY seq DESC LIMIT ?1"
        ).unwrap();
        stmt.query_map(params![limit], |row| {
            Ok(MintRecord {
                seq: row.get(0)?,
                txid: row.get(1)?,
                address: row.get(2)?,
                amount: row.get::<_, i64>(3)? as u64,
                block_height: row.get(4)?,
                proof_hash: row.get(5)?,
            })
        }).unwrap().filter_map(|r| r.ok()).collect()
    }

    pub fn get_mints_by_address(&self, address: &str) -> Vec<MintRecord> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT seq, txid, address, amount, block_height, proof_hash
             FROM mints WHERE address = ?1 ORDER BY seq DESC"
        ).unwrap();
        stmt.query_map(params![address], |row| {
            Ok(MintRecord {
                seq: row.get(0)?,
                txid: row.get(1)?,
                address: row.get(2)?,
                amount: row.get::<_, i64>(3)? as u64,
                block_height: row.get(4)?,
                proof_hash: row.get(5)?,
            })
        }).unwrap().filter_map(|r| r.ok()).collect()
    }

    pub fn get_mint_by_seq(&self, seq: u32) -> Option<MintRecord> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT seq, txid, address, amount, block_height, proof_hash
             FROM mints WHERE seq = ?1",
            params![seq],
            |row| Ok(MintRecord {
                seq: row.get(0)?,
                txid: row.get(1)?,
                address: row.get(2)?,
                amount: row.get::<_, i64>(3)? as u64,
                block_height: row.get(4)?,
                proof_hash: row.get(5)?,
            }),
        ).ok()
    }

    pub fn get_mint_by_txid(&self, txid: &str) -> Option<MintRecord> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT seq, txid, address, amount, block_height, proof_hash
             FROM mints WHERE txid = ?1",
            params![txid],
            |row| Ok(MintRecord {
                seq: row.get(0)?,
                txid: row.get(1)?,
                address: row.get(2)?,
                amount: row.get::<_, i64>(3)? as u64,
                block_height: row.get(4)?,
                proof_hash: row.get(5)?,
            }),
        ).ok()
    }

    pub fn get_mints_page(&self, page: u32, limit: u32) -> (Vec<MintRecord>, u32) {
        let total = self.get_mint_count() as u32;
        let offset = (page.saturating_sub(1)) * limit;
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT seq, txid, address, amount, block_height, proof_hash
             FROM mints ORDER BY seq ASC LIMIT ?1 OFFSET ?2"
        ).unwrap();
        let mints = stmt.query_map(params![limit, offset], |row| {
            Ok(MintRecord {
                seq: row.get(0)?,
                txid: row.get(1)?,
                address: row.get(2)?,
                amount: row.get::<_, i64>(3)? as u64,
                block_height: row.get(4)?,
                proof_hash: row.get(5)?,
            })
        }).unwrap().filter_map(|r| r.ok()).collect();
        (mints, total)
    }

    pub fn get_mint_count_by_address(&self, address: &str) -> u32 {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT COUNT(*) FROM mints WHERE address = ?1",
            params![address],
            |row| row.get::<_, i64>(0),
        ).unwrap_or(0) as u32
    }

    // ═══════════════════════════════════════
    //  USED PROOFS
    // ═══════════════════════════════════════

    pub fn has_proof(&self, proof_hash: &str) -> bool {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT 1 FROM used_proofs WHERE proof_hash = ?1",
            params![proof_hash],
            |_| Ok(()),
        ).is_ok()
    }

    pub fn add_proof(&self, proof_hash: &str) {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO used_proofs (proof_hash) VALUES (?1)",
            params![proof_hash],
        ).ok();
    }

    pub fn remove_proof(&self, proof_hash: &str) {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM used_proofs WHERE proof_hash = ?1",
            params![proof_hash],
        ).ok();
    }

    // ═══════════════════════════════════════
    //  TRANSFERS
    // ═══════════════════════════════════════

    pub fn add_transfer(&self, record: &TransferRecord) {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO transfers (txid, from_addr, to_addr, amount, block_height, batch_index)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                record.txid, record.from, record.to,
                record.amount as i64, record.block_height, record.batch_index
            ],
        ).ok();
    }

    pub fn has_transfer(&self, txid: &str, batch_index: u32) -> bool {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT 1 FROM transfers WHERE txid = ?1 AND batch_index = ?2",
            params![txid, batch_index],
            |_| Ok(()),
        ).is_ok()
    }

    pub fn get_transfers_by_address(&self, address: &str) -> Vec<TransferRecord> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT txid, from_addr, to_addr, amount, block_height, batch_index
             FROM transfers WHERE from_addr = ?1 OR to_addr = ?1
             ORDER BY id DESC"
        ).unwrap();
        stmt.query_map(params![address], |row| {
            Ok(TransferRecord {
                txid: row.get(0)?,
                from: row.get(1)?,
                to: row.get(2)?,
                amount: row.get::<_, i64>(3)? as u64,
                block_height: row.get(4)?,
                batch_index: row.get(5)?,
            })
        }).unwrap().filter_map(|r| r.ok()).collect()
    }

    pub fn get_transfer_count(&self) -> usize {
        let conn = self.conn.lock().unwrap();
        conn.query_row("SELECT COUNT(*) FROM transfers", [], |row| row.get::<_, i64>(0))
            .unwrap_or(0) as usize
    }

    // ═══════════════════════════════════════
    //  BLOCK HASHES (reorg 检测)
    // ═══════════════════════════════════════

    pub fn get_block_hash(&self, height: u32) -> Option<String> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT hash FROM block_hashes WHERE height = ?1",
            params![height],
            |row| row.get(0),
        ).ok()
    }

    pub fn set_block_hash(&self, height: u32, hash: &str) {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO block_hashes (height, hash) VALUES (?1, ?2)",
            params![height, hash],
        ).ok();
    }

    pub fn cleanup_block_hashes(&self, keep_from: u32) {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM block_hashes WHERE height < ?1",
            params![keep_from],
        ).ok();
    }

    // ═══════════════════════════════════════
    //  REORG 回滚
    // ═══════════════════════════════════════

    /// 回滚到指定区块高度之前 (不含该高度)
    /// 返回被回滚的 mints 和 transfers
    pub fn rollback_to_height(&self, reorg_height: u32) -> (Vec<MintRecord>, Vec<TransferRecord>) {
        // 获取要回滚的 mints
        let rolled_mints = {
            let conn = self.conn.lock().unwrap();
            let mut stmt = conn.prepare(
                "SELECT seq, txid, address, amount, block_height, proof_hash
                 FROM mints WHERE block_height >= ?1"
            ).unwrap();
            stmt.query_map(params![reorg_height], |row| {
                Ok(MintRecord {
                    seq: row.get(0)?,
                    txid: row.get(1)?,
                    address: row.get(2)?,
                    amount: row.get::<_, i64>(3)? as u64,
                    block_height: row.get(4)?,
                    proof_hash: row.get(5)?,
                })
            }).unwrap().filter_map(|r| r.ok()).collect::<Vec<_>>()
        };

        // 获取要回滚的 transfers
        let rolled_transfers = {
            let conn = self.conn.lock().unwrap();
            let mut stmt = conn.prepare(
                "SELECT txid, from_addr, to_addr, amount, block_height, batch_index
                 FROM transfers WHERE block_height >= ?1"
            ).unwrap();
            stmt.query_map(params![reorg_height], |row| {
                Ok(TransferRecord {
                    txid: row.get(0)?,
                    from: row.get(1)?,
                    to: row.get(2)?,
                    amount: row.get::<_, i64>(3)? as u64,
                    block_height: row.get(4)?,
                    batch_index: row.get(5)?,
                })
            }).unwrap().filter_map(|r| r.ok()).collect::<Vec<_>>()
        };

        // 回滚 transfer 余额
        for t in &rolled_transfers {
            self.add_balance(&t.from, t.amount);
            self.sub_balance(&t.to, t.amount);
        }

        // 回滚 mint 余额 + proof
        for m in &rolled_mints {
            self.sub_balance(&m.address, m.amount);
            self.remove_proof(&m.proof_hash);
        }

        // 删除记录
        {
            let conn = self.conn.lock().unwrap();
            conn.execute("DELETE FROM mints WHERE block_height >= ?1", params![reorg_height]).ok();
            conn.execute("DELETE FROM transfers WHERE block_height >= ?1", params![reorg_height]).ok();
            conn.execute("DELETE FROM block_hashes WHERE height >= ?1", params![reorg_height]).ok();
        }

        // 更新 meta
        let new_minted: u64 = {
            let conn = self.conn.lock().unwrap();
            conn.query_row("SELECT COALESCE(SUM(amount), 0) FROM mints", [], |row| row.get::<_, i64>(0))
                .unwrap_or(0) as u64
        };
        let new_next_seq: u32 = {
            let conn = self.conn.lock().unwrap();
            conn.query_row("SELECT COALESCE(MAX(seq), 0) + 1 FROM mints", [], |row| row.get::<_, i64>(0))
                .unwrap_or(1) as u32
        };
        self.set_minted(new_minted);
        self.set_next_seq(new_next_seq);
        self.set_scan_height(reorg_height.saturating_sub(1));

        (rolled_mints, rolled_transfers)
    }

    // ═══════════════════════════════════════
    //  JSON 迁移 (首次升级用)
    // ═══════════════════════════════════════

    /// 从旧 JSON 文件导入数据到 SQLite
    /// 成功后将 JSON 文件重命名为 .migrated
    pub fn migrate_from_json(&self, state_file: &str, transfers_file: &str, scan_file: &str, block_hashes_file: &str) {
        // 检查是否已有数据 (已迁移过则跳过)
        if self.get_mint_count() > 0 {
            println!("  [db] Already has data, skipping JSON migration");
            return;
        }

        // 迁移 indexer state
        if let Ok(content) = std::fs::read_to_string(state_file) {
            if let Ok(state) = serde_json::from_str::<serde_json::Value>(&content) {
                println!("  [db] Migrating indexer state from JSON...");

                // next_seq + minted
                if let Some(v) = state["next_seq"].as_u64() { self.set_meta_u32("next_seq", v as u32); }
                if let Some(v) = state["minted"].as_u64() { self.set_minted(v); }

                // balances
                if let Some(balances) = state["balances"].as_object() {
                    for (addr, val) in balances {
                        if let Some(amt) = val.as_u64() {
                            if amt > 0 { self.set_balance(addr, amt); }
                        }
                    }
                    println!("  [db] Migrated {} balances", balances.len());
                }

                // used_proofs
                if let Some(proofs) = state["used_proofs"].as_object() {
                    for (hash, _) in proofs { self.add_proof(hash); }
                    println!("  [db] Migrated {} used proofs", proofs.len());
                }

                // mints
                if let Some(mints) = state["mints"].as_array() {
                    for m in mints {
                        let record = MintRecord {
                            seq: m["seq"].as_u64().unwrap_or(0) as u32,
                            txid: m["txid"].as_str().unwrap_or("").to_string(),
                            address: m["address"].as_str().unwrap_or("").to_string(),
                            amount: m["amount"].as_u64().unwrap_or(0),
                            block_height: m["block_height"].as_u64().unwrap_or(0) as u32,
                            proof_hash: m["proof_hash"].as_str().unwrap_or("").to_string(),
                        };
                        self.add_mint(&record);
                    }
                    println!("  [db] Migrated {} mints", mints.len());
                }

                // 重命名旧文件
                std::fs::rename(state_file, format!("{}.migrated", state_file)).ok();
            }
        }

        // 迁移 transfers
        if let Ok(content) = std::fs::read_to_string(transfers_file) {
            if let Ok(transfers) = serde_json::from_str::<Vec<TransferRecord>>(&content) {
                for t in &transfers { self.add_transfer(t); }
                println!("  [db] Migrated {} transfers", transfers.len());
                std::fs::rename(transfers_file, format!("{}.migrated", transfers_file)).ok();
            }
        }

        // 迁移 scan height
        if let Ok(content) = std::fs::read_to_string(scan_file) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(h) = v["scan_height"].as_u64() {
                    self.set_scan_height(h as u32);
                    println!("  [db] Migrated scan height: {}", h);
                }
                std::fs::rename(scan_file, format!("{}.migrated", scan_file)).ok();
            }
        }

        // 迁移 block hashes
        if let Ok(content) = std::fs::read_to_string(block_hashes_file) {
            if let Ok(hashes) = serde_json::from_str::<HashMap<String, String>>(&content) {
                for (h, hash) in &hashes {
                    if let Ok(height) = h.parse::<u32>() {
                        self.set_block_hash(height, hash);
                    }
                }
                println!("  [db] Migrated {} block hashes", hashes.len());
                std::fs::rename(block_hashes_file, format!("{}.migrated", block_hashes_file)).ok();
            }
        }

        println!("  [db] ✅ Migration complete");
    }
}
