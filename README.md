# NEXUS Protocol

**[English](README.md)** | **[中文](README_CN.md)**

### The first dual-layer interlocking token on Bitcoin L1.

Every mint transaction simultaneously writes to **two data layers** — Witness (inscription) and OP\_RETURN — cryptographically bound to each other. You cannot mint with a website. You cannot mint with an API. You must run a **full Bitcoin archive node (~850GB)** and the NEXUS Reactor software.

---

## Why NEXUS Exists

Every Bitcoin token protocol so far has used **one** data layer:

| Protocol | Data Layer | Anyone can mint? |
|----------|-----------|-----------------|
| BRC-20 | Witness (inscription) | Yes, via website |
| Runes | OP\_RETURN | Yes, via website |
| Ordinals | Witness (inscription) | Yes, via website |
| **NEXUS** | **Witness + OP\_RETURN, interlocked** | **No. Full node only.** |

NEXUS is the first protocol that requires **both layers simultaneously**, with each layer containing the SHA-256 hash of the other. No existing tool — ord, rune cli, or any web minter — can construct this transaction. Only the NEXUS Reactor can.

---

## How It Works

```
┌─────────────────────────────────────────────────────┐
│                 NEXUS Mint Transaction              │
│                                                     │
│  WITNESS LAYER (Inscription)                        │
│  ┌───────────────────────────────────────┐          │
│  │ protocol:    "nexus"                  │          │
│  │ operation:   "mint"                   │          │
│  │ amount:      500 NXS                  │          │
│  │ node_proof:  <full node proof hash>   │          │
│  │ opr_hash:    SHA256(OP_RETURN data) ──┼──┐       │
│  └───────────────────────────────────────┘  │       │
│                                             │       │
│  OP_RETURN LAYER (Protocol)                 │       │
│  ┌───────────────────────────────────────┐  │       │
│  │ magic:       "NXS"                    │  │       │
│  │ version:     1                        │  │       │
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

The Reactor also verifies your `blocks/` directory:
- Total `blk*.dat` size > 500 GB
- At least 3,000 block files
- Early files (`blk00000.dat` through `blk00009.dat`) present (pruned nodes delete these)
- Valid mainnet magic bytes (supports Bitcoin Core 30.x XOR obfuscation)

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
| **Min Fee Rate** | 0.1 sat/vB |
| **Requirement** | BTC Full Archive Node + NEXUS Reactor |
| **Fair Launch** | No premine. No team allocation. FCFS. |

---

## Quick Start

### Prerequisites

- **OS**: Ubuntu 22.04+ / Debian 12+
- **Bitcoin Core**: 28.0+ (full archive, NOT pruned), supports 30.x with obfuscation
- **Disk**: ~850 GB SSD for blockchain data
- **Rust**: 1.70+ (`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`)
- **Python**: 3.10+ with pip
- **Python packages**: `bip_utils`, `base58`

### Install Dependencies

```bash
# Rust (if not installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env

# System packages
sudo apt update
sudo apt install -y build-essential pkg-config libssl-dev python3 python3-pip

# Python packages
pip install bip_utils base58 --break-system-packages -i https://pypi.org/simple/
```

### Build & Run

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

### Interactive Menu

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
  [6]  Create Wallet                

  [0]  Exit
```

---

## Minting Guide

### Step 1 — Install & Sync Full Node

Select `[1]` from the menu. The Reactor will:
- Auto-detect if Bitcoin Core is installed
- Let you choose data directory (`~/.bitcoin`, `/data/bitcoin`, or custom path)
- Auto-configure `dbcache` based on your system RAM
- Start `bitcoind` with optimized sync parameters

The Reactor **automatically detects existing nodes** on your server — checks running processes, common paths (`~/.bitcoin`, `/data/bitcoin`, etc.), and saved config.

### Step 2 — Create a Wallet

Select `[6]` to create a new wallet. Choose address type:

| Type | Prefix | Standard | Recommended |
|------|--------|----------|-------------|
| **Taproot** | `bc1p...` | BIP86 (P2TR) | ✅ Yes |
| **Native SegWit** | `bc1q...` | BIP84 (P2WPKH) | Good |
| **Nested SegWit** | `3...` | BIP49 (P2SH-P2WPKH) | Legacy |

The wallet generates:
- **12-word BIP39 mnemonic** (compatible with UniSat, OKX, Sparrow, etc.)
- **WIF private key** for each address type
- Auto-imports into Bitcoin Core for balance tracking

### Step 3 — Fund Your Address

Send at least **10,000 sats** to your Taproot address:
- 5,000 sats → protocol mint fee
- ~1,000 sats → miner fee (at 0.1-1 sat/vB)
- Remainder → returned as change

### Step 4 — Mint

Select `[4]` Mainnet Mint:
1. Select wallet (auto-detected from Bitcoin Core)
2. Enter the wallet number (for example, enter 1 for an already created wallet).
3. Set fee rate (minimum 0.1 sat/vB)
4. Confirm and broadcast

The Reactor handles everything: node verification → proof generation → dual-layer interlock → commit + reveal broadcast.

---

## Testnet (regtest)

Try the full mint cycle in 10 minutes without real BTC:

1. Build with regtest flag: `cargo build --release --features regtest`
2. Select `[1]` → `[3]` to start local regtest node (200 blocks, 5000 BTC)
3. Select `[3]` to mint — fully automated with instant block confirmation

---

## Architecture

```
nexus-protocol/
├── src/
│   ├── main.rs          # Reactor CLI — interactive menu + mint engine
│   ├── lib.rs           # Module exports
│   ├── constants.rs     # Protocol parameters (mainnet/regtest via feature flag)
│   ├── proof.rs         # Full node proof + Bitcoin Core 30.x obfuscation support
│   ├── transaction.rs   # Dual-layer interlock construction + verification
│   ├── indexer.rs       # Transaction validation engine (6 rules)
│   ├── node_detect.rs   # Auto-detect Bitcoin node + path management
│   └── ui.rs            # Terminal UI with color
├── scripts/
│   └── wallet_gen.py    # BIP39/86/84/49 wallet generator (bip_utils)
├── docs/
│   └── PROTOCOL.md      # Complete protocol specification
├── Cargo.toml
├── README.md            # English
└── README_CN.md         # 中文
```

---

## Mint Transaction Flow

```
[1] Verify Node     Auto-detect datadir → blk*.dat > 500GB → XOR decrypt magic ✓
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
2. OP\_RETURN starts with `"NXS"` magic bytes with correct binary format (68 bytes)
3. Dual-layer interlock hashes match (cross-verified)
4. Full node proof passes two-round verification (replay protection via used-proof table)
5. Exactly 5,000 sats sent to the protocol fee address
6. Total mints ≤ 42,000 (supply cap not exceeded)

Sequence numbers are **assigned by the Indexer** based on transaction position within each block. Users do not choose their own sequence — first confirmed, first served.

---

## Security

| Attack Vector | Defense |
|--------------|---------|
| API relay (no full node) | Two-round 15s window. Local ~100ms vs API ~5-15s |
| Pruned node disguise | Direct disk read: blk files > 500GB + early files exist |
| Shared Reactor proxy | Proof bound to minter's public key |
| Proof replay | Used-proof deduplication in Indexer |
| Interlock tampering | Bidirectional SHA-256 hash verification |
| Mint ordering | Indexer assigns sequence by tx position in block — FCFS |
| Bitcoin Core 30.x encryption | Auto-detect obfuscation key, XOR decrypt blk files |

---

## On-Chain Verification

Every NEXUS mint is permanently visible on-chain with two layers of data:

```
┌── Witness Layer / Inscription ──
│ Protocol:    nexus
│ Operation:   mint
│ Amount:      500 NXS
│ Node Proof:  4805dd3c7b566ea7...
│ OPR Hash:    49268182887e3829...
└─────────────────────────────────

┌── OP_RETURN Layer / Protocol ──
│ Magic:       NXS
│ Version:     1
│ Wit Hash:    6b51e93a6e32e477...
│ Proof Hash:  4805dd3c7b566ea7...
└─────────────────────────────────
```

Both layers cross-reference each other. Both contain the same full node proof hash.

---

## Configuration

The Reactor saves settings to `nexus_config.json`:

```json
{
  "bitcoin_datadir": "/root/.bitcoin",
  "rpc_url": "http://127.0.0.1:8332",
  "rpc_user": "nexus",
  "rpc_pass": "your_password",
  "network": "main"
}
```

Edit this file to configure RPC credentials, data directory, etc.

---

## FAQ

**Q: Why require a full node to mint?**
The barrier IS the value. Bitcoin was meant to be run by node operators, not website clickers. If you're not willing to dedicate 850GB to Bitcoin, you're not the target audience.

**Q: Can someone build a web minter?**
No. The full node proof requires reading raw bytes from local `blk*.dat` files at random offsets determined by the latest block hash. No public API provides this data in the required format within the 15-second window.

**Q: Is there a premine or team allocation?**
No. Zero premine. The protocol fee address receives 5,000 sats per mint — that's it. All 21,000,000 NXS are distributed through fair minting.

**Q: What happens after all 42,000 mints are done?**
Minting ends permanently. NXS can only be transferred, never created again.

**Q: Which wallets support NEXUS?**
The Reactor generates BIP39-standard wallets compatible with UniSat, OKX Wallet, Sparrow, and any BIP86-compliant wallet. Transfer support follows as Indexer infrastructure matures.

**Q: Does it work with Bitcoin Core 30.x?**
Yes. The Reactor auto-detects the XOR obfuscation key introduced in Bitcoin Core 30.0 and decrypts `blk*.dat` files transparently.

**Q: What is the minimum fee rate?**
0.1 sat/vB. You can set any fee rate when minting.

---

## Links

- **GitHub**: [github.com/btcnexus/nexus-protocol](https://github.com/btcnexus/nexus-protocol)
- **Protocol Spec**: [`docs/PROTOCOL.md`](docs/PROTOCOL.md)

---

## License

MIT

---

*NEXUS — If you don't run a node, you don't mint.*
