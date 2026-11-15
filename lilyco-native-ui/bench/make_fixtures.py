#!/usr/bin/env python3
"""从 fixtures/react-fixed.html + vendor/ 生成与实测逐字节一致的离线页面。"""
import hashlib
import re
from pathlib import Path

HERE = Path(__file__).resolve().parent
FIX = HERE / "fixtures"
V = HERE / "vendor"
GEN = FIX / ".generated"

fixed = (FIX / "react-fixed.html").read_text()
old = (FIX / "react-old-unpinned.html").read_text()
assert "react@18.3.1" in fixed and "@babel/standalone@7.28.4" in fixed
assert "lilyco-react" in fixed and 'runtime: "classic"' in fixed
assert "unpkg.com/react@18/" in old and "integrity=" not in old

assets = iter(["react-18.3.1.js", "react-dom-18.3.1.js", "babel-7.28.4.js"])
def inline(m):
    name = next(assets)
    return "<script>" + (V / name).read_text() + "</script>"

html, n = re.subn(r'<script[^>]+src="https://unpkg\.com/[^"]+"[^>]*></script>', inline, fixed)
assert n == 3, n
assert "unpkg" not in html

# 实测离线 fixture 把 registerPreset 放在 text/babel 脚本前；
# 生产固定版放在 head 里。两者都有效，这里逐字节对齐实测文件。
lines = html.splitlines(keepends=True)
i = next(i for i, l in enumerate(lines) if "Babel.registerPreset(" in l)
reg = lines.pop(i)
needle = '  <script type="text/babel" data-presets="lilyco-react">'
j = next(i for i, l in enumerate(lines) if needle in l)
lines.insert(j, reg)
html = "".join(lines)
assert html.count("Babel.registerPreset(") == 1
assert 'data-presets="lilyco-react"' in html

GEN.mkdir(exist_ok=True)
out = GEN / "react-offline.html"
out.write_text(html)
print("OFFLINE", out, len(html.encode()), "bytes", hashlib.sha256(html.encode()).hexdigest())
print("FIXED", (FIX / "react-fixed.html").stat().st_size, "bytes")
print("OLD", (FIX / "react-old-unpinned.html").stat().st_size, "bytes")
