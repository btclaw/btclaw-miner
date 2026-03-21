mod ui;
/// NEXUS Reactor — 交互式CLI菜单
///
/// [1] 安装/同步 BTC全节点
/// [2] 查看同步进度
/// [3] 测试网铸造 (regtest)
/// [4] 主网铸造状态 + 执行铸造
/// [5] 钱包信息 (UTXO/余额/代币)
/// [0] 退出

use std::io::{self, Write};
use std::process::Command;
use bitcoin::secp256k1::Secp256k1;
use bitcoin::key::TapTweak;
use bitcoin::{
    Address, Network, PrivateKey, PublicKey,
    Transaction, TxIn, TxOut, OutPoint, Txid, Sequence, Witness,
    ScriptBuf, Amount, CompressedPublicKey,
    taproot::{TaprootBuilder, LeafVersion},
    opcodes, script::Builder as ScriptBuilder,
    locktime::absolute::LockTime,
    transaction::Version,
    hashes::Hash,
    script::PushBytesBuf,
};
use std::str::FromStr;

use nexus_reactor::constants::*;
use nexus_reactor::proof;
use nexus_reactor::transaction;

// ═══════════════════════════════════════════
//  主菜单
// ═══════════════════════════════════════════

fn main() {
    print_banner();

    loop {

        ui::main_menu();

        let choice = read_line().trim().to_string();
        println!("");

        match choice.as_str() {
            "1" => menu_install_node(),
            "2" => menu_sync_progress(),
            "3" => menu_testnet_mint(),
            "4" => menu_mainnet_mint(),
            "5" => menu_wallet_info(),
            "0" | "q" | "exit" => {
                println!("  👋 再见!");
                break;
            }
            _ => println!("  ⚠️  无效选择，请输入 0-5"),
        }
    }
}

fn print_banner() {
    ui::banner();
}

// ═══════════════════════════════════════════
//  [1] 安装/同步 BTC全节点
// ═══════════════════════════════════════════

fn menu_install_node() {
    ui::sub_menu_node();

    match read_line().trim() {
        "1" => install_bitcoin_core(),
        "2" => start_mainnet_sync(),
        "3" => start_regtest_node(),
        _ => return,
    }
}

fn install_bitcoin_core() {
    println!("");
    println!("  📦 检查 Bitcoin Core...");

    let check = Command::new("bitcoind").arg("--version").output();
    if let Ok(output) = check {
        if output.status.success() {
            let ver = String::from_utf8_lossy(&output.stdout);
            println!("  ✅ 已安装: {}", ver.lines().next().unwrap_or(""));
            return;
        }
    }

    println!("  ⏳ 开始安装 Bitcoin Core 28.0...");
    println!("  (这需要下载约30MB, 请稍候)");
    println!("");

    let script = r#"
        cd /tmp
        ARCH=$(uname -m)
        if [ "$ARCH" = "x86_64" ]; then BA="x86_64-linux-gnu"; 
        elif [ "$ARCH" = "aarch64" ]; then BA="aarch64-linux-gnu";
        else echo "不支持的架构"; exit 1; fi
        wget -q "https://bitcoincore.org/bin/bitcoin-core-28.0/bitcoin-28.0-${BA}.tar.gz"
        tar xzf "bitcoin-28.0-${BA}.tar.gz"
        sudo install -m 0755 bitcoin-28.0/bin/* /usr/local/bin/
        rm -rf bitcoin-28.0 "bitcoin-28.0-${BA}.tar.gz"
        echo "DONE"
    "#;

    let output = Command::new("bash").arg("-c").arg(script).output();
    match output {
        Ok(o) if String::from_utf8_lossy(&o.stdout).contains("DONE") => {
            println!("  ✅ Bitcoin Core 安装完成!");
        }
        _ => println!("  ❌ 安装失败, 请手动安装: https://bitcoincore.org/en/download/"),
    }
}

fn start_mainnet_sync() {
    println!("");
    println!("  ⚠️  主网同步需要:");
    println!("    - 磁盘空间: ~600GB (SSD推荐)");
    println!("    - 同步时间: 1-7天 (取决于网络和硬件)");
    println!("    - 内存: 建议 4GB+");
    println!("");
    print!("  确定开始? (y/n) > ");
    io::stdout().flush().unwrap();

    if read_line().trim().to_lowercase() != "y" {
        println!("  已取消");
        return;
    }

    // 写配置
    std::fs::create_dir_all(expand_home("~/.bitcoin")).ok();
    let conf = r#"server=1
txindex=1
rpcuser=nexus
rpcpassword=nexustest123
dbcache=2048

[main]
rpcport=8332
rpcallowip=127.0.0.1
rpcbind=127.0.0.1
"#;
    std::fs::write(expand_home("~/.bitcoin/bitcoin.conf"), conf).ok();

    // 启动
    let _ = Command::new("bitcoind").arg("-daemon").spawn();
    println!("  ✅ 主网节点已启动! 使用选项 [2] 查看同步进度");
}

fn start_regtest_node() {
    println!("");
    println!("  🧪 启动regtest测试节点...");

    // 停旧的
    let _ = Command::new("bitcoin-cli")
        .args(["-regtest", "-rpcuser=nexus", "-rpcpassword=nexustest123", "stop"])
        .output();
    std::thread::sleep(std::time::Duration::from_secs(2));

    // 写配置
    std::fs::create_dir_all(expand_home("~/.bitcoin")).ok();
    let conf = r#"regtest=1
server=1
txindex=1
rpcuser=nexus
rpcpassword=nexustest123
fallbackfee=0.00001

[regtest]
rpcport=18443
rpcallowip=127.0.0.1
rpcbind=127.0.0.1
acceptnonstdtxn=1
datacarriersize=100000
"#;
    std::fs::write(expand_home("~/.bitcoin/bitcoin.conf"), conf).ok();

    // 启动
    let _ = Command::new("bitcoind").arg("-regtest").arg("-daemon").spawn();
    std::thread::sleep(std::time::Duration::from_secs(3));

    // 创建钱包
    btc_cli(&["createwallet", "nexus_test"], true);

    // 挖200个区块
    let addr = btc_cli_output(&["getnewaddress", "", "bech32m"], true);
    let addr = addr.trim();
    btc_cli(&["generatetoaddress", "200", addr], true);

    let balance = btc_cli_output(&["getbalance"], true);
    println!("  ✅ regtest节点已启动!");
    println!("     区块: 200");
    println!("     余额: {} BTC", balance.trim());
}

// ═══════════════════════════════════════════
//  [2] 查看同步进度
// ═══════════════════════════════════════════

fn menu_sync_progress() {
    println!("  ══ 同步进度 ══");
    println!("");

    // 先试主网
    let mainnet = btc_cli_output(&["getblockchaininfo"], false);

    if !mainnet.is_empty() && !mainnet.contains("error") {
        match serde_json::from_str::<serde_json::Value>(&mainnet) {
            Ok(info) => {
                let chain = info["chain"].as_str().unwrap_or("?");
                let blocks = info["blocks"].as_u64().unwrap_or(0);
                let headers = info["headers"].as_u64().unwrap_or(0);
                let progress = info["verificationprogress"].as_f64().unwrap_or(0.0);
                let ibd = info["initialblockdownload"].as_bool().unwrap_or(true);
                let size_on_disk = info["size_on_disk"].as_u64().unwrap_or(0);

                println!("  网络:       {}", chain);
                println!("  已同步区块: {}", blocks);
                println!("  最新区块:   {}", headers);

                if headers > 0 {
                    let remaining = headers.saturating_sub(blocks);
                    let pct = progress * 100.0;
                    
                    // 进度条
                    let bar_width = 30;
                    let filled = (pct / 100.0 * bar_width as f64) as usize;
                    let empty = bar_width - filled;
                    let bar = format!("[{}{}]", "█".repeat(filled), "░".repeat(empty));

                    println!("  进度:       {} {:.2}%", bar, pct);
                    println!("  剩余区块:   {}", remaining);
                    println!("  磁盘占用:   {:.2} GB", size_on_disk as f64 / 1e9);
                    
                    if ibd {
                        // 估算剩余时间
                        println!("  状态:       🔄 正在同步 (IBD)");
                        if blocks > 1000 {
                            println!("  提示:       预计还需数小时到数天");
                        }
                    } else {
                        println!("  状态:       ✅ 同步完成!");
                    }
                }
                return;
            }
            Err(_) => {}
        }
    }

    // 试regtest
    let regtest = btc_cli_output(&["getblockchaininfo"], true);
    if !regtest.is_empty() && !regtest.contains("error") {
        match serde_json::from_str::<serde_json::Value>(&regtest) {
            Ok(info) => {
                let blocks = info["blocks"].as_u64().unwrap_or(0);
                println!("  网络:       regtest (测试网)");
                println!("  区块高度:   {}", blocks);
                println!("  状态:       ✅ 本地测试网运行中");
                return;
            }
            Err(_) => {}
        }
    }

    println!("  ❌ 没有检测到运行中的Bitcoin Core节点");
    println!("     请先使用选项 [1] 安装并启动节点");
}

// ═══════════════════════════════════════════
//  [3] 测试网铸造 (regtest)
// ═══════════════════════════════════════════

fn menu_testnet_mint() {
    println!("  ══ 测试网铸造 (regtest) ══");
    println!("");

    // 检查regtest节点
    let info = btc_cli_output(&["getblockchaininfo"], true);
    if info.is_empty() || info.contains("error") {
        println!("  ❌ regtest节点未运行。请先用选项 [1c] 启动");
        return;
    }

    // 检查全节点
    let datadir = expand_home("~/.bitcoin/regtest");
    print!("  [1/4] 验证全节点... ");
    io::stdout().flush().unwrap();
    
    #[cfg(feature = "regtest")]
    match proof::verify_full_node(&datadir) {
        Ok(()) => println!("✅"),
        Err(e) => { println!("❌ {}", e); return; }
    }
    #[cfg(not(feature = "regtest"))]
    {
        println!("⚠️  请用 --features regtest 编译测试版");
        return;
    }

    // 生成/获取私钥
    println!("  [2/4] 准备钱包...");
    let addr = btc_cli_output(&["getnewaddress", "nexus_minter", "bech32m"], true);
    let addr = addr.trim();

    // 充值
    let _ = btc_cli_output(&["sendtoaddress", addr, "1.0"], true);
    let _ = btc_cli_output(&["generatetoaddress", "1", addr], true);

    // 生成私钥 (用Python因为Bitcoin Core 28移除了dumpprivkey)
    let privkey_output = Command::new("python3")
        .arg("-c")
        .arg(r#"
import os, hashlib, base58
privkey = os.urandom(32)
payload = b'\xef' + privkey + b'\x01'
checksum = hashlib.sha256(hashlib.sha256(payload).digest()).digest()[:4]
wif = base58.b58encode(payload + checksum).decode()
print(wif)
"#)
        .output()
        .expect("需要python3和base58库");
    let privkey_wif = String::from_utf8_lossy(&privkey_output.stdout).trim().to_string();

    // 导入私钥
    let desc_info = btc_cli_output(
        &["getdescriptorinfo", &format!("tr({})", privkey_wif)], true
    );
    let checksum = serde_json::from_str::<serde_json::Value>(&desc_info)
        .ok()
        .and_then(|v| v["checksum"].as_str().map(|s| s.to_string()))
        .unwrap_or_default();

    let import_json = format!(
        "[{{\"desc\": \"tr({})#{}\", \"timestamp\": \"now\"}}]",
        privkey_wif, checksum
    );
    let _ = btc_cli_output(&["importdescriptors", &import_json], true);

    // 获取地址
    let derive_desc = format!("tr({})#{}", privkey_wif, checksum);
    let addr_json = btc_cli_output(&["deriveaddresses", &derive_desc], true);
    let minter_addr = serde_json::from_str::<serde_json::Value>(&addr_json)
        .ok()
        .and_then(|v| v.as_array()?.first()?.as_str().map(|s| s.to_string()))
        .unwrap_or_default();

    // 给铸造者地址充值
    let _ = btc_cli_output(&["sendtoaddress", &minter_addr, "0.5"], true);
    let _ = btc_cli_output(&["generatetoaddress", "1", &minter_addr], true);

    println!("    铸造地址: {}", minter_addr);
    println!("    私钥: {}", privkey_wif);

    // 执行铸造
    println!("  [3/4] 执行铸造...");
    println!("");

    let result = execute_mint(
        &expand_home("~/.bitcoin/regtest"),
        "http://127.0.0.1:18443",
        "nexus",
        "nexustest123",
        &privkey_wif,
        1,
        Network::Regtest,
    );

    match result {
        Ok((commit_txid, reveal_txid)) => {
            // 挖块确认
            println!("  [4/4] 挖块确认...");
            let _ = btc_cli_output(&["generatetoaddress", "1", &minter_addr], true);

            println!("");
            println!("  ✅ 测试网铸造成功!");
            println!("  Commit: {}", commit_txid);
            println!("  Reveal: {}", reveal_txid);
            println!("");

            // 验证链上数据
            println!("");
            println!("  ── 链上验证 On-Chain Verification ──");
            println!("");
            let tx_raw = btc_cli_output(&["getrawtransaction", &reveal_txid, "1"], true);
            if let Ok(tx) = serde_json::from_str::<serde_json::Value>(&tx_raw) {
                let confirms = tx["confirmations"].as_u64().unwrap_or(0);
                println!("  确认数 Confirmations: {}", confirms);
                println!("");

                // Outputs
                if let Some(vouts) = tx["vout"].as_array() {
                    for (i, out) in vouts.iter().enumerate() {
                        let val = out["value"].as_f64().unwrap_or(0.0);
                        let typ = out["scriptPubKey"]["type"].as_str().unwrap_or("?");
                        println!("  Output[{}]: {} BTC ({})", i, val, typ);
                    }
                }
                println!("");

                // 铭文层解码
                if let Some(wit) = tx["vin"][0]["txinwitness"].as_array() {
                    if wit.len() >= 2 {
                        let script_hex = wit[1].as_str().unwrap_or("");
                        // 找JSON: 7b22 = {"
                        if let Some(idx) = script_hex.find("7b22") {
                            let json_hex = &script_hex[idx..];
                            let mut depth: i32 = 0;
                            let mut end = 0;
                            let bytes_iter: Vec<u8> = (0..json_hex.len()/2)
                                .filter_map(|i| u8::from_str_radix(&json_hex[i*2..i*2+2], 16).ok())
                                .collect();
                            for (i, &b) in bytes_iter.iter().enumerate() {
                                if b == b'{' { depth += 1; }
                                if b == b'}' { depth -= 1; if depth == 0 { end = i + 1; break; } }
                            }
                            if end > 0 {
                                let json_bytes: Vec<u8> = bytes_iter[..end].to_vec();
                                if let Ok(json_str) = String::from_utf8(json_bytes) {
                                    println!("  ┌── Witness Layer / 铭文层 ──");
                                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&json_str) {
                                        println!("  │ Protocol:    {}", parsed["p"].as_str().unwrap_or("?"));
                                        println!("  │ Operation:   {}", parsed["op"].as_str().unwrap_or("?"));
                                        // seq由Indexer分配，链上数据不含seq
                                        println!("  │ Amount:      {} NXS", parsed["amt"]);
                                        println!("  │ Node Proof:  {}", parsed["fnp"].as_str().unwrap_or("?"));
                                        println!("  │ OPR Hash:    {}", parsed["opr"].as_str().unwrap_or("?"));
                                    }
                                    println!("  └─────────────────────────────");
                                }
                            }
                        }
                    }
                }
                println!("");

                // OP_RETURN层解码
                if let Some(vouts) = tx["vout"].as_array() {
                    for out in vouts {
                        if out["scriptPubKey"]["type"].as_str() == Some("nulldata") {
                            let hex_data = out["scriptPubKey"]["hex"].as_str().unwrap_or("");
                            if hex_data.len() > 6 {
                                let data_start = if &hex_data[2..4] == "4c" { 6 } else { 4 };
                                if let Ok(data) = hex::decode(&hex_data[data_start..]) {
                                    if data.len() >= 68 {
                                        println!("  ┌── OP_RETURN Layer / 协议层 ──");
                                        println!("  │ Magic:       {}", String::from_utf8_lossy(&data[0..3]));
                                        println!("  │ Version:     {}", data[3]);
                                        println!("  │ Wit Hash:    {}", hex::encode(&data[4..36]));
                                        println!("  │ Proof Hash:  {}", hex::encode(&data[36..68]));
                                        println!("  └────────────────────────────────");
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        Err(e) => {
            println!("  ❌ 铸造失败: {}", e);
        }
    }
}

// ═══════════════════════════════════════════
//  [4] 主网铸造
// ═══════════════════════════════════════════

fn menu_mainnet_mint() {
    println!("  ══ 主网铸造 ══");
    println!("");

    // 检查主网节点
    let info = btc_cli_output(&["getblockchaininfo"], false);
    if info.is_empty() || info.contains("error") {
        println!("  ❌ 主网节点未运行。请先用选项 [1b] 启动并完成同步");
        return;
    }

    let parsed = serde_json::from_str::<serde_json::Value>(&info).ok();
    let ibd = parsed.as_ref()
        .and_then(|v| v["initialblockdownload"].as_bool())
        .unwrap_or(true);

    if ibd {
        println!("  ⚠️  节点还在同步中(IBD), 请等待同步完成后再铸造");
        println!("     使用选项 [2] 查看同步进度");
        return;
    }

    // 验证全节点
    let datadir = expand_home("~/.bitcoin");
    print!("  验证全节点... ");
    io::stdout().flush().unwrap();

    #[cfg(not(feature = "regtest"))]
    match proof::verify_full_node(&datadir) {
        Ok(()) => println!("✅"),
        Err(e) => { println!("❌ {}", e); return; }
    }
    #[cfg(feature = "regtest")]
    {
        println!("⚠️  当前是regtest编译版, 请用 cargo build --release 编译主网版");
        return;
    }

    // 输入私钥
    println!("");
    print!("  输入你的WIF私钥: ");
    io::stdout().flush().unwrap();
    let privkey_wif = read_line().trim().to_string();

    if privkey_wif.is_empty() {
        println!("  已取消");
        return;
    }

    // 输入费率
    print!("  矿工费率 (sat/vB, 建议10-50): ");
    io::stdout().flush().unwrap();
    let fee_rate: u64 = read_line().trim().parse().unwrap_or(10);

    println!("");
    println!("  ⚠️  即将在BTC主网执行铸造!");
    println!("     费用: {} sats 铸造费 + 矿工费", MINT_FEE_SATS);
    print!("  确认? (yes/no) > ");
    io::stdout().flush().unwrap();

    if read_line().trim() != "yes" {
        println!("  已取消");
        return;
    }

    let result = execute_mint(
        &datadir,
        "http://127.0.0.1:8332",
        "nexus",
        "nexustest123",
        &privkey_wif,
        fee_rate,
        Network::Bitcoin,
    );

    match result {
        Ok((commit, reveal)) => {
            println!("");
            println!("  ✅ 主网铸造交易已广播!");
            println!("  Commit: {}", commit);
            println!("  Reveal: {}", reveal);
            println!("  等待矿工确认...");
        }
        Err(e) => println!("  ❌ 铸造失败: {}", e),
    }
}

// ═══════════════════════════════════════════
//  [5] 钱包信息
// ═══════════════════════════════════════════

fn menu_wallet_info() {
    ui::sub_menu_wallet();

    let (is_regtest, rpc_port) = match read_line().trim() {
        "1" => (true, "18443"),
        "2" => (false, "8332"),
        _ => return,
    };

    println!("");

    // 余额
    let balance = if is_regtest {
        btc_cli_output(&["getbalance"], true)
    } else {
        btc_cli_output(&["getbalance"], false)
    };

    if balance.contains("error") || balance.is_empty() {
        println!("  ❌ 无法连接节点");
        return;
    }

    println!("  💰 BTC余额: {} BTC", balance.trim());

    // UTXO列表
    let utxo_json = if is_regtest {
        btc_cli_output(&["listunspent"], true)
    } else {
        btc_cli_output(&["listunspent"], false)
    };

    if let Ok(utxos) = serde_json::from_str::<serde_json::Value>(&utxo_json) {
        if let Some(arr) = utxos.as_array() {
            println!("  📦 UTXO数量: {}", arr.len());
            println!("");

            let total_sats: f64 = arr.iter()
                .map(|u| u["amount"].as_f64().unwrap_or(0.0))
                .sum();
            println!("  总计: {:.8} BTC ({} sats)", total_sats, (total_sats * 1e8) as u64);
            println!("");

            // 显示前10个UTXO
            let show = arr.len().min(10);
            for (i, utxo) in arr.iter().take(show).enumerate() {
                let txid = utxo["txid"].as_str().unwrap_or("?");
                let vout = utxo["vout"].as_u64().unwrap_or(0);
                let amount = utxo["amount"].as_f64().unwrap_or(0.0);
                let confirms = utxo["confirmations"].as_u64().unwrap_or(0);
                let addr = utxo["address"].as_str().unwrap_or("?");
                println!("  [{}] {}...:{} | {:.8} BTC | {} confirms",
                    i + 1, &txid[..16], vout, amount, confirms);
                println!("       {}", addr);
            }
            if arr.len() > 10 {
                println!("  ... 还有 {} 个UTXO未显示", arr.len() - 10);
            }
        }
    }

    // NEXUS代币余额 (查询Indexer)
    println!("");
    println!("  ── NEXUS代币 ──");

    let indexer_url = if is_regtest {
        "http://127.0.0.1:3000"
    } else {
        "http://127.0.0.1:3000"
    };

    let client = reqwest::blocking::Client::new();
    match client.get(&format!("{}/status", indexer_url)).send() {
        Ok(resp) => {
            if let Ok(status) = resp.json::<serde_json::Value>() {
                let minted = status["minted"].as_u64().unwrap_or(0);
                let total = status["total_supply"].as_u64().unwrap_or(MAX_SUPPLY);
                let next_seq = status["next_seq"].as_u64().unwrap_or(1);
                let remaining = TOTAL_MINTS as u64 - (next_seq - 1);

                println!("  铸造进度: {}/{} NXS", minted / 100000000, total / 100000000);
                println!("  已铸造笔数: {}/{}", next_seq - 1, TOTAL_MINTS);
                println!("  剩余: {} 笔", remaining);
            }
        }
        Err(_) => {
            println!("  ⚠️  Indexer未运行, 无法查询NEXUS代币余额");
            println!("     Indexer启动后将在此显示代币信息");
        }
    }
}

// ═══════════════════════════════════════════
//  铸造执行引擎 (主网/测试网共用)
// ═══════════════════════════════════════════

fn execute_mint(
    datadir: &str,
    rpc_url: &str,
    rpc_user: &str,
    rpc_pass: &str,
    privkey_wif: &str,
    fee_rate: u64,
    network: Network,
) -> Result<(String, String), String> {
    let secp = Secp256k1::new();

    // 解析私钥
    let privkey = PrivateKey::from_wif(privkey_wif)
        .map_err(|e| format!("私钥WIF格式无效: {}", e))?;
    let secret_key = privkey.inner;
    let keypair = bitcoin::secp256k1::Keypair::from_secret_key(&secp, &secret_key);
    let (x_only_pubkey, _) = keypair.x_only_public_key();
    let internal_key = bitcoin::key::UntweakedPublicKey::from(x_only_pubkey);
    let pubkey = PublicKey::from_private_key(&secp, &privkey);
    let minter_address = Address::p2tr(&secp, internal_key, None, network);

    println!("    地址: {}", minter_address);

    // 查询铸造进度
    let next_seq = query_indexer_seq();
    if next_seq > TOTAL_MINTS {
        return Err("铸造已结束，42,000笔全部完成".into());
    }
    println!("    进度: {} / {} mints remaining", TOTAL_MINTS - next_seq + 1, TOTAL_MINTS);

    // 获取最新区块
    let (block_hash_hex, block_height) = get_latest_block(rpc_url, rpc_user, rpc_pass)?;
    println!("    区块: {} ({})", block_height, &block_hash_hex[..12]);

    // 生成全节点证明
    print!("    生成证明... ");
    io::stdout().flush().unwrap();

    let block_hash_bytes: [u8; 32] = hex::decode(&block_hash_hex)
        .map_err(|e| e.to_string())?.try_into().map_err(|_| "hash len")?;
    let pubkey_bytes: [u8; 33] = pubkey.to_bytes().try_into()
        .map_err(|_| "pubkey len")?;

    let rpc_u = rpc_url.to_string();
    let rpc_usr = rpc_user.to_string();
    let rpc_pw = rpc_pass.to_string();
    let get_raw = move |h: u32| -> Result<Vec<u8>, String> {
        proof::read_raw_block_via_rpc(&rpc_u, &rpc_usr, &rpc_pw, h)
    };

    let two_round = proof::generate_proof(
        &block_hash_bytes, &block_hash_hex, block_height, &pubkey_bytes, &get_raw,
    ).map_err(|e| format!("证明生成失败: {}", e))?;
    println!("✅ ({}s)", two_round.round2_ts - two_round.round1_ts);

    // 构造互锁
    let interlock = transaction::build_interlock(&two_round)
        .map_err(|e| format!("互锁构造失败: {}", e))?;
    println!("    互锁: ✅");

    // 构造铭文脚本
    let inscription_script = ScriptBuilder::new()
        .push_x_only_key(&x_only_pubkey)
        .push_opcode(opcodes::all::OP_CHECKSIG)
        .push_opcode(opcodes::OP_FALSE)
        .push_opcode(opcodes::all::OP_IF)
        .push_slice(b"nexus")
        .push_slice([0x01])
        .push_slice(b"application/nexus-mint")
        .push_opcode(opcodes::all::OP_PUSHBYTES_0)
        .push_slice(PushBytesBuf::try_from(interlock.witness_json.as_bytes().to_vec())
            .map_err(|e| format!("payload too large: {}", e))?)
        .push_opcode(opcodes::all::OP_ENDIF)
        .into_script();

    // Taproot脚本树
    let taproot_builder = TaprootBuilder::new()
        .add_leaf(0, inscription_script.clone())
        .map_err(|e| format!("taproot leaf: {:?}", e))?;

    let spend_info = taproot_builder
        .finalize(&secp, internal_key)
        .map_err(|_| "taproot finalize失败")?;

    let commit_address = Address::p2tr_tweaked(spend_info.output_key(), network);

    // UTXO
    let utxos = list_unspent_rpc(rpc_url, rpc_user, rpc_pass, &minter_address.to_string())?;
    if utxos.is_empty() {
        return Err(format!("没有可用UTXO, 请先向 {} 发送BTC", minter_address));
    }

    let opreturn_script = transaction::build_opreturn_script(&interlock.opreturn_bytes);

    let reveal_vsize: u64 = 300 + (interlock.witness_json.len() as u64 / 4);
    let reveal_fee = reveal_vsize * fee_rate;
    let commit_output_value = 330 + MINT_FEE_SATS + reveal_fee;
    let commit_fee = 154 * fee_rate;
    let total_needed = commit_output_value + commit_fee;

    let utxo = utxos.iter().find(|u| u.2 >= total_needed)
        .ok_or(format!("UTXO不足, 需要 {} sats", total_needed))?;

    println!("    UTXO: {}...:{} ({} sats)", &utxo.0, utxo.1, utxo.2);

    // Commit交易
    let txid = Txid::from_str(&utxo.0).map_err(|e| e.to_string())?;
    let change_value = utxo.2.saturating_sub(commit_output_value + commit_fee);

    let mut commit_outputs = vec![TxOut {
        value: Amount::from_sat(commit_output_value),
        script_pubkey: commit_address.script_pubkey(),
    }];
    if change_value > 330 {
        commit_outputs.push(TxOut {
            value: Amount::from_sat(change_value),
            script_pubkey: minter_address.script_pubkey(),
        });
    }

    let mut commit_tx = Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: OutPoint::new(txid, utxo.1),
            script_sig: ScriptBuf::new(),
            sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
            witness: Witness::new(),
        }],
        output: commit_outputs,
    };

    // 签名Commit (key path, tweaked)
    {
        use bitcoin::sighash::{SighashCache, TapSighashType, Prevouts};
        let prevouts = [TxOut {
            value: Amount::from_sat(utxo.2),
            script_pubkey: minter_address.script_pubkey(),
        }];
        let mut cache = SighashCache::new(&commit_tx);
        let sighash = cache.taproot_key_spend_signature_hash(
            0, &Prevouts::All(&prevouts), TapSighashType::Default,
        ).map_err(|e| e.to_string())?;
        let msg = bitcoin::secp256k1::Message::from_digest(sighash.to_byte_array());
        let tweaked = keypair.tap_tweak(&secp, None);
        let sig = secp.sign_schnorr(&msg, &tweaked.to_inner());
        let schnorr_sig = bitcoin::taproot::Signature { signature: sig, sighash_type: TapSighashType::Default };
        commit_tx.input[0].witness = Witness::p2tr_key_spend(&schnorr_sig);
    }

    let commit_txid = commit_tx.compute_txid();

    // Reveal交易
    let fee_addr = Address::from_str(FEE_ADDRESS)
        .map_err(|e| format!("FEE_ADDRESS无效: {}", e))?
        .require_network(network)
        .map_err(|e| format!("FEE_ADDRESS网络不匹配: {}", e))?;

    let mut reveal_tx = Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: OutPoint::new(commit_txid, 0),
            script_sig: ScriptBuf::new(),
            sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
            witness: Witness::new(),
        }],
        output: vec![
            TxOut { value: Amount::from_sat(330), script_pubkey: minter_address.script_pubkey() },
            TxOut { value: Amount::from_sat(MINT_FEE_SATS), script_pubkey: fee_addr.script_pubkey() },
            TxOut { value: Amount::ZERO, script_pubkey: ScriptBuf::from_bytes(opreturn_script) },
        ],
    };

    // 签名Reveal (script path)
    {
        use bitcoin::sighash::{SighashCache, TapSighashType, Prevouts};
        let prevouts = [TxOut {
            value: Amount::from_sat(commit_output_value),
            script_pubkey: ScriptBuf::new_p2tr_tweaked(spend_info.output_key()),
        }];
        let leaf_hash = bitcoin::taproot::TapLeafHash::from_script(
            &inscription_script, LeafVersion::TapScript,
        );
        let mut cache = SighashCache::new(&reveal_tx);
        let sighash = cache.taproot_script_spend_signature_hash(
            0, &Prevouts::All(&prevouts), leaf_hash, TapSighashType::Default,
        ).map_err(|e| e.to_string())?;
        let msg = bitcoin::secp256k1::Message::from_digest(sighash.to_byte_array());
        let sig = secp.sign_schnorr(&msg, &keypair);
        let schnorr_sig = bitcoin::taproot::Signature { signature: sig, sighash_type: TapSighashType::Default };
        let control_block = spend_info
            .control_block(&(inscription_script.clone(), LeafVersion::TapScript))
            .ok_or("control block失败")?;
        let mut witness = Witness::new();
        witness.push(schnorr_sig.to_vec());
        witness.push(inscription_script.as_bytes());
        witness.push(control_block.serialize());
        reveal_tx.input[0].witness = witness;
    }

    let reveal_txid = reveal_tx.compute_txid();

    // 广播
    print!("    广播Commit... ");
    io::stdout().flush().unwrap();
    broadcast(rpc_url, rpc_user, rpc_pass, &commit_tx)?;
    println!("✅ {}", commit_txid);

    print!("    广播Reveal... ");
    io::stdout().flush().unwrap();
    broadcast(rpc_url, rpc_user, rpc_pass, &reveal_tx)?;
    println!("✅ {}", reveal_txid);

    Ok((commit_txid.to_string(), reveal_txid.to_string()))
}

// ═══════════════════════════════════════════
//  辅助函数
// ═══════════════════════════════════════════

fn read_line() -> String {
    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap_or(0);
    input
}

fn expand_home(path: &str) -> String {
    if path.starts_with("~/") || path == "~" {
        if let Some(home) = dirs::home_dir() {
            return path.replacen("~", &home.to_string_lossy(), 1);
        }
    }
    path.to_string()
}

fn btc_cli(args: &[&str], regtest: bool) -> bool {
    let output = btc_cli_output(args, regtest);
    !output.contains("error")
}

fn btc_cli_output(args: &[&str], regtest: bool) -> String {
    let mut cmd = Command::new("bitcoin-cli");
    if regtest {
        cmd.arg("-regtest");
    }
    cmd.args(["-rpcuser=nexus", "-rpcpassword=nexustest123"]);
    cmd.args(args);

    match cmd.output() {
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout).to_string();
            let stderr = String::from_utf8_lossy(&o.stderr).to_string();
            if !stderr.is_empty() && stdout.is_empty() {
                return format!("error: {}", stderr);
            }
            stdout
        }
        Err(e) => format!("error: {}", e),
    }
}

fn get_latest_block(rpc_url: &str, user: &str, pass: &str) -> Result<(String, u32), String> {
    let client = reqwest::blocking::Client::new();
    let height = rpc_json(&client, rpc_url, user, pass, "getblockcount", &[])?
        .as_u64().ok_or("getblockcount")? as u32;
    let hash = rpc_json(&client, rpc_url, user, pass, "getblockhash", &[serde_json::json!(height)])?
        .as_str().ok_or("getblockhash")?.to_string();
    Ok((hash, height))
}

fn list_unspent_rpc(rpc_url: &str, user: &str, pass: &str, addr: &str)
    -> Result<Vec<(String, u32, u64)>, String>
{
    let client = reqwest::blocking::Client::new();
    let result = rpc_json(&client, rpc_url, user, pass, "listunspent",
        &[serde_json::json!(1), serde_json::json!(9999999), serde_json::json!([addr])])?;
    Ok(result.as_array().unwrap_or(&vec![]).iter().map(|u| (
        u["txid"].as_str().unwrap_or("").to_string(),
        u["vout"].as_u64().unwrap_or(0) as u32,
        (u["amount"].as_f64().unwrap_or(0.0) * 1e8) as u64,
    )).collect())
}

fn broadcast(rpc_url: &str, user: &str, pass: &str, tx: &Transaction) -> Result<(), String> {
    let client = reqwest::blocking::Client::new();
    let hex = bitcoin::consensus::encode::serialize_hex(tx);
    rpc_json(&client, rpc_url, user, pass, "sendrawtransaction", &[serde_json::json!(hex)])?;
    Ok(())
}

fn query_indexer_seq() -> u32 {
    let client = reqwest::blocking::Client::new();
    client.get("http://127.0.0.1:3000/status").send().ok()
        .and_then(|r| r.json::<serde_json::Value>().ok())
        .and_then(|v| v["next_seq"].as_u64())
        .unwrap_or(1) as u32
}

fn rpc_json(
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
    if let Some(e) = resp.get("error") {
        if !e.is_null() { return Err(format!("RPC {}: {}", method, e)); }
    }
    resp.get("result").cloned().ok_or(format!("RPC {}: no result", method))
}
