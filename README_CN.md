# NEXUS Protocol

**[English](README.md)** | **[中文](README_CN.md)**

### 比特币 L1 上首个双层互锁代币协议。

每笔铸造交易同时写入**两个数据层** — Witness（铭文）和 OP\_RETURN — 通过密码学相互绑定。你无法通过网站铸造，无法通过 API 铸造。你必须拥有**对比特币原始区块数据的可验证访问能力**（通常为全归档节点 ~850GB）和 NEXUS Reactor 软件。

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

## 三种链上操作

NEXUS 定义三种操作，每种使用适当的数据层：

| 操作 | 数据层 | 需要全节点？ | 用途 |
|------|--------|------------|------|
| **部署** | Witness + OP\_RETURN | 是（仅部署者） | 创世铭文 — 定义代币参数 |
| **铸造** | Witness + OP\_RETURN（互锁） | 是 | 双层互锁 + 全节点证明 |
| **转移** | 仅 OP\_RETURN | 否 | 轻量转移 — Taproot 签名 = 所有权 |

铸造需要 Witness 层来嵌入全节点证明。转移只需 OP\_RETURN，因为 Taproot 签名已经证明了所有权 — 无需重新证明节点状态。

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
│  │ proof:   <完整两轮证明>               │  │       │
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
│  OUTPUT[1]: 1,500 sats → 协议费用                    │
│  OUTPUT[2]: OP_RETURN（协议数据）                     │
└─────────────────────────────────────────────────────┘
```

**两个层互相引用对方的哈希。篡改任何一个，另一个就会断裂。这就是互锁。**

### 链上数据格式

**Witness JSON**（嵌入 Taproot 铭文，超过 520 字节时自动分片）：
```json
{
  "p": "nexus",
  "op": "mint",
  "amt": 500,
  "pk": "b4906faaf2724a591af6ae26aed26c355e65f70...",
  "fnp": "9597d93d7cc4eb7b5bb38faae2e68733bcb7e...",
  "opr": "d61158fca158210c239eea6ea0182c6229785bf...",
  "proof": {
    "round1_hash": "2cf1baef431dff570be52c5807802...",
    "round1_ts": 1774636799,
    "round1_heights": [698166, 228405, 783396, 450259, 474437, 310513, 126297, 20911, 454193, 438057],
    "round2_hash": "33014a4c0fbdd0df6afb26241759df4...",
    "round2_ts": 1774636800,
    "round2_heights": [73488, 836059, 883435, 915542, 151845, 434755, 204560, 510522, 932028, 344689],
    "combined": "9597d93d7cc4eb7b5bb38faae2e68733bcb...",
    "block_hash": "0000000000000000000a3224d322dc7748829b4348e...",
    "block_height": 942504,
    "pubkey": "03b4906faaf2724a591af6ae26aed26c355e65f7056..."
  }
}
```

`proof` 字段包含完整的两轮全节点证明，使任何 Indexer 都能使用自己的 Bitcoin Core RPC 独立重算并验证证明。完整规范详见 [`docs/PROTOCOL.md`](docs/PROTOCOL.md)。

**OP\_RETURN**（任何区块浏览器上可读）：
```
NXS:MINT:500:w=592aa5cd2c86d856:p=9597d93d7cc4eb7b
```

---

## 转移协议

转移仅使用 **OP\_RETURN 层** — 无需 Witness 铭文，无需全节点。Taproot 签名证明所有权。

```
NXS:TRANSFER:<数量>
```

示例：NXS:TRANSFER:500

接收者地址从 OUTPUT[1]（NXS 标记输出）读取，不在 OP\_RETURN 中。这使数据载荷保持在比特币 80 字节中继限制以下。

转移验证规则：
1. **格式** — OP\_RETURN 以 `NXS:TRANSFER:` 开头，有效金额。接收者从 OUTPUT[1] 读取
2. **余额** — 发送者有足够的可用 NXS 余额
3. **签名** — 交易由发送者的 Taproot 密钥签名
4. **确认** — 需要 3 个区块确认才更新余额

3 区块确认规则防止双重花费：转移的 NXS 在广播时立即锁定，余额只在 3 个确认后才更新。

### 批量转账

当一次性购买多个卖单时，市场将它们合并为一笔交易：
```
NXS:BATCH:<金额_1>,<金额_2>[,<金额_3>,...]
```

示例：`NXS:BATCH:500,88` — 买家在一笔原子交易中从卖家 A 购买 500 NXS，从卖家 B 购买 88 NXS。每个金额对应相应的卖家输入，买家地址从 OUTPUT[N] 读取（N = 金额数量）。完整规范详见 [`docs/PROTOCOL.md` §16.5](docs/PROTOCOL.md)。

---

## 全节点证明

你不能只是声称你运行了全节点。你必须**证明**它 — 而且证明会被嵌入链上，任何人都能独立验证。

Reactor 生成一个**两轮密码学挑战**：

1. **第一轮**：你的公钥 + 最新区块哈希 → 确定性推导 10 个随机历史区块高度 → 读取原始区块字节 → 在计算偏移处提取 32 字节切片 → 将所有内容哈希为第一轮证明

2. **第二轮**：第一轮证明哈希 → 推导 10 个**不同的**区块高度（在第一轮完成前无法预测） → 相同提取过程 → 第二轮证明

3. **合并哈希**：SHA256(第一轮哈希 + 第二轮哈希) → 存入 `fnp` 字段

**完整的证明数据**（两轮哈希、时间戳、全部 20 个区块高度、区块哈希、公钥）被嵌入链上 Witness JSON 中。Indexer 使用自己的 Bitcoin Core RPC 独立重算两轮哈希，如果重算结果不匹配则拒绝该铸造。任何伪造的证明都无法通过验证。

Reactor 还会验证你的 `blocks/` 目录：
- `blk*.dat` 总大小 > 500 GB
- 至少 3,000 个区块文件
- 早期文件（`blk00000.dat` 到 `blk00009.dat`）存在（裁剪节点会删除这些）
- 有效的主网魔术字节（支持 Bitcoin Core 30.x XOR 混淆）

**不接受裁剪节点。不接受 SPV。不接受 API 中继。全归档节点，否则免谈。**

---

## 代币参数

| 参数 | 值 |
|------|-----|
| **名称** | NEXUS (NXS) |
| **总供应量** | 21,000,000 |
| **每次铸造** | 500 NXS（固定） |
| **总铸造次数** | 42,000 |
| **铸造费用** | 每次 1,500 sats |
| **最低费率** | 0.1 sat/vB |
| **要求** | BTC 全归档节点 + NEXUS Reactor |
| **公平发射** | 无预挖。无团队分配。先到先得。 |

---

## 快速开始

### 前置要求

- **操作系统**：Ubuntu 22.04+ / Debian 12+
- **Bitcoin Core**：28.0+（全归档，非裁剪），支持 30.x 混淆
- **硬盘**：~850 GB SSD 用于区块链数据
- **Rust**：1.70+
- **Python**：3.10+ 含 pip

### 构建和运行

# 安装 Rust（如果尚未安装）
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env

# 安装系统依赖
sudo apt update
sudo apt install -y build-essential pkg-config libssl-dev python3 python3-pip

# 安装 Python 依赖
pip install bip_utils base58 --break-system-packages -i https://pypi.org/simple/

# 克隆项目
git clone https://github.com/btcnexus/nexus-protocol.git
cd nexus-protocol

# 构建（主网）
cargo build --release

# 运行
./target/release/nexus-reactor

```bash
git clone https://github.com/btcnexus/nexus-protocol.git
cd nexus-protocol
cargo build --release
./target/release/nexus-reactor
```

---

## 铸造指南

1. **同步全节点** — 菜单 `[1]`，Reactor 自动配置
2. **创建钱包** — 菜单 `[6]`，选 Taproot（bc1p...），保存 12 个助记词
3. **充值** — 向 Taproot 地址发送至少 10,000 sats
4. **铸造** — 菜单 `[4]` → 单次或批量 → 设置费率 → 确认广播

---

## UTXO 安全

五层分类保护你的资产：

| 层级 | 检查 | 结果 |
|------|------|------|
| 1 | 在 `nxs_mints.json` 中 | 锁定 — 永不花费 |
| 2 | 金额 ≤ 546 sats | 锁定 — 可能携带铭文/Rune |
| 3 | 在 `nxs_locked.json` 中 | 锁定 |
| 4 | 在 `nxs_change.json` 中 | 可安全花费 |
| 5 | 金额 > 1,000 sats | 可安全花费 |

---

## 批量铸造

每次铸造使用不同区块高度生成证明。找零直接作为下一次输入。失败时已完成的铸造被保留。

---

## 测试网（regtest）

`cargo build --release --features regtest` → 选 `[1]` → `[3]` → 即时出块铸造。

---

## Web 仪表盘

已在 **[bitcoinexus.xyz](https://bitcoinexus.xyz)** 上线 — 铸造进度、持有者排行、最近铸造、地址查询、钱包连接（UniSat/OKX/Xverse）、中英双语。前端仅供查看 — 铸造仍需全节点。

---

## 安全性

| 攻击向量 | 防御 |
|---------|------|
| 伪造证明（随机 fnp） | Indexer 使用自己的 RPC 独立重算两轮哈希。伪造哈希将被拒绝。 |
| 无区块数据访问 | 证明需要 20 个随机区块的原始字节数据。没有真实区块数据，证明验证必定失败。 |
| 裁剪节点伪装 | 直接磁盘读取：blk 文件 > 500GB + 早期文件存在 |
| 身份欺骗 | `pk` 字段绑定到 Taproot 签名密钥 |
| 证明重放 | Indexer 中的已用证明去重 |
| 互锁篡改 | 双向 SHA-256 哈希验证（pk + proof 参与哈希） |
| 铸造排序 | Indexer 按区块中的交易位置分配序号 — 先到先得 |
| Bitcoin Core 30.x 加密 | 自动检测混淆密钥，XOR 解密 |
| DoS（垃圾证明） | 廉价检查优先；20 次 RPC 重算仅在所有轻量检查通过后触发 |
| 无限铸造 | 固定 `amt=500`，供应上限，证明唯一性 |
| UTXO 资产销毁 | 五层分类；dust 输出默认锁定 |
| 批量证明碰撞 | 每次使用不同区块高度 → 唯一证明哈希 |
| 转移双重花费 | 3 区块确认规则；待处理转移锁定发送者余额 |
| 转移余额不足 | Indexer 检查可用余额 = 总额 - 锁定额 |
| 批量转账解析攻击 | 金额数量必须等于卖家输入数量，不匹配则整个批量交易无效 |

---

## 常见问题

**问：为什么必须运行全节点才能铸造？**
门槛本身就是价值。如果你不愿意投入 850GB，说明你不是目标受众。

**问：有人能做一个网页铸造器吗？**
不能。全节点证明需要原始区块数据的访问能力。完整证明被嵌入链上并由 Indexer 独立验证 — 任何伪造的证明都无法通过重算验证。

**问：有人能伪造铸造吗？**
不能。Indexer 使用自己的 Bitcoin Core RPC 独立重算完整的两轮证明。在没有真实原始区块数据的情况下伪造有效证明在计算上是不可行的。

**问：能用远程 RPC 代替自己的节点吗？**
协议验证的是对原始区块数据的访问能力，而非物理磁盘位置。安全保证是：铸造需要对比特币原始区块数据的可验证访问能力 — 而不仅仅是密钥所有权或交易构造能力。

**问：有预挖吗？**
没有。零预挖。全部 21,000,000 NXS 通过公平铸造分发。

**问：转移怎么不需要 Witness 层？**
转移只需 OP\_RETURN — Taproot 签名证明所有权。无需全节点。

**问：为什么转移需要 3 个区块确认？**
防止区块链重组。转移的 NXS 在广播时锁定，防止双重花费。

**问：可以用任何钱包转移 NXS 吗？**
可以。任何 Taproot 兼容钱包（UniSat、OKX、Xverse）都可以通过 Web 前端进行转移。

**问：什么是批量转账？**
将多个卖单合并为一笔交易，使用 `NXS:BATCH` 格式。比多笔独立转账更节省矿工费。

**问：v3.3 更新了什么？**
完整的 TwoRoundProof 现在被嵌入链上 Witness JSON 中。Indexer 独立重算并验证每个证明，彻底消除了使用伪造证明哈希铸造的可能性。

---

## 链接

- **官网**：[bitcoinexus.xyz](https://bitcoinexus.xyz)
- **GitHub**：[github.com/btcnexus/nexus-protocol](https://github.com/btcnexus/nexus-protocol)
- **协议规范**：[`docs/PROTOCOL.md`](docs/PROTOCOL.md)
- **协议文档**：[bitcoinexus.xyz/protocol](https://bitcoinexus.xyz/protocol)

---

## 许可证

MIT

---

*NEXUS — 不运行节点，就不能铸造。*
