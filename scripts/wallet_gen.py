#!/usr/bin/env python3
"""NEXUS Wallet Generator — 生成BIP39助记词 + 3种地址类型"""

import sys
import os
import hashlib
import hmac
import struct

# ═══ BIP39 Mnemonic ═══

WORDLIST_URL = "https://raw.githubusercontent.com/bitcoin/bips/master/bip-0039/english.txt"
WORDLIST = None

def get_wordlist():
    global WORDLIST
    if WORDLIST:
        return WORDLIST
    
    # 内置前20个词用于测试，完整词表从文件或内嵌
    # 这里内嵌完整BIP39英文词表的方式：用hashlib验证entropy
    wordlist_path = os.path.join(os.path.dirname(os.path.abspath(__file__)), "bip39_english.txt")
    
    if os.path.exists(wordlist_path):
        with open(wordlist_path) as f:
            WORDLIST = [w.strip() for w in f.readlines() if w.strip()]
        return WORDLIST
    
    # 尝试用已安装的mnemonic库
    try:
        from mnemonic import Mnemonic
        m = Mnemonic("english")
        WORDLIST = m.wordlist
        return WORDLIST
    except ImportError:
        pass
    
    print("ERROR: Need mnemonic library. Run: pip install mnemonic --break-system-packages", file=sys.stderr)
    sys.exit(1)

def generate_mnemonic(strength=128):
    """生成12词助记词 (128bit entropy)"""
    try:
        from mnemonic import Mnemonic
        m = Mnemonic("english")
        return m.generate(strength)
    except ImportError:
        pass
    
    # 手动实现
    wordlist = get_wordlist()
    entropy = os.urandom(strength // 8)
    h = hashlib.sha256(entropy).digest()
    
    bits = bin(int.from_bytes(entropy, 'big'))[2:].zfill(strength)
    checksum_bits = bin(h[0])[2:].zfill(8)[:strength // 32]
    all_bits = bits + checksum_bits
    
    words = []
    for i in range(0, len(all_bits), 11):
        idx = int(all_bits[i:i+11], 2)
        words.append(wordlist[idx])
    
    return " ".join(words)

def mnemonic_to_seed(mnemonic, passphrase=""):
    """BIP39 助记词转seed"""
    try:
        from mnemonic import Mnemonic
        m = Mnemonic("english")
        return m.to_seed(mnemonic, passphrase)
    except ImportError:
        pass
    
    password = mnemonic.encode('utf-8')
    salt = ("mnemonic" + passphrase).encode('utf-8')
    return hashlib.pbkdf2_hmac('sha512', password, salt, 2048)

# ═══ BIP32 HD Key Derivation ═══

SECP256K1_ORDER = 0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEBAAEDCE6AF48A03BBFD25E8CD0364141

def point_from_privkey(privkey_int):
    """从私钥整数得到公钥点 (使用纯Python椭圆曲线)"""
    try:
        # 优先用ecdsa库
        import ecdsa
        sk = ecdsa.SigningKey.from_secret_exponent(privkey_int, curve=ecdsa.SECP256k1)
        vk = sk.get_verifying_key()
        x = int.from_bytes(vk.to_string()[:32], 'big')
        y = int.from_bytes(vk.to_string()[32:], 'big')
        return (x, y)
    except ImportError:
        pass
    
    # 纯Python secp256k1
    P = 0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEFFFFFC2F
    Gx = 0x79BE667EF9DCBBAC55A06295CE870B07029BFCDB2DCE28D959F2815B16F81798
    Gy = 0x483ADA7726A3C4655DA4FBFC0E1108A8FD17B448A68554199C47D08FFB10D4B8
    
    def modinv(a, m):
        g, x, _ = extended_gcd(a % m, m)
        return x % m
    
    def extended_gcd(a, b):
        if a == 0: return b, 0, 1
        g, x, y = extended_gcd(b % a, a)
        return g, y - (b // a) * x, x
    
    def point_add(p1, p2):
        if p1 is None: return p2
        if p2 is None: return p1
        x1, y1 = p1
        x2, y2 = p2
        if x1 == x2 and y1 != y2: return None
        if x1 == x2:
            lam = (3 * x1 * x1) * modinv(2 * y1, P) % P
        else:
            lam = (y2 - y1) * modinv(x2 - x1, P) % P
        x3 = (lam * lam - x1 - x2) % P
        y3 = (lam * (x1 - x3) - y1) % P
        return (x3, y3)
    
    def scalar_mult(k, point):
        result = None
        addend = point
        while k:
            if k & 1:
                result = point_add(result, addend)
            addend = point_add(addend, addend)
            k >>= 1
        return result
    
    return scalar_mult(privkey_int, (Gx, Gy))

def compress_pubkey(x, y):
    prefix = b'\x02' if y % 2 == 0 else b'\x03'
    return prefix + x.to_bytes(32, 'big')

def privkey_to_wif(privkey_bytes, compressed=True, mainnet=True):
    prefix = b'\x80' if mainnet else b'\xef'
    payload = prefix + privkey_bytes
    if compressed:
        payload += b'\x01'
    checksum = hashlib.sha256(hashlib.sha256(payload).digest()).digest()[:4]
    import base58
    return base58.b58encode(payload + checksum).decode()

def hash160(data):
    return hashlib.new('ripemd160', hashlib.sha256(data).digest()).digest()

def bech32_encode(hrp, witver, witprog):
    """Bech32/Bech32m encoding"""
    CHARSET = "qpzry9x8gf2tvdw0s3jn54khce6mua7l"
    BECH32M_CONST = 0x2bc830a3
    
    def bech32_polymod(values):
        GEN = [0x3b6a57b2, 0x26508e6d, 0x1ea119fa, 0x3d4233dd, 0x2a1462b3]
        chk = 1
        for v in values:
            b = chk >> 25
            chk = ((chk & 0x1ffffff) << 5) ^ v
            for i in range(5):
                chk ^= GEN[i] if ((b >> i) & 1) else 0
        return chk
    
    def hrp_expand(hrp):
        return [ord(x) >> 5 for x in hrp] + [0] + [ord(x) & 31 for x in hrp]
    
    def convertbits(data, frombits, tobits, pad=True):
        acc = 0
        bits = 0
        ret = []
        maxv = (1 << tobits) - 1
        for value in data:
            acc = (acc << frombits) | value
            bits += frombits
            while bits >= tobits:
                bits -= tobits
                ret.append((acc >> bits) & maxv)
        if pad and bits:
            ret.append((acc << (tobits - bits)) & maxv)
        return ret
    
    data = convertbits(witprog, 8, 5)
    const = BECH32M_CONST if witver > 0 else 1
    polymod = bech32_polymod(hrp_expand(hrp) + [witver] + data + [0,0,0,0,0,0]) ^ const
    checksum = [(polymod >> 5 * (5 - i)) & 31 for i in range(6)]
    
    return hrp + "1" + "".join(CHARSET[d] for d in [witver] + data + checksum)

def derive_child(parent_key, parent_chain, index, hardened=False):
    if hardened:
        index += 0x80000000
        data = b'\x00' + parent_key + struct.pack('>I', index)
    else:
        point = point_from_privkey(int.from_bytes(parent_key, 'big'))
        pubkey = compress_pubkey(point[0], point[1])
        data = pubkey + struct.pack('>I', index)
    
    I = hmac.new(parent_chain, data, hashlib.sha512).digest()
    IL = I[:32]
    IR = I[32:]
    
    child_key_int = (int.from_bytes(IL, 'big') + int.from_bytes(parent_key, 'big')) % SECP256K1_ORDER
    child_key = child_key_int.to_bytes(32, 'big')
    
    return child_key, IR

def derive_path(seed, path):
    """从seed派生指定路径的私钥"""
    I = hmac.new(b"Bitcoin seed", seed, hashlib.sha512).digest()
    key = I[:32]
    chain = I[32:]
    
    parts = path.replace("m/", "").split("/")
    for part in parts:
        hardened = part.endswith("'") or part.endswith("h")
        idx = int(part.rstrip("'h"))
        key, chain = derive_child(key, chain, idx, hardened)
    
    return key

# ═══ 地址生成 ═══

def generate_native_segwit(privkey_bytes):
    """P2WPKH - bc1q 地址 (BIP84: m/84'/0'/0'/0/0)"""
    point = point_from_privkey(int.from_bytes(privkey_bytes, 'big'))
    pubkey = compress_pubkey(point[0], point[1])
    h160 = hash160(pubkey)
    addr = bech32_encode("bc", 0, h160)
    return addr

def generate_nested_segwit(privkey_bytes):
    """P2SH-P2WPKH - 3... 地址 (BIP49: m/49'/0'/0'/0/0)"""
    point = point_from_privkey(int.from_bytes(privkey_bytes, 'big'))
    pubkey = compress_pubkey(point[0], point[1])
    h160 = hash160(pubkey)
    
    # P2SH(P2WPKH)
    redeem_script = b'\x00\x14' + h160
    script_hash = hash160(redeem_script)
    
    # Base58Check with version 0x05
    payload = b'\x05' + script_hash
    checksum = hashlib.sha256(hashlib.sha256(payload).digest()).digest()[:4]
    import base58
    return base58.b58encode(payload + checksum).decode()

def generate_taproot(privkey_bytes):
    """P2TR - bc1p 地址 (BIP86: m/86'/0'/0'/0/0)"""
    point = point_from_privkey(int.from_bytes(privkey_bytes, 'big'))
    x_bytes = point[0].to_bytes(32, 'big')
    
    # BIP341 tweaked key (no script tree)
    t = hashlib.sha256(b"TapTweak" + b"TapTweak" + x_bytes).digest()
    t_int = int.from_bytes(t, 'big')
    
    # If y is odd, negate private key
    if point[1] % 2 != 0:
        privkey_int = SECP256K1_ORDER - int.from_bytes(privkey_bytes, 'big')
    else:
        privkey_int = int.from_bytes(privkey_bytes, 'big')
    
    tweaked_int = (privkey_int + t_int) % SECP256K1_ORDER
    tweaked_point = point_from_privkey(tweaked_int)
    tweaked_x = tweaked_point[0].to_bytes(32, 'big')
    
    addr = bech32_encode("bc", 1, tweaked_x)
    return addr

# ═══ 主函数 ═══

def main():
    import base58
    
    addr_type = sys.argv[1] if len(sys.argv) > 1 else "taproot"
    
    # 生成助记词
    mnemonic = generate_mnemonic(128)  # 12 words
    seed = mnemonic_to_seed(mnemonic)
    
    results = {}
    
    if addr_type in ["all", "taproot"]:
        # BIP86: m/86'/0'/0'/0/0
        key = derive_path(seed, "86'/0'/0'/0/0")
        addr = generate_taproot(key)
        wif = privkey_to_wif(key)
        results["taproot"] = {"path": "m/86'/0'/0'/0/0", "address": addr, "wif": wif}
    
    if addr_type in ["all", "native_segwit"]:
        # BIP84: m/84'/0'/0'/0/0
        key = derive_path(seed, "84'/0'/0'/0/0")
        addr = generate_native_segwit(key)
        wif = privkey_to_wif(key)
        results["native_segwit"] = {"path": "m/84'/0'/0'/0/0", "address": addr, "wif": wif}
    
    if addr_type in ["all", "nested_segwit"]:
        # BIP49: m/49'/0'/0'/0/0
        key = derive_path(seed, "49'/0'/0'/0/0")
        addr = generate_nested_segwit(key)
        wif = privkey_to_wif(key)
        results["nested_segwit"] = {"path": "m/49'/0'/0'/0/0", "address": addr, "wif": wif}
    
    # 输出JSON
    import json
    output = {
        "mnemonic": mnemonic,
        "addresses": results
    }
    print(json.dumps(output))

if __name__ == "__main__":
    main()
