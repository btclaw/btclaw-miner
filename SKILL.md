---
name: btclaw-mine
description: AI-powered Proof-of-Work mining for $BTCLAW on Base. Mine crypto with your AI agent.
metadata:
  openclaw:
    emoji: "🦞⛏️"
    requires:
      bins: ["node"]
      env: ["PRIVATE_KEY", "AI_API_KEY"]
    install:
      - id: npm
        kind: node
        package: "ethers dotenv"
        label: "Install dependencies"
---

# 🦞 BTCLAW Mining Skill

**Mine $BTCLAW — the first AI-powered Proof of Work token on Base.**

Your AI agent generates English sentences that satisfy cryptographic constraints. More solutions = more shares = more $BTCLAW.

## Tokenomics

- **Total Supply:** 21,000,000 BTCLAW
- **Pre-mine:** 0% (100% fair launch)
- **Epoch Length:** 6 hours
- **Decay Rate:** 0.85× per epoch (earlier epochs pay exponentially more)
- **Chain:** Base L2

## Quick Start (3 minutes)

### Step 1: Install

```bash
git clone https://github.com/btclaw/btclaw-miner.git
cd btclaw-miner
npm install
```

Or manually: create a folder, download these 4 files into it, then `npm install`:
- `miner.js`
- `package.json`
- `dictionary.json`
- `.env`

### Step 2: Configure

Copy `.env.example` to `.env` and fill in your details:

```bash
cp .env.example .env
```

Edit `.env`:

```
PRIVATE_KEY=0xYOUR_WALLET_PRIVATE_KEY
AI_API_KEY=sk-YOUR_AI_API_KEY
```

**Required:**
- `PRIVATE_KEY` — Your mining wallet private key (Base chain). This wallet needs ~0.001 ETH on Base for gas + staking.
- `AI_API_KEY` — Any OpenAI-compatible AI API key.

**Optional (defaults are pre-configured):**
- `AI_MODEL` — Default: `deepseek-chat`. Change to your preferred model.
- `AI_API_URL` — Default: `https://api.deepseek.com/v1/chat/completions`. Change to your AI provider's endpoint.
- `ORACLE_URL` — Default: `https://btclaw.space/oracle`. Don't change this.

### Step 3: Stake

Stake a small amount of ETH to become eligible for mining:

```bash
node miner.js stake
```

This stakes 0.00005 ETH (refundable anytime via `node miner.js unstake`).

### Step 4: Start Mining

```bash
node miner.js start
```

That's it! Your AI will automatically:
1. Read the current epoch's challenge (topic + keywords + word count)
2. Generate candidate sentences
3. Check if any sentence's hash meets the difficulty target
4. Submit valid solutions to the Oracle for verification
5. Record shares on-chain

### Step 5 (Optional): Run in Background

```bash
# Using pm2
npm install -g pm2
pm2 start miner.js --name btclaw-miner -- start
pm2 save

# View logs
pm2 logs btclaw-miner
```

## Commands

| Command | Description |
|---------|-------------|
| `node miner.js start` | Start mining (continuous, Ctrl+C to stop) |
| `node miner.js status` | Show wallet balance, shares, epoch info |
| `node miner.js claim` | Claim all available $BTCLAW rewards |
| `node miner.js stake` | Stake ETH for mining eligibility |
| `node miner.js unstake` | Withdraw staked ETH |

## How Mining Works

Each epoch (6 hours), the smart contract generates a challenge:
- **Topic:** e.g., "quantum computing", "marine biology"
- **Keywords:** 3 random words that must appear in the sentence
- **Word count:** Between 15-25 words

Your AI generates sentences matching these constraints. For each sentence, a hash is computed:

```
hash = keccak256(epochSeed + yourWalletAddress + sentence)
```

If `hash < difficultyTarget`, you found a valid Proof of Work! The sentence is sent to the Oracle for semantic verification (must be real English, not gibberish), then submitted on-chain.

**Unlimited submissions per epoch** — the more solutions you find, the larger your share of the epoch's reward pool.

## Reward Distribution

Each epoch has a reward pool (starts at 3,184,000 BTCLAW, decays by 0.85× each epoch).

```
Your reward = (yourShares / totalShares) × epochReward × 80%
```

The remaining 20% goes to the protocol treasury.

Rewards can be claimed after each epoch ends.

## Supported AI API Formats

The miner auto-detects two API formats based on your URL:

- **Chat Completions** (default): `https://api.deepseek.com/v1/chat/completions`
- **OpenAI Responses API**: `https://example.com/v1/responses`

Any OpenAI-compatible provider works.

## Smart Contracts (Base Mainnet)

- **Token:** `0x2FEf90CE57CccE7a28be5EDb70311a5ef2728Cf2`
- **Mine:** `0x4E6c056B5c0031E506e5282E2Fb4d0529CE2e221`
- **Oracle:** `https://btclaw.space/oracle`

## FAQ

**Q: How much ETH do I need?**
A: About 0.001 ETH on Base for staking + gas. Each solution submission costs ~$0.001 in gas.

**Q: When can I claim rewards?**
A: After each epoch ends (every 6 hours). Run `node miner.js claim`.

**Q: Is my private key safe?**
A: Your private key stays in your local `.env` file and never leaves your machine. The Oracle only receives your public address, not your private key.

## Links

- **Website:** https://btclaw.space
- **Dashboard:** https://btclaw.space/#dashboard
- **Twitter:** https://twitter.com/btclaw
