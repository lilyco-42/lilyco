#!/usr/bin/env python3
"""办公文件的**独立读者**：只用 Python 标准库，把 lbin office-* 该报出来的事实算一遍。

它有两个用途，都跟「证据」有关：
1. 生成 Rust 测试里那些断言数字的来源 —— 断言不许是我照着 Rust 代码的结果抄的，
   必须是另一个语言、另一个实现从同一批字节里独立算出来的；两边不一致就要查是谁错。
2. CI 上把 `lbin office-*` 的 JSON 与这里的结果逐项对比（`office_probe.py`）。

覆盖：OOXML（docx / xlsx / pptx）、ODF（odt / ods / odp）、MS-CFB 复合文档
（.doc / .xls / .ppt 的流表与 OLE 属性集）、RTF。全部只读，不写任何文件。
"""

from __future__ import annotations

import json
import struct
import sys
import zipfile
from pathlib import Path
from xml.etree import ElementTree as ET

import sys as _sys
from pathlib import Path as _Path

_sys.path.insert(0, str(_Path(__file__).resolve().parent))
from lyco_rtf import rtf_info, rtf_text  # 独立 RTF 实现，与 lilyco-binfmt/src/rtf.rs 对账
from lyco_formats import xlsx_formats  # xlsx 数字格式的第二读者（与 numfmt.rs 对账）
from lyco_legacy import biff_workbook, doc_pieces, ppt_text  # 遗留格式的第二读者
import lyco_pdf  # PDF 那份读者：对象表 + 对象流 + 字符串三件事（lbin office-pdf 对账）

END = "END"  # CFB 的链结束标记
FREE = "FREE"


# ---------------------------------------------------------------- OOXML / ODF 的包
def opc_read(path: Path) -> dict:
    """包里有什么、每个部件声明成什么类型、关系指着谁"""
    with zipfile.ZipFile(path) as box:
        names = [one.filename for one in box.infolist()]
        parts = {one.filename: box.read(one.filename) for one in box.infolist()}
    out = {"parts": sorted(names), "count": len(names), "content_types": {}, "rels": {}}
    ct = parts.get("[Content_Types].xml")
    if ct is not None:
        root = ET.fromstring(ct)
        ns = "{http://schemas.openxmlformats.org/package/2006/content-types}"
        for one in root.findall(f"{ns}Default"):
            out["content_types"]["*." + one.get("Extension").lower()] = one.get("ContentType")
        for one in root.findall(f"{ns}Override"):
            out["content_types"][one.get("PartName").lstrip("/")] = one.get("ContentType")
    for name, data in parts.items():
        if not name.endswith(".rels"):
            continue
        root = ET.fromstring(data)
        ns = "{http://schemas.openxmlformats.org/package/2006/relationships}"
        base = Path(name).parent.parent  # `word/_rels/document.xml.rels` → `word`
        host = name[: -len(".rels")] if base.name == "_rels" else ""
        listed = []
        for one in root.findall(f"{ns}Relationship"):
            target = one.get("Target")
            mode = one.get("TargetMode", "Internal")
            if mode == "Internal":
                if target.startswith("/"):
                    resolved = target.lstrip("/")
                else:
                    prefix = "" if str(base) == "." else str(base).replace("\\", "/") + "/"
                    resolved = (prefix + target).replace("/./", "/")
                listed.append({"id": one.get("Id"), "type": one.get("Type"), "target": resolved})
            else:
                listed.append({"id": one.get("Id"), "type": one.get("Type"), "target": target,
                               "external": True})
        out["rels"][host or name] = listed
    return out


def xml_local(tag: str) -> str:
    return tag.rsplit("}", 1)[-1]


def walk_text(root, keep: tuple[str, ...], joiner: str = "\n") -> list[str]:
    """按「局部名在 keep 里」收集文本，返回段落列表（每个匹配元素一段）"""
    out: list[str] = []
    for node in root.iter():
        if xml_local(node.tag) in keep:
            text = "".join(node.itertext())
            out.append(text)
    return out


def local_attr(node, want: str):
    """按**局部名**取属性：OOXML 的属性都带 w: 前缀，前缀不是契约的一部分。"""
    for key, value in node.attrib.items():
        if key.rsplit("}", 1)[-1].rsplit(":", 1)[-1] == want:
            return value
    return None


def side_texts(parts: dict) -> list:
    """正文之外的部件：批注 / 脚注 / 尾注 / 页眉页脚 —— 与 Rust 那边同一套判据。

    脚注与尾注部件里白坐着两条「分隔符」（`w:type="separator"` 与
    `"continuationSeparator"`，Word 与 LibreOffice 都写）：它们不是文档里的注，
    两边都不交 —— 留着它们，`--keep-empty` 一开就凭空多出两条空脚注。
    """
    out: list = []
    names = sorted(parts)
    for name in names:
        base = name.rsplit("/", 1)[-1]
        if not name.startswith("word/") or not base.endswith(".xml"):
            continue
        stem = base[:-4]
        if stem == "footnotes":
            what, owner = "footnote", "footnote"
        elif stem == "endnotes":
            what, owner = "endnote", "endnote"
        elif stem == "comments":
            what, owner = "comment", "comment"
        elif stem.startswith("header"):
            what, owner = "header", None
        elif stem.startswith("footer"):
            what, owner = "footer", None
        else:
            continue
        root = ET.fromstring(parts[name])
        holders = [one for one in root.iter() if xml_local(one.tag) == owner] if owner else [root]
        for had in holders:
            if owner and (local_attr(had, "type") or "") in (
                "separator",
                "continuationSeparator",
            ):
                continue
            author = local_attr(had, "author") if owner else None
            stamp = local_attr(had, "date") if owner else None
            for para in [one for one in had.iter() if xml_local(one.tag) == "p"]:
                out.append(
                    {
                        "from": what,
                        "part": name,
                        "author": author,
                        "date": stamp,
                        "text": "".join(para.itertext()),
                    }
                )
    return out


def tally_of(texts: list) -> dict:
    """「多少字」这份账的口径：字符数、去空白的字符数、按空白切的词数。

    中文一整段可能只算一个「词」—— 那不是数错，是这个口径对中文意义有限，
    所以要跟 characters_no_spaces 一起看。与 Rust 那边 `Tally` 一条一条对：
    Python 的 str.split() == Rust 的 split_whitespace()，
    str.isspace() == char::is_whitespace()（对文件里真出现的那些字符）。
    """
    joined = [one or "" for one in texts]
    return {
        "characters": sum(len(one) for one in joined),
        "characters_no_spaces": sum(
            sum(1 for ch in one if not ch.isspace()) for one in joined
        ),
        "words_by_space": sum(len(one.split()) for one in joined),
    }


def _note_part_count(parts: dict, name: str, tag: str) -> int:
    """脚注 / 尾注部件里有几条**注**：分隔符那两条不算。

    Word 与 LibreOffice 都会在这个部件里写 `w:type="separator"` 与
    `"continuationSeparator"` 各一条（正文是空的）—— 按 `w:footnote` 元素个数
    数就会把一份两条脚注的文档报成四条。部件不在包里才是零个。
    """
    if name not in parts:
        return 0
    root = ET.fromstring(parts[name])
    return len(
        [
            one
            for one in root.iter()
            if xml_local(one.tag) == tag
            and (local_attr(one, "type") or "")
            not in ("separator", "continuationSeparator")
        ]
    )


def docx_facts(path: Path) -> dict:
    with zipfile.ZipFile(path) as box:
        names = [one.filename for one in box.infolist()]
        parts = {one.filename: box.read(one.filename) for one in box.infolist()}
    body = ET.fromstring(parts["word/document.xml"])
    paras = [one for one in body.iter() if xml_local(one.tag) == "p"]
    tables = [one for one in body.iter() if xml_local(one.tag) == "tbl"]
    rows = [one for one in body.iter() if xml_local(one.tag) == "tr"]
    cells = [one for one in body.iter() if xml_local(one.tag) == "tc"]
    drawings = [one for one in body.iter() if xml_local(one.tag) == "drawing"]
    breaks = [one for one in body.iter() if xml_local(one.tag) == "br"]
    styles: dict[str, int] = {}
    headings: list[tuple[int, str]] = []
    for one in paras:
        st = None
        for node in one.iter():
            if xml_local(node.tag) == "pStyle":
                st = node.get(
                    "{http://schemas.openxmlformats.org/wordprocessingml/2006/main}val"
                )
                break
        if st:
            styles[st] = styles.get(st, 0) + 1
            if st.lower().startswith("heading") or st.startswith("标题"):
                level = "".join(ch for ch in st if ch.isdigit()) or "1"
                headings.append((int(level), "".join(one.itertext())))
    links = [
        {
            "id": one.get("{http://schemas.openxmlformats.org/officeDocument/2006/relationships}id"),
            "text": "".join(node.text or "" for node in one.iter() if xml_local(node.tag) == "t"),
        }
        for one in body.iter()
        if xml_local(one.tag) == "hyperlink"
    ]
    rels = ET.fromstring(parts["word/_rels/document.xml.rels"])
    rns = "{http://schemas.openxmlformats.org/package/2006/relationships}"
    rel_map = {one.get("Id"): (one.get("Type").rsplit("/", 1)[-1], one.get("Target"),
                              one.get("TargetMode", "Internal")) for one in rels.findall(f"{rns}Relationship")}
    paragraphs = ["".join(one.itertext()) for one in paras]
    comments = 0
    if "word/comments.xml" in parts:
        c = ET.fromstring(parts["word/comments.xml"])
        comments = len([one for one in c.iter() if xml_local(one.tag) == "comment"])
    out = {
        "paragraph_count": len(paras),
        "paragraphs": paragraphs,
        "tables": len(tables),
        "table_rows": len(rows),
        "table_cells": len(cells),
        "drawings": len(drawings),
        "breaks": len(breaks),
        "styles": styles,
        "headings": [{"level": a, "text": b} for a, b in headings],
        "hyperlinks": [
            {
                "id": one["id"],
                "text": one["text"],
                "target": rel_map.get(one["id"], (None, None, None))[1],
                "external": rel_map.get(one["id"], (None, None, "Internal"))[2] == "External",
            }
            for one in links
        ],
        "media": sorted(one for one in names if one.startswith("word/media/")),
        "comments": comments,
        "side_texts": side_texts(parts),
        "sections": len([one for one in body.iter() if xml_local(one.tag) == "sectPr"]),
        # 部件在 ≠ 文档用了编号：notes.docx 带着 numbering.xml，正文里一个 numPr 都没有
        "has_numbering": any(xml_local(one.tag) == "numPr" for one in body.iter()),
        "numbering_part": "word/numbering.xml" in parts,
        "has_settings": "word/settings.xml" in parts,
        "has_styles_part": "word/styles.xml" in parts,
        "has_font_table": "word/fontTable.xml" in parts,
        "footnotes": _note_part_count(parts, "word/footnotes.xml", "footnote"),
        "endnotes": _note_part_count(parts, "word/endnotes.xml", "endnote"),
        "text": "\n".join(one for one in paragraphs if one),
        # 口径与 Rust 那边一致：每段先 trim 再数（run_text 会 trim）
        "statistics": {"ours": tally_of([one.strip() for one in paragraphs])},
    }
    return out


def xlsx_facts(path: Path) -> dict:
    parts = {}
    with zipfile.ZipFile(path) as box:
        names = [one.filename for one in box.infolist()]
        parts = {one.filename: box.read(one.filename) for one in box.infolist()}
    wb = ET.fromstring(parts["xl/workbook.xml"])
    sheets = []
    for one in wb.iter():
        if xml_local(one.tag) == "sheet":
            sheets.append(
                {
                    "name": one.get("name"),
                    "sheetId": one.get("sheetId"),
                    "state": one.get("state", "visible"),
                    "rid": one.get(
                        "{http://schemas.openxmlformats.org/officeDocument/2006/relationships}id"
                    ),
                }
            )
    shared = 0
    if "xl/sharedStrings.xml" in parts:
        sst = ET.fromstring(parts["xl/sharedStrings.xml"])
        shared = len([one for one in sst.iter() if xml_local(one.tag) == "si"])
    cells = 0
    formulas = 0
    numbers = 0
    strings_inline = 0
    merged = 0
    dims: dict[str, str] = {}
    for name in sorted(parts):
        if not (name.startswith("xl/worksheets/sheet") and name.endswith(".xml")):
            continue
        root = ET.fromstring(parts[name])
        local = name.rsplit("/", 1)[-1][: -len(".xml")]
        for one in root.iter():
            tag = xml_local(one.tag)
            if tag == "dimension":
                dims[local] = one.get("ref", "")
            elif tag == "c":
                cells += 1
                t = one.get("t")
                if t == "s":
                    pass  # 共享字符串索引：值本身在 sharedStrings 里，这里只数格子
                elif t == "inlineStr":
                    strings_inline += 1
                elif t in (None, "n"):
                    if has_local_child(one, "v"):
                        numbers += 1
                if has_local_child(one, "f"):
                    formulas += 1
            elif tag == "mergeCell":
                merged += 1
    return {
        "sheets": sheets,
        "sheet_count": len(sheets),
        "shared_strings": shared,
        "cells": cells,
        "formula_cells": formulas,
        "numeric_cells": numbers,
        "merged": merged,
        "dimensions": dims,
        "defined_names": len([
            one
            for one in wb.iter()
            if xml_local(one.tag) == "definedName"
        ]),
        "tables": len([one for one in names if one.startswith("xl/tables/")]),
        "media": sorted(one for one in names if one.startswith("xl/media/")),
        "external_links": sorted(one for one in names if one.startswith("xl/externalLinks/")),
        "styles_part": "xl/styles.xml" in parts,
        "calc_chain": "xl/calcChain.xml" in parts,
    }


def pptx_facts(path: Path) -> dict:
    with zipfile.ZipFile(path) as box:
        names = [one.filename for one in box.infolist()]
        parts = {one.filename: box.read(one.filename) for one in box.infolist()}
    slides = sorted(
        one
        for one in names
        if one.startswith("ppt/slides/slide") and one.endswith(".xml")
    )
    out_slides = []
    for name in slides:
        root = ET.fromstring(parts[name])
        texts = ["".join(one.itertext()) for one in root.iter() if xml_local(one.tag) == "t"]
        shapes = [one for one in root.iter() if xml_local(one.tag) == "sp"]
        pics = [one for one in root.iter() if xml_local(one.tag) == "pic"]
        tables = [one for one in root.iter() if xml_local(one.tag) == "graphicFrame"]
        placeholders = []
        for one in shapes:
            ph = None
            for node in one.iter():
                if xml_local(node.tag) == "ph":
                    ph = node.get("type") or "title"
                    break
            placeholders.append(ph)
        notes = ""
        stem = name[: -len(".xml")]
        note_name = stem.replace("/slides/", "/notesSlides/notesSlide")
        rels = parts.get(f"{stem[: stem.rindex('/')]}/_rels/{stem[stem.rindex('/') + 1 :]}.xml.rels")
        if rels is not None:
            r = ET.fromstring(rels)
            targets = [
                one.get("Target")
                for one in r.iter()
                if one.tag.endswith("Relationship") and "notesSlide" in (one.get("Target") or "")
            ]
            if targets:
                candidate = "ppt/notesSlides/" + targets[0].split("/")[-1]
                if candidate in parts:
                    nroot = ET.fromstring(parts[candidate])
                    notes = " ".join(
                        "".join(one.itertext()) for one in nroot.iter() if xml_local(one.tag) == "t"
                    )
        out_slides.append(
            {
                "part": name,
                "title": texts[0] if texts else "",
                "texts": texts,
                "shapes": len(shapes),
                "pictures": len(pics),
                "graphic_frames": len(tables),
                "placeholders": placeholders,
                "notes": notes.strip(),
            }
        )
    pres = ET.fromstring(parts["ppt/presentation.xml"])
    size = ""
    for one in pres.iter():
        if xml_local(one.tag) == "sldSz":
            size = f'{one.get("cx")}x{one.get("cy")}:{one.get("type", "")}'
    sld_master_ids = [
        one.get("{http://schemas.openxmlformats.org/officeDocument/2006/relationships}id")
        for one in pres.iter()
        if xml_local(one.tag) == "sldMasterId"
    ]
    return {
        "slide_count": len(slides),
        "slides": out_slides,
        "slide_size": size,
        "masters": sorted(one for one in names if one.startswith("ppt/slideMasters/slideMaster")),
        "master_refs": len(sld_master_ids),
        "layouts": sorted(one for one in names if one.startswith("ppt/slideLayouts/slideLayout")),
        "media": sorted(one for one in names if one.startswith("ppt/media/")),
        "theme": sorted(one for one in names if one.startswith("ppt/theme/")),
        "notesSlides": sorted(one for one in names if one.startswith("ppt/notesSlides/notesSlide")),
        "fonts": sorted(one for one in names if one.startswith("ppt/fonts/")),
        "transitions": len([
            one
            for name in slides
            for one in ET.fromstring(parts[name]).iter()
            if xml_local(one.tag) == "transition"
        ]),
    }


def of_local(node, want: str):
    """ODF 属性按局部名取，但躲开 LibreOffice 抄的那份 `calcext:` 副本
    （命名空间是 documentfoundation 的实验区，值是从正式那份抄来的）。
    """
    for key, value in node.attrib.items():
        if key.rsplit("}", 1)[-1] != want or "documentfoundation" in key:
            continue
        return value
    return None


def count_local(root, want: str) -> int:
    return sum(1 for one in root.iter() if xml_local(one.tag) == want)


def style_counts(paras: list) -> dict:
    """样式名 → 用了它几段（口径与 `office-doc` 的 styles 字段一致）"""
    out: dict[str, int] = {}
    for one in paras:
        name = of_local(one, "style-name")
        if name:
            out[name] = out.get(name, 0) + 1
    return dict(sorted(out.items()))


def odt_structure(path: Path) -> dict:
    """ODF 文字的结构账：口径与 `office-doc` 的 ODT 分支一条一条对（表格里也算段）。
    正文位置在 office:body 里的 office:text，取不到就退回全树。
    """
    with zipfile.ZipFile(path) as box:
        parts = {one.filename: box.read(one.filename) for one in box.infolist()}
    root = ET.fromstring(parts["content.xml"])
    body = None
    for one in root.iter():
        if xml_local(one.tag) == "body":
            for kid in one:
                if xml_local(kid.tag) == "text":
                    body = kid
                    break
        if body is not None:
            break
    body = body if body is not None else root

    def texts(node) -> str:
        return odf_para_text(node).strip()

    paras = body_paragraphs(body)
    notes_found = [one for one in body.iter() if xml_local(one.tag) == "note"]
    statistic = {}
    if "meta.xml" in parts:
        for one in ET.fromstring(parts["meta.xml"]).iter():
            if xml_local(one.tag) == "document-statistic":
                statistic = {key.rsplit("}", 1)[-1]: value for key, value in one.attrib.items()}
    return {
        "paragraphs": len(paras),
        "empty_paragraphs": sum(1 for one in paras if not texts(one)),
        # 标题在 ODF 里是 text:h 不是 text:p，字数那份要一起算（与 Rust 的 Tally 同一条）
        "statistics": {
            "ours": tally_of(
                [texts(one) for one in paras]
                + [texts(one) for one in body.iter() if xml_local(one.tag) == "h"]
            )
        },
        "headings": [
            {"level": of_local(one, "outline-level"), "text": texts(one)}
            for one in body.iter()
            if xml_local(one.tag) == "h"
        ],
        "styles": style_counts(paras),
        "tables": sum(1 for one in body.iter() if xml_local(one.tag) == "table"),
        # 逐张表的账：名字在 table:name，格子只数 table-cell（覆盖格另算）
        "table_list": [
            {
                "name": of_local(one, "name"),
                "rows": sum(1 for kid in one if xml_local(kid.tag) == "table-row"),
                "cells": sum(1 for kid in one.iter() if xml_local(kid.tag) == "table-cell"),
                "covered": sum(1 for kid in one.iter() if xml_local(kid.tag) == "covered-table-cell"),
                "text": sum(1 for kid in one.iter() if xml_local(kid.tag) == "p" and odf_para_text(kid).strip()),
            }
            for one in body.iter()
            if xml_local(one.tag) == "table"
        ],
        "table_rows": sum(1 for one in body.iter() if xml_local(one.tag) == "table-row"),
        "table_cells": sum(1 for one in body.iter() if xml_local(one.tag) == "table-cell"),
        "covered_cells": count_local(body, "covered-table-cell"),
        "sections": count_local(body, "section"),
        "breaks": count_local(body, "line-break"),
        "page_breaks": count_local(body, "soft-page-break"),
        "drawings": count_local(body, "frame"),
        "annotations": count_local(body, "annotation"),
        "lists": count_local(body, "list"),
        "list_styles": count_local(body, "list-style"),
        "bookmarks": count_local(body, "bookmark-start") + count_local(body, "bookmark"),
        "sequences": count_local(body, "sequence-decl"),
        "tracked_changes": count_local(body, "tracked-changes"),
        "hyperlinks": [
            {"target": of_local(one, "href"), "text": texts(one)}
            for one in body.iter()
            if xml_local(one.tag) == "a"
        ],
        "images": [
            of_local(one, "href")
            for one in body.iter()
            if xml_local(one.tag) == "image" and of_local(one, "href")
        ],
        "footnotes": sum(1 for one in notes_found if of_local(one, "note-class") == "footnote"),
        "endnotes": sum(1 for one in notes_found if of_local(one, "note-class") == "endnote"),
        "paragraph_texts": [texts(one) for one in paras],
        "annotation_texts": annotation_entries(body),
        "statistic": statistic,
    }


def body_paragraphs(root) -> list:
    """正文段：批注（`text:annotation`）里的那些 `text:p` 不算。

    ODF 的批注是嵌在正文段**里面**的，不是像 docx 那样另有一个 comments.xml 部件，
    所以「这一段有几段字」这件事得先把批注子树挖掉再数。
    ElementTree 没有父指针，就反过来做：先把批注里的段挑出来，按 id 排除。
    """
    inside = set()
    for owner in root.iter():
        if xml_local(owner.tag) != "annotation":
            continue
        for one in owner.iter():
            if xml_local(one.tag) == "p":
                inside.add(id(one))
    return [
        one
        for one in root.iter()
        if xml_local(one.tag) == "p" and id(one) not in inside
    ]


def body_paragraph_and_heading_nodes(root) -> list:
    """`office-text` 的 ODF 口径：正文段**和标题**按文档顺序一起走。

    这两份账本来就不是一个问句：office-doc 问「有几段」（只认 `text:p`，
    标题另有一份带层级的清单），office-text 问「页面上能读到哪几块字」，
    `text:h` 也是字。共用一份清单会让其中一边说谎，所以分开列。
    批注子树同样挖掉，理由与 `body_paragraphs` 一致。
    """
    inside = set()
    for owner in root.iter():
        if xml_local(owner.tag) != "annotation":
            continue
        for one in owner.iter():
            if xml_local(one.tag) in ("p", "h"):
                inside.add(id(one))
    return [
        one
        for one in root.iter()
        if xml_local(one.tag) in ("p", "h") and id(one) not in inside
    ]


def annotation_entries(body) -> list:
    """ODF 批注：作者与时间挂在它自己的 meta:creator / meta:date **孩子**上
    （docx 那边是 w:comment 的属性）—— 两份文件的存法正好相反，都得照文件读。
    """
    out = []
    for owner in body.iter():
        if xml_local(owner.tag) != "annotation":
            continue
        author = ""
        stamp = ""
        for kid in owner:
            if xml_local(kid.tag) == "creator":
                author = "".join(kid.itertext()).strip()
            if xml_local(kid.tag) == "date":
                stamp = "".join(kid.itertext()).strip()
        for para in [one for one in owner.iter() if xml_local(one.tag) == "p"]:
            out.append(
                {
                    "from": "annotation",
                    "author": author or None,
                    "date": stamp or None,
                    "text": "".join(para.itertext()).strip(),
                }
            )
    return out


def odf_para_text(node) -> str:
    """ODF 一段的字，批注子树挖掉 —— 与 `office-text` 的 ODF 口径一条一条对得上。

    注意批注元素**后面**那段尾巴字（`kid.tail`）还属于这一段：`</text:annotation>`
    之后、`</text:p>` 之前写的字是正文，挖掉批注不能把它一起挖走。
    """
    out = node.text or ""
    for kid in node:
        if xml_local(kid.tag) == "annotation":
            out += kid.tail or ""
            continue
        out += odf_para_text(kid) + (kid.tail or "")
    return out


def odt_facts(path: Path) -> dict:
    parts = {}
    with zipfile.ZipFile(path) as box:
        names = [one.filename for one in box.infolist()]
        parts = {one.filename: box.read(one.filename) for one in box.infolist()}
    root = ET.fromstring(parts["content.xml"])
    paras = body_paragraphs(root)
    heads = [one for one in root.iter() if xml_local(one.tag) == "h"]
    tables = [one for one in root.iter() if xml_local(one.tag) == "table"]
    metas = {}
    if "meta.xml" in parts:
        m = ET.fromstring(parts["meta.xml"])
        for one in m.iter():
            name = xml_local(one.tag)
            if one.text and one.text.strip():
                metas.setdefault(name, one.text.strip())
    # office-text 交的是「页面上读到的每一块字」，标题也算 —— 与「几段」是两问
    blocks = [
        odf_para_text(one).strip()
        for one in body_paragraph_and_heading_nodes(root)
    ]
    return {
        "paragraph_count": len(paras),
        "headings": ["".join(one.itertext()) for one in heads],
        "tables": len(tables),
        "text": "\n".join(blocks),
        "paragraphs": [one for one in blocks if one],
        "media": sorted(one for one in names if one.startswith("Pictures/")),
        "meta": metas,
        "parts": sorted(names),
        # 批注单独一份：作者与时间在 meta:creator / meta:date 这些孩子上
        "annotations": annotation_entries(root),
    }


def col_letter(index: int) -> str:
    """0 → A，25 → Z，26 → AA：ODF 的格子没有名字，位置得自己数出来"""
    name = ""
    at = index
    while True:
        name = chr(ord("A") + at % 26) + name
        at = at // 26 - 1
        if at < 0:
            return name


def ods_facts(path: Path) -> dict | None:
    """ODF 电子表格：格子内**不写数字**，写的是 office:value / date-value / boolean-value，
    位置要靠 table:number-columns-repeated 累加出来 —— 那属性一填就是 16381，
    照字面数就是每张表一万六千格。表是不是隐藏，也不在表上，在它引的那个自动样式里。
    不是表格的 ODF（文字/演示稿里的普通表）交回 None：判据是内容里有没有 spreadsheet 根。
    """
    with zipfile.ZipFile(path) as box:
        parts = {one.filename: box.read(one.filename) for one in box.infolist()}
    root = ET.fromstring(parts["content.xml"])
    if not any(xml_local(one.tag) == "spreadsheet" for one in root.iter()):
        return None

    def attr(node, want: str):
        """按局部名取属性，但躲开 LibreOffice 抄的那份副本：`calcext:value-type`
        的局部名跟 `office:value-type` 一模一样，命名空间却是 documentfoundation 的实验区。
        """
        for key, value in node.attrib.items():
            if key.rsplit("}", 1)[-1] != want or "documentfoundation" in key:
                continue
            return value
        return None

    def rep(node, want: str) -> int:
        try:
            return max(1, int(attr(node, want)))
        except (TypeError, ValueError):
            return 1

    # 自动样式家族 = table 的那些：table:display 说这张表可不可见
    shown: dict[str, bool] = {}
    for style in root.iter():
        if xml_local(style.tag) != "style" or attr(style, "family") != "table":
            continue
        holder = attr(style, "name") or ""
        for one in style:
            if xml_local(one.tag) == "table-properties":
                flag = attr(one, "display")
                shown[holder] = flag != "false"

    # 隐藏的行与列：`table:visibility="collapse"` 可以直接写在行/列上（LibreOffice
    # 那份就这么写），也可以只写在它引的那个自动样式里，两边都得看
    folded: dict[str, bool] = {}
    for style in root.iter():
        if xml_local(style.tag) != "style":
            continue
        wanted = {
            "table-row": "table-row-properties",
            "table-column": "table-column-properties",
        }.get(attr(style, "family") or "")
        if wanted is None:
            continue
        flag = any(
            xml_local(one.tag) == wanted and attr(one, "visibility") == "collapse"
            for one in style
        )
        folded[attr(style, "name") or ""] = flag

    sheets = []
    for table in root.iter():
        if xml_local(table.tag) != "table":
            continue
        cells: list = []
        covered = 0
        merged = 0
        used_rows = 0
        widest = 0
        hidden_rows = 0
        hidden_cols = 0
        for column in table:
            if xml_local(column.tag) != "table-column":
                continue
            if attr(column, "visibility") == "collapse" or folded.get(
                attr(column, "style-name") or ""
            ):
                # 一条元素盖几列，看它自己的 number-columns-repeated
                hidden_cols += rep(column, "number-columns-repeated")
        row_at = 0
        for row in table:
            if xml_local(row.tag) != "table-row":
                continue
            row_repeat = rep(row, "number-rows-repeated")
            if attr(row, "visibility") == "collapse" or folded.get(
                attr(row, "style-name") or ""
            ):
                hidden_rows += row_repeat
            col_at = 0
            row_cells = []
            for cell in row:
                kind = xml_local(cell.tag)
                if kind not in ("table-cell", "covered-table-cell"):
                    continue
                span = rep(cell, "number-columns-repeated")
                if kind == "covered-table-cell":
                    covered += 1
                    col_at += span
                    continue
                text = "\n".join(
                    "".join(one.itertext())
                    for one in cell
                    if xml_local(one.tag) == "p"
                )
                value = attr(cell, "value")
                stamp = attr(cell, "date-value")
                flag = attr(cell, "boolean-value")
                formula = attr(cell, "formula")
                if text or value or stamp or flag or formula:
                    cs = rep(cell, "number-columns-spanned")
                    rs = rep(cell, "number-rows-spanned")
                    if cs > 1 or rs > 1:
                        merged += 1
                    row_cells.append(
                        {
                            "ref": f"{col_letter(col_at)}{row_at + 1}",
                            "value_type": attr(cell, "value-type") or "empty",
                            "value": value,
                            "date_value": stamp,
                            "boolean_value": flag,
                            "formula": formula,
                            "text": text,
                            "style": attr(cell, "style-name"),
                            "columns_spanned": cs,
                            "rows_spanned": rs,
                        }
                    )
                    widest = max(widest, col_at + 1)
                col_at += span
            if row_cells:
                used_rows += row_repeat
                cells.extend(row_cells)
            row_at += row_repeat
        name = attr(table, "name") or ""
        sheets.append(
            {
                "name": name,
                "visible": shown.get(attr(table, "style-name"), True),
                "rows": used_rows,
                "columns": widest,
                "cells": len(cells),
                "cell_list": cells,
                "covered": covered,
                "merged": merged,
                "hidden_rows": hidden_rows,
                "hidden_cols": hidden_cols,
                "formulas": sum(1 for one in cells if one["formula"]),
            }
        )
    statistic = {}
    if "meta.xml" in parts:
        for one in ET.fromstring(parts["meta.xml"]).iter():
            if xml_local(one.tag) == "document-statistic":
                statistic = {key.rsplit("}", 1)[-1]: value for key, value in one.attrib.items()}
    return {
        "sheets": sheets,
        "cell_total": sum(one["cells"] for one in sheets),
        "statistic": statistic,
    }


def odf_page_text(path: Path) -> list:
    """ODF 的页眉与页脚：它们不在 content.xml，在 styles.xml 的 master-page 里。

    而且一个 master-page 可以有四个口袋（`style:header` / `header-left` /
    `header-first` 与页脚的对应三个），首页与左右页各一套是 ODF 的常规而不是特例。
    """
    with zipfile.ZipFile(path) as box:
        if "styles.xml" not in box.namelist():
            return []
        root = ET.fromstring(box.read("styles.xml"))
    slots = {
        "header": "header",
        "header-left": "header",
        "header-first": "header",
        "footer": "footer",
        "footer-left": "footer",
        "footer-first": "footer",
    }
    out: list = []
    for page in root.iter():
        if xml_local(page.tag) != "master-page":
            continue
        master = local_attr(page, "name")
        for slot in page:
            what = slots.get(xml_local(slot.tag))
            if what is None:
                continue
            for para in [one for one in slot.iter() if xml_local(one.tag) == "p"]:
                out.append(
                    {
                        "from": what,
                        "part": "styles.xml",
                        "master": master,
                        "slot": xml_local(slot.tag),
                        "text": "".join(para.itertext()).strip(),
                    }
                )
    return out


def has_scheme(raw: str) -> bool:
    """URI 的通用形状：`scheme ":"` —— scheme 是字母开头的 alnum / + / - / . 串。

    与 Rust 那边同一条规则，不查任何格式的名字表。
    """
    text = raw.lstrip()
    head, sep, _ = text.partition(":")
    if not sep or not head:
        return False
    if not head[0].isascii() or not head[0].isalpha():
        return False
    return all(ch.isascii() and (ch.isalnum() or ch in "+-.") for ch in head)


def odf_links(path: Path) -> dict:
    """ODF 的引用清单：关系表是 OPC 的东西，ODF 没有那份表，引用坐在 `xlink:href` 上。

    判据只有一条 URI 的通用形状：目标带 scheme 就算「在包外面」（`https:`、
    `vnd.sun.star.script:` 都算），没 scheme 的才是包内路径。前缀按局部名认，
    因为命名空间前缀是文件自己声明的。
    """
    out: list = []
    with zipfile.ZipFile(path) as box:
        for one in box.infolist():
            if not one.filename.endswith(".xml"):
                continue
            try:
                root = ET.fromstring(box.read(one.filename))
            except ET.ParseError:
                continue
            for node in root.iter():
                for key, value in node.attrib.items():
                    if key.rsplit("}", 1)[-1] != "href":
                        continue
                    out.append(
                        {
                            "target": value,
                            "part": one.filename,
                            "element": xml_local(node.tag),
                            "external": has_scheme(value),
                        }
                    )
    return {"links": out, "external": [one for one in out if one["external"]]}


def odp_facts(path: Path) -> dict | None:
    """ODF 演示稿：页面上的字与备注里的字是两件事，尺寸还得绕 master-page 那一跳。

    `presentation:notes` 里坐着三个框 —— 缩略图、真正的备注（class="notes"）、
    以及页码占位（class="page-number"，里面是样字 `<编号>`）。把整页的 `text:p`
    一把抓，就会把「<编号>」当成这页写了什么。
    """
    with zipfile.ZipFile(path) as box:
        parts = {one.filename: box.read(one.filename) for one in box.infolist()}
    if b"office:presentation" not in parts["content.xml"]:
        return None
    root = ET.fromstring(parts["content.xml"])
    styles = ET.fromstring(parts["styles.xml"]) if "styles.xml" in parts else None

    layout_of_master: dict = {}
    size_of_layout: dict = {}
    if styles is not None:
        for one in styles.iter():
            if xml_local(one.tag) == "master-page":
                layout_of_master[of_local(one, "name")] = of_local(one, "page-layout-name")
            if xml_local(one.tag) == "page-layout":
                for kid in one:
                    if xml_local(kid.tag) == "page-layout-properties":
                        size_of_layout[of_local(one, "name")] = {
                            "page_width": of_local(kid, "page-width"),
                            "page_height": of_local(kid, "page-height"),
                            "orientation": of_local(kid, "print-orientation"),
                        }

    def lines_of(frame) -> list:
        return [
            "".join(one.itertext()).strip()
            for one in frame.iter()
            if xml_local(one.tag) == "p" and "".join(one.itertext()).strip()
        ]

    slides = []
    for page in root.iter():
        if xml_local(page.tag) != "page":
            continue
        texts: list = []
        placeholders: list = []
        title = ""
        notes = ""
        note_classes: list = []
        block = None
        for kid in page:
            if xml_local(kid.tag) == "notes":
                block = kid
        for frame in page:
            if xml_local(frame.tag) != "frame":
                continue
            kind = of_local(frame, "class") or ""
            if kind and kind not in placeholders:
                placeholders.append(kind)
            lines = lines_of(frame)
            if kind == "title" and lines and not title:
                title = lines[0]
            texts.extend(lines)
        if block is not None:
            for frame in block:
                if xml_local(frame.tag) != "frame":
                    continue
                kind = of_local(frame, "class") or ""
                note_classes.append(kind)
                if kind == "notes":
                    notes = "\n".join(lines_of(frame))
        master = of_local(page, "master-page-name")
        slides.append(
            {
                "name": of_local(page, "name"),
                # 没有 title 占位时退回第一段：两边同一口径，不然比的是两份规则
                "title": title if title else (texts[0] if texts else ""),
                "master": master,
                "layout": of_local(page, "presentation-page-layout-name"),
                "placeholders": placeholders,
                "texts": texts,
                "notes": notes,
                "notes_frame_classes": note_classes,
                "pictures": sum(1 for one in page.iter() if xml_local(one.tag) == "image"),
                "tables": sum(1 for one in page.iter() if xml_local(one.tag) == "table"),
                "size": size_of_layout.get(layout_of_master.get(master)),
            }
        )
    return {
        "slides": slides,
        "masters": sorted({one["master"] for one in slides if one["master"]}),
        "layouts": sorted({one["layout"] for one in slides if one["layout"]}),
        # 页上写着版式名，文件里没有版式定义：这是这份真件的事实，不是我漏读
        "page_layout_defs": sum(1 for one in root.iter() if xml_local(one.tag) == "page-layout"),
    }


def split_ref(reference: str):
    """`B4` → (行 3, 列 1)。不是 A1 形状就交回 None —— 位置猜不出来就不铺网格。"""
    raw = (reference or "").strip()
    at = 0
    while at < len(raw) and raw[at].isalpha():
        at += 1
    letters, digits = raw[:at], raw[at:]
    if not letters or not digits or not digits.isdigit():
        return None
    col = 0
    for ch in letters.upper():
        col = col * 26 + (ord(ch) - ord("A") + 1)
    row = int(digits)
    if row == 0:
        return None
    return (row - 1, col - 1)


def csv_quote(raw: str) -> str:
    """RFC4180：带逗号、引号、换行就整体加引号，里面的引号翻倍"""
    if any(ch in raw for ch in ('"', ",", "\n", "\r")):
        return '"' + raw.replace('"', '""') + '"'
    return raw


def csv_render(cells: list) -> str:
    rows = max((one[0] for one in cells), default=-1) + 1
    cols = max((one[1] for one in cells), default=-1) + 1
    grid = [["" for _ in range(cols)] for _ in range(rows)]
    for row, col, raw in cells:
        grid[row][col] = raw
    return "".join(
        ",".join(csv_quote(one) for one in line) + "\n" for line in grid
    )


def number_text(raw: str) -> str:
    """按 lbin 的口径给数：整数值不写 `.0`（Rust 的 `{}` 就是这个样子）"""
    try:
        value = float(raw)
    except (TypeError, ValueError):
        return raw or ""
    return str(int(value)) if value.is_integer() else repr(value)


def xlsx_csv(path: Path) -> list:
    """每张表一个 (名字, CSV 文本)：字符串查 sharedStrings，日期给 ISO 串"""
    with zipfile.ZipFile(path) as box:
        parts = {one.filename: box.read(one.filename) for one in box.infolist()}
    wb = ET.fromstring(parts["xl/workbook.xml"])
    rels = {}
    if "xl/_rels/workbook.xml.rels" in parts:
        for one in ET.fromstring(parts["xl/_rels/workbook.xml.rels"]).iter():
            if xml_local(one.tag) == "Relationship":
                rels[one.get("Id")] = one.get("Target") or ""
    sst = []
    if "xl/sharedStrings.xml" in parts:
        for one in ET.fromstring(parts["xl/sharedStrings.xml"]).iter():
            if xml_local(one.tag) == "si":
                sst.append("".join(x.text or "" for x in one.iter() if xml_local(x.tag) == "t"))
    dates = {}
    for one in xlsx_formats(path)["cells"]:
        if one.get("as_date"):
            dates[f"{one['sheet']}!{one['ref']}"] = one["as_date"]
    out = []
    for sheet in wb.iter():
        if xml_local(sheet.tag) != "sheet":
            continue
        name = sheet.get("name") or ""
        rid = None
        for key, value in sheet.attrib.items():
            if key.rsplit("}", 1)[-1] == "id":
                rid = value
        target = rels.get(rid) or ""
        # 关系目标可以写成「包根起算」的绝对名（openpyxl 就用 "/xl/worksheets/sheet1.xml"）
        if target.startswith("/"):
            part = target[1:]
        elif target.startswith("xl/"):
            part = target
        else:
            part = "xl/" + target
        if part not in parts:
            continue
        cells = []
        root = ET.fromstring(parts[part])
        for one in root.iter():
            if xml_local(one.tag) != "c":
                continue
            ref = one.get("r") or ""
            spot = split_ref(ref)
            if spot is None:
                continue
            kind = one.get("t") or "n"
            value = ""
            inline = ""
            for kid in one:
                if xml_local(kid.tag) == "v":
                    value = "".join(kid.itertext()).strip()
                if xml_local(kid.tag) == "is":
                    inline = "".join(kid.itertext()).strip()
            if dates.get(f"{name}!{ref}"):
                display = dates[f"{name}!{ref}"]
            elif kind == "s":
                try:
                    display = sst[int(value)]
                except (ValueError, IndexError):
                    display = f"#SST 索引 {value} 越界"
            elif kind == "inlineStr":
                display = inline
            elif kind == "b":
                display = "TRUE" if value == "1" else "FALSE"
            elif kind == "e":
                display = f"#错误 {value}"
            elif kind in ("str", "n"):
                display = value if kind == "str" or not value else number_text(value)
            else:
                display = value
            cells.append((spot[0], spot[1], display))
        out.append((name, csv_render(cells)))
    return out


def ods_csv(path: Path) -> list:
    """ODF 表格：与 xlsx 同一口径 —— 给值不给显示格式，日期给 ISO，布尔给 TRUE/FALSE"""
    book = ods_facts(path) or {"sheets": []}
    out = []
    for one in book["sheets"]:
        cells = []
        for had in one["cell_list"]:
            spot = split_ref(had["ref"])
            if spot is None:
                continue
            if had["date_value"]:
                display = had["date_value"]
            elif had["value_type"] == "boolean":
                display = "TRUE" if had["boolean_value"] == "true" else "FALSE"
            elif had["value_type"] in ("float", "percentage", "currency") and had["value"]:
                display = number_text(had["value"])
            else:
                display = had["text"]
            cells.append((spot[0], spot[1], display))
        out.append((one["name"], csv_render(cells)))
    return out


def biff_csv(book: dict) -> dict:
    """遗留 .xls 的 CSV：位置**直接用记录里的 row/col 整数**，Rust 那边是从 `"B4"`
    反解 —— 两条算法不同，结果必须逐字相同，这样才叫对账。

    日期格这里给的是序列数：BIFF 这条路没有「查 cellXfs 拿格式码」那一步
    （命令在 notes 里也是这么说的），所以别装作它能换算成 ISO。
    """
    out = []
    for one in book["sheets"]:
        cells = []
        for had in book["cells"]:
            if had.get("sheet") != one["name"]:
                continue
            raw = had.get("value")
            display = raw if isinstance(raw, str) else number_text(raw)
            cells.append((int(had["row"]), int(had["col"]), display))
        out.append((one["name"], csv_render(cells)))
    return {"sheets": [{"name": name, "csv": body} for name, body in out]}


def xlsx_hidden(path: Path) -> dict:
    """每张表隐藏了多少行、多少列（OOXML 那条路）。

    `<col>` 是带跨度的：LibreOffice 把连续三列并成一条 `min="3" max="5"`，
    按元素个数数就少报两列；openpyxl 反过来一列一条。两边的数必须都是 3。
    `hidden` 的写法也不同：`"1"`（openpyxl）与 `"true"`（LibreOffice，
    而且没隐藏的行它也写 `hidden="false"`）。
    """
    with zipfile.ZipFile(path) as box:
        parts = {one.filename: box.read(one.filename) for one in box.infolist()}
    rels = {}
    if "xl/_rels/workbook.xml.rels" in parts:
        for one in ET.fromstring(parts["xl/_rels/workbook.xml.rels"]).iter():
            if xml_local(one.tag) == "Relationship":
                rels[one.get("Id")] = one.get("Target") or ""
    out: dict = {}
    wb = ET.fromstring(parts["xl/workbook.xml"])
    for sheet in wb.iter():
        if xml_local(sheet.tag) != "sheet":
            continue
        rid = None
        for key, value in sheet.attrib.items():
            if key.rsplit("}", 1)[-1] == "id":
                rid = value
        target = rels.get(rid) or ""
        if target.startswith("/"):
            part = target[1:]
        elif target.startswith("xl/"):
            part = target
        else:
            part = "xl/" + target
        if part not in parts:
            continue
        root = ET.fromstring(parts[part])
        rows = 0
        cols = 0
        for one in root.iter():
            tag = xml_local(one.tag)
            if (one.get("hidden") or "").lower() not in ("1", "true"):
                continue
            if tag == "row":
                rows += 1
            elif tag == "col":
                try:
                    first = int(one.get("min", "0"))
                    last = int(one.get("max", "0"))
                except ValueError:
                    first = last = 0
                cols += last - first + 1 if last >= first >= 1 else 1
        out[sheet.get("name") or ""] = {"hidden_rows": rows, "hidden_cols": cols}
    return out


def ods_styles(path: Path) -> dict:
    """ODF 的「这一格按什么格式显示」那一跳，独立算一遍。

    链路：格子的 `table:style-name` → 单元格样式的 `style:data-style-name`（父样式链
    上找）→ `number:*-style` 元素。数据样式可能在 content.xml 也可能在 styles.xml，
    两边都得读，同名时 content 里那份赢。
    **不重构格式串**：把元素树逐条抄成 token（`<number:text>-</number:text>` 记 `text:-`），
    再带上的只有样式自己写的 `number:decimal-places` 与 `number:currency-symbol`。
    """
    with zipfile.ZipFile(path) as box:
        parts = {one.filename: box.read(one.filename) for one in box.infolist()}

    def attr(node, want: str):
        for key, value in node.attrib.items():
            if key.rsplit("}", 1)[-1] != want or "documentfoundation" in key:
                continue
            return value
        return None

    cell_styles: dict[str, dict] = {}
    data_styles: dict[str, dict] = {}
    KIND = {
        "date-style": "date",
        "time-style": "time",
        "number-style": "number",
        "currency-style": "currency",
        "percentage-style": "percent",
        "text-style": "text",
        "boolean-style": "bool",
    }
    # 顺序就是优先级：先 styles.xml，后 content.xml
    for name in ("styles.xml", "content.xml"):
        if name not in parts:
            continue
        root = ET.fromstring(parts[name])
        for one in root.iter():
            if xml_local(one.tag) == "style" and attr(one, "family") == "table-cell":
                holder = attr(one, "name")
                if holder is None:
                    continue
                cell_styles[holder] = {
                    "data_style": attr(one, "data-style-name"),
                    "parent": attr(one, "parent-style-name"),
                }
            kind = KIND.get(xml_local(one.tag))
            if kind is None:
                continue
            holder = attr(one, "name")
            if holder is None:
                continue
            tokens = []
            decimals = None
            for kid in one:
                if xml_local(kid.tag) == "text":
                    tokens.append("text:" + ("".join(kid.itertext()) or ""))
                else:
                    tokens.append(xml_local(kid.tag))
                if decimals is None:
                    raw = attr(kid, "decimal-places")
                    if raw is not None:
                        try:
                            decimals = int(raw)
                        except ValueError:
                            decimals = None
            data_styles[holder] = {
                "kind": kind,
                "decimals": decimals,
                "currency_symbol": attr(one, "currency-symbol"),
                "tokens": tokens,
            }

    def data_of(cell_style):
        here = cell_style
        for _ in range(8):
            found = cell_styles.get(here)
            if found is None:
                return None
            want = found.get("data_style")
            if want:
                return data_styles.get(want), want
            here = found.get("parent")
            if not here:
                return None, None
        return None, None

    out = {}
    for holder in cell_styles:
        got = data_of(holder)
        if got is None:
            out[holder] = {"data_style": None, "format_kind": None}
            continue
        style, want = got
        out[holder] = {
            "data_style": want,
            "format_kind": (style or {}).get("kind"),
            "decimals": (style or {}).get("decimals"),
            "currency_symbol": (style or {}).get("currency_symbol"),
            "format_tokens": (style or {}).get("tokens", []),
        }
    return out


def csv_facts(path: Path) -> dict:
    sheets = xlsx_csv(path) if path.suffix.lower() in (".xlsx", ".xlsm") else ods_csv(path)
    return {"sheets": [{"name": name, "csv": body} for name, body in sheets]}


# ---------------------------------------------------------------- MS-CFB（.doc/.xls/.ppt）


def u8(buf: bytes, off: int):
    return buf[off] if 0 <= off < len(buf) else None


def u16(buf: bytes, off: int):
    return struct.unpack_from("<H", buf, off)[0] if off + 2 <= len(buf) else None


def u32(buf: bytes, off: int):
    return struct.unpack_from("<I", buf, off)[0] if off + 4 <= len(buf) else None


def i32(buf: bytes, off: int):
    return struct.unpack_from("<i", buf, off)[0] if off + 4 <= len(buf) else None


def u64(buf: bytes, off: int):
    return struct.unpack_from("<Q", buf, off)[0] if off + 8 <= len(buf) else None


def i64(buf: bytes, off: int):
    return struct.unpack_from("<q", buf, off)[0] if off + 8 <= len(buf) else None


def u32s(buf: bytes, off: int, count: int) -> list[int]:
    """从 off 起取 count 个 u32，越界就停在已经拿到的那些上（截断文件是常态）"""
    out = []
    for i in range(count):
        one = u32(buf, off + i * 4)
        if one is None:
            break
        out.append(one)
    return out


def local_child(node, want: str):
    for one in node:
        if xml_local(one.tag) == want:
            return one
    return None


def has_local_child(node, want: str) -> bool:
    return local_child(node, want) is not None


def cfb_parse(data: bytes) -> dict:
    """MS-CFB 复合文档：扇区 / 目录 / 迷你流全按规范自己走一遍，不借任何库。

    返回 `{meta..., "entries": [...], "streams": [{name, size, ...}], "bytes": {name: bytes}}`
    —— 元数据视图与字节视图出自**同一次**解析。写两份链遍历就是给自己留「两份不一致」的坑。
    """
    if data[:8] != bytes.fromhex("D0CF11E0A1B11AE1"):
        raise ValueError("不是 CFB 签名")
    minor, major, byte_order, shift, minishift = struct.unpack_from("<HHHHH", data, 24)
    if byte_order != 0xFFFE:
        raise ValueError(f"字节序标记 {byte_order:#x} 不是 little-endian 的 0xFFFE")
    sector = 1 << shift
    minisect = 1 << minishift
    num_fat = u32(data, 44) or 0
    dir_start = u32(data, 48) or 0
    cutoff = u32(data, 56) or 0
    minifat_start = u32(data, 60) or 0xFFFFFFFF
    difat_start = u32(data, 68) or 0xFFFFFFFF
    num_difat = u32(data, 72) or 0
    difat = u32s(data, 76, 109)

    notes: list[str] = []
    if major not in (3, 4):
        raise ValueError(f"主版本号 {major} 不认识（只有 3 与 4）")
    if sector not in (512, 4096):
        raise ValueError(f"扇区大小 {sector} 不对")
    if major == 3 and (u32(data, 40) or 0) != 0:
        notes.append("v3 的目录扇区数应为 0，但文件写了非 0")

    def offset(which: int) -> int:
        return (which + 1) * sector

    def sect(which: int) -> bytes:
        return data[offset(which) : offset(which) + sector]

    # DIFAT 链：header 里 109 条不够时，后面的扇区每条链尾再指一个
    fat_sectors: list[int] = [one for one in difat if one != 0xFFFFFFFF]
    nxt, guard = difat_start, 0
    while nxt not in (0xFFFFFFFE, 0xFFFFFFFF) and guard < 4096:
        guard += 1
        base = offset(nxt)
        if base + sector > len(data):
            notes.append("DIFAT 链越界，后面的 FAT 扇区读不到")
            break
        count = (sector // 4) - 1
        fat_sectors.extend(
            one for one in u32s(data, base, count) if one != 0xFFFFFFFF
        )
        nxt = u32(data, base + count * 4) or 0xFFFFFFFE
    if len(fat_sectors) != num_fat:
        notes.append(
            f"头部说 FAT 扇区有 {num_fat} 个，DIFAT 链给出 {len(fat_sectors)} 个"
        )

    fat: list[int] = []
    for one in sorted(fat_sectors):
        if offset(one) + sector > len(data):
            notes.append(f"FAT 扇区 {one} 越界")
            continue
        fat.extend(u32s(data, offset(one), sector // 4))

    def chain(start: int, table: list[int]) -> list[int]:
        out: list[int] = []
        cur = start
        while cur not in (0xFFFFFFFE, 0xFFFFFFFF):
            if cur in out:
                notes.append("链里出现环，已在此截断")
                break
            if cur >= len(table):
                notes.append(f"链指向 {cur}，超出 FAT 表长度 {len(table)}")
                break
            out.append(cur)
            cur = table[cur]
        return out

    dir_sectors = chain(dir_start, fat)
    raw_dir = b"".join(sect(one) for one in dir_sectors)
    entries = []
    for i in range(len(raw_dir) // 128):
        one = raw_dir[i * 128 : (i + 1) * 128]
        namelen = u16(one, 64) or 0
        chars = max(0, (namelen // 2) - 1) if namelen else 0
        kind = one[66]
        start = u32(one, 116) or 0
        size = i64(one, 120) or 0
        entries.append(
            {
                "index": i,
                "name": one[: chars * 2].decode("utf-16-le", "replace"),
                "type": {0: "unknown", 1: "storage", 2: "stream", 5: "root"}.get(kind, str(kind)),
                "start": start,
                "size": max(size, 0),
                "clsid": one[80:96].hex(),
                "created": u64(one, 100) or 0,
                "modified": u64(one, 108) or 0,
                "left": u32(one, 68) or 0,
                "right": u32(one, 72) or 0,
                "child": u32(one, 76) or 0,
            }
        )
    roots = [one for one in entries if one["type"] == "root"]

    mini_fat: list[int] = []
    for one in chain(minifat_start, fat):
        mini_fat.extend(u32s(data, offset(one), sector // 4))

    def follow(start: int, size: int, mini: bool) -> bytes:
        out = bytearray()
        if mini:
            for which in chain(start, mini_fat):
                out += mini_stream[which * minisect : (which + 1) * minisect]
        else:
            for which in chain(start, fat):
                out += sect(which)
        return bytes(out[:size])

    mini_stream = b""
    if roots:
        mini_stream = bytearray()  # type: ignore[assignment]
        buf = bytearray()
        for which in chain(roots[0]["start"], fat):
            buf += sect(which)
        mini_stream = bytes(buf[: roots[0]["size"]])

    streams: dict[str, bytes] = {}
    listing = []
    for one in entries:
        if one["type"] != "stream":
            continue
        mini = one["size"] < cutoff
        body = follow(one["start"], one["size"], mini) if one["size"] else b""
        streams[one["name"]] = body
        listing.append(
            {
                "name": one["name"],
                "size": one["size"],
                "start": one["start"],
                "via": "mini" if mini else "fat",
                "readable": len(body) == one["size"],
                "sha_head": body[:16].hex(),
            }
        )
    return {
        "container": "cfb",
        "minor": minor,
        "major": major,
        "sector_size": sector,
        "mini_sector_size": minisect,
        "mini_cutoff": cutoff,
        "fat_sectors": len(fat_sectors),
        "difat_chained": num_difat > 0,
        "directory_entries": len(entries),
        "storages": [one["name"] for one in entries if one["type"] in ("storage", "root")],
        "streams": listing,
        "root_clsclsid": roots[0]["clsid"] if roots else "",
        "notes": notes,
        "entries": entries,
        "bytes": streams,
    }


def cfb_open(data: bytes) -> dict:
    """给 JSON 报告用的视图：字节不进去（那是几十 KB 的十六进制，没人要看）"""
    parsed = cfb_parse(data)
    parsed.pop("entries", None)
    parsed.pop("bytes", None)
    return parsed


def cfb_stream(data: bytes, want: str) -> bytes:
    """按名字取一条流的字节（与元数据同一次解析，不另写一份链遍历）"""
    return cfb_parse(data)["bytes"].get(want, b"")


# ---------------------------------------------------------------- OLE 属性集（MS-OPS）
VT_NAMES = {
    2: "i2",
    3: "i4",
    7: "bool",
    11: "vbool",
    19: "dateTime",
    20: "i8",
    30: "lpstr",
    31: "lpwstr",
    48: "variant",
    64: "filetime",
    65: "blob",
    71: "vector(i4)",
    4126: "vector(lpstr)",
    4108: "vector(lpwstr)",
}


def propset_decode(data: bytes) -> dict:
    """序列化属性集：头部 + FMTID 表 + 每个 section 的 PID/值两张平行表

    每一步都先确认偏移落在缓冲区内再取值：属性集越界是遗留 Office 文件里最常见的
    一处残损，宁可少报一个属性，也不许把异常抛给整个读取。
    """
    order = u16(data, 0)
    if order != 0xFFFE:
        return {"error": f"字节序 {order:#x} 不是 0xFFFE"}
    # 头部：byteOrder(2) + format(2) + osVersion(4) + CLSID(16) = 24 字节，紧接着
    # numPropertySets(4)，然后才是 20 字节一条的 {FMTID, 属性集偏移} 数组 —— 24 与 28
    # 这两个数别记错：把 FMTID 的前四个字节当成属性集个数，会得到一个 40 亿的循环。
    count = u32(data, 24) or 0
    out: dict = {"sets": []}
    for i in range(count):
        fmt_off = 28 + i * 20
        if fmt_off + 20 > len(data):
            out.setdefault("truncated", []).append(f"第 {i} 个 FMTID 表项越界")
            break
        fmtid = data[fmt_off : fmt_off + 16]
        pos = u32(data, fmt_off + 16) or 0
        if pos + 8 > len(data):
            out.setdefault("truncated", []).append(f"第 {i} 个属性集偏移 {pos} 越界")
            continue
        nprops = u32(data, pos + 4) or 0
        pids, vals = [], []
        for j in range(nprops):
            base = pos + 8 + j * 8
            if base + 8 > len(data):
                break
            pids.append(u32(data, base))
            vals.append(u32(data, base + 4))
        props: dict = {}
        # VT_LPSTR 的字节按哪个字符集解释，是属性集**自己**用 PID 1 说的：LibreOffice
        # 写 UTF-8（codepage 65001），旧版 Word 写 cp1252。当成 latin-1 读，中文会碎。
        codepage = 1252
        for pid, where in zip(pids, vals):
            if pid in (1, 2) and u32(data, pos + where) == 2:
                raw = u16(data, pos + where + 4)
                if raw is not None:
                    codepage = raw
                break
        for pid, where in zip(pids, vals):
            off = pos + where
            vt = u32(data, off)
            if pid is None or vt is None:
                continue
            if vt == 30:  # VT_LPSTR
                n = u32(data, off + 4) or 0
                raw = data[off + 8 : off + 8 + n].rstrip(b"\x00")
                try:
                    props[pid] = raw.decode(f"cp{codepage}", "strict")
                except (LookupError, UnicodeDecodeError):
                    props[pid] = raw.decode("latin-1", "replace")
            elif vt == 31:  # VT_LPWSTR
                n = u32(data, off + 4) or 0
                props[pid] = data[off + 8 : off + 8 + n * 2].decode(
                    "utf-16-le", "replace"
                ).rstrip("\x00")
            elif vt == 3:
                props[pid] = i32(data, off + 4)
            elif vt == 19:  # VT_DATE
                props[pid] = struct.unpack_from("<d", data, off + 4)[0]
            elif vt == 64:  # VT_FILETIME
                ticks = u64(data, off + 4)
                props[pid] = {"filetime": ticks, "iso": filetime_iso(ticks)}
            elif vt == 2:
                props[pid] = struct.unpack_from("<h", data, off + 4)[0]
            elif vt == 7:
                props[pid] = u16(data, off + 4)
            elif vt == 4126:  # 计数后的 LPSTR 向量（Keywords 就是它）
                n = u32(data, off + 4) or 0
                cursor = off + 8
                items = []
                for _ in range(n):
                    ln = u32(data, cursor)
                    if ln is None:
                        break
                    cursor += 4
                    items.append(
                        data[cursor : cursor + ln].rstrip(b"\x00").decode("latin-1", "replace")
                    )
                    cursor += ln
                props[pid] = items
            elif vt == 4108:  # 计数后的 LPWSTR 向量
                n = u32(data, off + 4) or 0
                cursor = off + 8
                items = []
                for _ in range(n):
                    ln = u32(data, cursor)
                    if ln is None:
                        break
                    cursor += 4
                    items.append(
                        data[cursor : cursor + ln * 2].decode("utf-16-le", "replace").rstrip(
                            "\x00"
                        )
                    )
                    cursor += ln * 2
                props[pid] = items
            elif vt == 65:
                props[pid] = {"blob": u32(data, off + 4) or 0}
            else:
                props[pid] = {"vt": vt, "note": "un-decoded"}
        out["sets"].append(
            {"fmtid": fmtid.hex(), "property_count": len(pids), "properties": props}
        )
    return out


# ---------------------------------------------------------------- RTF
def filetime_iso(ticks):
    """FILETIME（1601-01-01 起的 100ns 数）→ ISO 日期；0 或越界就说什么也给不出"""
    if not ticks or ticks < 116444736000000000:
        return None
    import datetime

    secs = (ticks - 116444736000000000) / 10_000_000
    try:
        stamp = datetime.datetime(1970, 1, 1) + datetime.timedelta(seconds=secs)
    except OverflowError:
        return None
    return stamp.isoformat(timespec="seconds")


def docprops(path: Path) -> dict:
    """docProps/{core,app,custom,thumbnail}.xml 的全部字面值（属性名按局部名，不带前缀）"""
    out: dict = {}
    with zipfile.ZipFile(path) as box:
        names = set(one.filename for one in box.infolist())
        for part, key in (
            ("docProps/core.xml", "core"),
            ("docProps/app.xml", "app"),
            ("docProps/custom.xml", "custom"),
        ):
            if part not in names:
                continue
            root = ET.fromstring(box.read(part))
            flat: dict = {}
            for one in root.iter():
                if one.text and one.text.strip() and len(list(one)) == 0:
                    flat.setdefault(xml_local(one.tag), one.text.strip())
            out[key] = flat
        out["parts"] = sorted(one for one in names if one.startswith("docProps/"))
        out["has_thumbnail"] = any(one.startswith("docProps/thumbnail") for one in names)
    return out


def facts(path: Path) -> dict:
    data = path.read_bytes()
    out: dict = {"path": str(path), "size": len(data), "magic": data[:8].hex()}
    head = data[:8].hex().upper()
    if head == "D0CF11E0A1B11AE1":
        cfb = cfb_open(data)
        cfb_full = cfb_parse(data)
        out["container"] = "cfb"
        out["cfb"] = cfb
        cfb = cfb_full
        names = {one["name"]: one for one in cfb["streams"]}
        for stream, key in (
            ("\x05SummaryInformation", "summary"),
            ("\x05DocumentSummaryInformation", "docsummary"),
        ):
            if stream in names:
                out[key] = propset_decode(cfb_stream(data, stream))
        kinds = set(names)
        if "WordDocument" in kinds:
            out["app"] = "word"
        elif "Workbook" in kinds or "Book" in kinds:
            out["app"] = "excel"
        elif "PowerPoint Document" in kinds:
            out["app"] = "powerpoint"
        else:
            out["app"] = "unknown"
        out["has_vba"] = any("Macros" in one or "VBA" in one for one in names)
        # 遗留正文与表格记录：第二读者，Rust 那边 word.rs / biff.rs 要对得上
        streams = cfb["bytes"] if "bytes" in cfb else cfb_parse(data)["bytes"]
        if "WordDocument" in names:
            out["legacy_text"] = doc_pieces(streams)
        if "Workbook" in names or "Book" in names:
            out["biff"] = biff_workbook(streams)
            out["csv"] = biff_csv(out["biff"])
        if "PowerPoint Document" in streams:
            out["ppt_text"] = ppt_text(streams)
        return out
    if data[:2] == b"PK":
        out["container"] = "zip"
        pkg = opc_read(path)
        parts = set(pkg["parts"])
        out["opc"] = pkg
        out["docprops"] = docprops(path)
        if "word/document.xml" in parts:
            out["app"] = "word"
            out["ooxml"] = docx_facts(path)
        elif "xl/workbook.xml" in parts:
            out["app"] = "excel"
            out["ooxml"] = xlsx_facts(path)
            if path.suffix.lower() == ".xlsx":
                out["formats"] = xlsx_formats(path)
            out["csv"] = csv_facts(path)
            out["hidden"] = xlsx_hidden(path)
        elif "ppt/presentation.xml" in parts:
            out["app"] = "powerpoint"
            out["ooxml"] = pptx_facts(path)
        elif "content.xml" in parts:
            out["app"] = "opendocument"
            out["odf"] = odt_facts(path)
            out["links"] = odf_links(path)
            out["page_text"] = odf_page_text(path)
            out["odt"] = odt_structure(path)
            sheets = ods_facts(path)
            if sheets is not None:
                out["ods"] = sheets
                out["csv"] = csv_facts(path)
                out["ods_styles"] = ods_styles(path)
            deck = odp_facts(path)
            if deck is not None:
                out["odp"] = deck
        else:
            out["app"] = "unknown-zip"
        return out
    if data[:5] == b"{\\rtf":
        out["container"] = "rtf"
        out["rtf"] = rtf_text(data)
        out["rtf"]["info"] = rtf_info(data)
        out["app"] = "word"
        return out
    if data[:5] == b"%PDF-":
        # PDF 不是容器，是一张对象表：读法自成一份（lyco_pdf.py），这边只做转发
        out["container"] = "pdf"
        out["app"] = "pdf"
        out["pdf"] = lyco_pdf.pdf_facts(data)
        return out
    out["container"] = "other"
    return out


def main() -> int:
    args = sys.argv[1:] or [str(Path("lilyco-binfmt/tests/fixtures/office"))]
    target = Path(args[0])
    files = sorted(target.iterdir()) if target.is_dir() else [target]
    report = {}
    for one in files:
        if one.name.startswith(".") or one.suffix.lower() not in {
            ".doc", ".docx", ".xls", ".xlsx", ".ppt", ".pptx", ".odt", ".ods", ".odp", ".rtf", ".docm", ".dotx", ".xltm", ".xlsm", ".potx", ".pdf",
        }:
            continue
        try:
            report[one.name] = facts(one)
        except Exception as why:  # noqa: BLE001 - 报告失败原因比抛栈有用
            report[one.name] = {"error": f"{type(why).__name__}: {why}"}
    print(json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
