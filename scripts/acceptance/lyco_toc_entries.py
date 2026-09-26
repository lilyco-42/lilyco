# -*- coding: utf-8 -*-
"""目录里那几条**排出来的**条目（`lbin office-doc` 的 `contents.entries`）。

两家把同一件事写在三处不同的地方，所以这一本按各家的位置读、不合并：

| 一问 | OOXML (docx) | ODF (odt) |
| --- | --- | --- |
| 条目容器 | `w:sdt/w:sdtContent` 里的 `w:p` | `text:table-of-content/text:index-body` 里的 `text:p` |
| 条目文字与页码 | 同一个段里，中间隔一枚 `w:tab` **记号** | 同一个 `text:a` 里，中间隔一枚 `text:tab` |
| 跳到标题的地址 | `w:hyperlink/@w:anchor`（只有名字） | `text:a/@xlink:href`（带 `#`，照写） |
| 级别 | 段自己点的样式名 `TOC1` / `TOC2` | 段上只有 `P1` 这种自动样式名，**不算级别** → 顺锚点两跳看被指那段写的 `text:outline-level` |

两条读法上的坑，都是量出来的：
1. `w:pPr/w:tabs/w:tab` 是**制表位定义**（到哪儿对齐、什么引导字符），不是段里那一下制表符 ——
   只按局部名数 `tab` 会在第一个字之前先撞到一个，整条条目被误判成「制表符之后」；
2. ODF 的页码是挂在 `text:tab` **后面的一段裸文字**（ElementTree 里是这个元素的 `tail`），
   只收元素的 `.text` 就一个字也读不到 —— 与批注那一条同一个教训。
"""
from __future__ import annotations

from typing import Optional

TEXT = "urn:oasis:names:tc:opendocument:xmlns:text:1.0"
STYLE = "urn:oasis:names:tc:opendocument:xmlns:style:1.0"
XLINK = "http://www.w3.org/1999/xlink"
WORD = "http://schemas.openxmlformats.org/wordprocessingml/2006/main"


def local(tag: str) -> str:
    return tag.rsplit("}", 1)[-1] if "}" in tag else tag


def attr_local(node, want: str) -> Optional[str]:
    """按局部名取属性（Rust 的 `attr_local` 同一条：前缀是文件自己起的）"""
    for key, value in node.attrib.items():
        if local(key) == want:
            return value
    return None


def entry_flat(node, odf: bool) -> str:
    """段里的字按文件顺序拼，记号换成对应字符（`\\t` / `\\n`）"""
    out: list = []

    def walk(one):
        if odf and one.text:
            out.append(one.text)
        for kid in one:
            name = local(kid.tag)
            if not odf and name == "pPr":
                continue  # 制表位定义不住在字里
            if name == "tab":
                out.append("\t")
            elif name == "br":
                out.append("\n")
            elif odf and name == "line-break":
                out.append("\n")
            elif odf and name == "s":
                out.append(" " * int(kid.get("{%s}c" % TEXT) or "1"))
            elif not odf and name == "t":
                out.append(kid.text or "")
            else:
                walk(kid)
            if odf and kid.tail:
                out.append(kid.tail)

    walk(node)
    return "".join(out)


def split_entry(flat: str):
    head, sep, tail = flat.partition("\t")
    return head.strip(), (tail.strip() if sep else None)


def level_from_style(style: Optional[str]) -> Optional[int]:
    """只认 `toc` 开头、后面是一串数字的样式名（`TOC1` / `toc 2`）；`P1` 那种不算"""
    if not style:
        return None
    had = style.strip()
    low = had.lower()
    if not low.startswith("toc"):
        return None
    digits = low[3:].lstrip()
    return int(digits) if digits.isdigit() else None


def bookmark_targets(root) -> dict:
    """书签的名字 → 它所在那一段（元素、点的样式、写的级别），两家三个元素名都收"""
    out = {}
    for owner in [one for one in root.iter() if local(one.tag) in ("p", "h")]:
        names = []
        for kid in owner.iter():
            if local(kid.tag) in ("bookmarkStart", "bookmark", "bookmark-start"):
                had = attr_local(kid, "name")
                if had is not None:
                    names.append(had)
        style = None
        for kid in owner:
            if local(kid.tag) == "pPr":
                for two in kid:
                    if local(two.tag) == "pStyle":
                        style = attr_local(two, "val")
        if style is None:
            style = attr_local(owner, "style-name")
        level = attr_local(owner, "outline-level")
        for name in names:
            out[name] = {"element": local(owner.tag), "style": style,
                         "outline_level": level}
    return out


def toc_entries(paras: list, odf: bool, scope: str, targets: dict) -> dict:
    rows = []
    for index, one in enumerate(paras):
        if odf:
            style = attr_local(one, "style-name")
        else:
            style = None
            for kid in one:
                if local(kid.tag) == "pPr":
                    for two in kid:
                        if local(two.tag) == "pStyle":
                            style = attr_local(two, "val")
        anchor = None
        for kid in one.iter():
            if odf and local(kid.tag) == "a":
                anchor = attr_local(kid, "href")
            elif not odf and local(kid.tag) == "hyperlink":
                anchor = attr_local(kid, "anchor")
            if anchor is not None:
                break
        text, page = split_entry(entry_flat(one, odf))
        hit = targets.get((anchor or "").lstrip("#")) if anchor else None
        target_level = (hit or {}).get("outline_level")
        if odf:
            level = int(target_level) if (target_level or "").isdigit() else None
            frm = "target-outline-level" if level is not None else None
        else:
            level = level_from_style(style)
            frm = "paragraph-style" if level is not None else None
        rows.append({
            "index": index, "text": text, "page_written": page, "style": style,
            "anchor": anchor, "target_found": hit is not None,
            "target_element": (hit or {}).get("element"),
            "target_style": (hit or {}).get("style"),
            "target_outline_level": target_level,
            "level": level, "level_from": frm,
        })
    return {
        "scope": scope,
        "paras": len(rows),
        "entries": len([one for one in rows
                        if one["page_written"] is not None or one["anchor"] is not None]),
        "with_page": len([one for one in rows if one["page_written"] is not None]),
        "with_anchor": len([one for one in rows if one["anchor"] is not None]),
        "targets_found": len([one for one in rows if one["target_found"]]),
        "levels_resolved": len([one for one in rows if one["level"] is not None]),
        "list": rows,
    }


def _direct(node, want: str) -> list:
    return [one for one in node if local(one.tag) == want]


def docx_toc_entries(root) -> dict:
    for holder in root.iter():
        if local(holder.tag) == "sdtContent":
            return toc_entries(_direct(holder, "p"), False, "sdt-content",
                               bookmark_targets(root))
    return toc_entries([], False, "sdt-content", bookmark_targets(root))


def odf_toc_entries(root) -> dict:
    for holder in root.iter():
        if local(holder.tag) == "index-body":
            return toc_entries(_direct(holder, "p"), True, "index-body",
                               bookmark_targets(root))
    return toc_entries([], True, "index-body", bookmark_targets(root))
