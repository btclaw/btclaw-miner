/// NEXUS Protocol 常量定义
///
/// ⚠️ 测试模式 (regtest) 和 主网模式 通过 feature flag 切换
/// 编译主网版: cargo build --release
/// 编译测试版: cargo build --release --features regtest

/// 代币名称
pub const TOKEN_NAME: &str = "NEXUS";
pub const TOKEN_SYMBOL: &str = "NXS";

/// 总量: 21,000,000 × 10^8 (8位精度)
pub const MAX_SUPPLY: u64 = 21_000_000_00000000;

/// 每笔固定铸造量: 500 × 10^8
pub const MINT_AMOUNT: u64 = 500_00000000;

/// 总铸造笔数: 21,000,000 / 500 = 42,000
pub const TOTAL_MINTS: u32 = 42_000;

/// 铸造费 (satoshis)
pub const MINT_FEE_SATS: u64 = 5_000;

/// 项目方收费地址 (部署前替换为你的真实Taproot地址)
#[cfg(not(feature = "regtest"))]
pub const FEE_ADDRESS: &str = "bc1q_REPLACE_WITH_YOUR_ADDRESS";

#[cfg(feature = "regtest")]
pub const FEE_ADDRESS: &str = "bcrt1pt69vsuspaadg4kd3k8dv48edvq6e2x5td3m4gxjwc43ymd92ve8q7f67e0"; // 运行时动态设置

/// OP_RETURN 魔术数
pub const MAGIC: &[u8; 3] = b"NXS";

/// 协议版本
pub const VERSION: u8 = 0x01;

/// 全节点证明: 每轮挑战区块数
#[cfg(not(feature = "regtest"))]
pub const CHALLENGES_PER_ROUND: usize = 10;

#[cfg(feature = "regtest")]
pub const CHALLENGES_PER_ROUND: usize = 3; // regtest只有200个区块，用3个就够

/// 全节点证明: 两轮最大时间差 (秒)
pub const MAX_ROUND_GAP_SECS: u64 = 15;

/// 全节点证明: 每个切片大小
pub const SLICE_SIZE: usize = 32;

/// 磁盘验证: blk文件最小总大小
#[cfg(not(feature = "regtest"))]
pub const MIN_BLOCKS_DIR_SIZE: u64 = 500_000_000_000; // 500GB 主网

#[cfg(feature = "regtest")]
pub const MIN_BLOCKS_DIR_SIZE: u64 = 1_000; // 1KB regtest

/// 磁盘验证: 最少blk文件数量
#[cfg(not(feature = "regtest"))]
pub const MIN_BLK_FILE_COUNT: u32 = 3_000; // 主网

#[cfg(feature = "regtest")]
pub const MIN_BLK_FILE_COUNT: u32 = 1; // regtest只有1个blk文件

/// Bitcoin mainnet magic bytes
#[cfg(not(feature = "regtest"))]
pub const BTC_MAINNET_MAGIC: [u8; 4] = [0xF9, 0xBE, 0xB4, 0xD9];

/// Bitcoin regtest magic bytes
#[cfg(feature = "regtest")]
pub const BTC_MAINNET_MAGIC: [u8; 4] = [0xF4, 0xC1, 0xFF, 0x14];
