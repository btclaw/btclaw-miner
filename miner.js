/**
 * BTCLAW Miner
 * 
 * AI-powered Proof of Work mining client for $BTCLAW on Base.
 * Each epoch = 6 hours. Unlimited submissions per epoch.
 * More solutions = more shares = more $BTCLAW.
 * 
 * Usage:
 *   node miner.js start     — Start mining loop
 *   node miner.js status    — Show mining stats
 *   node miner.js claim     — Claim all unclaimed rewards
 *   node miner.js stake     — Stake ETH
 *   node miner.js unstake   — Withdraw staked ETH
 */

const { ethers } = require('ethers');
require('dotenv').config();

// ============ CONFIG ============

const RPC_URL        = process.env.RPC_URL || 'https://base-mainnet.public.blastapi.io';
const ORACLE_URL     = process.env.ORACLE_URL || 'https://btclaw.space/oracle';
const MINE_CONTRACT  = process.env.MINE_CONTRACT || '0x4E6c056B5c0031E506e5282E2Fb4d0529CE2e221';
const TOKEN_CONTRACT = process.env.TOKEN_CONTRACT || '0x2FEf90CE57CccE7a28be5EDb70311a5ef2728Cf2';
const PRIVATE_KEY    = process.env.PRIVATE_KEY;

const AI_API_KEY = process.env.AI_API_KEY;
const AI_MODEL   = process.env.AI_MODEL || 'deepseek-chat';
const AI_API_URL = process.env.AI_API_URL || 'https://api.deepseek.com/v1/chat/completions';

const DICTIONARY = require('./dictionary.json');

const TOPICS = [
    "technology", "space exploration", "ocean discovery", "artificial intelligence",
    "renewable energy", "biotechnology", "quantum computing", "climate change",
    "cryptocurrency", "robotics", "virtual reality", "genetic engineering",
    "sustainable farming", "nuclear fusion", "mars colonization", "deep learning",
    "cybersecurity", "autonomous vehicles", "brain interface", "nanotechnology",
    "blockchain", "internet of things", "cloud computing", "data science",
    "machine learning", "augmented reality", "drone technology", "smart cities",
    "biomedical engineering", "photonics", "material science", "aerospace",
    "marine biology", "astrophysics", "particle physics", "geology",
    "archaeology", "paleontology", "meteorology", "ecology",
    "neuroscience", "immunology", "pharmacology", "epidemiology",
    "urban planning", "architecture", "civil engineering", "transportation",
    "telecommunications", "semiconductor", "superconductor", "battery technology",
    "hydrogen energy", "carbon capture", "desalination", "vertical farming",
    "synthetic biology", "proteomics", "bioinformatics", "digital twin",
    "edge computing", "federated learning", "homomorphic encryption", "zero knowledge proof"
];

// ============ CONTRACT SETUP ============

const provider = new ethers.JsonRpcProvider(RPC_URL);
const wallet   = new ethers.Wallet(PRIVATE_KEY, provider);

const MINE_ABI = [
    "function getCurrentEpoch() view returns (uint256)",
    "function getChallenge(uint256) view returns (bytes32 seed, uint256 target, uint256 reward, uint256 epochStartTime, uint256 epochEndTime, uint256 currentSolutions)",
    "function getEpochSeed(uint256) view returns (bytes32)",
    "function currentTarget() view returns (uint256)",
    "function stakes(address) view returns (uint256)",
    "function minerShares(uint256, address) view returns (uint256)",
    "function epochTotalShares(uint256) view returns (uint256)",
    "function isMiningActive() view returns (bool)",
    "function MIN_STAKE() view returns (uint256)",
    "function submitSolution(uint256 epochId, string sentence, bytes signature) external",
    "function claimReward(uint256 epochId) external",
    "function batchClaimRewards(uint256[] epochIds) external",
    "function stake() external payable",
    "function unstake(uint256 amount) external",
    "function getEpochReward(uint256) view returns (uint256)",
    "function getEpochSolutionCount(uint256) view returns (uint256)",
    "function getEpochMinerCount(uint256) view returns (uint256)",
    "function TOTAL_EPOCHS() view returns (uint256)",
    "function miningStartTime() view returns (uint256)",
    "function EPOCH_DURATION() view returns (uint256)"
];

const TOKEN_ABI = [
    "function balanceOf(address) view returns (uint256)",
    "function symbol() view returns (string)"
];

const mineContract  = new ethers.Contract(MINE_CONTRACT, MINE_ABI, wallet);
const tokenContract = new ethers.Contract(TOKEN_CONTRACT, TOKEN_ABI, provider);

// ============ STATE ============

let isRunning = false;
let sessionStats = { solutionsFound: 0, totalAttempts: 0, totalAICalls: 0, startTime: 0 };
const submittedSentences = new Set();

// ============ CHALLENGE DERIVATION ============

function deriveChallenge(seed) {
    const seedBuf = ethers.getBytes(seed);
    const topicIndex = seedBuf[0] % TOPICS.length;
    const topic = TOPICS[topicIndex];
    const wordCount = (seedBuf[1] % 11) + 15;
    const keywords = [];
    for (let i = 0; i < 3; i++) {
        const idx = (seedBuf[2 + i * 2] * 256 + seedBuf[3 + i * 2]) % DICTIONARY.length;
        keywords.push(DICTIONARY[idx]);
    }
    return { topic, wordCount, keywords };
}

// ============ AI GENERATION ============

async function generateCandidates(topic, wordCount, keywords, batchSize = 10) {
    const prompt = `Write ${batchSize} English sentences about "${topic}".

MUST include ALL these words in each sentence: ${keywords.join(', ')}
MUST be EXACTLY ${wordCount} words per sentence (${wordCount - 1} spaces).

Double-check word count by counting spaces (${wordCount} words = ${wordCount - 1} spaces).

Output ${batchSize} sentences, one per line. No numbers, no bullets, nothing else.`;

    try {
        const isResponsesAPI = AI_API_URL.includes('/responses');

        let requestBody;
        if (isResponsesAPI) {
            requestBody = { model: AI_MODEL, input: prompt, stream: false };
        } else {
            requestBody = { model: AI_MODEL, max_tokens: 2000, stream: false, messages: [{ role: 'user', content: prompt }] };
        }

        const response = await fetch(AI_API_URL, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json', 'Authorization': `Bearer ${AI_API_KEY}` },
            body: JSON.stringify(requestBody)
        });

        const rawText = await response.text();

        let data;
        try {
            data = JSON.parse(rawText);
        } catch (e) {
            const contentParts = [];
            for (const line of rawText.split('\n')) {
                if (line.startsWith('data: ') && line !== 'data: [DONE]') {
                    try {
                        const chunk = JSON.parse(line.slice(6));
                        const delta = chunk.choices?.[0]?.delta?.content || '';
                        if (delta) contentParts.push(delta);
                    } catch (e3) {}
                }
            }
            if (contentParts.length > 0) {
                data = { choices: [{ message: { content: contentParts.join('') } }] };
            } else { return []; }
        }

        let text = '';
        if (data.choices && data.choices[0]) {
            text = data.choices[0].message?.content || data.choices[0].delta?.content || '';
        } else if (data.output) {
            if (typeof data.output === 'string') {
                text = data.output;
            } else if (Array.isArray(data.output)) {
                text = data.output.map(block => {
                    if (typeof block === 'string') return block;
                    if (block.type === 'message' && Array.isArray(block.content)) {
                        return block.content.filter(c => c.type === 'output_text' || c.type === 'text').map(c => c.text).join('\n');
                    }
                    return block.text || (typeof block.content === 'string' ? block.content : '');
                }).filter(Boolean).join('\n');
            }
        } else if (data.output_text) {
            text = data.output_text;
        } else { return []; }

        return text.split('\n')
            .map(s => s.trim().replace(/^\d+[\.\):\-]\s*/, '').replace(/^[-•*]\s*/, '').trim())
            .filter(s => s.length > 10 && !s.startsWith('Here') && !s.startsWith('Note') && !s.startsWith('Each'));
    } catch (err) {
        console.error('  ❌ AI error:', err.message);
        return [];
    }
}

// ============ LOCAL PRE-FILTER ============

function localFilter(sentence, keywords, wordCount) {
    const words = sentence.trim().split(/\s+/);
    if (words.length !== wordCount) return false;
    const lower = sentence.toLowerCase();
    for (const kw of keywords) {
        if (!lower.includes(kw.toLowerCase())) return false;
    }
    return true;
}

// ============ HASH CHECK ============

function checkHash(seed, miner, sentence, target) {
    const packed = ethers.solidityPacked(['bytes32', 'address', 'string'], [seed, miner, sentence]);
    const hash = ethers.keccak256(packed);
    return { hash, meets: BigInt(hash) < BigInt(target) };
}

// ============ ORACLE ============

async function getOracleSignature(epochId, miner, sentence) {
    const response = await fetch(`${ORACLE_URL}/validate`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ epochId, miner, sentence })
    });
    const data = await response.json();
    if (!data.valid) throw new Error(`Oracle: ${data.reason}`);
    return data.signature;
}

// ============ SUBMIT ============

async function submitOnChain(epochId, sentence, signature) {
    const tx = await mineContract.submitSolution(epochId, sentence, signature);
    return await tx.wait();
}

// ============ CORE MINING LOOP ============

async function miningLoop() {
    isRunning = true;
    sessionStats.startTime = Date.now();

    console.log('\n🦞 BTCLAW Miner v1.0');
    console.log(`   Wallet:  ${wallet.address}`);
    console.log(`   Oracle:  ${ORACLE_URL}`);
    console.log(`   AI:      ${AI_MODEL}\n`);

    const stk = await mineContract.stakes(wallet.address);
    const minStake = await mineContract.MIN_STAKE();
    if (stk < minStake) {
        console.log(`⚠️  Need ${ethers.formatEther(minStake)} ETH staked. Run: node miner.js stake`);
        return;
    }

    let lastClaimedEpoch = -1;

    while (isRunning) {
        try {
            const epochId = await mineContract.getCurrentEpoch();
            if (epochId.toString() === ethers.MaxUint256.toString()) { console.log('Mining ended.'); break; }

            const challenge = await mineContract.getChallenge(epochId);
            const seed = challenge.seed;
            const target = challenge.target;
            const reward = challenge.reward;
            const constraints = deriveChallenge(seed);

            console.log(`\nEpoch #${epochId} | Topic: ${constraints.topic} | Words: ${constraints.wordCount} | Keywords: ${constraints.keywords.join(', ')} | Reward: ${Number(ethers.formatEther(reward)).toLocaleString()} $BTCLAW`);

            // Auto-claim previous epoch
            if (Number(epochId) > 0 && Number(epochId) - 1 > lastClaimedEpoch) {
                try {
                    const prevEpoch = Number(epochId) - 1;
                    const prevShares = await mineContract.minerShares(prevEpoch, wallet.address);
                    if (prevShares > 0n) {
                        console.log(`  💰 Claiming epoch #${prevEpoch}...`);
                        const claimTx = await mineContract.claimReward(prevEpoch);
                        await claimTx.wait();
                        lastClaimedEpoch = prevEpoch;
                        const bal = await tokenContract.balanceOf(wallet.address);
                        console.log(`  ✅ Claimed! Balance: ${Number(ethers.formatEther(bal)).toLocaleString()} $BTCLAW`);
                    }
                } catch (e) {}
            }

            let epochSolutions = 0;

            while (isRunning) {
                const nowEpoch = await mineContract.getCurrentEpoch();
                if (nowEpoch.toString() !== epochId.toString()) {
                    console.log(`\n  ⏰ Epoch #${epochId} ended. Solutions: ${epochSolutions}`);
                    submittedSentences.clear();
                    break;
                }

                const sentences = await generateCandidates(constraints.topic, constraints.wordCount, constraints.keywords);
                sessionStats.totalAICalls++;

                if (sentences.length === 0) { await sleep(5000); continue; }

                // Pre-filter
                const valid = sentences.filter(s => localFilter(s, constraints.keywords, constraints.wordCount) && !submittedSentences.has(s));

                if (valid.length === 0) {
                    console.log(`  ⚠️  0/${sentences.length} passed filter. Retrying...`);
                    await sleep(2000);
                    continue;
                }

                for (const sentence of valid) {
                    sessionStats.totalAttempts++;
                    const result = checkHash(seed, wallet.address, sentence, target);

                    if (result.meets) {
                        try {
                            const sig = await getOracleSignature(Number(epochId), wallet.address, sentence);
                            const receipt = await submitOnChain(epochId, sentence, sig);
                            epochSolutions++;
                            sessionStats.solutionsFound++;
                            submittedSentences.add(sentence);
                            console.log(`  ✅ Solution accepted! TX: ${receipt.hash}`);
                        } catch (err) {
                            console.log(`  ❌ Submit failed: ${err.message.slice(0, 100)}`);
                        }
                    }
                }

                console.log(`  Checked: ${sessionStats.totalAttempts} | Found: ${sessionStats.solutionsFound} | This epoch: ${epochSolutions}`);
                await sleep(3000);
            }

        } catch (err) {
            console.error(`Error: ${err.message}`);
            await sleep(10000);
        }
    }
}

// ============ COMMANDS ============

async function printStatus() {
    const epochId = await mineContract.getCurrentEpoch();
    const balance = await tokenContract.balanceOf(wallet.address);
    const stk     = await mineContract.stakes(wallet.address);
    const ethBal  = await provider.getBalance(wallet.address);

    console.log(`Wallet:    ${wallet.address}`);
    console.log(`ETH:       ${Number(ethers.formatEther(ethBal)).toFixed(6)} ETH`);
    console.log(`Staked:    ${ethers.formatEther(stk)} ETH`);
    console.log(`$BTCLAW:   ${Number(ethers.formatEther(balance)).toLocaleString()}`);
    console.log(`Epoch:     ${epochId.toString() === ethers.MaxUint256.toString() ? 'N/A' : epochId}`);
}

async function doStake() {
    const minStake = await mineContract.MIN_STAKE();
    const current  = await mineContract.stakes(wallet.address);
    if (current >= minStake) { console.log('Already staked.'); return; }
    const needed = minStake - current;
    console.log(`Staking ${ethers.formatEther(needed)} ETH...`);
    const tx = await mineContract.stake({ value: needed });
    await tx.wait();
    console.log(`Done. TX: ${tx.hash}`);
}

async function doUnstake() {
    const current = await mineContract.stakes(wallet.address);
    if (current === 0n) { console.log('Nothing staked.'); return; }
    const tx = await mineContract.unstake(current);
    await tx.wait();
    console.log(`Unstaked. TX: ${tx.hash}`);
}

async function claimAll() {
    const currentEpoch = await mineContract.getCurrentEpoch();
    const maxEpoch = currentEpoch.toString() === ethers.MaxUint256.toString() ? 28 : Number(currentEpoch);
    const claimable = [];
    for (let i = 0; i < maxEpoch; i++) {
        const shares = await mineContract.minerShares(i, wallet.address);
        if (shares > 0n) claimable.push(i);
    }
    if (claimable.length === 0) { console.log('Nothing to claim.'); return; }
    const tx = await mineContract.batchClaimRewards(claimable);
    await tx.wait();
    const balance = await tokenContract.balanceOf(wallet.address);
    console.log(`Claimed! Balance: ${Number(ethers.formatEther(balance)).toLocaleString()} $BTCLAW`);
}

function sleep(ms) { return new Promise(r => setTimeout(r, ms)); }

if (!PRIVATE_KEY) { console.error('Missing PRIVATE_KEY in .env'); process.exit(1); }
if (!AI_API_KEY) { console.error('Missing AI_API_KEY in .env'); process.exit(1); }

const cmd = process.argv[2] || 'start';
switch (cmd) {
    case 'start':
        miningLoop().catch(console.error);
        process.on('SIGINT', () => { isRunning = false; });
        break;
    case 'status':  printStatus().catch(console.error); break;
    case 'stake':   doStake().catch(console.error); break;
    case 'unstake': doUnstake().catch(console.error); break;
    case 'claim':   claimAll().catch(console.error); break;
    default: console.log('Usage: node miner.js [start|status|stake|unstake|claim]');
}
