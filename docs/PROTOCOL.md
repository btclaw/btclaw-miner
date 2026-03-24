# NEXUS Protocol Specification v2.9

### The first dual-layer interlocking token on Bitcoin L1.

Every mint transaction simultaneously writes to **two data layers** — Witness (inscription) and OP_RETURN — cryptographically bound to each other. You cannot mint with a website. You cannot mint with an API. You must run a **full Bitcoin archive node (~850 GB)** and the NEXUS Reactor software.

---

## 1. Design Rationale

Every Bitcoin token protocol so far has used **one** data layer:

| Protocol   | Data Layer               | Anyone can mint?       |
| ---------- | ------------------------ | ---------------------- |
| BRC-20     | Witness (inscription)    | Yes, via website       |
| Runes      | OP_RETURN                | Yes, via website       |
| Ordinals   | Witness (inscription)    | Yes, via website       |
| **NEXUS**  | **Witness + OP_RETURN, interlocked** | **No. Full node only.** |

NEXUS is the first protocol that requires **both layers simultaneously**, with each layer containing the SHA-256 hash of the other. No existing tool — ord, rune cli, or any web minter — can construct this transaction. Only the NEXUS Reactor can.

---

## 2. Token Parameters

| Parameter        | Value                                          |
| ---------------- | ---------------------------------------------- |
| **Name**         | NEXUS (NXS)                                    |
| **Total Supply** | 21,000,000                                     |
| **Per Mint**     | 500 NXS (fixed)                                |
| **Total Mints**  | 42,000                                         |
| **Mint Fee**     | 5,000 sats per mint                            |
| **Min Fee Rate** | 0.1 sat/vB                                     |
| **Requirement**  | BTC Full Archive Node + NEXUS Reactor          |
| **Fair Launch**  | No premine. No team allocation. FCFS.          |

---

## 3. Transaction Structure

A single NEXUS mint consists of two on-chain transactions: **Commit** and **Reveal**.

### 3.1 Overview

```
┌─────────────────────────────────────────────────────────┐
│                  NEXUS Mint Transaction                  │
│                                                         │
│  WITNESS LAYER (Inscription JSON)                       │
│  ┌─────────────────────────────────────────┐            │
│  │ p:       "nexus"                        │            │
│  │ op:      "mint"                         │            │
│  │ amt:     500                            │            │
│  │ pk:      <minter x-only pubkey>         │            │
│  │ fnp:     <full node proof hash>         │            │
│  │ opr:     SHA256(OP_RETURN data) ────────┼──┐         │
│  └─────────────────────────────────────────┘  │         │
│                                               │         │
│  OP_RETURN LAYER (ASCII readable)             │         │
│  ┌─────────────────────────────────────────┐  │         │
│  │ NXS:MINT:500:w=<wit_hash>:p=<proof_hash>  │         │
│  │              ↑                          │  │         │
│  │   SHA256(Witness without opr) ──────────┼──┘         │
│  └─────────────────────────────────────────┘            │
│                                                         │
│  OUTPUT[0]: 330 sats  → minter (token holder)           │
│  OUTPUT[1]: 5,000 sats → protocol fee address           │
│  OUTPUT[2]: OP_RETURN  (protocol data)                  │
└─────────────────────────────────────────────────────────┘
```

**The two layers reference each other's hash. Tamper with one, the other breaks. This is the interlock.**

### 3.2 Witness Layer — Inscription JSON

Embedded in a Taproot inscription (Ordinals-compatible envelope), the Witness layer carries the following JSON:

```json
{
  "p":   "nexus",
  "op":  "mint",
  "amt": 500,
  "pk":  "b4906faaf2724a59...",
  "fnp": "a14075ce74aabea5...",
  "opr": "02935680defa678f..."
}
```

| Field | Type   | Description                                                        |
| ----- | ------ | ------------------------------------------------------------------ |
| `p`   | string | Protocol identifier. Must be `"nexus"`.                            |
| `op`  | string | Operation. Must be `"mint"`.                                       |
| `amt` | int    | Amount. Must be `500`.                                             |
| `pk`  | string | Minter's x-only public key (64 hex chars). Bound to the Taproot signing key. |
| `fnp` | string | Full node proof hash (64 hex chars). Output of the two-round challenge. |
| `opr` | string | SHA-256 hash of the OP_RETURN payload (64 hex chars). Interlock anchor. |

### 3.3 OP_RETURN Layer — ASCII Readable

The OP_RETURN output carries a human-readable ASCII string:

```
NXS:MINT:500:w=b8a4cee75bc2a205:p=a14075ce74aabea5
```

| Segment         | Description                                                                 |
| --------------- | --------------------------------------------------------------------------- |
| `NXS`           | Protocol magic prefix.                                                      |
| `MINT`          | Operation type.                                                             |
| `500`           | Mint amount.                                                                |
| `w=<16 hex>`    | First 8 bytes (16 hex chars) of SHA-256(Witness JSON **without** `opr` field). |
| `p=<16 hex>`    | First 8 bytes (16 hex chars) of the full node proof hash.                   |

### 3.4 Interlock Mechanism

The dual-layer interlock is bidirectional:

1. **Witness → OP_RETURN**: The `opr` field in the inscription JSON equals `SHA256(OP_RETURN_payload)`.
2. **OP_RETURN → Witness**: The `w=` value in the OP_RETURN equals the first 8 bytes of `SHA256(Witness_JSON_without_opr)`.

The `pk` field participates in the Witness hash, meaning the minter's identity is cryptographically embedded in the interlock. Changing the public key breaks the hash chain.

### 3.5 Transaction Outputs

| Output   | Value       | Purpose                           |
| -------- | ----------- | --------------------------------- |
| `[0]`    | 330 sats    | Minter's token-holding UTXO       |
| `[1]`    | 5,000 sats  | Protocol fee to designated address |
| `[2]`    | 0 sats      | OP_RETURN data carrier             |

Fee address: `bc1p8d6a2pu8hdpk9tq3tt64ys2947e4hgn6j5msgqaycptj08xgvrpqqtd98h`

### 3.6 Commit + Reveal Flow

NEXUS uses the standard Ordinals two-phase inscription pattern:

1. **Commit TX**: Sends BTC to a Taproot address whose script tree contains the inscription envelope (JSON payload). This locks the data into the script but does not reveal it.
2. **Reveal TX**: Spends the Commit output via the script path, exposing the inscription on-chain. The Reveal TX also attaches the OP_RETURN output and the protocol fee output.

---

## 4. Full Node Proof

You don't just claim you run a full node. You **prove** it.

### 4.1 Two-Round Cryptographic Challenge

The Reactor generates a proof that can only be produced by a machine with direct local access to the full blockchain data:

1. **Round 1**: Minter's x-only public key + latest block hash → deterministically derives 10 random historical block heights → reads raw bytes directly from local `blk*.dat` files → extracts 32-byte slices at computed offsets → hashes everything into Round 1 proof.
2. **Round 2**: Round 1 proof hash → derives 10 **different** block heights (unpredictable until Round 1 completes) → same extraction process → produces Round 2 proof.
3. **Both rounds must complete within 15 seconds.**

Performance benchmarks:
- Local NVMe SSD: ~100 ms
- Remote API relay: ~5–15 seconds (likely timeout)

### 4.2 Disk Verification

Before generating the proof, the Reactor verifies the local `blocks/` directory:

| Check                        | Requirement                                                                |
| ---------------------------- | -------------------------------------------------------------------------- |
| Total `blk*.dat` size        | > 500 GB                                                                   |
| Block file count             | ≥ 3,000 files                                                              |
| Early files present          | `blk00000.dat` through `blk00009.dat` must exist (pruned nodes delete these) |
| Mainnet magic bytes          | Valid magic `0xF9BEB4D9` (supports Bitcoin Core 30.x XOR obfuscation)      |

### 4.3 Bitcoin Core 30.x XOR Obfuscation Support

Bitcoin Core 30.0+ introduced XOR obfuscation for `blk*.dat` files. The Reactor:

1. Auto-detects the presence of an obfuscation key in the LevelDB data directory.
2. Reads the XOR key.
3. Transparently decrypts block data before extracting proof bytes.

This is handled automatically — no user configuration required.

### 4.4 Node Auto-Detection

The Reactor (`node_detect.rs`) automatically locates an existing Bitcoin node:

- Checks running `bitcoind` processes.
- Scans common data paths: `~/.bitcoin`, `/data/bitcoin`, custom paths.
- Reads saved configuration from `nexus_config.json`.
- Verifies RPC connectivity.

---

## 5. Identity Binding

### 5.1 pk Field

The `pk` field in the inscription JSON stores the minter's **x-only public key** (the 32-byte key used in Taproot/BIP340 signatures).

### 5.2 Verification

The Indexer enforces that:

1. The `pk` value in the JSON must exactly match the x-only public key that produced the Schnorr signature for the Reveal transaction's Taproot input.
2. This prevents identity spoofing — you cannot inscribe someone else's public key to claim their mint.

### 5.3 Hash Participation

Because `pk` is part of the Witness JSON, it participates in the interlock hash computation. Changing the public key changes the Witness hash, which breaks the OP_RETURN cross-reference (`w=` value). The identity is cryptographically fused into the interlock.

---

## 6. Indexer

### 6.1 Validation Rules (7 Rules)

A mint is valid if and only if **all 7 rules** pass. Rules are ordered by computational cost — cheapest first — to reject invalid transactions early and prevent DoS attacks:

| #  | Rule           | Description                                                                                             |
| -- | -------------- | ------------------------------------------------------------------------------------------------------- |
| 1  | **Format**     | Witness inscription contains `"nexus"` protocol identifier and valid JSON with all required fields (`p`, `op`, `amt`, `pk`, `fnp`, `opr`). |
| 2  | **OP_RETURN**  | Starts with `NXS:` prefix. Correct ASCII format: `NXS:MINT:500:w=<16hex>:p=<16hex>`.                   |
| 3  | **Fee**        | Exactly 5,000 sats sent to the protocol fee address. Checked early to reject spam.                      |
| 4  | **Interlock**  | Dual-layer hashes match: `SHA256(OP_RETURN) == witness.opr` **AND** `SHA256(witness_without_opr)[..8] == OP_RETURN.w`. |
| 5  | **Identity**   | `pk` field in JSON must match the Taproot x-only public key that signed the transaction.                |
| 6  | **Proof**      | Full node proof passes two-round verification with precheck (heights count, time window, field lengths) + replay protection via used-proof table. |
| 7  | **Supply**     | Total mints ≤ 42,000 (supply cap not exceeded).                                                         |

### 6.2 DoS Prefilter Strategy

The rule ordering is deliberate:

- **Rules 1–3** (Format, OP_RETURN, Fee) are string/integer checks — near zero cost. They eliminate the vast majority of irrelevant transactions.
- **Rule 4** (Interlock) requires two SHA-256 computations — still cheap but filters out any malformed attempts.
- **Rule 5** (Identity) requires public key extraction and comparison.
- **Rule 6** (Proof) is the most expensive — two-round verification with disk reads. Only reached if all cheaper checks pass.
- **Rule 7** (Supply) is a simple counter check, placed last because it only matters if everything else is valid.

### 6.3 Sequence Assignment

Mint sequence numbers are assigned by the Indexer based on **transaction position within each block**. The ordering rule is: first confirmed in a block, first assigned. This is a strict FCFS (first come, first served) model.

### 6.4 Replay Protection

Each full node proof is unique (derived from the minter's public key + current block hash + random block data). The Indexer maintains a **used-proof table** to reject any proof that has been seen before.

### 6.5 HTTP API Service

The Indexer runs as a standalone HTTP service (`src/bin/indexer.rs`) built with **actix-web**, exposing 7 API endpoints for querying protocol state:

| Endpoint               | Description                            |
| ---------------------- | -------------------------------------- |
| `GET /status`          | Protocol status (total mints, supply remaining, block height) |
| `GET /mint/{txid}`     | Lookup a specific mint by transaction ID |
| `GET /mints`           | List recent mints (paginated)          |
| `GET /holder/{address}`| Query NXS balance for an address       |
| `GET /holders`         | Holder ranking (top holders)           |
| `GET /supply`          | Current circulating supply             |
| `GET /health`          | Service health check                   |

Production deployment uses Cloudflare Tunnel for IP protection. API endpoint will be announced at mainnet launch.

---

## 7. Wallet

### 7.1 Wallet Generation

The Reactor includes a built-in wallet generator (`scripts/wallet_gen.py` using `bip_utils`) that supports three address types:

| Type              | Prefix    | Standard           | Recommended |
| ----------------- | --------- | ------------------ | ----------- |
| **Taproot**       | `bc1p...` | BIP86 (P2TR)       | ✅ Yes      |
| **Native SegWit** | `bc1q...` | BIP84 (P2WPKH)     | Good        |
| **Nested SegWit** | `3...`    | BIP49 (P2SH-P2WPKH)| Legacy      |

### 7.2 Output

For each wallet, the generator produces:

- **12-word BIP39 mnemonic** (standard, compatible with UniSat, OKX Wallet, Sparrow, and any BIP86-compliant wallet).
- **WIF private key** for each address type.
- **Auto-import** into Bitcoin Core for balance tracking via `importdescriptors`.

### 7.3 Funding Requirement

A minter must fund their Taproot address with at least **10,000 sats**:

| Component     | Amount        |
| ------------- | ------------- |
| Protocol fee  | 5,000 sats    |
| Miner fee     | ~1,000 sats (at 0.1–1 sat/vB) |
| Dust output   | 330 sats      |
| Change        | Remainder     |

---

## 8. Mint Transaction Flow

```
[1] Detect Node     Auto-detect datadir → blk*.dat > 500GB → XOR decrypt magic ✓
         │
[2] Generate Proof  Two-round challenge → 20 random blocks → 15s window
         │
[3] Build Interlock Witness JSON (with pk) ←SHA256→ OP_RETURN (ASCII)
         │
[4] Commit TX       BTC → Taproot address with inscription script tree
         │
[5] Reveal TX       Script-path spend → inscription + OP_RETURN + fee
         │
[6] Confirmed       Block inclusion → Indexer validates 7 rules → 500 NXS credited
```

---

## 9. Security Model

| Attack Vector                | Defense                                                                          |
| ---------------------------- | -------------------------------------------------------------------------------- |
| API relay (no full node)     | Two-round 15s window. Local NVMe ~100ms vs remote API ~5–15s (timeout).          |
| Pruned node disguise         | Direct disk read: blk files > 500GB + early files (`blk00000–00009`) must exist. |
| Identity spoofing            | `pk` field bound to Taproot signing key + Indexer cross-check (Rule 5).          |
| Proof replay                 | Used-proof deduplication table in Indexer (Rule 6).                              |
| Interlock tampering          | Bidirectional SHA-256 verification; `pk` participates in hash (Rule 4).          |
| Mint ordering manipulation   | Indexer assigns sequence by tx position in block — strict FCFS.                  |
| Bitcoin Core 30.x encryption | Auto-detect XOR obfuscation key, transparent decrypt of blk files.               |
| DoS (spam invalid proofs)    | Cheap checks first (fee, format, interlock) before expensive proof verification. |
| Unlimited mint attempts      | Fixed `amt=500`, supply cap enforced at 42,000 mints, proof uniqueness.          |

---

## 10. Architecture

```
nexus-protocol/
├── src/
│   ├── main.rs          # Reactor CLI — interactive menu + mint engine
│   ├── lib.rs           # Module exports
│   ├── constants.rs     # Protocol parameters (mainnet/regtest via feature flag)
│   ├── proof.rs         # Full node proof + Bitcoin Core 30.x XOR obfuscation
│   ├── transaction.rs   # Dual-layer interlock + pk identity binding
│   ├── indexer.rs       # Transaction validation engine (7 rules + DoS prefilter)
│   ├── node_detect.rs   # Auto-detect Bitcoin node + path management
│   ├── ui.rs            # Terminal UI with color
│   └── bin/
│       └── indexer.rs   # Indexer HTTP service (actix-web, 7 API endpoints)
├── scripts/
│   └── wallet_gen.py    # BIP39/86/84/49 wallet generator (bip_utils)
├── docs/
│   └── PROTOCOL.md      # This file — complete protocol specification
├── Cargo.toml
├── README.md            # English
└── README_CN.md         # 中文
```

---

## 11. Configuration

The Reactor persists settings in `nexus_config.json`:

```json
{
  "bitcoin_datadir": "/root/.bitcoin",
  "rpc_url": "http://127.0.0.1:8332",
  "rpc_user": "nexus",
  "rpc_pass": "your_password",
  "network": "main"
}
```

---

## 12. Testnet (regtest)

The full mint cycle can be tested locally without real BTC:

1. Build with regtest flag: `cargo build --release --features regtest`
2. Select `[1]` → `[3]` from the Reactor menu to start a local regtest node (200 blocks, 5000 BTC).
3. Select `[3]` to mint — fully automated with instant block confirmation.

No 850 GB download. Instant blocks. Full protocol verification.

---

## 13. On-Chain Verification

Every NEXUS mint is permanently visible on any block explorer:

**OP_RETURN (human-readable):**
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

Both layers cross-reference each other. The `pk` field binds the mint to the signing key. Verifiable by anyone running a Bitcoin full node.

---

## 14. FAQ

**Q: Why require a full node to mint?**
The barrier IS the value. Bitcoin was meant to be run by node operators, not website clickers. If you're not willing to dedicate 850 GB to Bitcoin, you're not the target audience.

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

---

## Links

- **GitHub**: [github.com/btcnexus/nexus-protocol](https://github.com/btcnexus/nexus-protocol)
- **API**: *(To be announced at mainnet launch)*

---

## License

MIT

---

*NEXUS — If you don't run a node, you don't mint.*
