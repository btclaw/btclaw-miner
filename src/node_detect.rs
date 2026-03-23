use std::path::Path;
use std::process::Command;
use serde::{Serialize, Deserialize};

const CONFIG_FILE: &str = "nexus_config.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NexusConfig {
    pub bitcoin_datadir: Option<String>,
    pub rpc_url: String,
    pub rpc_user: String,
    pub rpc_pass: String,
    pub network: String,
}

impl Default for NexusConfig {
    fn default() -> Self {
        Self {
            bitcoin_datadir: None,
            rpc_url: "http://127.0.0.1:8332".into(),
            rpc_user: "nexus".into(),
            rpc_pass: "nexustest123".into(),
            network: "main".into(),
        }
    }
}

impl NexusConfig {
    pub fn load() -> Self {
        std::fs::read_to_string(CONFIG_FILE).ok()
            .and_then(|j| serde_json::from_str(&j).ok())
            .unwrap_or_default()
    }
    pub fn save(&self) {
        if let Ok(j) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(CONFIG_FILE, j);
        }
    }
}

#[derive(Debug)]
pub struct NodeDetection {
    pub found: bool,
    pub datadir: String,
    pub source: String,
    pub running: bool,
    pub chain: String,
    pub blocks: u64,
    pub headers: u64,
    pub progress: f64,
    pub ibd: bool,
    pub size_gb: f64,
}

/// 自动检测服务器上的Bitcoin全节点
/// 检查顺序: 用户配置 → 运行中进程 → 常见路径 → bitcoin-cli
pub fn detect_node(config: &NexusConfig) -> NodeDetection {
    let mut r = NodeDetection {
        found: false, datadir: String::new(), source: String::new(),
        running: false, chain: String::new(), blocks: 0, headers: 0,
        progress: 0.0, ibd: true, size_gb: 0.0,
    };

    // 1. 用户配置的路径
    if let Some(ref dir) = config.bitcoin_datadir {
        if has_blocks(dir) {
            r.found = true;
            r.datadir = dir.clone();
            r.source = "User config / 用户配置".into();
            fill(&mut r, config);
            return r;
        }
    }

    // 2. 运行中的bitcoind进程
    if let Some(dir) = detect_process() {
        if has_blocks(&dir) {
            r.found = true;
            r.datadir = dir;
            r.source = "Running process / 运行中进程".into();
            r.running = true;
            fill(&mut r, config);
            return r;
        }
    }

    // 3. 常见路径
    for p in common_paths() {
        if has_blocks(&p) {
            r.found = true;
            r.datadir = p;
            r.source = "Auto-detected / 自动检测".into();
            fill(&mut r, config);
            return r;
        }
    }

    // 4. bitcoin-cli
    if let Some(dir) = detect_cli(config) {
        r.found = true;
        r.datadir = dir;
        r.source = "bitcoin-cli / RPC".into();
        r.running = true;
        fill(&mut r, config);
        return r;
    }

    r
}

fn has_blocks(dir: &str) -> bool {
    let blocks = Path::new(dir).join("blocks");
    if !blocks.exists() { return false; }
    if let Ok(entries) = std::fs::read_dir(&blocks) {
        for e in entries.flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            if n.starts_with("blk") && n.ends_with(".dat") { return true; }
        }
    }
    false
}

fn detect_process() -> Option<String> {
    let o = Command::new("bash").arg("-c")
        .arg("ps aux | grep bitcoind | grep -v grep")
        .output().ok()?;
    let s = String::from_utf8_lossy(&o.stdout);
    for line in s.lines() {
        if let Some(i) = line.find("-datadir=") {
            let rest = &line[i + 9..];
            let dir = rest.split_whitespace().next().unwrap_or("");
            if !dir.is_empty() { return Some(dir.to_string()); }
        }
    }
    if !s.trim().is_empty() {
        if let Some(home) = dirs::home_dir() {
            let d = home.join(".bitcoin");
            if d.exists() { return Some(d.to_string_lossy().to_string()); }
        }
    }
    None
}

fn common_paths() -> Vec<String> {
    let mut v = Vec::new();
    if let Some(home) = dirs::home_dir() {
        v.push(home.join(".bitcoin").to_string_lossy().to_string());
    }
    for p in &[
        "/data/bitcoin", "/data/btc", "/data/.bitcoin",
        "/mnt/data/bitcoin", "/mnt/bitcoin", "/opt/bitcoin",
        "/home/bitcoin/.bitcoin", "/var/lib/bitcoin", "/srv/bitcoin",
    ] {
        v.push(p.to_string());
    }
    // 扫描/data/下所有子目录
    if let Ok(entries) = std::fs::read_dir("/data") {
        for e in entries.flatten() {
            let p = e.path().to_string_lossy().to_string();
            if !v.contains(&p) { v.push(p); }
        }
    }
    v
}

fn detect_cli(config: &NexusConfig) -> Option<String> {
    let o = Command::new("bitcoin-cli")
        .args([
            &format!("-rpcuser={}", config.rpc_user),
            &format!("-rpcpassword={}", config.rpc_pass),
            "getblockchaininfo",
        ]).output().ok()?;
    if o.status.success() {
        if let Some(home) = dirs::home_dir() {
            return Some(home.join(".bitcoin").to_string_lossy().to_string());
        }
    }
    None
}

fn fill(r: &mut NodeDetection, config: &NexusConfig) {
    r.size_gb = dir_size_gb(&format!("{}/blocks", r.datadir));
    let args = vec![
        format!("-rpcuser={}", config.rpc_user),
        format!("-rpcpassword={}", config.rpc_pass),
        "getblockchaininfo".into(),
    ];
    if let Ok(o) = Command::new("bitcoin-cli").args(&args).output() {
        if o.status.success() {
            let s = String::from_utf8_lossy(&o.stdout);
            if let Ok(info) = serde_json::from_str::<serde_json::Value>(&s) {
                r.running = true;
                r.chain = info["chain"].as_str().unwrap_or("?").into();
                r.blocks = info["blocks"].as_u64().unwrap_or(0);
                r.headers = info["headers"].as_u64().unwrap_or(0);
                r.progress = info["verificationprogress"].as_f64().unwrap_or(0.0);
                r.ibd = info["initialblockdownload"].as_bool().unwrap_or(true);
            }
        }
    }
}

fn dir_size_gb(path: &str) -> f64 {
    Command::new("du").args(["-sb", path]).output().ok()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout).split_whitespace().next()
                .and_then(|v| v.parse::<u64>().ok()).unwrap_or(0) as f64 / 1e9
        }).unwrap_or(0.0)
}

pub fn print_detection(det: &NodeDetection) {
    let g = "\x1b[32m"; let y = "\x1b[33m"; let c = "\x1b[36m";
    let w = "\x1b[97m"; let d = "\x1b[90m"; let b = "\x1b[1m";
    let r = "\x1b[0m";

    if det.found {
        println!("  {g}{b}✓ Bitcoin node found / 检测到全节点{r}");
        println!("    {w}Path:{r}      {c}{}{r}", det.datadir);
        println!("    {w}Source:{r}    {d}{}{r}", det.source);
        println!("    {w}Size:{r}      {y}{:.1} GB{r}", det.size_gb);
        if det.running {
            println!("    {w}Chain:{r}     {g}{}{r}", det.chain);
            println!("    {w}Blocks:{r}    {w}{} / {}{r}", det.blocks, det.headers);
            let pct = det.progress * 100.0;
            let f = (pct / 100.0 * 25.0) as usize;
            let e = 25usize.saturating_sub(f);
            println!("    {w}Progress:{r}  {g}{}{d}{}{r} {w}{b}{:.2}%{r}",
                "█".repeat(f), "░".repeat(e), pct);
            if det.ibd {
                println!("    {w}Status:{r}    {y}Syncing... / 同步中{r}");
            } else {
                println!("    {w}Status:{r}    {g}{b}Fully synced ✓{r}");
            }
        } else {
            println!("    {w}Status:{r}    {y}Not running / 未运行{r}");
        }
    } else {
        println!("  {y}{b}✗ No Bitcoin node found / 未检测到全节点{r}");
        println!("    {d}Use [1] to install and sync{r}");
    }
}

pub fn choose_datadir() -> String {
    let y = "\x1b[33m"; let w = "\x1b[97m"; let d = "\x1b[90m";
    let b = "\x1b[1m"; let r = "\x1b[0m"; let c = "\x1b[36m";

    println!("  {c}{b}── Choose data directory / 选择数据目录 ──{r}");
    println!();
    println!("    {y}{b}[1]{r}  {w}~/.bitcoin{r}              {d}默认路径{r}");
    println!("    {y}{b}[2]{r}  {w}/data/bitcoin{r}           {d}数据盘{r}");
    println!("    {y}{b}[3]{r}  {w}Custom / 自定义路径{r}");
    println!();
    print!("  {y}{b}> {r}");
    std::io::Write::flush(&mut std::io::stdout()).unwrap();

    let mut input = String::new();
    std::io::stdin().read_line(&mut input).unwrap_or(0);

    match input.trim() {
        "1" => dirs::home_dir().unwrap_or_default()
            .join(".bitcoin").to_string_lossy().to_string(),
        "2" => "/data/bitcoin".to_string(),
        _ => {
            print!("  {w}Full path / 完整路径: {r}");
            std::io::Write::flush(&mut std::io::stdout()).unwrap();
            let mut p = String::new();
            std::io::stdin().read_line(&mut p).unwrap_or(0);
            let p = p.trim().to_string();
            if p.is_empty() {
                dirs::home_dir().unwrap_or_default()
                    .join(".bitcoin").to_string_lossy().to_string()
            } else { p }
        }
    }
}
