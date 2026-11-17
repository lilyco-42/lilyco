#!/usr/bin/env python3
"""生成 13 变体 React 离线内联页与未锁版本的对照页。"""
import hashlib
import re
from pathlib import Path

HERE = Path(__file__).resolve().parent
FIX = HERE / "fixtures"
V = HERE / "vendor"
GEN = FIX / ".generated"
GEN.mkdir(exist_ok=True)

fixed = (FIX / "all-13-react-fixed.html").read_text()
assert "react@18.3.1" in fixed and "@babel/standalone@7.28.4" in fixed
assert "lilyco-react" in fixed
assert '"类型":"计算器"' in fixed and '"类型":"按钮"' in fixed

assets = iter(["react-18.3.1.js", "react-dom-18.3.1.js", "babel-7.28.4.js"])
def inline(m):
    return "<script>" + (V / next(assets)).read_text() + "</script>"

html, n = re.subn(r'<script[^>]+src="https://unpkg\.com/[^"]+"[^>]*></script>', inline, fixed)
assert n == 3, n
assert "unpkg" not in html
assert html.count("Babel.registerPreset(") == 1
assert 'data-presets="lilyco-react"' in html
out = GEN / "all-13-react-offline.html"
out.write_text(html)
print("ALL13_OFFLINE", out, len(html.encode()), "bytes", hashlib.sha256(html.encode()).hexdigest())

# 复刻修复前的未锁版本：三条浮点 CDN、无 SRI、无显式 classic preset。
old = re.sub(
    r'<script[^>]+src="https://unpkg\.com/react@18\.3\.1/umd/react\.production\.min\.js"[^>]*></script>',
    '<script crossorigin src="https://unpkg.com/react@18/umd/react.production.min.js"></script>', fixed)
old = re.sub(
    r'<script[^>]+src="https://unpkg\.com/react-dom@18\.3\.1/umd/react-dom\.production\.min\.js"[^>]*></script>',
    '<script crossorigin src="https://unpkg.com/react-dom@18/umd/react-dom.production.min.js"></script>', old)
old = re.sub(
    r'<script[^>]+src="https://unpkg\.com/@babel/standalone@7\.28\.4/babel\.min\.js"[^>]*></script>',
    '<script src="https://unpkg.com/@babel/standalone/babel.min.js"></script>', old)
old = "\n".join(l for l in old.splitlines() if "Babel.registerPreset(" not in l) + "\n"
old = old.replace('data-presets="lilyco-react"', 'data-presets="react"')
assert "integrity=" not in old and 'unpkg.com/react@18/' in old
assert 'data-presets="react"' in old and "lilyco-react" not in old
old_out = FIX / "all-13-react-old-unpinned.html"
old_out.write_text(old)
print("ALL13_OLD", old_out, len(old.encode()), "bytes", hashlib.sha256(old.encode()).hexdigest())
print("ALL13_FIXED", (FIX / "all-13-react-fixed.html").stat().st_size, "bytes")
