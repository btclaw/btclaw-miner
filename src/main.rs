mod ui;
/// NEXUS Reactor — 交互式CLI菜单
///
/// [1] 安装/同步 BTC全节点
/// [2] 查看同步进度
/// [3] 测试网铸造 (regtest)
/// [4] 主网铸造 (单次 / 批量)
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
use nexus_reactor::utxo::{self, UtxoManager, UtxoRecord};

// ═══════════════════════════════════════════
//  主菜单
// ═══════════════════════════════════════════

fn main() {
    print_banner();

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
//  RPC辅助
// ═══════════════════════════════════════════

fn get_rpc_config() -> (String, String) {
    let config = node_detect::NexusConfig::load();
    (config.rpc_user, config.rpc_pass)
}

fn btc_cli_output(args: &[&str], regtest: bool) -> String {
    let (rpc_user, rpc_pass) = get_rpc_config();
    let mut cmd = Command::new("bitcoin-cli");
    if regtest { cmd.arg("-regtest"); }
    cmd.arg(format!("-rpcuser={}", rpc_user));
    cmd.arg(format!("-rpcpassword={}", rpc_pass));
    cmd.args(args);
    match cmd.output() {
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout).to_string();
            let stderr = String::from_utf8_lossy(&o.stderr).to_string();
            if !stderr.is_empty() && stdout.is_empty() { return format!("error: {}", stderr); }
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
    println!("  ⏳ 开始安装 Bitcoin Core 30.2...");
    println!("  (这需要下载约74MB, 请稍候)");
    println!("");
    let script = r#"
        cd /tmp
        ARCH=$(uname -m)
        if [ "$ARCH" = "x86_64" ]; then BA="x86_64-linux-gnu"; 
        elif [ "$ARCH" = "aarch64" ]; then BA="aarch64-linux-gnu";
        else echo "不支持的架构"; exit 1; fi
        wget -q "https://bitcoincore.org/bin/bitcoin-core-30.2/bitcoin-30.2-${BA}.tar.gz"
        tar xzf "bitcoin-30.2-${BA}.tar.gz"
        sudo install -m 0755 bitcoin-30.2/bin/* /usr/local/bin/
        rm -rf bitcoin-30.2 "bitcoin-30.2-${BA}.tar.gz"
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
        if read_line().trim().to_lowercase() != "y" { println!("  Cancelled / 已取消"); return; }
    }
    let datadir = node_detect::choose_datadir();
    println!("");
    println!("  Data directory / 数据目录: {}", datadir);
    println!("");
    print!("  Start sync? / 确定开始? (y/n) > ");
    io::stdout().flush().unwrap();
    if read_line().trim().to_lowercase() != "y" { println!("  Cancelled / 已取消"); return; }
    std::fs::create_dir_all(&datadir).ok();
    let mem_gb = get_system_memory_gb();
    let dbcache = ((mem_gb as u64).saturating_sub(4) * 1024).max(2048).min(32000);
    let (rpc_user, rpc_pass) = get_rpc_config();
    let conf = format!(
"server=1\ntxindex=1\nrpcuser={}\nrpcpassword={}\nfallbackfee=0.00001\n\n# Auto-configured: {}GB RAM detected\ndbcache={}\npar=0\nmaxconnections=80\nblocksonly=1\nassumevalid=0000000000000000000220e01aac81f0a001c38c8a51e54688a9ded7b1db93ed\n\n[main]\nrpcport=8332\nrpcallowip=127.0.0.1\nrpcbind=127.0.0.1\n", rpc_user, rpc_pass, mem_gb, dbcache);
    std::fs::write(format!("{}/bitcoin.conf", datadir), &conf).ok();
    let _ = Command::new("bitcoind").arg(format!("-datadir={}", datadir)).arg("-daemon").spawn();
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
    let _ = Command::new("bitcoin-cli").args(["-regtest", "-rpcuser=nexus", "-rpcpassword=nexustest123", "stop"]).output();
    std::thread::sleep(std::time::Duration::from_secs(2));
    std::fs::create_dir_all(expand_home("~/.bitcoin")).ok();
    let conf = "regtest=1\nserver=1\ntxindex=1\nrpcuser=nexus\nrpcpassword=nexustest123\nfallbackfee=0.00001\n\n[regtest]\nrpcport=18443\nrpcallowip=127.0.0.1\nrpcbind=127.0.0.1\nacceptnonstdtxn=1\ndatacarriersize=100000\n";
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
    { println!("⚠️  请用 --features regtest 编译测试版"); return; }

    println!("  [2/4] 准备钱包...");
    let addr = btc_cli_output(&["getnewaddress", "nexus_minter", "bech32m"], true);
    let addr = addr.trim();
    let _ = btc_cli_output(&["sendtoaddress", addr, "1.0"], true);
    let _ = btc_cli_output(&["generatetoaddress", "1", addr], true);
    let privkey_output = Command::new("python3").arg("-c").arg(r#"
import os, hashlib, base58
privkey = os.urandom(32)
payload = b'\xef' + privkey + b'\x01'
checksum = hashlib.sha256(hashlib.sha256(payload).digest()).digest()[:4]
wif = base58.b58encode(payload + checksum).decode()
print(wif)
"#).output().expect("需要python3和base58库");
    let privkey_wif = String::from_utf8_lossy(&privkey_output.stdout).trim().to_string();
    let desc_info = btc_cli_output(&["getdescriptorinfo", &format!("tr({})", privkey_wif)], true);
    let checksum = serde_json::from_str::<serde_json::Value>(&desc_info).ok()
        .and_then(|v| v["checksum"].as_str().map(|s| s.to_string())).unwrap_or_default();
    let import_json = format!("[{{\"desc\": \"tr({})#{}\", \"timestamp\": \"now\"}}]", privkey_wif, checksum);
    let _ = btc_cli_output(&["importdescriptors", &import_json], true);
    let derive_desc = format!("tr({})#{}", privkey_wif, checksum);
    let addr_json = btc_cli_output(&["deriveaddresses", &derive_desc], true);
    let minter_addr = serde_json::from_str::<serde_json::Value>(&addr_json).ok()
        .and_then(|v| v.as_array()?.first()?.as_str().map(|s| s.to_string())).unwrap_or_default();
    let _ = btc_cli_output(&["sendtoaddress", &minter_addr, "0.5"], true);
    let _ = btc_cli_output(&["generatetoaddress", "1", &minter_addr], true);
    println!("    铸造地址: {}", minter_addr);
    println!("    私钥: {}", privkey_wif);
    println!("  [3/4] 执行铸造...");
    println!("");

    let result = execute_mint(
        &expand_home("~/.bitcoin/regtest"), "http://127.0.0.1:18443",
        "nexus", "nexustest123", &privkey_wif, 1.0, Network::Regtest,
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
                            let mut depth: i32 = 0; let mut end = 0;
                            let bytes_iter: Vec<u8> = (0..json_hex.len()/2).filter_map(|i| u8::from_str_radix(&json_hex[i*2..i*2+2], 16).ok()).collect();
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
                                            if parts.len() >= 5 {
                                                println!("  │ Magic:       NXS");
                                                println!("  │ Operation:   {}", parts[1]);
                                                println!("  │ Amount:      {}", parts[2]);
                                                println!("  │ Wit Hash:    {}", parts[3].strip_prefix("w=").unwrap_or(parts[3]));
                                                println!("  │ Proof Hash:  {}", parts[4].strip_prefix("p=").unwrap_or(parts[4]));
                                            }
                                            println!("  └────────────────────────────────");
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        Err(e) => println!("  ❌ 铸造失败: {}", e),
    }
}

// ═══════════════════════════════════════════
//  [4] 主网铸造 (单次 / 批量)
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
    { println!("⚠️  当前是regtest编译版, 请用 cargo build --release 编译主网版"); return; }

    // 选择钱包
    let wallets_json = btc_cli_output(&["listwallets"], false);
    let wallets: Vec<String> = serde_json::from_str::<serde_json::Value>(&wallets_json).ok()
        .and_then(|v| v.as_array().map(|a| a.iter().filter_map(|w| w.as_str().map(|s| s.to_string())).collect()))
        .unwrap_or_default();
    let wallet_name = if wallets.len() == 1 {
        println!("  Using wallet: {}", wallets[0]);
        wallets[0].clone()
    } else if wallets.len() > 1 {
        println!("  Wallets:");
        for (i, w) in wallets.iter().enumerate() { println!("    [{}] {}", i + 1, w); }
        print!("  Select wallet / 选择钱包: ");
        io::stdout().flush().unwrap();
        let sel: usize = read_line().trim().parse().unwrap_or(1);
        wallets.get(sel.saturating_sub(1)).cloned().unwrap_or_default()
    } else {
        println!("  ⚠ No wallet found. Use [6] to create one.");
        return;
    };

    // 读取WIF
    let wallet_file = format!("{}_wallet.json", wallet_name);
    let privkey_wif = if let Ok(json_str) = std::fs::read_to_string(&wallet_file) {
        if let Ok(data) = serde_json::from_str::<serde_json::Value>(&json_str) {
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
                print!("  ⚠ No WIF found. Enter manually: ");
                io::stdout().flush().unwrap();
                let w = read_line().trim().to_string();
                if w.is_empty() { println!("  Cancelled"); return; }
                w
            }
        } else {
            print!("  ⚠ Cannot parse wallet file. Enter WIF: ");
            io::stdout().flush().unwrap();
            let w = read_line().trim().to_string();
            if w.is_empty() { println!("  Cancelled"); return; }
            w
        }
    } else {
        print!("  ⚠ No wallet file. Enter WIF / 输入WIF私钥: ");
        io::stdout().flush().unwrap();
        let w = read_line().trim().to_string();
        if w.is_empty() { println!("  Cancelled"); return; }
        w
    };

    // Fee rate
    println!("");
    print!("  Fee rate (sat/vB, min 0.1) / 矿工费率: ");
    io::stdout().flush().unwrap();
    let fee_rate_f: f64 = read_line().trim().parse().unwrap_or(1.0);
    let fee_rate_f = if fee_rate_f < 0.1 { 0.1 } else { fee_rate_f };
    println!("  Fee rate: {} sat/vB", fee_rate_f);

    // ═══ 单次 / 批量选择 ═══
    println!("");
    println!("    \x1b[33m\x1b[1m[1]\x1b[0m  Single mint / 单次铸造 (500 NXS)");
    println!("    \x1b[33m\x1b[1m[2]\x1b[0m  Batch mint / 批量铸造");
    print!("  > ");
    io::stdout().flush().unwrap();
    let mint_mode = read_line().trim().to_string();

    let rpc_url = format!("http://127.0.0.1:8332/wallet/{}", wallet_name);

    match mint_mode.as_str() {
        "1" => {
            // ═══ 单次铸造 (原逻辑) ═══
            println!("");
            println!("  ⚠️  About to mint on BTC MAINNET! / 即将在主网铸造!");
            println!("     Fee: {} sats mint fee + miner fee", MINT_FEE_SATS);
            print!("  Confirm? / 确认? (yes/no) > ");
            io::stdout().flush().unwrap();
            if read_line().trim() != "yes" { println!("  Cancelled / 已取消"); return; }

            let result = execute_mint(
                &datadir, &rpc_url, &config.rpc_user, &config.rpc_pass,
                &privkey_wif, fee_rate_f, Network::Bitcoin,
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
        "2" => {
            // ═══ 批量铸造 ═══
            println!("");
            println!("  ── Batch Mint / 批量铸造 ──");
            println!("  Scanning UTXOs / 扫描UTXO池...");

            // 解析私钥得到地址 (用于扫描UTXO)
            let secp = Secp256k1::new();
            let privkey = match PrivateKey::from_wif(&privkey_wif) {
                Ok(pk) => pk,
                Err(e) => { println!("  ❌ Invalid WIF: {}", e); return; }
            };
            let keypair = bitcoin::secp256k1::Keypair::from_secret_key(&secp, &privkey.inner);
            let (x_only_pubkey, _) = keypair.x_only_public_key();
            let internal_key = bitcoin::key::UntweakedPublicKey::from(x_only_pubkey);
            let minter_address = Address::p2tr(&secp, internal_key, None, Network::Bitcoin);

            // 加载 UTXO 管理器并扫描
            let mut utxo_mgr = UtxoManager::load();
            let raw_utxos = match list_unspent_rpc(&rpc_url, &config.rpc_user, &config.rpc_pass, &minter_address.to_string()) {
                Ok(u) => u,
                Err(e) => { println!("  ❌ Failed to scan UTXOs: {}", e); return; }
            };
            if raw_utxos.is_empty() {
                println!("  ❌ No UTXOs. Send BTC to {}", minter_address);
                return;
            }
            let utxo_records: Vec<UtxoRecord> = raw_utxos.iter().map(|u| UtxoRecord {
                txid: u.0.clone(), vout: u.1, amount: u.2,
                confirmations: 1, address: minter_address.to_string(),
            }).collect();
            let live_keys: std::collections::HashSet<String> = utxo_records.iter().map(|r| r.key()).collect();
            utxo_mgr.cleanup_spent_changes(&live_keys);

            // 预计算单次铸造成本
            let estimated_witness_len: u64 = 850;
            let reveal_vsize: u64 = 300 + (estimated_witness_len / 4);
            let reveal_fee = (reveal_vsize as f64 * fee_rate_f).ceil() as u64;
            let commit_output_value = TOKEN_OUTPUT_SATS + MINT_FEE_SATS + reveal_fee;
            let commit_fee_single = ((COMMIT_VSIZE_BASE + COMMIT_VSIZE_PER_INPUT + COMMIT_VSIZE_PER_OUTPUT) as f64 * fee_rate_f).ceil() as u64;
            let cost_per_mint = commit_output_value + commit_fee_single;

            // 计算可用余额和可铸数量
            let pre_check = utxo_mgr.pre_check(&utxo_records, cost_per_mint);
            pre_check.print();

            if !pre_check.sufficient {
                println!("  ❌ 余额不足, 需充值至少 {} sats 到 {}", pre_check.deficit, minter_address);
                return;
            }

            let max_mintable = (pre_check.available_sats / cost_per_mint) as u32;
            let max_mintable = max_mintable.min(TOTAL_MINTS - query_indexer_seq() + 1); // 不超过剩余铸造数

            println!("  ── Batch Calculation / 批量计算 ──");
            println!("    Single mint cost:   {} sats (commit {} + fee {})", cost_per_mint, commit_output_value, commit_fee_single);
            println!("    Available balance:   {} sats", pre_check.available_sats);
            println!("    Max mintable:        {} mints ({} NXS)", max_mintable, max_mintable as u64 * MINT_AMOUNT);
            println!("");

            if max_mintable == 0 {
                println!("  ❌ 余额不足以铸造任何一笔");
                return;
            }

            print!("  How many to mint? / 铸造几张? (1-{}) > ", max_mintable);
            io::stdout().flush().unwrap();
            let batch_count: u32 = read_line().trim().parse().unwrap_or(0);
            if batch_count == 0 || batch_count > max_mintable {
                println!("  ❌ Invalid count / 无效数量");
                return;
            }

            let total_cost = cost_per_mint * batch_count as u64;
            let total_nxs = batch_count as u64 * MINT_AMOUNT;
            println!("");
            println!("  ⚠️  About to batch mint on BTC MAINNET! / 即将在主网批量铸造!");
            println!("     Mints: {} x 500 NXS = {} NXS", batch_count, total_nxs);
            println!("     Est. cost: ~{} sats ({} per mint)", total_cost, cost_per_mint);
            println!("     Fee: {} sats mint fee + miner fee per tx", MINT_FEE_SATS);
            print!("  Confirm? / 确认? (yes/no) > ");
            io::stdout().flush().unwrap();
            if read_line().trim() != "yes" { println!("  Cancelled / 已取消"); return; }

            let results = execute_batch_mint(
                &datadir, &rpc_url, &config.rpc_user, &config.rpc_pass,
                &privkey_wif, fee_rate_f, Network::Bitcoin, batch_count,
            );
            match results {
                Ok(mints) => {
                    println!("");
                    println!("  \x1b[32m\x1b[1m══ Batch Complete! / 批量铸造完成! ══\x1b[0m");
                    println!("    Minted: {} NXS ({} x 500)", mints.len() as u64 * MINT_AMOUNT, mints.len());
                    println!("");
                    for (i, (c, r)) in mints.iter().enumerate() {
                        println!("    #{} Commit: {}  Reveal: {}", i + 1, &c[..16], &r[..16]);
                    }
                    println!("");
                    println!("  Waiting for confirmation... / 等待确认...");
                }
                Err(e) => println!("  ❌ Batch mint failed / 批量铸造失败: {}", e),
            }
        }
        _ => {
            println!("  Cancelled / 已取消");
        }
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
    let addr_type = match choice.as_str() { "1" => "taproot", "2" => "native_segwit", "3" => "nested_segwit", "4" => "all", _ => return };
    println!();
    print!("  Wallet name / 钱包名称: ");
    io::stdout().flush().unwrap();
    let wallet_name = read_line().trim().to_string();
    let wallet_name = if wallet_name.is_empty() { "nexus_wallet".to_string() } else { wallet_name };
    println!();
    println!("  Generating wallet... / 正在生成钱包...");
    println!();
    let output = Command::new("python3").arg("scripts/wallet_gen.py").arg(addr_type).output();
    let output = match output { Ok(o) => o, Err(e) => { println!("  {red}❌ Failed: {}{r}", e); return; } };
    if !output.status.success() { let err = String::from_utf8_lossy(&output.stderr); println!("  {red}❌ Generation failed: {}{r}", err); return; }
    let json_str = String::from_utf8_lossy(&output.stdout);
    let data: serde_json::Value = match serde_json::from_str(&json_str) { Ok(v) => v, Err(e) => { println!("  {red}❌ Parse error: {}{r}", e); return; } };
    if let Some(err) = data["error"].as_str() { println!("  {red}❌ {}{r}", err); return; }
    let mnemonic = data["mnemonic"].as_str().unwrap_or("?");
    let addresses = &data["addresses"];
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
    let mut import_wifs: Vec<(String, String)> = Vec::new();
    if let Some(taproot) = addresses.get("taproot") {
        println!("  {c}{b}── Taproot (P2TR) ──{r}  {d}{}{r}", taproot["path"].as_str().unwrap_or(""));
        println!("  {w}Address:{r} {g}{b}{}{r}", taproot["address"].as_str().unwrap_or("?"));
        println!("  {w}WIF Key:{r} {y}{}{r}", taproot["wif"].as_str().unwrap_or("?"));
        println!();
        import_wifs.push((taproot["wif"].as_str().unwrap_or("").to_string(), "tr".to_string()));
    }
    if let Some(native) = addresses.get("native_segwit") {
        println!("  {c}{b}── Native SegWit (P2WPKH) ──{r}  {d}{}{r}", native["path"].as_str().unwrap_or(""));
        println!("  {w}Address:{r} {g}{b}{}{r}", native["address"].as_str().unwrap_or("?"));
        println!("  {w}WIF Key:{r} {y}{}{r}", native["wif"].as_str().unwrap_or("?"));
        println!();
        import_wifs.push((native["wif"].as_str().unwrap_or("").to_string(), "wpkh".to_string()));
    }
    if let Some(nested) = addresses.get("nested_segwit") {
        println!("  {c}{b}── Nested SegWit (P2SH-P2WPKH) ──{r}  {d}{}{r}", nested["path"].as_str().unwrap_or(""));
        println!("  {w}Address:{r} {g}{b}{}{r}", nested["address"].as_str().unwrap_or("?"));
        println!("  {w}WIF Key:{r} {y}{}{r}", nested["wif"].as_str().unwrap_or("?"));
        println!();
        import_wifs.push((nested["wif"].as_str().unwrap_or("").to_string(), "sh(wpkh".to_string()));
    }
    let wallet_file = format!("{}_wallet.json", wallet_name);
    let save_data = serde_json::json!({"name": wallet_name, "mnemonic": mnemonic, "addresses": addresses});
    if let Ok(j) = serde_json::to_string_pretty(&save_data) { std::fs::write(&wallet_file, &j).ok(); }
    println!("  {d}Saved to / 已保存到: {wallet_file}{r}");
    println!("  {red}{b}Delete this file after backing up! / 备份后请删除此文件!{r}");
    println!();
    println!("  {c}{b}── Importing to Bitcoin Core / 导入全节点 ──{r}");
    let create_result = btc_cli_output(&["createwallet", &wallet_name, "false", "false", "", "false", "true", "true"], false);
    if create_result.contains("error") && !create_result.contains("already exists") { let _ = btc_cli_output(&["loadwallet", &wallet_name], false); }
    let mut imported = 0;
    for (wif, desc_type) in &import_wifs {
        let desc = if desc_type == "tr" { format!("tr({})", wif) } else if desc_type.starts_with("sh(wpkh") { format!("sh(wpkh({}))", wif) } else { format!("wpkh({})", wif) };
        let desc_info = btc_cli_output(&[&format!("-rpcwallet={}", wallet_name), "getdescriptorinfo", &desc], false);
        let checksum = serde_json::from_str::<serde_json::Value>(&desc_info).ok().and_then(|v| v["checksum"].as_str().map(|s| s.to_string())).unwrap_or_default();
        if checksum.is_empty() { println!("  {y}⚠ Could not get checksum for {}{r}", desc_type); continue; }
        let import_json = format!("[{{\"desc\": \"{}#{}\", \"timestamp\": \"now\", \"active\": false}}]", desc, checksum);
        let result = btc_cli_output(&[&format!("-rpcwallet={}", wallet_name), "importdescriptors", &import_json], false);
        if !result.contains("error") || result.contains("\"success\": true") { imported += 1; println!("  {g}✓ Imported {} key{r}", desc_type); }
        else { println!("  {y}⚠ Failed to import {}{r}", desc_type); }
    }
    if imported > 0 { println!(); println!("  {g}{b}✓ Wallet imported to Bitcoin Core!{r}"); }
    println!();
    if addresses.get("taproot").is_some() {
        let addr = addresses["taproot"]["address"].as_str().unwrap_or("");
        println!("  {c}{b}── Ready to Mint / 准备铸造 ──{r}");
        println!("  {w}1. Send BTC to: {g}{b}{}{r}", addr);
        println!("  {w}2. Need at least ~10,000 sats{r}");
        println!("  {w}3. Use [4] Mainnet Mint{r}");
    }
}

// ═══════════════════════════════════════════
//  [5] 钱包信息
// ═══════════════════════════════════════════

fn menu_wallet_info() {
    ui::sub_menu_wallet();
    let (is_regtest, _rpc_port) = match read_line().trim() { "1" => (true, "18443"), "2" => (false, "8332"), _ => return };
    println!("");
    let wallets_json = btc_cli_output(&["listwallets"], is_regtest);
    if wallets_json.contains("error") { println!("  ❌ Cannot connect to node"); return; }
    let wallets: Vec<String> = serde_json::from_str::<serde_json::Value>(&wallets_json).ok()
        .and_then(|v| v.as_array().map(|a| a.iter().filter_map(|w| w.as_str().map(|s| s.to_string())).collect())).unwrap_or_default();
    if wallets.is_empty() { println!("  No wallets found. Use [6] to create one."); return; }
    println!("  Wallets / 钱包列表:");
    for (i, w) in wallets.iter().enumerate() { println!("    [{}] {}", i + 1, w); }
    println!();
    print!("  Select wallet / 选择钱包 (number): ");
    io::stdout().flush().unwrap();
    let sel: usize = read_line().trim().parse().unwrap_or(1);
    let wallet_name = match wallets.get(sel.saturating_sub(1)) { Some(w) => w.clone(), None => { println!("  Invalid"); return; } };
    println!("");
    let balance = btc_cli_output(&[&format!("-rpcwallet={}", wallet_name), "getbalance"], is_regtest);
    if balance.contains("error") || balance.is_empty() { println!("  ❌ Cannot get balance"); return; }
    println!("  💰 Wallet: {}", wallet_name);
    println!("  💰 BTC Balance: {} BTC", balance.trim());
    let utxo_json = btc_cli_output(&[&format!("-rpcwallet={}", wallet_name), "listunspent"], is_regtest);
    if let Ok(utxos) = serde_json::from_str::<serde_json::Value>(&utxo_json) {
        if let Some(arr) = utxos.as_array() {
            println!("  📦 UTXOs: {}", arr.len());
            println!("");
            let total_sats: f64 = arr.iter().map(|u| u["amount"].as_f64().unwrap_or(0.0)).sum();
            println!("  Total: {:.8} BTC ({} sats)", total_sats, (total_sats * 1e8) as u64);
            println!("");
            let show = arr.len().min(10);
            for (i, utxo) in arr.iter().take(show).enumerate() {
                println!("  [{}] {}:{} | {:.8} BTC | {} confirms", i + 1,
                    utxo["txid"].as_str().unwrap_or("?"), utxo["vout"].as_u64().unwrap_or(0),
                    utxo["amount"].as_f64().unwrap_or(0.0), utxo["confirmations"].as_u64().unwrap_or(0));
                println!("       {}", utxo["address"].as_str().unwrap_or("?"));
            }
            if arr.len() > 10 { println!("  ... {} more UTXOs", arr.len() - 10); }
        }
    }
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
        Err(_) => println!("  ⚠️  Indexer not running"),
    }
}

// ═══════════════════════════════════════════
//  单次铸造引擎 (v2.9, 不变)
// ═══════════════════════════════════════════

fn execute_mint(
    datadir: &str, rpc_url: &str, rpc_user: &str, rpc_pass: &str,
    privkey_wif: &str, fee_rate: f64, network: Network,
) -> Result<(String, String), String> {
    let secp = Secp256k1::new();
    let privkey = PrivateKey::from_wif(privkey_wif).map_err(|e| format!("Invalid WIF: {}", e))?;
    let secret_key = privkey.inner;
    let keypair = bitcoin::secp256k1::Keypair::from_secret_key(&secp, &secret_key);
    let (x_only_pubkey, _) = keypair.x_only_public_key();
    let internal_key = bitcoin::key::UntweakedPublicKey::from(x_only_pubkey);
    let pubkey = PublicKey::from_private_key(&secp, &privkey);
    let minter_address = Address::p2tr(&secp, internal_key, None, network);
    println!("    Address: {}", minter_address);

    let next_seq = query_indexer_seq();
    if next_seq > TOTAL_MINTS { return Err("Minting ended / 铸造已结束".into()); }
    println!("    Progress: {} / {} remaining", TOTAL_MINTS - next_seq + 1, TOTAL_MINTS);

    let (block_hash_hex, block_height) = get_latest_block(rpc_url, rpc_user, rpc_pass)?;
    println!("    Block: {} ({})", block_height, &block_hash_hex[..12]);

    print!("    Generating proof... ");
    io::stdout().flush().unwrap();
    let block_hash_bytes: [u8; 32] = hex::decode(&block_hash_hex).map_err(|e| e.to_string())?.try_into().map_err(|_| "hash len")?;
    let pubkey_bytes: [u8; 33] = pubkey.to_bytes().try_into().map_err(|_| "pubkey len")?;
    let rpc_u = rpc_url.to_string(); let rpc_usr = rpc_user.to_string(); let rpc_pw = rpc_pass.to_string();
    let get_raw = move |h: u32| -> Result<Vec<u8>, String> { proof::read_raw_block_via_rpc(&rpc_u, &rpc_usr, &rpc_pw, h) };
    let two_round = proof::generate_proof(&block_hash_bytes, &block_hash_hex, block_height, &pubkey_bytes, &get_raw).map_err(|e| format!("Proof failed: {}", e))?;
    println!("✅ ({}s)", two_round.round2_ts - two_round.round1_ts);

    let pubkey_hex = hex::encode(x_only_pubkey.serialize());
    let interlock = transaction::build_interlock(&two_round, &pubkey_hex).map_err(|e| format!("Interlock failed: {}", e))?;
    println!("    Interlock: ✅");

    let inscription_script = {
        let json_bytes = interlock.witness_json.as_bytes();
        let mut builder = ScriptBuilder::new()
            .push_x_only_key(&x_only_pubkey).push_opcode(opcodes::all::OP_CHECKSIG)
            .push_opcode(opcodes::OP_FALSE).push_opcode(opcodes::all::OP_IF)
            .push_slice(b"nexus").push_slice([0x01]).push_slice(b"application/nexus-mint")
            .push_opcode(opcodes::all::OP_PUSHBYTES_0);
        for chunk in json_bytes.chunks(520) {
            builder = builder.push_slice(
                PushBytesBuf::try_from(chunk.to_vec()).map_err(|e| format!("payload chunk: {}", e))?
            );
        }
        builder.push_opcode(opcodes::all::OP_ENDIF).into_script()
    };
    let taproot_builder = TaprootBuilder::new().add_leaf(0, inscription_script.clone()).map_err(|e| format!("taproot leaf: {:?}", e))?;
    let spend_info = taproot_builder.finalize(&secp, internal_key).map_err(|_| "taproot finalize failed")?;
    let commit_address = Address::p2tr_tweaked(spend_info.output_key(), network);

    // UTXO 安全管理
    let mut utxo_mgr = UtxoManager::load();
    let raw_utxos = list_unspent_rpc(rpc_url, rpc_user, rpc_pass, &minter_address.to_string())?;
    if raw_utxos.is_empty() { return Err(format!("No UTXOs, send BTC to {}", minter_address)); }
    let utxo_records: Vec<UtxoRecord> = raw_utxos.iter().map(|u| UtxoRecord { txid: u.0.clone(), vout: u.1, amount: u.2, confirmations: 1, address: minter_address.to_string() }).collect();
    let live_keys: std::collections::HashSet<String> = utxo_records.iter().map(|r| r.key()).collect();
    utxo_mgr.cleanup_spent_changes(&live_keys);

    let opreturn_script = transaction::build_opreturn_script(&interlock.opreturn_bytes);
    let reveal_vsize: u64 = 300 + (interlock.witness_json.len() as u64 / 4);
    let reveal_fee = (reveal_vsize as f64 * fee_rate).ceil() as u64;
    let commit_output_value = TOKEN_OUTPUT_SATS + MINT_FEE_SATS + reveal_fee;
    let commit_vsize_estimate = COMMIT_VSIZE_BASE + COMMIT_VSIZE_PER_INPUT + COMMIT_VSIZE_PER_OUTPUT;
    let commit_fee_estimate = (commit_vsize_estimate as f64 * fee_rate).ceil() as u64;
    let total_needed = commit_output_value + commit_fee_estimate + DUST_LIMIT;

    let pre_check = utxo_mgr.pre_check(&utxo_records, total_needed);
    pre_check.print();
    if !pre_check.sufficient { return Err(format!("余额不足, 需充值至少 {} sats 到 {}", pre_check.deficit, minter_address)); }

    let (selected_utxos, selected_total) = utxo_mgr.select_for_commit(&utxo_records, total_needed)?;
    println!("    Selected UTXOs / 选中UTXO:");
    for u in &selected_utxos { println!("      {}:{} ({} sats)", &u.txid[..12], u.vout, u.amount); }

    let actual_commit_vsize = COMMIT_VSIZE_BASE + COMMIT_VSIZE_PER_INPUT * selected_utxos.len() as u64 + COMMIT_VSIZE_PER_OUTPUT;
    let commit_fee = (actual_commit_vsize as f64 * fee_rate).ceil() as u64;
    let change_value = selected_total.saturating_sub(commit_output_value + commit_fee);
    println!("    Commit output: {} sats", commit_output_value);
    println!("    Commit fee:    {} sats ({} inputs, {} vB)", commit_fee, selected_utxos.len(), actual_commit_vsize);
    if change_value > DUST_LIMIT { println!("    Change:        {} sats (回到钱包)", change_value); }

    // 构建 Commit TX
    let commit_inputs: Vec<TxIn> = selected_utxos.iter().map(|u| TxIn {
        previous_output: OutPoint::new(Txid::from_str(&u.txid).expect("invalid txid"), u.vout),
        script_sig: ScriptBuf::new(), sequence: Sequence::ENABLE_RBF_NO_LOCKTIME, witness: Witness::new(),
    }).collect();
    let mut commit_outputs = vec![TxOut { value: Amount::from_sat(commit_output_value), script_pubkey: commit_address.script_pubkey() }];
    if change_value > DUST_LIMIT { commit_outputs.push(TxOut { value: Amount::from_sat(change_value), script_pubkey: minter_address.script_pubkey() }); }
    let mut commit_tx = Transaction { version: Version::TWO, lock_time: LockTime::ZERO, input: commit_inputs, output: commit_outputs };
    {
        use bitcoin::sighash::{SighashCache, TapSighashType, Prevouts};
        let prevouts_vec: Vec<TxOut> = selected_utxos.iter().map(|u| TxOut { value: Amount::from_sat(u.amount), script_pubkey: minter_address.script_pubkey() }).collect();
        for i in 0..selected_utxos.len() {
            let sighash = { let mut cache = SighashCache::new(&commit_tx); cache.taproot_key_spend_signature_hash(i, &Prevouts::All(&prevouts_vec), TapSighashType::Default).map_err(|e| e.to_string())? };
            let msg = bitcoin::secp256k1::Message::from_digest(sighash.to_byte_array());
            let tweaked = keypair.tap_tweak(&secp, None);
            let sig = secp.sign_schnorr(&msg, &tweaked.to_inner());
            commit_tx.input[i].witness = Witness::p2tr_key_spend(&bitcoin::taproot::Signature { signature: sig, sighash_type: TapSighashType::Default });
        }
    }
    let commit_txid = commit_tx.compute_txid();

    // 构建 Reveal TX
    let fee_addr = Address::from_str(FEE_ADDRESS).map_err(|e| format!("FEE_ADDRESS: {}", e))?.require_network(network).map_err(|e| format!("network: {}", e))?;
    let mut reveal_tx = Transaction { version: Version::TWO, lock_time: LockTime::ZERO,
        input: vec![TxIn { previous_output: OutPoint::new(commit_txid, 0), script_sig: ScriptBuf::new(), sequence: Sequence::ENABLE_RBF_NO_LOCKTIME, witness: Witness::new() }],
        output: vec![
            TxOut { value: Amount::from_sat(TOKEN_OUTPUT_SATS), script_pubkey: minter_address.script_pubkey() },
            TxOut { value: Amount::from_sat(MINT_FEE_SATS), script_pubkey: fee_addr.script_pubkey() },
            TxOut { value: Amount::ZERO, script_pubkey: ScriptBuf::from_bytes(opreturn_script) },
        ],
    };
    {
        use bitcoin::sighash::{SighashCache, TapSighashType, Prevouts};
        let prevouts = [TxOut { value: Amount::from_sat(commit_output_value), script_pubkey: ScriptBuf::new_p2tr_tweaked(spend_info.output_key()) }];
        let leaf_hash = bitcoin::taproot::TapLeafHash::from_script(&inscription_script, LeafVersion::TapScript);
        let mut cache = SighashCache::new(&reveal_tx);
        let sighash = cache.taproot_script_spend_signature_hash(0, &Prevouts::All(&prevouts), leaf_hash, TapSighashType::Default).map_err(|e| e.to_string())?;
        let msg = bitcoin::secp256k1::Message::from_digest(sighash.to_byte_array());
        let sig = secp.sign_schnorr(&msg, &keypair);
        let control_block = spend_info.control_block(&(inscription_script.clone(), LeafVersion::TapScript)).ok_or("control block failed")?;
        let mut witness = Witness::new();
        witness.push(bitcoin::taproot::Signature { signature: sig, sighash_type: TapSighashType::Default }.to_vec());
        witness.push(inscription_script.as_bytes());
        witness.push(control_block.serialize());
        reveal_tx.input[0].witness = witness;
    }
    let reveal_txid = reveal_tx.compute_txid();

    // 广播 + 记录
    print!("    Broadcasting Commit... "); io::stdout().flush().unwrap();
    broadcast(rpc_url, rpc_user, rpc_pass, &commit_tx)?; println!("✅ {}", commit_txid);
    print!("    Broadcasting Reveal... "); io::stdout().flush().unwrap();
    broadcast(rpc_url, rpc_user, rpc_pass, &reveal_tx)?; println!("✅ {}", reveal_txid);
    utxo_mgr.record_mint(&reveal_txid.to_string(), 0, TOKEN_OUTPUT_SATS);
    if change_value > DUST_LIMIT {
        utxo_mgr.record_change(&commit_txid.to_string(), 1, change_value);
        println!("    Change recorded: {}:{} ({} sats)", &commit_txid.to_string()[..12], 1, change_value);
    }
    utxo_mgr.save();
    println!("    UTXO records saved / UTXO记录已保存");
    Ok((commit_txid.to_string(), reveal_txid.to_string()))
}

// ═══════════════════════════════════════════
//  批量铸造引擎 (v2.9 新增)
//  每笔使用不同区块高度生成独立proof
//  链式找零: 每笔Commit找零作为下一笔输入
// ═══════════════════════════════════════════

fn execute_batch_mint(
    datadir: &str, rpc_url: &str, rpc_user: &str, rpc_pass: &str,
    privkey_wif: &str, fee_rate: f64, network: Network, count: u32,
) -> Result<Vec<(String, String)>, String> {
    let secp = Secp256k1::new();
    let privkey = PrivateKey::from_wif(privkey_wif).map_err(|e| format!("Invalid WIF: {}", e))?;
    let secret_key = privkey.inner;
    let keypair = bitcoin::secp256k1::Keypair::from_secret_key(&secp, &secret_key);
    let (x_only_pubkey, _) = keypair.x_only_public_key();
    let internal_key = bitcoin::key::UntweakedPublicKey::from(x_only_pubkey);
    let pubkey = PublicKey::from_private_key(&secp, &privkey);
    let minter_address = Address::p2tr(&secp, internal_key, None, network);
    let pubkey_hex = hex::encode(x_only_pubkey.serialize());
    let pubkey_bytes: [u8; 33] = pubkey.to_bytes().try_into().map_err(|_| "pubkey len")?;

    println!("    Address: {}", minter_address);

    // 获取最新区块高度
    let (latest_hash, latest_height) = get_latest_block(rpc_url, rpc_user, rpc_pass)?;
    println!("    Latest block: {} ({})", latest_height, &latest_hash[..12]);

    // 加载 UTXO 管理器
    let mut utxo_mgr = UtxoManager::load();

    // 获取可用 UTXO
    let raw_utxos = list_unspent_rpc(rpc_url, rpc_user, rpc_pass, &minter_address.to_string())?;
    if raw_utxos.is_empty() { return Err(format!("No UTXOs, send BTC to {}", minter_address)); }
    let utxo_records: Vec<UtxoRecord> = raw_utxos.iter().map(|u| UtxoRecord {
        txid: u.0.clone(), vout: u.1, amount: u.2, confirmations: 1, address: minter_address.to_string(),
    }).collect();
    let live_keys: std::collections::HashSet<String> = utxo_records.iter().map(|r| r.key()).collect();
    utxo_mgr.cleanup_spent_changes(&live_keys);

    // 预计算固定值
    let estimated_witness_len: u64 = 850;
    let reveal_vsize: u64 = 300 + (estimated_witness_len / 4);
    let reveal_fee = (reveal_vsize as f64 * fee_rate).ceil() as u64;
    let commit_output_value = TOKEN_OUTPUT_SATS + MINT_FEE_SATS + reveal_fee;
    let commit_fee_single = ((COMMIT_VSIZE_BASE + COMMIT_VSIZE_PER_INPUT + COMMIT_VSIZE_PER_OUTPUT) as f64 * fee_rate).ceil() as u64;
    let total_needed_all = (commit_output_value + commit_fee_single) * count as u64 + DUST_LIMIT;

    // 选择足够铸造 count 笔的 UTXO
    let (selected_utxos, selected_total) = utxo_mgr.select_for_commit(&utxo_records, total_needed_all)?;
    println!("    Selected {} UTXOs, total {} sats for {} mints", selected_utxos.len(), selected_total, count);

    let fee_addr = Address::from_str(FEE_ADDRESS).map_err(|e| format!("FEE_ADDRESS: {}", e))?
        .require_network(network).map_err(|e| format!("network: {}", e))?;

    let mut results: Vec<(String, String)> = Vec::new();

    // 当前可用的输入 UTXO (第一笔用选中的, 后续用找零)
    let mut current_inputs: Vec<UtxoRecord> = selected_utxos.clone();
    let mut current_total: u64 = selected_total;

    for i in 0..count {
        let mint_num = i + 1;
        let mint_block_height = latest_height - i;

        println!("");
        println!("  \x1b[36m\x1b[1m── Mint #{} / {} ──\x1b[0m  [block {}]", mint_num, count, mint_block_height);

        // 获取该高度的区块hash
        let mint_block_hash = get_block_at_height(rpc_url, rpc_user, rpc_pass, mint_block_height)?;
        let mint_block_hash_bytes: [u8; 32] = hex::decode(&mint_block_hash)
            .map_err(|e| e.to_string())?.try_into().map_err(|_| "hash len")?;

        // 生成独立 proof (不同 block_hash → 不同 seed → 不同 proof)
        print!("    Generating proof... ");
        io::stdout().flush().unwrap();
        let rpc_u = rpc_url.to_string();
        let rpc_usr = rpc_user.to_string();
        let rpc_pw = rpc_pass.to_string();
        let get_raw = move |h: u32| -> Result<Vec<u8>, String> {
            proof::read_raw_block_via_rpc(&rpc_u, &rpc_usr, &rpc_pw, h)
        };
        let two_round = proof::generate_proof(
            &mint_block_hash_bytes, &mint_block_hash, mint_block_height, &pubkey_bytes, &get_raw,
        ).map_err(|e| format!("Proof #{} failed: {}", mint_num, e))?;
        println!("✅ ({}s)", two_round.round2_ts - two_round.round1_ts);

        // 构建互锁
        let interlock = transaction::build_interlock(&two_round, &pubkey_hex)
            .map_err(|e| format!("Interlock #{} failed: {}", mint_num, e))?;

        // 构建铭文脚本 (每笔不同, 因为 proof hash 不同)
        let inscription_script = ScriptBuilder::new()
            .push_x_only_key(&x_only_pubkey).push_opcode(opcodes::all::OP_CHECKSIG)
            .push_opcode(opcodes::OP_FALSE).push_opcode(opcodes::all::OP_IF)
            .push_slice(b"nexus").push_slice([0x01]).push_slice(b"application/nexus-mint")
            .push_opcode(opcodes::all::OP_PUSHBYTES_0)
            .push_slice(PushBytesBuf::try_from(interlock.witness_json.as_bytes().to_vec())
                .map_err(|e| format!("payload: {}", e))?)
            .push_opcode(opcodes::all::OP_ENDIF).into_script();

        let taproot_builder = TaprootBuilder::new()
            .add_leaf(0, inscription_script.clone())
            .map_err(|e| format!("taproot leaf: {:?}", e))?;
        let spend_info = taproot_builder.finalize(&secp, internal_key)
            .map_err(|_| "taproot finalize failed")?;
        let commit_address = Address::p2tr_tweaked(spend_info.output_key(), network);

        let opreturn_script = transaction::build_opreturn_script(&interlock.opreturn_bytes);

        // 计算 Commit 费用
        let actual_commit_vsize = COMMIT_VSIZE_BASE
            + COMMIT_VSIZE_PER_INPUT * current_inputs.len() as u64
            + COMMIT_VSIZE_PER_OUTPUT;
        let commit_fee = (actual_commit_vsize as f64 * fee_rate).ceil() as u64;
        let change_value = current_total.saturating_sub(commit_output_value + commit_fee);

        println!("    Input: {} sats ({} UTXOs)", current_total, current_inputs.len());
        println!("    Commit: {} → commit addr | {} → change | {} fee",
            commit_output_value, if change_value > DUST_LIMIT { change_value } else { 0 }, commit_fee);

        // 构建 Commit TX
        let commit_inputs_tx: Vec<TxIn> = current_inputs.iter().map(|u| TxIn {
            previous_output: OutPoint::new(Txid::from_str(&u.txid).expect("txid"), u.vout),
            script_sig: ScriptBuf::new(), sequence: Sequence::ENABLE_RBF_NO_LOCKTIME, witness: Witness::new(),
        }).collect();

        let mut commit_outputs = vec![TxOut {
            value: Amount::from_sat(commit_output_value), script_pubkey: commit_address.script_pubkey(),
        }];
        if change_value > DUST_LIMIT {
            commit_outputs.push(TxOut {
                value: Amount::from_sat(change_value), script_pubkey: minter_address.script_pubkey(),
            });
        }

        let mut commit_tx = Transaction {
            version: Version::TWO, lock_time: LockTime::ZERO,
            input: commit_inputs_tx, output: commit_outputs,
        };

        // 签名 Commit TX (所有输入)
        {
            use bitcoin::sighash::{SighashCache, TapSighashType, Prevouts};
            let prevouts_vec: Vec<TxOut> = current_inputs.iter().map(|u| TxOut {
                value: Amount::from_sat(u.amount), script_pubkey: minter_address.script_pubkey(),
            }).collect();
            for idx in 0..current_inputs.len() {
                let sighash = {
                    let mut cache = SighashCache::new(&commit_tx);
                    cache.taproot_key_spend_signature_hash(idx, &Prevouts::All(&prevouts_vec), TapSighashType::Default)
                        .map_err(|e| e.to_string())?
                };
                let msg = bitcoin::secp256k1::Message::from_digest(sighash.to_byte_array());
                let tweaked = keypair.tap_tweak(&secp, None);
                let sig = secp.sign_schnorr(&msg, &tweaked.to_inner());
                commit_tx.input[idx].witness = Witness::p2tr_key_spend(
                    &bitcoin::taproot::Signature { signature: sig, sighash_type: TapSighashType::Default }
                );
            }
        }
        let commit_txid = commit_tx.compute_txid();

        // 构建 Reveal TX
        let mut reveal_tx = Transaction {
            version: Version::TWO, lock_time: LockTime::ZERO,
            input: vec![TxIn {
                previous_output: OutPoint::new(commit_txid, 0),
                script_sig: ScriptBuf::new(), sequence: Sequence::ENABLE_RBF_NO_LOCKTIME, witness: Witness::new(),
            }],
            output: vec![
                TxOut { value: Amount::from_sat(TOKEN_OUTPUT_SATS), script_pubkey: minter_address.script_pubkey() },
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
            let leaf_hash = bitcoin::taproot::TapLeafHash::from_script(&inscription_script, LeafVersion::TapScript);
            let mut cache = SighashCache::new(&reveal_tx);
            let sighash = cache.taproot_script_spend_signature_hash(
                0, &Prevouts::All(&prevouts), leaf_hash, TapSighashType::Default,
            ).map_err(|e| e.to_string())?;
            let msg = bitcoin::secp256k1::Message::from_digest(sighash.to_byte_array());
            let sig = secp.sign_schnorr(&msg, &keypair);
            let control_block = spend_info.control_block(&(inscription_script.clone(), LeafVersion::TapScript))
                .ok_or("control block failed")?;
            let mut witness = Witness::new();
            witness.push(bitcoin::taproot::Signature { signature: sig, sighash_type: TapSighashType::Default }.to_vec());
            witness.push(inscription_script.as_bytes());
            witness.push(control_block.serialize());
            reveal_tx.input[0].witness = witness;
        }
        let reveal_txid = reveal_tx.compute_txid();

        // 广播
        print!("    Broadcasting Commit... ");
        io::stdout().flush().unwrap();
        match broadcast(rpc_url, rpc_user, rpc_pass, &commit_tx) {
            Ok(()) => println!("✅ {}", commit_txid),
            Err(e) => {
                println!("❌ {}", e);
                println!("  ⚠️  Stopped at mint #{}/{}. {} mints succeeded.", mint_num, count, results.len());
                utxo_mgr.save();
                return if results.is_empty() { Err(format!("Commit #{} failed: {}", mint_num, e)) } else { Ok(results) };
            }
        }

        print!("    Broadcasting Reveal... ");
        io::stdout().flush().unwrap();
        match broadcast(rpc_url, rpc_user, rpc_pass, &reveal_tx) {
            Ok(()) => println!("✅ {}", reveal_txid),
            Err(e) => {
                println!("❌ {}", e);
                println!("  ⚠️  Commit succeeded but Reveal failed at #{}/{}.", mint_num, count);
                utxo_mgr.save();
                return if results.is_empty() { Err(format!("Reveal #{} failed: {}", mint_num, e)) } else { Ok(results) };
            }
        }

        // 记录 UTXO
        utxo_mgr.record_mint(&reveal_txid.to_string(), 0, TOKEN_OUTPUT_SATS);
        if change_value > DUST_LIMIT {
            utxo_mgr.record_change(&commit_txid.to_string(), 1, change_value);
        }

        results.push((commit_txid.to_string(), reveal_txid.to_string()));
        println!("    \x1b[32m\x1b[1m+500 NXS\x1b[0m  (mint #{}/{})", mint_num, count);

        // 更新下一笔的输入: 用本次 Commit 的找零
        if i < count - 1 {
            if change_value > DUST_LIMIT {
                current_inputs = vec![UtxoRecord {
                    txid: commit_txid.to_string(), vout: 1, amount: change_value,
                    confirmations: 0, address: minter_address.to_string(),
                }];
                current_total = change_value;
            } else {
                println!("  ⚠️  No change left for next mint. Stopped at #{}/{}.", mint_num, count);
                break;
            }
        }
    }

    // 保存所有 UTXO 记录
    utxo_mgr.save();
    println!("");
    println!("    UTXO records saved / UTXO记录已保存");
    println!("    nxs_mints.json:  +{} locked", results.len());
    println!("    nxs_change.json: updated");

    Ok(results)
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
        if let Some(home) = dirs::home_dir() { return path.replacen("~", &home.to_string_lossy(), 1); }
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

/// 获取指定高度的区块hash (批量铸造用)
fn get_block_at_height(rpc_url: &str, user: &str, pass: &str, height: u32) -> Result<String, String> {
    let client = reqwest::blocking::Client::new();
    let hash = rpc_json(&client, rpc_url, user, pass, "getblockhash", &[serde_json::json!(height)])?
        .as_str().ok_or("getblockhash")?.to_string();
    Ok(hash)
}

fn list_unspent_rpc(rpc_url: &str, user: &str, pass: &str, addr: &str) -> Result<Vec<(String, u32, u64)>, String> {
    let client = reqwest::blocking::Client::new();
    let result = rpc_json(&client, rpc_url, user, pass, "listunspent",
        &[serde_json::json!(0), serde_json::json!(9999999), serde_json::json!([addr])])?;
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
    client: &reqwest::blocking::Client, url: &str, user: &str, pass: &str,
    method: &str, params: &[serde_json::Value],
) -> Result<serde_json::Value, String> {
    let body = serde_json::json!({"jsonrpc": "2.0", "id": "nexus", "method": method, "params": params});
    let resp: serde_json::Value = client.post(url).basic_auth(user, Some(pass))
        .json(&body).send().map_err(|e| e.to_string())?.json().map_err(|e| e.to_string())?;
    if let Some(e) = resp.get("error") { if !e.is_null() { return Err(format!("RPC {}: {}", method, e)); } }
    resp.get("result").cloned().ok_or(format!("RPC {}: no result", method))
}
