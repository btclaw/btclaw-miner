mod ui;
/// NEXUS Reactor — 交互式CLI菜单
///
/// [1] 安装/同步 BTC全节点
/// [2] 查看同步进度
/// [3] 测试网铸造 (regtest)
/// [4] 主网铸造
/// [5] 钱包信息
/// [6] 创建钱包
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
use nexus_reactor::node_detect;

// ═══════════════════════════════════════════
//  主菜单
// ═══════════════════════════════════════════

fn main() {
    print_banner();

    // 自动检测全节点
    let mut config = node_detect::NexusConfig::load();
    println!("  Scanning for Bitcoin node... / 正在检测全节点...");
    println!();
    let detection = node_detect::detect_node(&config);
    node_detect::print_detection(&detection);
    if detection.found && config.bitcoin_datadir.is_none() {
        config.bitcoin_datadir = Some(detection.datadir.clone());
        config.save();
        println!();
        println!("  \x1b[90mPath saved to nexus_config.json\x1b[0m");
    }
    println!();

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
            "6" => menu_create_wallet(),
            "0" | "q" | "exit" => {
                println!("  👋 再见!");
                break;
            }
            _ => println!("  ⚠️  无效选择，请输入 0-6"),
        }
    }
}

fn print_banner() {
    ui::banner();
}

// ═══════════════════════════════════════════
//  RPC辅助 — 从config读取密码
// ═══════════════════════════════════════════

fn get_rpc_config() -> (String, String) {
    let config = node_detect::NexusConfig::load();
    (config.rpc_user, config.rpc_pass)
}

fn btc_cli_output(args: &[&str], regtest: bool) -> String {
    let (rpc_user, rpc_pass) = get_rpc_config();
    let mut cmd = Command::new("bitcoin-cli");
    if regtest {
        cmd.arg("-regtest");
    }
    cmd.arg(format!("-rpcuser={}", rpc_user));
    cmd.arg(format!("-rpcpassword={}", rpc_pass));
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

fn btc_cli(args: &[&str], regtest: bool) -> bool {
    let output = btc_cli_output(args, regtest);
    !output.contains("error")
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
    println!("  ⚠️  Mainnet sync requirements / 主网同步需要:");
    println!("    - Disk / 磁盘: ~850GB (SSD recommended)");
    println!("    - Time / 时间: 8-72h (depends on hardware)");
    println!("    - RAM / 内存: 4GB+ (more = faster)");
    println!("");

    let config = node_detect::NexusConfig::load();
    let det = node_detect::detect_node(&config);
    if det.found && det.running {
        println!("  ⚠️  Node already running / 节点已在运行!");
        node_detect::print_detection(&det);
        println!();
        print!("  Continue anyway? / 仍然继续? (y/n) > ");
        io::stdout().flush().unwrap();
        if read_line().trim().to_lowercase() != "y" {
            println!("  Cancelled / 已取消");
            return;
        }
    }

    let datadir = node_detect::choose_datadir();
    println!("");
    println!("  Data directory / 数据目录: {}", datadir);
    println!("");
    print!("  Start sync? / 确定开始? (y/n) > ");
    io::stdout().flush().unwrap();

    if read_line().trim().to_lowercase() != "y" {
        println!("  Cancelled / 已取消");
        return;
    }

    std::fs::create_dir_all(&datadir).ok();

    let mem_gb = get_system_memory_gb();
    let dbcache = ((mem_gb as u64).saturating_sub(4) * 1024).max(2048).min(32000);
    let (rpc_user, rpc_pass) = get_rpc_config();

    let conf = format!(
"server=1
txindex=1
rpcuser={}
rpcpassword={}
fallbackfee=0.00001

# Auto-configured: {}GB RAM detected
dbcache={}
par=0
maxconnections=80
blocksonly=1
assumevalid=0000000000000000000220e01aac81f0a001c38c8a51e54688a9ded7b1db93ed

[main]
rpcport=8332
rpcallowip=127.0.0.1
rpcbind=127.0.0.1
", rpc_user, rpc_pass, mem_gb, dbcache);

    std::fs::write(format!("{}/bitcoin.conf", datadir), &conf).ok();

    let _ = Command::new("bitcoind")
        .arg(format!("-datadir={}", datadir))
        .arg("-daemon")
        .spawn();

    let mut config = node_detect::NexusConfig::load();
    config.bitcoin_datadir = Some(datadir.clone());
    config.save();

    println!("");
    println!("  ✅ Node started! / 节点已启动!");
    println!("     Datadir: {}", datadir);
    println!("     dbcache: {} MB ({}GB RAM detected)", dbcache, mem_gb);
    println!("     Use [2] to check progress / 用[2]查看同步进度");
}

fn get_system_memory_gb() -> u32 {
    Command::new("bash").arg("-c")
        .arg("grep MemTotal /proc/meminfo | awk '{print int($2/1024/1024)}'")
        .output().ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().parse().unwrap_or(8))
        .unwrap_or(8)
}

fn start_regtest_node() {
    println!("");
    println!("  🧪 启动regtest测试节点...");

    let _ = Command::new("bitcoin-cli")
        .args(["-regtest", "-rpcuser=nexus", "-rpcpassword=nexustest123", "stop"])
        .output();
    std::thread::sleep(std::time::Duration::from_secs(2));

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

    let _ = Command::new("bitcoind").arg("-regtest").arg("-daemon").spawn();
    std::thread::sleep(std::time::Duration::from_secs(3));

    btc_cli(&["createwallet", "nexus_test"], true);

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
    println!("  ══ Sync Progress / 同步进度 ══");
    println!("");

    let config = node_detect::NexusConfig::load();
    let det = node_detect::detect_node(&config);

    if det.found {
        node_detect::print_detection(&det);
        if det.running && det.headers > 0 {
            let remaining = det.headers.saturating_sub(det.blocks);
            println!("");
            println!("    Remaining / 剩余: {} blocks", remaining);
            println!("    Disk / 磁盘:      {:.1} GB", det.size_gb);
        }
        return;
    }

    let mainnet = btc_cli_output(&["getblockchaininfo"], false);
    if !mainnet.is_empty() && !mainnet.contains("error") {
        if let Ok(info) = serde_json::from_str::<serde_json::Value>(&mainnet) {
            let blocks = info["blocks"].as_u64().unwrap_or(0);
            let headers = info["headers"].as_u64().unwrap_or(0);
            let progress = info["verificationprogress"].as_f64().unwrap_or(0.0);
            let ibd = info["initialblockdownload"].as_bool().unwrap_or(true);
            let size_on_disk = info["size_on_disk"].as_u64().unwrap_or(0);

            println!("  Blocks:     {} / {}", blocks, headers);
            let pct = progress * 100.0;
            let bar_w = 30;
            let filled = (pct / 100.0 * bar_w as f64) as usize;
            let empty = bar_w - filled;
            println!("  Progress:   [{}{}] {:.2}%", "█".repeat(filled), "░".repeat(empty), pct);
            println!("  Disk:       {:.2} GB", size_on_disk as f64 / 1e9);
            if ibd { println!("  Status:     Syncing..."); } else { println!("  Status:     ✅ Synced!"); }
            return;
        }
    }

    println!("  ❌ No Bitcoin node found / 未检测到节点");
    println!("     Use [1] to install and sync / 用[1]安装同步");
}

// ═══════════════════════════════════════════
//  [3] 测试网铸造 (regtest)
// ═══════════════════════════════════════════

fn menu_testnet_mint() {
    println!("  ══ 测试网铸造 (regtest) ══");
    println!("");

    let info = btc_cli_output(&["getblockchaininfo"], true);
    if info.is_empty() || info.contains("error") {
        println!("  ❌ regtest节点未运行。请先用选项 [1] → [3] 启动");
        return;
    }

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

    println!("  [2/4] 准备钱包...");
    let addr = btc_cli_output(&["getnewaddress", "nexus_minter", "bech32m"], true);
    let addr = addr.trim();

    let _ = btc_cli_output(&["sendtoaddress", addr, "1.0"], true);
    let _ = btc_cli_output(&["generatetoaddress", "1", addr], true);

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

    let derive_desc = format!("tr({})#{}", privkey_wif, checksum);
    let addr_json = btc_cli_output(&["deriveaddresses", &derive_desc], true);
    let minter_addr = serde_json::from_str::<serde_json::Value>(&addr_json)
        .ok()
        .and_then(|v| v.as_array()?.first()?.as_str().map(|s| s.to_string()))
        .unwrap_or_default();

    let _ = btc_cli_output(&["sendtoaddress", &minter_addr, "0.5"], true);
    let _ = btc_cli_output(&["generatetoaddress", "1", &minter_addr], true);

    println!("    铸造地址: {}", minter_addr);
    println!("    私钥: {}", privkey_wif);

    println!("  [3/4] 执行铸造...");
    println!("");

    let result = execute_mint(
        &expand_home("~/.bitcoin/regtest"),
        "http://127.0.0.1:18443",
        "nexus",
        "nexustest123",
        &privkey_wif,
        1.0,
        Network::Regtest,
    );

    match result {
        Ok((commit_txid, reveal_txid)) => {
            println!("  [4/4] 挖块确认...");
            let _ = btc_cli_output(&["generatetoaddress", "1", &minter_addr], true);

            println!("");
            println!("  ✅ 测试网铸造成功!");
            println!("  Commit: {}", commit_txid);
            println!("  Reveal: {}", reveal_txid);
            println!("");

            println!("  ── 链上验证 On-Chain Verification ──");
            println!("");
            let tx_raw = btc_cli_output(&["getrawtransaction", &reveal_txid, "1"], true);
            if let Ok(tx) = serde_json::from_str::<serde_json::Value>(&tx_raw) {
                let confirms = tx["confirmations"].as_u64().unwrap_or(0);
                println!("  Confirmations: {}", confirms);
                println!("");

                if let Some(vouts) = tx["vout"].as_array() {
                    for (i, out) in vouts.iter().enumerate() {
                        let val = out["value"].as_f64().unwrap_or(0.0);
                        let typ = out["scriptPubKey"]["type"].as_str().unwrap_or("?");
                        println!("  Output[{}]: {} BTC ({})", i, val, typ);
                    }
                }
                println!("");

                if let Some(wit) = tx["vin"][0]["txinwitness"].as_array() {
                    if wit.len() >= 2 {
                        let script_hex = wit[1].as_str().unwrap_or("");
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

                if let Some(vouts) = tx["vout"].as_array() {
                    for out in vouts {
                        if out["scriptPubKey"]["type"].as_str() == Some("nulldata") {
                            let hex_data = out["scriptPubKey"]["hex"].as_str().unwrap_or("");
                            if hex_data.len() > 6 {
                                let data_start = if &hex_data[2..4] == "4c" { 6 } else { 4 };
                                if let Ok(data) = hex::decode(&hex_data[data_start..]) {
                                    if let Ok(text) = String::from_utf8(data.clone()) {
                                        if text.starts_with("NXS:") {
                                            let parts: Vec<&str> = text.split(':').collect();
                                            println!("  ┌── OP_RETURN Layer / 协议层 ──");
                                            println!("  │ Raw:         {}", text);
                                            if parts.len() >= 4 {
                                                println!("  │ Magic:       NXS");
                                                println!("  │ Version:     {}", parts[1]);
                                                println!("  │ Wit Hash:    {}", parts[2].strip_prefix("w=").unwrap_or(parts[2]));
                                                println!("  │ Proof Hash:  {}", parts[3].strip_prefix("p=").unwrap_or(parts[3]));
                                            }
                                            println!("  └────────────────────────────────");
                                        }
                                    } else if data.len() >= 68 {
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
    println!("  ══ Mainnet Mint / 主网铸造 ══");
    println!("");

    let config = node_detect::NexusConfig::load();
    let det = node_detect::detect_node(&config);

    if !det.found || !det.running {
        println!("  ❌ No running mainnet node found / 未检测到主网节点");
        println!("     Use [1] to install and sync / 用[1]安装同步");
        return;
    }

    if det.ibd {
        println!("  ⚠️  Node still syncing (IBD) / 节点还在同步中");
        node_detect::print_detection(&det);
        println!("     Wait for sync to complete / 请等待同步完成");
        return;
    }

    node_detect::print_detection(&det);
    println!("");

    let datadir = det.datadir.clone();
    print!("  Verifying full node / 验证全节点... ");
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

    // 选择钱包
    let wallets_json = btc_cli_output(&["listwallets"], false);
    let wallets: Vec<String> = serde_json::from_str::<serde_json::Value>(&wallets_json)
        .ok()
        .and_then(|v| v.as_array().map(|a| a.iter().filter_map(|w| w.as_str().map(|s| s.to_string())).collect()))
        .unwrap_or_default();

    let wallet_name = if wallets.len() == 1 {
        println!("  Using wallet: {}", wallets[0]);
        wallets[0].clone()
    } else if wallets.len() > 1 {
        println!("  Wallets:");
        for (i, w) in wallets.iter().enumerate() {
            println!("    [{}] {}", i + 1, w);
        }
        print!("  Select wallet / 选择钱包: ");
        io::stdout().flush().unwrap();
        let sel: usize = read_line().trim().parse().unwrap_or(1);
        wallets.get(sel.saturating_sub(1)).cloned().unwrap_or_default()
    } else {
        println!("  ⚠ No wallet found. Use [6] to create one.");
        return;
    };

    // 自动从钱包文件读取WIF
    let wallet_file = format!("{}_wallet.json", wallet_name);
    let privkey_wif = if let Ok(json_str) = std::fs::read_to_string(&wallet_file) {
        if let Ok(data) = serde_json::from_str::<serde_json::Value>(&json_str) {
            // 优先taproot，其次native_segwit
            let wif = data["addresses"]["taproot"]["wif"].as_str()
                .or_else(|| data["addresses"]["native_segwit"]["wif"].as_str())
                .or_else(|| data["addresses"]["nested_segwit"]["wif"].as_str());
            if let Some(w) = wif {
                let addr = data["addresses"]["taproot"]["address"].as_str().unwrap_or("?");
                println!("  Wallet file: {}", wallet_file);
                println!("  Address: {}", addr);
                println!("  WIF: {}...{}", &w[..8], &w[w.len()-4..]);
                println!("");
                w.to_string()
            } else {
                println!("  ⚠ No WIF found in {}. Enter manually:", wallet_file);
                print!("  WIF private key: ");
                io::stdout().flush().unwrap();
                let w = read_line().trim().to_string();
                if w.is_empty() { println!("  Cancelled"); return; }
                w
            }
        } else {
            println!("  ⚠ Cannot parse {}. Enter manually:", wallet_file);
            print!("  WIF private key: ");
            io::stdout().flush().unwrap();
            let w = read_line().trim().to_string();
            if w.is_empty() { println!("  Cancelled"); return; }
            w
        }
    } else {
        println!("  ⚠ No wallet file found ({}). Enter WIF manually:", wallet_file);
        print!("  WIF private key / 输入WIF私钥: ");
        io::stdout().flush().unwrap();
        let w = read_line().trim().to_string();
        if w.is_empty() { println!("  Cancelled"); return; }
        w
    };

    println!("");
    print!("  Fee rate (sat/vB, min 0.1) / 矿工费率: ");
    io::stdout().flush().unwrap();
    let fee_rate_f: f64 = read_line().trim().parse().unwrap_or(1.0);
    let fee_rate_f = if fee_rate_f < 0.1 { 0.1 } else { fee_rate_f };
    println!("  Fee rate: {} sat/vB", fee_rate_f);

    println!("");
    println!("  ⚠️  About to mint on BTC MAINNET! / 即将在主网铸造!");
    println!("     Fee: {} sats mint fee + miner fee", MINT_FEE_SATS);
    print!("  Confirm? / 确认? (yes/no) > ");
    io::stdout().flush().unwrap();

    if read_line().trim() != "yes" {
        println!("  Cancelled / 已取消");
        return;
    }

    let rpc_url = format!("http://127.0.0.1:8332/wallet/{}", wallet_name);
    let result = execute_mint(
        &datadir,
        &rpc_url,
        &config.rpc_user,
        &config.rpc_pass,
        &privkey_wif,
        fee_rate_f,
        Network::Bitcoin,
    );

    match result {
        Ok((commit, reveal)) => {
            println!("");
            println!("  ✅ Mainnet mint broadcast! / 主网铸造已广播!");
            println!("  Commit: {}", commit);
            println!("  Reveal: {}", reveal);
            println!("  Waiting for confirmation... / 等待确认...");
        }
        Err(e) => println!("  ❌ Mint failed / 铸造失败: {}", e),
    }
}

// ═══════════════════════════════════════════
//  [6] 创建钱包
// ═══════════════════════════════════════════

fn menu_create_wallet() {
    let c = "\x1b[36m"; let y = "\x1b[33m"; let g = "\x1b[32m";
    let w = "\x1b[97m"; let d = "\x1b[90m"; let b = "\x1b[1m";
    let r = "\x1b[0m"; let red = "\x1b[31m";

    println!("  {c}{b}── Create Wallet / 创建钱包 ──{r}");
    println!();
    println!("    {y}{b}[1]{r}  {w}Taproot (bc1p...){r}            {d}P2TR - BIP86 推荐{r}");
    println!("    {y}{b}[2]{r}  {w}Native SegWit (bc1q...){r}      {d}P2WPKH - BIP84{r}");
    println!("    {y}{b}[3]{r}  {w}Nested SegWit (3...){r}         {d}P2SH-P2WPKH - BIP49{r}");
    println!("    {y}{b}[4]{r}  {w}All types / 全部生成{r}");
    println!("    {d}[0]  Back 返回{r}");
    println!();
    print!("  {y}{b}> {r}");
    io::stdout().flush().unwrap();

    let choice = read_line().trim().to_string();
    let addr_type = match choice.as_str() {
        "1" => "taproot",
        "2" => "native_segwit",
        "3" => "nested_segwit",
        "4" => "all",
        _ => return,
    };

    println!();
    print!("  Wallet name / 钱包名称: ");
    io::stdout().flush().unwrap();
    let wallet_name = read_line().trim().to_string();
    let wallet_name = if wallet_name.is_empty() { "nexus_wallet".to_string() } else { wallet_name };

    println!();
    println!("  Generating wallet... / 正在生成钱包...");
    println!();

    // 调用Python生成钱包
    let output = Command::new("python3")
        .arg("scripts/wallet_gen.py")
        .arg(addr_type)
        .output();

    let output = match output {
        Ok(o) => o,
        Err(e) => {
            println!("  {red}❌ Failed: {}{r}", e);
            println!("  Install: pip install bip_utils --break-system-packages -i https://pypi.org/simple/");
            return;
        }
    };

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        println!("  {red}❌ Generation failed: {}{r}", err);
        println!("  Install: pip install bip_utils --break-system-packages -i https://pypi.org/simple/");
        return;
    }

    let json_str = String::from_utf8_lossy(&output.stdout);
    let data: serde_json::Value = match serde_json::from_str(&json_str) {
        Ok(v) => v,
        Err(e) => {
            println!("  {red}❌ Parse error: {}{r}", e);
            println!("  Raw: {}", json_str);
            return;
        }
    };

    // 检查是否有error
    if let Some(err) = data["error"].as_str() {
        println!("  {red}❌ {}{r}", err);
        return;
    }

    let mnemonic = data["mnemonic"].as_str().unwrap_or("?");
    let addresses = &data["addresses"];

    // 显示结果
    println!("  {g}{b}╔════════════════════════════════════════════════════════════╗{r}");
    println!("  {g}{b}║  Wallet Created / 钱包已创建                              ║{r}");
    println!("  {g}{b}╚════════════════════════════════════════════════════════════╝{r}");
    println!();
    println!("  {w}{b}Name / 名称:{r}  {c}{}{r}", wallet_name);
    println!();

    println!("  {red}{b}╔════════════════════════════════════════════════════════════╗{r}");
    println!("  {red}{b}║  ⚠ SAVE THIS! NEVER SHARE! / 请保存! 不要分享!            ║{r}");
    println!("  {red}{b}╚════════════════════════════════════════════════════════════╝{r}");
    println!();
    println!("  {w}{b}Mnemonic / 助记词 (12 words):{r}");
    println!("  {y}{b}{}{r}", mnemonic);
    println!();

    // 收集要导入Bitcoin Core的WIF
    let mut import_wifs: Vec<(String, String)> = Vec::new(); // (wif, addr_type_name)

    if let Some(taproot) = addresses.get("taproot") {
        let addr = taproot["address"].as_str().unwrap_or("?");
        let wif = taproot["wif"].as_str().unwrap_or("?");
        println!("  {c}{b}── Taproot (P2TR) ──{r}  {d}{}{r}", taproot["path"].as_str().unwrap_or(""));
        println!("  {w}Address:{r} {g}{b}{}{r}", addr);
        println!("  {w}WIF Key:{r} {y}{}{r}", wif);
        println!();
        import_wifs.push((wif.to_string(), "tr".to_string()));
    }

    if let Some(native) = addresses.get("native_segwit") {
        let addr = native["address"].as_str().unwrap_or("?");
        let wif = native["wif"].as_str().unwrap_or("?");
        println!("  {c}{b}── Native SegWit (P2WPKH) ──{r}  {d}{}{r}", native["path"].as_str().unwrap_or(""));
        println!("  {w}Address:{r} {g}{b}{}{r}", addr);
        println!("  {w}WIF Key:{r} {y}{}{r}", wif);
        println!();
        import_wifs.push((wif.to_string(), "wpkh".to_string()));
    }

    if let Some(nested) = addresses.get("nested_segwit") {
        let addr = nested["address"].as_str().unwrap_or("?");
        let wif = nested["wif"].as_str().unwrap_or("?");
        println!("  {c}{b}── Nested SegWit (P2SH-P2WPKH) ──{r}  {d}{}{r}", nested["path"].as_str().unwrap_or(""));
        println!("  {w}Address:{r} {g}{b}{}{r}", addr);
        println!("  {w}WIF Key:{r} {y}{}{r}", wif);
        println!();
        import_wifs.push((wif.to_string(), "sh(wpkh".to_string()));
    }

    // 保存到文件
    let wallet_file = format!("{}_wallet.json", wallet_name);
    let save_data = serde_json::json!({
        "name": wallet_name,
        "mnemonic": mnemonic,
        "addresses": addresses,
    });
    if let Ok(j) = serde_json::to_string_pretty(&save_data) {
        std::fs::write(&wallet_file, &j).ok();
        println!("  {d}Saved to / 已保存到: {wallet_file}{r}");
        println!("  {red}{b}Delete this file after backing up! / 备份后请删除此文件!{r}");
    }
    println!();

    // 导入Bitcoin Core
    println!("  {c}{b}── Importing to Bitcoin Core / 导入全节点 ──{r}");

    // 先创建钱包（如果不存在）
    let create_result = btc_cli_output(
        &["createwallet", &wallet_name, "false", "false", "", "false", "true", "true"],
        false,
    );
    if create_result.contains("error") && !create_result.contains("already exists") {
        // 钱包可能已存在，尝试加载
        let _ = btc_cli_output(&["loadwallet", &wallet_name], false);
    }

    // 导入每个私钥
    let mut imported = 0;
    for (wif, desc_type) in &import_wifs {
        let desc = if desc_type == "tr" {
            format!("tr({})", wif)
        } else if desc_type.starts_with("sh(wpkh") {
            format!("sh(wpkh({}))", wif)
        } else {
            format!("wpkh({})", wif)
        };

        // 获取checksum
        let desc_info = btc_cli_output(
            &[&format!("-rpcwallet={}", wallet_name), "getdescriptorinfo", &desc],
            false,
        );
        let checksum = serde_json::from_str::<serde_json::Value>(&desc_info)
            .ok()
            .and_then(|v| v["checksum"].as_str().map(|s| s.to_string()))
            .unwrap_or_default();

        if checksum.is_empty() {
            println!("  {y}⚠ Could not get checksum for {}{r}", desc_type);
            continue;
        }

        let import_json = format!(
            "[{{\"desc\": \"{}#{}\", \"timestamp\": \"now\", \"active\": false}}]",
            desc, checksum
        );
        let result = btc_cli_output(
            &[&format!("-rpcwallet={}", wallet_name), "importdescriptors", &import_json],
            false,
        );

        if !result.contains("error") || result.contains("\"success\": true") {
            imported += 1;
            println!("  {g}✓ Imported {} key{r}", desc_type);
        } else {
            println!("  {y}⚠ Failed to import {}: {}{r}", desc_type, result.trim());
        }
    }

    if imported > 0 {
        println!();
        println!("  {g}{b}✓ Wallet imported to Bitcoin Core!{r}");
        println!("  {d}Wallet name: {}{r}", wallet_name);
        println!("  {d}Use [5] Wallet Info to check balance{r}");
    } else {
        println!();
        println!("  {y}⚠ Could not import to Bitcoin Core (node may not be running){r}");
        println!("  {d}You can still use [4] Mainnet Mint with the WIF key directly{r}");
    }

    println!();

    // 铸造提示
    if addresses.get("taproot").is_some() {
        let addr = addresses["taproot"]["address"].as_str().unwrap_or("");
        println!("  {c}{b}── Ready to Mint / 准备铸造 ──{r}");
        println!("  {w}1. Send BTC to your Taproot address / 向Taproot地址充值:{r}");
        println!("     {g}{b}{}{r}", addr);
        println!("  {w}2. Need at least ~10,000 sats (5,000 fee + miner gas){r}");
        println!("  {w}3. Use [4] Mainnet Mint with your WIF key / 用[4]铸造{r}");
    }
}

// ═══════════════════════════════════════════
//  [5] 钱包信息
// ═══════════════════════════════════════════

fn menu_wallet_info() {
    ui::sub_menu_wallet();

    let (is_regtest, _rpc_port) = match read_line().trim() {
        "1" => (true, "18443"),
        "2" => (false, "8332"),
        _ => return,
    };

    println!("");

    // 列出所有钱包
    let wallets_json = btc_cli_output(&["listwallets"], is_regtest);
    if wallets_json.contains("error") {
        println!("  ❌ Cannot connect to node / 无法连接节点");
        return;
    }

    let wallets: Vec<String> = serde_json::from_str::<serde_json::Value>(&wallets_json)
        .ok()
        .and_then(|v| v.as_array().map(|a| {
            a.iter().filter_map(|w| w.as_str().map(|s| s.to_string())).collect()
        }))
        .unwrap_or_default();

    if wallets.is_empty() {
        println!("  No wallets found / 未找到钱包");
        println!("  Use [6] to create a wallet / 用[6]创建钱包");
        return;
    }

    println!("  Wallets / 钱包列表:");
    for (i, w) in wallets.iter().enumerate() {
        println!("    [{}] {}", i + 1, w);
    }
    println!();
    print!("  Select wallet / 选择钱包 (number): ");
    io::stdout().flush().unwrap();
    let sel: usize = read_line().trim().parse().unwrap_or(1);
    let wallet_name = match wallets.get(sel.saturating_sub(1)) {
        Some(w) => w.clone(),
        None => { println!("  Invalid selection"); return; }
    };

    println!("");

    // 余额
    let balance = btc_cli_output(
        &[&format!("-rpcwallet={}", wallet_name), "getbalance"],
        is_regtest,
    );

    if balance.contains("error") || balance.is_empty() {
        println!("  ❌ Cannot get balance for wallet: {}", wallet_name);
        return;
    }

    println!("  💰 Wallet: {}", wallet_name);
    println!("  💰 BTC Balance: {} BTC", balance.trim());

    // UTXO列表
    let utxo_json = btc_cli_output(
        &[&format!("-rpcwallet={}", wallet_name), "listunspent"],
        is_regtest,
    );

    if let Ok(utxos) = serde_json::from_str::<serde_json::Value>(&utxo_json) {
        if let Some(arr) = utxos.as_array() {
            println!("  📦 UTXOs: {}", arr.len());
            println!("");

            let total_sats: f64 = arr.iter()
                .map(|u| u["amount"].as_f64().unwrap_or(0.0))
                .sum();
            println!("  Total: {:.8} BTC ({} sats)", total_sats, (total_sats * 1e8) as u64);
            println!("");

            let show = arr.len().min(10);
            for (i, utxo) in arr.iter().take(show).enumerate() {
                let txid = utxo["txid"].as_str().unwrap_or("?");
                let vout = utxo["vout"].as_u64().unwrap_or(0);
                let amount = utxo["amount"].as_f64().unwrap_or(0.0);
                let confirms = utxo["confirmations"].as_u64().unwrap_or(0);
                let addr = utxo["address"].as_str().unwrap_or("?");
                println!("  [{}] {}:{} | {:.8} BTC | {} confirms",
                    i + 1, txid, vout, amount, confirms);
                println!("       {}", addr);
            }
            if arr.len() > 10 {
                println!("  ... {} more UTXOs", arr.len() - 10);
            }
        }
    }

    // NEXUS代币余额
    println!("");
    println!("  ── NEXUS Token ──");
    let client = reqwest::blocking::Client::new();
    match client.get("http://127.0.0.1:3000/status").send() {
        Ok(resp) => {
            if let Ok(status) = resp.json::<serde_json::Value>() {
                let minted = status["minted"].as_u64().unwrap_or(0);
                let next_seq = status["next_seq"].as_u64().unwrap_or(1);
                let remaining = TOTAL_MINTS as u64 - (next_seq - 1);
                println!("  Minted: {} / {} NXS", minted, MAX_SUPPLY);
                println!("  Mints done: {} / {}", next_seq - 1, TOTAL_MINTS);
                println!("  Remaining: {}", remaining);
            }
        }
        Err(_) => {
            println!("  ⚠️  Indexer not running / Indexer未运行");
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
    fee_rate: f64,
    network: Network,
) -> Result<(String, String), String> {
    let secp = Secp256k1::new();

    let privkey = PrivateKey::from_wif(privkey_wif)
        .map_err(|e| format!("Invalid WIF: {}", e))?;
    let secret_key = privkey.inner;
    let keypair = bitcoin::secp256k1::Keypair::from_secret_key(&secp, &secret_key);
    let (x_only_pubkey, _) = keypair.x_only_public_key();
    let internal_key = bitcoin::key::UntweakedPublicKey::from(x_only_pubkey);
    let pubkey = PublicKey::from_private_key(&secp, &privkey);
    let minter_address = Address::p2tr(&secp, internal_key, None, network);

    println!("    Address: {}", minter_address);

    let next_seq = query_indexer_seq();
    if next_seq > TOTAL_MINTS {
        return Err("Minting ended / 铸造已结束".into());
    }
    println!("    Progress: {} / {} remaining", TOTAL_MINTS - next_seq + 1, TOTAL_MINTS);

    let (block_hash_hex, block_height) = get_latest_block(rpc_url, rpc_user, rpc_pass)?;
    println!("    Block: {} ({})", block_height, &block_hash_hex[..12]);

    print!("    Generating proof... ");
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
    ).map_err(|e| format!("Proof failed: {}", e))?;
    println!("✅ ({}s)", two_round.round2_ts - two_round.round1_ts);

    let pubkey_hex = hex::encode(x_only_pubkey.serialize());
    let interlock = transaction::build_interlock(&two_round, &pubkey_hex)
        .map_err(|e| format!("Interlock failed: {}", e))?;
    println!("    Interlock: ✅");

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

    let taproot_builder = TaprootBuilder::new()
        .add_leaf(0, inscription_script.clone())
        .map_err(|e| format!("taproot leaf: {:?}", e))?;

    let spend_info = taproot_builder
        .finalize(&secp, internal_key)
        .map_err(|_| "taproot finalize failed")?;

    let commit_address = Address::p2tr_tweaked(spend_info.output_key(), network);

    let utxos = list_unspent_rpc(rpc_url, rpc_user, rpc_pass, &minter_address.to_string())?;
    if utxos.is_empty() {
        return Err(format!("No UTXOs, send BTC to {}", minter_address));
    }

    let opreturn_script = transaction::build_opreturn_script(&interlock.opreturn_bytes);

    let reveal_vsize: u64 = 300 + (interlock.witness_json.len() as u64 / 4);
    let reveal_fee = (reveal_vsize as f64 * fee_rate).ceil() as u64;
    let commit_output_value = 330 + MINT_FEE_SATS + reveal_fee;
    let commit_fee = (154.0 * fee_rate).ceil() as u64;
    let total_needed = commit_output_value + commit_fee;

    let utxo = utxos.iter().find(|u| u.2 >= total_needed)
        .ok_or(format!("UTXO insufficient, need {} sats", total_needed))?;

    println!("    UTXO: {}:{} ({} sats)", &utxo.0, utxo.1, utxo.2);

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

    let fee_addr = Address::from_str(FEE_ADDRESS)
        .map_err(|e| format!("FEE_ADDRESS invalid: {}", e))?
        .require_network(network)
        .map_err(|e| format!("FEE_ADDRESS network mismatch: {}", e))?;

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
            .ok_or("control block failed")?;
        let mut witness = Witness::new();
        witness.push(schnorr_sig.to_vec());
        witness.push(inscription_script.as_bytes());
        witness.push(control_block.serialize());
        reveal_tx.input[0].witness = witness;
    }

    let reveal_txid = reveal_tx.compute_txid();

    print!("    Broadcasting Commit... ");
    io::stdout().flush().unwrap();
    broadcast(rpc_url, rpc_user, rpc_pass, &commit_tx)?;
    println!("✅ {}", commit_txid);

    print!("    Broadcasting Reveal... ");
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
