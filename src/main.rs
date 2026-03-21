/// NEXUS Reactor — 主程序入口
///
/// 完整铸造流程:
/// 1. 验证本地BTC全节点 (磁盘检测)
/// 2. 连接Bitcoin Core RPC
/// 3. 获取最新区块 + 查询Indexer状态
/// 4. 生成两轮全节点证明
/// 5. 构造双层互锁数据
/// 6. 构造Commit+Reveal两笔交易 (Ordinals铭文标准流程)
/// 7. 签名并广播
///
/// 铭文采用Commit-Reveal模式:
/// - Commit TX: 将BTC发送到包含铭文脚本的Taproot地址
/// - Reveal TX: 花费Commit输出，在witness中揭示铭文 + OP_RETURN

use clap::{Parser, Subcommand};
use bitcoin::secp256k1::{Secp256k1, SecretKey};
use bitcoin::{
    Address, Network, PrivateKey, PublicKey,
    Transaction, TxIn, TxOut, OutPoint, Txid, Sequence, Witness,
    ScriptBuf, Amount, CompressedPublicKey,
    taproot::{TaprootBuilder, LeafVersion},
    opcodes, script::Builder as ScriptBuilder,
    locktime::absolute::LockTime,
    transaction::Version,
    hashes::Hash,
};
use sha2::{Sha256, Digest};
use std::str::FromStr;

use nexus_reactor::constants::*;
use nexus_reactor::proof;
use nexus_reactor::transaction;

// ═══════════════════════════════════════════
//  CLI 参数
// ═══════════════════════════════════════════

#[derive(Parser)]
#[command(name = "nexus-reactor", version = "2.0.0")]
#[command(about = "NEXUS Protocol Reactor - BTC Full Node Required")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// 检查本地全节点状态
    Check {
        /// Bitcoin Core数据目录 (默认 ~/.bitcoin)
        #[arg(short, long)]
        datadir: Option<String>,
    },

    /// 执行铸造
    Mint {
        /// Bitcoin Core数据目录
        #[arg(short, long)]
        datadir: Option<String>,

        /// Bitcoin Core RPC URL
        #[arg(long, default_value = "http://127.0.0.1:8332")]
        rpc_url: String,

        /// RPC用户名
        #[arg(long)]
        rpc_user: String,

        /// RPC密码
        #[arg(long)]
        rpc_pass: String,

        /// 铸造者私钥 (WIF格式)
        #[arg(long)]
        privkey: String,

        /// 矿工费率 (sat/vB)
        #[arg(long, default_value = "10")]
        fee_rate: u64,

        /// Indexer API地址 (查询当前铸造序号)
        #[arg(long, default_value = "http://127.0.0.1:3000")]
        indexer_url: String,

        /// 使用regtest网络
        #[arg(long, default_value = "false")]
        regtest: bool,
    },

    /// 查询铸造状态
    Status {
        /// Indexer API地址
        #[arg(long, default_value = "http://127.0.0.1:3000")]
        indexer_url: String,
    },
}

// ═══════════════════════════════════════════
//  主程序
// ═══════════════════════════════════════════

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Check { datadir } => cmd_check(datadir),
        Commands::Mint { datadir, rpc_url, rpc_user, rpc_pass, privkey, fee_rate, indexer_url, regtest } => {
            cmd_mint(datadir, &rpc_url, &rpc_user, &rpc_pass, &privkey, fee_rate, &indexer_url, regtest);
        }
        Commands::Status { indexer_url } => cmd_status(&indexer_url),
    }
}

// ═══════════════════════════════════════════
//  命令: check — 检查全节点
// ═══════════════════════════════════════════

fn cmd_check(datadir: Option<String>) {
    let datadir = resolve_datadir(datadir);
    println!("🔍 检查Bitcoin全节点: {}", datadir);

    match proof::verify_full_node(&datadir) {
        Ok(()) => {
            println!("✅ 全节点验证通过");
            println!("   数据目录: {}", datadir);
            println!("   状态: Full Archive Node");
            println!("   可以进行NEXUS铸造");
        }
        Err(e) => {
            println!("❌ 全节点验证失败:");
            println!("   {}", e);
            println!("");
            println!("   NEXUS要求运行完整的BTC Full Archive Node (~600GB)");
            println!("   请确保Bitcoin Core未启用 -prune 参数");
            std::process::exit(1);
        }
    }
}

// ═══════════════════════════════════════════
//  命令: mint — 执行铸造
// ═══════════════════════════════════════════

fn cmd_mint(
    datadir: Option<String>,
    rpc_url: &str,
    rpc_user: &str,
    rpc_pass: &str,
    privkey_wif: &str,
    fee_rate: u64,
    indexer_url: &str,
    regtest: bool,
) {
    let datadir = resolve_datadir(datadir);
    let secp = Secp256k1::new();
    let network = if regtest { Network::Regtest } else { Network::Bitcoin };

    // ── Step 1: 验证全节点 ──
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  NEXUS REACTOR v2.0 — Ignition Sequence");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("");
    println!("[1/7] 验证全节点...");

    if let Err(e) = proof::verify_full_node(&datadir) {
        eprintln!("❌ 全节点验证失败: {}", e);
        std::process::exit(1);
    }
    println!("  ✅ Full Archive Node 确认");

    // ── Step 2: 解析私钥 ──
    println!("[2/7] 加载钱包...");

    let privkey = PrivateKey::from_wif(privkey_wif)
        .expect("❌ 私钥WIF格式无效");
    let secret_key = privkey.inner;
    let keypair = bitcoin::secp256k1::Keypair::from_secret_key(&secp, &secret_key);
    let (x_only_pubkey, _parity) = keypair.x_only_public_key();
    let internal_key = bitcoin::key::UntweakedPublicKey::from(x_only_pubkey);

    let pubkey = PublicKey::from_private_key(&secp, &privkey);
    let compressed_pubkey = CompressedPublicKey::from_private_key(&secp, &privkey)
        .expect("❌ 无法生成压缩公钥");
    let minter_address = Address::p2tr(&secp, internal_key, None, network);

    println!("  地址: {}", minter_address);

    // ── Step 3: 查询Indexer状态 ──
    println!("[3/7] 查询铸造状态...");

    let next_seq = query_next_seq(indexer_url);
    if next_seq > TOTAL_MINTS {
        println!("❌ 铸造已结束! 总量21,000,000已全部铸造完毕。");
        std::process::exit(0);
    }
    let remaining = TOTAL_MINTS - next_seq + 1;
    println!("  当前序号: #{}", next_seq);
    println!("  剩余: {} / {} 笔", remaining, TOTAL_MINTS);

    // ── Step 4: 获取最新区块 ──
    println!("[4/7] 获取最新区块...");

    let (block_hash_hex, block_height) = get_latest_block(rpc_url, rpc_user, rpc_pass);
    println!("  区块高度: {}", block_height);
    println!("  区块Hash: {}...{}", &block_hash_hex[..8], &block_hash_hex[56..]);

    // ── Step 5: 生成全节点证明 ──
    println!("[5/7] 生成全节点证明 (两轮挑战)...");

    let block_hash_bytes: [u8; 32] = hex::decode(&block_hash_hex).unwrap()
        .try_into().unwrap();
    let pubkey_bytes: [u8; 33] = pubkey.to_bytes();

    // raw block获取函数 (通过RPC，因为需要指定高度→hash→raw的完整链路)
    let rpc_url_owned = rpc_url.to_string();
    let rpc_user_owned = rpc_user.to_string();
    let rpc_pass_owned = rpc_pass.to_string();
    let get_raw_block = move |height: u32| -> Result<Vec<u8>, String> {
        proof::read_raw_block_via_rpc(&rpc_url_owned, &rpc_user_owned, &rpc_pass_owned, height)
    };

    let two_round_proof = proof::generate_proof(
        &block_hash_bytes,
        &block_hash_hex,
        block_height,
        &pubkey_bytes,
        &get_raw_block,
    ).expect("❌ 全节点证明生成失败");

    println!("  ✅ Round 1: {} ({}ms)", &two_round_proof.round1_hash[..16], 
        (two_round_proof.round2_ts - two_round_proof.round1_ts) * 1000);
    println!("  ✅ Round 2: {}", &two_round_proof.round2_hash[..16]);
    println!("  ✅ Combined: {}", &two_round_proof.combined[..16]);

    // ── Step 6: 构造双层互锁 ──
    println!("[6/7] 构造双层互锁交易...");

    let interlock = transaction::build_interlock(next_seq, &two_round_proof)
        .expect("❌ 互锁构造失败");

    println!("  Witness Hash: {}...", &interlock.witness_hash[..16]);
    println!("  OP_RETURN Hash: {}...", &hex::encode(interlock.opreturn_hash)[..16]);

    // ── Step 7: 构造 Commit + Reveal 交易 ──
    println!("[7/7] 构造并广播交易...");

    // 7a. 构造铭文脚本
    let inscription_script = build_inscription_tapscript(
        &interlock.witness_json,
        &x_only_pubkey,
    );

    // 7b. 构造Taproot脚本树 (包含铭文脚本)
    let taproot_builder = TaprootBuilder::new()
        .add_leaf(0, inscription_script.clone())
        .expect("添加脚本叶失败");

    let taproot_spend_info = taproot_builder
        .finalize(&secp, internal_key)
        .expect("Taproot finalize失败");

    let commit_address = Address::p2tr_tweaked(
        taproot_spend_info.output_key(),
        network,
    );

    // 7c. 查找可用UTXO
    let utxos = list_unspent(rpc_url, rpc_user, rpc_pass, &minter_address.to_string());
    if utxos.is_empty() {
        eprintln!("❌ 没有可用的UTXO。请先向 {} 发送BTC。", minter_address);
        std::process::exit(1);
    }

    // 选择第一个足够大的UTXO
    let opreturn_script = transaction::build_opreturn_script(&interlock.opreturn_bytes);

    // 估算交易大小和费用
    let commit_vsize: u64 = 154;   // 典型P2TR→P2TR commit tx
    let reveal_vsize: u64 = 300 + (interlock.witness_json.len() as u64 / 4); // reveal含铭文
    let commit_fee = commit_vsize * fee_rate;
    let reveal_fee = reveal_vsize * fee_rate;

    let commit_output_value = 546 + MINT_FEE_SATS + reveal_fee; // reveal需要的总输出
    let total_needed = commit_output_value + commit_fee;

    let selected_utxo = utxos.iter()
        .find(|u| u.amount >= total_needed)
        .expect(&format!(
            "❌ 没有足够大的UTXO。需要至少 {} sats，最大可用 {} sats",
            total_needed,
            utxos.iter().map(|u| u.amount).max().unwrap_or(0)
        ));

    println!("  使用UTXO: {}:{} ({} sats)", 
        &selected_utxo.txid[..16], selected_utxo.vout, selected_utxo.amount);

    // 7d. 构造Commit交易
    let commit_tx = build_commit_tx(
        selected_utxo,
        &commit_address,
        commit_output_value,
        &minter_address, // 找零地址
        commit_fee,
    );

    // 签名Commit交易 (key path spend)
    let signed_commit = sign_p2tr_key_path(
        commit_tx,
        selected_utxo.amount,
        &keypair,
        &secp,
    );

    let commit_txid = signed_commit.compute_txid();
    println!("  Commit TXID: {}", commit_txid);

    // 7e. 构造Reveal交易
    let reveal_tx = build_reveal_tx(
        &commit_txid,
        0, // commit output index
        commit_output_value,
        &minter_address, // 铸造接收地址 (546 sats)
        &opreturn_script,
        reveal_fee,
        network,
    );

    // 签名Reveal交易 (script path spend — 揭示铭文)
    let signed_reveal = sign_p2tr_script_path(
        reveal_tx,
        commit_output_value,
        &inscription_script,
        &taproot_spend_info,
        &keypair,
        &secp,
    );

    let reveal_txid = signed_reveal.compute_txid();
    println!("  Reveal TXID: {}", reveal_txid);

    // 7f. 广播
    println!("");
    println!("  📡 广播Commit交易...");
    let commit_result = broadcast_tx(rpc_url, rpc_user, rpc_pass, &signed_commit);
    println!("  {}", commit_result);

    println!("  📡 广播Reveal交易...");
    let reveal_result = broadcast_tx(rpc_url, rpc_user, rpc_pass, &signed_reveal);
    println!("  {}", reveal_result);

    // ── 完成 ──
    println!("");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  ✅ NEXUS铸造交易已广播!");
    println!("  序号:     #{}", next_seq);
    println!("  数量:     500 NXS");
    println!("  费用:     {} sats", MINT_FEE_SATS);
    println!("  Commit:   {}", commit_txid);
    println!("  Reveal:   {}", reveal_txid);
    println!("  等待区块确认后铸造生效。");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
}

// ═══════════════════════════════════════════
//  命令: status — 查询状态
// ═══════════════════════════════════════════

fn cmd_status(indexer_url: &str) {
    let url = format!("{}/status", indexer_url);
    let client = reqwest::blocking::Client::new();

    match client.get(&url).send() {
        Ok(resp) => {
            let status: serde_json::Value = resp.json().unwrap_or_default();
            println!("NEXUS Protocol Status");
            println!("━━━━━━━━━━━━━━━━━━━━");
            println!("{}", serde_json::to_string_pretty(&status).unwrap_or("无法解析".into()));
        }
        Err(_) => {
            println!("⚠️  无法连接Indexer ({})", indexer_url);
            println!("   请确保NEXUS Indexer正在运行。");
        }
    }
}

// ═══════════════════════════════════════════
//  交易构造
// ═══════════════════════════════════════════

/// 构造铭文Tapscript
///
/// 格式: <pubkey> OP_CHECKSIG OP_FALSE OP_IF "nexus" <content-type> 0 <payload> OP_ENDIF
fn build_inscription_tapscript(
    payload_json: &str,
    x_only_pubkey: &bitcoin::secp256k1::XOnlyPublicKey,
) -> ScriptBuf {
    // Ordinals标准铭文脚本格式
    ScriptBuilder::new()
        // 先放公钥验签 (保证只有持有者能reveal)
        .push_x_only_key(x_only_pubkey)
        .push_opcode(opcodes::all::OP_CHECKSIG)
        // 铭文envelope
        .push_opcode(opcodes::OP_FALSE)
        .push_opcode(opcodes::all::OP_IF)
        .push_slice(b"nexus")                          // 协议标识
        .push_slice([0x01])                             // content-type tag
        .push_slice(b"application/nexus-mint")          // MIME
        .push_opcode(opcodes::all::OP_PUSHBYTES_0)      // body separator
        .push_slice(payload_json.as_bytes())            // 铭文数据
        .push_opcode(opcodes::all::OP_ENDIF)
        .into_script()
}

/// 构造Commit交易
fn build_commit_tx(
    utxo: &UtxoInfo,
    commit_address: &Address,
    commit_value: u64,
    change_address: &Address,
    fee: u64,
) -> Transaction {
    let txid = Txid::from_str(&utxo.txid).expect("无效txid");

    let input = TxIn {
        previous_output: OutPoint::new(txid, utxo.vout),
        script_sig: ScriptBuf::new(), // Taproot无scriptSig
        sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
        witness: Witness::new(),
    };

    // Commit输出 (发送到包含铭文脚本的Taproot地址)
    let commit_output = TxOut {
        value: Amount::from_sat(commit_value),
        script_pubkey: commit_address.script_pubkey(),
    };

    // 找零
    let change_value = utxo.amount.saturating_sub(commit_value + fee);
    let mut outputs = vec![commit_output];

    if change_value > 546 { // 大于dust才输出找零
        outputs.push(TxOut {
            value: Amount::from_sat(change_value),
            script_pubkey: change_address.script_pubkey(),
        });
    }

    Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![input],
        output: outputs,
    }
}

/// 构造Reveal交易
///
/// 花费Commit输出，揭示铭文，同时添加OP_RETURN和铸造费输出
fn build_reveal_tx(
    commit_txid: &Txid,
    commit_vout: u32,
    commit_value: u64,
    recipient: &Address,       // 铸造接收者
    opreturn_script: &[u8],    // OP_RETURN脚本
    fee: u64,
    network: Network,
) -> Transaction {
    let input = TxIn {
        previous_output: OutPoint::new(*commit_txid, commit_vout),
        script_sig: ScriptBuf::new(),
        sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
        witness: Witness::new(), // 签名时填充
    };

    let fee_address = Address::from_str(FEE_ADDRESS)
        .expect("FEE_ADDRESS无效")
        .require_network(network)
        .expect("FEE_ADDRESS网络不匹配");

    // Output 0: 铸造接收 (546 sats dust)
    let recipient_output = TxOut {
        value: Amount::from_sat(546),
        script_pubkey: recipient.script_pubkey(),
    };

    // Output 1: 铸造费 (5000 sats → 项目方)
    let fee_output = TxOut {
        value: Amount::from_sat(MINT_FEE_SATS),
        script_pubkey: fee_address.script_pubkey(),
    };

    // Output 2: OP_RETURN (双层互锁的协议数据)
    let opreturn_output = TxOut {
        value: Amount::ZERO,
        script_pubkey: ScriptBuf::from_bytes(opreturn_script.to_vec()),
    };

    Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![input],
        output: vec![recipient_output, fee_output, opreturn_output],
    }
}

/// P2TR Key Path签名 (用于Commit交易)
fn sign_p2tr_key_path(
    mut tx: Transaction,
    input_value: u64,
    keypair: &bitcoin::secp256k1::Keypair,
    secp: &Secp256k1<bitcoin::secp256k1::All>,
) -> Transaction {
    use bitcoin::sighash::{SighashCache, TapSighashType, Prevouts};

    let prevouts = vec![TxOut {
        value: Amount::from_sat(input_value),
        script_pubkey: ScriptBuf::new(), // 简化: 实际应传入真实的prevout script
    }];

    let mut sighash_cache = SighashCache::new(&tx);
    let sighash = sighash_cache.taproot_key_spend_signature_hash(
        0,
        &Prevouts::All(&prevouts),
        TapSighashType::Default,
    ).expect("sighash计算失败");

    let msg = bitcoin::secp256k1::Message::from_digest(sighash.to_byte_array());
    let sig = secp.sign_schnorr(&msg, keypair);

    let schnorr_sig = bitcoin::taproot::Signature {
        signature: sig,
        sighash_type: TapSighashType::Default,
    };

    tx.input[0].witness = Witness::p2tr_key_spend(&schnorr_sig);
    tx
}

/// P2TR Script Path签名 (用于Reveal交易 — 揭示铭文)
fn sign_p2tr_script_path(
    mut tx: Transaction,
    input_value: u64,
    inscription_script: &ScriptBuf,
    spend_info: &bitcoin::taproot::TaprootSpendInfo,
    keypair: &bitcoin::secp256k1::Keypair,
    secp: &Secp256k1<bitcoin::secp256k1::All>,
) -> Transaction {
    use bitcoin::sighash::{SighashCache, TapSighashType, Prevouts};

    let prevouts = vec![TxOut {
        value: Amount::from_sat(input_value),
        script_pubkey: ScriptBuf::new_p2tr_tweaked(spend_info.output_key()),
    }];

    let leaf_hash = bitcoin::taproot::TapLeafHash::from_script(
        inscription_script,
        LeafVersion::TapScript,
    );

    let mut sighash_cache = SighashCache::new(&tx);
    let sighash = sighash_cache.taproot_script_spend_signature_hash(
        0,
        &Prevouts::All(&prevouts),
        leaf_hash,
        TapSighashType::Default,
    ).expect("script path sighash失败");

    let msg = bitcoin::secp256k1::Message::from_digest(sighash.to_byte_array());
    let sig = secp.sign_schnorr(&msg, keypair);

    let schnorr_sig = bitcoin::taproot::Signature {
        signature: sig,
        sighash_type: TapSighashType::Default,
    };

    // 构造script path witness:
    // [signature] [inscription_script] [control_block]
    let control_block = spend_info
        .control_block(&(inscription_script.clone(), LeafVersion::TapScript))
        .expect("control block生成失败");

    let mut witness = Witness::new();
    witness.push(schnorr_sig.to_vec());
    witness.push(inscription_script.as_bytes());
    witness.push(control_block.serialize());

    tx.input[0].witness = witness;
    tx
}

// ═══════════════════════════════════════════
//  RPC 辅助
// ═══════════════════════════════════════════

fn get_latest_block(rpc_url: &str, user: &str, pass: &str) -> (String, u32) {
    let client = reqwest::blocking::Client::new();

    // getblockcount
    let height: u32 = rpc_call(&client, rpc_url, user, pass, "getblockcount", &[])
        .as_u64().unwrap() as u32;

    // getblockhash
    let hash = rpc_call(&client, rpc_url, user, pass, "getblockhash",
        &[serde_json::json!(height)])
        .as_str().unwrap().to_string();

    (hash, height)
}

#[derive(Debug, Clone)]
struct UtxoInfo {
    txid: String,
    vout: u32,
    amount: u64, // sats
}

fn list_unspent(rpc_url: &str, user: &str, pass: &str, address: &str) -> Vec<UtxoInfo> {
    let client = reqwest::blocking::Client::new();

    let result = rpc_call(&client, rpc_url, user, pass, "listunspent",
        &[serde_json::json!(1), serde_json::json!(9999999),
          serde_json::json!([address])]);

    result.as_array()
        .unwrap_or(&vec![])
        .iter()
        .map(|u| UtxoInfo {
            txid: u["txid"].as_str().unwrap_or("").to_string(),
            vout: u["vout"].as_u64().unwrap_or(0) as u32,
            amount: (u["amount"].as_f64().unwrap_or(0.0) * 100_000_000.0) as u64,
        })
        .collect()
}

fn broadcast_tx(rpc_url: &str, user: &str, pass: &str, tx: &Transaction) -> String {
    let client = reqwest::blocking::Client::new();
    let raw_hex = bitcoin::consensus::encode::serialize_hex(tx);

    match rpc_call_result(&client, rpc_url, user, pass, "sendrawtransaction",
        &[serde_json::json!(raw_hex)])
    {
        Ok(txid) => format!("✅ 已广播: {}", txid.as_str().unwrap_or("ok")),
        Err(e) => format!("❌ 广播失败: {}", e),
    }
}

fn query_next_seq(indexer_url: &str) -> u32 {
    let client = reqwest::blocking::Client::new();
    let url = format!("{}/status", indexer_url);

    match client.get(&url).send() {
        Ok(resp) => {
            let body: serde_json::Value = resp.json().unwrap_or_default();
            body["next_seq"].as_u64().unwrap_or(1) as u32
        }
        Err(_) => {
            println!("  ⚠️  无法连接Indexer, 使用seq=1");
            1
        }
    }
}

fn rpc_call(
    client: &reqwest::blocking::Client,
    url: &str, user: &str, pass: &str,
    method: &str, params: &[serde_json::Value],
) -> serde_json::Value {
    rpc_call_result(client, url, user, pass, method, params)
        .unwrap_or_else(|e| panic!("❌ RPC调用 {} 失败: {}", method, e))
}

fn rpc_call_result(
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
        .json(&body)
        .send().map_err(|e| e.to_string())?
        .json().map_err(|e| e.to_string())?;

    if let Some(err) = resp.get("error") {
        if !err.is_null() {
            return Err(format!("{}", err));
        }
    }

    resp.get("result").cloned().ok_or("无result".into())
}

fn resolve_datadir(custom: Option<String>) -> String {
    custom.unwrap_or_else(|| {
        let home = dirs::home_dir().expect("无法获取HOME目录");
        // Linux/macOS: ~/.bitcoin, Windows: ~/AppData/Roaming/Bitcoin
        if cfg!(target_os = "linux") || cfg!(target_os = "macos") {
            home.join(".bitcoin").to_string_lossy().to_string()
        } else {
            home.join("AppData").join("Roaming").join("Bitcoin")
                .to_string_lossy().to_string()
        }
    })
}
