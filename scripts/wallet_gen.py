#!/usr/bin/env python3
"""NEXUS Wallet Generator — BIP39 + BIP86/84/49 标准地址生成
使用 bip_utils 库确保地址与 UniSat/Sparrow/OKX 等钱包完全一致

安装依赖: pip install bip_utils --break-system-packages -i https://pypi.org/simple/
"""

import sys
import json

try:
    from bip_utils import (
        Bip39MnemonicGenerator, Bip39SeedGenerator, Bip39WordsNum,
        Bip86, Bip86Coins,
        Bip84, Bip84Coins,
        Bip49, Bip49Coins,
    )
except ImportError:
    print(json.dumps({"error": "bip_utils not installed. Run: pip install bip_utils --break-system-packages -i https://pypi.org/simple/"}))
    sys.exit(1)


def generate_wallet(addr_type="all"):
    # 生成12词助记词
    mnemonic = Bip39MnemonicGenerator().FromWordsNumber(Bip39WordsNum.WORDS_NUM_12)
    mnemonic_str = mnemonic.ToStr()
    seed = Bip39SeedGenerator(mnemonic_str).Generate()

    results = {}

    if addr_type in ["all", "taproot"]:
        # BIP86: m/86'/0'/0'/0/0 → Taproot (bc1p...)
        bip86 = Bip86.FromSeed(seed, Bip86Coins.BITCOIN)
        key = bip86.Purpose().Coin().Account(0).Change(Bip86.CHANGE_TYPE_EXTERNAL).AddressIndex(0)
        results["taproot"] = {
            "path": "m/86'/0'/0'/0/0",
            "address": key.PublicKey().ToAddress(),
            "wif": key.PrivateKey().ToWif(),
        }

    if addr_type in ["all", "native_segwit"]:
        # BIP84: m/84'/0'/0'/0/0 → Native SegWit (bc1q...)
        bip84 = Bip84.FromSeed(seed, Bip84Coins.BITCOIN)
        key = bip84.Purpose().Coin().Account(0).Change(Bip84.CHANGE_TYPE_EXTERNAL).AddressIndex(0)
        results["native_segwit"] = {
            "path": "m/84'/0'/0'/0/0",
            "address": key.PublicKey().ToAddress(),
            "wif": key.PrivateKey().ToWif(),
        }

    if addr_type in ["all", "nested_segwit"]:
        # BIP49: m/49'/0'/0'/0/0 → Nested SegWit (3...)
        bip49 = Bip49.FromSeed(seed, Bip49Coins.BITCOIN)
        key = bip49.Purpose().Coin().Account(0).Change(Bip49.CHANGE_TYPE_EXTERNAL).AddressIndex(0)
        results["nested_segwit"] = {
            "path": "m/49'/0'/0'/0/0",
            "address": key.PublicKey().ToAddress(),
            "wif": key.PrivateKey().ToWif(),
        }

    return {
        "mnemonic": mnemonic_str,
        "addresses": results,
    }


if __name__ == "__main__":
    addr_type = sys.argv[1] if len(sys.argv) > 1 else "all"
    result = generate_wallet(addr_type)
    print(json.dumps(result))
