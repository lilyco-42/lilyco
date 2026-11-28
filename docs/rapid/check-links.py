#!/usr/bin/env python3
"""校验 docs/rapid 下的相对 markdown 链接与页内锚点。

用法:
    python3 docs/rapid/check-links.py

退出码 0 = 全部可达；1 = 有断链或锚点不存在。
只检查以 .md 结尾的相对链接；外链与其它后缀跳过。
"""

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent
LINK = re.compile(r"\]\(([^)\s]+?)(?:#([^)\s]+))?\)")


def slug(title: str) -> str:
    s = title.strip().lower()
    s = re.sub(r"[^\w\s-]", "", s, flags=re.UNICODE)
    s = re.sub(r"\s+", "-", s)
    return s


def anchors(path: pathlib.Path) -> set[str]:
    found = set()
    for line in path.read_text(encoding="utf-8").splitlines():
        m = re.match(r"^(#{1,6})\s+(.*)$", line)
        if m:
            found.add(slug(m.group(2)))
    return found


def main() -> int:
    files = sorted(ROOT.rglob("*.md"))
    total = 0
    with_anchor = 0
    bad = []
    for src in files:
        text = src.read_text(encoding="utf-8")
        for m in LINK.finditer(text):
            target = m.group(1)
            if target.startswith(("http://", "https://", "mailto:")):
                continue
            if not target.endswith(".md"):
                continue
            if "*" in target:
                continue
            total += 1
            dest = (src.parent / target).resolve()
            if not dest.exists():
                bad.append((src.relative_to(ROOT), target, "文件不存在"))
                continue
            frag = m.group(2)
            if frag:
                with_anchor += 1
                if frag not in anchors(dest):
                    bad.append((src.relative_to(ROOT), f"{target}#{frag}", "锚点不存在"))
    print(f"检查 {len(files)} 个文件，{total} 条相对链接（{with_anchor} 条含锚点）")
    for src, target, reason in bad:
        print(f"  ✗ {src} -> {target} ({reason})")
    if bad:
        print(f"失败：{len(bad)} 条断链")
        return 1
    print("通过：全部可达")
    return 0


if __name__ == "__main__":
    sys.exit(main())