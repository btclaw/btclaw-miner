# NEXUS Protocol

### The first dual-layer interlocking token on Bitcoin L1.

Every mint transaction simultaneously writes to **two data layers** — Witness (inscription) and OP\_RETURN — cryptographically bound to each other. You cannot mint with a website. You cannot mint with an API. You must run a **full Bitcoin archive node (~600GB)** and the NEXUS Reactor software.

---

## Why NEXUS exists

Every Bitcoin token protocol so far has used **one** data layer:

| Protocol | Data Layer | Anyone can mint? |
|----------|-----------|-----------------|
| BRC-20 | Witness (inscription) | Yes, via website |
| Runes | OP\_RETURN | Yes, via website |
| Ordinals | Witness (inscription) | Yes, via website |
| **NEXUS** | **Witness + OP\_RETURN, interlocked** | **No. Full node only.** |

NEXUS is the first protocol that requires **both layers simultaneously**, with each layer containing the SHA-256 hash of the other. No existing tool — ord, rune cli, or any web minter — can construct this transaction. Only the NEXUS Reactor can.

---

## How it works

```
┌─────────────────────────────────────────────────────┐
│                  NEXUS Mint Transaction              │
│                                                     │
│  WITNESS LAYER (Inscription)                        │
│  ┌───────────────────────────────────────┐          │
│  │ protocol:    "nexus"                  │          │
│  │ operation:   "mint"                   │          │
│  │ sequence:    #1                       │          │
│  │ amount:      500 NXS                  │          │
│  │ node_proof:  <full node proof hash>   │          │
│  │ opr_hash:    SHA256(OP_RETURN data) ──┼──┐       │
│  └───────────────────────────────────────┘  │       │
│                                             │       │
│  OP_RETURN LAYER (Protocol)                 │       │
│  ┌───────────────────────────────────────┐  │       │
│  │ magic:       "NXS"                    │  │       │
│  │ version:     1                        │  │       │
│  │ mint_seq:    #1                       │  │       │
│  │ wit_hash:    SHA256(Witness data) ────┼──┘       │
│  │ proof_hash:  <full node proof hash>   │          │
│  └───────────────────────────────────────┘          │
│                                                     │
│  OUTPUT[0]: 330 sats → minter (token holder)        │
│  OUTPUT[1]: 5,000 sats → protocol fee               │
│  OUTPUT[2]: OP_RETURN (protocol data)               │
└─────────────────────────────────────────────────────┘
```

**The two layers reference each other's hash. Tamper with one, the other breaks. This is the interlock.**

---

## Full Node Proof

You don't just claim you run a full node. You **prove** it.

The Reactor generates a **two-round cryptographic challenge**:

1. **Round 1**: Your public key + latest block hash → derives 10 random historical block heights → reads raw bytes directly from your local `blk*.dat` files → extracts 32-byte slices at computed offsets → hashes everything into Round 1 proof

2. **Round 2**: Round 1 proof hash → derives 10 **different** block heights (unpredictable until Round 1 completes) → same extraction process → Round 2 proof

3. **Both rounds must complete within 15 seconds**

Local NVMe SSD: ~100ms. Remote API relay: ~5-15 seconds (likely timeout).

The Reactor also verifies your `~/.bitcoin/blocks/` directory contains:
- Total `blk*.dat` size > 500 GB
- At least 3,000 block files
- Early files (`blk00000.dat` through `blk00009.dat`) present (pruned nodes delete these)
- Valid mainnet magic bytes

**No pruned node. No SPV. No API relay. Full archive or nothing.**

---

## Token Parameters

| Parameter | Value |
|-----------|-------|
| **Name** | NEXUS (NXS) |
| **Total Supply** | 21,000,000 |
| **Per Mint** | 500 NXS (fixed) |
| **Total Mints** | 42,000 |
| **Mint Fee** | 5,000 sats per mint |
| **Requirement** | BTC Full Archive Node + NEXUS Reactor |
| **Fair Launch** | No premine. No team allocation. FCFS. |

---

## Quick Start

### Prerequisites

- Ubuntu 22.04+ / Debian 12+
- Bitcoin Core 28.0+ (full archive, NOT pruned)
- ~600 GB SSD for blockchain data
- Rust toolchain

### Install & Run

```bash
# Clone
git clone https://github.com/btcnexus/nexus-protocol.git
cd nexus-protocol

# Build for mainnet
cargo build --release

# Build for testnet (regtest)
cargo build --release --features regtest

# Launch Reactor
./target/release/nexus-reactor
```

The interactive menu guides you through everything:

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

  [1]  Install / Sync Full Node
  [2]  Sync Progress
  [3]  Testnet Mint (regtest)
  [4]  Mainnet Mint
  [5]  Wallet Info

  [0]  Exit
```

### Testnet — Try it in 10 minutes

Select `[1]` → `[3]` to start a local regtest node. Then select `[3]` from the main menu to execute a full mint cycle: node verification → proof generation → dual-layer interlock → commit + reveal → block confirmation → on-chain verification.

No real BTC needed. No 600GB download. Instant blocks.

---

## Architecture

```
nexus-protocol/
├── src/
│   ├── main.rs          # Reactor CLI — interactive menu + mint engine
│   ├── lib.rs           # Module exports
│   ├── constants.rs     # All protocol parameters (mainnet/regtest via feature flag)
│   ├── proof.rs         # Full node proof: disk verification + two-round challenge
│   ├── transaction.rs   # Dual-layer interlock construction + verification
│   ├── indexer.rs       # Transaction validation engine (6 rules)
│   └── ui.rs            # Terminal UI with color
├── docs/
│   ├── PROTOCOL.md      # Complete protocol specification
│   └── SECURITY_AUDIT.md
└── Cargo.toml
```

---

## Mint Transaction Flow

```
[1] Verify Node     Read ~/.bitcoin/blocks/blk*.dat → size > 500GB ✓
         │
[2] Generate Proof  Two-round challenge → 20 random blocks → 15s window
         │
[3] Build Interlock Witness payload ←SHA256→ OP_RETURN payload
         │
[4] Commit TX       BTC → Taproot address with inscription script tree
         │
[5] Reveal TX       Script-path spend → inscription + OP_RETURN + 5000 sat fee
         │
[6] Confirmed       Block inclusion → Indexer validates → 500 NXS credited
```

---

## Indexer Validation Rules

A mint is valid if and only if **all 6 conditions** are met:

1. Witness inscription contains `"nexus"` protocol identifier and valid JSON
2. OP\_RETURN starts with `"NXS"` magic bytes with correct binary format
3. Dual-layer interlock hashes match (cross-verified)
4. Full node proof passes two-round verification
5. Exactly 5,000 sats sent to the protocol fee address
6. `mint_seq ≤ 42,000` (supply cap not exceeded)

Sequence numbers assigned by block confirmation order. First confirmed, first served.

---

## Security

| Attack Vector | Defense |
|--------------|---------|
| API relay (no full node) | Two-round 15s window. Local ~100ms vs API ~5-15s |
| Pruned node disguise | Direct disk read: blk files > 500GB + early files exist |
| Shared Reactor proxy | Proof bound to minter's public key |
| Proof replay | Used-proof deduplication in Indexer |
| Interlock tampering | Bidirectional SHA-256 hash verification |
| Sequence race | FCFS by block confirmation — same as BRC-20/Runes |

Full audit: [`docs/SECURITY_AUDIT.md`](docs/SECURITY_AUDIT.md)

---

## On-Chain Verification

Every NEXUS mint is permanently visible on-chain with two layers of data:

```
┌── Witness Layer / Inscription ──
│ Protocol:    nexus
│ Operation:   mint
│ Sequence:    #1
│ Amount:      500 NXS
│ Node Proof:  1be38a64af1bc4d2...
│ OPR Hash:    874b4a6c3fc4331c...
└─────────────────────────────────

┌── OP_RETURN Layer / Protocol ──
│ Magic:       NXS
│ Version:     1
│ Mint Seq:    #1
│ Wit Hash:    91c34342219faab3...
│ Proof Hash:  1be38a64af1bc4d2...
└─────────────────────────────────
```

Both layers cross-reference each other. Both contain the same full node proof hash. Verifiable by anyone running a Bitcoin full node.

---

## FAQ

**Q: Why require a full node to mint?**
The barrier IS the value. Bitcoin was meant to be run by node operators, not website clickers. If you're not willing to dedicate 600GB to Bitcoin, you're not the target audience.

**Q: Can someone build a web minter?**
No. The full node proof requires reading raw bytes from local `blk*.dat` files at random offsets determined by the latest block hash. No public API provides this data in the required format within the 15-second window.

**Q: Is there a premine or team allocation?**
No. Zero premine. The protocol fee address receives 5,000 sats per mint — that's it. All 21,000,000 NXS are distributed through fair minting.

**Q: What happens after all 42,000 mints are done?**
Minting ends permanently. NXS can only be transferred, never created again.

**Q: Which wallets support NEXUS?**
The NEXUS Reactor handles minting. Transfer support will follow as Indexer infrastructure matures.

---

## Links

- **GitHub**: [github.com/btcnexus/nexus-protocol](https://github.com/btcnexus/nexus-protocol)
- **Protocol Spec**: [`docs/PROTOCOL.md`](docs/PROTOCOL.md)
- **Security Audit**: [`docs/SECURITY_AUDIT.md`](docs/SECURITY_AUDIT.md)

---

## License

MIT

---

*NEXUS — If you don't run a node, you don't mint.*


# NEXUS Protocol Specification v2.0

## 代币参数

- 名称: NEXUS (NXS)
- 总量: 21,000,000
- 精度: 8位小数
- 每笔铸造量: 500 NXS（固定，不递减）
- 总铸造笔数: 42,000笔
- 铸造费: 5,000 sats/笔
- 项目方总收入: 42,000 × 5,000 = 2.1 BTC

## 铸造规则

极简：构造合法的NEXUS铸造交易 → 广播 → 被任意区块确认 → 铸造成功。
无区块上限、无地址冷却、无时间窗口、无减半。
先到先得，确认即生效。铸完42,000笔即结束。

唯一门槛：必须运行BTC Full Archive Node + NEXUS Reactor软件。

## 铸造交易结构

```
Bitcoin Transaction
│
├── INPUT[0]:
│   └── witness:
│       └── INSCRIPTION ENVELOPE:
│           OP_FALSE OP_IF
│             OP_PUSH "nexus"
│             OP_PUSH "application/nexus-mint"
│             OP_PUSH <witness_payload_json>   ← 含OP_RETURN的hash
│           OP_ENDIF
│           <signature> <pubkey>
│
├── OUTPUT[0]: 铸造者Taproot地址 (546 sats)
├── OUTPUT[1]: 项目方收费地址 (5,000 sats)
├── OUTPUT[2]: OP_RETURN
│   └── "NXS" | version | mint_seq | witness_hash | full_node_proof
│
└── nLockTime: 0（无窗口限制）
```

## 双层互锁

- witness_payload.opr = SHA256(OP_RETURN完整数据)
- OP_RETURN.witness_hash = SHA256(witness_payload_json)
- 两层互相引用 → 任何单一工具无法构造

## 全节点证明

两轮挑战，15秒时间窗口：
- Round 1: 基于最新区块hash派生10个随机历史区块 → 从本地blk*.dat读取切片
- Round 2: 基于Round 1结果派生另外10个区块 → 再读取切片
- combined_proof = SHA256(round1 || round2)

验证点：
- 直接读取 ~/.bitcoin/blocks/blk*.dat（不走RPC）
- 验证blk文件总大小 > 500GB
- 验证早期blk00000.dat存在且有效
- 两轮必须15秒内完成（本地SSD ~100ms，API ~5-15s超时）

## Indexer规则

铸造有效条件（仅6条）：
1. Witness铭文含 "nexus" 协议标识且格式正确
2. OP_RETURN以 "NXS" 开头且格式正确
3. 双层互锁hash验证通过
4. 全节点证明验证通过
5. 铸造费5,000 sats正确发送到项目方地址
6. 总铸造量未超过21,000,000 (即mint_seq <= 42,000)

序号分配：按区块确认顺序+区块内交易位置排序，先确认先得。

## 转账

OP_RETURN格式：
"NXS" | 0x02 | <from> | <to> | <amount>
无需全节点证明。
