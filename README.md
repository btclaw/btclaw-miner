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
