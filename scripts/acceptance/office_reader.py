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
import re
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
from lyco_revisions import docx_revisions, odt_revisions  # 修订那份账的第二读者
from lyco_protect import (
    docx_protection,
    ods_protection,
    odt_protection,
    xlsx_protection,
)  # 「还能动吗」那份账的第二读者
from lyco_pages import convert  # 长度与 twips 换成 0.01mm 的那条整数式子（与 paper.rs 同一条）
from lyco_pages import UNIT as MM_UNIT  # 那个单位的名字，只说一次
import lyco_pdf  # PDF 那份读者：对象表 + 对象流 + 字符串三件事（lbin office-pdf 对账）
import lyco_pdf_nav  # PDF 的「去哪儿」那一层：书签 / 链接 / 权限位

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


def docx_contents(root) -> dict:
    """这份 docx 有没有目录、收了几级。

    OOXML 里目录有两种长相：`w:sdt` 套着 `docPartGallery="Table of Contents"`
    （Word 与 LibreOffice 都这么写），以及一条 `TOC \\o "1-2" \\h` 的域指令
    （可以没有 sdt 壳，Word 老格式与很多转换器就这样）。**几级写在域指令里**，
    不是属性 —— 与 ODF 那边（`text:table-of-content-source/@outline-level`）根本两种说法。
    """
    galleries: list = []
    for one in root.iter():
        if xml_local(one.tag) == "docPartGallery":
            galleries.append(local_attr(one, "val"))
    fields: list = []
    for one in root.iter():
        tag = xml_local(one.tag)
        if tag == "instrText":
            fields.append("".join(one.itertext()).strip())
        elif tag == "fldSimple":
            fields.append((one.get("{%s}instr" % REL_NS) or "").strip())
    toc_fields = [one for one in fields if one.upper().startswith("TOC")]
    levels = None
    for one in toc_fields:
        found = re.search(r'\\o\s+"([^"]+)"', one)
        if found:
            levels = found.group(1)
            break
    gallery = "Table of Contents" in galleries
    return {
        "present": bool(gallery or toc_fields),
        "via": "doc-part-gallery" if gallery else ("field" if toc_fields else None),
        "galleries": galleries,
        "fields": toc_fields,
        "levels": levels,
        "sdt": len([one for one in root.iter() if xml_local(one.tag) == "sdt"]),
    }


def _first_kid(node, want: str):
    """直接儿子里第一个局部名等于 want 的（与 Rust 的 `Node::child` 同一条）"""
    for one in node:
        if xml_local(one.tag) == want:
            return one
    return None


def _first_any(node, want: str):
    """往下找第一个局部名等于 want 的，不含自己（与 Rust 的 `descendants(..).next()` 同一条）"""
    for one in node.iter():
        if one is not node and xml_local(one.tag) == want:
            return one
    return None


def _size_row(node):
    """一处尺寸：文件写的数与按 `paper::emu` 那条整数式子换出来的 0.01mm"""
    if node is None:
        return None
    cx = local_attr(node, "cx")
    cy = local_attr(node, "cy")
    return {
        "cx": cx,
        "cy": cy,
        "mm_w": mm_of(cx, "emu"),
        "mm_h": mm_of(cy, "emu"),
    }


def _alt_row(node):
    """一句替代文字：`descr` 在不在单列一个键（空的 `descr=""` 与没写不是一回事）"""
    if node is None:
        return None
    return {
        "id": local_attr(node, "id"),
        "name": local_attr(node, "name"),
        "descr": local_attr(node, "descr"),
        "descr_written": local_attr(node, "descr") is not None,
    }


def _position_row(node):
    """浮起来的那张图摆在哪里：相对什么写在属性上，摆在哪写在孩子的名字与文字里"""
    if node is None:
        return None
    kids = [one for one in node]
    kid = kids[0] if kids else None
    return {
        "written": written_attrs(node),
        "element": xml_local(kid.tag) if kid is not None else None,
        "value": "".join(kid.itertext()) if kid is not None else None,
    }


def docx_picture_rows(body, parts: dict) -> list:
    """文档里的图（与 Rust 的 `docx_pictures` 同一条规则）

    尺寸两处（`wp:extent` 与 `pic:spPr/a:xfrm/a:ext`）、替代文字两处（`wp:docPr` 与
    `pic:cNvPr`）、锁两处（`a:graphicFrameLocks` 与 `a:picLocks`）—— 两处的数可以不一样，
    所以两处都交、不挑一个。地址只有号（`a:blip/@r:embed`），要顺着**这份件自己的**关系表
    解成包内路径，解出来而包里又没有就交 null（Rust 那边 `resolved` 同一条）。
    """
    rels: dict[str, tuple] = {}
    if "word/_rels/document.xml.rels" in parts:
        for one in ET.fromstring(parts["word/_rels/document.xml.rels"]):
            if xml_local(one.tag) != "Relationship" or one.get("Id") in rels:
                continue
            rels[one.get("Id")] = (one.get("Target"), one.get("TargetMode") or "")
    names = set(parts)
    out = []
    for drawing in [one for one in body.iter() if xml_local(one.tag) == "drawing"]:
        frames = [one for one in drawing.iter() if xml_local(one.tag) == "inline"]
        if not frames:
            frames = [one for one in drawing.iter() if xml_local(one.tag) == "anchor"]
        if not frames:
            continue
        frame = frames[0]
        blip = _first_any(frame, "blip")
        embed = local_attr(blip, "embed") if blip is not None else None
        raw, mode = rels.get(embed, (None, "")) if embed else (None, "")
        target = None
        if raw is not None and mode != "External":
            got = opc_target("word/document.xml", raw)
            target = got if got in names else None
        wrap = next((one for one in frame
                     if xml_local(one.tag).startswith("wrap")), None)
        data = _first_any(frame, "graphicData")
        # 这里不能写 `a or b`：ElementTree 里一个没有孩子的元素是**假**的，
        # 而 `wp:extent` 恰好就是没有孩子的那个元素
        extent = _first_kid(frame, "extent")
        if extent is None:
            extent = _first_any(frame, "extent")
        out.append({
            "placed": xml_local(frame.tag),
            "written": written_attrs(frame),
            "effect_extent": written_attrs(_first_kid(frame, "effectExtent"))
            if _first_kid(frame, "effectExtent") is not None else None,
            "extent": _size_row(extent),
            "pic_extent": _size_row(_first_any(frame, "ext")),
            "simple_pos": written_attrs(_first_kid(frame, "simplePos"))
            if _first_kid(frame, "simplePos") is not None else None,
            "position_h": _position_row(_first_kid(frame, "positionH")),
            "position_v": _position_row(_first_kid(frame, "positionV")),
            "alt": _alt_row(_first_any(frame, "docPr")),
            "alt_in_picture": _alt_row(_first_any(frame, "cNvPr")),
            "blip_id": embed,
            "target": target,
            "locks": written_attrs(_first_any(frame, "graphicFrameLocks"))
            if _first_any(frame, "graphicFrameLocks") is not None else None,
            "pic_locks": written_attrs(_first_any(frame, "picLocks"))
            if _first_any(frame, "picLocks") is not None else None,
            "wrap": xml_local(wrap.tag) if wrap is not None else None,
            "wrap_written": written_attrs(wrap) if wrap is not None else None,
            "graphic_children": [xml_local(one.tag) for one in data] if data is not None else [],
        })
    return out


def pptx_picture_rows(root, parts: dict, name: str) -> list:
    """页上那张图（与 Rust 的 `slide_pictures` 同一条规则）

    pptx 只把尺寸写在 `p:spPr/a:xfrm/a:ext` **一处**（docx 那边有 `wp:extent` 与
    `a:ext` 两处而两家写的数不同），位置 `a:off` 倒写在这一层；替代文字也只有
    `p:cNvPr/@descr` 一处。python-pptx 在没给替代文字时把**源文件名**写进那个键
    （`descr="dot.png"`），所以 `descr` 按写的交、`descr_written` 只说这个属性在不在，
    「那是不是一句描述」不归读者判。号只在这页自己的关系表里解成包内路径。
    """
    stem = name[: -len(".xml")]
    rel_part = f"{stem[: stem.rindex('/')]}/_rels/{stem[stem.rindex('/') + 1 :]}.xml.rels"
    hop: dict[str, tuple] = {}
    if rel_part in parts:
        for one in ET.fromstring(parts[rel_part]):
            if xml_local(one.tag) != "Relationship" or one.get("Id") in hop:
                continue
            hop[one.get("Id")] = (one.get("Target"), one.get("TargetMode") or "")
    out = []
    for pic in [one for one in root.iter() if xml_local(one.tag) == "pic"]:
        named = _first_any(pic, "cNvPr")
        blip = _first_any(pic, "blip")
        embed = local_attr(blip, "embed") if blip is not None else None
        raw_target, mode = hop.get(embed, (None, "")) if embed else (None, "")
        target_part = None
        if raw_target is not None and mode != "External":
            got = opc_target(name, raw_target)
            target_part = got if got in parts else None
        xfrm = _first_any(pic, "xfrm")
        off = _first_kid(xfrm, "off") if xfrm is not None else None
        ext = _first_kid(xfrm, "ext") if xfrm is not None else None
        locks = _first_any(pic, "picLocks")
        stretch = _first_any(pic, "stretch")
        geom = _first_any(pic, "prstGeom")
        off_x = local_attr(off, "x") if off is not None else None
        off_y = local_attr(off, "y") if off is not None else None
        out.append({
            "id": local_attr(named, "id") if named is not None else None,
            "name": local_attr(named, "name") if named is not None else None,
            "descr": local_attr(named, "descr") if named is not None else None,
            "descr_written": named is not None and local_attr(named, "descr") is not None,
            "written": written_attrs(named) if named is not None else None,
            "blip_id": embed,
            "target": target_part,
            "ext": _size_row(ext),
            "off": {
                "x": off_x,
                "y": off_y,
                "mm_x": mm_of(off_x, "emu"),
                "mm_y": mm_of(off_y, "emu"),
            },
            "locks": written_attrs(locks) if locks is not None else None,
            # 拉伸那份写法：`<a:stretch><a:fillRect/></a:stretch>` 与一个空的 `<a:stretch/>`
            "stretch": [xml_local(one.tag) for one in stretch] if stretch is not None else None,
            "prst": local_attr(geom, "prst") if geom is not None else None,
        })
    return out


def odf_text_root(root):
    """`office:body` 里那个 `office:text`（与 Rust 那边同一条：找不到就退回整份根）"""
    for one in root.iter():
        if xml_local(one.tag) != "body":
            continue
        kid = _first_kid(one, "text")
        return kid if kid is not None else root
    return root


def odt_picture_rows(text_root, prefixes: dict) -> list:
    """ODF 里的图（与 Rust 的 `odt_pictures` 同一条规则）

    摆法写在**属性** `text:anchor-type` 上而 OOXML 写在元素名上，尺寸是自带单位的串
    （`svg:width="4.001cm"`），替代文字是**孩子元素** `svg:desc`（所以「有没有」要另问），
    地址直接写在 `draw:image/@xlink:href` 而没有关系表这一层。只数带 `draw:image` 的 frame。
    """
    out = []
    for frame in [one for one in text_root.iter()
                  if xml_local(one.tag) == "frame" and _first_kid(one, "image") is not None]:
        image = _first_kid(frame, "image")
        desc = _first_kid(frame, "desc")
        out.append({
            "written": written_kept(frame, prefixes),
            "placed": local_attr(frame, "anchor-type"),
            "style": local_attr(frame, "style-name"),
            "mm_w": mm_of(local_attr(frame, "width"), None),
            "mm_h": mm_of(local_attr(frame, "height"), None),
            "href": local_attr(image, "href"),
            "mime": local_attr(image, "mime-type"),
            "image_written": written_kept(image, prefixes),
            "alt": "".join(desc.itertext()) if desc is not None else None,
            "alt_written": desc is not None,
        })
    return out


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
        # 正文里那几张图逐张的账（两处尺寸、两处替代文字、两处锁、绕排与摆放）
        "picture_rows": docx_picture_rows(body, parts),
        "comments": comments,
        "side_texts": side_texts(parts),
        "sections": len([one for one in body.iter() if xml_local(one.tag) == "sectPr"]),
        # 段落自己写的格式（这一族不用跳样式）与一节一条的分栏
        "paragraph_formats": docx_paragraph_formats(paras),
        # 字符格式是另一本账：那一串字自己带一份 rPr（含「有这一格而里面是空的」那一种）
        "run_formats": docx_run_formats(paras, parts),
        "columns": docx_columns(body),
        # 这张表多宽的三本账（w:tblW / 网格 / 每格），一条也不替另一条圆场
        "table_layouts": docx_table_layouts(body),
        # 编号那一路要跳三跳，段上没写的那些要看样式里那份 numPr
        "numbering": docx_numbering(
            paras,
            ET.fromstring(parts["word/numbering.xml"]) if "word/numbering.xml" in parts else None,
            ET.fromstring(parts["word/styles.xml"]) if "word/styles.xml" in parts else None,
            "word/numbering.xml" in parts,
        ),
        # 部件在 ≠ 文档用了编号：notes.docx 带着 numbering.xml，正文里一个 numPr 都没有
        "has_numbering": any(xml_local(one.tag) == "numPr" for one in body.iter()),
        "numbering_part": "word/numbering.xml" in parts,
        "has_settings": "word/settings.xml" in parts,
        "has_styles_part": "word/styles.xml" in parts,
        "has_font_table": "word/fontTable.xml" in parts,
        "footnotes": _note_part_count(parts, "word/footnotes.xml", "footnote"),
        "endnotes": _note_part_count(parts, "word/endnotes.xml", "endnote"),
        "contents": docx_contents(body),
        "text": "\n".join(one for one in paragraphs if one),
        # 口径与 Rust 那边一致：每段先 trim 再数（run_text 会 trim）
        "statistics": {"ours": tally_of([one.strip() for one in paragraphs])},
    }
    return out


W_NS = "{http://schemas.openxmlformats.org/wordprocessingml/2006/main}"


REL_NS = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"


def opc_target(source: str, target: str) -> str:
    """把一条关系的 Target 解成包内路径。

    以 `/` 开头是相对包根的绝对路径（openpyxl 就这么写），否则相对**宿主部件所在目录**
    （LibreOffice 写 `../comments1.xml`）。少认一种写法就会把「有批注」读成「没有」。
    """
    if target.startswith("/"):
        return target[1:]
    head = source.rsplit("/", 1)[0] if "/" in source else ""
    parts: list[str] = [one for one in head.split("/") if one] if head else []
    for piece in target.split("/"):
        if piece in ("", "."):
            continue
        if piece == "..":
            if parts:
                parts.pop()
            continue
        parts.append(piece)
    return "/".join(parts)


def rel_target(parts: dict, source: str, kind: str) -> str | None:
    """宿主部件的关系表里第一条 Type 以 `kind` 结尾的关系，解析成包内路径。"""
    head, _, base = source.rpartition("/")
    rels = f"{head}/_rels/{base}.rels"
    if rels not in parts:
        return None
    root = ET.fromstring(parts[rels])
    for one in root.iter():
        if xml_local(one.tag) != "Relationship":
            continue
        if not (one.get("Type") or "").endswith("/" + kind):
            continue
        target = one.get("Target") or ""
        if (one.get("TargetMode") or "") == "External":
            continue
        return opc_target(source, target)
    return None


def xlsx_comments(path: Path) -> dict:
    """这张工作簿里每**张表**的批注：批注不住在 sheetN.xml 里。

    路径是 `xl/workbook.xml` →（rels）→ 这张表的部件 →（这张表自己的 rels）→
    批注部件。两个生产者把那个部件放在两个地方（`xl/comments/comment1.xml` 与
    `xl/comments1.xml`），关系 Target 也一个绝对一个相对，所以两跳都要真走。
    作者名不在 `<comment>` 上，是一个 `authorId` 下标，指向同一个部件开头的
    `<authors><author>` 列表 —— 按名字找会一条也找不到。
    """
    with zipfile.ZipFile(path) as box:
        parts = {one.filename: box.read(one.filename) for one in box.infolist()}
    if "xl/workbook.xml" not in parts:
        return {}
    book = ET.fromstring(parts["xl/workbook.xml"])
    out: dict = {}
    for sheet in [one for one in book.iter() if xml_local(one.tag) == "sheet"]:
        rid = sheet.get("{%s}id" % REL_NS)
        # 表的部件名不在 workbook.xml 里，只有 r:id；工作簿的关系表里 Id 才是键
        part = next(
            (
                opc_target("xl/workbook.xml", one.get("Target") or "")
                for one in (
                    ET.fromstring(parts["xl/_rels/workbook.xml.rels"]).iter()
                    if "xl/_rels/workbook.xml.rels" in parts
                    else []
                )
                if xml_local(one.tag) == "Relationship" and one.get("Id") == rid
            ),
            None,
        )
        name = sheet.get("name") or ""
        notes: list = []
        if part and part in parts:
            located = rel_target(parts, part, "comments")
            if located and located in parts:
                root = ET.fromstring(parts[located])
                authors = [
                    "".join(one.itertext())
                    for one in root.iter()
                    if xml_local(one.tag) == "author"
                ]
                for one in root.iter():
                    if xml_local(one.tag) != "comment":
                        continue
                    try:
                        who = authors[int(one.get("authorId") or 0)]
                    except (ValueError, IndexError):
                        who = None
                    notes.append(
                        {
                            "ref": one.get("ref"),
                            "author": who,
                            "date": one.get("date"),
                            "text": "".join(
                                got
                                for kid in one.iter()
                                if xml_local(kid.tag) == "t"
                                for got in kid.itertext()
                            ),
                        }
                    )
        out[name] = notes
    return out


PRINT_ELEMENTS = {"pageMargins": "margins", "pageSetup": "setup", "printOptions": "options"}


def xlsx_print_setup(parts: dict) -> dict:
    """每张表的打印设置：三个元素各自「在不在」

    openpyxl 只写 `pageMargins`（`pageSetup` 与 `printOptions` 整个不存在 —— 那些开关是
    「没说」，不是 false），LibreOffice 重写同一份东西把十个属性全写出来，连
    `paperSize="9"` 与两个 dpi 都不省。边距这一族是英寸的浮点串，照文件写的交：
    openpyxl 的 0.5 与 LibreOffice 的 0.511811023622047 是两个生产者的差。
    """
    out: dict = {}
    for name in sorted(parts):
        if not (name.startswith("xl/worksheets/sheet") and name.endswith(".xml")):
            continue
        root = ET.fromstring(parts[name])
        one = {"margins": None, "setup": None, "options": None, "margin_unit": "inch"}
        for node in root.iter():
            which = PRINT_ELEMENTS.get(xml_local(node.tag))
            # 只取第一个：Rust 那份是 `descendants(name).first()`，两边必须是同一条规则，
            # 否则真出现两个同名元素时两家会各挑一个
            if which and one[which] is None:
                one[which] = {key.rsplit("}", 1)[-1]: value for key, value in node.attrib.items()}
        out[name.rsplit("/", 1)[-1][: -len(".xml")]] = one
    return out


HF_SLOTS = ("oddHeader", "oddFooter", "evenHeader", "evenFooter", "firstHeader", "firstFooter")
HF_MARKS = {"L": "left", "C": "center", "R": "right"}


def first_descendant(root, want: str):
    """按局部名找第一个后代元素（含自己）—— Rust 那份是 `descendants(want).first()`，
    两边必须同一条规则，不然真出现两个同名元素时各家挑一个"""
    for one in root.iter():
        if xml_local(one.tag) == want:
            return one
    return None


def hf_scan(text: str):
    """页眉页脚那一串 `&` 码：只有 &L / &C / &R 是分段标记，别的一律原样列出来

    按码位扫而不是按字符数：`&"Calibri"` 里带引号（LibreOffice 每段前面都补一个，
    openpyxl 一个不写），`&&` 是一个货真价实的 & 而不是标记，末尾落单的 `&` 什么都不接。
    分段只交「文件自己标出来的那几段」—— 一个标记都没有时是空表，不硬造一段。
    """
    chars = list(text)
    marks: list = []
    fields: list = []
    index = 0
    while index < len(chars):
        if chars[index] != "&":
            index += 1
            continue
        if index + 1 >= len(chars):
            fields.append("&")
            index += 1
            continue
        nxt = chars[index + 1]
        if nxt == "&":
            fields.append("&&")
            index += 2
            continue
        if nxt == '"':
            stop = index + 2
            while stop < len(chars) and chars[stop] != '"':
                stop += 1
            stop = min(stop, len(chars) - 1)
            fields.append("".join(chars[index:stop + 1]))
            index = stop + 1
            continue
        if nxt in HF_MARKS:
            marks.append((HF_MARKS[nxt], index, index + 2))
            index += 2
            continue
        fields.append("&" + nxt)
        index += 2
    segments = []
    for position, entry in enumerate(marks):
        stop = marks[position + 1][1] if position + 1 < len(marks) else len(chars)
        segments.append({"at": entry[0], "text": "".join(chars[entry[2]:stop])})
    return segments, fields


def xlsx_view_of(root, limit: int = 200) -> dict:
    """这一张表的窗口状态：`sheetView` 写了哪些开关、有没有 `pane`、几条 `selection`

    两家对同一个开关的拼法不同（openpyxl 写 `showGridLines="0"`，LibreOffice 写
    `"false"` 并且把十个属性全补出来），所以属性照文件交，不折成同一个布尔。
    """
    views = [one for one in root.iter() if xml_local(one.tag) == "sheetView"]
    pane = first_descendant(root, "pane")
    return {
        "written": written_attrs(views[0]) if views else None,
        "count": len(views),
        "pane": written_attrs(pane) if pane is not None else None,
        "selections": [
            written_attrs(one)
            for one in [one for one in root.iter() if xml_local(one.tag) == "selection"][:limit]
        ],
    }


def xlsx_header_footer_of(root) -> dict:
    """这一张表的页眉页脚：六个段落固定交，`present` 说清元素在不在

    第三张表这一族干脆不写 `headerFooter`（present 全 false、text 全 null），
    LibreOffice 却六个都写出来而里面是空的（present true、text ""）—— 这两件事不能并成一谈。
    """
    holder = first_descendant(root, "headerFooter")
    slots = []
    for name in HF_SLOTS:
        node = first_descendant(root, name)
        if node is None:
            slots.append({"element": name, "present": False, "text": None,
                          "segments": [], "fields": []})
            continue
        text = node.text or ""
        segments, fields = hf_scan(text)
        slots.append({"element": name, "present": True, "text": text,
                      "segments": segments, "fields": fields})
    return {
        "written": written_attrs(holder) if holder is not None else None,
        "present": holder is not None,
        "slots": slots,
        "written_slots": len([one for one in slots if one["present"] and one["text"]]),
    }


def xlsx_layout_of(root, limit: int = 200) -> dict:
    """这一张表的尺寸账：sheetFormatPr、cols 里每一条 col、带高度的 row

    宽度与高度都按文件写的字符串交：同一列在一家是 22.5、在另一家是 20.47，同一行
    一家写 40、另一家写 39.75 —— 那是两个生产者各自的换算，折成一个数就是替文件编东西。
    `col` 一条可以顶很多列（min 与 max），所以「几条」与「盖住几列」分开交，不展开。
    """
    holder = first_descendant(root, "sheetFormatPr")
    cols = [one for one in root.iter() if xml_local(one.tag) == "col"]
    covered = 0
    exact = bool(cols)
    for one in cols:
        low, high = one.get("min"), one.get("max")
        try:
            low, high = int(low), int(high)
        except (TypeError, ValueError):
            exact = False
            continue
        if high < low:
            exact = False
            continue
        covered += high - low + 1
    rows = [one for one in root.iter() if xml_local(one.tag) == "row"]
    tall = [one for one in rows
            if any(one.get(name) is not None for name in ("ht", "customHeight", "hidden"))]
    return {
        "format": written_attrs(holder) if holder is not None else None,
        "columns": {"written": len(cols),
                    "covered": covered if exact else None,
                    "list": [written_attrs(one) for one in cols[:limit]]},
        "rows": {"elements": len(rows),
                 "with_height": len([one for one in rows if one.get("ht") is not None]),
                 "spoken": len(tall),
                 "list": [written_attrs(one) for one in tall[:limit]]},
    }


def xlsx_filter_of(root, limit: int = 200) -> dict:
    """这一张表的筛选：autoFilter 在不在、范围、哪几列在筛、筛掉之后 sheetPr 怎么说

    `filterColumn` 上的 `hiddenButton` / `showButton` 只有一家写，`filters` 上的 `blank`
    也是；被筛掉的值（`<filter val="甲"/>`）按文件写的顺序交出来。
    """
    holder = first_descendant(root, "autoFilter")
    columns = []
    for one in [one for one in root.iter() if xml_local(one.tag) == "filterColumn"][:limit]:
        kinds, vals = [], []
        for kid in one:
            if xml_local(kid.tag) != "filters":
                continue
            kinds.append("filters")
            for deep in kid:
                if xml_local(deep.tag) == "filter":
                    vals.append(deep.get("val"))
                else:
                    kinds.append("filters/%s" % xml_local(deep.tag))
        columns.append({"written": written_attrs(one), "kinds": kinds, "vals": vals})
    sheet_pr = first_descendant(root, "sheetPr")
    return {
        "present": holder is not None,
        "written": written_attrs(holder) if holder is not None else None,
        "mode": sheet_pr.get("filterMode") if sheet_pr is not None else None,
        "columns": columns,
    }


def xlsx_table_one(parts: dict, name: str, limit: int = 200) -> dict:
    """表对象那一跳指到的部件：属性照交，列名单独列出（名字是文件自己写的）"""
    if name not in parts:
        return {"part": name, "present": False}
    root = ET.fromstring(parts[name])
    holder = first_descendant(root, "table")
    columns = [one for one in root.iter() if xml_local(one.tag) == "tableColumn"]
    holder_cols = first_descendant(root, "tableColumns")
    written = holder_cols.get("count") if holder_cols is not None else None
    found = len(columns)
    inner = first_descendant(root, "autoFilter")
    style = first_descendant(root, "tableStyleInfo")
    return {
        "part": name,
        "present": True,
        "written": written_attrs(holder) if holder is not None else None,
        "columns": {"written": written, "found": found,
                    "whole": written is None or _as_int(written) == found,
                    "names": [one.get("name") for one in columns[:limit]],
                    "list": [written_attrs(one) for one in columns[:limit]]},
        "filter": {"present": inner is not None,
                   "written": written_attrs(inner) if inner is not None else None},
        "style": written_attrs(style) if style is not None else None,
    }


def _as_int(raw):
    text = str(raw).strip()
    return int(text) if text.isdigit() else None


def xlsx_tables_of(parts: dict, source: str, limit: int = 200) -> dict:
    """这一张表挂上的表对象：tableParts 自报的数与实际条数并排，部件顺着关系表找"""
    name = source
    if not (name.startswith("xl/worksheets/sheet") and name.endswith(".xml")):
        return {"tables": 0, "table_list": [], "parts": {"written": None, "found": 0, "whole": True}}
    root = ET.fromstring(parts[name])
    holder = first_descendant(root, "tableParts")
    refs = [one for one in root.iter() if xml_local(one.tag) == "tablePart"]
    written = holder.get("count") if holder is not None else None
    targets = [target for kind, target in rels_of_parts(parts, name) if kind == "table"]
    listed = [xlsx_table_one(parts, one, limit) for one in targets[:limit]]
    found = len(refs)
    return {
        "tables": len(listed),
        "table_list": listed,
        "parts": {"written": written, "found": found,
                  "whole": written is None or _as_int(written) == found,
                  "resolved": len(listed)},
    }


def xlsx_views(parts: dict) -> dict:
    out: dict = {}
    for name in sorted(parts):
        if not (name.startswith("xl/worksheets/sheet") and name.endswith(".xml")):
            continue
        out[name.rsplit("/", 1)[-1][: -len(".xml")]] = xlsx_view_of(ET.fromstring(parts[name]))
    return out


def xlsx_headers(parts: dict) -> dict:
    out: dict = {}
    for name in sorted(parts):
        if not (name.startswith("xl/worksheets/sheet") and name.endswith(".xml")):
            continue
        out[name.rsplit("/", 1)[-1][: -len(".xml")]] = xlsx_header_footer_of(
            ET.fromstring(parts[name])
        )
    return out


def _kids(node, want: str) -> list:
    """直接儿子里局部名等于 want 的那些，按文件顺序"""
    return [one for one in node if xml_local(one.tag) == want]


ODF_INDENT = ("margin-left", "margin-right", "text-indent", "auto-text-indent")


ODF_NS_PREFIX = {"http://www.w3.org/XML/1998/namespace": "xml"}


def ns_prefixes(text: str) -> dict:
    """把这份 XML 自己声明的 `xmlns:前缀="URI"` 反查成 URI → 前缀

    ODF 的属性名是带前缀的，而且 `fo:margin-left` 与 `loext:margin-left` 是两个不同的东西
    （前者的值是 `3cm`，后者是 `2ic`）—— 只按局部名收就会互相盖掉，所以这里按文件自己写的
    名字交。前缀不是契约的一部分，但**这一族是照前缀写的**，要交出「文件写了什么」就得留着它。
    """
    out = dict(ODF_NS_PREFIX)
    for prefix, uri in re.findall(r'xmlns:([A-Za-z0-9_\-]+)="([^"]*)"', text):
        out.setdefault(uri, prefix)
    return out


def written_kept(node, prefixes: dict) -> dict:
    """属性按**文件写的名字**交：`{uri}margin-left` 还原成 `fo:margin-left`，认不出前缀就交局部名"""
    out = {}
    for key, value in node.attrib.items():
        if "}" in key:
            at = key.index("}")
            head = prefixes.get(key[1:at])
            local = key[at + 1:]
            out["%s:%s" % (head, local) if head else local] = value
        else:
            out[key] = value
    return out


def docx_paragraph_formats(paras: list, limit: int = 200) -> dict:
    """docx 的段落格式：`w:jc` / `w:ind` / `w:spacing` 就写在段自己的 `w:pPr` 上

    这一族不用跳样式（样式表里那份不是「这一段写的」），所以只交段上有的；
    `w:leftChars="200"` 是第二种单位（两个字），照字符串交，`chars_written` 只是
    说「这一族的缩进里出现了 Chars 后缀」，不替它折算成厘米。
    """
    entries = []
    alignment = indents = spacings = 0
    for index, one in enumerate(paras):
        holder = _kids(one, "pPr")
        if not holder:
            continue
        kids = list(holder[0])
        jc = _kids(holder[0], "jc")
        ind = _kids(holder[0], "ind")
        spacing = _kids(holder[0], "spacing")
        style = _kids(holder[0], "pStyle")
        entry = {
            "index": index,
            "style": local_attr(style[0], "val") if style else None,
            "elements": [xml_local(kid.tag) for kid in kids],
            "alignment": local_attr(jc[0], "val") if jc else None,
            "indent": written_attrs(ind[0]) if ind else None,
            "spacing": written_attrs(spacing[0]) if spacing else None,
            "chars_written": (
                any(xml_local(key).endswith("Chars") for key in ind[0].attrib) if ind else False
            ),
        }
        alignment += 1 if jc else 0
        indents += 1 if ind else 0
        spacings += 1 if spacing else 0
        entries.append(entry)
    return {
        "checked": len(paras),
        "listed": len(entries),
        "with_alignment": alignment,
        "with_indent": indents,
        "with_spacing": spacings,
        "list": entries[:limit],
    }


RUN_SWITCHES = (("b", "bold"), ("i", "italic"), ("strike", "strike"), ("u", "underline"))
RUN_VALUES = (
    ("color", "color"),
    ("highlight", "highlight"),
    ("sz", "size"),
    ("vertAlign", "position"),
    ("rFonts", "fonts"),
)
# 「关」的四种写法：元素的**存在**本身不算表态（`<w:b w:val="0"/>` 说的是「不粗」）
RUN_OFF = ("0", "false", "off", "none")
# ODF 那四个开关住在样式的 `style:text-properties` 上，拼法与 OOXML 完全不同
ODF_RUN_SWITCHES = (
    ("fo:font-weight", "bold"),
    ("fo:font-style", "italic"),
    ("style:text-line-through-style", "strike"),
    ("style:text-underline-style", "underline"),
)
ODF_RUN_OFF = ("normal", "none")
# 认得的「段里的一条元素」：链接与那四种自己算的都与 span 走同一趟账
ODF_PIECES = ("span", "a", "date", "time", "sequence", "page-number", "expression")
ODF_FIELDS = ODF_PIECES[2:]
# 一串字里的「引用」那三种：元素名、它跳去的那本账、交出去用的键名
RUN_REF_KINDS = (
    ("footnoteReference", "footnote", "footnote"),
    ("endnoteReference", "endnote", "endnote"),
    ("commentReference", "comment", "comment"),
)


def docx_note_index(parts: dict) -> list:
    """注那两份部件里的号：`(文件写的 id, 是哪一种)`，按文件顺序

    分隔符两条（`separator` / `continuationSeparator`）不算注 —— 与
    `structure.footnotes` 那本账同一个口径，两边数不一样就会在探针里露出来。
    """
    out = []
    for part, kind in (("word/footnotes.xml", "footnote"), ("word/endnotes.xml", "endnote")):
        if part not in parts:
            continue
        root = ET.fromstring(parts[part])
        for one in _all(root, kind):
            if local_attr(one, "type") in ("separator", "continuationSeparator"):
                continue
            ident = local_attr(one, "id")
            if ident is not None:
                out.append((ident, kind))
    return out


def docx_bookmark_names(parts: dict) -> list:
    """`w:bookmarkStart` 写的那几个名字：站内跳转的 `w:anchor` 对着名字查，不对着号查"""
    if "word/document.xml" not in parts:
        return []
    root = ET.fromstring(parts["word/document.xml"])
    return [
        local_attr(one, "name")
        for one in _all(root, "bookmarkStart")
        if local_attr(one, "name") is not None
    ]


def _run_text(run) -> str:
    """一串字的「字」只算**直接**的 `w:t`：域指令不是页面上的字，图与引用更不是；
    往全树找还会把文本框里另一块的字算进来（那一块有自己的账）"""
    return "".join(str(kid.text or "") for kid in _kids(run, "t"))


def _run_switch(holder, name: str):
    """`rPr` 里那一条开关：没这个孩子 = 没说；有孩子没写 val = 开；写了才按字面判"""
    if holder is None:
        return None
    got = _kids(holder, name)
    if not got:
        return None
    raw = local_attr(got[0], "val")
    return True if raw is None else raw not in RUN_OFF


def _docx_runs(para) -> list:
    """段里的串：直接坐在段下的 `w:r`，加上超链接与修订那三个壳里的，
    每条同时带上**包着它的那个壳**（没有壳就是 None）。不往全树找，
    免得把文本框里另一段的字算到这一段头上"""
    out = []
    for kid in para:
        name = xml_local(kid.tag)
        if name == "r":
            out.append((kid, None))
        elif name in ("hyperlink", "ins", "del"):
            out.extend((one, kid) for one in kid if xml_local(one.tag) == "r")
    return out


def docx_character_styles(parts: dict) -> dict:
    """`word/styles.xml` 里 `w:type="character"` 的那些定义：号 → (名字, 父号, 孩子的账)

    定义里的 `w:rStyle` 不算它「自己说的格式」（那是继承链上的一节），所以滤掉；
    名字与父**只报不跟**（链是文件的，不是读者编的）。
    """
    out = {}
    if "word/styles.xml" not in parts:
        return out
    root = ET.fromstring(parts["word/styles.xml"])
    for one in _all(root, "style"):
        if local_attr(one, "type") != "character":
            continue
        ident = local_attr(one, "styleId")
        if ident is None or ident in out:
            continue
        name = _kids(one, "name")
        parent = _kids(one, "basedOn")
        holder = _kids(one, "rPr")
        kids = [
            kid
            for kid in (holder[0] if holder else [])
            if xml_local(kid.tag) != "rStyle"
        ]
        out[ident] = (
            local_attr(name[0], "val") if name else None,
            local_attr(parent[0], "val") if parent else None,
            [
                {"element": xml_local(kid.tag), "written": written_attrs(kid)}
                for kid in kids
            ],
        )
    return out


def docx_run_formats(paras: list, parts: dict, limit: int = 200) -> dict:
    """docx 的字符格式：每一串字自己带一份 `w:rPr`，与段上那份 `w:pPr` 是两本账

    `with_props`（有 rPr 这一格）与 `props_empty`（有而里面一个格式孩子都没有）分开数：
    python-docx 不给没格式的那一串写 rPr，LibreOffice 重写时给每一串都补一个空的。
    """
    sheet = docx_character_styles(parts)
    notes = docx_note_index(parts)
    marks = docx_bookmark_names(parts)
    seen_refs = []
    entries = []
    checked = with_props = props_empty = with_format = 0
    with_style = style_found = where_both = 0
    with_text = with_ref = ref_found = field_runs = 0
    with_wrap = wrap_link = wrap_ins = wrap_del = 0
    with_anchor = anchor_found = 0
    on = {key: 0 for _, key in RUN_SWITCHES}
    off = {key: 0 for _, key in RUN_SWITCHES}
    from_style = {key: 0 for _, key in RUN_SWITCHES}
    for index, para in enumerate(paras):
        for at, (run, wrap) in enumerate(_docx_runs(para)):
            checked += 1
            props = _kids(run, "rPr")
            holder = props[0] if props else None
            kids = [kid for kid in holder] if holder is not None else []
            if holder is not None:
                with_props += 1
                if not kids:
                    props_empty += 1
            if kids:
                with_format += 1
            switches = {key: _run_switch(holder, name) for name, key in RUN_SWITCHES}
            for _, key in RUN_SWITCHES:
                if switches[key] is True:
                    on[key] += 1
                elif switches[key] is False:
                    off[key] += 1
            values = {}
            for name, key in RUN_VALUES:
                got = _kids(holder, name) if holder is not None else []
                if not got:
                    values[key] = None
                elif name == "rFonts":
                    values[key] = written_attrs(got[0])
                else:
                    values[key] = local_attr(got[0], "val")
            holder_style = _kids(holder, "rStyle") if holder is not None else []
            named = local_attr(holder_style[0], "val") if holder_style else None
            got = sheet.get(named) if named else None
            if named is not None:
                with_style += 1
            if got is not None:
                style_found += 1
            char_switches = {}
            for name, key in RUN_SWITCHES:
                said = [one for one in (got[2] if got else []) if one["element"] == name]
                if not said:
                    value = None
                else:
                    raw = said[0]["written"].get("val")
                    value = True if raw is None else raw not in RUN_OFF
                if value is not None and switches[key] is not None:
                    where_both += 1
                if value is not None and switches[key] is None:
                    from_style[key] += 1
                char_switches[key] = value
            body = [kid for kid in run if xml_local(kid.tag) not in ("#text", "rPr")]
            own = _run_text(run)
            if own:
                with_text += 1
            refs = {}
            note = None
            for element, ledger, key in RUN_REF_KINDS:
                mark = _kids(run, element)
                ident = local_attr(mark[0], "id") if mark else None
                # 批注的号也交，但它不跳注那两份部件（批注住在 comments.xml，另有一本账）
                if ident is not None and ledger != "comment":
                    seen_refs.append((ident, ledger))
                    # 号是分种类的两条账：脚注的 `2` 与尾注的 `2` 是两条不同的注，只按号对会串门
                    found = [
                        one for one in notes if one[0] == ident and one[1] == ledger
                    ]
                    note = {
                        "kind": ledger,
                        "id": ident,
                        "found": bool(found),
                        "at": notes.index(found[0]) if found else None,
                    }
                    if found:
                        ref_found += 1
                refs[key] = ident
            if any(value is not None for value in refs.values()):
                with_ref += 1
            breaks = [local_attr(kid, "type") for kid in body if xml_local(kid.tag) == "br"]
            instructions = [
                str(kid.text or "") for kid in body if xml_local(kid.tag) == "instrText"
            ]
            fields = [
                local_attr(kid, "fldCharType")
                for kid in body
                if xml_local(kid.tag) == "fldChar"
            ]
            if instructions or fields:
                field_runs += 1
            # 这一串字是被谁包起来的：壳上有作者、时间、链接的号与锚，而串自己一个都没有
            wrapper = xml_local(wrap.tag) if wrap is not None else None
            wrapped = written_attrs(wrap) if wrap is not None else None
            if wrapper is not None:
                with_wrap += 1
            if wrapper == "hyperlink":
                wrap_link += 1
            elif wrapper == "ins":
                wrap_ins += 1
            elif wrapper == "del":
                wrap_del += 1
            # 站内跳转的地址写在 `w:anchor` 上（外部链接写的是 `r:id`），而它对的是书签的**名字**
            anchor = local_attr(wrap, "anchor") if wrap is not None else None
            link_found = None if anchor is None else (anchor in marks)
            if anchor is not None:
                with_anchor += 1
                if link_found:
                    anchor_found += 1
            entries.append(
                {
                    "para": index,
                    "at": at,
                    "text": own,
                    "props_written": holder is not None,
                    "props_attrs": written_attrs(holder) if holder is not None else None,
                    "elements": [xml_local(kid.tag) for kid in kids],
                    "switches": switches,
                    "values": values,
                    "format": [
                        {"element": xml_local(kid.tag), "written": written_attrs(kid)}
                        for kid in kids
                        if xml_local(kid.tag) != "rStyle"
                    ],
                    # 样式那一跳：`w:rStyle` 只有一个号，那句话住在另一个部件里
                    "style": named,
                    "style_found": None if named is None else got is not None,
                    "style_name": got[0] if got else None,
                    "style_parent": got[1] if got else None,
                    "style_format": got[2] if got else None,
                    "style_switches": char_switches,
                    "contents": [
                        {"element": xml_local(kid.tag), "written": written_attrs(kid)}
                        for kid in body
                    ],
                    "refs": refs,
                    "note": note,
                    "wrapped": wrapper,
                    "wrapped_written": wrapped,
                    "link_anchor": anchor,
                    "link_found": link_found,
                    "breaks": breaks,
                    "instructions": instructions,
                    "field_chars": fields,
                }
            )
    out = {
        "checked": checked,
        "listed": len(entries),
        "with_props": with_props,
        "props_empty": props_empty,
        "with_format": with_format,
        "with_style": with_style,
        "style_found": style_found,
        "where_both_spoke": where_both,
        "runs_with_text": with_text,
        "runs_with_ref": with_ref,
        "ref_found": ref_found,
        "field_runs": field_runs,
        "runs_wrapped": with_wrap,
        "wrapped_hyperlink": wrap_link,
        "wrapped_ins": wrap_ins,
        "wrapped_del": wrap_del,
        "runs_with_anchor": with_anchor,
        "anchors_found": anchor_found,
        "anchors_missing": with_anchor - anchor_found,
        "bookmark_names": len(marks),
        "notes_in_parts": len(notes),
        "notes_referenced": len(set(seen_refs)),
        "notes_unreferenced": len(notes) - len(set(seen_refs)),
        "list": entries[:limit],
    }
    for key in ("bold", "italic", "strike", "underline"):
        out["%s_on" % key] = on[key]
        out["%s_off" % key] = off[key]
        out["%s_from_style" % key] = from_style[key]
    return out


def _keep_text(raw):
    """与 Rust 的 `push_text` 同一条：空的不算，「纯空白而且带换行」的是排版噪声也不算"""
    if raw is None or raw == "":
        return None
    if raw.strip() == "" and ("\n" in raw or "\r" in raw):
        return None
    return raw


def _own_text(node) -> str:
    """一条 span 自己**直接**带的那些字（head 加上每个孩子的 tail），与 Rust 的 `own_text` 同一条"""
    parts = []
    head = _keep_text(node.text)
    if head is not None:
        parts.append(head)
    for kid in node:
        tail = _keep_text(kid.tail)
        if tail is not None:
            parts.append(tail)
    return "".join(parts)


def _para_pieces(node, depth: int = 1) -> list:
    """一段里的「串」按文件的顺序摊平成 (元素, 那份字或那个元素, 第几层)

    `text:span` 可以套 `text:span`（实测 LibreOffice 把「样式说斜、段上自己说粗」写成
    外面一层点 `Emphasis`、里面一层点 `T1`），所以这一趟是递归的。套在里面那些字
    **只算在外层那条 span 的 text 上**，不再单独出一条不包起来的字 —— 同一句话在两层
    各出现一次，条数就成了读者造出来的。别的元素（注、软分页、书签…）整块跳过：
    注有自己那份账，不在带它的那一段里再算一遍。
    """
    out: list = []

    def walk(holder, level: int, bare_text: bool) -> None:
        head = _keep_text(holder.text)
        if head is not None and bare_text:
            out.append(("#text", head, level))
        for kid in holder:
            name = xml_local(kid.tag)
            if name in ODF_PIECES:
                out.append((name, kid, level))
                walk(kid, level + 1, False)
            # 一个 span 后面的那半句字是**这一段**的话（不是里层那条 span 的），
            # 里层那些字只算在里层那条的 text 上
            tail = _keep_text(kid.tail)
            if tail is not None and bare_text:
                out.append(("#text", tail, level))

    walk(node, depth, True)
    return out


def _odf_run_switch(written, name: str):
    if not isinstance(written, dict):
        return None
    raw = written.get(name)
    return None if raw is None else raw not in ODF_RUN_OFF


def odf_run_formats(
    paras: list,
    root,
    prefixes: dict,
    styles_root=None,
    styles_prefixes: dict | None = None,
    limit: int = 200,
) -> dict:
    """ODF 的字符格式：`text:span` 点名一个 family=text 的样式，值在它的
    `style:text-properties` 上；夹在 span 中间的字文件根本没给它们立元素（`#text` 那一行）

    那一跳找两处：content.xml（LibreOffice 把 T1…T12 这些自动样式写在这儿）先，
    styles.xml 后，`found_in` 说住在哪一份。同名取先看到的那一个（Rust 那边是 `find`）。
    """
    found: dict[str, tuple] = {}
    for part, tree, table in (
        ("content", root, prefixes),
        ("styles", styles_root, styles_prefixes or prefixes),
    ):
        if tree is None:
            continue
        for one in _all(tree, "style"):
            if local_attr(one, "family") != "text":
                continue
            name = local_attr(one, "name")
            if name is None or name in found:
                continue
            props = _kids(one, "text-properties")
            found[name] = (
                local_attr(one, "parent-style-name"),
                written_kept(props[0], table) if props else None,
                local_attr(one, "display-name"),
                part,
            )
    marks = [
        local_attr(one, "name")
        for one in _all(root, "bookmark-start")
        if local_attr(one, "name") is not None
    ]
    entries = []
    spans = nested = bare = resolved = with_format = 0
    links = fields = with_anchor = anchor_found = 0
    on = {key: 0 for _, key in ODF_RUN_SWITCHES}
    off = {key: 0 for _, key in ODF_RUN_SWITCHES}
    for index, para in enumerate(paras):
        for at, (element, piece, depth) in enumerate(_para_pieces(para)):
            if element != "#text":
                # 链接与域各是一条，不并进 spans 那本账（那一条说的是「点一个字符样式的串」）
                if element == "a":
                    links += 1
                elif element in ODF_FIELDS:
                    fields += 1
                else:
                    spans += 1
                raw_href = local_attr(piece, "href")
                anchor = raw_href[1:] if raw_href and raw_href.startswith("#") else None
                found_anchor = None if anchor is None else (anchor in marks)
                if anchor is not None:
                    with_anchor += 1
                    if found_anchor:
                        anchor_found += 1
                name = local_attr(piece, "style-name")
                got = found.get(name) if name else None
                if got is not None:
                    resolved += 1
                written = got[1] if got else None
                if isinstance(written, dict) and written:
                    with_format += 1
                if depth > 1:
                    nested += 1
                switches = {
                    key: _odf_run_switch(written, attr) for attr, key in ODF_RUN_SWITCHES
                }
                for _, key in ODF_RUN_SWITCHES:
                    if switches[key] is True:
                        on[key] += 1
                    elif switches[key] is False:
                        off[key] += 1
                entries.append(
                    {
                        "para": index,
                        "at": at,
                        "element": element,
                        "depth": depth,
                        "text": _own_text(piece),
                        "style": name,
                        "resolved": got is not None,
                        "found_in": got[3] if got else None,
                        "parent": got[0] if got else None,
                        "display": got[2] if got else None,
                        "written": written,
                        "own_written": written_kept(piece, prefixes),
                        "link_href": raw_href,
                        "link_anchor": anchor,
                        "link_found": found_anchor,
                        "switches": switches,
                    }
                )
                continue
            bare += 1
            entries.append(
                {
                    "para": index,
                    "at": at,
                    "element": "#text",
                    "depth": depth,
                    "text": piece,
                    "style": None,
                    "resolved": None,
                    "found_in": None,
                    "parent": None,
                    "display": None,
                    "written": None,
                    "own_written": None,
                    "link_href": None,
                    "link_anchor": None,
                    "link_found": None,
                    "switches": {key: None for _, key in ODF_RUN_SWITCHES},
                }
            )
    out = {
        "checked": spans + bare + links + fields,
        "listed": len(entries),
        "spans": spans,
        "nested_spans": nested,
        "links": links,
        "field_pieces": fields,
        "bookmarks_written": len(marks),
        "runs_with_anchor": with_anchor,
        "anchors_found": anchor_found,
        "anchors_missing": with_anchor - anchor_found,
        "bare_text": bare,
        "resolved": resolved,
        "with_format": with_format,
        "list": entries[:limit],
    }
    for key in ("bold", "italic", "strike", "underline"):
        out["%s_on" % key] = on[key]
        out["%s_off" % key] = off[key]
    return out


def docx_columns(body, limit: int = 200) -> dict:
    """docx 的分栏：一节一条 `sectPr/w:cols`（`num` 省掉就是没说要分栏）"""
    entries = []
    for index, sect in enumerate(_all(body, "sectPr")[:limit]):
        cols = _kids(sect, "cols")
        entries.append(
            {
                "at": index,
                "present": bool(cols),
                "written": written_attrs(cols[0]) if cols else None,
                "count": local_attr(cols[0], "num") if cols else None,
                "space": local_attr(cols[0], "space") if cols else None,
                "parts": [written_attrs(one) for one in _kids(cols[0], "column")] if cols else [],
            }
        )
    return {
        "sections": len(entries),
        "written": len([one for one in entries if one["present"]]),
        "multi": len([one for one in entries
                      if (one["count"] or "").isdigit() and int(one["count"]) > 1]),
        "list": entries,
    }


def _all(root, want: str) -> list:
    """后代里按局部名挑（文档顺序），与 Rust 的 descendants(want) 同一条规则"""
    return [one for one in root.iter() if xml_local(one.tag) == want]


def odf_paragraph_formats(paras: list, root, prefixes: dict, limit: int = 200) -> dict:
    """ODF 的段落格式：段上只有一个样式名，属性在 `style:paragraph-properties` 上

    一跳：`text:p/@text:style-name` → 同一个部件里那个 `style:style` →
    `style:paragraph-properties` 的属性。**只找得到 content.xml 里那一份**：
    第三段点名的 `Standard` 住在 styles.xml，那是文档默认，不是这一段写的 ——
    所以那条 `resolved: false`、`written: null`，父样式链更不去猜。
    """
    styles = {}
    for one in _all(root, "style"):
        name = local_attr(one, "name")
        if name is None or local_attr(one, "family") not in ("paragraph", "text"):
            continue
        if name in styles:
            continue  # 同名样式取第一个：Rust 那边是 `find`，两边必须同一条规则
        props = _kids(one, "paragraph-properties")
        styles[name] = (
            local_attr(one, "family"),
            local_attr(one, "parent-style-name"),
            written_kept(props[0], prefixes) if props else None,
        )
    entries = []
    resolved = 0
    for index, one in enumerate(paras):
        name = local_attr(one, "style-name")
        got = styles.get(name) if name else None
        entry = {
            "index": index,
            "style": name,
            "resolved": got is not None,
            "family": got[0] if got else None,
            "parent": got[1] if got else None,
            "written": got[2] if got else None,
        }
        if got and got[2] is not None:
            written = got[2]
            entry["alignment"] = written.get("fo:text-align")
            entry["indent"] = {
                key: value for key, value in written.items() if key.rsplit(":", 1)[-1] in ODF_INDENT
            }
            entry["spacing"] = {
                key: value
                for key, value in written.items()
                if key.rsplit(":", 1)[-1] in ("margin-top", "margin-bottom", "line-height",
                                              "contextual-spacing")
            }
            resolved += 1
        entries.append(entry)
    return {
        "checked": len(paras),
        "listed": len(entries),
        "resolved": resolved,
        "list": entries[:limit],
    }


def odf_columns(root, prefixes: dict, limit: int = 200) -> dict:
    """ODF 的分栏：`text:section` 点名一个 family=section 的样式，栏在它的 section-properties 里

    与 docx 完全不同：那里是「一节一条 w:cols」，这里是「一个内联区一个区样式」，
    而且每一栏还各写一份 `style:column`（宽度是 `rel-width="32767*"` 那种相对数）。
    """
    styles = {}
    for one in _all(root, "style"):
        name = local_attr(one, "name")
        if name is None or local_attr(one, "family") != "section":
            continue
        if name in styles:
            continue  # 同名区样式取第一个，与 Rust 的 `find` 同一条规则
        props = _kids(one, "section-properties")
        cols = _kids(props[0], "columns") if props else []
        styles[name] = (
            written_kept(cols[0], prefixes) if cols else None,
            [written_kept(kid, prefixes) for kid in _kids(cols[0], "column")] if cols else [],
            local_attr(props[0], "dont-balance-text-columns") if props else None,
            bool(props),
        )
    entries = []
    for one in _all(root, "section")[:limit]:
        name = local_attr(one, "style-name")
        got = styles.get(name) if name else None
        entries.append(
            {
                "name": local_attr(one, "name"),
                "style": name,
                "resolved": got is not None,
                "has_properties": bool(got[3]) if got else False,
                "written": got[0] if got else None,
                "parts": got[1] if got else [],
                "dont_balance": got[2] if got else None,
            }
        )
    return {
        "sections": len(entries),
        # 「写了栏这件事」= 那个 `style:columns` 元素在，哪怕它是空的（Rust 那边判的是 null，
        # 一个 `<style:columns/>` 在两家都算说过话，所以这里判 is not None 而不是判真假）
        "written": len([one for one in entries if one["written"] is not None]),
        "list": entries,
    }


def odf_attr(node, want: str):
    """按局部名取属性，但躲开 LibreOffice 抄的那份 `calcext:`（与 Rust 的 `attr_of` 同一条）"""
    for key, value in node.attrib.items():
        head, _, local = key.rpartition("}")
        local = local.rsplit(":", 1)[-1]
        if local != want:
            continue
        if "calcext" in head:
            continue
        return value
    return None


def _kid(node, want: str):
    """直接儿子里第一个局部名等于 want 的（与 Rust 的 `child` 同一条）"""
    for one in node:
        if xml_local(one.tag) == want:
            return one
    return None


def docx_num_pr(pPr):
    """`w:pPr` 里那个 `w:numPr`：两个开关各交自己写的那个数，没写交 None"""
    if pPr is None:
        return None, None, False
    inner = _kid(pPr, "numPr")
    if inner is None:
        return None, None, False
    ilvl = _kid(inner, "ilvl")
    num_id = _kid(inner, "numId")
    return (
        ilvl.get(W_NS + "val") if ilvl is not None else None,
        num_id.get(W_NS + "val") if num_id is not None else None,
        True,
    )


def docx_val_children(node) -> dict:
    """一个元素下那些「子元素各带一个 `w:val`」的收成一张表，同名取第一条"""
    out = {}
    for kid in node:
        name = xml_local(kid.tag)
        if name in ("pPr", "rPr") or name in out:
            continue
        val = kid.get(W_NS + "val")
        if val is not None:
            out[name] = val
    return out


def docx_level_written(node) -> dict:
    """一份 `w:lvl`：格式串与起始值在 `written`，缩进与字体各一份属性表"""
    pPr = _kid(node, "pPr")
    rPr = _kid(node, "rPr")
    ind = _kid(pPr, "ind") if pPr is not None else None
    fonts = _kid(rPr, "rFonts") if rPr is not None else None
    return {
        "ilvl": node.get(W_NS + "ilvl"),
        "written": docx_val_children(node),
        "indent": written_attrs(ind) if ind is not None else None,
        "fonts": written_attrs(fonts) if fonts is not None else None,
    }


def docx_numbering(paras: list, numbering, style_root, has_part: bool, limit: int = 200) -> dict:
    """docx 的编号：段上那份 `w:numPr` 与段点名的样式里那份，两路都看，三跳走到底"""
    nums: dict[str, str | None] = {}
    abstracts: dict[str, dict] = {}
    if numbering is not None:
        for one in numbering:
            if xml_local(one.tag) != "num":
                continue
            ident = one.get(W_NS + "numId")
            if ident is None or ident in nums:
                continue
            held = _kid(one, "abstractNumId")
            nums[ident] = held.get(W_NS + "val") if held is not None else None
        for one in numbering:
            if xml_local(one.tag) != "abstractNum":
                continue
            ident = one.get(W_NS + "abstractNumId")
            if ident is None or ident in abstracts:
                continue
            abstracts[ident] = {
                "written": docx_val_children(one),
                "levels": [
                    docx_level_written(kid)
                    for kid in list(one)
                    if xml_local(kid.tag) == "lvl"
                ][:limit],
            }
    style_nums: dict[str, tuple] = {}
    if style_root is not None:
        for one in style_root:
            if xml_local(one.tag) != "style":
                continue
            ident = one.get(W_NS + "styleId")
            if ident is None or ident in style_nums:
                continue
            style_nums[ident] = docx_num_pr(_kid(one, "pPr"))[0:2]
    entries = []
    used: list[str] = []
    on_paragraph = via_style = both = unresolved = 0
    for index, one in enumerate(paras):
        holder = _kid(one, "pPr")
        if holder is None:
            continue
        ilvl, num_id, para_has = docx_num_pr(holder)
        style = _kid(holder, "pStyle")
        style_name = style.get(W_NS + "val") if style is not None else None
        got = style_nums.get(style_name) if style_name else None
        style_has = bool(got and got[1] is not None)
        if not para_has and not style_has:
            continue
        if para_has and style_has:
            from_where = "both"
            both += 1
        elif para_has:
            from_where = "paragraph"
            on_paragraph += 1
        else:
            from_where = "style"
            via_style += 1
        chosen_id = num_id if num_id is not None else (got[1] if got else None)
        chosen_ilvl = ilvl if ilvl is not None else (got[0] if got else None)
        abstract = nums.get(chosen_id) if chosen_id in nums else None
        held = abstract is not None and chosen_id in nums
        definition = abstracts.get(abstract) if abstract else None
        level = None
        if definition:
            for one2 in definition["levels"]:
                if one2["ilvl"] == chosen_ilvl:
                    level = one2
                    break
        if chosen_id is not None and chosen_id not in used:
            used.append(chosen_id)
        if chosen_id is not None and chosen_id not in nums:
            unresolved += 1
        entries.append(
            {
                "index": index,
                "style": style_name,
                "from": from_where,
                "num_id": chosen_id,
                "ilvl": chosen_ilvl,
                "para_num_id": num_id,
                "style_num_id": got[1] if got else None,
                "abstract": abstract,
                "resolved": chosen_id in nums if chosen_id is not None else False,
                "abstract_found": definition is not None,
                "level_found": level is not None,
                "level": level,
            }
        )
    definitions = []
    for key in list(nums)[:limit]:
        definition = abstracts.get(nums[key]) if nums[key] in abstracts else None
        definitions.append(
            {
                "num_id": key,
                "abstract": nums[key],
                "abstract_found": definition is not None,
                "written": definition["written"] if definition else None,
                "referenced": key in used,
                "levels": definition["levels"] if definition else [],
            }
        )
    return {
        "part": has_part,
        "nums": len(nums),
        "abstracts": len(abstracts),
        "checked": len(paras),
        "listed": len(entries),
        "on_paragraph": on_paragraph,
        "via_style": via_style,
        "both": both,
        "unresolved": unresolved,
        "used": used,
        "list": entries[:limit],
        "definitions": definitions,
    }


def odf_list_depths(node, depth: int, chain: list, into: list) -> None:
    """与 Rust 的 `odf_list_depths` 同一条：`text:list` 进一层加一档，批注与修订表绕开"""
    for kid in node:
        tag = xml_local(kid.tag)
        if tag in ("annotation", "tracked-changes"):
            continue
        if tag == "p":
            into.append((depth, list(chain)))
            continue
        if tag == "list":
            chain.append(odf_attr(kid, "style-name"))
            odf_list_depths(kid, depth + 1, chain, into)
            chain.pop()
            continue
        odf_list_depths(kid, depth, chain, into)


def odf_numbering(paras: list, body, content_root, style_root, prefixes: dict,
                  limit: int = 200) -> dict:
    """ODF 的编号：级别是嵌套层数，定义常在另一个部件里"""
    depths: list = []
    odf_list_depths(body, 0, [], depths)
    roots = [(content_root, "content.xml")]
    if style_root is not None:
        roots.append((style_root, "styles.xml"))
    para_styles: dict[str, tuple] = {}
    list_styles: dict[str, dict] = {}
    for root, part in roots:
        for one in root.iter():
            if xml_local(one.tag) != "style":
                continue
            name = odf_attr(one, "name")
            if name is None or name in para_styles:
                continue
            para_styles[name] = (odf_attr(one, "list-style-name"), part)
        for one in root.iter():
            if xml_local(one.tag) != "list-style":
                continue
            name = odf_attr(one, "name")
            if name is None or name in list_styles:
                continue
            kids = list(one)
            list_styles[name] = {
                "found": len(kids),
                "part": part,
                "levels": [
                    {
                        "kind": xml_local(kid.tag),
                        "level": odf_attr(kid, "level"),
                        "written": written_kept(kid, prefixes),
                    }
                    for kid in kids
                ][:limit],
            }
    entries = []
    in_list = max_depth = resolved = 0
    for index, one in enumerate(paras):
        depth, chain = depths[index] if index < len(depths) else (0, [])
        style = odf_attr(one, "style-name")
        got = para_styles.get(style) if style else None
        named = got[0] if got else None
        style_part = got[1] if got else None
        if depth == 0 and named is None:
            continue
        definition = list_styles.get(named) if named else None
        if depth > 0:
            in_list += 1
        max_depth = max(max_depth, depth)
        if definition is not None:
            resolved += 1
        level = None
        if definition:
            for one2 in definition["levels"]:
                try:
                    if one2["level"] is not None and int(one2["level"]) == depth:
                        level = one2
                        break
                except ValueError:
                    continue
        entries.append(
            {
                "index": index,
                "style": style,
                "style_part": style_part,
                "list_style": named,
                "list_part": definition["part"] if definition else None,
                "depth": depth,
                "chain": chain,
                "resolved": definition is not None,
                "level_found": level is not None,
                "level": level,
            }
        )
    definitions = [
        {"name": key, "found": list_styles[key]["found"], "part": list_styles[key]["part"],
         "levels": list_styles[key]["levels"]}
        for key in list(list_styles)[:limit]
    ]
    return {
        "lists": sum(1 for one in body.iter() if xml_local(one.tag) == "list"),
        "items": sum(1 for one in body.iter() if xml_local(one.tag) == "list-item"),
        "checked": len(paras),
        "listed": len(entries),
        "in_list": in_list,
        "max_depth": max_depth,
        "resolved": resolved,
        "styles": len(list_styles),
        "in_content": sum(1 for one in list_styles.values() if one["part"] == "content.xml"),
        "in_styles": sum(1 for one in list_styles.values() if one["part"] == "styles.xml"),
        "list": entries[:limit],
        "definitions": definitions,
    }


def mm_of(raw, unit_hint):
    """换成 0.01mm；不是这个形状的串就交 None（Rust 那边 parse 不上也是 None，不抛）"""
    if raw is None:
        return None
    try:
        return convert(raw, unit_hint)[0]
    except (ValueError, KeyError, IndexError):
        return None


def docx_table_layouts(body, limit: int = 200) -> dict:
    """docx 这张表多宽：`w:tblW`、`w:tblGrid/w:gridCol`、每一格自己的 `w:tcW`，三本账各数各的

    计数只数**上榜的那几本表**（与 Rust 那边同一个封顶口径），行与格只取这张表自己的
    直接孩子，套在格里的另一张表不算进来。
    """
    tbls = [one for one in body.iter() if xml_local(one.tag) == "tbl"][:limit]
    entries = []
    said = auto_said = grid_sum = 0
    shade_cells = border_cells = empty_border_cells = align_cells = 0
    for at, tbl in enumerate(tbls):
        props = _kid(tbl, "tblPr")
        width = _kid(props, "tblW") if props is not None else None
        raw = width.get(W_NS + "w") if width is not None else None
        kind = width.get(W_NS + "type") if width is not None else None
        said += 1 if width is not None else 0
        auto_said += 1 if kind == "auto" else 0
        grid_holder = _kid(tbl, "tblGrid")
        grid = (
            [written_attrs(kid) for kid in grid_holder if xml_local(kid.tag) == "gridCol"]
            if grid_holder is not None
            else []
        )
        for one in grid:
            grid_sum += mm_of(one.get("w"), "twips") or 0
        rows = [one for one in tbl if xml_local(one.tag) == "tr"]
        cells = []
        for row, tr in enumerate(rows):
            for col, tc in enumerate([one for one in tr if xml_local(one.tag) == "tc"]):
                tc_pr = _kid(tc, "tcPr")
                one_width = _kid(tc_pr, "tcW") if tc_pr is not None else None
                span = _kid(tc_pr, "gridSpan") if tc_pr is not None else None
                shd = _kid(tc_pr, "shd") if tc_pr is not None else None
                edges = _kid(tc_pr, "tcBorders") if tc_pr is not None else None
                align = _kid(tc_pr, "vAlign") if tc_pr is not None else None
                border_map = (
                    {
                        xml_local(kid.tag): written_attrs(kid)
                        for kid in edges
                        if any(
                            key.rsplit("}", 1)[-1].rsplit(":", 1)[-1] in ("val", "color")
                            for key in kid.attrib
                        )
                    }
                    if edges is not None
                    else None
                )
                if (
                    one_width is None
                    and span is None
                    and shd is None
                    and align is None
                    and not (edges is not None and len(edges))
                ):
                    continue
                shade_cells += 1 if shd is not None else 0
                border_cells += 1 if border_map else 0
                empty_border_cells += 1 if (edges is not None and not len(edges)) else 0
                align_cells += 1 if align is not None else 0
                cells.append(
                    {
                        "row": row,
                        "col": col,
                        "written": written_attrs(one_width) if one_width is not None else None,
                        "span": span.get(W_NS + "val") if span is not None else None,
                        "shading": written_attrs(shd) if shd is not None else None,
                        "borders": border_map,
                        "borders_present": edges is not None,
                        "valign": align.get(W_NS + "val") if align is not None else None,
                    }
                )
        jc = _kid(props, "jc") if props is not None else None
        ind = _kid(props, "tblInd") if props is not None else None
        lay = _kid(props, "tblLayout") if props is not None else None
        mar = _kid(props, "tblCellMar") if props is not None else None
        entries.append(
            {
                "at": at,
                "written": written_attrs(width) if width is not None else None,
                "w": raw,
                "kind": kind,
                "mm": mm_of(raw, "twips"),
                "align": jc.get(W_NS + "val") if jc is not None else None,
                "indent": written_attrs(ind) if ind is not None else None,
                "layout": lay.get(W_NS + "type") if lay is not None else None,
                "cell_mar": mar is not None,
                "rows": len(rows),
                "grid": grid,
                "cols": len(grid),
                "cells": cells[:limit],
            }
        )
    return {
        "listed": len(entries),
        "with_tblW": said,
        "auto": auto_said,
        "grid_sum": grid_sum,
        "shade_cells": shade_cells,
        "border_cells": border_cells,
        "empty_border_cells": empty_border_cells,
        "align_cells": align_cells,
        "list": entries,
    }


CELL_EDGES = (
    "fo:border", "fo:border-left", "fo:border-right", "fo:border-top", "fo:border-bottom",
    "fo:border-before", "fo:border-after", "fo:border-start", "fo:border-end",
)


def edge_lined(raw: str) -> bool:
    """一条边的值里有没有线：三段式（`0.0pt none #000000`）里那个关键字也算「没线」"""
    return not any(one.lower() in ("none", "hidden") for one in raw.split())


def odf_table_layouts(body, content_root, style_root, prefixes: dict, limit: int = 200) -> dict:
    """ODF 这张表多宽：一条列元素顶几列写在 `number-columns-repeated`，宽度一跳在列样式上

    格子那一本与 docx 那份同形（`shading` / `borders` / `valign`），但值都在
    family=table-cell 的**自动样式**上（LibreOffice 按地址起名 `表格1.A1`）：
    底色 `fo:background-color="#ffff00"`（小写带 `#`，docx 是 `w:fill="FFFF00"`）、
    边 `fo:border-<方位>`（一整串 `<宽度> <样式> <颜色>`；双线 LO 写的 2.25pt 是三根线
    合起来的，另有 `style:border-line-width-top="0.026cm 0.026cm 0.026cm"` 记每根多宽，
    同一个 docx `w:sz="6"` 的两种说法）、垂直对齐 `style:vertical-align`。
    每格都带一份 `fo:padding-*`（默认值也写出来），被合并掉的格子是
    `table:covered-table-cell`（没有内容，另数一本）。
    """
    roots = [(content_root, "content.xml")]
    if style_root is not None:
        roots.append((style_root, "styles.xml"))
    columns: dict[str, tuple] = {}
    table_styles: dict[str, tuple] = {}
    cell_styles: dict[str, tuple] = {}
    for root, part in roots:
        for one in root.iter():
            if xml_local(one.tag) != "style":
                continue
            name = odf_attr(one, "name")
            if name is None:
                continue
            fam = odf_attr(one, "family")
            if fam == "table-column" and name not in columns:
                kid = _kid(one, "table-column-properties")
                columns[name] = (written_kept(kid, prefixes) if kid is not None else None, part)
            if fam == "table" and name not in table_styles:
                kid = _kid(one, "table-properties")
                table_styles[name] = (written_kept(kid, prefixes) if kid is not None else None, part)
            if fam == "table-cell" and name not in cell_styles:
                kid = _kid(one, "table-cell-properties")
                cell_styles[name] = (written_kept(kid, prefixes) if kid is not None else None, part)
    entries = []
    elements = covered = resolved = 0
    cell_elements = covered_cells = cells_unresolved = 0
    shade_cells = align_cells = lined_cells = padded_cells = 0
    for at, tbl in enumerate(
        [one for one in body.iter() if xml_local(one.tag) == "table"][:limit]
    ):
        style = odf_attr(tbl, "style-name")
        held = table_styles.get(style) if style else None
        cols = []
        for kid in tbl:
            if xml_local(kid.tag) != "table-column":
                continue
            elements += 1
            name = odf_attr(kid, "style-name")
            got = columns.get(name) if name else None
            raw_rep = odf_attr(kid, "number-columns-repeated")
            try:
                repeated = int((raw_rep or "1").strip())
            except ValueError:
                repeated = 1
            covered += repeated
            resolved += 1 if got is not None else 0
            width = got[0] if got else None
            cols.append(
                {
                    "written": written_kept(kid, prefixes),
                    "style": name,
                    "style_part": got[1] if got else None,
                    "repeated": repeated,
                    "width": width,
                    "mm": mm_of(width.get("style:column-width") if width else None, None),
                }
            )
        cell_list = []
        my_cells = my_covered = 0
        rows = [one for one in tbl if xml_local(one.tag) == "table-row"]
        for row, tr in enumerate(rows):
            kids = [
                one for one in tr
                if xml_local(one.tag) in ("table-cell", "covered-table-cell")
            ]
            for col, tc in enumerate(kids):
                is_covered = xml_local(tc.tag) == "covered-table-cell"
                my_cells += 1
                my_covered += 1 if is_covered else 0
                covered_cells += 1 if is_covered else 0
                cname = odf_attr(tc, "style-name")
                got = cell_styles.get(cname) if cname else None
                if cname and got is None:
                    cells_unresolved += 1
                props = got[0] if got else None
                smap = props or {}
                shading = smap.get("fo:background-color")
                valign = smap.get("style:vertical-align")
                edges = {
                    key.rsplit(":", 1)[-1]: value
                    for key, value in smap.items()
                    if key in CELL_EDGES
                }
                padded = any(key.startswith("fo:padding") for key in smap)
                lined = any(edge_lined(value) for value in edges.values())
                shade_cells += 1 if shading is not None else 0
                align_cells += 1 if valign is not None else 0
                lined_cells += 1 if lined else 0
                padded_cells += 1 if padded else 0
                cell_list.append(
                    {
                        "row": row,
                        "col": col,
                        "covered": is_covered,
                        "attrs": written_kept(tc, prefixes),
                        "style": cname,
                        "style_part": got[1] if got else None,
                        "written": props,
                        "shading": shading,
                        "valign": valign,
                        "borders": edges,
                        "borders_present": bool(edges),
                        "lined": lined,
                        "padded": padded,
                    }
                )
        cell_elements += my_cells
        entries.append(
            {
                "at": at,
                "cell_elements": my_cells,
                "covered_cells": my_covered,
                "cells": cell_list[:limit],
                "name": odf_attr(tbl, "name"),
                "style": style,
                "style_part": held[1] if held else None,
                "written": held[0] if held else None,
                "mm": mm_of(held[0].get("style:width") if held and held[0] else None, None),
                "columns": cols[:limit],
            }
        )
    return {
        "tables": len(entries),
        "column_elements": elements,
        "covered": covered,
        "resolved": resolved,
        "column_styles": len(columns),
        "table_styles": len(table_styles),
        "cell_styles": len(cell_styles),
        "cell_styles_in_content": sum(1 for one in cell_styles.values() if one[1] == "content.xml"),
        "cell_styles_in_styles": sum(1 for one in cell_styles.values() if one[1] == "styles.xml"),
        "cell_elements": cell_elements,
        "covered_cells": covered_cells,
        "cells_unresolved": cells_unresolved,
        "shade_cells": shade_cells,
        "align_cells": align_cells,
        "lined_cells": lined_cells,
        "padded_cells": padded_cells,
        "list": entries,
    }


def null_cache() -> dict:
    return {"written": None, "points": 0, "values": [], "whole": True}


def no_ref() -> dict:
    """整个引用不在：形状要能跟「写了引用但没有缓存」分开（三个都是 null，不是缺键）"""
    return {"via": None, "ref": None, "text": None, "cache": null_cache()}


def number_or_text(raw: str):
    """Rust 那边 `numeric_or_text` 的同一条式子：能当数看就当数，否则原样交字串"""
    try:
        return float(raw)
    except ValueError:
        return raw


def chart_ref(node) -> dict:
    """一个引用：`c:strRef` / `c:numRef`，或者 openpyxl 那种只有 `c:rich` 的字面量

    引用串**照文件写的交** —— 同一段格子在两个生产者手里是两种写法（`'数据'!B1`
    与 `数据!$B$1`），替它们归一化就是替文件编东西。缓存那份把 `ptCount` 自报的数
    与实际点数一起交，对不上就看得见。
    """
    for kid in node:
        if not xml_local(kid.tag).endswith("Ref"):
            continue
        found = [t.text or "" for t in kid.iter() if xml_local(t.tag) == "f"]
        pts = [t for t in kid.iter() if xml_local(t.tag) == "pt"]
        values = []
        for pt in pts:
            inner = [t for t in pt if xml_local(t.tag) == "v"]
            values.append(number_or_text((inner[0].text or "").strip()) if inner else None)
        stated = [t for t in kid.iter() if xml_local(t.tag) == "ptCount"]
        written = stated[0].get("val") if stated else None
        whole = written is None or (
            written.strip().isdigit() and int(written.strip()) == len(pts)
        )
        return {
            "via": xml_local(kid.tag),
            "ref": (found[0].strip() if found else None),
            "text": None,
            "cache": {"written": written, "points": len(pts), "values": values, "whole": whole},
        }
    joined = "".join(t.text or "" for t in node.iter() if xml_local(t.tag) == "t").strip()
    return {
        "via": "text" if joined else None,
        "ref": None,
        "text": joined or None,
        "cache": null_cache(),
    }


def chart_one(root, part: str) -> dict:
    """一张图：类型那一组、标题的两种写法、每条系列，以及有没有缓存过数值"""
    charts = [t for t in root.iter() if xml_local(t.tag) == "chart"]
    if not charts:
        return {"part": part, "present": False}
    chart = charts[0]
    titled = [t for t in chart if xml_local(t.tag) == "title"]
    if titled:
        inside = [t for t in titled[0] if xml_local(t.tag) == "tx"]
        title = chart_ref(inside[0] if inside else titled[0])
    else:
        title = no_ref()
    areas = [t for t in chart if xml_local(t.tag) == "plotArea"]
    groups = []
    cached = False
    for group in [t for area in areas for t in area if xml_local(t.tag).endswith("Chart")]:
        written: dict = {}
        axis_ids: list = []
        series: list = []
        for one in group:
            which = xml_local(one.tag)
            if which == "ser":
                idx = [t for t in one if xml_local(t.tag) == "idx"]
                order = [t for t in one if xml_local(t.tag) == "order"]
                entry = {}
                for key in ("tx", "cat", "val"):
                    kids = [t for t in one if xml_local(t.tag) == key]
                    entry[key] = chart_ref(kids[0]) if kids else no_ref()
                series.append({
                    "index": idx[0].get("val") if idx else None,
                    "order": order[0].get("val") if order else None,
                    "name": entry["tx"],
                    "cat": entry["cat"],
                    "val": entry["val"],
                })
            elif which == "axId":
                if one.get("val") is not None:
                    axis_ids.append(one.get("val"))
            elif one.get("val") is not None:
                written[which] = one.get("val")
        cached = cached or any(one["val"]["cache"]["points"] > 0 for one in series)
        groups.append({
            "kind": xml_local(group.tag),
            "written": written,
            "axis_ids": axis_ids,
            "series": len(series),
            "series_list": series,
        })
    return {"part": part, "present": True, "title": title, "cached": cached, "groups": groups}


def xlsx_charts(parts: dict) -> dict:
    """每张表上的图：表 →（自己的关系表）→ 画法部件 →（它的关系表）→ 图部件"""
    out: dict = {}
    for name in sorted(parts):
        if not (name.startswith("xl/worksheets/sheet") and name.endswith(".xml")):
            continue
        found: list = []
        for relation in rels_of_parts(parts, name):
            target = relation[1]
            if not (relation[0] == "drawing" and target.endswith(".xml") and "/drawings/" in target):
                continue
            for kind, chart in rels_of_parts(parts, target):
                if kind != "chart" or "/charts/" not in chart or chart not in parts:
                    continue
                found.append(chart_one(ET.fromstring(parts[chart]), chart))
        out[name.rsplit("/", 1)[-1][: -len(".xml")]] = found
    return out


def rels_of_parts(parts: dict, source: str) -> list:
    """那个部件自己的关系表：(Type 结尾那个名字, 解成包内全名的 Target)，按文件里的顺序"""
    dir_name = source.rsplit("/", 1)[0] if "/" in source else ""
    base = source.rsplit("/", 1)[-1]
    name = ("_rels/%s.rels" % base) if not dir_name else ("%s/_rels/%s.rels" % (dir_name, base))
    if name not in parts:
        return []
    root = ET.fromstring(parts[name])
    out = []
    for one in root.iter():
        if xml_local(one.tag) != "Relationship" or one.get("TargetMode") == "External":
            continue
        kind = (one.get("Type") or "").rsplit("/", 1)[-1]
        target = one.get("Target")
        if target is None:
            continue
        out.append((kind, opc_target(source, target)))
    return out


def written_attrs(node) -> dict:
    """一个元素上写着的属性：去掉命名空间前缀，值原样交（与 Rust 的 written_attrs 同一条）"""
    return {xml_local(key): value for key, value in node.attrib.items()}


def dxf_table(parts: dict):
    """styles.xml 里 dxfs 那一跳：(自报的 count, 每条 dxf 里出现过的元素路径)

    路径一层到底写成 `font/b` 这样：openpyxl 的一条 dxf 只写 font/b 与 font/color，
    LibreOffice 重写同一条会补上 name / family / sz —— 只交出现过的名字，不替两边凑形状。
    """
    if "xl/styles.xml" not in parts:
        return None, []
    root = ET.fromstring(parts["xl/styles.xml"])
    holder = [one for one in root.iter() if xml_local(one.tag) == "dxfs"]
    if not holder:
        return None, []
    written = holder[0].get("count")
    kinds = []
    for one in holder[0]:
        if xml_local(one.tag) != "dxf":
            continue
        paths = []
        for kid in one:
            kids = list(kid)
            if not kids:
                paths.append(xml_local(kid.tag))
            else:
                paths += ["%s/%s" % (xml_local(kid.tag), xml_local(deep.tag)) for deep in kids]
        kinds.append(paths)
    return written, kinds


def sheet_rules(root, dxfs: list, limit: int = 200) -> dict:
    """这一张表上的规则：条件格式那块（一块一套范围，里面若干条 cfRule）与数据验证"""
    blocks = []
    for block in [one for one in root.iter() if xml_local(one.tag) == "conditionalFormatting"]:
        rules = []
        for rule in [one for one in block if xml_local(one.tag) == "cfRule"][:limit]:
            index = rule.get("dxfId")
            at = None
            try:
                at = int(index.strip()) if index is not None else None
            except ValueError:
                at = None
            if index is None:
                dxf = {"written": None, "found": False, "kinds": []}
            elif at is not None and at < len(dxfs):
                dxf = {"written": index, "found": True, "kinds": dxfs[at]}
            else:
                dxf = {"written": index, "found": False, "kinds": []}
            found = [one for one in rule
                     if xml_local(one.tag) in ("iconSet", "colorScale", "dataBar", "extLst")]
            detail = found[0] if found else None
            scale = None
            if detail is not None and xml_local(detail.tag) != "extLst":
                scale = {
                    "kind": xml_local(detail.tag),
                    "written": written_attrs(detail),
                    "cfvo": [written_attrs(one) for one in detail if xml_local(one.tag) == "cfvo"],
                    "colors": [one.get("rgb") for one in detail
                               if xml_local(one.tag) == "color" and one.get("rgb") is not None],
                }
            rules.append({
                "type": rule.get("type"),
                "priority": rule.get("priority"),
                "operator": rule.get("operator"),
                "dxf": dxf,
                "formulas": [(one.text or "").strip() for one in rule
                             if xml_local(one.tag) == "formula"],
                "scale": scale,
                "written": written_attrs(rule),
            })
        blocks.append({"sqref": block.get("sqref"), "rules": len(rules), "rule_list": rules})
    holders = [one for one in root.iter() if xml_local(one.tag) == "dataValidations"]
    items = []
    if holders:
        for one in [t for t in holders[0] if xml_local(t.tag) == "dataValidation"][:limit]:
            items.append({
                "sqref": one.get("sqref"),
                "type": one.get("type"),
                "operator": one.get("operator"),
                "formulas": [(t.text or "").strip() for t in one
                             if xml_local(t.tag) in ("formula1", "formula2")],
                "written": written_attrs(one),
            })
    written = holders[0].get("count") if holders else None
    whole = written is None or (written.strip().isdigit() and int(written.strip()) == len(items))
    return {
        "conditional": blocks,
        "validations": {"written": written, "found": len(items), "whole": whole, "list": items},
    }


def xlsx_rules(parts: dict, dxfs: list) -> dict:
    """每张表上的规则那份账（条件格式 + 数据验证），按 sheetN 归位"""
    out: dict = {}
    for name in sorted(parts):
        if not (name.startswith("xl/worksheets/sheet") and name.endswith(".xml")):
            continue
        out[name.rsplit("/", 1)[-1][: -len(".xml")]] = sheet_rules(
            ET.fromstring(parts[name]), dxfs
        )
    return out


def xlsx_facts(path: Path) -> dict:
    parts = {}
    with zipfile.ZipFile(path) as box:
        names = [one.filename for one in box.infolist()]
        parts = {one.filename: box.read(one.filename) for one in box.infolist()}
    print_setups = xlsx_print_setup(parts)
    views = xlsx_views(parts)
    headers = xlsx_headers(parts)
    chart_lists = xlsx_charts(parts)
    dxf_written, dxf_kinds = dxf_table(parts)
    rules_by_sheet = xlsx_rules(parts, dxf_kinds)
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
    strings: list = []
    sst_count = None
    sst_unique = None
    if "xl/sharedStrings.xml" in parts:
        sst = ET.fromstring(parts["xl/sharedStrings.xml"])
        sst_count = sst.get("count")
        sst_unique = sst.get("uniqueCount")
        for one in sst.iter():
            if xml_local(one.tag) == "si":
                strings.append(ooxml_string_parts(one))
    shared = len(strings)
    by_index = dict(enumerate(strings))
    # 「长相」那一跳：cellXfs + fonts / fills / borders 三张表
    tables = xlsx_style_tables(parts)
    cell_styles: list = []
    bold_cells = 0
    fill_cells = 0
    wrap_cells = 0
    bare_style = 0
    cells = 0
    formulas = 0
    numbers = 0
    typed = 0
    errored = 0
    str_results = 0
    strings_inline = 0
    rich_cells = 0
    preserved_cells = 0
    cell_strings: list = []
    merged = 0
    dims: dict[str, str] = {}
    layouts: dict = {}
    filters: dict = {}
    sheet_tables: dict = {}
    for name in sorted(parts):
        if not (name.startswith("xl/worksheets/sheet") and name.endswith(".xml")):
            continue
        root = ET.fromstring(parts[name])
        local = name.rsplit("/", 1)[-1][: -len(".xml")]
        layouts[local] = xlsx_layout_of(root)
        filters[local] = xlsx_filter_of(root)
        sheet_tables[local] = xlsx_tables_of(parts, name)
        for one in root.iter():
            tag = xml_local(one.tag)
            if tag == "dimension":
                dims[local] = one.get("ref", "")
            elif tag == "c":
                cells += 1
                t = one.get("t")
                # 每一格自己一次：这一格的字是不是一个「串」（s 或 inlineStr）
                parts_str = None
                # 「文件写了 t」与「t 没写、按规范默认 n」是两件事：openpyxl 给公式格一个
                # 都不写，LibreOffice 重写同一份时连数字格都写 t="n"
                if t is not None:
                    typed += 1
                if t == "e":
                    errored += 1
                elif t == "str":
                    str_results += 1
                elif t == "s":
                    # 索引在这里，字在 sharedStrings 里：绕那一跳把分段也取出来
                    spot = local_child(one, "v")
                    try:
                        parts_str = by_index.get(int((spot.text or "").strip()))
                    except (AttributeError, TypeError, ValueError):
                        parts_str = None
                elif t == "inlineStr":
                    strings_inline += 1
                    holder = local_child(one, "is")
                    parts_str = ooxml_string_parts(holder) if holder is not None else None
                elif t in (None, "n"):
                    if has_local_child(one, "v"):
                        numbers += 1
                if parts_str is not None:
                    cell_strings.append(
                        dict(
                            [("sheet", local), ("ref", one.get("r"))]
                            + [(key, value) for key, value in parts_str.items()]
                        )
                    )
                    if parts_str["rich"]:
                        rich_cells += 1
                    if parts_str["preserved"]:
                        preserved_cells += 1
                raw_style = one.get("s")
                try:
                    which = int((raw_style or "0").strip())
                except ValueError:
                    which = 0
                looks = style_appearance(tables, which)
                cell_styles.append(
                    dict(
                        [("sheet", local), ("ref", one.get("r")),
                         ("style", which), ("style_written", raw_style is not None)]
                        + list(looks.items())
                    )
                )
                if looks["style_bold"] is True:
                    bold_cells += 1
                if looks["style_filled"] is True:
                    fill_cells += 1
                if looks["style_wrapped"] is True:
                    wrap_cells += 1
                if raw_style is None:
                    bare_style += 1
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
        "error_cells": errored,
        "string_result_cells": str_results,
        "cells_with_written_type": typed,
        "cells_with_runs": rich_cells,
        "cells_with_preserved_space": preserved_cells,
        "cell_strings": cell_strings,
        "cells_bold": bold_cells,
        "cells_filled": fill_cells,
        "cells_wrapped": wrap_cells,
        "cells_without_style_written": bare_style,
        "cell_styles": cell_styles,
        "style_tables": tables["ledger"],
        "strings": {
            "count_written": sst_count,
            "unique_written": sst_unique,
            "entries": shared,
            "with_runs": sum(1 for one in strings if one["rich"]),
            "with_preserved_space": sum(1 for one in strings if one["preserved"]),
            # 与 Rust 那边同一个算法：没有这张表、或者这个数解不出来，都算「对不上」
            "unique_matches": _unique_matches(sst_unique, shared),
        },
        "merged": merged,
        "dimensions": dims,
        "print_setup": print_setups,
        "views": views,
        "headers": headers,
        "layouts": layouts,
        "filters": filters,
        "sheet_tables": sheet_tables,
        "charts": chart_lists,
        "dxfs": {"written": dxf_written, "found": len(dxf_kinds),
                 "whole": dxf_written is None or int(dxf_written) == len(dxf_kinds)},
        "rules": rules_by_sheet,
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


def _unique_matches(raw, count: int) -> bool:
    """`sst/@uniqueCount` 与条数对不对得上：解不出来（含整个没这张表）算 False"""
    try:
        return int((raw or "").strip()) == count
    except (TypeError, ValueError):
        return False


def ooxml_string_parts(holder, limit: int = 200) -> dict:
    """一条「串」（`si` 或 `is`）：整串的字 + 文件把它分成的几段

    与 Rust 的 `string_parts` 同一条规则：`t` 直接坐在串下是一段没有格式的字，
    `r` 是带 `rPr` 的一段（`rPr` 在不在、它自己写着的属性、里面那几个孩子元素
    分三处交 —— 两家生产者把格式写在孩子上（`<b val="true"/>`），rPr 自己一个属性都不写）。
    整串**不 strip**：首尾那两个空格是文件写的，LibreOffice 自己导出的 CSV 也带着它们。
    """
    runs: list = []
    rich = False
    preserved = False
    for kid in holder:
        tag = xml_local(kid.tag)
        props = None
        if tag == "t":
            word = kid
        elif tag == "r":
            rich = True
            # 不能用 `or`：ElementTree 的 Element 没有孩子时是**假值**，
            # `<t>重要</t>` 正是这种（0 个孩子、有字），`or` 会把它当成没有而退回 `r`
            word = local_child(kid, "t")
            if word is None:
                word = kid
            props = local_child(kid, "rPr")
        else:
            continue
        space = word.get("{http://www.w3.org/XML/1998/namespace}space")
        if space is not None:
            preserved = True
        if len(runs) >= limit:
            continue
        runs.append(
            {
                "element": tag,
                "text": word.text or "",
                "space": space,
                "props_written": props is not None,
                "props_attrs": (
                    dict(sorted((xml_local(key), value) for key, value in props.attrib.items()))
                    if props is not None
                    else None
                ),
                "format": (
                    [
                        {
                            "element": xml_local(one.tag),
                            "attrs": dict(
                                sorted(
                                    (xml_local(key), value)
                                    for key, value in one.attrib.items()
                                )
                            ),
                        }
                        for one in props
                    ]
                    if props is not None
                    else None
                ),
            }
        )
    text = "".join(
        (one.text or "") for one in holder.iter() if xml_local(one.tag) == "t"
    )
    return {
        "text": text,
        "runs": runs,
        "run_total": sum(
            1
            for one in holder
            if xml_local(one.tag) in ("t", "r")
        ),
        "rich": rich,
        "preserved": preserved,
    }


def ooxml_attr(node, want: str):
    """按局部名取属性（`r:id` 与 `id` 都算 `id`，与 Rust 的 `attr_local` 同一条）"""
    for key, value in node.attrib.items():
        _, _, tail = key.rpartition("}")
        if (tail or key).rsplit(":", 1)[-1] == want:
            return value
    return None


def emu_written(raw):
    """画幅那个整数字符串：只认一串纯数字（Rust 那边 parse::<u64> 就是这个口径，
    带符号、带空格、带小数点都不算数），否则照原样把串交出去"""
    if raw is None:
        return None
    if raw and all(one in "0123456789" for one in raw):
        return int(raw)
    return raw


def emu_summand(raw):
    """加进「网格之和」的那个数：这里允许首尾空白（与 Rust 的 `trim().parse::<i64>()` 同一口径），
    数不出来就不加"""
    if raw is None:
        return None
    try:
        return int(raw.strip())
    except ValueError:
        return None


def span_written(node, want: str) -> int:
    """「这一元素顶几个」：没写、写坏、写成 0 都按 1（与 .ods 那族同一口径）"""
    got = emu_summand(ooxml_attr(node, want))
    return got if got is not None and got > 0 else 1


def ooxml_para_text(node) -> str:
    """一个段的字：按文档顺序拼 `.text` 与每个孩子的 `.tail`（Rust 那边把每段文字都存成
    `#text` 孩子，所以「前 span 中 /span 后」两边都读成「前中后」），
    而 `a:tab` 给一个制表、`a:br` / `a:cr` 给一个换行 —— 那两个是空元素，
    只拼 itertext 就把它们读没了。带换行的纯空白整段丢掉（pretty-print 的缩进噪声），
    不带换行的空白留着（`<w:t xml:space="preserve"> </w:t>` 真是一个空格）"""
    out: list = []

    def keep(chunk: str) -> bool:
        if not chunk:
            return False
        return not (chunk.strip() == "" and ("\n" in chunk or "\r" in chunk))

    def walk(one):
        name = xml_local(one.tag)
        if name == "tab":
            out.append("\t")
            return
        if name in ("br", "cr"):
            out.append("\n")
            return
        if one.text and keep(one.text):
            out.append(one.text)
        for kid in one:
            walk(kid)
            if kid.tail and keep(kid.tail):
                out.append(kid.tail)

    walk(node)
    return "".join(out).strip()


def slide_tables_of(root, limit: int = 100) -> list:
    """这一页上那张表（pptx）：与 src/office_slide.rs 的 `slide_tables` 同一份账。

    合并在这一族是「被合掉的那一格照样在场」（`hMerge` / `vMerge`），起点那格写
    `gridSpan` / `rowSpan` —— 所以几个格、跨度之和、网格几列是三个数，这里三个都交。
    `limit` 是 office-slide 的 LIMIT_DEFAULT（100），不是 office-sheet 的那个 200。
    """
    out: list = []
    tables = [one for one in root.iter() if xml_local(one.tag) == "tbl"]
    for index, tbl in enumerate(tables):
        holder = _kid(tbl, "tblPr")
        style = _kid(holder, "tableStyleId") if holder is not None else None
        grid_holder = _kid(tbl, "tblGrid")
        cols = _kids(grid_holder, "gridCol") if grid_holder is not None else []
        grid: list = []
        grid_sum = 0
        for one in cols:
            raw = ooxml_attr(one, "w")
            part = emu_summand(raw)
            if part is not None:
                grid_sum += part
            if len(grid) < limit:
                grid.append(
                    {
                        "written": written_attrs(one),
                        "w": emu_written(raw),
                        "mm": mm_of(raw, "emu"),
                    }
                )
        rows: list = []
        row_elements = cell_elements = span_total = 0
        with_text = merged_from = spanning = 0
        for row in _kids(tbl, "tr"):
            row_elements += 1
            cells: list = []
            row_cells = row_spans = row_text = 0
            for at, tc in enumerate(_kids(row, "tc")):
                row_cells += 1
                span_cols = span_written(tc, "gridSpan")
                span_rows = span_written(tc, "rowSpan")
                row_spans += span_cols
                if span_cols > 1 or span_rows > 1:
                    spanning += 1
                from_merge = ooxml_attr(tc, "hMerge") is not None or ooxml_attr(
                    tc, "vMerge"
                ) is not None
                if from_merge:
                    merged_from += 1
                props = _kid(tc, "tcPr")
                body = _kid(tc, "txBody")
                paras = _kids(body, "p") if body is not None else []
                text = "\n".join(ooxml_para_text(one) for one in paras)
                if text:
                    with_text += 1
                    row_text += 1
                runs = (
                    len([one for one in body.iter() if xml_local(one.tag) == "r"])
                    if body is not None
                    else 0
                )
                paths = [xml_local(one.tag) for one in props] if props is not None else []
                inner = None
                if body is not None:
                    got = _kid(body, "bodyPr")
                    if got is not None:
                        inner = written_attrs(got)
                if len(cells) < limit:
                    cells.append(
                        {
                            "at": at,
                            "written": written_attrs(tc),
                            "span_cols": span_cols,
                            "span_rows": span_rows,
                            "merge_from": from_merge,
                            "tcpr_present": props is not None,
                            "tcpr": written_attrs(props) if props is not None else None,
                            "tcpr_paths": paths,
                            "body": inner,
                            "text": text,
                            "paragraphs": len(paras),
                            "runs": runs,
                        }
                    )
            cell_elements += row_cells
            span_total += row_spans
            if len(rows) < limit:
                rows.append(
                    {
                        "written": written_attrs(row),
                        "h": emu_written(ooxml_attr(row, "h")),
                        "mm": mm_of(ooxml_attr(row, "h"), "emu"),
                        "cells": row_cells,
                        "span_sum": row_spans,
                        "with_text": row_text,
                        "list": cells,
                    }
                )
        out.append(
            {
                "at": index,
                "pr_present": holder is not None,
                "written": written_attrs(holder) if holder is not None else None,
                "style_present": style is not None,
                "style_id": "".join(style.itertext()).strip() if style is not None else None,
                "grid": grid,
                "column_elements": len(cols),
                "grid_sum": grid_sum,
                "grid_sum_mm": mm_of(str(grid_sum), "emu"),
                "rows": rows,
                "row_elements": row_elements,
                "cell_elements": cell_elements,
                "span_sum": span_total,
                "with_text": with_text,
                "merged_from": merged_from,
                "spanning": spanning,
            }
        )
    return out


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
        # 这一页的图：页的关系表里 kind 是 chart 的那几条（LibreOffice 另往 ppt/charts/
        # 里塞 style 与 colors 部件，所以只认关系，不按目录数）
        page_charts = []
        for kind, target in rels_of_parts(parts, name):
            if kind == "chart" and target in parts:
                page_charts.append(chart_one(ET.fromstring(parts[target]), target))
        # 页上的链接：两跳 —— run 里只有号，地址在这一页的关系表里
        rel_part = (
            f"{stem[: stem.rindex('/')]}/_rels/{stem[stem.rindex('/') + 1 :]}.xml.rels"
        )
        rels_root = ET.fromstring(parts[rel_part]) if rel_part in parts else None
        out_slides.append(
            {
                "part": name,
                "links": pptx_slide_links(root, rels_root),
                "relationships": slide_rels(rels_root, name),
                # 「放映时隐藏」这一族就写在根元素上一个 show="0"；没写等于没藏
                "hidden": root.get("show") == "0",
                "title": texts[0] if texts else "",
                "texts": texts,
                "text_runs": len(texts),
                "shapes": len(shapes),
                "pictures": len(pics),
                "picture_rows": pptx_picture_rows(root, parts, name),
                "graphic_frames": len(tables),
                "placeholders": placeholders,
                "notes": notes.strip(),
                "charts": len(page_charts),
                "chart_list": page_charts,
                # 这一页上那张表的网（与 src/office_slide.rs 的 slide_tables 同一份账）
                "table_list": slide_tables_of(root),
            }
        )
    pres = ET.fromstring(parts["ppt/presentation.xml"])
    size = ""
    size_type = None
    for one in pres.iter():
        if xml_local(one.tag) == "sldSz":
            size = f'{one.get("cx")}x{one.get("cy")}:{one.get("type", "")}'
            # type 这个属性 python-pptx 写了、LibreOffice 重写同一份稿子时整个省掉：
            # 没写就是没写，不替它填 "custom"
            size_type = one.get("type")
    sld_master_ids = [
        one.get("{http://schemas.openxmlformats.org/officeDocument/2006/relationships}id")
        for one in pres.iter()
        if xml_local(one.tag) == "sldMasterId"
    ]
    return {
        "slide_count": len(slides),
        "slides": out_slides,
        "slide_size": size,
        "slide_size_type": size_type,
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


def odf_page_breaks(root, body) -> int:
    r"""ODF 的换页有几处：段落自己并不带换页元素，换页写在**它的样式**上

    `text:p` 上只有 `text:style-name="P2"`，而 `fo:break-before="page"` 坐在同一个部件里
    那个 `style:style` 的 `style:paragraph-properties` 上 —— 与 .ods 的数据样式同一类两跳。
    只看段落点名的那一个样式：父样式链上也可能写，但四份真件都写在自己身上，
    没有样本就不去猜那条链（`style:parent-style-name` 因此不跟）。
    """
    named = set()
    for one in root.iter():
        if xml_local(one.tag) != "style" or of_local(one, "family") != "paragraph":
            continue
        for kid in one:
            if xml_local(kid.tag) == "paragraph-properties" and of_local(kid, "break-before") == "page":
                which = of_local(one, "name")
                if which:
                    named.add(which)
                break
    hits = 0
    for one in body.iter():
        if xml_local(one.tag) not in ("p", "h"):
            continue
        if of_local(one, "style-name") in named:
            hits += 1
    return hits


def style_counts(paras: list) -> dict:
    """样式名 → 用了它几段（口径与 `office-doc` 的 styles 字段一致）"""
    out: dict[str, int] = {}
    for one in paras:
        name = of_local(one, "style-name")
        if name:
            out[name] = out.get(name, 0) + 1
    return dict(sorted(out.items()))


def odf_contents(root) -> dict:
    """这份 ODF 有没有目录、收了几级：目录是 `text:table-of-content` 那一块。

    与 OOXML 的分别是真的：这里「几级」写在 `text:table-of-content-source` 的
    `outline-level` 属性上（LibreOffice 那份写 2），目录名与「是否受保护」也都在元素上；
    OOXML 那边这些全在域指令的文字里。所以两边各报各的，不强行统一成一个字段。
    """
    blocks = [one for one in root.iter() if xml_local(one.tag) == "table-of-content"]
    if not blocks:
        return {"present": False, "names": [], "outline_level": None, "entry_templates": 0, "title": None}
    names = [local_attr(one, "name") for one in blocks]
    level = None
    title = None
    for one in root.iter():
        if xml_local(one.tag) == "table-of-content-source" and level is None:
            level = local_attr(one, "outline-level")
        if xml_local(one.tag) == "index-title-template" and title is None:
            got = "".join(one.itertext()).strip()
            title = got or None
    return {
        "present": True,
        "names": [one for one in names if one],
        "outline_level": level,
        "entry_templates": len(
            [one for one in root.iter() if xml_local(one.tag) == "table-of-content-entry-template"]
        ),
        "title": title,
    }


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
    prefixes = ns_prefixes(parts["content.xml"].decode("utf8", "replace"))

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
        # 换页写在段落样式上（`fo:break-before="page"`），不是正文里的元素：见 odf_page_breaks
        "page_breaks": odf_page_breaks(root, body),
        # 段落格式与分栏：这一族都住在样式那一跳上（不是正文元素），见上面两个函数
        "paragraph_formats": odf_paragraph_formats(odf_body_paragraphs(body), root, prefixes),
        # 字符格式：span 点名的那一份样式可能在两个部件之一，而夹在 span 中间的字
        # 文件根本没给它立一个元素（`#text` 那一行）
        "run_formats": odf_run_formats(
            odf_body_paragraphs(body),
            root,
            prefixes,
            ET.fromstring(parts["styles.xml"]) if "styles.xml" in parts else None,
            {
                **ns_prefixes(parts["styles.xml"].decode("utf8", "replace")),
                **prefixes,
            }
            if "styles.xml" in parts
            else prefixes,
        ),
        "columns": odf_columns(root, prefixes),
        # 那份定义 LibreOffice 全写在 styles.xml，段样式在 content.xml：跨部件的一跳
        # （前缀要按**各自那份件**自己声明的 xmlns 还原，所以两张表并起来用，content 优先）
        "numbering": odf_numbering(
            odf_body_paragraphs(body),
            body,
            root,
            ET.fromstring(parts["styles.xml"]) if "styles.xml" in parts else None,
            {
                **ns_prefixes(parts["styles.xml"].decode("utf8", "replace")),
                **prefixes,
            }
            if "styles.xml" in parts
            else prefixes,
        ),
        # 列宽一跳在 family=table-column 的样式上，那份样式 LibreOffice 写在 content.xml
        "table_layouts": odf_table_layouts(
            body,
            root,
            ET.fromstring(parts["styles.xml"]) if "styles.xml" in parts else None,
            {
                **ns_prefixes(parts["styles.xml"].decode("utf8", "replace")),
                **prefixes,
            }
            if "styles.xml" in parts
            else prefixes,
        ),
        # text:soft-page-break 是另一件事：渲染时落下的那个位置，不是作者要的换页
        "soft_page_breaks": count_local(body, "soft-page-break"),
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
        "contents": odf_contents(root),
        "paragraph_texts": [texts(one) for one in paras],
        "annotation_texts": annotation_entries(body),
        "statistic": statistic,
    }


def docx_revision_ledger(path: Path) -> dict:
    """修订这份账的第二读者（Rust 那边是 `lilyco-binfmt/src/revise.rs`）"""
    with zipfile.ZipFile(path) as box:
        names = set(box.namelist())
        document = ET.fromstring(box.read("word/document.xml"))
        settings = (
            ET.fromstring(box.read("word/settings.xml"))
            if "word/settings.xml" in names
            else None
        )
    return docx_revisions(document, settings)


def odt_revision_ledger(path: Path) -> dict | None:
    """ODF 那一侧同一条规则的另一份实现；不是文字文档就交回 None"""
    with zipfile.ZipFile(path) as box:
        if "content.xml" not in box.namelist():
            return None
        return odt_revisions(ET.fromstring(box.read("content.xml")))


def protection_for(path: Path) -> dict | None:
    """保护这份账的第二读者（Rust 那边是 `lilyco-binfmt/src/protect.rs`）

    认四家：docx / xlsx / odt / ods。其余（pptx、rtf…）交回 None，不硬套一个形状。
    """
    with zipfile.ZipFile(path) as box:
        names = set(box.namelist())

        def read(part: str):
            return ET.fromstring(box.read(part)) if part in names else None

        if "word/document.xml" in names:
            return docx_protection(read("word/settings.xml"))
        if "xl/workbook.xml" in names:
            workbook = read("xl/workbook.xml")
            rels = {}
            raw = read("xl/_rels/workbook.xml.rels")
            if raw is not None:
                for one in raw.iter():
                    if xml_local(one.tag) == "Relationship":
                        rels[one.get("Id")] = one.get("Target") or ""
            sheets = []
            for one in workbook.iter():
                if xml_local(one.tag) != "sheet":
                    continue
                rid = next(
                    (value for key, value in one.attrib.items() if xml_local(key) == "id"), ""
                )
                target = rels.get(rid, "")
                part = target.lstrip("/") if target.startswith("/") else "xl/" + target
                if part in names:
                    sheets.append((one.get("name"), ET.fromstring(box.read(part))))
            return xlsx_protection(workbook, sheets)
        if "content.xml" in names:
            content = read("content.xml")
            if any(xml_local(one.tag) == "spreadsheet" for one in content.iter()):
                return ods_protection(content)
            return odt_protection(read("settings.xml"))
        return None


def body_paragraphs(root) -> list:
    """正文段：批注（`text:annotation`）与修订表（`text:tracked-changes`）里的那些不算。

    ODF 的批注是嵌在正文段**里面**的，不是像 docx 那样另有一个 comments.xml 部件，
    所以「这一段有几段字」这件事得先把批注子树挖掉再数。
    修订表里那份 `text:p` 装的是**被删掉**的字，正文那个位置只剩一个 `text:change`
    标记 —— 不挖掉就等于把删掉的段落读回正文。
    ElementTree 没有父指针，就反过来做：先把这些子树里的段挑出来，按 id 排除。
    """
    inside = set()
    for owner in root.iter():
        if xml_local(owner.tag) not in ("annotation", "tracked-changes"):
            continue
        for one in owner.iter():
            if xml_local(one.tag) == "p":
                inside.add(id(one))
    return [
        one
        for one in root.iter()
        if xml_local(one.tag) == "p" and id(one) not in inside
    ]


def odf_body_paragraphs(root) -> list:
    """`office-doc` 的 ODF 口径：走到 `text:p` 就停，**不钻进它里面**

    为什么与 `body_paragraphs` 不是一份清单：ODF 的注坐在正文段**里面**
    （`text:p > text:note > text:note-body > text:p`），全树数会把注里那几段
    也算成正文段。`notes-end.odt` 实测两个数：不落进段里 4、全树数 7，
    而 LibreOffice 自己写在 meta.xml 的 `paragraph-count` 是那个 7 ——
    「这份文档有几段」没有唯一答案，office-doc 这一族取 4（与它自己那份
    `structure.paragraphs` 同一份清单，段格式与编号那两份账的 index 才对得上），
    生产者那个 7 在 `statistics.producer` 里并排交。
    """
    out: list = []

    def walk(node) -> None:
        for kid in node:
            tag = xml_local(kid.tag)
            if tag in ("annotation", "tracked-changes"):
                continue
            if tag == "p":
                out.append(kid)
                continue
            walk(kid)

    walk(root)
    return out


def body_paragraph_and_heading_nodes(root) -> list:
    """`office-text` 的 ODF 口径：正文段**和标题**按文档顺序一起走。

    这两份账本来就不是一个问句：office-doc 问「有几段」（只认 `text:p`，
    标题另有一份带层级的清单），office-text 问「页面上能读到哪几块字」，
    `text:h` 也是字。共用一份清单会让其中一边说谎，所以分开列。
    批注子树同样挖掉，理由与 `body_paragraphs` 一致；修订表（`text:tracked-changes`）
    里那份 `text:p` 装的是**被删掉**的字，页面上读不到，也得挖掉。
    """
    inside = set()
    for owner in root.iter():
        if xml_local(owner.tag) not in ("annotation", "tracked-changes"):
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
        # 正文里那几张图逐张的账：摆法在属性上、尺寸自带单位、替代文字是孩子元素
        "picture_rows": odt_picture_rows(
            odf_text_root(root), ns_prefixes(parts["content.xml"].decode("utf8"))
        ),
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


def _para_text(node) -> str:
    """一段里的字，按 ODF 的写法展开：`text:s` / `text:tab` / `text:line-break` 是**记号**
    而不是字面（`text:c` 说那一个记号顶几个空格）。首尾不 strip —— 那两个空格是文件写的，
    LibreOffice 把同一份 .ods 自己导成 CSV 时一个都不少。
    """
    out: list = []
    if node.text:
        # 段自己的第一个字在 `.text` 里（ElementTree 不把它做成孩子）——
        # 漏了它，`<text:p>甲</text:p>` 就成一个字也没有
        out.append(node.text)

    def walk(one) -> None:
        if xml_local(one.tag) == "annotation":
            return
        tag = xml_local(one.tag)
        if tag == "s":
            try:
                times = int((of_attr(one, "c") or "1").strip())
            except ValueError:
                times = 1
            out.append(" " * min(max(times, 0), 4096))
            return
        if tag == "tab":
            out.append("\t")
            return
        if tag == "line-break":
            out.append("\n")
            return
        if one.text:
            out.append(one.text)
        for kid in one:
            walk(kid)
            if kid.tail:
                out.append(kid.tail)

    for kid in node:
        walk(kid)
        if kid.tail:
            out.append(kid.tail)
    return "".join(out)


def _cell_marks(node) -> tuple:
    """这一格里 `text:span` 的条数与那三种记号（s / tab / line-break）的条数"""
    spans = 0
    specials = 0
    for one in node.iter():
        tag = xml_local(one.tag)
        if tag == "span":
            spans += 1
        elif tag in ("s", "tab", "line-break"):
            specials += 1
    return spans, specials


def _cell_paragraphs(node) -> list:
    """这一格「算内容」的段：批注（`office:annotation`）整个子树跳过。

    要递归而不只看直子：`text:list` 里的段也算这一格的字。
    LibreOffice 把批注写成格子的孩子元素，一锅端就会把注的文字当成格子的内容 ——
    与 .odt 那边「批注与修订表里的段不算正文」是同一条规矩。
    """
    out: list = []
    for kid in node:
        tag = xml_local(kid.tag)
        if tag == "annotation":
            continue
        if tag == "p":
            out.append(_para_text(kid))
            continue
        out.extend(_cell_paragraphs(kid))
    return out


def _first_text(node, wants: tuple) -> str | None:
    """孩子元素里第一条有字的（空的 `<meta:date-string/>` 算没写，不算写了空时间）"""
    for want in wants:
        for one in node.iter():
            if xml_local(one.tag) == want:
                got = "".join(one.itertext()).strip()
                if got:
                    return got
                break
    return None


XLINK = "{http://www.w3.org/1999/xlink}href"


def of_attr(node, want: str):
    """按局部名取一个属性（ODF 的属性都带前缀，且同一局部名可能来自两个命名空间）"""
    for key, value in node.attrib.items():
        if key.rsplit("}", 1)[-1] == want and "documentfoundation" not in key:
            return value
    return None


def odf_written_attrs(node) -> dict:
    """一个元素上写着的属性（局部名 → 原值）。LibreOffice 抄的那份 `calcext:` 副本不看：
    它的局部名与 `office:value-type` 一模一样，看了就会两家各说一句话。"""
    return {
        key.rsplit("}", 1)[-1]: value
        for key, value in node.attrib.items()
        if "openoffice.org/2020/calc" not in key and "documentfoundation" not in key
    }


def odf_chart_object(parts: dict, href: str) -> dict:
    """一个嵌入的图对象：`Object N/` 那一份 content.xml 里的 chart:chart

    ODF 的地址是**第三种写法**（`数据.B2:数据.B3`：点分隔、不带 `$`、不引号），
    与 OOXML 两家的 `'数据'!$B$2:$B$3` / `数据!$B$2:$B$3` 都不是一种东西 —— 照写交。
    `chart:data-point@chart:repeated` 是自报的「这一条顶几个点」，与 `chart:series`
    指的那段区间的宽度对不对，交给读的人自己看（两条都交）。
    """
    folder = href.lstrip("./").rstrip("/")
    name = folder + "/content.xml"
    if name not in parts:
        return {"object": folder, "present": False}
    root = ET.fromstring(parts[name])
    charts = [one for one in root.iter() if xml_local(one.tag) == "chart"]
    klass = None
    if charts:
        klass = of_attr(charts[0], "class")
    series = []
    categories = None
    title = None
    for one in root.iter():
        which = xml_local(one.tag)
        if which == "title":
            text = " ".join((t or "").strip() for t in one.itertext()).strip()
            title = text or None
        elif which == "series":
            points = [t for t in one if xml_local(t.tag) == "data-point"]
            stated = 0
            for one_point in points:
                try:
                    stated += max(1, int(of_attr(one_point, "repeated") or 1))
                except ValueError:
                    stated += 1
            series.append({
                "class": of_attr(one, "class"),
                "values": of_attr(one, "values-cell-range-address"),
                "label": of_attr(one, "label-cell-address"),
                "point_elements": len(points),
                "points_written": stated if points else None,
                "written": odf_written_attrs(one),
            })
        elif which == "categories":
            points = [t for t in one if xml_local(t.tag) == "data-point"]
            categories = {
                "address": of_attr(one, "cell-range-address"),
                "point_elements": len(points),
                "written": odf_written_attrs(one),
            }
    cached = []
    for table in [one for one in root.iter() if xml_local(one.tag) == "table"]:
        if of_attr(table, "name") != "local-table":
            continue
        for row in [one for one in table.iter() if xml_local(one.tag) == "table-row"]:
            line = []
            for cell in [one for one in row if xml_local(one.tag) in ("table-cell", "header-cell")]:
                span = 1
                try:
                    span = max(1, int(of_attr(cell, "number-columns-repeated") or 1))
                except ValueError:
                    span = 1
                # 只取 text:p 的字：格子里还嵌着 `table:desc` 那种写地址的东西，
                # 一锅端 itertext 会把「收入 数据.B1:数据.B1」当成一个字
                texts = [
                    " ".join((t or "").strip() for t in para.itertext()).strip()
                    for para in cell.iter()
                    if xml_local(para.tag) == "p"
                ]
                value = of_attr(cell, "value")
                line.append({
                    "repeated": span,
                    "text": " ".join(one for one in texts if one) or None,
                    "value": value,
                    "written": odf_written_attrs(cell),
                })
            cached.append({"cells": line})
    return {
        "object": folder,
        "present": True,
        "class": klass,
        "title": title,
        "series": len(series),
        "series_list": series,
        "categories": categories,
        "local_table": cached,
    }


def odf_charts_of(parts: dict, host) -> list:
    """这个宿主（一张 ODS 表或一页 ODP）里的图：`draw:frame` → `draw:object@xlink:href`"""
    found = []
    for frame in [one for one in host.iter() if xml_local(one.tag) == "frame"]:
        for one in frame:
            if xml_local(one.tag) != "object":
                continue
            href = one.get(XLINK)
            if not href:
                continue
            report = odf_chart_object(parts, href)
            report["frame"] = of_attr(frame, "name")
            report["preview"] = bool(any(
                xml_local(kid.tag) == "image" and (kid.get(XLINK) or "").startswith("./ObjectReplacements/")
                for kid in frame
            ))
            found.append(report)
    return found


# 逐条尺寸账的上限，与 src/odsheet.rs 的 MAX_SIZE_ELEMENTS 同一个数：
# 总账按全部元素加，账本只存前这么多条
MAX_SIZE_ELEMENTS = 4096
# 表元素自己可以写的那四个数（没写的交 null，「没写」与「写了 0」不是一回事）
STATED_ON_TABLE = (
    "number-columns",
    "number-rows",
    "default-column-width",
    "default-row-height",
)


def ods_facts(path: Path, limit: int = 200, require_spreadsheet: bool = True) -> dict | None:
    """ODF 电子表格：格子内**不写数字**，写的是 office:value / date-value / boolean-value，
    位置要靠 table:number-columns-repeated 累加出来 —— 那属性一填就是 16381，
    照字面数就是每张表一万六千格。表是不是隐藏，也不在表上，在它引的那个自动样式里。
    不是表格的 ODF（文字/演示稿里的普通表）交回 None：判据是内容里有没有 spreadsheet 根。
    """
    with zipfile.ZipFile(path) as box:
        parts = {one.filename: box.read(one.filename) for one in box.infolist()}
    root = ET.fromstring(parts["content.xml"])
    if require_spreadsheet and not any(xml_local(one.tag) == "spreadsheet" for one in root.iter()):
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

    # 尺寸也不在列/行元素上：一跳在它点名的那份自动样式里（与 src/odsheet.rs 同一条路）。
    # 顺路把 visibility 与 parent-style-name 也抄下来：前者是隐藏的第二条来路，
    # 后者在这里不顺链再跳，只是让「样式找着了却没有尺寸」这件事有个说法
    SIZED = {
        "table-column": ("table-column-properties", "column-width", "use-optimal-column-width"),
        "table-row": ("table-row-properties", "row-height", "use-optimal-row-height"),
    }
    col_sized: dict[str, dict] = {}
    row_sized: dict[str, dict] = {}
    for style in root.iter():
        if xml_local(style.tag) != "style":
            continue
        shape = SIZED.get(attr(style, "family") or "")
        if shape is None:
            continue
        holder, want_size, want_optimal = shape
        props = next((one for one in style if xml_local(one.tag) == holder), None)
        got = {
            "size": attr(props, want_size) if props is not None else None,
            "optimal": attr(props, want_optimal) if props is not None else None,
            "visibility": attr(props, "visibility") if props is not None else None,
            "parent": attr(style, "parent-style-name"),
        }
        (col_sized if shape[0] == "table-column-properties" else row_sized)[
            attr(style, "name") or ""
        ] = got

    def axis_of(elems: list, kind: str) -> dict:
        """列或行：逐条账本（读的一边最多存 MAX_SIZE_ELEMENTS 条）+ 按全部元素加的总账。
        三个「看见多少」各是各的：elements/spans 全量，listed 是账本存下的，shown 是这次交的"""
        styled = col_sized if kind == "column" else row_sized
        rep_attr = "number-columns-repeated" if kind == "column" else "number-rows-repeated"
        listed: list = []
        whole = {
            "elements": 0,
            "spans": 0,
            "resolved": 0,
            "with_size": 0,
            "optimal": 0,
            "spoken_visibility": 0,
        }
        for one in elems:
            named = attr(one, "style-name")
            hit = styled.get(named) if named is not None else None
            had = {
                "style": named,
                "repeated": rep(one, rep_attr),
                "element_visibility": attr(one, "visibility"),
                "size": (hit or {}).get("size"),
                "size_mm": mm_of((hit or {}).get("size"), None),
                "optimal": (hit or {}).get("optimal"),
                "style_visibility": (hit or {}).get("visibility"),
                "style_parent": (hit or {}).get("parent"),
                "resolved": hit is not None,
            }
            whole["elements"] += 1
            whole["spans"] += had["repeated"]
            whole["resolved"] += int(had["resolved"])
            whole["with_size"] += int(had["size"] is not None)
            whole["optimal"] += int(had["optimal"] is not None)
            whole["spoken_visibility"] += int(had["element_visibility"] is not None)
            if len(listed) < MAX_SIZE_ELEMENTS:
                listed.append(had)
        shown = min(len(listed), limit)
        return dict(whole, listed=len(listed), shown=shown, list=listed[:shown])

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
        cell_notes: list = []
        # 一次收集，两份账共用（尺寸账与隐藏账必须走同一批元素）
        col_elems = [one for one in table if xml_local(one.tag) == "table-column"]
        row_elems = [one for one in table if xml_local(one.tag) == "table-row"]
        for column in col_elems:
            if attr(column, "visibility") == "collapse" or folded.get(
                attr(column, "style-name") or ""
            ):
                # 一条元素盖几列，看它自己的 number-columns-repeated
                hidden_cols += rep(column, "number-columns-repeated")
        row_at = 0
        for row in row_elems:
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
                text = "\n".join(_cell_paragraphs(cell))
                marks = _cell_marks(cell)
                # 批注不算这一格的字，但它自己要交账（作者、时间、正文）
                for had in [one for one in cell.iter() if xml_local(one.tag) == "annotation"]:
                    stamp = _first_text(had, ("date-string", "date"))
                    cell_notes.append(
                        {
                            "ref": f"{col_letter(col_at)}{row_at + 1}",
                            "author": _first_text(had, ("creator",)),
                            "date": stamp,
                            "text": "\n".join(_cell_paragraphs(had)),
                        }
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
                            # 没写 `office:value-type` 就是 null：以前这里兜一个 "empty"，
                            # 而 Rust 那边按「有没有字」推 string / empty —— 两种猜法在 .ods
                            # 上恰好撞不到一起（进了账本的格子全写了类型），到 odp 就分家了
                            "value_type": attr(cell, "value-type"),
                            "value": value,
                            "date_value": stamp,
                            "boolean_value": flag,
                            "formula": formula,
                            "text": text,
                            "spans": marks[0],
                            "specials": marks[1],
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
                "comments": cell_notes,
                "formulas": sum(1 for one in cells if one["formula"]),
                # 这一张表上的图：ODS 的 draw:frame 就住在 table:table 里面
                "charts": odf_charts_of(parts, table),
                "layout": {
                    "unit": MM_UNIT,
                    "columns": axis_of(col_elems, "column"),
                    "rows": axis_of(row_elems, "row"),
                    "stated": {one: attr(table, one) for one in STATED_ON_TABLE},
                },
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


def _ns_prefixes(raw: bytes) -> dict:
    """那份件自己声明的 xmlns：URI → 文件用的前缀（默认命名空间记成空前缀）

    ElementTree 解析时把前缀丢了，只剩 `{URI}局部名`。这一族要交的就是「文件怎么写」，
    所以按这份表把前缀还回去。
    """
    out = {}
    for prefix, uri in re.findall(rb'xmlns:([\w.\-]+)="([^"]+)"', raw):
        out[uri.decode("utf-8")] = prefix.decode("utf-8")
    for uri in re.findall(rb'xmlns="([^"]+)"', raw):
        out.setdefault(uri.decode("utf-8"), "")
    return out


def _written_name(tag: str, nsmap: dict) -> str:
    """`{URI}local` → 文件写的那个名字（`loext:graphic-properties` 这种）"""
    if not tag.startswith("{"):
        return tag
    uri, local = tag[1:].split("}", 1)
    prefix = nsmap.get(uri)
    return f"{prefix}:{local}" if prefix else local


def _written_attrs(node, nsmap: dict) -> dict:
    """一个元素的全部属性，键按文件写的名字（前缀留着）—— 与 Rust 的 kept_attrs 同一条"""
    return {_written_name(key, nsmap): value for key, value in node.attrib.items()}


ODP_STYLE_PARTS = ("content.xml", "styles.xml")
ODP_PROPS = {
    "graphic-properties": "graphic",
    "paragraph-properties": "paragraph",
    "table-cell-properties": "table_cell",
}


def odp_cell_styles(path: Path) -> dict:
    """两份件里所有 `style:style`（family=table-cell）：格子样式那三处 properties 各住在哪儿

    两份都走：实测这一族的五份格子样式全在 content.xml 的 automatic-styles 里，`styles.xml`
    里一份 family=table-cell 都没有，而占位格点的 `standard` 是 family=graphic 的另一个东西
    （占位格不进账本，所以这里数不到它）—— 只读一份就等于替文件定规矩。同名先到的一条算数
    （与 Rust 那边同一条规则）。
    """
    out: dict = {}

    def attr(node, want: str):
        """按局部名取属性，躲开 LibreOffice 抄的那份副本（与 ods_facts 里同一条）"""
        for key, value in node.attrib.items():
            if key.rsplit("}", 1)[-1] != want or "documentfoundation" in key:
                continue
            return value
        return None

    with zipfile.ZipFile(path) as box:
        names = set(one.filename for one in box.infolist())
        for part in ODP_STYLE_PARTS:
            if part not in names:
                continue
            raw = box.read(part)
            nsmap = _ns_prefixes(raw)
            for one in ET.fromstring(raw).iter():
                if xml_local(one.tag) != "style" or attr(one, "family") != "table-cell":
                    continue
                name = attr(one, "name")
                if not name or name in out:
                    continue
                had = {
                    "name": name,
                    "part": part,
                    "family": attr(one, "family"),
                    "parent": attr(one, "parent-style-name"),
                }
                for key in ODP_PROPS.values():
                    had[f"{key}_element"] = None
                    had[f"{key}_attrs"] = None
                for kid in one:
                    slot = ODP_PROPS.get(xml_local(kid.tag))
                    if slot is None:
                        continue
                    had[f"{slot}_element"] = _written_name(kid.tag, nsmap)
                    had[f"{slot}_attrs"] = _written_attrs(kid, nsmap)
                out[name] = had
    return out


def odp_style_props(styles: dict, name):
    """一格点的那份样式找到了没有、找到了就把它写的东西原样交"""
    had = styles.get(name) if name else None
    if had is None:
        return {
            "style": name,
            "found": False,
            "part": None,
            "family": None,
            "parent": None,
            "graphic_element": None,
            "graphic": None,
            "paragraph_element": None,
            "paragraph": None,
            "table_cell_element": None,
            "table_cell": None,
        }
    return {
        "style": name,
        "found": True,
        "part": had["part"],
        "family": had["family"],
        "parent": had["parent"],
        "graphic_element": had["graphic_element"],
        "graphic": had["graphic_attrs"],
        "paragraph_element": had["paragraph_element"],
        "paragraph": had["paragraph_attrs"],
        "table_cell_element": had["table_cell_element"],
        "table_cell": had["table_cell_attrs"],
    }


def odp_cell_tally(cell_list: list, styles: dict) -> dict:
    """这张表上的格子有多少点了样式、点到的解开了没有、那三处 properties 各有几格"""
    named = resolved = unwritten = 0
    with_props = {"graphic": 0, "paragraph": 0, "table_cell": 0}
    elements: list = []
    for one in cell_list:
        name = one["style"]
        if not name:
            unwritten += 1
            continue
        named += 1
        had = styles.get(name)
        if had is None:
            continue
        resolved += 1
        for slot in with_props:
            if had[f"{slot}_element"] is None:
                continue
            with_props[slot] += 1
            if had[f"{slot}_element"] not in elements:
                elements.append(had[f"{slot}_element"])
    return {
        "named": named,
        "resolved": resolved,
        "unwritten": unwritten,
        "with_graphic": with_props["graphic"],
        "with_paragraph": with_props["paragraph"],
        "with_table_cell_properties": with_props["table_cell"],
        "elements": elements,
    }


def link_scheme(raw: str):
    """地址里 scheme 那一截：`https` / `mailto` …；没有冒号、冒号前是空的（`#那一页`
    这种站内跳法）、只有**一个字母**（那是 Windows 的盘符不是 scheme）、或者太长太怪的，
    一律 None —— 与 Rust 的 `link_scheme` 同一条，只说文件写了什么，不猜它是哪一类
    """
    head, sep, _rest = raw.partition(":")
    if not sep:
        return None
    if len(head) < 2 or len(head) > 8:
        return None
    if not all(one.isascii() and (one.isalnum() or one in "+-.") for one in head):
        return None
    return head.lower()


def _link_rows(rows: list) -> dict:
    """一页的链接那一份账：合计三个数 + 逐条"""
    return {
        "total": len(rows),
        "external": sum(1 for one in rows if one["external"] is True),
        "unresolved": sum(1 for one in rows if one["target"] is None),
        "list": rows,
    }


def slide_rels(rels_root, source: str) -> list:
    """一个部件自己的关系表，内、外都要（按文件写的顺序）：kind / target / external

    内部那条的 `target` 按源部件解成包内全名，外部的照原样交。`Type` 或 `Target`
    没写的条目不收 —— 那在两边都不成一条关系。Rust 侧同名账在 src/office_slide.rs。
    """
    if rels_root is None:
        return []
    out: list = []
    for one in rels_root.iter():
        if xml_local(one.tag) != "Relationship":
            continue
        had_type, raw = one.get("Type"), one.get("Target")
        if had_type is None or raw is None:
            continue
        external = one.get("TargetMode") == "External"
        out.append(
            {
                "kind": had_type.rsplit("/", 1)[-1],
                "target": raw if external else opc_target(source, raw),
                "external": external,
            }
        )
    return out


def pptx_slide_links(root, rels_root, limit: int = 200) -> dict:
    """OOXML 一页上的链接：run 的 `a:rPr/a:hlinkClick` 只写一个号，地址在页自己的关系表里

    `TargetMode` 没写时交 None（那与写了 `External` 是两件事）；号在关系表里找不到时
    `target` / `external` 都交 None，而那个号照交 —— 「写了个指不到东西的号」是文件说的话。
    连号都没写时 `id` 也交 None（不是空串）。
    """
    pool: dict = {}
    if rels_root is not None:
        for one in rels_root.iter():
            if xml_local(one.tag) != "Relationship":
                continue
            if (of_local(one, "Type") or "").rsplit("/", 1)[-1] != "hyperlink":
                continue
            had = of_local(one, "Id")
            if not had:
                continue
            mode = of_local(one, "TargetMode")
            pool[had] = (of_local(one, "Target"), None if mode is None else mode == "External")
    rows: list = []
    for run in root.iter():
        if xml_local(run.tag) != "r":
            continue
        click = None
        for one in run.iter():
            if xml_local(one.tag) == "hlinkClick":
                click = one
                break
        if click is None:
            continue
        # 关系号一定带前缀（`r:id`）：那个名字是文档自己声明的，按局部名去找
        rid = of_local(click, "id")
        target, mode = pool.get(rid, (None, None)) if rid else (None, None)
        rows.append(
            {
                "text": "".join(
                    one.text or "" for one in run.iter() if xml_local(one.tag) == "t"
                ),
                "target": target,
                "scheme": link_scheme(target) if target else None,
                "external": mode,
                "hop": "rels",
                "id": rid,
            }
        )
        if len(rows) >= limit:
            break
    return _link_rows(rows)


def odp_slide_links(frames: list, limit: int = 200) -> dict:
    """ODF 一页上的链接：地址就挂在字上（`text:a/@xlink:href`），没有第二跳

    所以 `external` 与 `id` 都是 None —— 这一族没有那个开关，也没有号；
    走的是「页上除 `presentation:notes` 以外的那几块」：备注里的链不是页面上的链。
    （不是只走 `draw:frame` —— 实测 python-pptx 那个文本框被 Impress 改写成了
    `draw:custom-shape`，只认 frame 会把三条链全读成 0。）
    """
    rows: list = []
    for owner in frames:
        for one in owner.iter():
            if xml_local(one.tag) != "a":
                continue
            target = of_local(one, "href")
            if target is None:
                continue
            rows.append(
                {
                    "text": " ".join("".join(one.itertext()).split()),
                    "target": target,
                    "scheme": link_scheme(target),
                    "external": None,
                    "hop": "inline",
                    "id": None,
                }
            )
            if len(rows) >= limit:
                break
        if len(rows) >= limit:
            break
    return _link_rows(rows)


def odp_slide_tables(path: Path, limit: int = 100) -> list:
    """每一页上那些表：`draw:frame` 那一份（名字与位置都只在容器上）+ ODF 表那一份。

    表的读法直接复用 `ods_facts`（页上的表与 .ods 里的表是同一种元素，两本账不该各写一遍）；
    配对按文档顺序 —— `ods_facts` 走的是全局 `table` 元素的文档顺序，这边按页取「带表的
    frame」，两边看到的必然是同一批元素。返回的是**按页分组**的一个列表，与 office-slide
    每页那个 `table_list` 一对一。`limit` 是 office-slide 的那个 100（不是 office-sheet 的 200）。
    """
    facts = ods_facts(path, limit=limit, require_spreadsheet=False)
    if facts is None:
        return []
    styles = odp_cell_styles(path)
    with zipfile.ZipFile(path) as box:
        root = ET.fromstring(box.read("content.xml"))
    tables = facts["sheets"]
    out: list = []
    seen = 0
    for page in [one for one in root.iter() if xml_local(one.tag) == "page"]:
        group: list = []
        at = 0
        for frame in page:
            if xml_local(frame.tag) != "frame":
                continue
            tbl = _kid(frame, "table")
            if tbl is None or at >= limit:
                continue
            if seen >= len(tables):
                break
            one = tables[seen]
            seen += 1
            rows = [
                {
                    "ref": had["ref"],
                    "text": had["text"],
                    "kind": had["value_type"],
                    "span_cols": had["columns_spanned"],
                    "span_rows": had["rows_spanned"],
                    "style": had["style"],
                    "style_props": odp_style_props(styles, had["style"]),
                }
                for had in one["cell_list"][:limit]
            ]
            group.append(
                {
                    "at": at,
                    "frame": written_attrs(frame),
                    "written": written_attrs(tbl),
                    "name": one["name"],
                    "state": "visible" if one["visible"] else "hidden",
                    "rows": one["rows"],
                    "columns": one["columns"],
                    "cells": one["cells"],
                    "covered": one["covered"],
                    "merged": one["merged"],
                    "cell_list": rows,
                    "cell_styles": odp_cell_tally(rows, styles),
                    "layout": one["layout"],
                }
            )
            at += 1
        out.append(group)
    return out


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


def odp_drawing_page_styles(parts: dict) -> dict:
    """两份件里 family=drawing-page 的样式：名字 → (在哪个部件, 那份 properties 写的 visibility)

    同名取第一个（与 Rust 那边 `find` 同一条规则）。`visibility` 没写是 None ——
    那与「写了 visible」是两件事，而**没找到那份样式**又是第三件事（交回 hidden None）。
    """
    out: dict = {}
    for part in ("content.xml", "styles.xml"):
        if part not in parts:
            continue
        for one in ET.fromstring(parts[part]).iter():
            if xml_local(one.tag) != "style" or of_local(one, "family") != "drawing-page":
                continue
            name = of_local(one, "name")
            if name is None or name in out:
                continue
            props = None
            for kid in one:
                if xml_local(kid.tag) == "drawing-page-properties":
                    props = kid
                    break
            out[name] = (part, of_local(props, "visibility") if props is not None else None)
    return out


def odp_page_visibility(styles: dict, named) -> dict:
    """一页在放映时藏不藏：ODF 不写在页上，写在页点名的那份 drawing-page 样式里"""
    if named is None or named not in styles:
        return {
            "hidden": None,
            "page_style": named,
            "style_found": False,
            "visibility_written": None,
        }
    part, written = styles[named]
    return {
        "hidden": written == "hidden",
        "page_style": named,
        "style_found": True,
        "visibility_written": written,
        "style_part": part,
    }


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
    # 页的「放映时隐藏」在那份 family=drawing-page 的样式里，两份件都要收（不赌自动样式
    # 一定在 content.xml）
    page_styles = odp_drawing_page_styles(parts)

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
"charts": odf_charts_of(parts, page),
                # 没有 title 占位时退回第一段：两边同一口径，不然比的是两份规则
                "title": title if title else (texts[0] if texts else ""),
                "master": master,
                "layout": of_local(page, "presentation-page-layout-name"),
                "placeholders": placeholders,
                "texts": texts,
                "notes": notes,
                "notes_frame_classes": note_classes,
                "pictures": sum(1 for one in page.iter() if xml_local(one.tag) == "image"),
                "picture_rows": odt_picture_rows(
                    page, ns_prefixes(parts["content.xml"].decode("utf8"))
                ),
                "tables": sum(1 for one in page.iter() if xml_local(one.tag) == "table"),
                # 这一族的链接直接写在字上，且只走页上的 frame（备注那一块另算）
                "links": odp_slide_links(
                    [one for one in page if xml_local(one.tag) != "notes"]
                ),
                # 藏不藏要跳一跳：页只点名样式，那句话在那份样式里
                "hidden": odp_page_visibility(
                    page_styles, of_local(page, "style-name")
                )["hidden"],
                "visibility": odp_page_visibility(page_styles, of_local(page, "style-name")),
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
                    # 不 strip：首尾那两个空格是文件写的（LO 自己的 CSV 也带着它们）
                    inline = "".join(kid.itertext())
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
                # 文件自己就写着显示的那串（`<v>#DIV/0!</v>`）：一字不加。
                # 这一支以前与 Rust 一起编成 `#错误 #DIV/0!` —— 两份读者一起错，
                # 谁也没撞上，因为手上一直没有真写出 t="e" 的生产者（现在有了：errors-lo.xlsx）
                display = value
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


def _said_on(had):
    """OOXML 那两种布尔拼法：元素在而没写 val 按 true，0 / false / none 按 false，没这个元素是 None"""
    if had is None:
        return None
    value = had.get("val")
    if value is None:
        return True
    return value.strip().lower() not in ("0", "false", "none")


def _style_row(node, depth: int = 2):
    """一张表的一行：自己写着的属性 + 孩子元素（名字与各自的属性），按文件原样

    与 Rust 的 `row_json` 同一条：往下走两层，因为底色字色写在第三层
    （`fill > patternFill > fgColor`），只走一层就看不见那个颜色串。
    """
    parts = [
        {"element": xml_local(kid.tag),
         "attrs": dict(sorted((xml_local(key), value) for key, value in kid.attrib.items())),
         "parts": _style_row(kid, depth - 1)["parts"]}
        for kid in node
    ] if depth > 0 else []
    return {
        "attrs": dict(sorted((xml_local(key), value) for key, value in node.attrib.items())),
        "parts": parts,
    }


def xlsx_style_tables(parts: dict) -> dict:
    """`cellXfs` 那一跳的四张表（与 numfmt.rs 的 `table_rows` 同一条规则）"""
    tables = {"fonts": [], "fills": [], "borders": [], "xfs": [], "style_xfs": []}
    if "xl/styles.xml" not in parts:
        tables["ledger"] = {"part": False}
        return tables
    root = ET.fromstring(parts["xl/styles.xml"])

    def holder(want: str):
        for one in root.iter():
            if xml_local(one.tag) == want:
                return one
        return None

    def rows_of(want: str, keep: str) -> list:
        top = holder(want)
        if top is None:
            return []
        return [_style_row(one) for one in top if xml_local(one.tag) == keep]

    def stated(want: str, found: int) -> dict:
        top = holder(want)
        written = None if top is None else top.get("count")
        try:
            agreed = written is None or int((written or "").strip()) == found
        except ValueError:
            agreed = False
        return {"written": written, "found": found, "whole": agreed}

    tables["fonts"] = rows_of("fonts", "font")
    tables["fills"] = rows_of("fills", "fill")
    tables["borders"] = rows_of("borders", "border")
    tables["xfs"] = rows_of("cellXfs", "xf")
    tables["style_xfs"] = rows_of("cellStyleXfs", "xf")
    tables["ledger"] = {
        "part": True,
        "fonts": stated("fonts", len(tables["fonts"])),
        "fills": stated("fills", len(tables["fills"])),
        "borders": stated("borders", len(tables["borders"])),
        "cell_xfs": stated("cellXfs", len(tables["xfs"])),
        "cell_style_xfs": stated("cellStyleXfs", len(tables["style_xfs"])),
    }
    return tables


def child_attrs(row, name: str):
    """一行表内容里那个孩子元素的属性（没有那个孩子就给 None）"""
    if row is None:
        return None
    for one in row.get("parts") or []:
        if one["element"] == name:
            return one["attrs"]
    return None


def style_appearance(tables: dict, index: int) -> dict:
    """一个格子的长相：`cellXfs` 那条自己说了什么 + 顺着三个号查到的三行"""
    blank = {
        "style_found": False,
        "style_attrs": None,
        "style_font_id": None,
        "style_font": None,
        "style_fill_id": None,
        "style_fill": None,
        "style_border_id": None,
        "style_border": None,
        "style_alignment": None,
        "style_bold": None,
        "style_filled": None,
        "style_wrapped": None,
    }
    rows = tables.get("xfs") or []
    if index >= len(rows):
        return blank
    row = rows[index]
    attrs = row["attrs"]

    def pick(key: str, table: list):
        raw = attrs.get(key)
        try:
            which = int((raw or "").strip())
        except ValueError:
            return None
        return table[which] if 0 <= which < len(table) else None

    font = pick("fontId", tables["fonts"])
    fill = pick("fillId", tables["fills"])
    border = pick("borderId", tables["borders"])
    alignment = child_attrs(row, "alignment")
    pattern = child_attrs(fill, "patternFill") if fill is not None else None
    filled = None
    if pattern is not None:
        filled = pattern.get("patternType") not in (None, "none")
    # `wrapText` 是属性形式的开关：没写这个属性就是「文件没说」（null），
    # 不像 `<b/>` 那种元素形式可以按 true 算
    wrapped = None
    if alignment is not None:
        raw = alignment.get("wrapText")
        if raw is not None:
            wrapped = raw.strip().lower() not in ("", "0", "false", "none")
    return {
        "style_found": True,
        "style_attrs": attrs,
        "style_font_id": attrs.get("fontId"),
        "style_font": font,
        "style_fill_id": attrs.get("fillId"),
        "style_fill": fill,
        "style_border_id": attrs.get("borderId"),
        "style_border": border,
        "style_alignment": alignment,
        "style_bold": _said_on(child_attrs(font, "b") if font is not None else None),
        "style_filled": filled,
        "style_wrapped": wrapped,
    }


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
            out["revisions"] = docx_revision_ledger(path)
            out["protection"] = protection_for(path)
        elif "xl/workbook.xml" in parts:
            out["app"] = "excel"
            out["ooxml"] = xlsx_facts(path)
            out["protection"] = protection_for(path)
            if path.suffix.lower() == ".xlsx":
                out["formats"] = xlsx_formats(path)
            out["csv"] = csv_facts(path)
            out["hidden"] = xlsx_hidden(path)
            out["comments"] = xlsx_comments(path)
        elif "ppt/presentation.xml" in parts:
            out["app"] = "powerpoint"
            out["ooxml"] = pptx_facts(path)
        elif "content.xml" in parts:
            out["app"] = "opendocument"
            out["odf"] = odt_facts(path)
            out["links"] = odf_links(path)
            out["page_text"] = odf_page_text(path)
            out["odt"] = odt_structure(path)
            ledger = odt_revision_ledger(path)
            if ledger is not None:
                out["revisions"] = ledger
            out["protection"] = protection_for(path)
            sheets = ods_facts(path)
            if sheets is not None:
                out["ods"] = sheets
                out["csv"] = csv_facts(path)
                out["ods_styles"] = ods_styles(path)
            deck = odp_facts(path)
            if deck is not None:
                # 每页那张表的账（frame 那一份 + ODF 表那一份），按页一对一挂上去
                groups = odp_slide_tables(path)
                for which, slide in enumerate(deck["slides"]):
                    slide["table_list"] = groups[which] if which < len(groups) else []
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
        one = lyco_pdf.pdf_facts(data)
        one["text"] = lyco_pdf.page_text(data)
        # 去哪儿那一层：书签、页内链接、权限位（lbin office-pdf 的 outline/links/permissions）
        nav = lyco_pdf_nav.permissions(data)
        sealed = bool(nav.get("encrypted"))
        one["permissions"] = nav
        # 表单那一份：`/AcroForm` → `/Fields` → `/Kids`，值只交文件写的
        one["form"] = lyco_pdf.form_facts(data)
        one["outline"] = lyco_pdf_nav.outlines(data, encrypted=sealed)
        one["links"] = lyco_pdf_nav.links(data, encrypted=sealed)
        one["annotations"] = lyco_pdf_nav.annotations(data, encrypted=sealed)
        out["pdf"] = one
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
