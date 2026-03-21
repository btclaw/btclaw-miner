const C: &str = "\x1b[36m";
const Y: &str = "\x1b[33m";
const G: &str = "\x1b[32m";
const W: &str = "\x1b[97m";
const D: &str = "\x1b[90m";
const R: &str = "\x1b[0m";
const B: &str = "\x1b[1m";

pub fn banner() {
    print!("\x1b[2J\x1b[H");
    println!();
    println!("  {C}{B}════════════════════════════════════════════════════════════{R}");
    println!();
    println!("  {W}{B}  ███╗   ██╗ ███████╗ ██╗  ██╗ ██╗   ██╗ ███████╗{R}");
    println!("  {W}{B}  ████╗  ██║ ██╔════╝ ╚██╗██╔╝ ██║   ██║ ██╔════╝{R}");
    println!("  {W}{B}  ██╔██╗ ██║ █████╗    ╚███╔╝  ██║   ██║ ███████╗{R}");
    println!("  {W}{B}  ██║╚██╗██║ ██╔══╝    ██╔██╗  ██║   ██║ ╚════██║{R}");
    println!("  {W}{B}  ██║ ╚████║ ███████╗ ██╔╝ ╚██╗╚██████╔╝ ███████║{R}");
    println!("  {W}{B}  ╚═╝  ╚═══╝ ╚══════╝ ╚═╝   ╚═╝ ╚═════╝ ╚══════╝{R}");
    println!();
    println!("  {W}{B}  REACTOR v2.0{R}  {D}Dual-Layer Interlocking Mint Protocol{R}");
    println!("  {D}  ──────────────────────────────────────────────────{R}");
    println!();
    println!("  {W}  Network   {D}Bitcoin L1       {D}│  {W}Supply  {G}21,000,000 NXS{R}");
    println!("  {W}  Per Mint  {D}500 NXS          {D}│  {W}Fee     {Y}5,000 sats{R}");
    println!("  {W}  Security  {D}Full Node        {D}│  {W}Total   {Y}42,000 mints{R}");
    println!();
    println!("  {C}{B}════════════════════════════════════════════════════════════{R}");
    println!();
}

pub fn main_menu() {
    println!("  {C}{B}────────────────────────────────────────────────────────────{R}");
    println!();
    println!("    {Y}{B}[1]{R}  {W}Install / Sync Full Node     {D}安装/同步全节点{R}");
    println!("    {Y}{B}[2]{R}  {W}Sync Progress                {D}查看同步进度{R}");
    println!("    {G}{B}[3]{R}  {W}Testnet Mint (regtest)       {D}测试网铸造{R}");
    println!("    {Y}{B}[4]{R}  {W}Mainnet Mint                 {D}主网铸造{R}");
    println!("    {W}{B}[5]{R}  {W}Wallet Info                  {D}钱包信息{R}");
    println!();
    println!("    {D}[0]  Exit 退出{R}");
    println!();
    println!("  {C}{B}────────────────────────────────────────────────────────────{R}");
    println!();
    print!("  {Y}{B}> {R}");
    std::io::Write::flush(&mut std::io::stdout()).unwrap();
}

pub fn sub_menu_node() {
    println!("  {C}{B}── {W}Install / Sync BTC Full Node{R}  {D}安装/同步全节点{R}  {C}{B}──{R}");
    println!();
    println!("    {Y}{B}[1]{R}  {W}Install Bitcoin Core          {D}安装{R}");
    println!("    {Y}{B}[2]{R}  {W}Start Mainnet Sync (~600GB)   {D}主网同步{R}");
    println!("    {G}{B}[3]{R}  {W}Start Regtest Node (<1GB)     {D}测试节点{R}");
    println!("    {D}[0]  Back 返回{R}");
    println!();
    print!("  {Y}{B}> {R}");
    std::io::Write::flush(&mut std::io::stdout()).unwrap();
}

pub fn sub_menu_wallet() {
    println!("  {C}{B}── {W}Wallet Info{R}  {D}钱包信息{R}  {C}{B}──{R}");
    println!();
    println!("    {G}{B}[1]{R}  {W}Regtest Wallet               {D}测试网钱包{R}");
    println!("    {Y}{B}[2]{R}  {W}Mainnet Wallet               {D}主网钱包{R}");
    println!("    {D}[0]  Back 返回{R}");
    println!();
    print!("  {Y}{B}> {R}");
    std::io::Write::flush(&mut std::io::stdout()).unwrap();
}
