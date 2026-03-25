# NEXUS Protocol

**[English](README.md)** | **[中文](README_CN.md)**

### 比特币 L1 上首个双层互锁代币协议。

每笔铸造交易同时写入**两个数据层** — Witness（铭文）和 OP\_RETURN — 通过密码学相互绑定。你无法通过网站铸造，无法通过 API 铸造。你必须运行一个**比特币全归档节点（~850GB）**和 NEXUS Reactor 软件。

---

## 为什么要做 NEXUS

目前所有比特币代币协议都只使用**一个**数据层：

| 协议 | 数据层 | 任何人都能铸造？ |
|------|--------|----------------|
| BRC-20 | Witness（铭文） | 是，通过网站 |
| Runes | OP\_RETURN | 是，通过网站 |
| Ordinals | Witness（铭文） | 是，通过网站 |
| **NEXUS** | **Witness + OP\_RETURN，互锁** | **不行。仅限全节点。** |

NEXUS 是第一个要求**同时使用两个数据层**的协议，每个层都包含另一个层的 SHA-256 哈希。没有任何现有工具 — ord、rune cli 或任何网页铸造器 — 能构建这种交易。只有 NEXUS Reactor 可以。

---

## 工作原理

```
┌─────────────────────────────────────────────────────┐
│                NEXUS 铸造交易                        │
│                                                     │
│  WITNESS 层（铭文 JSON）                             │
│  ┌───────────────────────────────────────┐          │
│  │ p:       "nexus"                      │          │
│  │ op:      "mint"                       │          │
│  │ amt:     500                          │          │
│  │ pk:      <铸造者 x-only 公钥>          │          │
│  │ fnp:     <全节点证明哈希>              │          │
│  │ opr:     SHA256(OP_RETURN 数据) ──────┼──┐       │
│  └───────────────────────────────────────┘  │       │
│                                             │       │
│  OP_RETURN 层（ASCII 可读）                  │       │
│  ┌───────────────────────────────────────┐  │       │
│  │ NXS:MINT:500:w=<wit哈希>:p=<证明哈希>  │  │       │
│  │         ↑                             │  │       │
│  │   SHA256(不含 opr 的 Witness) ────────┼──┘       │
│  └───────────────────────────────────────┘          │
│                                                     │
│  OUTPUT[0]: 330 sats → 铸造者（代币持有者）            │
│  OUTPUT[1]: 5,000 sats → 协议费用                    │
│  OUTPUT[2]: OP_RETURN（协议数据）                     │
└─────────────────────────────────────────────────────┘
```

**两个层互相引用对方的哈希。篡改任何一个，另一个就会断裂。这就是互锁。**

### 链上数据格式

**Witness JSON**（嵌入 Taproot 铭文）：
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

**OP\_RETURN**（任何区块浏览器上可读）：
```
NXS:MINT:500:w=b8a4cee75bc2a205:p=a14075ce74aabea5
```

---

## 全节点证明

你不能只是声称你运行了全节点。你必须**证明**它。

Reactor 生成一个**两轮密码学挑战**：

1. **第一轮**：你的公钥 + 最新区块哈希 → 确定性推导 10 个随机历史区块高度 → 直接从本地 `blk*.dat` 文件读取原始字节 → 在计算偏移处提取 32 字节切片 → 将所有内容哈希为第一轮证明

2. **第二轮**：第一轮证明哈希 → 推导 10 个**不同的**区块高度（在第一轮完成前无法预测） → 相同提取过程 → 第二轮证明

3. **两轮必须在 15 秒内完成**

本地 NVMe SSD：~100ms。远程 API 中继：~5-15 秒（可能超时）。

Reactor 还会验证你的 `blocks/` 目录：
- `blk*.dat` 总大小 > 500 GB
- 至少 3,000 个区块文件
- 早期文件存在（`blk00000.dat` 到 `blk00009.dat`，裁剪节点会删除这些）
- 有效的主网 magic bytes（支持 Bitcoin Core 30.x XOR 混淆）

**不接受裁剪节点。不接受 SPV。不接受 API 中继。全归档节点，否则免谈。**

---

## 代币参数

| 参数 | 值 |
|------|-----|
| **名称** | NEXUS (NXS) |
| **总供应量** | 21,000,000 |
| **每次铸造** | 500 NXS（固定） |
| **总铸造次数** | 42,000 |
| **铸造费用** | 每次 5,000 sats |
| **最低费率** | 0.1 sat/vB |
| **要求** | BTC 全归档节点 + NEXUS Reactor |
| **公平发射** | 无预挖。无团队分配。先到先得。 |

---

## 快速开始

### 前置要求

- **操作系统**：Ubuntu 22.04+ / Debian 12+
- **Bitcoin Core**：28.0+（全归档，非裁剪），支持 30.x 混淆
- **硬盘**：~850 GB SSD 用于区块链数据
- **Rust**：1.70+（`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`）
- **Python**：3.10+ 含 pip
- **Python 包**：`bip_utils`、`base58`

### 安装依赖

```bash
# Rust（如未安装）
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env

# 系统包
sudo apt update
sudo apt install -y build-essential pkg-config libssl-dev python3 python3-pip

# Python 包
pip install bip_utils base58 --break-system-packages -i https://pypi.org/simple/
```

### 构建和运行

```bash
# 克隆
git clone https://github.com/btcnexus/nexus-protocol.git
cd nexus-protocol

# 构建主网版本
cargo build --release

# 构建测试网版本（regtest）
cargo build --release --features regtest

# 启动 Reactor
./target/release/nexus-reactor
```

### 交互菜单

```
════════════════════════════════════════════════════════════

  ███╗   ██╗ ███████╗ ██╗  ██╗ ██╗   ██╗ ███████╗
  ████╗  ██║ ██╔════╝ ╚██╗██╔╝ ██║   ██║ ██╔════╝
  ██╔██╗ ██║ █████╗    ╚███╔╝  ██║   ██║ ███████╗
  ██║╚██╗██║ ██╔══╝    ██╔██╗  ██║   ██║ ╚════██║
  ██║ ╚████║ ███████╗ ██╔╝ ╚██╗╚██████╔╝ ███████║
  ╚═╝  ╚═══╝ ╚══════╝ ╚═╝   ╚═╝ ╚═════╝ ╚══════╝

  REACTOR v2.0  双层互锁铸造协议

────────────────────────────────────────────────────────────

  [1]  安装 / 同步全节点
  [2]  同步进度
  [3]  测试网铸造（regtest）
  [4]  主网铸造（单次 / 批量）
  [5]  钱包信息
  [6]  创建钱包

  [0]  退出
```

---

## 铸造指南

### 第一步 — 安装并同步全节点

从菜单选择 `[1]`。Reactor 将：
- 自动检测是否已安装 Bitcoin Core
- 让你选择数据目录（`~/.bitcoin`、`/data/bitcoin` 或自定义路径）
- 根据系统内存自动配置 `dbcache`
- 使用优化参数启动 `bitcoind`

Reactor **自动检测服务器上的现有节点** — 检查运行中的进程、常用路径（`~/.bitcoin`、`/data/bitcoin` 等）和已保存的配置。

### 第二步 — 创建钱包

选择 `[6]` 创建新钱包。选择地址类型：

| 类型 | 前缀 | 标准 | 推荐 |
|------|------|------|------|
| **Taproot** | `bc1p...` | BIP86 (P2TR) | ✅ 是 |
| **原生 SegWit** | `bc1q...` | BIP84 (P2WPKH) | 良好 |
| **嵌套 SegWit** | `3...` | BIP49 (P2SH-P2WPKH) | 旧版 |

钱包生成：
- **12 个 BIP39 助记词**（标准格式，兼容 UniSat、OKX、Sparrow 等）
- 每种地址类型的 **WIF 私钥**
- 自动导入 Bitcoin Core 用于余额追踪

### 第三步 — 充值地址

向你的 Taproot 地址发送至少 **10,000 sats**：
- 5,000 sats → 协议铸造费
- ~20-1,000 sats → 矿工费（0.1-1 sat/vB）
- 剩余 → 作为找零返回

### 第四步 — 铸造

选择 `[4]` 主网铸造，然后选择：
- **[1] 单次铸造** — 铸造一次 500 NXS
- **[2] 批量铸造** — 一次会话中铸造多笔（见下方批量铸造）

单次铸造流程：
1. 选择钱包（从 Bitcoin Core 自动检测）
2. 输入钱包编号（例如输入 1 选择已创建的钱包）
3. 设置费率（最低 0.1 sat/vB）
4. 确认并广播

Reactor 处理一切：UTXO 安全扫描 → 节点验证 → 证明生成 → 双层互锁 → commit + reveal 广播 → UTXO 记录更新。

---

## UTXO 安全

Reactor 通过**五层 UTXO 分类**系统保护你的资产。在选择任何 UTXO 作为交易输入之前，每个都会被检查：

| 层级 | 检查 | 结果 |
|------|------|------|
| 1 | 在 `nxs_mints.json` 中（我们铸造的代币） | 锁定 — 永不花费 |
| 2 | 金额 ≤ 546 sats | 锁定 — 可能携带铭文/Rune |
| 3 | 在 `nxs_locked.json` 中（检测到的协议资产） | 锁定 |
| 4 | 在 `nxs_change.json` 中（我们的 Commit TX 找零） | 可安全花费 |
| 5 | 金额 > 1,000 sats | 可安全花费 |
| — | 547–1,000 sats | 灰色地带 — 默认锁定 |

Reactor 还支持**多 UTXO 合并** — 当单个 UTXO 不够大时，多个安全 UTXO 合并作为 Commit TX 输入（最多 10 个）。尽可能产生找零输出，消除浪费。

每次铸造前，**预检查**显示你的 UTXO 池状态：
```
── UTXO 池状态 ──
总 UTXO：     8
可花费：      2 (15,000 sats)
已锁定：      5 (1,650 sats)
灰色地带：    1 (800 sats)
需要：        1,308 sats
状态：        ✅ 余额充足
```

---

## 批量铸造

在单次会话中铸造多个 NXS 代币：

1. 选择 `[4]` → `[2]` 批量铸造
2. 输入费率
3. Reactor 扫描 UTXO，计算最大可铸造数量
4. 输入要铸造的数量（1–N）
5. Reactor 按顺序执行 N 次铸造 — 每次使用唯一证明

批量中每次铸造使用**不同的区块高度**生成证明（`block_height`、`block_height-1`、`block_height-2`...），产生完全独立的证明。每次 Commit TX 的找零直接作为下一次的输入 — 无需等待确认。

```
铸造 #1 [区块 942022]: 15,000 → 726 commit + 14,077 找零
铸造 #2 [区块 942021]: 14,077 → 726 commit + 13,154 找零
铸造 #3 [区块 942020]: 13,154 → 726 commit + 12,231 找零
```

如果批量中任何广播失败，已完成的铸造会被保留。Reactor 保存所有 UTXO 记录，你可以重启继续。

---

## 测试网（regtest）

10 分钟内体验完整铸造流程，无需真实 BTC：

1. 使用 regtest 标志构建：`cargo build --release --features regtest`
2. 选择 `[1]` → `[3]` 启动本地 regtest 节点（200 个区块，5000 BTC）
3. 选择 `[3]` 铸造 — 全自动，即时区块确认

---

## Web 仪表盘

协议仪表盘已在 **[bitcoinexus.xyz](https://bitcoinexus.xyz)** 上线，功能包括：

- **实时铸造进度** — 进度条、百分比、剩余铸造次数
- **持有者排行榜** — 按 NXS 余额排名的前 N 名持有者
- **最近铸造动态** — 最新铸造活动及交易链接
- **地址 / 交易查询** — 按比特币地址或交易哈希搜索
- **钱包连接** — 支持 UniSat、OKX Wallet、Xverse
- **双语支持** — 中文 / 英文一键切换

前端仅用于**查看** — 铸造仍需全节点和 NEXUS Reactor CLI。

### API 端点

Indexer（`src/bin/indexer.rs`）在 `api.bitcoinexus.xyz` 提供 REST API：

| 端点 | 描述 |
|------|------|
| `GET /api/status` | 协议状态（供应量、已铸造、持有者、扫描高度） |
| `GET /api/balance/{address}` | 查询地址 NXS 余额 |
| `GET /api/mint/{seq}` | 按序号查询铸造记录 |
| `GET /api/mints?page=1&limit=20` | 分页铸造列表 |
| `GET /api/mints/recent` | 最近 20 条铸造（最新在前） |
| `GET /api/mints/address/{address}` | 按地址查询所有铸造记录 + 余额 |
| `GET /api/holders` | 前 100 名持有者排行 |
| `GET /api/tx/{txid}` | 按 reveal txid 查询铸造记录 |
| `GET /api/mint/tx/{txid}` | 按 txid 查询（前端格式） |
| `GET /api/health` | 服务健康检查 |

---

## 架构

```
nexus-protocol/
├── src/
│   ├── main.rs          # Reactor CLI — 交互菜单 + 单次/批量铸造引擎
│   ├── lib.rs           # 模块导出
│   ├── constants.rs     # 协议参数（通过 feature flag 切换主网/regtest）
│   ├── proof.rs         # 全节点证明 + Bitcoin Core 30.x 混淆支持
│   ├── transaction.rs   # 双层互锁 + pk 身份绑定
│   ├── indexer.rs       # 交易验证引擎（7 条规则 + DoS 预过滤）
│   ├── utxo.rs          # UTXO 安全分类 + 选择 + 记录追踪
│   ├── node_detect.rs   # 自动检测比特币节点 + 路径管理
│   ├── ui.rs            # 终端 UI（带颜色）
│   └── bin/
│       └── indexer.rs   # Indexer HTTP 服务（actix-web，10 个 API 端点 + CORS）
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
[1] 验证节点      自动检测 datadir → blk*.dat > 500GB → XOR 解密 magic ✓
         │
[2] 生成证明      两轮挑战 → 20 个随机区块 → 15 秒窗口
         │
[3] 构建互锁      Witness JSON（含 pk）←SHA256→ OP_RETURN（ASCII）
         │
[4] UTXO 选择     加载记录 → 五层分类 → 选择 + 合并输入
         │
[5] Commit TX     BTC → 含铭文脚本树的 Taproot 地址 + 找零
         │
[6] Reveal TX     脚本路径花费 → 铭文 + OP_RETURN + 费用
         │
[7] 记录 UTXO     Reveal output[0] → nxs_mints.json | 找零 → nxs_change.json
         │
[8] 确认          区块纳入 → Indexer 验证 7 条规则 → 500 NXS 入账
```

### 批量铸造

```
[1] 验证节点 + 扫描 UTXO → 计算最大可铸造数量
         │
[2] 用户选择数量（1-N）
         │
    ┌────┴────────────────────────────────────────────┐
    │  对每次铸造：                                    │
    │    获取区块哈希（latest_height - i）              │
    │    生成唯一证明 → 构建互锁                        │
    │    Commit TX（输入 = 上一次找零） → Reveal         │
    │    记录铸造 + 找零                                │
    │    如果广播失败 → 停止，保存，报告                  │
    └────┬────────────────────────────────────────────┘
         │
[3] 显示摘要（N 次铸造，总 NXS，剩余余额）
```

---

## Indexer 验证规则

铸造有效当且仅当**所有规则**通过（按成本排序 — 廉价检查优先以防止 DoS）：

1. **格式**：Witness 铭文包含 `"nexus"` 协议标识符和有效 JSON，含所有必填字段（`p`、`op`、`amt`、`pk`、`fnp`、`opr`）
2. **OP\_RETURN**：以 `NXS:` 前缀开头，正确的 ASCII 格式（`NXS:MINT:500:w=<16hex>:p=<16hex>`）
3. **费用**：恰好 5,000 sats 发送到协议费用地址（提前检查以拒绝垃圾交易）
4. **互锁**：双层哈希匹配 — `SHA256(OP_RETURN) == witness.opr` 且 `SHA256(witness_without_opr)[..8] == OP_RETURN.w`
5. **身份**：JSON 中的 `pk` 字段必须匹配签署交易的 Taproot x-only 公钥（防止身份欺骗）
6. **证明**：全节点证明通过两轮验证，含预检查（高度数量、时间窗口、字段长度）+ 通过已用证明表防重放
7. **供应量**：总铸造次数 ≤ 42,000（未超过供应上限）

序号由 **Indexer 分配**，基于交易在区块中的位置。先确认，先分配。

---

## 安全性

| 攻击向量 | 防御 |
|---------|------|
| API 中继（无全节点） | 两轮 15 秒窗口。本地 ~100ms vs API ~5-15s |
| 裁剪节点伪装 | 直接磁盘读取：blk 文件 > 500GB + 早期文件存在 |
| 身份欺骗 | `pk` 字段绑定到 Taproot 签名密钥 + Indexer 交叉验证 |
| 证明重放 | Indexer 中的已用证明去重 |
| 互锁篡改 | 双向 SHA-256 哈希验证（pk 参与哈希） |
| 铸造排序 | Indexer 按区块中的交易位置分配序号 — 先到先得 |
| Bitcoin Core 30.x 加密 | 自动检测混淆密钥，XOR 解密 blk 文件 |
| DoS（垃圾无效证明） | 廉价检查优先（费用、格式、互锁），然后才是昂贵的证明验证 |
| 无限铸造 | 固定 `amt=500`，供应上限强制执行，证明唯一性 |
| 携带资产的 UTXO 销毁 | 五层 UTXO 分类；330/546 sats 输出默认锁定 |
| 批量证明碰撞 | 每次批量铸造使用不同区块高度 → 唯一证明哈希 |

---

## 链上验证

每笔 NEXUS 铸造都可在任何区块浏览器上永久查看：

**OP\_RETURN（人类可读）：**
```
NXS:MINT:500:w=b8a4cee75bc2a205:p=a14075ce74aabea5
```

**Witness 铭文（JSON）：**
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

## 配置

Reactor 将设置保存到 `nexus_config.json`：

```json
{
  "bitcoin_datadir": "/root/.bitcoin",
  "rpc_url": "http://127.0.0.1:8332",
  "rpc_user": "nexus",
  "rpc_pass": "your_password",
  "network": "main"
}
```

UTXO 追踪文件（运行时自动生成）：

| 文件 | 用途 |
|------|------|
| `nxs_mints.json` | 锁定的代币 UTXO（Reveal output[0]） |
| `nxs_change.json` | 可复用的找零 UTXO（Commit 找零输出） |
| `nxs_locked.json` | 检测到的外部协议资产 UTXO |

---

## 常见问题

**问：为什么必须运行全节点才能铸造？**
门槛本身就是价值。比特币本来就是让节点运营者来运行的，而不是给网页点击者用的。如果你不愿意为比特币投入 850GB 存储空间，说明你不是目标受众。

**问：有人能做一个网页铸造器吗？**
不能。全节点证明需要在最新区块哈希决定的随机偏移处从本地 `blk*.dat` 文件读取原始字节。没有公共 API 能在 15 秒窗口内提供所需格式的数据。

**问：有人能伪造铸造吗？**
不能。Indexer 验证 7 条规则，包括双层哈希互锁、身份绑定（pk 必须匹配签名密钥）、证明唯一性和费用支付。伪造任何一个元素都会打破链条。

**问：有预挖或团队分配吗？**
没有。零预挖。协议费用地址每次铸造收取 5,000 sats — 仅此而已。全部 21,000,000 NXS 通过公平铸造分发。

**问：42,000 次铸造完成后会怎样？**
铸造永久结束。NXS 只能转让，不能再创造。

**问：哪些钱包支持 NEXUS？**
Reactor 生成 BIP39 标准钱包，兼容 UniSat、OKX Wallet、Sparrow 和任何 BIP86 兼容钱包。随着 Indexer 基础设施成熟，转账支持将随之推出。

**问：支持 Bitcoin Core 30.x 吗？**
支持。Reactor 自动检测 Bitcoin Core 30.0 引入的 XOR 混淆密钥并透明解密 `blk*.dat` 文件。

**问：最低费率是多少？**
0.1 sat/vB。铸造时可设置任意费率。

**问：批量铸造怎么工作？**
Reactor 可以在一次会话中铸造多个 NXS 代币。每次铸造使用不同的区块高度生成证明，确保证明唯一。每次 Commit TX 的找零直接输入下一次，实现无需等待确认的链式铸造。

**问：批量铸造会销毁我的铭文或 Runes 吗？**
不会。Reactor 在使用前对每个 UTXO 进行分类。≤ 546 sats 的输出和已知协议绑定的 UTXO 会自动锁定，永远不会被选为输入。

**问：有 Web 前端吗？**
有。协议仪表盘已在 [bitcoinexus.xyz](https://bitcoinexus.xyz) 上线，包括实时铸造进度、持有者排行榜、最近铸造动态、地址/交易查询和钱包连接支持（UniSat、OKX Wallet、Xverse）。前端仅用于查看 — 铸造仍需全节点和 NEXUS Reactor CLI。

---

## 链接

- **官网**：[bitcoinexus.xyz](https://bitcoinexus.xyz)
- **GitHub**：[github.com/btcnexus/nexus-protocol](https://github.com/btcnexus/nexus-protocol)
- **API**：[api.bitcoinexus.xyz/api/status](https://api.bitcoinexus.xyz/api/status)
- **协议规范**：[`docs/PROTOCOL.md`](docs/PROTOCOL.md)

---

## 许可证

MIT

---

*NEXUS — 不运行节点，就不能铸造。*
