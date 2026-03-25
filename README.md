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
  [4]  Mainnet Mint (Single / Batch)
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
- ~20-1,000 sats → miner fee (at 0.1-1 sat/vB)
- Remainder → returned as change

### Step 4 — Mint

Select `[4]` Mainnet Mint, then choose:
- **[1] Single mint** — one mint of 500 NXS
- **[2] Batch mint** — mint multiple in one session (see Batch Minting below)

For single mint:
1. Select wallet (auto-detected from Bitcoin Core)
2. Enter the wallet number (e.g. enter 1 for an already created wallet)
3. Set fee rate (minimum 0.1 sat/vB)
4. Confirm and broadcast

The Reactor handles everything: UTXO safety scan → node verification → proof generation → dual-layer interlock → commit + reveal broadcast → UTXO record update.

---

## UTXO Safety

The Reactor protects your assets with a **five-layer UTXO classification** system. Before selecting any UTXO as a transaction input, each one is checked:

| Layer | Check | Result |
|-------|-------|--------|
| 1 | In `nxs_mints.json` (our minted tokens) | Locked — never spend |
| 2 | Amount ≤ 546 sats | Locked — likely carries inscription/Rune |
| 3 | In `nxs_locked.json` (detected protocol assets) | Locked |
| 4 | In `nxs_change.json` (our Commit TX change) | Safe to spend |
| 5 | Amount > 1,000 sats | Safe to spend |
| — | 547–1,000 sats | Gray zone — default locked |

The Reactor also supports **multi-UTXO merge** — when no single UTXO is large enough, multiple safe UTXOs are combined as Commit TX inputs (up to 10). Change outputs are always produced when possible, eliminating wasted sats.

Before every mint, a **pre-check** displays your UTXO pool status:
```
── UTXO Pool Status ──
Total UTXOs:    8
Spendable:      2 (15,000 sats)
Locked:         5 (1,650 sats)
Gray zone:      1 (800 sats)
Need:           1,308 sats
Status:         ✅ Balance sufficient
```

---

## Batch Minting

Mint multiple NXS tokens in a single session:

1. Select `[4]` → `[2]` Batch mint
2. Enter fee rate
3. Reactor scans UTXOs, calculates maximum mintable count
4. Enter how many to mint (1–N)
5. Reactor executes N mints sequentially — each with a unique proof

Each mint in a batch uses a **different block height** for proof generation (`block_height`, `block_height-1`, `block_height-2`, ...), producing completely independent proofs. Change from each Commit TX feeds directly into the next — no waiting for confirmations.

```
Mint #1 [block 942022]: 15,000 → 726 commit + 14,077 change
Mint #2 [block 942021]: 14,077 → 726 commit + 13,154 change
Mint #3 [block 942020]: 13,154 → 726 commit + 12,231 change
```

If any broadcast fails mid-batch, already-completed mints are preserved. The Reactor saves all UTXO records and you can restart to continue.

---

## Testnet (regtest)

Try the full mint cycle in 10 minutes without real BTC:

1. Build with regtest flag: `cargo build --release --features regtest`
2. Select `[1]` → `[3]` to start local regtest node (200 blocks, 5000 BTC)
3. Select `[3]` to mint — fully automated with instant block confirmation

---

## Web Dashboard

The protocol dashboard is live at **[bitcoinexus.xyz](https://bitcoinexus.xyz)** with:

- **Real-time minting progress** — progress bar, percentage, remaining mints
- **Holder leaderboard** — top holders ranked by NXS balance
- **Recent mint feed** — latest minting activity with transaction links
- **Address / TX lookup** — search by Bitcoin address or transaction hash
- **Wallet connect** — UniSat, OKX Wallet, Xverse support
- **Bilingual** — English / Chinese toggle

The frontend is for **viewing only** — minting still requires a full node and the NEXUS Reactor CLI.

### API Endpoints

The Indexer (`src/bin/indexer.rs`) serves a REST API at `api.bitcoinexus.xyz`:

| Endpoint | Description |
|----------|-------------|
| `GET /api/status` | Protocol status (supply, minted, holders, scan height) |
| `GET /api/balance/{address}` | NXS balance for an address |
| `GET /api/mint/{seq}` | Lookup mint by sequence number |
| `GET /api/mints?page=1&limit=20` | Paginated mint list |
| `GET /api/mints/recent` | Latest 20 mints (newest first) |
| `GET /api/mints/address/{address}` | All mints for an address + balance |
| `GET /api/holders` | Top 100 holders ranked by balance |
| `GET /api/tx/{txid}` | Lookup mint by reveal txid |
| `GET /api/mint/tx/{txid}` | Lookup mint by txid (frontend format) |
| `GET /api/health` | Service health check |

---

## Architecture

```
nexus-protocol/
├── src/
│   ├── main.rs          # Reactor CLI — interactive menu + single/batch mint engine
│   ├── lib.rs           # Module exports
│   ├── constants.rs     # Protocol parameters (mainnet/regtest via feature flag)
│   ├── proof.rs         # Full node proof + Bitcoin Core 30.x obfuscation support
│   ├── transaction.rs   # Dual-layer interlock + pk identity binding
│   ├── indexer.rs       # Transaction validation engine (7 rules + DoS prefilter)
│   ├── utxo.rs          # UTXO safety classification + selection + record tracking
│   ├── node_detect.rs   # Auto-detect Bitcoin node + path management
│   ├── ui.rs            # Terminal UI with color
│   └── bin/
│       └── indexer.rs   # Indexer HTTP service (actix-web, 10 API endpoints + CORS)
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

### Single Mint

```
[1] Verify Node     Auto-detect datadir → blk*.dat > 500GB → XOR decrypt magic ✓
         │
[2] Generate Proof  Two-round challenge → 20 random blocks → 15s window
         │
[3] Build Interlock Witness JSON (with pk) ←SHA256→ OP_RETURN (ASCII)
         │
[4] UTXO Select     Load records → 5-layer classify → select + merge inputs
         │
[5] Commit TX       BTC → Taproot address with inscription script tree + change
         │
[6] Reveal TX       Script-path spend → inscription + OP_RETURN + fee
         │
[7] Record UTXOs    Reveal output[0] → nxs_mints.json | Change → nxs_change.json
         │
[8] Confirmed       Block inclusion → Indexer validates 7 rules → 500 NXS credited
```

### Batch Mint

```
[1] Verify Node + Scan UTXOs → Calculate max mintable
         │
[2] User selects count (1-N)
         │
    ┌────┴────────────────────────────────────────────┐
    │  FOR each mint:                                 │
    │    Get block hash at (latest_height - i)        │
    │    Generate unique proof → Build interlock       │
    │    Commit TX (input = previous change) → Reveal  │
    │    Record mint + change                          │
    │    If broadcast fails → stop, save, report       │
    └────┬────────────────────────────────────────────┘
         │
[3] Display summary (N mints, total NXS, remaining balance)
```

---

## Indexer Validation Rules

A mint is valid if and only if **all rules** pass (ordered by cost — cheap checks first to prevent DoS):

1. **Format**: Witness inscription contains `"nexus"` protocol identifier and valid JSON with required fields (`p`, `op`, `amt`, `pk`, `fnp`, `opr`)
2. **OP\_RETURN**: Starts with `NXS:` prefix, correct ASCII format (`NXS:MINT:500:w=<16hex>:p=<16hex>`)
3. **Fee**: Exactly 5,000 sats sent to the protocol fee address (checked early to reject spam)
4. **Interlock**: Dual-layer hashes match — `SHA256(OP_RETURN) == witness.opr` and `SHA256(witness_without_opr)[..8] == OP_RETURN.w`
5. **Identity**: `pk` field in JSON must match the Taproot x-only public key that signed the transaction (prevents identity spoofing)
6. **Proof**: Full node proof passes two-round verification with precheck (heights count, time window, field lengths) + replay protection via used-proof table
7. **Supply**: Total mints ≤ 42,000 (supply cap not exceeded)

Sequence numbers are **assigned by the Indexer** based on transaction position within each block. First confirmed, first served.

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
| DoS (spam invalid proofs) | Cheap checks first (fee, format, interlock) before expensive proof verification |
| Unlimited mint | Fixed `amt=500`, supply cap enforced, proof uniqueness |
| Asset-bearing UTXO burn | Five-layer UTXO classification; 330/546 sats outputs locked by default |
| Batch proof collision | Each batch mint uses a different block height → unique proof hash |

---

## On-Chain Verification

Every NEXUS mint is permanently visible on any block explorer:

**OP\_RETURN (human-readable):**
```
NXS:MINT:500:w=b8a4cee75bc2a205:p=a14075ce74aabea5
```

**Witness inscription (JSON):**
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

Both layers cross-reference each other. The `pk` field binds the mint to the signing key.

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

UTXO tracking files (auto-generated at runtime):

| File | Purpose |
|------|---------|
| `nxs_mints.json` | Locked token UTXOs (Reveal output[0]) |
| `nxs_change.json` | Reusable change UTXOs (Commit change output) |
| `nxs_locked.json` | Detected external protocol asset UTXOs |

---

## FAQ

**Q: Why require a full node to mint?**
The barrier IS the value. Bitcoin was meant to be run by node operators, not website clickers. If you're not willing to dedicate 850GB to Bitcoin, you're not the target audience.

**Q: Can someone build a web minter?**
No. The full node proof requires reading raw bytes from local `blk*.dat` files at random offsets determined by the latest block hash. No public API provides this data in the required format within the 15-second window.

**Q: Can someone fake a mint?**
No. The Indexer validates 7 rules including dual-layer hash interlock, identity binding (pk must match signing key), proof uniqueness, and fee payment. Forging any single element breaks the chain.

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

**Q: How does batch minting work?**
The Reactor can mint multiple NXS tokens in one session. Each mint uses a different block height for proof generation, ensuring unique proofs. Change from each Commit TX feeds into the next, enabling chain-linked minting without waiting for confirmations.

**Q: Will batch minting burn my inscriptions or Runes?**
No. The Reactor classifies every UTXO before use. Outputs ≤ 546 sats and known protocol-bound UTXOs are automatically locked and never selected as inputs.

**Q: Is there a web frontend?**
Yes. The protocol dashboard is live at [bitcoinexus.xyz](https://bitcoinexus.xyz) with real-time minting progress, holder leaderboard, recent mint feed, address/transaction lookup, and wallet connection support (UniSat, OKX Wallet, Xverse). The frontend is for viewing only — minting still requires a full node and the NEXUS Reactor CLI.

---

## Links

- **Website**: [bitcoinexus.xyz](https://bitcoinexus.xyz)
- **GitHub**: [github.com/btcnexus/nexus-protocol](https://github.com/btcnexus/nexus-protocol)
- **API**: [api.bitcoinexus.xyz/api/status](https://api.bitcoinexus.xyz/api/status)
- **Protocol Spec**: [`docs/PROTOCOL.md`](docs/PROTOCOL.md)

---

## License

MIT

---

*NEXUS — If you don't run a node, you don't mint.*
