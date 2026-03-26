# NEXUS Protocol Specification v3.0

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

NEXUS defines three on-chain operations, each using the appropriate data layer(s):

| Operation    | Data Layer                        | Full Node Required? | Reason                     |
| ------------ | --------------------------------- | ------------------- | -------------------------- |
| **Deploy**   | Witness + OP_RETURN               | Yes (deployer only) | Genesis inscription        |
| **Mint**     | Witness + OP_RETURN (interlocked) | Yes                 | Full node proof in Witness |
| **Transfer** | OP_RETURN only                    | No                  | Signature = ownership      |

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

### 3.1 Overview

A single NEXUS mint consists of two on-chain transactions: **Commit** and **Reveal**.

```
┌─────────────────────────────────────────────────────────┐
│                 NEXUS Mint Transaction                  │
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
│  │ NXS:MINT:500:w=<wit_hash>:p=<proof_hash>   │         │
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

1. **Commit TX**: Sends BTC to a Taproot address whose script tree contains the inscription envelope (JSON payload). Supports multiple UTXO inputs with automatic change output.
2. **Reveal TX**: Spends the Commit output via the script path, exposing the inscription on-chain. The Reveal TX also attaches the OP_RETURN output and the protocol fee output.

### 3.7 Deploy Transaction

The Deploy transaction creates the NEXUS protocol on Bitcoin. It is executed exactly once and defines all token parameters permanently on-chain. No further deploys are possible.

#### Deploy Witness Layer

Content-Type: `application/nexus-deploy`

```json
{
  "p": "nexus",
  "op": "deploy",
  "tick": "NXS",
  "max": 21000000,
  "lim": 500,
  "total_mints": 42000,
  "fee": 5000,
  "pk": "d2275bb54312700c0a0453e43b7ffde25871d898097c03539128c258604259ed",
  "opr": "38687f4a3ea51169ef8ab2139f3131d649b6bfb8c86761832..."
}
```

| Field         | Type   | Description                                    |
| ------------- | ------ | ---------------------------------------------- |
| `p`           | string | Protocol identifier. Must be `"nexus"`.        |
| `op`          | string | Operation. Must be `"deploy"`.                 |
| `tick`        | string | Token ticker symbol. `"NXS"`.                  |
| `max`         | int    | Maximum total supply: 21,000,000.              |
| `lim`         | int    | Tokens per mint: 500.                          |
| `total_mints` | int    | Maximum number of mints: 42,000.               |
| `fee`         | int    | Protocol fee per mint in sats: 5,000.          |
| `pk`          | string | Deployer's x-only public key (64 hex chars).   |
| `opr`         | string | SHA-256 hash of the OP_RETURN payload.         |

#### Deploy OP_RETURN Layer

```
NXS:DEPLOY:NXS:max=21000000:lim=500:fee=5000
```

Human-readable ASCII string defining all protocol parameters.

#### Deploy Transaction Outputs

| Output   | Value    | Purpose                          |
| -------- | -------- | -------------------------------- |
| `[0]`    | 330 sats | Deployer's address               |
| `[1]`    | 0 sats   | OP_RETURN (protocol parameters)  |

#### On-Chain Reference

| Transaction | TXID |
| ----------- | ---- |
| Commit TX   | `c72a693c52db9764d94167876ee5a9889b30f5e5cd183e9d03b96add5136f7fa` |
| Reveal TX   | `450ae05b1e066a51a9fa3ce17b4781442eb90e367fcf5ba7e9753c7ecb465124` |
| Deployer    | `bc1prup3j0l8p832kcxx02cvjj52s6etu20gvk0ppd4f5zsqed4kjawsfx800y` |
| Deployer PK | `d2275bb54312700c0a0453e43b7ffde25871d898097c03539128c258604259ed` |

The Deploy transaction follows the same Commit + Reveal pattern as Mint (§3.6), but uses `application/nexus-deploy` as the inscription content type instead of `application/nexus-mint`.

---

## 4. Full Node Proof

You don't just claim you run a full node. You **prove** it.

### 4.1 Two-Round Cryptographic Challenge

The Reactor generates a proof that can only be produced by a machine with direct local access to the full blockchain data:

1. **Round 1**: Minter's x-only public key + block hash → deterministically derives 10 random historical block heights → reads raw bytes directly from local `blk*.dat` files → extracts 32-byte slices at computed offsets → hashes everything into Round 1 proof.
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

### 4.5 Proof Differentiation for Batch Minting

The proof seed is derived from `SHA256(block_hash + pubkey + domain)`. Since the same pubkey + same block_hash produces the same proof, batch minting uses a different block height for each mint in the batch:

```
Mint #1: block_height      (latest)     → unique seed → unique proof
Mint #2: block_height - 1  (previous)   → unique seed → unique proof
Mint #3: block_height - 2               → unique seed → unique proof
```

Different block hashes produce entirely different challenge heights and proof outputs. The Indexer's used-proof table sees each as a distinct entry — no replay conflict.

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

## 6. UTXO Safety Management

### 6.1 Problem

Bitcoin wallets may contain UTXOs that carry protocol assets — inscriptions, Runes, BRC-20 tokens — typically bound to 330 or 546 satoshi outputs. If the Reactor blindly selects these as inputs for a Commit TX, the associated assets are permanently destroyed.

### 6.2 Five-Layer Classification

Every UTXO in the minter's wallet is classified before it can be used as a Commit TX input. The classification runs in order; the first matching rule determines the result:

| Layer | Check                          | Result            | Rationale                                        |
| ----- | ------------------------------ | ----------------- | ------------------------------------------------ |
| 1     | `txid:vout` in `nxs_mints.json` | Locked (NXS mint) | Our own minted token — never spend               |
| 2     | Amount ≤ 546 sats              | Locked (dust)     | Almost certainly carries a protocol asset         |
| 3     | `txid:vout` in `nxs_locked.json` | Locked (external) | Previously detected inscription/Rune/protocol data |
| 4     | `txid:vout` in `nxs_change.json` | Spendable (known change) | Our own Commit TX change — safe to reuse |
| 5     | Amount > 1,000 sats            | Spendable         | Large enough to be plain BTC with high confidence |
| —     | 547–1,000 sats                 | Gray zone         | Default locked; user can manually unlock          |

### 6.3 Local Record Files

The Reactor maintains three JSON files in the working directory:

| File               | Contents                                                  | Written when                     |
| ------------------ | --------------------------------------------------------- | -------------------------------- |
| `nxs_mints.json`   | Reveal TX `output[0]` (330 sats token UTXOs)              | After each successful Reveal broadcast |
| `nxs_change.json`  | Commit TX change outputs                                  | After each successful Commit broadcast |
| `nxs_locked.json`  | UTXOs detected as carrying external protocol assets       | During source-TX analysis        |

These files are loaded at mint time, updated after broadcast, and saved atomically. If the Reactor crashes mid-mint, the next run reconciles by comparing local records against `listunspent` results.

### 6.4 Multi-UTXO Merge

When no single UTXO is large enough for a Commit TX, the Reactor merges multiple UTXOs as inputs:

1. Priority: known change UTXOs first, then large spendable UTXOs.
2. Sorted by amount descending — prefer fewer, larger inputs.
3. Maximum 10 inputs per Commit TX (avoids oversized transactions).
4. Each additional input adds ~68 vB to the Commit TX; fees are recalculated accordingly.
5. All inputs are signed with Taproot key-path Schnorr signatures.

### 6.5 Change Output Optimization

The Commit TX always attempts to produce a change output:

- Change = total input − commit output value − miner fee.
- If change > 546 sats (dust limit): a change output is added, returning funds to the minter's address.
- If change ≤ 546 sats: no change output (excess becomes miner fee).

This eliminates the previous behavior where small-input Commit TXs donated the entire difference to miners.

### 6.6 Pre-Mint Balance Check

Before constructing any transaction, the Reactor scans and classifies all UTXOs, then displays a summary:

```
── UTXO Pool Status ──
Total UTXOs:    8
Spendable:      2 (15,000 sats)
Locked:         5 (1,650 sats)
Gray zone:      1 (800 sats)
Need:           1,308 sats
Status:         ✅ Balance sufficient
```

If the available balance is insufficient, the Reactor reports the exact deficit and the address to fund — no failed transaction is broadcast.

---

## 7. Batch Minting

### 7.1 Overview

The Reactor supports minting multiple NXS tokens in a single session. Each mint in the batch is an independent Commit + Reveal pair with its own unique proof, interlock, and on-chain footprint. The protocol treats each mint identically to a single mint — batch minting is a client-side convenience, not a protocol-level operation.

### 7.2 Flow

1. User selects **Batch mint** from the mainnet menu.
2. User enters fee rate.
3. Reactor scans UTXOs, applies safety classification, calculates per-mint cost.
4. Displays: available balance, maximum mintable count, cost per mint, total cost.
5. User enters desired count (≤ max).
6. Reactor executes N mints sequentially:
   - Each mint uses `block_height - i` for proof generation (see §4.5).
   - Each Commit TX uses the previous Commit's change as input (chain-linked).
   - Each Reveal TX is broadcast immediately after its Commit.
   - UTXO records are updated after each successful broadcast.
7. If any broadcast fails, execution stops. Already-completed mints are preserved.

### 7.3 Chain-Linked Change

Batch minting reuses change outputs without waiting for block confirmation:

```
UTXO (15,000 sats)
  → Commit #1: 726 commit + 14,077 change + 197 fee
    → Commit #2 (input: 14,077 from mempool): 726 + 13,154 change + 197 fee
      → Commit #3 (input: 13,154 from mempool): 726 + 12,231 change + 197 fee
```

This is safe because `listunspent` with `minconf=0` includes unconfirmed UTXOs, and Bitcoin Core accepts spending unconfirmed outputs up to a chain depth of 25 transactions. A batch of 10 mints creates a chain depth of 10 — well within limits.

### 7.4 Proof Uniqueness

Each mint in a batch produces a completely different proof because the block hash input differs:

| Mint | Block Height | Proof Combined Hash |
| ---- | ------------ | ------------------- |
| #1   | 942022       | `7f3c8b...` (unique) |
| #2   | 942021       | `a1e4d9...` (unique) |
| #3   | 942020       | `52bf71...` (unique) |

The Indexer's `used_proofs` table stores each `proof.combined` hash independently. No replay conflict occurs.

### 7.5 Failure Handling

If Commit or Reveal broadcast fails at mint #K out of N:

- Mints #1 through #K-1 are already on-chain and recorded in local JSON files.
- The Reactor prints: "Stopped at mint #K/N. K-1 mints succeeded."
- The user can restart and continue — the Reactor picks up the remaining change UTXO.

---

## 8. Indexer

### 8.1 Validation Rules (7 Rules)

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

### 8.2 DoS Prefilter Strategy

The rule ordering is deliberate:

- **Rules 1–3** (Format, OP_RETURN, Fee) are string/integer checks — near zero cost. They eliminate the vast majority of irrelevant transactions.
- **Rule 4** (Interlock) requires two SHA-256 computations — still cheap but filters out any malformed attempts.
- **Rule 5** (Identity) requires public key extraction and comparison.
- **Rule 6** (Proof) is the most expensive — two-round verification with disk reads. Only reached if all cheaper checks pass.
- **Rule 7** (Supply) is a simple counter check, placed last because it only matters if everything else is valid.

### 8.3 Sequence Assignment

Mint sequence numbers are assigned by the Indexer based on **transaction position within each block**. The ordering rule is: first confirmed in a block, first assigned. This is a strict FCFS (first come, first served) model.

### 8.4 Replay Protection

Each full node proof is unique (derived from the minter's public key + block hash + random block data). The Indexer maintains a **used-proof table** to reject any proof that has been seen before. Batch mints produce distinct proofs because each uses a different block height (§4.5).

### 8.5 HTTP API Service

The Indexer runs as a standalone HTTP service (`src/bin/indexer.rs`) built with **actix-web**, with a response cache layer (RwLock) for high-concurrency performance and CORS enabled for cross-origin frontend access. All endpoints use the `/api` prefix, with legacy non-prefixed routes maintained for backward compatibility.

#### Core Endpoints

| Endpoint                       | Method | Description                                              |
| ------------------------------ | ------ | -------------------------------------------------------- |
| `GET /api/status`              | GET    | Protocol status: total supply, minted, holders, mint fee, mints remaining, scan height |
| `GET /api/balance/{address}`   | GET    | Query NXS balance for a specific address                 |
| `GET /api/mint/{seq}`          | GET    | Lookup a specific mint by sequence number                |
| `GET /api/mints?page=1&limit=20` | GET | Paginated mint list (oldest first, max 100 per page)     |
| `GET /api/holders`             | GET    | Holder ranking (top 100), sorted by balance descending. Returns `address`, `balance`, and `mint_count` per holder |
| `GET /api/tx/{txid}`           | GET    | Lookup a specific mint by reveal transaction ID          |
| `GET /api/health`              | GET    | Service health check: status, protocol name, version, scan height |

#### Frontend Endpoints

| Endpoint                          | Method | Description                                              |
| --------------------------------- | ------ | -------------------------------------------------------- |
| `GET /api/mints/recent`           | GET    | Recent mints (latest 20, newest first)                   |
| `GET /api/mints/address/{address}`| GET    | All mints for a specific address, plus balance and mint_count |
| `GET /api/mint/tx/{txid}`         | GET    | Lookup mint by reveal txid (frontend-compatible format)  |

API endpoint: https://api.bitcoinexus.xyz

---

## 9. Wallet

### 9.1 Wallet Generation

The Reactor includes a built-in wallet generator (`scripts/wallet_gen.py` using `bip_utils`) that supports three address types:

| Type              | Prefix    | Standard           | Recommended |
| ----------------- | --------- | ------------------ | ----------- |
| **Taproot**       | `bc1p...` | BIP86 (P2TR)       | ✅ Yes      |
| **Native SegWit** | `bc1q...` | BIP84 (P2WPKH)     | Good        |
| **Nested SegWit** | `3...`    | BIP49 (P2SH-P2WPKH)| Legacy      |

### 9.2 Output

For each wallet, the generator produces:

- **12-word BIP39 mnemonic** (standard, compatible with UniSat, OKX Wallet, Sparrow, and any BIP86-compliant wallet).
- **WIF private key** for each address type.
- **Auto-import** into Bitcoin Core for balance tracking via `importdescriptors`.

### 9.3 Funding Requirement

A minter must fund their Taproot address with at least **10,000 sats**:

| Component     | Amount        |
| ------------- | ------------- |
| Protocol fee  | 5,000 sats    |
| Miner fee     | ~20-1000 sats (at 0.1–1 sat/vB) |
| Dust output   | 330 sats      |
| Change        | Remainder     |

---

## 10. Mint Transaction Flow

### 10.1 Single Mint

```
[1] Detect Node     Auto-detect datadir → blk*.dat > 500GB → XOR decrypt magic ✓
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

### 10.2 Batch Mint

```
[1] Detect Node + Verify
         │
[2] Scan UTXOs → Classify → Calculate max mintable
         │
[3] User selects count (1-N)
         │
    ┌────┴────────────────────────────────────────────┐
    │  FOR i = 0 to count-1:                          │
    │    [a] Get block hash at (latest_height - i)    │
    │    [b] Generate proof (unique per block)        │
    │    [c] Build interlock + inscription             │
    │    [d] Commit TX (input = previous change)       │
    │    [e] Reveal TX                                 │
    │    [f] Record mint + change                      │
    │    [g] If broadcast fails → stop, save, report   │
    └────┬────────────────────────────────────────────┘
         │
[4] Save all UTXO records
         │
[5] Display summary (N mints, total NXS, remaining balance)
```

---

## 11. Security Model

| Attack Vector                   | Defense                                                                          |
| ------------------------------- | -------------------------------------------------------------------------------- |
| API relay (no full node)        | Two-round 15s window. Local NVMe ~100ms vs remote API ~5–15s (timeout).          |
| Pruned node disguise            | Direct disk read: blk files > 500GB + early files (`blk00000–00009`) must exist. |
| Identity spoofing               | `pk` field bound to Taproot signing key + Indexer cross-check (Rule 5).          |
| Proof replay                    | Used-proof deduplication table in Indexer (Rule 6).                              |
| Interlock tampering             | Bidirectional SHA-256 verification; `pk` participates in hash (Rule 4).          |
| Mint ordering manipulation      | Indexer assigns sequence by tx position in block — strict FCFS.                  |
| Bitcoin Core 30.x encryption    | Auto-detect XOR obfuscation key, transparent decrypt of blk files.               |
| DoS (spam invalid proofs)       | Cheap checks first (fee, format, interlock) before expensive proof verification. |
| Unlimited mint attempts         | Fixed `amt=500`, supply cap enforced at 42,000 mints, proof uniqueness.          |
| Asset-bearing UTXO burn         | Five-layer UTXO classification; 330/546 sats outputs locked by default (§6).     |
| Batch proof collision           | Each batch mint uses a different block height → unique proof hash (§4.5).        |
| Transfer double-spend           | 3-block confirmation rule. Pending transfers lock sender's balance on broadcast. |
| Transfer insufficient balance   | Indexer checks available_balance = total - locked before accepting transfer.     |
| Batch transfer parsing attack   | Amount count must equal seller input count; any mismatch invalidates the entire batch. |

---

## 12. Architecture

```
nexus-protocol/
├── src/
│   ├── main.rs          # Reactor CLI — menu + single/batch mint engine
│   ├── lib.rs           # Module exports
│   ├── constants.rs     # Protocol parameters (mainnet/regtest via feature flag)
│   ├── proof.rs         # Full node proof + Bitcoin Core 30.x XOR obfuscation
│   ├── transaction.rs   # Dual-layer interlock + pk identity binding
│   ├── indexer.rs       # Transaction validation engine (7 rules + DoS prefilter)
│   ├── utxo.rs          # UTXO safety classification + selection + record tracking
│   ├── node_detect.rs   # Auto-detect Bitcoin node + path management
│   ├── ui.rs            # Terminal UI with color
│   └── bin/
│       └── indexer.rs   # Indexer HTTP service (actix-web, 10 API endpoints + cache layer)
├── scripts/
│   └── wallet_gen.py    # BIP39/86/84/49 wallet generator (bip_utils)
├── docs/
│   └── PROTOCOL.md      # This file — complete protocol specification
├── Cargo.toml
├── README.md            # English
└── README_CN.md         # 中文
```

---

## 13. Configuration

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

UTXO tracking files (auto-generated at runtime):

| File               | Purpose                                      |
| ------------------ | -------------------------------------------- |
| `nxs_mints.json`   | Locked token UTXOs (Reveal output[0])        |
| `nxs_change.json`  | Reusable change UTXOs (Commit change output) |
| `nxs_locked.json`  | Detected external protocol asset UTXOs       |

---

## 14. Testnet (regtest)

The full mint cycle can be tested locally without real BTC:

1. Build with regtest flag: `cargo build --release --features regtest`
2. Select `[1]` → `[3]` from the Reactor menu to start a local regtest node (200 blocks, 5000 BTC).
3. Select `[3]` to mint — fully automated with instant block confirmation.

No 850 GB download. Instant blocks. Full protocol verification.

---

## 15. On-Chain Verification

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

## 16. Transfer Protocol

### 16.1 Design Rationale

Mint requires the Witness inscription layer to embed the full node proof — this is the core innovation of NEXUS. Transfer, however, does not need to prove node ownership. The sender's Taproot signature already proves "I own this address and its NXS balance."

Therefore, Transfer uses **only the OP_RETURN layer** — lightweight, cheap, and executable from any wallet without running a full node.

### 16.2 Transaction Structure

```
┌─────────────────────────────────────────────────────┐
│            NEXUS Transfer Transaction               │
│                                                     │
│  INPUT[0]: Sender's UTXO (Taproot signature)        │
│            → Signature = proof of ownership         │
│                                                     │
│  OP_RETURN LAYER (ASCII readable)                   │
│  ┌─────────────────────────────────────────┐        │
│  │ NXS:TRANSFER:<amount>                   │        │
│  └─────────────────────────────────────────┘        │
│                                                     │
│  OUTPUT[0]: seller_receives + UTXO → seller         │
│  OUTPUT[1]: 330 sats  → recipient (NXS marker)      │
│  OUTPUT[2]: OP_RETURN (transfer data)               │
│  OUTPUT[3]: change    → buyer (remaining BTC)       │
└─────────────────────────────────────────────────────┘
```

### 16.3 OP_RETURN Format

```
NXS:TRANSFER:<amount>
```

| Segment         | Description                                          |
| --------------- | ---------------------------------------------------- |
| `NXS`           | Protocol magic prefix.                               |
| `TRANSFER`      | Operation type.                                      |
| `<amount>`      | Number of NXS tokens to transfer (integer).          |

The recipient address is not in the OP_RETURN — it is read from OUTPUT[1] (the NXS marker output). This keeps the OP_RETURN payload under Bitcoin's 80-byte relay limit.

Example:
```
NXS:TRANSFER:500
```

### 16.4 Transaction Outputs

| Output   | Value    | Purpose                                    |
| -------- | -------- | ------------------------------------------ |
| `[0]`    | Variable | Seller receives (payment + UTXO returned)  |
| `[1]`    | 330 sats | Recipient's NXS marker UTXO                |
| `[2]`    | 0 sats   | OP_RETURN data carrier                     |
| `[3]`    | Variable | Change returned to buyer                   |

The Indexer reads the recipient address from OUTPUT[1].

### 16.5 Batch Transfer (Multi-Order Purchase)

When a buyer purchases multiple sell orders in a single transaction, the market constructs a Batch Transfer that atomically moves NXS from multiple sellers to one buyer.

#### OP_RETURN Format

```
NXS:BATCH:<amount_1>,<amount_2>[,<amount_3>,...]
```

Example (buying two orders: 500 NXS + 88 NXS):
```
NXS:BATCH:500,88
```

The number of comma-separated amounts (N) must equal the number of seller inputs. The OP_RETURN payload must not exceed 80 bytes (Bitcoin's relay limit).

#### Transaction Structure

```
┌─────────────────────────────────────────────────────────┐
│          NEXUS Batch Transfer Transaction               │
│                                                         │
│  INPUT[0]:     Seller_A UTXO (SIGHASH_SINGLE|ANYCANPAY) │
│  INPUT[1]:     Seller_B UTXO (SIGHASH_SINGLE|ANYCANPAY) │
│  ...                                                    │
│  INPUT[N-1]:   Seller_N UTXO                            │
│  INPUT[N..]:   Buyer UTXO(s)                            │
│                                                         │
│  OUTPUT[0]:    Seller_A receives (payment + UTXO)       │
│  OUTPUT[1]:    Seller_B receives (payment + UTXO)       │
│  ...                                                    │
│  OUTPUT[N-1]:  Seller_N receives                        │
│  OUTPUT[N]:    330 sats → Buyer (NXS marker)            │
│  OUTPUT[N+1]:  Fee to protocol address (if ≥ 330 sats)  │
│  OUTPUT[N+2]:  OP_RETURN  NXS:BATCH:<amounts>           │
│  OUTPUT[N+3]:  Change → Buyer (if ≥ 330 sats)           │
└─────────────────────────────────────────────────────────┘
```

**Key rule**: The recipient (buyer) address is always read from OUTPUT[N], where N equals the number of amounts in the BATCH payload.

#### Indexer Validation

For each `i` in `0..N`:

1. `sender_i` = `vin[i].prevout.address`
2. `amount_i` = `amounts[i]` from the OP_RETURN
3. `recipient` = `OUTPUT[N].address`
4. Verify `sender_i` balance ≥ `amount_i`
5. Execute: `sender_i.balance -= amount_i`, `recipient.balance += amount_i`

Each (sender → recipient, amount) pair is recorded as an independent TransferRecord with a `batch_index` field (0, 1, 2, ...).

#### On-Chain Reference

| Transaction | TXID |
| ----------- | ---- |
| First Batch Transfer | `b698ed234d6f25ed254d3f25ccf828ff9d03751cd59fd005b1c5ab645a3ab788` |
| Block | 942321 |
| Payload | `NXS:BATCH:500,88` |
| Sellers | 2 (bc1plwqj... → 500 NXS, bc1pclp0... → 88 NXS) |
| Buyer | bc1ps47y... (received 588 NXS total) |

#### Backward Compatibility

Single transfers continue to use `NXS:TRANSFER:<amount>` with the recipient at OUTPUT[1]. The Indexer supports both formats. Older Indexer versions that do not recognize `NXS:BATCH:` will safely ignore these transactions (the `NXS:TRANSFER:` prefix check does not match).

### 16.6 Indexer Validation Rules (Transfer)

| #  | Rule            | Description                                                                         |
| -- | --------------- | ----------------------------------------------------------------------------------- |
| 1  | **Format**      | OP_RETURN starts with `NXS:TRANSFER:` (single) or `NXS:BATCH:` (multi). Recipient read from OUTPUT[1] (single) or OUTPUT[N] (batch, where N = amount count). |
| 2  | **Balance**     | Sender has sufficient available NXS balance (amount ≤ available balance).           |
| 3  | **Signature**   | Transaction signed by sender's Taproot key (proves address ownership).              |
| 4  | **Confirmation**| 3 block confirmations required before balances update.                              |

### 16.7 Three-Block Confirmation Rule

Transfer balance updates require 3 block confirmations to prevent issues from blockchain reorganizations:

```
TX Broadcast     → Sender's NXS locked (unavailable for other transfers)
1 Confirmation   → TX in block. Waiting.
2 Confirmations  → Reorg protection. Waiting.
3 Confirmations  → FINALIZED. Sender balance decremented, recipient credited.
```

The lock-on-broadcast mechanism prevents double-spending: once a transfer TX enters the mempool, the transferred amount is immediately deducted from the sender's available balance, even before block inclusion.

### 16.8 Available Balance Calculation

```
available_balance = total_balance - locked_in_pending_transfers - locked_in_open_orders
```

A sender cannot transfer or list more NXS than their available balance. This prevents:
- Transferring the same NXS to multiple recipients
- Listing NXS for sale while simultaneously transferring it
- Spending NXS that is locked in an open market order

---

## 17. FAQ

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

**Q: How does batch minting work?**
The Reactor can mint multiple NXS tokens in one session. Each mint uses a different block height for proof generation, ensuring unique proofs. Change from each Commit TX feeds into the next, enabling chain-linked minting without waiting for confirmations.

**Q: Will batch minting burn my inscriptions or Runes?**
No. The Reactor classifies every UTXO before use. Outputs ≤ 546 sats and known protocol-bound UTXOs are automatically locked and never selected as inputs.

**Q: Is there a web frontend?**
Yes. The protocol dashboard is live at [bitcoinexus.xyz](https://bitcoinexus.xyz) with real-time minting progress, holder leaderboard, recent mint feed, address/transaction lookup, and wallet connection support (UniSat, OKX Wallet, Xverse). The frontend queries the Indexer API at `api.bitcoinexus.xyz`. Note: the frontend is for viewing only — minting still requires a full node and the NEXUS Reactor CLI.

**Q: How does Transfer work without the Witness layer?**
Transfer only needs OP_RETURN because it doesn't require a full node proof. The Taproot signature proves ownership of the sending address. The Indexer verifies the sender has sufficient balance and the signature is valid.

**Q: Why 3 block confirmations for transfers?**
To protect against blockchain reorganizations. In a reorg, a confirmed transfer could be reversed. Waiting for 3 confirmations makes this extremely unlikely. During the waiting period, the transferred NXS is locked to prevent double-spending.

**Q: Can I transfer NXS from any wallet?**
Yes. Transfer only requires creating a standard Bitcoin transaction with an OP_RETURN output. Any Taproot-compatible wallet (UniSat, OKX Wallet, Xverse) can construct this transaction through the NEXUS web frontend.

**Q: What is a Batch Transfer?**
When a buyer purchases multiple sell orders at once, the market combines them into a single Bitcoin transaction using `NXS:BATCH:<amt1>,<amt2>,...` in the OP_RETURN. Each amount maps to the corresponding seller input. The buyer address is read from OUTPUT[N] where N is the number of amounts. This is more gas-efficient than executing multiple separate transfers.

---

## Links

- **Website**: [bitcoinexus.xyz](https://bitcoinexus.xyz)
- **GitHub**: [github.com/btcnexus/nexus-protocol](https://github.com/btcnexus/nexus-protocol)
- **API**: [api.bitcoinexus.xyz/api/status](https://api.bitcoinexus.xyz/api/status)
- **Protocol Docs**: [bitcoinexus.xyz/protocol.html](https://bitcoinexus.xyz/protocol.html)

---

## License

MIT

---

*NEXUS — If you don't run a node, you don't mint.*
