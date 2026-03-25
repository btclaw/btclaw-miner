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

## Three On-Chain Operations

NEXUS defines three operations, each using the appropriate data layer(s):

| Operation | Data Layer | Full Node Required? | Purpose |
|-----------|-----------|-------------------|---------|
| **Deploy** | Witness + OP\_RETURN | Yes (deployer only) | Genesis inscription — defines token parameters |
| **Mint** | Witness + OP\_RETURN (interlocked) | Yes | Dual-layer interlock + full node proof |
| **Transfer** | OP\_RETURN only | No | Lightweight transfer — Taproot signature = ownership |

Mint requires the Witness layer to embed the full node proof. Transfer only needs OP\_RETURN because the Taproot signature already proves ownership — no need to re-prove node status.

---

## How It Works

```
┌─────────────────────────────────────────────────────┐
│                NEXUS Mint Transaction               │
│                                                     │
│  WITNESS LAYER (Inscription JSON)                   │
│  ┌───────────────────────────────────────┐          │
│  │ p:       "nexus"                      │          │
│  │ op:      "mint"                       │          │
│  │ amt:     500                          │          │
│  │ pk:      <minter x-only pubkey>       │          │
│  │ fnp:     <full node proof hash>       │          │
│  │ opr:     SHA256(OP_RETURN data) ──────┼──┐       │
│  └───────────────────────────────────────┘  │       │
│                                             │       │
│  OP_RETURN LAYER (ASCII readable)           │       │
│  ┌───────────────────────────────────────┐  │       │
│  │ NXS:MINT:500:w=<wit_hash>:p=<proof_hash> │   │   │
│  │         ↑                             │  │       │
│  │   SHA256(Witness without opr) ────────┼──┘       │
│  └───────────────────────────────────────┘          │
│                                                     │
│  OUTPUT[0]: 330 sats → minter (token holder)        │
│  OUTPUT[1]: 5,000 sats → protocol fee               │
│  OUTPUT[2]: OP_RETURN (protocol data)               │
└─────────────────────────────────────────────────────┘
```

**The two layers reference each other's hash. Tamper with one, the other breaks. This is the interlock.**

### On-Chain Data Format

**Witness JSON** (embedded in Taproot inscription):
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

**OP\_RETURN** (human-readable on any block explorer):
```
NXS:MINT:500:w=b8a4cee75bc2a205:p=a14075ce74aabea5
```

---

## Transfer Protocol

Transfer uses **only the OP\_RETURN layer** — no Witness inscription, no full node required. The Taproot signature proves ownership.

```
NXS:TRANSFER:<amount>:to=<recipient_address>
```

Example: `NXS:TRANSFER:1000:to=bc1prh30dts9mn738...jy3t6z`

Transfer validation rules:
1. **Format** — OP\_RETURN starts with `NXS:TRANSFER`, valid amount, valid `to=` address
2. **Balance** — Sender has sufficient available NXS balance
3. **Signature** — Transaction signed by sender's Taproot key
4. **Confirmation** — 3 block confirmations required before balances update

The 3-block confirmation rule prevents double-spending: transferred NXS is locked on broadcast, and balances only update after 3 confirmations.

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

---

## Minting Guide

### Step 1 — Install & Sync Full Node

Select `[1]` from the menu. The Reactor will auto-detect Bitcoin Core, configure data directory, and start `bitcoind` with optimized sync parameters.

### Step 2 — Create a Wallet

Select `[6]` to create a new wallet. Choose Taproot (`bc1p...`, BIP86) for best compatibility. The wallet generates a 12-word BIP39 mnemonic compatible with UniSat, OKX, Sparrow, etc.

### Step 3 — Fund Your Address

Send at least **10,000 sats** to your Taproot address (5,000 fee + miner fee + dust).

### Step 4 — Mint

Select `[4]` Mainnet Mint → `[1]` Single mint or `[2]` Batch mint → set fee rate → confirm.

---

## UTXO Safety

Five-layer UTXO classification protects your assets:

| Layer | Check | Result |
|-------|-------|--------|
| 1 | In `nxs_mints.json` | Locked — never spend |
| 2 | Amount ≤ 546 sats | Locked — likely inscription/Rune |
| 3 | In `nxs_locked.json` | Locked |
| 4 | In `nxs_change.json` | Safe to spend |
| 5 | Amount > 1,000 sats | Safe to spend |

---

## Batch Minting

Each mint uses a different block height for proof generation, producing independent proofs. Change from each Commit TX feeds directly into the next — no waiting for confirmations. If any broadcast fails, already-completed mints are preserved.

---

## Testnet (regtest)

`cargo build --release --features regtest` → Select `[1]` → `[3]` → mint with instant blocks.

---

## Web Dashboard

Live at **[bitcoinexus.xyz](https://bitcoinexus.xyz)** — minting progress, holder leaderboard, recent mints, address lookup, wallet connect (UniSat/OKX/Xverse), bilingual EN/CN. Frontend is viewing only — minting requires a full node.

---

## Security

| Attack Vector | Defense |
|--------------|---------|
| API relay (no full node) | Two-round 15s window. Local ~100ms vs API ~5-15s |
| Pruned node disguise | Direct disk read: blk files > 500GB + early files exist |
| Identity spoofing | `pk` field bound to Taproot signing key + Indexer cross-check |
| Proof replay | Used-proof deduplication in Indexer |
| Interlock tampering | Bidirectional SHA-256 hash verification (pk participates in hash) |
| Mint ordering | Indexer assigns sequence by tx position in block — FCFS |
| Bitcoin Core 30.x encryption | Auto-detect obfuscation key, XOR decrypt blk files |
| DoS (spam invalid proofs) | Cheap checks first before expensive proof verification |
| Unlimited mint | Fixed `amt=500`, supply cap enforced, proof uniqueness |
| Asset-bearing UTXO burn | Five-layer UTXO classification; dust outputs locked by default |
| Batch proof collision | Each batch mint uses a different block height → unique proof |
| Transfer double-spend | 3-block confirmation rule; pending transfers lock sender's balance |
| Transfer insufficient balance | Indexer checks available\_balance = total - locked before accepting |

---

## FAQ

**Q: Why require a full node to mint?**
The barrier IS the value. If you're not willing to dedicate 850GB to Bitcoin, you're not the target audience.

**Q: Can someone build a web minter?**
No. The full node proof requires local `blk*.dat` reads within a 15-second window.

**Q: Is there a premine?**
No. Zero premine. All 21,000,000 NXS distributed through fair minting.

**Q: How does Transfer work without the Witness layer?**
Transfer only needs OP\_RETURN — the Taproot signature proves ownership. No full node required.

**Q: Why 3 block confirmations for transfers?**
Protects against blockchain reorganizations. Transferred NXS is locked on broadcast to prevent double-spending.

**Q: Can I transfer NXS from any wallet?**
Yes. Any Taproot-compatible wallet (UniSat, OKX, Xverse) can transfer through the web frontend.

---

## Links

- **Website**: [bitcoinexus.xyz](https://bitcoinexus.xyz)
- **GitHub**: [github.com/btcnexus/nexus-protocol](https://github.com/btcnexus/nexus-protocol)
- **API**: [api.bitcoinexus.xyz/api/status](https://api.bitcoinexus.xyz/api/status)
- **Protocol Spec**: [`docs/PROTOCOL.md`](docs/PROTOCOL.md)
- **Protocol Docs**: [bitcoinexus.xyz/protocol.html](https://bitcoinexus.xyz/protocol.html)

---

## License

MIT

---

*NEXUS — If you don't run a node, you don't mint.*
