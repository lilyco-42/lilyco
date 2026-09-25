#!/usr/bin/env python3
"""docx → markdown 的**独立读者**：与 `lilyco-binfmt/src/markdown.rs` 逐字对账。

这里的每一条规则都是从真件量出来的（`md.docx` 由 python-docx 写、每段只管一件事，
`md-lo.docx` 是 LibreOffice 的 docx → docx 重写）：两家的渲染结果一字不差，
而账本说得出「号写在段上还是样式上」这种差别。Rust 那边要抄的就是这个口径。
"""

from __future__ import annotations

import re
import zipfile
from pathlib import Path
from xml.etree import ElementTree as ET

W = "{http://schemas.openxmlformats.org/wordprocessingml/2006/main}"
R = "{http://schemas.openxmlformats.org/officeDocument/2006/relationships}"
# OOXML 说「不」的三种拼法（`w:val` 上）
OFF = ("0", "false", "none")
# 段内的硬换行先占一个不会出现在正文里的字符，拼完再换成「反斜杠 + 换行」
HARD = "\u0000"
# 一段普通正文的行首长得像结构记号吗（标题与列表项的前缀是渲染器自己加的，不算）
LEAD = re.compile(r"^(#|>|[-+] )|^\d+[.)]")


def local(tag: str) -> str:
    return tag.rsplit("}", 1)[-1]


def kids(node, want: str) -> list:
    """直接孩子里那个名字的元素（ElementTree 不给孩子指父亲，也不把空白算成元素）"""
    return [one for one in node if local(one.tag) == want]


def rels_of(parts: dict, part: str) -> dict:
    """一个部件自己的关系表：rId → 地址**按写的**原样（外部关系也是原样）"""
    at = part.rfind("/")
    rel = part[:at + 1] + "_rels/" + part[at + 1:] + ".rels"
    out = {}
    if rel not in parts:
        return out
    for one in ET.fromstring(parts[rel]):
        if local(one.tag) != "Relationship":
            continue
        out[one.attrib.get("Id")] = one.attrib.get("Target") or ""
    return out


def style_names(parts: dict) -> dict:
    out = {}
    if "word/styles.xml" not in parts:
        return out
    for one in ET.fromstring(parts["word/styles.xml"]).iter():
        if local(one.tag) != "style":
            continue
        named = kids(one, "name")
        out[one.attrib.get(W + "styleId")] = (
            named[0].attrib.get(W + "val") if named else None)
    return out


def style_lists(parts: dict) -> dict:
    """样式 id → (numId, ilvl)：段上没写 numPr 时，号可能写在它点名的那份样式上"""
    out = {}
    if "word/styles.xml" not in parts:
        return out
    for one in ET.fromstring(parts["word/styles.xml"]).iter():
        if local(one.tag) != "style":
            continue
        props = kids(one, "pPr")
        if not props:
            continue
        hit = kids(props[0], "numPr")
        if not hit:
            continue
        numid = kids(hit[0], "numId")
        ilvl = kids(hit[0], "ilvl")
        if not numid:
            continue
        out[one.attrib.get(W + "styleId")] = (
            numid[0].attrib.get(W + "val"),
            ilvl[0].attrib.get(W + "val") if ilvl else None)
    return out


def numbering_of(parts: dict):
    """(numId → abstractNumId) 与 (abstractNumId → {ilvl: numFmt})：这一问要跳三跳"""
    if "word/numbering.xml" not in parts:
        return {}, {}
    root = ET.fromstring(parts["word/numbering.xml"])
    abstract = {}
    for one in kids(root, "abstractNum"):
        levels = {}
        for lvl in kids(one, "lvl"):
            got = kids(lvl, "numFmt")
            levels[lvl.attrib.get(W + "ilvl")] = (
                got[0].attrib.get(W + "val") if got else None)
        abstract[one.attrib.get(W + "abstractNumId")] = levels
    by_num = {}
    for one in kids(root, "num"):
        hit = kids(one, "abstractNumId")
        by_num[one.attrib.get(W + "numId")] = (
            hit[0].attrib.get(W + "val") if hit else None)
    return by_num, abstract


def heading_level(para, names: dict):
    """样式名与样式 id 两种来源都认（都按 heading 打头 + 数字），再看 w:outlineLvl"""
    hit = [one for one in para.iter() if local(one.tag) == "pStyle"]
    if hit:
        sid = hit[0].attrib.get(W + "val")
        for source in (names.get(sid), sid):
            if not source:
                continue
            low = source.lower().replace("_20_", " ")
            if not (low.startswith("heading") or low.startswith("标题")):
                continue
            digits = "".join(ch for ch in source if ch.isdigit())
            return max(1, int(digits)) if digits else 1
    hit = [one for one in para.iter() if local(one.tag) == "outlineLvl"]
    if hit:
        raw = hit[0].attrib.get(W + "val")
        if raw is not None and raw.isdigit():
            return int(raw) + 1
    return None


def list_of(para, by_num: dict, abstract: dict, style_lists_map: dict):
    """返回 (深度, 这一级的 numFmt, 号是不是写在样式上)；没写 numPr 就不是列表项"""
    props = kids(para, "pPr")
    hit = kids(props[0], "numPr") if props else []
    if hit:
        numid = kids(hit[0], "numId")
        ilvl = kids(hit[0], "ilvl")
        raw_num = numid[0].attrib.get(W + "val") if numid else None
        raw_lvl = ilvl[0].attrib.get(W + "val") if ilvl else None
        depth = int(raw_lvl) if (raw_lvl or "").isdigit() else 0
        return depth, _format_of(by_num, abstract, raw_num, depth), False
    found = [one for one in para.iter() if local(one.tag) == "pStyle"]
    if not found:
        return None
    sid = found[0].attrib.get(W + "val")
    if sid not in style_lists_map:
        return None
    raw_num, raw_lvl = style_lists_map[sid]
    depth = int(raw_lvl) if (raw_lvl or "").isdigit() else 0
    return depth, _format_of(by_num, abstract, raw_num, depth), True


def _format_of(by_num: dict, abstract: dict, num_id, depth: int):
    aid = by_num.get(num_id) if num_id else None
    got = (abstract.get(aid) or {}).get(str(depth))
    return got


def esc(text: str, pipe: bool = False) -> str:
    r"""正文里的 markdown 记号按字交；竖线只在格子里才补（表外的 `|` 不是记号）"""
    marks = "\\*_`[]<>" + ("|" if pipe else "")
    out = []
    for ch in text:
        if ch in marks:
            out.append("\\")
        out.append(ch)
    return "".join(out)


def seg_text(node) -> str:
    """一串字里能进正文的部分：rPr 整块跳过，tab 与 br 还原，图不写字"""
    pieces = []
    for one in node:
        name = local(one.tag)
        if name == "rPr":
            continue
        if name == "tab":
            pieces.append(" ")
        elif name == "br":
            kind = one.attrib.get(W + "type") or "textWrapping"
            pieces.append(HARD if kind == "textWrapping" else "")
        elif name in ("t", "delText"):
            pieces.append(one.text or "")
        elif name in ("drawing", "pict", "object"):
            continue
        else:
            pieces.append(seg_text(one))
    return "".join(pieces)


def picture(node, rels: dict):
    """一张图：第一个写了 descr 的 docPr / cNvPr，与 blip 的 r:embed 查到的地址"""
    alt = ""
    embed = None
    for one in node.iter():
        name = local(one.tag)
        if not alt and name in ("docPr", "cNvPr"):
            hit = one.attrib.get("descr")
            if hit:
                alt = hit
        if embed is None and name == "blip":
            embed = one.attrib.get(R + "embed")
    return {"alt": alt, "target": rels.get(embed, "") if embed else ""}


def run_seg(node, ctx, out: list, link=None):
    props = kids(node, "rPr")
    bold = italic = False
    for one in (props[0] if props else []):
        name = local(one.tag)
        on = one.attrib.get(W + "val") not in OFF
        if name in ("b", "bCs"):
            bold = on or bold
        elif name in ("i", "iCs"):
            italic = on or italic
    text = seg_text(node)
    if text:
        out.append({"text": text, "bold": bold, "italic": italic, "link": link})
    for one in kids(node, "drawing") + kids(node, "pict"):
        out.append({"image": picture(one, ctx["rels"])})


def segments(para, ctx) -> list:
    """一段拆成一串带形状的片段：链接与图是壳，壳里的字带着壳出来"""
    out: list = []
    for one in para:
        name = local(one.tag)
        if name == "pPr":
            continue
        if name == "r":
            run_seg(one, ctx, out)
        elif name == "hyperlink":
            inside: list = []
            for kid in one:
                if local(kid.tag) == "r":
                    run_seg(kid, ctx, inside, link="")
            rid = one.attrib.get(R + "id")
            anchor = one.attrib.get(W + "anchor")
            if rid is not None:
                target = ctx["rels"].get(rid, "")
            elif anchor:
                target = "#" + anchor
            else:
                target = ""
            for slot in inside:
                slot["link"] = target
            out.extend(inside)
        elif name in ("drawing", "pict", "object"):
            out.append({"image": picture(one, ctx["rels"])})
        elif name in ("ins", "smartTag", "sdt", "sdtContent"):
            out.extend(segments(one, ctx))
        elif name == "del":
            continue
        else:
            out.extend(segments(one, ctx))
    return out


def render(segs: list, pipe: bool = False) -> str:
    """拼成 markdown：相邻同形状的并成一段（两家生产者拆 run 的习惯不同，
    同一种形状必须拼回同一个字面量，不然 `**a****b**` 那种就露出来了）"""
    merged: list = []
    for one in segs:
        if "image" in one:
            merged.append(dict(one))
            continue
        if merged and "image" not in merged[-1] and (
                merged[-1].get("bold") == one.get("bold")
                and merged[-1].get("italic") == one.get("italic")
                and merged[-1].get("link") == one.get("link")):
            merged[-1]["text"] += one["text"]
            continue
        merged.append(dict(one))
    out = []
    for one in merged:
        if "image" in one:
            out.append("![%s](%s)" % (one["image"]["alt"], one["image"]["target"]))
            continue
        text = one["text"]
        if not text.strip() and HARD not in text:
            continue
        body = esc(text, pipe)
        if one.get("bold") and one.get("italic"):
            body = "***%s***" % body
        elif one.get("bold"):
            body = "**%s**" % body
        elif one.get("italic"):
            body = "*%s*" % body
        if one.get("link") is not None:
            body = "[%s](%s)" % (body, one["link"])
        out.append(body.replace(HARD, "\\\n"))
    return "".join(out)


def cell_text(tc, ctx) -> str:
    """一个格子的字：多段用 <br> 连，竖线在这里才转义"""
    bits = []
    for one in tc:
        if local(one.tag) == "p":
            flat = render(segments(one, ctx), pipe=True).strip().replace("\n", "<br>")
            if flat:
                bits.append(flat)
    return "<br>".join(bits)


def table_md(tbl, ctx) -> str:
    rows = [[cell_text(one, ctx) for one in kids(tr, "tc")] for tr in kids(tbl, "tr")]
    if not rows:
        return ""
    width = max(len(one) for one in rows)
    for one in rows:
        one.extend([""] * (width - len(one)))
    lines = ["| " + " | ".join(rows[0]) + " |", "| " + " | ".join(["---"] * width) + " |"]
    for one in rows[1:]:
        lines.append("| " + " | ".join(one) + " |")
    return "\n".join(lines)


_OFFICE_NS = "{urn:oasis:names:tc:opendocument:xmlns:office:1.0}"
_STYLE_NS = "{urn:oasis:names:tc:opendocument:xmlns:style:1.0}"
_TEXT_NS = "{urn:oasis:names:tc:opendocument:xmlns:text:1.0}"
_TABLE_NS = "{urn:oasis:names:tc:opendocument:xmlns:table:1.0}"
_XLINK_NS = "{http://www.w3.org/1999/xlink}"
_FO_NS = "{urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0}"


def _md_local(tag: str) -> str:
    return tag.rsplit("}", 1)[-1]


def odf_styles(parts: dict):
    """两份件都走，**先到先得**（与编号那一条同一口径）

    实测：`text:span` 点的字符样式在 content.xml（自动样式）与 styles.xml（命名样式）里都有，
    列表样式 `text:list-style` 全在 styles.xml（这份语料 300 条、content 里 0 条）。
    """
    chars = {}
    lists = {}
    for part in ("content.xml", "styles.xml"):
        if part not in parts:
            continue
        for one in ET.fromstring(parts[part]).iter():
            kind = _md_local(one.tag)
            name = one.attrib.get(_STYLE_NS + "name") or one.attrib.get(_TEXT_NS + "name")
            if kind == "style" and one.attrib.get(_STYLE_NS + "family") == "text" and name:
                if name in chars:
                    continue
                props = [ch for ch in one if _md_local(ch.tag) == "text-properties"]
                # ElementTree 把前缀换成命名空间 URI：`fo:font-weight` 在 attrib 里是 `{...}font-weight`
                weight = props[0].attrib.get(_FO_NS + "font-weight") if props else None
                style = props[0].attrib.get(_FO_NS + "font-style") if props else None
                chars[name] = (
                    weight is not None and weight not in ("normal", "0"),
                    style is not None and style not in ("normal", "none", "0"),
                )
            elif kind == "list-style" and name and name not in lists:
                levels = {}
                for lvl in one:
                    if _md_local(lvl.tag) not in ("list-level-style-number",
                                                  "list-level-style-bullet"):
                        continue
                    depth = lvl.attrib.get(_TEXT_NS + "level")
                    if depth is not None:
                        levels[depth] = "bullet" if _md_local(lvl.tag).endswith("bullet") else "number"
                lists[name] = levels
    return chars, lists


def odf_heading_level(node):
    """`text:h` 的层级写在属性上；没写就当一段普通正文（这一族没有别的凭据可查）"""
    raw = node.attrib.get(_TEXT_NS + "outline-level")
    if raw is not None and raw.isdigit():
        return max(1, int(raw))
    return None


def _md_seg(text, bold, italic, link):
    return {"text": text, "bold": bold, "italic": italic, "link": link}


_MD_LEAF = ("s", "tab", "line-break", "frame", "object", "annotation", "note",
            "tracked-changes", "soft-page-break")


def odf_inline(node, ctx, out: list, link=None, bold=False, italic=False):
    """一棵子树按文档顺序摊成片段：字（含 `text:s` / `text:tab` / 换行记号）、span 的粗斜、链接、图

    ODF 的字挂在元素的 `.text` 与孩子们的 `.tail` 上（不是各自一个孩子），所以这三段都要按
    「谁在说什么话」各归其位：孩子的字带孩子的形状，`tail` 是**父亲**的话，用父亲的形状交。

    批注（`text:annotation`）、注（`text:note`）、修订表（`text:tracked-changes`）整块不算正文的字
    —— 与 docx 那一本同一口径（那边的这些字住在**别的部件**里，本来也不在正文）；
    `text:soft-page-break` 是渲染时落下的位置，也不写字。
    """
    if node.text:
        out.append(_md_seg(node.text, bold, italic, link))
    for one in node:
        kind = _md_local(one.tag)
        stats = ctx["stats"]
        child_bold, child_italic, child_link = bold, italic, link
        if kind in ("annotation", "note", "tracked-changes"):
            if kind == "annotation":
                stats["annotations_dropped"] += 1
            else:
                stats["notes_dropped"] += 1
        elif kind == "soft-page-break":
            pass
        elif kind == "s":
            raw = one.attrib.get(_TEXT_NS + "c") or "1"
            stats["space_markers"] += 1
            # 上限 64 与 markdown.rs 同一条：记号自己说几个空格就展开几个，但别让一枚属性撑爆内存
            out.append(_md_seg(" " * (min(int(raw), 64) if raw.isdigit() else 1),
                               bold, italic, link))
        elif kind == "tab":
            out.append(_md_seg("\t", bold, italic, link))
        elif kind == "line-break":
            out.append(_md_seg(HARD, bold, italic, link))
        elif kind == "span":
            name = one.attrib.get(_TEXT_NS + "style-name")
            got = ctx["chars"].get(name) if name else None
            if got is None:
                stats["spans_unresolved"] += 1
                got = (False, False)
            child_bold = bold or got[0]
            child_italic = italic or got[1]
        elif kind == "a":
            child_link = one.attrib.get(_XLINK_NS + "href") or ""
            stats["links"] += 1
        elif kind in ("frame", "object"):
            image = ""
            for hit in one.iter():
                if _md_local(hit.tag) == "image":
                    image = hit.attrib.get(_XLINK_NS + "href") or ""
                    break
            stats["images"] += 1
            out.append({"image": {"alt": "", "target": image}})
        if kind not in _MD_LEAF:
            # `text:span` 与 `text:a` 是壳：壳里的字带着算好的形状递归进来（叶子只出记号）
            odf_inline(one, ctx, out, child_link, child_bold, child_italic)
        # `.tail` 是**这个孩子之后**的字，属于父亲：无论孩子是记号（`text:s` / `text:tab` /
        # `text:line-break`）、图，还是被整块跳过的批注，尾巴都得留下 —— 漏了它，
        # 「两处空格 之间是一个记号」会只剩前半句（真件量到的）
        if one.tail:
            out.append(_md_seg(one.tail, bold, italic, link))


def odf_cell_text(tc, ctx) -> str:
    bits = []
    for one in tc:
        if _md_local(one.tag) in ("p", "h"):
            made = render(odf_segments_collect(one, ctx), pipe=True).strip()
            flat = made.replace("\n", "<br>")
            if flat:
                bits.append(flat)
    return "<br>".join(bits)


def odf_segments_collect(node, ctx) -> list:
    out: list = []
    odf_inline(node, ctx, out)
    return out


def odf_table_md(tbl, ctx) -> str:
    rows = []
    for tr in tbl:
        if _md_local(tr.tag) != "table-row":
            continue
        cells = []
        for tc in tr:
            if _md_local(tc.tag) not in ("table-cell", "covered-table-cell"):
                continue
            if _md_local(tc.tag) == "covered-table-cell":
                ctx["stats"]["covered_cells"] += 1
            rep = tc.attrib.get(_TABLE_NS + "number-columns-repeated") or "1"
            times = int(rep) if rep.isdigit() else 1
            made = odf_cell_text(tc, ctx)
            cells.extend([made] * min(times, 64))
            if times > 1:
                ctx["stats"]["repeated_spans"] += times - 1
        rows.append(cells)
    if not rows:
        return ""
    width = max(len(one) for one in rows)
    for one in rows:
        one.extend([""] * (width - len(one)))
    lines = ["| " + " | ".join(rows[0]) + " |", "| " + " | ".join(["---"] * width) + " |"]
    for one in rows[1:]:
        lines.append("| " + " | ".join(one) + " |")
    return "\n".join(lines)


def odf_markdown(path: Path, budget: int = 20000) -> dict:
    """一份 odt 的结构渲染：与 docx 那一本同一个键形状，只是层级与记号是另一族的写法"""
    with zipfile.ZipFile(path) as box:
        parts = {one.filename: box.read(one.filename) for one in box.infolist()}
    if "content.xml" not in parts:
        return {"family": "odf", "available": False}
    root = ET.fromstring(parts["content.xml"])
    body = None
    for one in root.iter():
        if one.tag == _OFFICE_NS + "text":
            body = one
            break
    chars, lists = odf_styles(parts)
    ctx = {"rels": {}, "styles": {}, "chars": chars, "lists": lists,
           "stats": {
               "paragraphs": 0, "headings": 0, "list_items": 0, "bullet_items": 0,
               "ordered_items": 0, "unresolved_fmt": 0,
               "tables": 0, "table_rows": 0, "empty_dropped": 0,
               "lists_named": 0, "lists_unnamed": 0, "spans_unresolved": 0,
               "annotations_dropped": 0, "notes_dropped": 0, "space_markers": 0,
               "links": 0, "images": 0, "covered_cells": 0, "repeated_spans": 0,
           }}
    blocks: list = []
    stats = ctx["stats"]

    def walk(node, depth: int, list_name):
        for one in node:
            kind = _md_local(one.tag)
            if kind == "h":
                emit(one, depth, list_name, heading=True)
            elif kind == "p":
                emit(one, depth, list_name, heading=False)
            elif kind == "list":
                name = one.attrib.get(_TEXT_NS + "style-name")
                stats["lists_named" if name else "lists_unnamed"] += 1
                walk(one, depth + 1, name or list_name)
            elif kind == "list-item":
                walk(one, depth, list_name)
            elif kind == "table":
                made = odf_table_md(one, ctx)
                if made:
                    stats["tables"] += 1
                    stats["table_rows"] += len([x for x in one
                                                if _md_local(x.tag) == "table-row"])
                    blocks.append((False, made))
            else:
                walk(one, depth, list_name)

    def emit(par, depth: int, list_name, heading: bool):
        text = render(odf_segments_collect(par, ctx)).strip()
        if not text:
            stats["empty_dropped"] += 1
            return
        level = odf_heading_level(par) if heading else None
        if heading and level:
            stats["headings"] += 1
            blocks.append((False, "#" * level + " " + text))
            return
        if depth > 0:
            stats["list_items"] += 1
            fmt = (ctx["lists"].get(list_name or "") or {}).get(str(depth))
            indent = "  " * (depth - 1)
            if fmt == "number":
                stats["ordered_items"] += 1
                blocks.append((True, indent + "1. " + text))
            else:
                if fmt is None:
                    stats["unresolved_fmt"] += 1
                stats["bullet_items"] += 1
                blocks.append((True, indent + "- " + text))
            return
        stats["paragraphs"] += 1
        blocks.append((False, "\\" + text if LEAD.match(text) else text))

    if body is not None:
        walk(body, 0, None)
    pieces = []
    for index, (is_list, block) in enumerate(blocks):
        if index:
            pieces.append("\n" if is_list and blocks[index - 1][0] else "\n\n")
        pieces.append(block)
    text = ("".join(pieces) + "\n") if blocks else ""
    chars_count = len(text)
    return {
        "family": "odf",
        "available": True,
        "text": text[:budget],
        "chars": chars_count,
        "cut": chars_count > budget,
        "blocks": len(blocks),
        **stats,
    }


def docx_markdown(path: Path, budget: int = 20000) -> dict:
    """一份 docx 的结构渲染（Rust 那一本 `crate::markdown::docx` 的对照）"""
    with zipfile.ZipFile(path) as box:
        parts = {one.filename: box.read(one.filename) for one in box.infolist()}
    if "word/document.xml" not in parts:
        return {"family": "ooxml", "available": False}
    root = ET.fromstring(parts["word/document.xml"])
    ctx = {"rels": rels_of(parts, "word/document.xml"), "styles": style_names(parts)}
    by_num, abstract = numbering_of(parts)
    lists = style_lists(parts)
    body = kids(root, "body")
    blocks: list = []
    stats = {
        "paragraphs": 0, "headings": 0, "list_items": 0, "bullet_items": 0,
        "ordered_items": 0, "list_from_style": 0, "unresolved_fmt": 0,
        "tables": 0, "table_rows": 0, "empty_dropped": 0,
    }
    for one in (body[0] if body else root):
        kind = local(one.tag)
        if kind == "p":
            text = render(segments(one, ctx)).strip()
            if not text:
                stats["empty_dropped"] += 1
                continue
            level = heading_level(one, ctx["styles"])
            if level:
                stats["headings"] += 1
                blocks.append((False, "#" * level + " " + text))
                continue
            item = list_of(one, by_num, abstract, lists)
            if item:
                depth, fmt, from_style = item
                stats["list_items"] += 1
                if from_style:
                    stats["list_from_style"] += 1
                indent = "  " * depth
                if fmt is None:
                    stats["unresolved_fmt"] += 1
                    stats["bullet_items"] += 1
                    blocks.append((True, indent + "- " + text))
                elif fmt == "bullet":
                    stats["bullet_items"] += 1
                    blocks.append((True, indent + "- " + text))
                else:
                    stats["ordered_items"] += 1
                    blocks.append((True, indent + "1. " + text))
                continue
            stats["paragraphs"] += 1
            blocks.append((False, "\\" + text if LEAD.match(text) else text))
        elif kind == "tbl":
            made = table_md(one, ctx)
            if made:
                stats["tables"] += 1
                stats["table_rows"] += len(kids(one, "tr"))
                blocks.append((False, made))
    pieces = []
    for index, (is_list, block) in enumerate(blocks):
        if index:
            pieces.append("\n" if is_list and blocks[index - 1][0] else "\n\n")
        pieces.append(block)
    text = ("".join(pieces) + "\n") if blocks else ""
    chars = len(text)
    shown = text[:budget]
    return {
        "family": "ooxml",
        "available": True,
        "text": shown,
        "chars": chars,
        "cut": chars > budget,
        "blocks": len(blocks),
        **stats,
    }
