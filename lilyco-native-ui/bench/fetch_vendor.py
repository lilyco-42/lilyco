#!/usr/bin/env python3
"""下载 bench/vendor/ 的固定版本脚本，并按 SRI (sha384) + sha256 双重校验。

vendor/ 不入库：四个脚本共 5.4 MB，会触发仓库大文件守则。这里把版本、URL 和
哈希钉死，保证任何一台机器拉下来的字节与 2026-11-14 实测时完全一致。

    python3 fetch_vendor.py          # 缺失则下载，已存在则校验
    python3 fetch_vendor.py --check  # 只校验，不联网
"""

import base64
import hashlib
import sys
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
VENDOR = HERE / "vendor"

# name: (url, sri, sha256)
# 前三个与 lilyco-ultra-ui/src/generator.rs 的 HEAD_SCRIPTS 一一对应；
# babel-8.1.2.js 只用于复现 "未锁版本浮到 8" 的白屏故障，不参与离线页生成。
ASSETS = {
    "react-18.3.1.js": (
        "https://unpkg.com/react@18.3.1/umd/react.production.min.js",
        "sha384-DGyLxAyjq0f9SPpVevD6IgztCFlnMF6oW/XQGmfe+IsZ8TqEiDrcHkMLKI6fiB/Z",
        "sha256-d949f1c3687aedadcedac85261865f29b17cd273997e7f6b2bfc53b2f9d4c4dd",
    ),
    "react-dom-18.3.1.js": (
        "https://unpkg.com/react-dom@18.3.1/umd/react-dom.production.min.js",
        "sha384-gTGxhz21lVGYNMcdJOyq01Edg0jhn/c22nsx0kyqP0TxaV5WVdsSH1fSDUf5YJj1",
        "sha256-35f4f974f4b2bcd44da73963347f8952e341f83909e4498227d4e26b98f66f0d",
    ),
    "babel-7.28.4.js": (
        "https://unpkg.com/@babel/standalone@7.28.4/babel.min.js",
        "sha384-tL0JdJBWAk5nHKZhc/dtWf7bZRpYP13x4HjH85NrwCr/JkBnrZ7RNBOAdDzJlpof",
        "sha256-254d0fe4bd4a17bcceb0623a467de5f69e9938ee07de3bff9851dcb94adeb03d",
    ),
    "babel-8.1.2.js": (
        "https://unpkg.com/@babel/standalone@8.1.2/babel.min.js",
        "sha384-UAlciMo5b2RD3a217OI/cCTcJmMLNExS2TKgjGbkN2s81rj5PMd2J2kKlsD3PS8x",
        "sha256-c8a2d62549b6221b23f2555e2316e4fe64e716149545785ee3281fde2a5b90e1",
    ),
}


def digest(data: bytes) -> dict[str, str]:
    return {
        "sha384": "sha384-" + base64.b64encode(hashlib.sha384(data).digest()).decode(),
        "sha256": "sha256-" + hashlib.sha256(data).hexdigest(),
    }


def check(name: str, data: bytes, sri: str, sha256: str) -> None:
    got = digest(data)
    if got["sha384"] != sri:
        raise SystemExit(f"{name}: sha384 不匹配\n  期望 {sri}\n  实际 {got['sha384']}")
    if got["sha256"] != sha256:
        raise SystemExit(f"{name}: sha256 不匹配\n  期望 {sha256}\n  实际 {got['sha256']}")


def fetch(name: str, url: str) -> bytes:
    print(f"下载 {name} <- {url}")
    with urllib.request.urlopen(url, timeout=60) as resp:
        return resp.read()


def main() -> None:
    only_check = "--check" in sys.argv[1:]
    VENDOR.mkdir(exist_ok=True)
    for name, (url, sri, sha256) in ASSETS.items():
        path = VENDOR / name
        if path.exists():
            check(name, path.read_bytes(), sri, sha256)
            print(f"OK   {name}")
            continue
        if only_check:
            raise SystemExit(f"{name}: 文件缺失（先运行 python3 fetch_vendor.py）")
        data = fetch(name, url)
        check(name, data, sri, sha256)
        path.write_bytes(data)
        print(f"OK   {name} ({len(data)} bytes)")


if __name__ == "__main__":
    main()
