# NEXUS Protocol

**[English](README.md)** | **[中文](README_CN.md)**

### 比特币 L1 上首个双层互锁代币协议

每笔铸造交易同时写入 **两个数据层** — Witness（铭文）和 OP\_RETURN — 通过密码学互相绑定。你无法通过网页铸造，无法通过API铸造。你必须运行一个 **完整的比特币归档节点 (~850GB)** 并使用 NEXUS Reactor 软件。

---

## 为什么做 NEXUS

目前所有比特币代币协议都只用了 **一个** 数据层：

| 协议 | 数据层 | 任何人都能铸造？ |
|------|--------|-----------------|
| BRC-20 | Witness（铭文） | 是，通过网页 |
| Runes | OP\_RETURN | 是，通过网页 |
| Ordinals | Witness（铭文） | 是，通过网页 |
| **NEXUS** | **Witness + OP\_RETURN，互锁** | **不行，必须全节点** |

NEXUS 是首个要求 **同时使用两层** 的协议，每层包含另一层的 SHA-256 哈希。现有任何工具 — ord、rune cli 或任何网页铸造器 — 都无法构造这种交易。只有 NEXUS Reactor 可以。

---

## 工作原理

```
┌─────────────────────────────────────────────────────┐
│                  NEXUS 铸造交易                      │
│                                                     │
│  铭文层 (Witness JSON)                               │
│  ┌───────────────────────────────────────┐          │
│  │ p:       "nexus"                      │          │
│  │ op:      "mint"                       │          │
│  │ amt:     500                          │          │
│  │ pk:      <铸造者x-only公钥>            │          │
│  │ fnp:     <全节点证明哈希>              │          │
│  │ opr:     SHA256(OP_RETURN数据) ───────┼──┐       │
│  └───────────────────────────────────────┘  │       │
│                                             │       │
│  协议层 (OP_RETURN ASCII可读)                │       │
│  ┌───────────────────────────────────────┐  │       │
│  │ NXS:MINT:500:w=<wit哈希>:p=<proof哈希>    │  │    │
│  │         ↑                             │  │       │
│  │   SHA256(铭文去掉opr后) ──────────────┼──┘       │
│  └───────────────────────────────────────┘          │
│                                                     │
│  OUTPUT[0]: 330 sats → 铸造者（代币持有者）          │
│  OUTPUT[1]: 5,000 sats → 协议费                     │
│  OUTPUT[2]: OP_RETURN（协议数据）                    │
└─────────────────────────────────────────────────────┘
```

**两层数据互相引用对方的哈希。篡改一层，另一层就对不上。这就是互锁。**

### 链上数据格式

**铭文JSON**（嵌入Taproot铭文中）：
```json
{
  "p": "nexus",
  "op": "mint",
  "amt": 500,
  "pk": "b4906faaf2724a59...",
  "fnp": "a14075ce74aabea5...",
  "opr": "02935680defa678f..."
}
```

**OP\_RETURN**（区块浏览器直接可读）：
```
NXS:MINT:500:w=b8a4cee75bc2a205:p=a14075ce74aabea5
```

---

## 全节点证明

你不是声称自己跑了全节点 — 你需要 **证明**。

Reactor 生成 **两轮密码学挑战**：

1. **第一轮**：你的公钥 + 最新区块哈希 → 派生10个随机历史区块高度 → 直接从本地 `blk*.dat` 文件读取原始字节 → 在计算出的偏移量提取32字节切片 → 哈希为第一轮证明

2. **第二轮**：第一轮证明哈希 → 派生10个 **不同的** 区块高度（第一轮完成前不可预测）→ 同样的提取过程 → 第二轮证明

3. **两轮必须在15秒内完成**

本地 NVMe SSD：约100毫秒。远程API中继：约5-15秒（超时）。

Reactor 还验证你的 `blocks/` 目录：
- `blk*.dat` 文件总大小 > 500 GB
- 至少3,000个区块文件
- 早期文件（`blk00000.dat` 到 `blk00009.dat`）全部存在（pruned节点会删除这些）
- 有效的主网magic字节（支持 Bitcoin Core 30.x XOR 加密）

**不接受pruned节点。不接受SPV。不接受API中继。完整归档节点，或者别来。**

---

## 代币参数

| 参数 | 值 |
|------|-----|
| **名称** | NEXUS (NXS) |
| **总量** | 21,000,000 |
| **每笔铸造** | 500 NXS（固定） |
| **总铸造数** | 42,000 笔 |
| **铸造费** | 5,000 sats/笔 |
| **最低费率** | 0.1 sat/vB |
| **要求** | BTC完整归档节点 + NEXUS Reactor |
| **公平发射** | 零预挖。零团队分配。先到先得。 |

---

## 快速开始

### 系统要求

- **系统**: Ubuntu 22.04+ / Debian 12+
- **Bitcoin Core**: 28.0+（完整归档，非pruned），支持30.x加密版本
- **磁盘**: ~850 GB SSD 存放区块数据
- **Rust**: 1.70+
- **Python**: 3.10+ 带 pip
- **Python库**: `bip_utils`, `base58`

### 安装依赖

```bash
# Rust（如果未安装）
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env

# 系统包
sudo apt update
sudo apt install -y build-essential pkg-config libssl-dev python3 python3-pip

# Python包
pip install bip_utils base58 --break-system-packages -i https://pypi.org/simple/
```

### 编译运行

```bash
# 克隆
git clone https://github.com/btcnexus/nexus-protocol.git
cd nexus-protocol

# 编译主网版
cargo build --release

# 编译测试版（regtest）
cargo build --release --features regtest

# 启动 Reactor
./target/release/nexus-reactor
```

### 交互式菜单

```
════════════════════════════════════════════════════════════

  ███╗   ██╗ ███████╗ ██╗  ██╗ ██╗   ██╗ ███████╗
  ████╗  ██║ ██╔════╝ ╚██╗██╔╝ ██║   ██║ ██╔════╝
  ██╔██╗ ██║ █████╗    ╚███╔╝  ██║   ██║ ███████╗
  ██║╚██╗██║ ██╔══╝    ██╔██╗  ██║   ██║ ╚════██║
  ██║ ╚████║ ███████╗ ██╔╝ ╚██╗╚██████╔╝ ███████║
  ╚═╝  ╚═══╝ ╚══════╝ ╚═╝   ╚═╝ ╚═════╝ ╚══════╝

  REACTOR v2.0  Dual-Layer Interlocking Mint Protocol

────────────────────────────────────────────────────────────

  [1]  Install / Sync Full Node     安装/同步全节点
  [2]  Sync Progress                查看同步进度
  [3]  Testnet Mint (regtest)       测试网铸造
  [4]  Mainnet Mint                 主网铸造（单次/批量）
  [5]  Wallet Info                  钱包信息
  [6]  Create Wallet                创建钱包

  [0]  Exit 退出
```

---

## 铸造教程

### 第一步 — 安装同步全节点

菜单选 `[1]`。Reactor 会：
- 自动检测是否安装了 Bitcoin Core
- 让你选择数据目录（`~/.bitcoin`、`/data/bitcoin`、或自定义路径）
- 根据系统内存自动配置 `dbcache`
- 用优化参数启动 `bitcoind`

Reactor **自动检测服务器上已有的节点** — 检查运行中的进程、常见路径、已保存的配置。

### 第二步 — 创建钱包

选 `[6]` 创建新钱包。选择地址类型：

| 类型 | 前缀 | 标准 | 推荐 |
|------|------|------|------|
| **Taproot** | `bc1p...` | BIP86 (P2TR) | ✅ 推荐 |
| **原生隔离见证** | `bc1q...` | BIP84 (P2WPKH) | 不错 |
| **嵌套隔离见证** | `3...` | BIP49 (P2SH-P2WPKH) | 旧版 |

钱包生成内容：
- **12词 BIP39 助记词**（兼容 UniSat、OKX、Sparrow 等钱包）
- 每种地址类型的 **WIF 私钥**
- 自动导入 Bitcoin Core 用于余额查询

### 第三步 — 充值

向你的 Taproot 地址充值至少 **10,000 sats**：
- 5,000 sats → 协议铸造费
- ~20-1,000 sats → 矿工费（0.1-1 sat/vB）
- 剩余 → 作为找零返回

### 第四步 — 铸造

选 `[4]` 主网铸造，然后选择：
- **[1] 单次铸造** — 铸造一笔 500 NXS
- **[2] 批量铸造** — 一次铸造多笔（详见下方批量铸造章节）

单次铸造流程：
1. 选择钱包（自动从 Bitcoin Core 检测）
2. 输入钱包编号（例如已创建钱包填写数字1即可）
3. 设置费率（最低 0.1 sat/vB）
4. 确认并广播

Reactor 处理一切：UTXO安全扫描 → 节点验证 → 证明生成 → 双层互锁 → Commit + Reveal 广播 → UTXO记录更新。

---

## UTXO 安全管理

Reactor 通过 **五层UTXO分类** 系统保护你的资产。在选择任何UTXO作为交易输入前，每个UTXO都会被检查：

| 层级 | 检查 | 结果 |
|------|------|------|
| 1 | 在 `nxs_mints.json` 中（我们铸造的代币） | 锁定 — 绝不花费 |
| 2 | 金额 ≤ 546 sats | 锁定 — 大概率携带铭文/符文 |
| 3 | 在 `nxs_locked.json` 中（检测到的协议资产） | 锁定 |
| 4 | 在 `nxs_change.json` 中（我们的Commit TX找零） | 安全可花费 |
| 5 | 金额 > 1,000 sats | 安全可花费 |
| — | 547–1,000 sats | 灰色地带 — 默认锁定 |

Reactor 还支持 **多UTXO合并** — 当单个UTXO不够大时，自动合并多个安全UTXO作为 Commit TX 输入（最多10个）。找零输出会尽可能生成，避免浪费。

每次铸造前会显示 **UTXO池预检** 报告：
```
── UTXO Pool Status / UTXO池状态 ──
Total UTXOs:    8
Spendable:      2 (15,000 sats)
Locked:         5 (1,650 sats)
Gray zone:      1 (800 sats)
Need:           1,308 sats
Status:         ✅ 余额充足
```

---

## 批量铸造

一次会话中铸造多笔 NXS：

1. 选 `[4]` → `[2]` 批量铸造
2. 输入费率
3. Reactor 扫描UTXO，计算最大可铸造数量
4. 输入你要铸造的数量（1–N）
5. Reactor 逐笔执行 — 每笔使用独立的证明

批量铸造中每笔使用 **不同的区块高度** 生成证明（`block_height`、`block_height-1`、`block_height-2`...），产生完全独立的证明。每笔 Commit TX 的找零直接作为下一笔的输入 — 不需要等确认。

```
铸造 #1 [区块 942022]: 15,000 → 726 commit + 14,077 找零
铸造 #2 [区块 942021]: 14,077 → 726 commit + 13,154 找零
铸造 #3 [区块 942020]: 13,154 → 726 commit + 12,231 找零
```

如果广播中途失败，已完成的铸造会被保留。Reactor 保存所有UTXO记录，你可以重启继续铸造。

---

## 测试网（regtest）

10分钟内体验完整铸造流程，无需真实 BTC：

1. 用 regtest 编译：`cargo build --release --features regtest`
2. 选 `[1]` → `[3]` 启动本地测试节点（200个区块，5000 BTC）
3. 选 `[3]` 铸造 — 全自动，秒确认

---

## 项目结构

```
nexus-protocol/
├── src/
│   ├── main.rs          # Reactor CLI — 交互菜单 + 单次/批量铸造引擎
│   ├── lib.rs           # 模块导出
│   ├── constants.rs     # 协议参数（主网/regtest 通过 feature flag 切换）
│   ├── proof.rs         # 全节点证明 + Bitcoin Core 30.x 加密支持
│   ├── transaction.rs   # 双层互锁构造 + pk身份绑定
│   ├── indexer.rs       # 交易验证引擎（7条规则 + DoS预过滤）
│   ├── utxo.rs          # UTXO安全分类 + 选择 + 记录追踪
│   ├── node_detect.rs   # 自动检测节点 + 路径管理
│   └── ui.rs            # 终端彩色界面
├── scripts/
│   └── wallet_gen.py    # BIP39/86/84/49 钱包生成器（bip_utils）
├── docs/
│   └── PROTOCOL.md      # 完整协议规范
├── Cargo.toml
├── README.md            # English
└── README_CN.md         # 中文
```

---

## 铸造交易流程

### 单次铸造

```
[1] 验证节点     自动检测数据目录 → blk*.dat > 500GB → XOR解密验证 ✓
         │
[2] 生成证明     两轮挑战 → 20个随机区块 → 15秒窗口
         │
[3] 构建互锁     铭文JSON(含pk) ←SHA256→ OP_RETURN(ASCII)
         │
[4] UTXO选择     加载记录 → 五层分类 → 选择 + 合并输入
         │
[5] Commit交易   BTC → 包含铭文脚本树的Taproot地址 + 找零
         │
[6] Reveal交易   脚本路径花费 → 铭文 + OP_RETURN + 铸造费
         │
[7] 记录UTXO     Reveal output[0] → nxs_mints.json | 找零 → nxs_change.json
         │
[8] 确认入块     区块打包 → Indexer验证7条规则 → 记入500 NXS
```

### 批量铸造

```
[1] 验证节点 + 扫描UTXO → 计算最大可铸数量
         │
[2] 用户选择铸造数量 (1-N)
         │
    ┌────┴────────────────────────────────────────────┐
    │  每笔铸造:                                       │
    │    获取 (最新高度 - i) 的区块哈希               │
    │    生成独立证明 → 构建互锁                       │
    │    Commit TX (输入 = 上一笔找零) → Reveal TX     │
    │    记录铸造 + 找零                               │
    │    如果广播失败 → 停止, 保存, 报告               │
    └────┬────────────────────────────────────────────┘
         │
[3] 显示总结 (N笔铸造, 总NXS, 剩余余额)
```

---

## Indexer 验证规则

铸造有效当且仅当 **全部规则** 通过（按成本排序——轻量检查在前，防DoS）：

1. **格式验证**：Witness铭文包含 `"nexus"` 协议标识和有效JSON，含必需字段（`p`、`op`、`amt`、`pk`、`fnp`、`opr`）
2. **OP\_RETURN**：以 `NXS:` 开头，正确的ASCII格式（`NXS:MINT:500:w=<16hex>:p=<16hex>`）
3. **铸造费**：恰好发送指定sats到协议费地址（前置检查，快速拒绝垃圾）
4. **互锁验证**：双层哈希匹配 — `SHA256(OP_RETURN) == witness.opr` 且 `SHA256(witness去掉opr)[..8] == OP_RETURN.w`
5. **身份绑定**：JSON中的 `pk` 字段必须匹配签名交易的Taproot x-only公钥（防身份冒充）
6. **全节点证明**：两轮验证通过，含预检查（heights数量、时间窗口、字段长度）+ 已用证明去重防重放
7. **总量上限**：总铸造数 ≤ 42,000

序号由 **Indexer 按区块内交易位置分配**。先确认先得。

---

## 安全性

| 攻击向量 | 防御 |
|---------|------|
| API中继（无全节点） | 两轮15秒窗口。本地约100ms vs API约5-15s |
| Pruned节点伪装 | 直接读磁盘：blk文件 > 500GB + 早期文件存在 |
| 身份冒充 | `pk`字段绑定Taproot签名密钥 + Indexer交叉验证 |
| 证明重放 | Indexer中的已用证明去重表 |
| 互锁篡改 | 双向SHA-256哈希验证（pk参与哈希计算） |
| 铸造排序 | Indexer按区块内交易位置分配序号 — 先到先得 |
| Bitcoin Core 30.x加密 | 自动检测obfuscation key，XOR解密blk文件 |
| DoS（垃圾proof刷量） | 轻量检查优先（fee、格式、互锁），再跑昂贵的proof验证 |
| 无限铸造 | 固定 `amt=500`、总量上限、proof唯一性 |
| 资产UTXO误烧 | 五层UTXO分类；330/546 sats输出默认锁定 |
| 批量proof碰撞 | 每笔批量铸造使用不同区块高度 → 唯一proof哈希 |

---

## 链上验证

每笔 NEXUS 铸造在任何区块浏览器上永久可见：

**OP\_RETURN（人类可读）：**
```
NXS:MINT:500:w=b8a4cee75bc2a205:p=a14075ce74aabea5
```

**铭文JSON：**
```json
{
  "p": "nexus",
  "op": "mint",
  "amt": 500,
  "pk": "b4906faaf2724a591af6ae26aed26c355e65f70565d4c3c0665eeebcbc58332d",
  "fnp": "a14075ce74aabea522d36247e144ea019bda1cb79393323f1133ee3b59344c9f",
  "opr": "02935680defa678f4df10356b5254c0966718d58280e5d3ca89ac05cc7002ba3"
}
```

两层互相引用。`pk` 字段将铸造绑定到签名密钥。

---

## 配置文件

Reactor 将设置保存到 `nexus_config.json`：

```json
{
  "bitcoin_datadir": "/root/.bitcoin",
  "rpc_url": "http://127.0.0.1:8332",
  "rpc_user": "nexus",
  "rpc_pass": "你的密码",
  "network": "main"
}
```

UTXO追踪文件（运行时自动生成）：

| 文件 | 用途 |
|------|------|
| `nxs_mints.json` | 锁定的代币UTXO（Reveal output[0]） |
| `nxs_change.json` | 可复用的找零UTXO（Commit找零输出） |
| `nxs_locked.json` | 检测到的外部协议资产UTXO |

---

## 常见问题

**问：为什么铸造必须跑全节点？**
门槛就是价值。比特币本来就该由节点运营者运行，而不是网页点击者。如果你不愿意为比特币投入850GB，你不是目标用户。

**问：能不能做个网页铸造器？**
不能。全节点证明需要从本地 `blk*.dat` 文件的随机偏移读取原始字节。没有公共API能在15秒窗口内提供这些数据。

**问：能不能假冒铸造？**
不能。Indexer验证7条规则，包括双层哈希互锁、身份绑定（pk必须匹配签名密钥）、proof唯一性和铸造费支付。伪造任何一个环节都会导致验证失败。

**问：有预挖或团队分配吗？**
没有。零预挖。协议费地址每笔铸造收5,000 sats — 仅此而已。全部21,000,000 NXS通过公平铸造分发。

**问：42,000笔铸造完后呢？**
铸造永久结束。NXS只能转账，不能再创建。

**问：哪些钱包支持 NEXUS？**
Reactor 生成 BIP39 标准钱包，兼容 UniSat、OKX Wallet、Sparrow 及任何 BIP86 钱包。转账功能将随 Indexer 基础设施成熟而跟进。

**问：支持 Bitcoin Core 30.x 吗？**
支持。Reactor 自动检测 Bitcoin Core 30.0 引入的 XOR obfuscation key，透明解密 `blk*.dat` 文件。

**问：最低费率是多少？**
0.1 sat/vB。铸造时可设置任意费率。

**问：批量铸造怎么用？**
Reactor 可以在一次会话中铸造多笔 NXS。每笔使用不同的区块高度生成证明，确保proof唯一。每笔 Commit TX 的找零直接传递给下一笔，实现链式铸造，无需等待确认。

**问：批量铸造会不会烧掉我的铭文或符文？**
不会。Reactor 在使用前会分类检查每个UTXO。≤ 546 sats 的输出和已知协议绑定的UTXO会被自动锁定，绝不会被选为输入。

---

## 链接

- **GitHub**: [github.com/btcnexus/nexus-protocol](https://github.com/btcnexus/nexus-protocol)
- **协议规范**: [`docs/PROTOCOL.md`](docs/PROTOCOL.md)

---

## 许可证

MIT

---

*NEXUS — 不跑节点，不铸造。*
