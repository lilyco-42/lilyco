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
from lyco_markdown import docx_markdown as docx_markdown_ledger  # 结构搬进 markdown：docx 那一本
from lyco_markdown import odf_markdown as odf_markdown_ledger  # 同一本的 ODF 那一面（与 markdown.rs::odf 同口径）
from lyco_equations import docx_equations as docx_equations_ledger  # OMML 那一份账的第二读者
from lyco_equations import odf_equations as odf_equations_ledger  # 公式部件（MathML）那一份
from lyco_equations import odp_equations as odp_equations_ledger  # 放映每一页的公式（同一条判据）
from lyco_equations import pptx_equations as pptx_equations_ledger  # pptx：文本体里的 OMML

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


SLOT_KEYS = ("header:default", "header:first", "header:even",
             "footer:default", "footer:first", "footer:even")


def docx_header_footers(body, parts: dict, limit: int = 200) -> dict:
    """分节的页眉页脚六格：`w:sectPr` 没写那一种引用，就是沿用**上一处写了它的节**

    「自己写的」与「真正用到的」两个都交、不合并；`w:titlePg` 与 `w:evenAndOddHeaders`
    按写的交（元素在不在、值写的是什么），「所以这一格显不显示」不归读者判。
    """
    rels: dict[str, tuple] = {}
    if "word/_rels/document.xml.rels" in parts:
        for one in ET.fromstring(parts["word/_rels/document.xml.rels"]):
            if xml_local(one.tag) != "Relationship" or one.get("Id") in rels:
                continue
            rels[one.get("Id")] = (one.get("Target"), one.get("TargetMode") or "")
    names = set(parts)
    eo = {"written": False, "val": None}
    if "word/settings.xml" in parts:
        settings = ET.fromstring(parts["word/settings.xml"])
        got = [one for one in settings.iter() if xml_local(one.tag) == "evenAndOddHeaders"]
        if got:
            eo = {"written": True, "val": local_attr(got[0], "val")}
    sections = []
    last_own: list = [None] * 6
    written = inherited = refs_total = refs_unresolved = parts_missing = 0
    sects = [one for one in body.iter() if xml_local(one.tag) == "sectPr"]
    for index, sect in enumerate(sects):
        mine: dict = {}
        refs_here = 0
        for kid in sect:
            name = xml_local(kid.tag)
            if name == "headerReference":
                kind = "header"
            elif name == "footerReference":
                kind = "footer"
            else:
                continue
            typ = local_attr(kid, "type") or "default"
            ident = local_attr(kid, "id")
            hit = rels.get(ident) if ident else None
            refs_here += 1
            refs_total += 1
            if ident is not None and hit is None:
                refs_unresolved += 1
            raw, mode = hit if hit else (None, "")
            part = None
            if raw is not None and mode != "External":
                got = opc_target("word/document.xml", raw)
                part = got if got in names else None
            external = (mode == "External") if hit is not None else None
            exists = part is not None
            # `part` 为空有两种来路：站外关系，或者指着包里没有的部件 —— 后者另数一本
            if hit is not None and not external and not exists:
                parts_missing += 1
            key = "%s:%s" % (kind, typ)
            if key not in mine:  # 同一种写了两条：取第一条，与 Rust 的 `find` 同一个口径
                mine[key] = {
                    "written": written_attrs(kid),
                    "id": ident,
                    "target": raw,
                    "part": part,
                    "part_exists": exists,
                    "external": external,
                }
        slots: dict = {}
        for which, key in enumerate(SLOT_KEYS):
            own = mine.get(key)
            row = None
            if own is not None:
                written += 1
                last_own[which] = own
                row = ("own", own)
            elif last_own[which] is not None:
                inherited += 1
                row = ("earlier-section", last_own[which])
            if row is None:
                slots[key] = None
            else:
                got = dict(row[1])
                got["from"] = row[0]
                slots[key] = got
        sections.append({
            "index": index,
            "slots": slots,
            "written_refs": refs_here,
            "title_pg_written": any(xml_local(one.tag) == "titlePg" for one in sect),
        })
    return {
        "sections": sections[:limit],
        "sections_total": len(sects),
        "listed": len(sections),
        "slot_keys": list(SLOT_KEYS),
        "slots_written": written,
        "slots_inherited": inherited,
        "refs_total": refs_total,
        "refs_unresolved": refs_unresolved,
        "parts_missing": parts_missing,
        "even_and_odd_headers": eo,
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


def _local_in(attrs: dict, local: str):
    """一份「文件写的名字 → 值」的表里按局部名取那一个（前缀不参与匹配）"""
    for key, value in attrs.items():
        if key.rsplit(":", 1)[-1] == local:
            return value
    return None


def _bump(tally: dict, raw) -> None:
    """把文件写的那一串当键数一遍；没说话的那些数成一格 `(没写)`"""
    key = raw if raw is not None else "(没写)"
    tally[key] = tally.get(key, 0) + 1


THEME_WHICH = {
    "HAnsi": "latin",
    "Latin": "latin",
    "EastAsia": "ea",
    "EA": "ea",
    "Bidi": "cs",
    "CS": "cs",
    "ComplexScript": "cs",
}


def theme_hop(raw: str):
    """主题那一路的指针：前缀定槽、后缀定那一路。认不出的后缀交 None，不猜"""
    for prefix, slot in (("major", "majorFont"), ("minor", "minorFont")):
        if raw.startswith(prefix):
            which = THEME_WHICH.get(raw[len(prefix):])
            return (slot, which) if which else None
    return None


OOXML_FONT_POINTERS = ("ascii", "hAnsi", "eastAsia", "cs")


def docx_fonts(parts: dict, body, limit: int = 100) -> dict:
    r"""这份文档要点哪些字体（OOXML）：表在 `word/fontTable.xml`，点在每一格 `w:rFonts` 上

    与 `src/office_doc.rs` 的 `docx_fonts` 一份账。三个坑：一个选择按书写系统写成四个属性
    （python-docx 一次写 `ascii` 与 `hAnsi` 两遍，于是「几条属性」不是「几段字」）；
    指向主题的那一路要再跳一跳才落到字面名，而第四个的拼法是 `cstheme`（小写 th）；
    `cs=""` 是「写了，说的是空话」，与整个没这个属性是两件事。
    表里没写过的行按 `sorted()` 走 —— Rust 那边属性表是排序的（BTreeMap），
    两边同一个顺序才比得出整份账。
    """
    declared: list = []
    names: list = []
    table_part = "word/fontTable.xml" if "word/fontTable.xml" in parts else None
    embed_refs = 0
    embed_by: dict = {}
    if table_part:
        raw = parts[table_part]
        nsmap = _ns_prefixes(raw)
        for one in ET.fromstring(raw).iter():
            if xml_local(one.tag) != "font":
                continue
            written = written_attrs(one)
            name = written.get("name")
            if name is not None:
                names.append(name)
            embeds = []
            for kid in one:
                if not xml_local(kid.tag).startswith("embed"):
                    continue
                embed_refs += 1
                _bump(embed_by, xml_local(kid.tag))
                embeds.append({"element": _written_name(kid.tag, nsmap),
                               "written": written_attrs(kid)})
            if len(declared) < limit:
                declared.append({"name": name, "written": written, "embeds": embeds})
    theme_part = "word/theme/theme1.xml" if "word/theme/theme1.xml" in parts else None
    scheme: list = []
    if theme_part:
        for one in ET.fromstring(parts[theme_part]).iter():
            if xml_local(one.tag) not in ("majorFont", "minorFont"):
                continue
            for kid in one:
                which = xml_local(kid.tag)
                if which not in ("latin", "ea", "cs"):
                    continue
                scheme.append((xml_local(one.tag), which, written_attrs(kid)))
    roots = [(body, "document.xml")]
    if "word/styles.xml" in parts:
        roots.append((ET.fromstring(parts["word/styles.xml"]), "styles.xml"))
    rows: list = []
    elements = empty_written = theme_refs = theme_resolved = 0
    pointed: dict = {}
    themed: dict = {}
    other_attrs: dict = {}
    for root, part in roots:
        for one in root.iter():
            if xml_local(one.tag) != "rFonts":
                continue
            elements += 1
            written = written_attrs(one)
            points: list = []
            hops: list = []
            rest: list = []
            for key in sorted(written):
                raw = written[key]
                if key in OOXML_FONT_POINTERS:
                    _bump(pointed, raw)
                    if raw == "":
                        empty_written += 1
                    points.append({"attr": key, "value": raw, "declared": raw in names})
                    continue
                if key.endswith("heme"):
                    theme_refs += 1
                    _bump(themed, raw)
                    hop = theme_hop(raw)
                    found = None
                    if hop is not None:
                        for slot, which, had in scheme:
                            if (slot, which) == hop:
                                found = _local_in(had, "typeface")
                                break
                    if found is not None:
                        theme_resolved += 1
                    hops.append({
                        "attr": key,
                        "value": raw,
                        "slot": hop[0] if hop else None,
                        "which": hop[1] if hop else None,
                        "typeface": found,
                        "resolved": found is not None,
                    })
                    continue
                _bump(other_attrs, key)
                rest.append(key)
            if len(rows) < limit:
                rows.append({
                    "part": part,
                    "written": written,
                    "points": points,
                    "themes": hops,
                    "other_attrs": rest,
                })
    undeclared = [key for key in sorted(pointed) if key and key not in names]
    unused = [one for one in names if one not in pointed]
    return {
        "family": "ooxml",
        "table_part": table_part,
        "declared": declared,
        "declared_total": len(declared),
        "declared_names": len(names),
        "pointer_elements": elements,
        "rows": rows,
        "pointed_by_value": pointed,
        "themed_by_value": themed,
        "other_attrs": other_attrs,
        "empty_written": empty_written,
        "undeclared": undeclared,
        "undeclared_total": len(undeclared),
        "declared_unused": unused,
        "declared_unused_total": len(unused),
        "theme_part": theme_part,
        "scheme": [{"slot": slot, "which": which, "written": had}
                   for slot, which, had in scheme],
        "theme_refs": theme_refs,
        "theme_resolved": theme_resolved,
        "embedded_refs": embed_refs,
        "embedded_by_element": embed_by,
        "embedded_parts": sorted(one for one in parts if one.startswith("word/fonts/")),
    }


def odf_header_footers(path: Path, limit: int = 100) -> dict:
    r"""ODF 的页眉页脚在**母版页**上（`style:master-page`），一格一个子元素

    六格：`style:header` / `style:footer` 再各配 `-first`（第一页）与 `-left`（偶数页）。
    这一族没有「沿用上一节」这件事：节只点名一份版式（`text:section/@style:page-layout-name`），
    而「正文用哪份母版页」在这些文件里根本没写 —— 所以每格只有两种答案（写了 / null），
    `used_by_sections` 另说有没有一节点过这份母版页的名。
    实测最要紧的一条：LibreOffice 把两节的 docx 转成 odt 时**不写 text:section**，
    而是造出第二份母版页（`Converted1`）把第二节那句页眉搬进去，全文没有任何一节点它的名。
    """
    slots_of = ("header", "header-first", "header-left", "footer", "footer-first", "footer-left")
    keys_of = ("header:default", "header:first", "header:left",
               "footer:default", "footer:first", "footer:left")
    field_words = ("page-number", "page-count", "date", "time", "expression", "sender-full-name")
    with zipfile.ZipFile(path) as box:
        have = set(one.filename for one in box.infolist())
        roots = []
        for part in ("content.xml", "styles.xml"):
            if part not in have:
                continue
            raw = box.read(part)
            roots.append((part, ET.fromstring(raw), _ns_prefixes(raw)))
    sections: list = []
    named: list = []
    layouts_named = 0
    for part, root, nsmap in roots:
        for one in root.iter():
            if xml_local(one.tag) != "section":
                continue
            written = _written_attrs(one, nsmap)
            master = _local_in(written, "master-page-name")
            layout = _local_in(written, "page-layout-name")
            if master is not None:
                named.append(master)
            if layout is not None:
                layouts_named += 1
            if len(sections) < limit:
                sections.append({
                    "name": _local_in(written, "name"),
                    "style": _local_in(written, "style-name"),
                    "master_page": master,
                    "page_layout": layout,
                    "written": written,
                    "part": part,
                })
    masters: list = []
    seen: list = []
    dup = slots_written = field_slots = unnamed = 0
    for part, root, nsmap in roots:
        for one in root.iter():
            if xml_local(one.tag) != "master-page":
                continue
            written = _written_attrs(one, nsmap)
            name = _local_in(written, "name") or ""
            if name in seen:
                dup += 1
                continue
            seen.append(name)
            if name not in named:
                unnamed += 1
            slots: dict = {}
            mine = 0
            for key, slot in zip(keys_of, slots_of):
                holder = None
                for kid in one:
                    if xml_local(kid.tag) == slot:
                        holder = kid
                        break
                if holder is None:
                    slots[key] = None
                    continue
                # 段可以在 `style:region-left` / `-right` 里面（.ods 的 Report 那份就是），
                # 所以走全部后代；每一半另交一份
                paras = [had for had in holder.iter() if xml_local(had.tag) == "p"]
                regions = []
                for kid in holder:
                    if xml_local(kid.tag) not in ("region-left", "region-right"):
                        continue
                    inner = [had for had in kid.iter() if xml_local(had.tag) == "p"]
                    regions.append({
                        "element": _written_name(kid.tag, nsmap),
                        "paragraphs": len(inner),
                        "text": "\n".join(
                            "".join(had.itertext()).strip() for had in inner
                        ),
                    })
                fields: dict = {}
                for kind in field_words:
                    hits = [had for had in holder.iter() if xml_local(had.tag) == kind]
                    if hits:
                        fields[kind] = len(hits)
                mine += 1
                slots_written += 1
                if fields:
                    field_slots += 1
                slots[key] = {
                    "present": True,
                    "element": _written_name(holder.tag, nsmap),
                    "written": _written_attrs(holder, nsmap),
                    "display_written": _local_in(
                        _written_attrs(holder, nsmap), "display"
                    ),
                    "paragraphs": len(paras),
                    "text": "\n".join("".join(had.itertext()).strip() for had in paras),
                    "fields": fields,
                    "regions": regions,
                }
            if len(masters) < limit:
                masters.append({
                    "name": name,
                    "page_layout": _local_in(written, "page-layout-name"),
                    "written": written,
                    "used_by_sections": [had["name"] for had in sections
                                         if had["master_page"] == name],
                    "slots_written": mine,
                    "slots": slots,
                    "part": part,
                })
    return {
        "family": "odf",
        "masters": masters,
        "masters_total": len(seen),
        "masters_duplicated": dup,
        "masters_named_by_section": len(seen) - unnamed,
        "masters_unnamed": unnamed,
        "slots_written": slots_written,
        "field_slots": field_slots,
        "sections": sections,
        "sections_total": len(sections),
        "layouts_named_by_section": layouts_named,
    }


def odf_page_styles(path: Path, limit: int = 100) -> dict:
    """同一份账的入口：office-sheet 那边只要文件名与 limit（与 Rust 的 `odf_page_styles` 一条）"""
    out = odf_header_footers(path, limit)
    out["available"] = True
    return out


def xlsx_print_ranges(path: Path, limit: int = 100) -> dict:
    r"""「打哪几行几列、每页重复哪一行」在 OOXML 里**不在表上**：那是 workbook.xml 的两条保留名

    `_xlnm.Print_Area` 与 `_xlnm.Print_Titles`，归属靠 `localSheetId` —— 那个数数的是
    `<sheets>` 里的**顺序**（不是 `sheetId`，也不是 `r:id`，那三套号各自编）。一条 definedName
    里可以塞好几段（逗号分隔）；sheet 名带不带引号是生产者的写法，不替它们归一化。
    """
    with zipfile.ZipFile(path) as box:
        have = set(one.filename for one in box.infolist())
        if "xl/workbook.xml" not in have:
            return {"available": False}
        raw = box.read("xl/workbook.xml")
    root = ET.fromstring(raw)
    nsmap = _ns_prefixes(raw)
    order = [
        _local_in(_written_attrs(one, nsmap), "name") or ""
        for one in root.iter()
        if xml_local(one.tag) == "sheet"
    ]
    entries: list = []
    for one in root.iter():
        if xml_local(one.tag) != "definedName":
            continue
        written = _written_attrs(one, nsmap)
        name = _local_in(written, "name") or ""
        id_written = _local_in(written, "localSheetId")
        text = (one.text or "").strip()
        index = None
        if id_written is not None:
            try:
                index = int(id_written)
            except ValueError:
                index = None
        in_range = index is not None and 0 <= index < len(order)
        entries.append({
            "name": name,
            "reserved": name.startswith("_xlnm."),
            "local_sheet_id_written": id_written,
            "local_sheet_id": index,
            "resolved_sheet": order[index] if in_range else None,
            "in_range": in_range,
            "text": text,
            "ranges": [chunk for chunk in text.split(",") if chunk],
            "quoted_names": "'" in text,
            "written": written,
        })
    by_name: dict = {}
    for one in entries:
        by_name[one["name"]] = by_name.get(one["name"], 0) + 1
    sheets = []
    for position, name in enumerate(order):
        mine = [one for one in entries if one["resolved_sheet"] == name]
        areas = [one for one in mine if one["name"] == "_xlnm.Print_Area"]
        titles = [one for one in mine if one["name"] == "_xlnm.Print_Titles"]
        sheets.append({
            "index": position,
            "name": name,
            "area_entries": len(areas),
            "titles_entries": len(titles),
            "area_ranges": [rng for one in areas for rng in one["ranges"]],
            "titles_ranges": [rng for one in titles for rng in one["ranges"]],
        })
    return {
        "family": "ooxml",
        "available": True,
        "sheets_total": len(order),
        "defined_total": len(entries),
        "print_entries": sum(1 for one in entries if one["reserved"]),
        "unresolved": sum(1 for one in entries if one["reserved"] and not one["in_range"]),
        "quoted_entries": sum(1 for one in entries if one["reserved"] and one["quoted_names"]),
        "by_name": by_name,
        "sheets": sheets[:limit],
        "entries": entries[:limit],
    }


def ods_print_ranges(path: Path, limit: int = 100) -> dict:
    r"""同一问在 ODF 里住在**两处**：表自己身上的 `table:print-ranges`，与另写的 `table:named-*`

    后者是 LibreOffice 为了与 Excel 来回而留的副本，名字一律 `Excel_BuiltIn_Print_Area` /
    `_Print_Titles`。实测两条要紧的：一段范围走 `table:named-range`、两段走 `table:named-expression`
    （同一个选择在一种文件里是两种元素）；而那一份件里五样的 `table:base-cell-address` 全是同一个
    （第一张表的 A1）—— 所以「这是哪张表的」只在地址串里，不在这条指针上。
    `range-usable-as` 更是「重复行」与「重复列」都写同一串，读出「重复的是行」只能看地址形状。
    """
    with zipfile.ZipFile(path) as box:
        have = set(one.filename for one in box.infolist())
        if "content.xml" not in have:
            return {"available": False}
        raw = box.read("content.xml")
    root = ET.fromstring(raw)
    nsmap = _ns_prefixes(raw)
    tables = []
    named = []
    for one in root.iter():
        local = xml_local(one.tag)
        if local == "table":
            written = _written_attrs(one, nsmap)
            value = _local_in(written, "print-ranges")
            ranges = [] if value is None else [chunk for chunk in value.split() if chunk]
            tables.append({
                "name": _local_in(written, "name"),
                "print_ranges_written": value,
                "ranges": ranges,
                "range_total": len(ranges),
            })
            continue
        if local in ("named-range", "named-expression"):
            written = _written_attrs(one, nsmap)
            name = _local_in(written, "name") or ""
            named.append({
                "element": _written_name(one.tag, nsmap),
                "name": name,
                "built_in": name.startswith("Excel_BuiltIn_"),
                "base_cell_address": _local_in(written, "base-cell-address"),
                "cell_range_address": _local_in(written, "cell-range-address"),
                "expression": _local_in(written, "expression"),
                "usable_as": _local_in(written, "range-usable-as"),
                "written": written,
            })
    bases = sorted(set(one["base_cell_address"] for one in named if one["base_cell_address"]))
    usable: dict = {}
    for one in named:
        if one["usable_as"] is not None:
            usable[one["usable_as"]] = usable.get(one["usable_as"], 0) + 1
    return {
        "family": "odf",
        "available": True,
        "tables_total": len(tables),
        "with_print_ranges": sum(1 for one in tables if one["print_ranges_written"] is not None),
        "named_total": len(named),
        "built_in_total": sum(1 for one in named if one["built_in"]),
        "named_by_element": {
            "range": sum(1 for one in named if one["element"] == "table:named-range"),
            "expression": sum(1 for one in named if one["element"] == "table:named-expression"),
        },
        "distinct_base_addresses": len(bases),
        "usable_as_written": usable,
        "tables": tables[:limit],
        "named": named[:limit],
    }


def pptx_placeholder_hops(path: Path, limit: int = 100) -> dict:
    r"""「这框对应版式里哪一条」是**一跳**：页上的 `p:ph/@idx` 对版式里那条 `p:ph/@idx`

    实测这一跳在重写那份是**断的**：python-pptx 写 `idx="1"`，LibreOffice 重写时把正文
    占位符写成空元素 `<p:ph/>` —— 号没了，于是只能交「找不到」而不是按规范默认替它接上。
    版式自己的清单也整份抄下来（`type` 与 `idx` 都按写的），因为「这一页点的是哪份版式」
    本身要走页自己那张关系表（`.../_rels/slideN.xml.rels`，Type 结尾是 `slideLayout`）。
    """
    with zipfile.ZipFile(path) as box:
        names = [one.filename for one in box.infolist()]
        parts = {one.filename: box.read(one.filename) for one in box.infolist()}
    slides = sorted(one for one in names if one.startswith("ppt/slides/slide") and one.endswith(".xml"))
    rows = []
    for name in slides:
        stem = name[: name.rindex("/")]
        rel_name = f"{stem}/_rels/{name[name.rindex('/') + 1 :]}.rels"
        layout = None
        if rel_name in parts:
            rroot = ET.fromstring(parts[rel_name])
            for rel in rroot.iter():
                if not rel.tag.endswith("Relationship"):
                    continue
                if (rel.get("Type") or "").endswith("/slideLayout"):
                    raw = rel.get("Target") or ""
                    layout = "ppt/slideLayouts/" + raw.split("/")[-1]
                    break
        mine = []
        if layout in parts:
            lroot = ET.fromstring(parts[layout])
            for node in lroot.iter():
                if xml_local(node.tag) != "ph":
                    continue
                mine.append({
                    "type": node.get("type"),
                    "idx": node.get("idx"),
                    "sz": node.get("sz"),
                    "orient": node.get("orient"),
                })
        shape_rows = []
        for sp in [one for one in ET.fromstring(parts[name]).iter() if xml_local(one.tag) == "sp"]:
            holder = None
            for node in sp.iter():
                if xml_local(node.tag) == "ph":
                    holder = node
                    break
            nm = None
            for node in sp.iter():
                if xml_local(node.tag) == "cNvPr":
                    nm = node.get("name")
                    break
            wrote = holder is not None
            idx = holder.get("idx") if wrote else None
            kind = holder.get("type") if wrote else None
            matched = None
            if wrote:
                # 号写了就按号对；没写号才按名对（版式里也有不写 type 的那一条）
                for one in mine:
                    if idx is not None:
                        if one["idx"] == idx:
                            matched = one
                            break
                    elif one["idx"] is None and one["type"] == kind:
                        matched = one
                        break
            shape_rows.append({
                "name": nm,
                "ph_element": wrote,
                "type_written": kind,
                "idx_written": idx,
                "layout_matched": matched,
                "hop": ("by_idx" if idx is not None else "by_type") if wrote else "no_ph",
            })
        rows.append({
            "part": name,
            "layout_part": layout,
            "layout_found": layout in parts,
            "layout_placeholders": mine[:limit],
            "shapes": shape_rows[:limit],
        })
    total = sum(len(one["shapes"]) for one in rows)
    return {
        "family": "ooxml",
        "available": True,
        "slides": rows[:limit],
        "slide_total": len(rows),
        "shape_total": total,
        "with_ph": sum(1 for one in rows for s in one["shapes"] if s["ph_element"]),
        "hop_found": sum(1 for one in rows for s in one["shapes"] if s["layout_matched"] is not None),
        "hop_missing": sum(1 for one in rows for s in one["shapes"]
                           if s["ph_element"] and s["layout_matched"] is None),
        "no_idx_written": sum(1 for one in rows for s in one["shapes"]
                              if s["ph_element"] and s["idx_written"] is None),
    }


def docx_repeat_headers(path: Path, limit: int = 100) -> dict:
    r"""「这张表的哪几行每页重复」在 OOXML 是**行上**的一个无值元素 `w:trPr/w:tblHeader`

    元素在场就是重复，没有值可写，所以只交在场与否；哪一行标了就交哪一行（按行的先后）。
    另把「标了但不是从第一行起」单数一本：那是 Word 允许、LibreOffice 不认的一种写法，
    实测重写会把这种标记整个丢掉 —— 所以这份账只说文件写了什么，不说打印时会怎样。
    """
    with zipfile.ZipFile(path) as box:
        have = set(one.filename for one in box.infolist())
        if "word/document.xml" not in have:
            return {"available": False}
        raw = box.read("word/document.xml")
    root = ET.fromstring(raw)
    tables = []
    for index, table in enumerate([one for one in root.iter() if xml_local(one.tag) == "tbl"]):
        rows = [one for one in table if xml_local(one.tag) == "tr"]
        marks = []
        for row in rows:
            holder = None
            for kid in row:
                if xml_local(kid.tag) == "trPr":
                    holder = kid
                    break
            marks.append(bool(holder is not None
                               and any(xml_local(kid.tag) == "tblHeader" for kid in holder)))
        leading = marks and marks[0]
        tables.append({
            "index": index,
            "rows": len(rows),
            "header_rows": marks,
            "header_count": sum(1 for one in marks if one),
            "tr_pr_elements": sum(
                1 for row in rows
                for kid in row if xml_local(kid.tag) == "trPr"
            ),
            "contiguous_from_first": bool(leading and all(
                marks[i] >= marks[i + 1] for i in range(len(marks) - 1)
            )),
        })
    return {
        "family": "ooxml",
        "available": True,
        "tables_total": len(tables),
        "marked": sum(1 for one in tables if one["header_count"]),
        "header_rows_total": sum(one["header_count"] for one in tables),
        "non_leading": sum(1 for one in tables
                           if one["header_count"] and not (one["header_rows"] and one["header_rows"][0])),
        "tables": tables[:limit],
    }


def odt_repeat_headers(path: Path, limit: int = 100) -> dict:
    r"""同一问在 ODF 写在**表身上**：`table:header-rows` 配 `table:header-rows-repeated`

    一个是「几行算表头」，另一个是「每页重复几行」，两个都是数 —— 与 OOXML 那种一行一个
    标记的形状不同，所以两家各交各的，不折算。实测最要紧的一条：LibreOffice 把带
    `w:tblHeader` 的 docx 转成 odt 时，**这两个属性一个都不写**（这一格整个不见了），
    所以这里交 null 而不是 0：null 是「这份文件没说」，0 才是「它说了不重复」。
    """
    with zipfile.ZipFile(path) as box:
        have = set(one.filename for one in box.infolist())
        if "content.xml" not in have:
            return {"available": False}
        raw = box.read("content.xml")
    root = ET.fromstring(raw)
    nsmap = _ns_prefixes(raw)
    tables = []
    for index, one in enumerate([kid for kid in root.iter() if xml_local(kid.tag) == "table"]):
        written = _written_attrs(one, nsmap)
        tables.append({
            "index": index,
            "name": _local_in(written, "name"),
            "header_rows": _local_in(written, "header-rows"),
            "header_rows_repeated": _local_in(written, "header-rows-repeated"),
            "header_column": _local_in(written, "header-column"),
            "header_columns_repeated": _local_in(written, "header-columns-repeated"),
        })
    return {
        "family": "odf",
        "available": True,
        "tables_total": len(tables),
        "with_header_rows": sum(1 for one in tables if one["header_rows"] is not None),
        "with_repeated": sum(1 for one in tables if one["header_rows_repeated"] is not None),
        "tables": tables[:limit],
    }



def _tab_stop_written(node, nsmap) -> dict:
    """一条制表位定义：属性按文件写的局部名交出去，没写的键留 null"""
    written = _written_attrs(node, nsmap)
    return {
        "position": _local_in(written, "position"),
        "type": _local_in(written, "type"),
        "char": _local_in(written, "char"),
        "leader_style": _local_in(written, "leader-style"),
        "leader_text": _local_in(written, "leader-text"),
    }


def _stops_under(props, nsmap) -> list:
    """一份 `style:paragraph-properties` 里的制表位（可能与本问无关的别的属性不碰）"""
    out = []
    for group in props:
        if xml_local(group.tag) != "tab-stops":
            continue
        for tab in group:
            if xml_local(tab.tag) == "tab-stop":
                out.append(_tab_stop_written(tab, nsmap))
    return out


def docx_tab_stops(path: Path, limit: int = 100) -> dict:
    r"""「这一段上有哪几个制表位」在 OOXML 写在**段上**：`w:pPr/w:tabs/w:tab`

    三个属性 `w:pos`（twip）/ `w:val`（对齐）/ `w:leader`（引导符）都按写的交：没写 `w:val`
    不等于「左对齐」，那是规范默认值而不是这份文件说的话。另数一本「这一段里有几个制表**字符**」
    —— 与定义同名（run 里的 `w:tab`），全局数一遍就会把定义当成字符。
    """
    with zipfile.ZipFile(path) as box:
        have = set(one.filename for one in box.infolist())
        if "word/document.xml" not in have:
            return {"available": False}
        raw = box.read("word/document.xml")
    root = ET.fromstring(raw)
    body = [one for one in root if xml_local(one.tag) == "body"]
    if not body:
        return {"available": False}
    rows = []
    vals, leaders, positions = {}, {}, []
    stops_total = without_val = without_leader = chars_total = 0
    for index, para in enumerate([one for one in body[0].iter() if xml_local(one.tag) == "p"]):
        holder = None
        for kid in para:
            if xml_local(kid.tag) == "pPr":
                holder = kid
                break
        stops = []
        if holder is not None:
            for group in holder:
                if xml_local(group.tag) != "tabs":
                    continue
                for tab in group:
                    if xml_local(tab.tag) != "tab":
                        continue
                    written = _written_attrs(tab, {})
                    stops.append({
                        "pos_written": _local_in(written, "pos"),
                        "val": _local_in(written, "val"),
                        "leader": _local_in(written, "leader"),
                    })
        chars = 0
        for run in [one for one in para.iter() if xml_local(one.tag) == "r"]:
            chars += sum(1 for kid in run if xml_local(kid.tag) == "tab")
        chars_total += chars
        stops_total += len(stops)
        for one in stops:
            if one["val"] is None:
                without_val += 1
            else:
                vals[one["val"]] = vals.get(one["val"], 0) + 1
            if one["leader"] is None:
                without_leader += 1
            else:
                leaders[one["leader"]] = leaders.get(one["leader"], 0) + 1
            if one["pos_written"] is not None and one["pos_written"] not in positions:
                positions.append(one["pos_written"])
        rows.append({"index": index, "stops": stops, "tab_chars": chars})
    return {
        "family": "ooxml",
        "available": True,
        "paragraphs_total": len(rows),
        "with_stops": sum(1 for one in rows if one["stops"]),
        "stops_total": stops_total,
        "tab_chars_total": chars_total,
        "stops_without_val": without_val,
        "stops_without_leader": without_leader,
        "vals_written": vals,
        "leaders_written": leaders,
        "distinct_positions": positions,
        "paragraphs": rows[:limit],
    }


def _odf_paragraph_styles(roots: list, nsmaps: list) -> list:
    """两份件里所有**有名字的**段落样式：(名字, 那份样式写的制表位, 写的父样式名)"""
    out = []
    for root, nsmap in zip(roots, nsmaps):
        for node in root.iter():
            if xml_local(node.tag) != "style":
                continue
            written = _written_attrs(node, nsmap)
            if _local_in(written, "family") != "paragraph":
                continue
            name = _local_in(written, "name")
            if name is None:
                continue
            stops = []
            for props in node:
                if xml_local(props.tag) == "paragraph-properties":
                    stops.extend(_stops_under(props, nsmap))
            out.append((name, stops, _local_in(written, "parent-style-name")))
    return out


def odf_tab_stops(path: Path, limit: int = 100) -> dict:
    r"""同一问在 ODF 一跳之外：段只点一个样式名，制表位在那份样式的段落属性里

    `style:position` 是带单位的串（同一条 9cm 在 docx 是 5102 twip，而 LibreOffice 转过来
    写成 **8.999cm** —— 换算的账不归读者平），`style:type` 没写是「左」而这份件没说，
    「引导符」是 `leader-style` 与 `leader-text` **两个**属性合起来的。
    `style:default-style` 没有名字可点却照样落到每一段上，所以另交一份。
    """
    with zipfile.ZipFile(path) as box:
        have = set(one.filename for one in box.infolist())
        if "content.xml" not in have:
            return {"available": False}
        content_raw = box.read("content.xml")
        crowd = ET.fromstring(content_raw)
        roots = [crowd]
        nsmaps = [_ns_prefixes(content_raw)]
        if "styles.xml" in have:
            styles_raw = box.read("styles.xml")
            roots.append(ET.fromstring(styles_raw))
            nsmaps.append(_ns_prefixes(styles_raw))
    table = _odf_paragraph_styles(roots, nsmaps)
    defaults = []
    for root, nsmap in zip(roots, nsmaps):
        for node in root.iter():
            if xml_local(node.tag) != "default-style":
                continue
            if _local_in(_written_attrs(node, nsmap), "family") != "paragraph":
                continue
            for props in node:
                if xml_local(props.tag) == "paragraph-properties":
                    defaults.extend(_stops_under(props, nsmap))
    rows = []
    types, leader_styles, positions = {}, {}, []
    stops_total = without_type = chars_total = 0
    pointed = []
    for index, para in enumerate([one for one in crowd.iter() if xml_local(one.tag) == "p"]):
        written = _written_attrs(para, nsmaps[0])
        name = _local_in(written, "style-name")
        found = None
        if name is not None:
            for had in table:
                if had[0] == name:
                    found = had
                    break
        stops = found[1] if found else []
        if name is not None and stops and name not in pointed:
            pointed.append(name)
        chars = sum(1 for one in para.iter() if xml_local(one.tag) == "tab")
        chars_total += chars
        stops_total += len(stops)
        for one in stops:
            if one["type"] is None:
                without_type += 1
            else:
                types[one["type"]] = types.get(one["type"], 0) + 1
            if one["leader_style"] is not None:
                leader_styles[one["leader_style"]] = (
                    leader_styles.get(one["leader_style"], 0) + 1
                )
            if one["position"] is not None and one["position"] not in positions:
                positions.append(one["position"])
        rows.append({
            "index": index,
            "style_written": name,
            "style_found": found is not None,
            "parent_style_written": found[2] if found else None,
            "stops": stops,
            "tab_chars": chars,
        })
    return {
        "family": "odf",
        "available": True,
        "paragraphs_total": len(rows),
        "with_stops": sum(1 for one in rows if one["stops"]),
        "stops_total": stops_total,
        "tab_chars_total": chars_total,
        "styles_total": len(table),
        "styles_with_stops": sum(1 for had in table if had[1]),
        "unpointed_styles": [had[0] for had in table if had[1] and had[0] not in pointed],
        "default_style_stops": defaults,
        "stops_without_type": without_type,
        "types_written": types,
        "leader_styles_written": leader_styles,
        "distinct_positions": positions,
        "paragraphs": rows[:limit],
    }


def docx_section_starts(path: Path, limit: int = 100) -> dict:
    r"""「这一节是从哪儿开始的」：`w:sectPr/w:type` 在不在、写了哪个值

    实测两份件：python-docx 那份把第一节设成「另起一页」之后**根本没有写 `w:type`**
    （那是 Word 的默认值），而 LibreOffice 重写同一份时把它写成 `<w:type w:val="nextPage"/>` ——
    「没说」与「说了默认」是两件事，所以 `element_present` 与 `type_written` 分两格交。
    `continuous` / `evenPage` 两个值往返一字未变：这一族生产者改的是「说没说」，不是说什么。
    """
    rows = []
    types = []
    with zipfile.ZipFile(path) as box:
        if "word/document.xml" not in box.namelist():
            return {"available": False}
        root = ET.fromstring(box.read("word/document.xml"))
    for index, sect in enumerate([one for one in root.iter() if xml_local(one.tag) == "sectPr"]):
        holders = [one for one in sect.iter() if xml_local(one.tag) == "type"]
        holders = [one for one in holders if one is not sect]
        if not holders:
            rows.append({
                "section": index,
                "element_present": False,
                "type_written": None,
                "written": {},
            })
            continue
        attrs = _written_attrs(holders[0], {})
        value = _local_in(attrs, "val")
        if value is not None and value not in types:
            types.append(value)
        rows.append({
            "section": index,
            "element_present": True,
            "type_written": value,
            "written": attrs,
        })
    return {
        "family": "ooxml",
        "available": True,
        "sections_total": len(rows),
        "with_element": len([one for one in rows if one["element_present"]]),
        "type_missing": len([one for one in rows if one["type_written"] is None]),
        "distinct_types": types,
        "sections": rows[:limit],
    }


def docx_comment_ledger(path: Path, limit: int = 100) -> dict:
    r"""批注那一份账（OOXML）：内容在 `word/comments.xml`，锚点在正文里，两边按 `w:id` 配

    三条要紧的都在真件里：部件里那几条的**先后**与正文锚点的先后不是一套（LibreOffice 重写
    同一份把部件排成 1,0,2 而正文一字没动），`w:date` 带 Z 而 ODF 那份不带，
    以及「有内容没锚点」与「有锚点没内容」是两个方向都要数的。
    """
    with zipfile.ZipFile(path) as box:
        have = set(one.filename for one in box.infolist())
        if "word/document.xml" not in have:
            return {"available": False}
        root = ET.fromstring(box.read("word/document.xml"))
        from_part = []
        part_written = "word/comments.xml" in have
        if part_written:
            croot = ET.fromstring(box.read("word/comments.xml"))
            for index, one in enumerate([k for k in croot.iter() if xml_local(k.tag) == "comment"]):
                written = _written_attrs(one, {})
                paras = [k for k in one.iter() if xml_local(k.tag) == "p"]
                from_part.append({
                    "part_index": index,
                    "id": _local_in(written, "id"),
                    "author": _local_in(written, "author"),
                    "initials": _local_in(written, "initials"),
                    "date": _local_in(written, "date"),
                    "paragraphs": len(paras),
                    "text": "\n".join(ooxml_para_text(k) for k in paras),
                })
    body = [one for one in root if xml_local(one.tag) == "body"]
    starts, ends, refs, host = [], [], [], []
    paras = [one for one in body[0].iter() if xml_local(one.tag) == "p"] if body else []
    for index, para in enumerate(paras):
        here = []
        for kind, sink in (("commentReference", refs), ("commentRangeStart", starts),
                           ("commentRangeEnd", ends)):
            for had in [k for k in para.iter() if xml_local(k.tag) == kind]:
                value = _local_in(_written_attrs(had, {}), "id")
                if value is None:
                    continue
                sink.append(value)
                if kind == "commentReference":
                    here.append(value)
        if here:
            host.append({"paragraph": index, "ids": here})
    orphans = [one for one in from_part if one["id"] not in refs]
    dangling = []
    for value in refs:
        if value not in [one["id"] for one in from_part] and value not in dangling:
            dangling.append(value)
    authors = []
    for one in from_part:
        if one["author"] is not None and one["author"] not in authors:
            authors.append(one["author"])
    return {
        "family": "ooxml",
        "available": True,
        "part_written": part_written,
        "comments_total": len(from_part),
        "anchor_starts": len(starts),
        "anchor_ends": len(ends),
        "anchor_references": len(refs),
        "range_asymmetric": len(starts) != len(ends),
        "orphans_without_anchor": len(orphans),
        "anchors_without_comment": len(dangling),
        "distinct_authors": authors,
        "comments": from_part[:limit],
        "hosts": host[:limit],
    }


def docx_comment_threads(path: Path, limit: int = 100) -> dict:
    r"""批注的「谁回复谁」与「结没结」：值不住在 `w:comment` 上，在另外两份部件里，靠段号连

    四条实测（`crep.docx` 手写的正例、`crep-lo.docx` 与 `crep-r.docx` 两个 LibreOffice 方向）：
    `w15:commentEx/@w15:paraId` 指的是**批注体内那一段的 `w14:paraId`**，不是元素自己的 `w:id`，
    所以这是两跳；回复是 `@w15:paraIdParent`（又一个段号）；`@w15:done` 才是「结没结」。
    LibreOffice 的 **docx→docx** 把两份部件整个不写（连 `w14:paraId` 也一并没了），
    而它的 **odt→docx** 那一路会写 `w15:done="1"` —— 且只对已解决那一条写记录，
    没解决的那一条**记录整个不存在**：「没写」与「写了 0」是两件事，各交各的。
    第三份部件 `word/commentsIds.xml` 又是第四个号（`w16cid:durableId`），同样按段号连。
    """
    with zipfile.ZipFile(path) as box:
        have = set(one.filename for one in box.infolist())
        comments = []
        if "word/comments.xml" in have:
            croot = ET.fromstring(box.read("word/comments.xml"))
            comments = [k for k in croot.iter() if xml_local(k.tag) == "comment"]
        ext = []
        if "word/commentsExtended.xml" in have:
            eroot = ET.fromstring(box.read("word/commentsExtended.xml"))
            ext = [k for k in eroot.iter() if xml_local(k.tag) == "commentEx"]
        ids = []
        if "word/commentsIds.xml" in have:
            iroot = ET.fromstring(box.read("word/commentsIds.xml"))
            ids = [k for k in iroot.iter() if xml_local(k.tag) == "commentId"]

    def of(node, want):
        if node is None:
            return None
        for key, value in node.attrib.items():
            if xml_local(key) == want:
                return value
        return None

    ext_by_pid = {}
    for one in ext:
        pid = of(one, "paraId")
        if pid is not None and pid not in ext_by_pid:
            ext_by_pid[pid] = one
    ids_by_pid = {}
    for one in ids:
        pid = of(one, "paraId")
        if pid is not None and pid not in ids_by_pid:
            ids_by_pid[pid] = one

    rows = []
    para_owner = {}
    for index, one in enumerate(comments):
        paras = [k for k in one.iter() if xml_local(k.tag) == "p"]
        pid = of(paras[0], "paraId") if paras else None
        if pid is not None and pid not in para_owner:
            para_owner[pid] = index
        rows.append({
            "index": index,
            "id": of(one, "id"),
            "author": of(one, "author"),
            "para_id": pid,
            "ex_found": False,
            "done_written": None,
            "done": None,
            "parent_para_id": None,
            "replies_to": None,
            "durable_id": None,
        })
    matched_ext = set()
    matched_ids = set()
    for row in rows:
        pid = row["para_id"]
        had = ext_by_pid.get(pid) if pid is not None else None
        if had is not None:
            matched_ext.add(pid)
            row["ex_found"] = True
            done = of(had, "done")
            row["done_written"] = done
            row["done"] = (done == "1") if done is not None else None
            row["parent_para_id"] = of(had, "paraIdParent")
        cid = ids_by_pid.get(pid) if pid is not None else None
        if cid is not None:
            matched_ids.add(pid)
            row["durable_id"] = of(cid, "durableId")
    for row in rows:
        parent = row["parent_para_id"]
        if parent is not None:
            row["replies_to"] = para_owner.get(parent)
    replies = [one for one in rows if one["parent_para_id"] is not None]
    return {
        "family": "ooxml",
        "available": True,
        "comments_total": len(rows),
        "paras_with_para_id": len([one for one in rows if one["para_id"] is not None]),
        "ext_part_written": bool(ext) or "word/commentsExtended.xml" in have,
        "ext_total": len(ext),
        "ext_matched": len(matched_ext),
        "ext_orphans": len(ext) - len(matched_ext),
        "done_written_total": len([one for one in rows if one["done_written"] is not None]),
        "done_true": len([one for one in rows if one["done"] is True]),
        "done_false": len([one for one in rows if one["done"] is False]),
        "ex_without_done": len([one for one in rows if one["ex_found"] and one["done_written"] is None]),
        "replies_total": len(replies),
        "replies_dangling": len([one for one in replies if one["replies_to"] is None]),
        "ids_part_written": "word/commentsIds.xml" in have,
        "ids_total": len(ids),
        "ids_matched": len(matched_ids),
        "ids_orphans": len(ids) - len(matched_ids),
        "threads": rows[:limit],
    }


def odf_comment_threads(path: Path, limit: int = 100) -> dict:
    r"""同一问在 ODF：「结没结」直接压在注自己身上（`loext:resolved`），而回复没有位置

    实测：LibreOffice 导出的 odt / odp 每条 `office:annotation` 都写 `loext:resolved="false"`，
    **而 .ods 的格子注一条都不写**（`cell-notes.ods` 三条全无）—— 所以「写了 false」与
    「没写这个属性」是两个答案。回复那一路这一族没有任何对应物：`crep.odt` 里
    `reply` / `thread` / `parent` 一个词都没有，所以本账**不交回复那一格**（不是 0）。
    两份件都要走：注可以坐在 content.xml 也可以坐在 styles.xml。
    """
    rows = []
    parts_seen = []
    with zipfile.ZipFile(path) as box:
        for part in ("content.xml", "styles.xml"):
            if part not in box.namelist():
                continue
            raw = box.read(part)
            try:
                root = ET.fromstring(raw)
            except ET.ParseError:
                continue
            found_here = False
            for one in root.iter():
                if xml_local(one.tag) != "annotation":
                    continue
                found_here = True
                written = None
                for key, value in one.attrib.items():
                    if xml_local(key) == "resolved":
                        written = value
                name = None
                for key, value in one.attrib.items():
                    if xml_local(key) == "name":
                        name = value
                rows.append({
                    "index": len(rows),
                    "part": part,
                    "name_written": name,
                    "resolved_written": written,
                    "resolved": (written == "true") if written is not None else None,
                })
            if found_here and part not in parts_seen:
                parts_seen.append(part)
    return {
        "family": "odf",
        "available": True,
        "annotations_total": len(rows),
        "parts_seen": parts_seen,
        "with_resolved_written": len([one for one in rows if one["resolved_written"] is not None]),
        "resolved_true": len([one for one in rows if one["resolved"] is True]),
        "resolved_false": len([one for one in rows if one["resolved"] is False]),
        "without_resolved": len([one for one in rows if one["resolved_written"] is None]),
        "annotations": rows[:limit],
    }


def odf_comment_ledger(path: Path, limit: int = 100) -> dict:
    r"""同一问在 ODF 只有一处：`text:annotation` 坐在它所属的那一段里面

    作者是孩子元素 `dc:creator`、时间是 `dc:date`（**没有 Z**，与 OOXML 那种带 Z 的写法不同）。
    「几段」在这里是两个数：全文的 `text:p` 把批注里的那些也算进去了，所以正文段数另交一份。
    """
    with zipfile.ZipFile(path) as box:
        have = set(one.filename for one in box.infolist())
        if "content.xml" not in have:
            return {"available": False}
        crowd = ET.fromstring(box.read("content.xml"))
    paras = [one for one in crowd.iter() if xml_local(one.tag) == "p"]
    rows = []
    inner = 0
    for index, para in enumerate(paras):
        for one in [k for k in para.iter() if xml_local(k.tag) == "annotation"]:
            held = [k for k in one.iter() if xml_local(k.tag) == "p"]
            inner += len(held)
            creator = [k for k in one if xml_local(k.tag) == "creator"]
            stamp = [k for k in one if xml_local(k.tag) == "date"]
            rows.append({
                "order": len(rows),
                "host_paragraph": index,
                "creator": "".join(creator[0].itertext()).strip() if creator else None,
                "date": "".join(stamp[0].itertext()).strip() if stamp else None,
                "paragraphs": len(held),
                "text": "\n".join("".join(had.itertext()).strip() for had in held),
            })
    creators = []
    for one in rows:
        if one["creator"] is not None and one["creator"] not in creators:
            creators.append(one["creator"])
    return {
        "family": "odf",
        "available": True,
        "annotations_total": len(rows),
        "paragraphs_total": len(paras),
        "paragraphs_in_annotations": inner,
        "paragraphs_body_only": len(paras) - inner,
        "hosted_in": len({one["host_paragraph"] for one in rows}),
        "distinct_creators": creators,
        "annotations": rows[:limit],
    }


KEEP_NAMES = (("keepNext", "keep_next"), ("keepLines", "keep_lines"),
              ("pageBreakBefore", "page_break_before"), ("widowControl", "widow_control"))
ODF_KEEP_WORDS = (("keep-with-next", "keep_with_next"), ("keep-together", "keep_together"),
                  ("break-before", "break_before"), ("widows", "widows"),
                  ("orphans", "orphans"))
OFF_WORDS = ("0", "false", "off", "none")


def _switch(present: bool, raw) -> dict:
    """一个开关的三种状态：元素在不在、文件给没给值、按给的词算开着还是关着"""
    off = raw in OFF_WORDS
    return {"present": present, "val": raw,
            "on_written": bool(present and not off), "off_written": bool(present and off)}


def docx_keep_switches(path: Path, limit: int = 100) -> dict:
    r"""「这一段与下一页的关系」那四个开关（OOXML）：`w:pPr` 下面的四个元素

    前三个没有值就是开着（实测 python-docx 写出来是空元素），而 `w:widowControl` 这一族常反过来
    写 `w:val="0"` 表示关掉 —— 所以「在场」与「开着」不是一回事，`present` 与 `val` 两样都交。
    """
    with zipfile.ZipFile(path) as box:
        have = set(one.filename for one in box.infolist())
        if "word/document.xml" not in have:
            return {"available": False}
        root = ET.fromstring(box.read("word/document.xml"))
    body = [one for one in root if xml_local(one.tag) == "body"]
    paras = [one for one in body[0].iter() if xml_local(one.tag) == "p"] if body else []
    rows, indexed, states, p_pr = [], [], {}, 0
    for index, para in enumerate(paras):
        holder = None
        for kid in para:
            if xml_local(kid.tag) == "pPr":
                holder = kid
                break
        if holder is not None:
            p_pr += 1
        mine = {"index": index, "has_pPr": holder is not None}
        touched = False
        for name, key in KEEP_NAMES:
            found = None
            if holder is not None:
                for kid in holder:
                    if xml_local(kid.tag) == name:
                        found = kid
                        break
            raw = None
            if found is not None:
                raw = _local_in(_written_attrs(found, {}), "val")
            one = _switch(found is not None, raw)
            mine[key] = one
            if found is not None:
                touched = True
                grouped = name + (" bare" if raw is None else " with_value")
                states[grouped] = states.get(grouped, 0) + 1
        if touched:
            indexed.append(index)
        rows.append(mine)
    return {
        "family": "ooxml",
        "available": True,
        "paragraphs_total": len(rows),
        "p_pr_elements": p_pr,
        "paragraphs_with_any": len(indexed),
        "paragraphs_indexed": indexed,
        "states_written": states,
        "paragraphs": rows[:limit],
    }


def odf_keep_switches(path: Path, limit: int = 100) -> dict:
    r"""同一问在 ODF 一跳在样式里，而且**孤行控制是两个数**

    `fo:keep-with-next` / `fo:keep-together` / `fo:break-before` 是一个词，而关掉孤行控制写成
    `fo:widows="0"` 配 `fo:orphans="0"` —— 与 OOXML 那一枚 `w:widowControl w:val="0"` 两种形状。
    """
    with zipfile.ZipFile(path) as box:
        have = set(one.filename for one in box.infolist())
        if "content.xml" not in have:
            return {"available": False}
        content_raw = box.read("content.xml")
        crowd = ET.fromstring(content_raw)
        roots = [crowd]
        nsmaps = [_ns_prefixes(content_raw)]
        if "styles.xml" in have:
            styles_raw = box.read("styles.xml")
            roots.append(ET.fromstring(styles_raw))
            nsmaps.append(_ns_prefixes(styles_raw))
    table = []
    for root, nsmap in zip(roots, nsmaps):
        for node in root.iter():
            if xml_local(node.tag) != "style":
                continue
            written = _written_attrs(node, nsmap)
            if _local_in(written, "family") != "paragraph":
                continue
            name = _local_in(written, "name")
            if name is None:
                continue
            held = []
            for props in node:
                if xml_local(props.tag) != "paragraph-properties":
                    continue
                pw = _written_attrs(props, nsmap)
                for key, _want in ODF_KEEP_WORDS:
                    raw = _local_in(pw, key)
                    if raw is not None:
                        held.append((key, raw))
            table.append((name, held))
    rows, words, resolved, with_any = [], {}, 0, 0
    paras = [one for one in crowd.iter() if xml_local(one.tag) == "p"]
    for index, para in enumerate(paras):
        name = _local_in(_written_attrs(para, nsmaps[0]), "style-name")
        found = None
        if name is not None:
            for had in table:
                if had[0] == name:
                    found = had
                    break
        held = found[1] if found else []
        mine = {"index": index, "style_written": name, "style_found": found is not None}
        for key, want in ODF_KEEP_WORDS:
            raw = None
            for had in held:
                if had[0] == key:
                    raw = had[1]
                    break
            mine[want] = {"written": raw, "present": raw is not None}
            if raw is not None:
                grouped = "%s=%s" % (key, raw)
                words[grouped] = words.get(grouped, 0) + 1
        if held:
            with_any += 1
        if found is not None:
            resolved += 1
        rows.append(mine)
    return {
        "family": "odf",
        "available": True,
        "paragraphs_total": len(rows),
        "resolved": resolved,
        "paragraphs_with_any": with_any,
        "styles_total": len(table),
        "words_written": words,
        "paragraphs": rows[:limit],
    }



def docx_table_styles(path: Path, limit: int = 100) -> dict:
    r"""「这张表套的是哪个样式」在 OOXML 是两样东西：样式 id（`w:tblStyle`）与
    那枚 `w:tblLook`（六个位 + 一个 `w:val` 的十六进制缓存）

    两样可以不一致：改了位而缓存没重算是真件里就有的（python-docx 那份），
    LibreOffice 重写同一份时会把它重算 —— 所以两边都按写的交，不合并、不判谁对。
    """
    with zipfile.ZipFile(path) as box:
        have = set(one.filename for one in box.infolist())
        if "word/document.xml" not in have:
            return {"available": False}
        root = ET.fromstring(box.read("word/document.xml"))
    body = [one for one in root if xml_local(one.tag) == "body"]
    rows, styles, with_style, with_look = [], [], 0, 0
    for index, table in enumerate([one for one in (body[0].iter() if body else [])
                                   if xml_local(one.tag) == "tbl"]):
        holder = [kid for kid in table if xml_local(kid.tag) == "tblPr"]
        named = [k for k in holder[0] if xml_local(k.tag) == "tblStyle"] if holder else []
        style_written = None
        if named:
            style_written = _local_in(_written_attrs(named[0], {}), "val")
        look = [k for k in holder[0] if xml_local(k.tag) == "tblLook"] if holder else []
        written = {}
        if look:
            written = {k.split(":")[-1]: v for k, v in _written_attrs(look[0], {}).items()
                       if not k.startswith("xmlns")}
        if style_written is not None:
            with_style += 1
            if style_written not in styles:
                styles.append(style_written)
        if look:
            with_look += 1
        rows.append({
            "index": index,
            "style_written": style_written,
            "has_tblPr": bool(holder),
            "has_tbl_look": bool(look),
            "look_written": written,
        })
    return {
        "family": "ooxml",
        "available": True,
        "tables_total": len(rows),
        "with_style_written": with_style,
        "with_look": with_look,
        "distinct_styles": styles,
        "tables": rows[:limit],
    }


def odf_table_styles(path: Path, limit: int = 100) -> dict:
    r"""同一问在 ODF 只有一个名字：`table:style-name` 点在一份 family=table 的样式上

    那枚 look 在这一族**不存在** —— 不交 0 也不交 null，就是这个键没有。转过来那份件里
    OOXML 的样式名整个不见了（四张表各点一份自动样式 `表格1`…），所以样式那一路的信息
    在这一转里丢了：交看到的，不补。
    """
    with zipfile.ZipFile(path) as box:
        have = set(one.filename for one in box.infolist())
        if "content.xml" not in have:
            return {"available": False}
        content_raw = box.read("content.xml")
        crowd = ET.fromstring(content_raw)
        roots = [crowd]
        nsmaps = [_ns_prefixes(content_raw)]
        if "styles.xml" in have:
            styles_raw = box.read("styles.xml")
            roots.append(ET.fromstring(styles_raw))
            nsmaps.append(_ns_prefixes(styles_raw))
    known = []
    parents = {}
    for root, nsmap in zip(roots, nsmaps):
        for node in root.iter():
            if xml_local(node.tag) != "style":
                continue
            written = _written_attrs(node, nsmap)
            if _local_in(written, "family") != "table":
                continue
            name = _local_in(written, "name")
            if name is None or name in known:
                continue
            known.append(name)
            parents[name] = _local_in(written, "parent-style-name")
    rows, found_count = [], 0
    for index, table in enumerate([one for one in crowd.iter() if xml_local(one.tag) == "table"]):
        written = _written_attrs(table, nsmaps[0])
        name = _local_in(written, "style-name")
        found = name is not None and name in known
        if found:
            found_count += 1
        rows.append({
            "index": index,
            "table_name": _local_in(written, "name"),
            "style_written": name,
            "style_found": found,
            "parent_style_written": parents.get(name) if found else None,
        })
    return {
        "family": "odf",
        "available": True,
        "tables_total": len(rows),
        "with_style_written": sum(1 for one in rows if one["style_written"] is not None),
        "style_found_total": found_count,
        "styles_total": len(known),
        "tables": rows[:limit],
    }


ODF_LINE_WORDS = ["line-height", "line-height-style", "line-height-inherit", "line-break"]


def docx_line_spacing(path: Path, limit: int = 100) -> dict:
    r"""「这一段的行距」在 OOXML 是 `w:pPr/w:spacing` 上的**两枚**属性

    `w:line` 那个数的单位由 `w:lineRule` 决定：`auto` 时是 1/240 倍（`360` = 1.5 倍），
    `exact` / `atLeast` 时是 twip（22 磅 = `440`）。实测同一份稿子里 1.5 倍与「至少 18 磅」
    的 `w:line` 都是 `360` —— 只交那一个数会把两种单位读成一种，所以两枚分开各自按写的交。
    """
    with zipfile.ZipFile(path) as box:
        have = set(one.filename for one in box.infolist())
        if "word/document.xml" not in have:
            return {"available": False}
        root = ET.fromstring(box.read("word/document.xml"))
    body = [one for one in root if xml_local(one.tag) == "body"]
    paras = [one for one in body[0].iter() if xml_local(one.tag) == "p"] if body else []
    rows, rules, with_line, with_rule, both = [], {}, 0, 0, 0
    for index, para in enumerate(paras):
        holder = None
        for kid in para:
            if xml_local(kid.tag) == "pPr":
                holder = kid
                break
        spacing = None
        if holder is not None:
            for kid in holder:
                if xml_local(kid.tag) == "spacing":
                    spacing = kid
                    break
        written = _written_attrs(spacing, {}) if spacing is not None else {}
        line = _local_in(written, "line") if spacing is not None else None
        rule = _local_in(written, "lineRule") if spacing is not None else None
        if line is not None:
            with_line += 1
        if rule is not None:
            with_rule += 1
            rules[rule] = rules.get(rule, 0) + 1
        if line is not None and rule is not None:
            both += 1
        rows.append({
            "index": index,
            "has_pPr": holder is not None,
            "has_spacing": spacing is not None,
            "line_written": line,
            "rule_written": rule,
        })
    return {
        "family": "ooxml",
        "available": True,
        "paragraphs_total": len(rows),
        "with_line_written": with_line,
        "with_rule_written": with_rule,
        "with_both": both,
        "rules_written": rules,
        "paragraphs": rows[:limit],
    }


def odf_line_spacing(path: Path, limit: int = 100) -> dict:
    r"""同一问在 ODF 一跳在段点的那份样式里，而**单位写在串上**

    倍数变成百分数（`150%`）、固定值变成长度（`0.776cm`），读者不换算、不约分。
    实测 `atLeast` 那一段转过来后**四个属性一个都没写** —— 那是转换丢的，不替它接。
    """
    with zipfile.ZipFile(path) as box:
        have = set(one.filename for one in box.infolist())
        if "content.xml" not in have:
            return {"available": False}
        content_raw = box.read("content.xml")
        crowd = ET.fromstring(content_raw)
        roots = [crowd]
        nsmaps = [_ns_prefixes(content_raw)]
        if "styles.xml" in have:
            styles_raw = box.read("styles.xml")
            roots.append(ET.fromstring(styles_raw))
            nsmaps.append(_ns_prefixes(styles_raw))
    table = []
    for root, nsmap in zip(roots, nsmaps):
        for node in root.iter():
            if xml_local(node.tag) != "style":
                continue
            written = _written_attrs(node, nsmap)
            if _local_in(written, "family") != "paragraph":
                continue
            name = _local_in(written, "name")
            if name is None:
                continue
            held = []
            for props in node:
                if xml_local(props.tag) != "paragraph-properties":
                    continue
                pw = _written_attrs(props, nsmap)
                for key in ODF_LINE_WORDS:
                    raw = _local_in(pw, key)
                    if raw is not None:
                        held.append((key, raw))
            table.append((name, held))
    rows, forms, with_height = [], {}, 0
    paras = [one for one in crowd.iter() if xml_local(one.tag) == "p"]
    for index, para in enumerate(paras):
        name = _local_in(_written_attrs(para, nsmaps[0]), "style-name")
        found = None
        if name is not None:
            for had in table:
                if had[0] == name:
                    found = had
                    break
        held = found[1] if found else []
        height = None
        for had in held:
            if had[0] == "line-height":
                height = had[1]
        mine = {"index": index, "style_written": name, "line_height_written": height}
        if height is not None:
            with_height += 1
            # 单位是写在串上的：只按「数字之后的那一串」分类，不换算、不约分
            at = 0
            while at < len(height) and (height[at] in "0123456789.-"):
                at += 1
            key = height[at:] if height[at:] else "无单位"
            forms[key] = forms.get(key, 0) + 1
        for key in ODF_LINE_WORDS[1:]:
            raw = None
            for had in held:
                if had[0] == key:
                    raw = had[1]
            mine[key.replace("-", "_")] = raw
        rows.append(mine)
    return {
        "family": "odf",
        "available": True,
        "paragraphs_total": len(rows),
        "with_line_height": with_height,
        "styles_total": len(table),
        "unit_forms": forms,
        "paragraphs": rows[:limit],
    }


ODF_BOX_LOCALS = ("border", "background-color", "padding")


def _docx_local_attrs(node) -> dict:
    """OOXML 那一族按**局部名**收属性（前缀是文件自己声明的）；与 Rust 的 local_attrs 同一条"""
    return {key.split(":")[-1]: value
            for key, value in _written_attrs(node, {}).items() if not key.startswith("xmlns")}


def _odf_box_attrs(props, nsmap: dict) -> list:
    """一份 `style:paragraph-properties` 里与「框与底」有关的那几条（名字按文件写的，前缀留着）"""
    out = []
    for key, value in _written_attrs(props, nsmap).items():
        if key == "xmlns" or key.startswith("xmlns:"):
            continue
        local = key.rsplit(":", 1)[-1]
        if local in ODF_BOX_LOCALS or local.startswith("border-") or local.startswith("padding-"):
            out.append((key, value))
    return out


def docx_para_borders(path: Path, limit: int = 100) -> dict:
    r"""「这一段自己有没有说画个框、铺个底」在 OOXML 是 `w:pPr` 下的**两枚元素**

    `w:pBdr` 是装边的壳（里面 `w:top`… 各带 `val` / `sz` / `space` / `color`，`sz` 是 1/8 磅），
    `w:shd` 是底纹。壳可以在而里面一条边都没写 —— 那是文件说过的话，不能读成「没写边框」，
    所以 `border_element` 与 `edge_count` 分开数。
    """
    with zipfile.ZipFile(path) as box:
        have = set(one.filename for one in box.infolist())
        if "word/document.xml" not in have:
            return {"available": False}
        root = ET.fromstring(box.read("word/document.xml"))
    body = [one for one in root if xml_local(one.tag) == "body"]
    paras = [one for one in body[0].iter() if xml_local(one.tag) == "p"] if body else []
    rows, vals, fills = [], {}, []
    p_pr = with_border = empty_border = edges_total = with_shading = 0
    for index, para in enumerate(paras):
        holder = None
        for kid in para:
            if xml_local(kid.tag) == "pPr":
                holder = kid
                break
        border = shading = None
        if holder is not None:
            for kid in holder:
                local = xml_local(kid.tag)
                if border is None and local == "pBdr":
                    border = kid
                if shading is None and local == "shd":
                    shading = kid
        edges = {}
        if border is not None:
            for kid in border:
                edges[xml_local(kid.tag)] = _docx_local_attrs(kid)
        if holder is not None:
            p_pr += 1
        if border is not None:
            with_border += 1
            if not len(border):
                empty_border += 1
        edges_total += len(edges)
        shading_attrs = _docx_local_attrs(shading) if shading is not None else None
        if shading is not None:
            with_shading += 1
            raw = shading_attrs.get("val")
            if raw is not None:
                vals[raw] = vals.get(raw, 0) + 1
            raw = shading_attrs.get("fill")
            if raw is not None and raw not in fills:
                fills.append(raw)
        rows.append({
            "index": index,
            "has_pPr": holder is not None,
            "border_element": border is not None,
            "edges": edges,
            "edge_count": len(edges),
            "shading_written": shading is not None,
            "shading": shading_attrs,
        })
    return {
        "family": "ooxml",
        "available": True,
        "paragraphs_total": len(rows),
        "p_pr_elements": p_pr,
        "with_border_element": with_border,
        "border_element_empty": empty_border,
        "edges_total": edges_total,
        "with_shading": with_shading,
        "shading_vals": vals,
        "distinct_fills": fills,
        "paragraphs": rows[:limit],
    }


def odf_para_borders(path: Path, limit: int = 100) -> dict:
    r"""同一问在 ODF 一跳在段点的那份样式上，而**一条 shorthand 顶四条边**

    `fo:border="0.74pt solid #ff0000"` 说的是四条边（值里塞着宽度、样式、颜色三段），也可以四条
    各写一遍 —— 而「这一边没有」在这一族是**明写着 `none`** 的，与 OOXML 那不写这一条边不是一回事。
    底纹是 `fo:background-color`，docx 那面 `w:space`（边离字多远）在这里搬成 `fo:padding`。
    """
    with zipfile.ZipFile(path) as box:
        have = set(one.filename for one in box.infolist())
        if "content.xml" not in have:
            return {"available": False}
        content_raw = box.read("content.xml")
        crowd = ET.fromstring(content_raw)
        roots = [crowd]
        nsmaps = [_ns_prefixes(content_raw)]
        if "styles.xml" in have:
            styles_raw = box.read("styles.xml")
            roots.append(ET.fromstring(styles_raw))
            nsmaps.append(_ns_prefixes(styles_raw))
    table = []
    for root, nsmap in zip(roots, nsmaps):
        for node in root.iter():
            if xml_local(node.tag) != "style":
                continue
            written = _written_attrs(node, nsmap)
            if _local_in(written, "family") != "paragraph":
                continue
            name = _local_in(written, "name")
            if name is None:
                continue
            held = []
            for props in node:
                if xml_local(props.tag) == "paragraph-properties":
                    held = _odf_box_attrs(props, nsmap)
                    break
            table.append((name, held))
    rows = []
    resolved = with_shorthand = with_side = sides_written = sides_none = with_background = 0
    paras = [one for one in crowd.iter() if xml_local(one.tag) == "p"]
    for index, para in enumerate(paras):
        name = _local_in(_written_attrs(para, nsmaps[0]), "style-name")
        hit = None
        if name is not None:
            for had in table:
                if had[0] == name:
                    hit = had
                    break
        if hit is not None:
            resolved += 1
        held = hit[1] if hit is not None else []
        shorthand = background = padding = None
        sides, line_widths = {}, {}
        for key, value in held:
            local = key.rsplit(":", 1)[-1]
            if local == "border":
                shorthand = value
            elif local == "background-color":
                background = value
            elif local == "padding":
                padding = value
            elif local.startswith("border-line-width"):
                line_widths[local[len("border-line-width"):].lstrip("-")] = value
            elif local.startswith("border-"):
                sides[key] = value
                sides_written += 1
                if value == "none":
                    sides_none += 1
            elif local.startswith("padding-"):
                sides[key] = value
        if shorthand is not None:
            with_shorthand += 1
        if sides:
            with_side += 1
        if background is not None:
            with_background += 1
        rows.append({
            "index": index,
            "style_written": name,
            "style_found": hit is not None,
            "border_shorthand": shorthand,
            "sides_written": sides,
            "background_written": background,
            "padding_written": padding,
            "line_widths": line_widths,
        })
    return {
        "family": "odf",
        "available": True,
        "paragraphs_total": len(rows),
        "styles_total": len(table),
        "style_found_total": resolved,
        "with_shorthand": with_shorthand,
        "with_side_elements": with_side,
        "sides_written": sides_written,
        "sides_none": sides_none,
        "with_background": with_background,
        "paragraphs": rows[:limit],
    }


def _collapsed_words(node) -> str:
    """子树里所有的字压成一行（与 Rust 的 inline_text 同一条：折空白、去首尾）"""
    return " ".join("".join(node.itertext()).split())


def _box_ledger(holder, want: str):
    """一个容器往下数：几份写字的格子、格子们带几段（直接孩子 / 整棵树）、说了什么"""
    boxes = [one for one in holder.iter() if xml_local(one.tag) == want]
    direct = anywhere = 0
    parts = []
    for one in boxes:
        direct += len([kid for kid in one if xml_local(kid.tag) == "p"])
        anywhere += len([x for x in one.iter() if xml_local(x.tag) == "p"])
        words = _collapsed_words(one)
        if words:
            parts.append(words)
    return len(boxes), direct, anywhere, parts


def _note_box_text(seen: list, words: str) -> None:
    if words and words not in seen:
        seen.append(words)


def docx_text_boxes(path: Path, limit: int = 100) -> dict:
    r"""「文档里有几个文本框、框里写了什么」在 OOXML 有两种容器

    LibreOffice 的 docx 导出把**同一个框写两份**：`w:drawing`（DrawingML，尺寸在 `wp:extent`
    的 EMU 上）与 `w:pict`（VML，尺寸在 `v:shape/@style` 那个 cm 串上），两份里各带一份
    `w:txbxContent`，字一模一样 —— 所以「几份格子」与「几句话」是两个数，合成一个就把同一句话
    读成两遍、或把两遍读成两个框。两类容器排不出同一个序，所以各走一条 list。
    """
    with zipfile.ZipFile(path) as box:
        have = set(one.filename for one in box.infolist())
        if "word/document.xml" not in have:
            return {"available": False}
        root = ET.fromstring(box.read("word/document.xml"))
    body = [one for one in root if xml_local(one.tag) == "body"]
    kids = body[0] if body else None
    rows = []
    boxes_total = drawings = picts = text_p = text_any = 0
    texts: list = []
    for index, holder in enumerate([x for x in kids.iter() if xml_local(x.tag) == "drawing"]
                                   if kids is not None else []):
        count, direct, anywhere, parts = _box_ledger(holder, "txbxContent")
        joined = " / ".join(parts)
        boxes_total += count
        text_p += direct
        text_any += anywhere
        if count > 0:
            drawings += 1
            _note_box_text(texts, joined)
        anchor = [x for x in holder.iter() if xml_local(x.tag) == "anchor"]
        inline = [x for x in holder.iter() if xml_local(x.tag) == "inline"]
        extent = [x for x in holder.iter() if xml_local(x.tag) == "extent"]
        rows.append({
            "kind": "drawing",
            "index": index,
            "text_boxes": count,
            "paragraphs_direct": direct,
            "paragraphs_anywhere": anywhere,
            "text": joined,
            "anchor_element": xml_local(anchor[0].tag) if anchor else None,
            "inline_element": xml_local(inline[0].tag) if inline else None,
            "extent_written": ({"cx": _local_in(_written_attrs(extent[0], {}), "cx"),
                                "cy": _local_in(_written_attrs(extent[0], {}), "cy")}
                               if extent else None),
        })
    for index, holder in enumerate([x for x in kids.iter() if xml_local(x.tag) == "pict"]
                                   if kids is not None else []):
        count, direct, anywhere, parts = _box_ledger(holder, "txbxContent")
        joined = " / ".join(parts)
        boxes_total += count
        text_p += direct
        text_any += anywhere
        if count > 0:
            picts += 1
            _note_box_text(texts, joined)
        shapes = [x for x in holder.iter() if xml_local(x.tag) == "shape"]
        rows.append({
            "kind": "pict",
            "index": index,
            "text_boxes": count,
            "paragraphs_direct": direct,
            "paragraphs_anywhere": anywhere,
            "text": joined,
            "textbox_elements": len([x for x in holder.iter() if xml_local(x.tag) == "textbox"]),
            "shape_style": (_local_in(_written_attrs(shapes[0], {}), "style") if shapes else None),
        })
    return {
        "family": "ooxml",
        "available": True,
        "boxes_total": boxes_total,
        "drawings_with_boxes": drawings,
        "picts_with_boxes": picts,
        "distinct_text_count": len(texts),
        "distinct_texts": texts,
        "paragraphs_direct_of_body": (len([kid for kid in kids if xml_local(kid.tag) == "p"])
                                      if kids is not None else 0),
        "paragraphs_anywhere": (len([x for x in kids.iter() if xml_local(x.tag) == "p"])
                                if kids is not None else 0),
        "paragraphs_in_boxes_direct": text_p,
        "paragraphs_in_boxes_anywhere": text_any,
        "boxes": rows[:limit],
    }


def odf_text_boxes(path: Path, limit: int = 100) -> dict:
    r"""同一问在 ODF 是一个 `draw:frame` 套一个 `draw:text-box`，尺寸是自带单位的串

    `svg:width="5cm"` 与 OOXML 那两处的 EMU / `style="width:5cm"` 都不是一回事，按写的交。
    LibreOffice 重写同一份 odt 时会挂 `draw:style-name="Frame"`、**把 `svg:x` / `svg:y` /
    `draw:z-index` 整个丢掉**，并把 5cm 换成 `5.001cm` —— 那些都是文件自己的话，不替它接。
    """
    with zipfile.ZipFile(path) as box:
        have = set(one.filename for one in box.infolist())
        if "content.xml" not in have:
            return {"available": False}
        crowd = ET.fromstring(content_raw := box.read("content.xml"))
    nsmap = _ns_prefixes(content_raw)
    rows = []
    with_boxes = box_total = text_p = text_any = 0
    texts: list = []
    frames = [x for x in crowd.iter() if xml_local(x.tag) == "frame"]
    for index, holder in enumerate(frames):
        count, direct, anywhere, parts = _box_ledger(holder, "text-box")
        joined = " / ".join(parts)
        box_total += count
        text_p += direct
        text_any += anywhere
        if count > 0:
            with_boxes += 1
            _note_box_text(texts, joined)
        written = _written_attrs(holder, nsmap)
        rows.append({
            "kind": "frame",
            "index": index,
            "text_boxes": count,
            "paragraphs_direct": direct,
            "paragraphs_anywhere": anywhere,
            "text": joined,
            "name_written": _local_in(written, "name"),
            "style_written": _local_in(written, "style-name"),
            "anchor_written": _local_in(written, "anchor-type"),
            "width_written": _local_in(written, "width"),
            "height_written": _local_in(written, "height"),
            "x_written": _local_in(written, "x"),
            "y_written": _local_in(written, "y"),
            "z_index_written": _local_in(written, "z-index"),
        })
    # 与 Rust 同一条：按局部名找一个**真有 `text:p` 孩子**的 `text` 元素（两跳取 office:body
    # → office:text 在两个读者里会从不同的节点出发，所以不这么走）
    text_root = None
    for had in crowd.iter():
        if xml_local(had.tag) != "text":
            continue
        if any(xml_local(kid.tag) == "p" for kid in had):
            text_root = had
            break
    direct_body = 0 if text_root is None else len(
        [kid for kid in text_root if xml_local(kid.tag) == "p"])
    return {
        "family": "odf",
        "available": True,
        "frames_total": len(frames),
        "frames_with_boxes": with_boxes,
        "text_box_elements": box_total,
        "distinct_text_count": len(texts),
        "distinct_texts": texts,
        "paragraphs_direct_of_text": direct_body,
        "paragraphs_anywhere": len([x for x in crowd.iter() if xml_local(x.tag) == "p"]),
        "paragraphs_in_boxes_direct": text_p,
        "paragraphs_in_boxes_anywhere": text_any,
        "boxes": rows[:limit],
    }


def _pairs_by(key: str, rows: list) -> bool:
    """按写着的号/名字问一句「对面有没有同样的一个」（None 与空串都不算一个）"""
    want = key
    return want is not None and want in [one for one in rows if one is not None]


def _tally_name(seen: list, raw) -> None:
    if raw is None:
        return
    for had in seen:
        if had[0] == raw:
            had[1] += 1
            return
    seen.append([raw, 1])


def docx_bookmark_pairs(path: Path, limit: int = 100) -> dict:
    r"""「这些书签是怎么配对的」：`w:bookmarkStart` 写名字，`w:bookmarkEnd` **只写号**

    所以闭没闭只能按 `w:id` 配；号是生产者自己排的（LibreOffice 重写时整批重排）。
    「开始没有结束」与「结束没有开始」两本账各数各的；名字以下划线开头的是 Word 自己的
    光标记号（`_GoBack`），另数一笔，不混进「这份文档有几个书签」。
    """
    with zipfile.ZipFile(path) as box:
        have = set(one.filename for one in box.infolist())
        if "word/document.xml" not in have:
            return {"available": False}
        root = ET.fromstring(box.read("word/document.xml"))
    body = [one for one in root if xml_local(one.tag) == "body"]
    kids = body[0] if body else None
    paras = [x for x in kids.iter() if xml_local(x.tag) == "p"] if kids is not None else []
    starts, ends = [], []
    for index, para in enumerate(paras):
        for kid in [x for x in para.iter() if xml_local(x.tag) == "bookmarkStart"]:
            written = {k.split(":")[-1]: v for k, v in _written_attrs(kid, {}).items()}
            starts.append({
                "paragraph": index,
                "id_written": written.get("id"),
                "name_written": written.get("name"),
                "hidden": (written.get("name") or "").startswith("_"),
            })
        for kid in [x for x in para.iter() if xml_local(x.tag) == "bookmarkEnd"]:
            written = {k.split(":")[-1]: v for k, v in _written_attrs(kid, {}).items()}
            ends.append({
                "paragraph": index,
                "id_written": written.get("id"),
                "name_written": written.get("name"),
            })
    all_marks = (len([x for x in kids.iter() if xml_local(x.tag) == "bookmarkStart"])
                 + len([x for x in kids.iter() if xml_local(x.tag) == "bookmarkEnd"])) if kids is not None else 0
    loose = max(0, all_marks - len(starts) - len(ends))
    start_ids = [one["id_written"] for one in starts]
    end_ids = [one["id_written"] for one in ends]
    names: list = []
    closed = hidden = 0
    for one in starts:
        done = _pairs_by(one["id_written"], end_ids)
        one["has_end"] = done
        closed += 1 if done else 0
        _tally_name(names, one["name_written"])
        hidden += 1 if one["hidden"] else 0
    for one in ends:
        one["has_start"] = _pairs_by(one["id_written"], start_ids)
    return {
        "family": "ooxml",
        "available": True,
        "starts_total": len(starts),
        "ends_total": len(ends),
        "pairs_closed": closed,
        "starts_without_end": len([one for one in starts if not one["has_end"]]),
        "ends_without_start": len([one for one in ends if not one["has_start"]]),
        "names_total": sum(had[1] for had in names),
        "distinct_names": [had[0] for had in names],
        "duplicate_names": [had[0] for had in names if had[1] > 1],
        "hidden_starts": hidden,
        "marks_outside_paragraphs": loose,
        "starts": starts[:limit],
        "ends": ends[:limit],
    }


def odf_bookmark_pairs(path: Path, limit: int = 100) -> dict:
    r"""同一问在 ODF 是**三种记号**：`text:bookmark` 是一枚点，start/end 才是一对跨段的

    配对按 `text:name`（这一族的 start 与 end 两头都写名字，没有号可配）。最要紧的一条：
    同段起止的一对在这一族被写成**一枚点**，所以「start 几条」在两族不是同一个问。
    """
    with zipfile.ZipFile(path) as box:
        have = set(one.filename for one in box.infolist())
        if "content.xml" not in have:
            return {"available": False}
        raw = box.read("content.xml")
        crowd = ET.fromstring(raw)
    nsmap = _ns_prefixes(raw)
    points, span_starts, span_ends = [], [], []
    paras = [x for x in crowd.iter() if xml_local(x.tag) == "p"]
    for index, para in enumerate(paras):
        for tag, sink in (("bookmark", points), ("bookmark-start", span_starts),
                          ("bookmark-end", span_ends)):
            for kid in [x for x in para.iter() if xml_local(x.tag) == tag]:
                sink.append({"paragraph": index,
                             "name_written": _local_in(_written_attrs(kid, nsmap), "name")})
    all_marks = sum(len([x for x in crowd.iter() if xml_local(x.tag) == tag])
                    for tag in ("bookmark", "bookmark-start", "bookmark-end"))
    loose = max(0, all_marks - len(points) - len(span_starts) - len(span_ends))
    start_names = [one["name_written"] for one in span_starts]
    end_names = [one["name_written"] for one in span_ends]
    closed = 0
    for one in span_starts:
        one["has_end"] = _pairs_by(one["name_written"], end_names)
        closed += 1 if one["has_end"] else 0
    for one in span_ends:
        one["has_start"] = _pairs_by(one["name_written"], start_names)
    names: list = []
    for one in points + span_starts:
        _tally_name(names, one["name_written"])
    return {
        "family": "odf",
        "available": True,
        "points_total": len(points),
        "spans_start": len(span_starts),
        "spans_end": len(span_ends),
        "spans_closed": closed,
        "starts_without_end": len([one for one in span_starts if not one["has_end"]]),
        "ends_without_start": len([one for one in span_ends if not one["has_start"]]),
        "names_total": sum(had[1] for had in names),
        "distinct_names": [had[0] for had in names],
        "duplicate_names": [had[0] for had in names if had[1] > 1],
        "marks_outside_paragraphs": loose,
        "points": points[:limit],
        "span_starts": span_starts[:limit],
        "span_ends": span_ends[:limit],
    }


def docx_page_numbering(path: Path, limit: int = 100) -> dict:
    r"""「这一节的页码怎么写」：`w:sectPr/w:pgNumType` 一枚元素、三个可以各自缺的属性

    `w:fmt` / `w:start` / `w:chpNum` 都可以不写。元素在场而属性是空的，与这一节根本没有
    这个元素，是两件事 —— 实测 LibreOffice 的 docx 导出只写 `fmt`，源件里明写的
    「从第 7 页开始」整个没跟过来（`start_written` 因此是 null，不是 7、也不是 1）。
    """
    with zipfile.ZipFile(path) as box:
        have = set(one.filename for one in box.infolist())
        if "word/document.xml" not in have:
            return {"available": False}
        root = ET.fromstring(box.read("word/document.xml"))
    body = [one for one in root if xml_local(one.tag) == "body"]
    rows = []
    sections = with_element = start_total = 0
    fmts: list = []
    for index, sect in enumerate([x for x in (body[0].iter() if body else [])
                                  if xml_local(x.tag) == "sectPr"]):
        sections += 1
        holder = [k for k in sect if xml_local(k.tag) == "pgNumType"]
        one = holder[0] if holder else None
        written = _docx_local_attrs(one) if one is not None else {}
        if one is not None:
            with_element += 1
        if written.get("start") is not None:
            start_total += 1
        if written.get("fmt") is not None and written["fmt"] not in fmts:
            fmts.append(written["fmt"])
        rows.append({
            "section": index,
            "element_present": one is not None,
            "start_written": written.get("start"),
            "fmt_written": written.get("fmt"),
            "chpnum_written": written.get("chpNum"),
            "written": written,
        })
    return {
        "family": "ooxml",
        "available": True,
        "sections_total": sections,
        "with_element": with_element,
        "start_written_total": start_total,
        "distinct_fmts": fmts,
        "sections": rows[:limit],
    }


PNUM_LOCALS = ("num-format", "page-number", "use-page-numbering", "num-prefix", "num-suffix")


def odf_page_numbering(path: Path, limit: int = 100) -> dict:
    r"""同一问在 ODF 写在页版式上（`style:num-format` / `style:page-number`），不是一节一条

    两份件都走（版式通常在 styles.xml，但不赌）。这一族的字母表与 `w:fmt` 不是一套词汇
    （`1` / `i` / `I` / `a` / `A` / `none` 对 `decimal` / `upperRoman`…），两边各按写的交、不折算。
    """
    with zipfile.ZipFile(path) as box:
        have = set(one.filename for one in box.infolist())
        if "content.xml" not in have:
            return {"available": False}
        maps = {}
        roots = [("content.xml", ET.fromstring(box.read("content.xml")))]
        maps["content.xml"] = _ns_prefixes(box.read("content.xml"))
        if "styles.xml" in have:
            roots.append(("styles.xml", ET.fromstring(box.read("styles.xml"))))
            maps["styles.xml"] = _ns_prefixes(box.read("styles.xml"))
    rows = []
    layouts = with_format = with_start = masters = 0
    formats: list = []
    for part, root in roots:
        nsmap = maps[part]
        for one in root.iter():
            if xml_local(one.tag) != "page-layout":
                continue
            name = _local_in(_written_attrs(one, nsmap), "name")
            kids = [k for k in one.iter() if xml_local(k.tag) == "page-layout-properties"]
            layouts += 1
            had = kids[0] if kids else None
            written = {}
            fmt = start = None
            if had is not None:
                for key, value in _written_attrs(had, nsmap).items():
                    local = key.rsplit(":", 1)[-1]
                    if key == "xmlns" or key.startswith("xmlns:") or local not in PNUM_LOCALS:
                        continue
                    written[local] = value
                    if local == "num-format":
                        fmt = value
                        with_format += 1
                        if value not in formats:
                            formats.append(value)
                    if local == "page-number":
                        start = value
                        with_start += 1
            rows.append({
                "part": part,
                "layout_name": name,
                "element_present": had is not None,
                "num_format_written": fmt,
                "page_number_written": start,
                "written": written,
            })
        masters += len([x for x in root.iter() if xml_local(x.tag) == "master-page"])
    return {
        "family": "odf",
        "available": True,
        "layouts_total": layouts,
        "masters_total": masters,
        "with_num_format": with_format,
        "with_page_number": with_start,
        "distinct_formats": formats,
        "layouts": rows[:limit],
    }


LANG_ATTRS = ("language", "country", "script")


def docx_languages(path: Path, limit: int = 100) -> dict:
    r"""`w:lang` 有三个属性：`w:val`（拉丁那一路）、`w:eastAsia`（中日韩那一路）、
    `w:bidi`（复杂脚本从右往左那一路）—— 一条元素可以同时说三路，也可以只说其中一路。
    """
    with zipfile.ZipFile(path) as box:
        have = set(one.filename for one in box.infolist())
        if "word/document.xml" not in have:
            return {"available": False}
        doc = ET.fromstring(box.read("word/document.xml"))
        styles_root = (ET.fromstring(box.read("word/styles.xml"))
                       if "word/styles.xml" in have else None)

    def attrs_of(node) -> dict:
        out = {}
        for key, value in node.attrib.items():
            tail = key.rsplit("}", 1)[-1]
            if key == "xmlns" or key.startswith("xmlns:"):
                continue
            out[tail] = value
        return out

    def langs(node):
        return [one for one in node.iter() if xml_local(one.tag) == "lang"]

    defaults = None
    if styles_root is not None:
        holder = [one for one in styles_root.iter() if xml_local(one.tag) == "docDefaults"]
        if holder:
            got = langs(holder[0])
            defaults = attrs_of(got[0]) if got else None
    style_rows = []
    if styles_root is not None:
        for one in styles_root.iter():
            if xml_local(one.tag) != "style":
                continue
            sid = stype = None
            for key, value in one.attrib.items():
                tail = key.rsplit("}", 1)[-1]
                if tail == "styleId":
                    sid = value
                elif tail == "type":
                    stype = value
            rpr = [kid for kid in one if xml_local(kid.tag) == "rPr"]
            got = langs(rpr[0]) if rpr else []
            if got:
                style_rows.append({"style_id": sid, "style_type": stype,
                                   "attrs": attrs_of(got[0])})
    body = [one for one in doc if xml_local(one.tag) == "body"]
    para_rows = []
    if body:
        for index, para in enumerate([kid for kid in body[0] if xml_local(kid.tag) == "p"]):
            ppr = [kid for kid in para if xml_local(kid.tag) == "pPr"]
            if not ppr:
                continue
            rpr = [kid for kid in ppr[0] if xml_local(kid.tag) == "rPr"]
            if not rpr:
                continue
            got = langs(rpr[0])
            if got:
                para_rows.append({"index": index, "attrs": attrs_of(got[0])})
    run_rows = []
    if body:
        for index, run in enumerate([one for one in body[0].iter()
                                     if xml_local(one.tag) == "r"]):
            rpr = [kid for kid in run if xml_local(kid.tag) == "rPr"]
            if not rpr:
                continue
            got = langs(rpr[0])
            if got:
                run_rows.append({"run": index, "attrs": attrs_of(got[0])})
    in_doc = len(langs(doc)) if body else 0
    in_styles = len(langs(styles_root)) if styles_root is not None else 0
    allnodes = langs(doc) + (langs(styles_root) if styles_root is not None else [])
    tables = {"val": [], "eastAsia": [], "bidi": []}
    for one in allnodes:
        got = attrs_of(one)
        for key in tables:
            value = got.get(key)
            if value is not None and value not in tables[key]:
                tables[key].append(value)
    levels = [one for one, had in (("doc_defaults", defaults is not None),
                                   ("styles", bool(style_rows)),
                                   ("paragraphs", bool(para_rows)),
                                   ("runs", bool(run_rows))) if had]
    return {
        "family": "ooxml",
        "available": True,
        "elements_total": in_doc + in_styles,
        "in_document": in_doc,
        "in_styles": in_styles,
        "doc_defaults_written": defaults is not None,
        "doc_defaults": defaults,
        "styles": style_rows[:limit],
        "styles_with_lang": len(style_rows),
        "paragraphs": para_rows[:limit],
        "paragraphs_with_lang": len(para_rows),
        "runs": run_rows[:limit],
        "runs_with_lang": len(run_rows),
        "distinct_vals": tables["val"],
        "distinct_east_asia": tables["eastAsia"],
        "distinct_bidi": tables["bidi"],
        "levels_seen": levels,
    }


def odf_languages(path: Path, limit: int = 100) -> dict:
    r"""ODF 把语言拆成两枚属性（`fo:language` + `fo:country`，另有 `fo:script`），
    宿主是字符属性 `style:text-properties`；值可以是字面 `none` —— 那是「说了：没有语言」，
    与整族一个字不写是两件事。
    """
    with zipfile.ZipFile(path) as box:
        have = set(one.filename for one in box.infolist())
        if "content.xml" not in have:
            return {"available": False}
        roots = [("content.xml", ET.fromstring(box.read("content.xml")))]
        if "styles.xml" in have:
            roots.append(("styles.xml", ET.fromstring(box.read("styles.xml"))))

    def picked(node):
        out = {}
        for key, value in node.attrib.items():
            tail = key.rsplit("}", 1)[-1]
            if key == "xmlns" or key.startswith("xmlns:"):
                continue
            if tail in LANG_ATTRS:
                out[tail] = value
        return out or None

    entries = []
    anywhere = 0
    for part, root in roots:
        for one in root.iter():
            if xml_local(one.tag) != "text-properties":
                continue
            if picked(one) is not None:
                anywhere += 1
        # 宿主走法（与 Rust 同一条，不用父指针）：两趟 —— 先所有 style，再所有 default-style
        for kind in ("style", "default-style"):
            for holder in root.iter():
                if xml_local(holder.tag) != kind:
                    continue
                name = family = None
                for key, value in holder.attrib.items():
                    tail = key.rsplit("}", 1)[-1]
                    if tail == "name":
                        name = value
                    elif tail == "family":
                        family = value
                for kid in holder:
                    if xml_local(kid.tag) != "text-properties":
                        continue
                    table = picked(kid)
                    if table is None:
                        continue
                    entries.append({"part": part, "holder": kind,
                                    "style_name": name, "family": family, "attrs": table})
    langs = sorted(set(one["attrs"].get("language") for one in entries
                       if one["attrs"].get("language")))
    countries = sorted(set(one["attrs"].get("country") for one in entries
                           if one["attrs"].get("country")))
    scripts = sorted(set(one["attrs"].get("script") for one in entries
                         if one["attrs"].get("script")))
    return {
        "family": "odf",
        "available": True,
        "elements_total": anywhere,
        "under_style": len([one for one in entries if one["holder"]]),
        "not_under_style": max(0, anywhere - len(entries)),
        "none_written": len([one for one in entries
                             if one["attrs"].get("language") == "none"]),
        "distinct_languages": langs,
        "distinct_countries": countries,
        "distinct_scripts": scripts,
        "parts_seen": sorted(set(one["part"] for one in entries)),
        "entries": entries[:limit],
    }


def docx_note_settings(path: Path, limit: int = 100) -> dict:
    r"""注的编号设置在 OOXML 写在**两处**：`w:settings.xml` 的 `w:footnotePr` / `w:endnotePr`，
    以及每一条 `w:sectPr` 里的同名元素。两处内容可以不一样。

    实测 `nset.docx`：settings 那份说了 `numStart="5"` 与 `numRestart="eachPage"`，
    sectPr 那一份只有 `pos` 与 `numFmt` —— 「从几开始」只在一处说过话。settings 那份还带
    两个 `w:footnote w:id` / `w:endnote w:id` 孩子（分隔符与延续分隔符的引用），sectPr 那份没有。
    """
    with zipfile.ZipFile(path) as box:
        have = set(one.filename for one in box.infolist())
        if "word/document.xml" not in have:
            return {"available": False}
        doc = ET.fromstring(box.read("word/document.xml"))
        settings = (ET.fromstring(box.read("word/settings.xml"))
                    if "word/settings.xml" in have else None)

    def attrs_of(node) -> dict:
        out = {}
        for key, value in node.attrib.items():
            tail = key.rsplit("}", 1)[-1]
            if key == "xmlns" or key.startswith("xmlns:"):
                continue
            out[tail] = value
        return out

    def one_holder(root, tag):
        if root is None:
            return None
        got = [one for one in root.iter() if xml_local(one.tag) == tag]
        return got[0] if got else None

    def read_pr(holder):
        """`w:footnotePr` / `w:endnotePr` 自己不写属性：值在孩子身上（`<w:numStart w:val="5"/>`），
        而 `w:footnote` / `w:endnote` 那两个孩子是分隔符引用（带 `w:id`）—— 两类各交一份。"""
        if holder is None:
            return None
        table = {}
        refs = []
        for kid in holder:
            name = xml_local(kid.tag)
            if name in ("footnote", "endnote"):
                refs.append(attrs_of(kid).get("id"))
                continue
            got = attrs_of(kid)
            table[name] = got.get("val") if "val" in got else got
        return {"written": table, "note_refs": refs, "holder_attrs": attrs_of(holder)}

    footnote_settings = read_pr(one_holder(settings, "footnotePr"))
    endnote_settings = read_pr(one_holder(settings, "endnotePr"))
    body = [one for one in doc if xml_local(one.tag) == "body"]
    sects = [x for x in (body[0].iter() if body else []) if xml_local(x.tag) == "sectPr"]
    rows = []
    for index, sect in enumerate(sects):
        row = {"section": index}
        for key, tag in (("footnote", "footnotePr"), ("endnote", "endnotePr")):
            holder = [kid for kid in sect if xml_local(kid.tag) == tag]
            got = read_pr(holder[0]) if holder else None
            row[key + "_written"] = got is not None
            row[key] = got
        rows.append(row)
    s_attrs = set()
    for one in (footnote_settings, endnote_settings):
        if one:
            s_attrs |= set(one["written"])
    x_attrs = set()
    for row in rows:
        for key in ("footnote", "endnote"):
            if row[key]:
                x_attrs |= set(row[key]["written"])
    return {
        "family": "ooxml",
        "available": True,
        "settings_part": settings is not None,
        "footnote_written": footnote_settings is not None,
        "endnote_written": endnote_settings is not None,
        "footnote": footnote_settings,
        "endnote": endnote_settings,
        "sections_total": len(sects),
        "sections_with_footnote_pr": len([one for one in rows if one["footnote_written"]]),
        "sections_with_endnote_pr": len([one for one in rows if one["endnote_written"]]),
        "sections": rows[:limit],
        "attrs_only_in_settings": sorted(s_attrs - x_attrs),
        "attrs_only_in_sections": sorted(x_attrs - s_attrs),
        "attrs_in_both": sorted(s_attrs & x_attrs),
    }


def odf_note_settings(path: Path, limit: int = 100) -> dict:
    r"""同一问在 ODF 是 `text:notes-configuration` 一份一类注，实测两份都在 **styles.xml**。

    两类注答得不对称：footnote 那份带 `text:footnotes-position` 与 `text:start-numbering-at`，
    endnote 那份只有 `style:num-format` 与 `text:start-value` —— 没有说的就交 null。
    词汇与 OOXML 不是一套（`1` / `i` 对 `decimal` / `lowerRoman`），两边各按写的交。
    """
    with zipfile.ZipFile(path) as box:
        have = set(one.filename for one in box.infolist())
        if "content.xml" not in have:
            return {"available": False}
        roots = [("content.xml", ET.fromstring(box.read("content.xml")))]
        if "styles.xml" in have:
            roots.append(("styles.xml", ET.fromstring(box.read("styles.xml"))))

    rows = []
    for part, root in roots:
        for one in root.iter():
            if xml_local(one.tag) != "notes-configuration":
                continue
            table = {}
            for key, value in one.attrib.items():
                tail = key.rsplit("}", 1)[-1]
                if key == "xmlns" or key.startswith("xmlns:"):
                    continue
                table[tail] = value
            rows.append({
                "part": part,
                "note_class": table.get("note-class"),
                "written": table,
                "num_format": table.get("num-format"),
                "start_value": table.get("start-value"),
                "position_written": "footnotes-position" in table,
                "start_numbering_written": "start-numbering-at" in table,
            })
    formats = []
    classes = []
    for one in rows:
        if one["num_format"] is not None and one["num_format"] not in formats:
            formats.append(one["num_format"])
        if one["note_class"] is not None and one["note_class"] not in classes:
            classes.append(one["note_class"])
    return {
        "family": "odf",
        "available": True,
        "configs_total": len(rows),
        "classes_written": classes,
        "distinct_num_formats": formats,
        "with_position": len([one for one in rows if one["position_written"]]),
        "with_start_numbering": len([one for one in rows if one["start_numbering_written"]]),
        "parts_seen": sorted(set(one["part"] for one in rows)),
        "configs": rows[:limit],
    }


PPTX_SHAPE_KINDS = ("sp", "pic", "graphicFrame", "grpSp", "cxnSp")
ODP_SHAPE_KINDS = ("g", "frame", "custom-shape", "control", "image")


def _shape_attrs(node) -> dict:
    """一个元素的属性表（局部名）；`xmlns` 那类声明不算属性，两边同一条"""
    out = {}
    for key, value in node.attrib.items():
        if key == "xmlns" or key.startswith("xmlns:"):
            continue
        out[key.rsplit("}", 1)[-1]] = value
    return out


def _own_xfrm(node) -> dict:
    r"""这个形状**自己**的 `a:xfrm`：直接孩子那枚，或某个 `*Pr` 直接孩子里那枚

    LibreOffice 重写时会连 `spTree` 自己的那份 `grpSpPr` 也补一个全 0 的 `a:xfrm`，
    而 `graphicFrame` 的 `xfrm` 干脆是直接孩子（不在任何 `*Pr` 里）—— 所以「往下找第一个」
    会把孙子的坐标算到爷爷头上。只看两层，不钻。
    """
    kids = list(node)
    for kid in kids:
        if xml_local(kid.tag) == "xfrm":
            return {xml_local(g.tag): _shape_attrs(g) for g in list(kid)}
    for kid in kids:
        if not xml_local(kid.tag).endswith("Pr"):
            continue
        for g in list(kid):
            if xml_local(g.tag) == "xfrm":
                return {xml_local(x.tag): _shape_attrs(x) for x in list(g)}
    return {}


def _own_cnvpr(node):
    r"""这个形状自己的那枚 `p:cNvPr`（在 `nv*Pr` 里），组合不会拿到孩子的"""
    for kid in list(node):
        name = xml_local(kid.tag)
        if not (name.startswith("nv") and name.endswith("Pr")):
            continue
        for one in kid.iter():
            if xml_local(one.tag) == "cNvPr":
                return one
    return None


def _own_placeholder(node) -> bool:
    r"""同一个 `nv*Pr` 里有没有 `ph`（只往下找一层的子树，不进孩子的形状）"""
    for kid in list(node):
        name = xml_local(kid.tag)
        if not (name.startswith("nv") and name.endswith("Pr")):
            continue
        if any(xml_local(one.tag) == "ph" for one in kid.iter()):
            return True
    return False


def _shape_size(node) -> dict:
    """ODF 那一族的尺寸与位置是自带单位的串，原样留着"""
    want = ("x", "y", "width", "height", "z-index", "transform")
    return {key: value for key, value in _shape_attrs(node).items() if key in want}


def _shape_paragraphs(node, holders) -> int:
    r"""这个形状**自己**的段有几段 —— 三种挂法：pptx 在直接孩子 `txBody` 里，
    ODF 的 `draw:frame` 在直接孩子 `draw:text-box` 里，而 `draw:custom-shape` 把
    `text:p` 直接挂在形状自己身上（实测 deck-gr.odp 三个 custom-shape 各带一段，
    只按「有没有 text-box」数出来全是 0）。只算自己这一层，组合里那些记在孩子身上。
    """
    total = 0
    for kid in list(node):
        name = xml_local(kid.tag)
        if name in holders:
            total += len([x for x in kid.iter() if xml_local(x.tag) in ("p", "h")])
        elif name in ("p", "h"):
            total += 1
    return total


def _text_carrier(node, holders):
    r"""字装在哪一层：`txBody` / `text-box` / `self` / None（这一个不装字）"""
    for kid in list(node):
        if xml_local(kid.tag) in holders:
            return xml_local(kid.tag)
    for kid in list(node):
        if xml_local(kid.tag) in ("p", "h"):
            return "self"
    return None


def _tally(rows):
    kinds = []
    names = []
    carriers = []
    for one in rows:
        if one["kind"] not in kinds:
            kinds.append(one["kind"])
        if one["name"] and one["name"] not in names:
            names.append(one["name"])
        if one["text_carrier"] and one["text_carrier"] not in carriers:
            carriers.append(one["text_carrier"])
    return kinds, names, carriers


def _ledger(family, rows):
    kinds, names, carriers = _tally(rows)
    return {
        "family": family,
        "available": True,
        "shapes_total": len(rows),
        "top_level": len([one for one in rows if one["depth"] == 0]),
        "nested": len([one for one in rows if one["depth"] != 0]),
        "groups": len([one for one in rows if one["kind"] in ("grpSp", "g")]),
        "unnamed": len([one for one in rows if one["name"] is None]),
        "placeholders": len([one for one in rows if one["placeholder"] is True]),
        "carriers_seen": carriers,
        "paragraphs_in_shapes": sum(one["paragraphs_direct"] for one in rows),
        "kinds_seen": kinds,
        "distinct_names": names,
        "max_depth": max([one["depth"] for one in rows], default=0),
        "shapes": rows,
    }


def slide_shape_tree_pptx(slide_root, limit: int = 400) -> dict:
    r"""这一页有哪些形状、哪个是组合：`p:spTree` 的直接孩子按序就是叠放序

    组的 `cNvPr` 排在自己的孩子之前，所以「文档序第一个 `cNvPr`」就是这个形状自己的，
    不需要父指针。`xfrm` 四格 `off` / `ext` / `chOff` / `chExt` 原样交（EMU），组合那一份
    实测 `off` 是 0,0 而 `ext` 与 `chExt` 一模一样 —— 按写的交，不替它换算。
    """
    trees = [one for one in slide_root.iter() if xml_local(one.tag) == "spTree"]
    rows = []

    def walk(node, depth, parent):
        for one in list(node):
            if xml_local(one.tag) not in PPTX_SHAPE_KINDS:
                continue
            mine = len(rows)
            cnv = _own_cnvpr(one)
            rows.append({
                "index": mine,
                "kind": xml_local(one.tag),
                "name": cnv.attrib.get("name") if cnv is not None else None,
                "id": cnv.attrib.get("id") if cnv is not None else None,
                "depth": depth,
                "parent": parent,
                "placeholder": _own_placeholder(one),
                "xfrm": _own_xfrm(one),
                "size_written": {},
                "text_carrier": _text_carrier(one, ("txBody",)),
                "has_text_body": any(xml_local(kid.tag) == "txBody" for kid in list(one)),
                "paragraphs_direct": _shape_paragraphs(one, ("txBody",)),
                "children": len([x for x in list(one)
                                 if xml_local(x.tag) in PPTX_SHAPE_KINDS]),
            })
            if xml_local(one.tag) == "grpSp":
                walk(one, depth + 1, mine)

    if trees:
        walk(trees[0], 0, None)
    return _ledger("ooxml", rows[:limit])


def slide_shape_tree_odp(page, limit: int = 400) -> dict:
    r"""同一问在 ODF：这一页 `draw:page` 的孩子，分组是 `svg:g`（不是 `draw:group`）

    `notes` 不在形状白名单里，所以备注页那棵树天然进不来。这一族没有 `cNvPr`，`id` 一律
    null，尺寸是 `x="0.278cm"` 这种自带单位的串。`nested` 与 pptx 不同义：`draw:frame` 套
    `draw:image` 也算一层，所以它可以 nonzero 而 `groups` 是 0。
    """
    rows = []

    def walk(node, depth, parent):
        for one in list(node):
            if xml_local(one.tag) not in ODP_SHAPE_KINDS:
                continue
            mine = len(rows)
            name = None
            for key, value in one.attrib.items():
                if key.rsplit("}", 1)[-1] == "name" and name is None:
                    name = value
            rows.append({
                "index": mine,
                "kind": xml_local(one.tag),
                "name": name,
                "id": None,
                "depth": depth,
                "parent": parent,
                "placeholder": None,
                "xfrm": {},
                "size_written": _shape_size(one),
                "text_carrier": _text_carrier(one, ("text-box",)),
                "has_text_body": any(xml_local(kid.tag) == "text-box" for kid in list(one)),
                "paragraphs_direct": _shape_paragraphs(one, ("text-box",)),
                "children": len([x for x in list(one)
                                 if xml_local(x.tag) in ODP_SHAPE_KINDS]),
            })
            walk(one, depth + 1, mine)

    walk(page, 0, None)
    return _ledger("odf", rows[:limit])


def _f_attr_map(node) -> dict:
    """一枚元素的属性表（局部名，`xmlns` 那类不算）"""
    out = {}
    for key, value in node.attrib.items():
        if key == "xmlns" or key.startswith("xmlns:"):
            continue
        out[key.rsplit("}", 1)[-1]] = value
    return out


def xlsx_formula_elems(path: Path, limit: int = 400) -> dict:
    r"""公式那枚 `<f>` 自己写了什么 —— 共享公式的跟随格在文件里**没有公式正文**

    Excel 把一列里长得一样的公式存成一份共享组：主格写 `<f t="shared" ref="B1:B8" si="0">A1*2</f>`，
    跟随格只写 `<f t="shared" si="0"/>` —— 正文是空的，要按 `si` 找到主格再按行平移才知道它是什么。
    所以「这一列几格有公式」「几格写了正文」「缓存值在不在」是三个数（实测 `shared.xlsx` 是
    16 / 9 / 16）。两个生产者对同一件事写得不一样：openpyxl 的 `<f>` 一个属性都不写，
    LibreOffice 每条都写 `aca="false"`（实测 16 条全带），而它**不写共享组**（读进去再导出，
    八条各写自己的正文 —— 见 `shared-lo.xlsx`）。
    """
    rows = []
    sheets = []
    with zipfile.ZipFile(path) as box:
        names = sorted(one.filename for one in box.infolist()
                       if one.filename.startswith("xl/worksheets/sheet")
                       and one.filename.endswith(".xml"))
        for name in names:
            root = ET.fromstring(box.read(name))
            sheets.append(name)
            for cell in root.iter():
                if xml_local(cell.tag) != "c":
                    continue
                kids = [one for one in cell if xml_local(one.tag) == "f"]
                if not kids:
                    continue
                had = kids[0]
                body = (had.text or "").strip()
                cached = [one for one in cell if xml_local(one.tag) == "v"]
                rows.append({
                    "sheet": name,
                    "cell": _f_attr_map(cell).get("r"),
                    "attrs": _f_attr_map(had),
                    "text": body,
                    "text_written": bool(body),
                    "shared": _f_attr_map(had).get("t") == "shared",
                    "si": _f_attr_map(had).get("si"),
                    "ref_written": _f_attr_map(had).get("ref"),
                    "cached_written": bool(cached),
                    "cached": (cached[0].text or "").strip() if cached else None,
                })
    values = {}
    for one in rows:
        for key, value in one["attrs"].items():
            bucket = values.setdefault(key, {})
            bucket[value] = bucket.get(value, 0) + 1
    seen = []
    for one in rows:
        for key in one["attrs"]:
            if key not in seen:
                seen.append(key)
    order = sorted(values, key=lambda k: -sum(values[k].values()))
    return {
        "family": "ooxml",
        "available": True,
        "sheets_seen": len(sheets),
        "formula_elems": len(rows),
        "with_attrs": len([one for one in rows if one["attrs"]]),
        "attrs_seen": seen,
        "attr_values": {key: values[key] for key in order},
        "text_written": len([one for one in rows if one["text_written"]]),
        "empty_text": len([one for one in rows if not one["text_written"]]),
        "empty_text_with_cached": len([one for one in rows
                                       if not one["text_written"] and one["cached_written"]]),
        "shared_elems": len([one for one in rows if one["shared"]]),
        "shared_masters": len([one for one in rows if one["shared"] and one["text_written"]]),
        "shared_followers": len([one for one in rows if one["shared"] and not one["text_written"]]),
        "si_written": len([one for one in rows if one["si"] is not None]),
        "ref_written_elems": len([one for one in rows if one["ref_written"] is not None]),
        "cached_elems": len([one for one in rows if one["cached_written"]]),
        "cells": rows[:limit],
    }


def ods_formula_elems(path: Path, limit: int = 400) -> dict:
    r"""同一问在 ODF：公式是格子身上的一个属性，**每条都带正文**，没有共享组这一层

    实测 `shared.ods`：16 个有公式的格子，16 条都写着完整文本（`of:=[.A2]*2` 这种逐行平移），
    空文本 0 条 —— 也就是说 LibreOffice 把 xlsx 那份共享组读进去之后，按它自己的存法
    一个字都不省。`of:` 那个前缀按写的留着（`ooo:` 是另一族写的），交在 `formula_prefixes`
    这一格里（xlsx 那一族没有前缀这回事，所以那个键在那边整个不出现）。

    带公式的是**格子自己**，所以 `attrs` 交的是那一格写着的属性全表（照文件写的，局部名），
    公式只是其中一个；`sheet` 是它所在那张 `table:table` 写的名字（ODF 里「哪张表」就是
    「哪个表」，而这一族**没有**格子地址这个东西 —— 列可以整个不写、行可以用 repeated
    顶好几行，所以 `cell` 逐条交 null，那是「数过了，这一族没写」）。
    """
    with zipfile.ZipFile(path) as box:
        if "content.xml" not in box.namelist():
            return {"family": "odf", "available": False}
        root = ET.fromstring(box.read("content.xml"))
    tables = [one for one in root.iter() if xml_local(one.tag) == "table"]
    # 往上找 enclosing 的 table:table —— ElementTree 不给孩子指父亲，这里自己建一张父表
    parent = {}
    for one in root.iter():
        for kid in one:
            parent[kid] = one

    def owner(node):
        walk = parent.get(node)
        while walk is not None:
            if xml_local(walk.tag) == "table":
                return _f_attr_map(walk).get("name")
            walk = parent.get(walk)
        return None

    rows = []
    seen_attrs: list = []
    values: dict = {}
    for cell in root.iter():
        if xml_local(cell.tag) not in ("table-cell", "covered-table-cell"):
            continue
        attrs = _f_attr_map(cell)
        if "formula" not in attrs:
            continue
        text = attrs["formula"]
        kids = [one for one in cell if xml_local(one.tag) == "p"]
        for key in attrs:
            if key not in seen_attrs:
                seen_attrs.append(key)
        for key, value in attrs.items():
            bucket = values.setdefault(key, {})
            bucket[value] = bucket.get(value, 0) + 1
        rows.append({
            "sheet": owner(cell),
            "cell": None,
            "style": attrs.get("style-name"),
            "attrs": attrs,
            "text": text,
            "text_written": bool(text),
            "cached_written": "value" in attrs,
            "cached": attrs.get("value"),
            "paragraphs": len(kids),
        })
    prefixes = {}
    for one in rows:
        head = one["text"].split(":", 1)[0] if ":" in one["text"] else ""
        prefixes[head] = prefixes.get(head, 0) + 1
    return {
        "family": "odf",
        "available": True,
        "tables_seen": len(tables),
        "formula_elems": len(rows),
        "with_attrs": len([one for one in rows if one["attrs"]]),
        "attrs_seen": seen_attrs,
        "attr_values": values,
        "formula_prefixes": prefixes,
        "text_written": len([one for one in rows if one["text_written"]]),
        "empty_text": len([one for one in rows if not one["text_written"]]),
        "empty_text_with_cached": len([one for one in rows
                                        if not one["text_written"] and one["cached_written"]]),
        # 共享组那一层在 ODF 没有位置：不是 0（数过了没有），而是这个键整个不交
        "cached_elems": len([one for one in rows if one["cached_written"]]),
        "cells": rows[:limit],
    }


def odf_style_holders(node) -> list:

    """`style:style` 与 `style:default-style` 都算样式持有者，按文档顺序（不往里套）"""
    out: list = []
    for kid in node:
        if xml_local(kid.tag) in ("style", "default-style"):
            out.append(kid)
            continue
        out.extend(odf_style_holders(kid))
    return out


def odf_fonts(path: Path, limit: int = 100) -> dict:
    r"""同一问的 ODF 形状：表是 `style:font-face`，点它的地方在样式里，而且是两种指针

    `style:font-name` 对的是 face 的 `style:name`，`style:font-family` 对的是那条
    `svg:font-family` —— 两个各自数一遍、各自判落没落地（并成一个数就会把「表里没这个名字」
    与「族名没出现过」混成一件事）。实测 fonts.odp/odt 里这张表在 content.xml 与 styles.xml
    各写一份一模一样的，所以两份都走、同名先到的一条算数。
    """
    with zipfile.ZipFile(path) as box:
        names = set(one.filename for one in box.infolist())
        raws = {part: box.read(part) for part in ("content.xml", "styles.xml") if part in names}
    roots = []
    for part, raw in raws.items():
        roots.append((ET.fromstring(raw), _ns_prefixes(raw), part))
    faces: list = []
    declared_names: list = []
    families: list = []
    dup = quoted = no_family = charset = embed_refs = 0
    for root, nsmap, part in roots:
        for one in root.iter():
            if xml_local(one.tag) != "font-face":
                continue
            written = _written_attrs(one, nsmap)
            name = _local_in(written, "name") or ""
            family = _local_in(written, "font-family")
            if name in declared_names:
                dup += 1
                continue
            declared_names.append(name)
            if family is None:
                no_family += 1
            else:
                families.append(family)
                if len(family) > 1 and family.startswith("'") and family.endswith("'"):
                    quoted += 1
            if _local_in(written, "font-charset") is not None:
                charset += 1
            if _local_in(written, "embed") is not None:
                embed_refs += 1
            if len(faces) < limit:
                faces.append({"name": name, "family": family, "written": written, "part": part})
    rows: list = []
    elements = 0
    by_name: dict = {}
    by_family: dict = {}
    for root, nsmap, part in roots:
        for holder in odf_style_holders(root):
            holder_attrs = _written_attrs(holder, nsmap)
            style_name = _local_in(holder_attrs, "name")
            style_family = _local_in(holder_attrs, "family")
            for kid in holder:
                if xml_local(kid.tag) != "text-properties":
                    continue
                written = _written_attrs(kid, nsmap)
                name = _local_in(written, "font-name")
                family = _local_in(written, "font-family")
                if name is None and family is None:
                    continue
                elements += 1
                _bump(by_name, name)
                _bump(by_family, family)
                if len(rows) < limit:
                    rows.append({
                        "part": part,
                        "style": style_name,
                        "style_family": style_family,
                        "written": written,
                        "font_name": name,
                        "font_family": family,
                        "name_declared": name in declared_names if name is not None else False,
                        "family_declared": family in families if family is not None else False,
                    })
    undeclared_names = [key for key in sorted(by_name)
                        if not key.startswith("(没写)") and key not in declared_names]
    undeclared_families = [key for key in sorted(by_family)
                           if not key.startswith("(没写)") and key not in families]
    unused = [one for one in declared_names if one not in by_name]
    return {
        "family": "odf",
        "faces": faces,
        "faces_total": len(faces),
        "faces_duplicated": dup,
        "families_quoted": quoted,
        "faces_no_family": no_family,
        "faces_with_charset": charset,
        "embedded_refs": embed_refs,
        "pointer_elements": elements,
        "rows": rows,
        "by_font_name": by_name,
        "by_font_family": by_family,
        "undeclared_names": undeclared_names,
        "undeclared_families": undeclared_families,
        "declared_unused": unused,
        "declared_unused_total": len(unused),
        "theme": None,
    }


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
        # 字体那份账：表在 fontTable、点在每一格 rFonts（一个选择四个属性）、主题再一跳
        "fonts": docx_fonts(parts, body),
        # 分节的页眉页脚六格（自己写的与真正沿用的分开交）
        "header_footers": docx_header_footers(body, parts),
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
                    # 只交文件写着的 `type`：没写就是 null。规范有个默认值，
                    # 但那是规范的话，不是这份文件的话（python-pptx 的正文占位符
                    # 只写 idx，LibreOffice 重写那份连 idx 都丢了）
                    ph = node.get("type")
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
                "autofit": slide_autofit(root, _ns_prefixes(parts[name])),
                # 切换的细则：几条、每条写了哪些属性、效果孩子自己带了什么
                "transition_detail": slide_transition_detail(root),
                "shape_tree": slide_shape_tree_pptx(root),
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


def slide_transition_detail(slide_root) -> dict:
    r"""这一页写了什么切换：`p:transition` 的属性与它的效果孩子，全按写的交

    与 Rust 的 `slide_transitions` 同一条口径（属性按局部名，前缀是文件自己声明的）。
    一页写两条是真的会发生的（LibreOffice 给一份什么都没写的稿子补了两条），
    所以这里交的是「几条 + 每条自己写了什么」，不是一个布尔。
    """
    rows = []
    for one in slide_root.iter():
        if xml_local(one.tag) != "transition":
            continue
        effects = [{"element": xml_local(kid.tag), "written": _docx_local_attrs(kid)}
                   for kid in one]
        rows.append({"written": _docx_local_attrs(one), "effects": effects})
    return {"elements": len(rows), "list": rows}


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
        # 字体：表是 style:font-face（名字与族名两个键），点它的地方在样式里，两种指针各数一遍
        "fonts": odf_fonts(path),
        # 页眉页脚在母版页上：六格一份账，另说有没有一节点过这份母版页的名
        "header_footers": odf_header_footers(path),
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


def slide_autofit(root, nsmap: dict, limit: int = 100) -> dict:
    r"""pptx：一个框怎么处理「字比框大」写在 `a:bodyPr` 的**独子元素名**上

    与 `src/office_slide.rs` 的 `slide_autofit` 同一份账。要盯的是「一个子元素都没有」
    那一档：python-pptx 在 MSO_AUTO_SIZE.NONE 什么都不写，而它给新建文本框的默认是
    `<a:spAutoFit/>` —— 「没有子元素」与「写了 a:noAutofit」在文件里是两件事。
    """
    rows: list = []
    by_element: dict = {}
    shapes = no_body = says_nothing = shrinks = wrap_written = box_written = second = 0
    for index, sp in enumerate([one for one in root.iter() if xml_local(one.tag) == "sp"]):
        shapes += 1
        body = None
        for node in sp.iter():
            if xml_local(node.tag) == "txBody":
                body = _first_kid(node, "bodyPr")
                break
        element = None
        autofit_written = None
        others: list = []
        if body is not None:
            for kid in body:
                if xml_local(kid.tag).lower().endswith("autofit"):
                    if element is None:
                        element = _written_name(kid.tag, nsmap)
                        autofit_written = written_attrs(kid)
                        by_element[element] = by_element.get(element, 0) + 1
                        continue
                    second += 1
                others.append(_written_name(kid.tag, nsmap))
        xfrm = None
        for node in sp.iter():
            if xml_local(node.tag) == "xfrm":
                xfrm = node
                break
        off = _first_kid(xfrm, "off") if xfrm is not None else None
        ext = _first_kid(xfrm, "ext") if xfrm is not None else None
        wrap = of_local(body, "wrap") if body is not None else None
        if wrap is not None:
            wrap_written += 1
        if off is not None or ext is not None:
            box_written += 1
        shrinks_here = bool(element) and element.rsplit(":", 1)[-1] == "normAutofit"
        if shrinks_here:
            shrinks += 1
        if body is not None and element is None:
            says_nothing += 1
        if body is None:
            no_body += 1
        if len(rows) < limit:
            paras = [one for one in sp.iter() if xml_local(one.tag) == "p"]
            rows.append(
                {
                    "shape": index,
                    "name": (
                        of_local(_first_any(sp, "cNvPr"), "name")
                        if _first_any(sp, "cNvPr") is not None
                        else None
                    ),
                    "placeholder": (
                        of_local(_first_any(sp, "ph"), "type")
                        if _first_any(sp, "ph") is not None
                        else None
                    ),
                    "paragraphs": len(paras),
                    "text": "".join(paras[0].itertext()).strip() if paras else "",
                    "has_bodyPr": body is not None,
                    "written": written_attrs(body) if body is not None else None,
                    "autofit_element": element,
                    "autofit_written": autofit_written,
                    "other_children": others,
                    "shrinks_text": shrinks_here,
                    "off": written_attrs(off) if off is not None else None,
                    "ext": written_attrs(ext) if ext is not None else None,
                    "off_mm": {
                        key: (mm_of(off.get(key), "emu") if off is not None and off.get(key) is not None else None)
                        for key in ("x", "y")
                    },
                    "ext_mm": {
                        key: (mm_of(ext.get(key), "emu") if ext is not None and ext.get(key) is not None else None)
                        for key in ("cx", "cy")
                    },
                }
            )
    return {
        "family": "ooxml",
        "shapes": shapes,
        "checked": len(rows),
        "rows": rows,
        "no_bodyPr": no_body,
        "by_element": by_element,
        "second_autofit_child": second,
        "shrinks_text": shrinks,
        "says_nothing": says_nothing,
        "wrap_written": wrap_written,
        "box_written": box_written,
        "bodyPr_total": count_local(root, "bodyPr"),
    }


def odp_graphic_styles_of(parts: dict) -> dict:
    """两份件里 family=graphic 的样式：名字 → 它那一处 graphic-properties（前缀原样留着）

    两份都走、同名先到的一条算数 —— 与 `odp_cell_styles` 同一条规则，不拿「自动样式一定在
    content.xml」当文件规矩。
    """
    out: dict = {}
    for part in ODP_STYLE_PARTS:
        raw = parts.get(part)
        if raw is None:
            continue
        nsmap = _ns_prefixes(raw)
        for one in ET.fromstring(raw).iter():
            if xml_local(one.tag) != "style" or of_local(one, "family") != "graphic":
                continue
            name = of_local(one, "name")
            if not name or name in out:
                continue
            props = None
            for kid in one:
                if xml_local(kid.tag) == "graphic-properties":
                    props = kid
                    break
            out[name] = {
                "name": name,
                "part": part,
                "parent": of_local(one, "parent-style-name"),
                "element": _written_name(props.tag, nsmap) if props is not None else None,
                "attrs": _written_attrs(props, nsmap) if props is not None else None,
            }
    return out


ODP_AUTOFIT_WANTS = ("shrink-to-fit", "fit-to-size", "wrap-option")


def odp_autofit(page, styles: dict, limit: int = 100) -> dict:
    r"""odp：同一个问题写在**样式**上 —— 框点名的 graphic 样式 → 那条 graphic-properties

    实测 LibreOffice 把 pptx 的「什么都不做」与「框随字长」两档写成一模一样的一条
    （shrink-to-fit=false + fit-to-size=false），所以这一族解不出 spAutoFit 那一档；
    三个值各数各的，不替谁猜回哪一档。
    """
    rows: list = []
    shapes = found = missing = props_written = says_nothing = shrinks = box_written = 0
    tallies = {key: {} for key in ODP_AUTOFIT_WANTS}
    for index, shape in enumerate(
        [one for one in page.iter() if xml_local(one.tag) == "custom-shape"]
    ):
        shapes += 1
        named = None
        for key, value in shape.attrib.items():
            local = key.rsplit("}", 1)[-1]
            if local != "style-name":
                continue
            uri = key[1:].split("}", 1)[0] if key.startswith("{") else ""
            # 与 Rust 同一条：只排掉 text:style-name（那一段字点的样式），前缀叫什么不管
            if uri.endswith("text:1.0"):
                continue
            named = value
            break
        had = styles.get(named) if named else None
        if had is None:
            missing += 1
        else:
            found += 1
        attrs = (had or {}).get("attrs")
        picked = {}
        if attrs is not None:
            props_written += 1
            for key, value in attrs.items():
                if key.rsplit(":", 1)[-1] in ODP_AUTOFIT_WANTS:
                    picked[key] = value
        values = {}
        for want in ODP_AUTOFIT_WANTS:
            raw = None
            for key, value in (attrs or {}).items():
                if key.rsplit(":", 1)[-1] == want:
                    raw = value
                    break
            values[want] = raw
            slot = tallies[want]
            mark = raw if raw is not None else "(没写)"
            slot[mark] = slot.get(mark, 0) + 1
        shrinks_here = values["shrink-to-fit"] == "true"
        if shrinks_here:
            shrinks += 1
        if attrs is not None and not picked:
            says_nothing += 1
        box = {key: of_local(shape, key) for key in ("x", "y", "width", "height")}
        if any(value is not None for value in box.values()):
            box_written += 1
        if len(rows) < limit:
            paras = [one for one in shape.iter() if xml_local(one.tag) == "p"]
            rows.append(
                {
                    "shape": index,
                    "name": of_local(shape, "name"),
                    "style": named,
                    "style_found": had is not None,
                    "style_part": (had or {}).get("part"),
                    "style_parent": (had or {}).get("parent"),
                    "props_element": (had or {}).get("element"),
                    "written": picked if attrs is not None else None,
                    "shrink_to_fit": values["shrink-to-fit"],
                    "fit_to_size": values["fit-to-size"],
                    "wrap_option": values["wrap-option"],
                    "paragraphs": len(paras),
                    "text": "".join(paras[0].itertext()).strip() if paras else "",
                    "shrinks_text": shrinks_here,
                    "box": box,
                }
            )
    return {
        "family": "odf",
        "shapes": shapes,
        "checked": len(rows),
        "rows": rows,
        "style_found": found,
        "style_missing": missing,
        "props_written": props_written,
        "shrink_to_fit": tallies["shrink-to-fit"],
        "fit_to_size": tallies["fit-to-size"],
        "wrap_option": tallies["wrap-option"],
        "shrinks_text": shrinks,
        "says_nothing": says_nothing,
        "box_written": box_written,
        "frames": count_local(page, "frame"),
    }


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


ODF_EFFECT_LOCALS = ("type", "subtype", "duration", "direction", "fadeColor")


def odp_transition(parts: dict, named, page) -> dict:
    r"""同一页的切换在 ODF 写在两处：页点名的 drawing-page 样式 + 页体内的动画树

    两处都按写的交，不互证、不挑一个当准（第二读者与 Rust 同一条口径：属性收局部名，
    `xmlns` 那类声明不算属性）。样式找到了但那一份 properties 没写，`style_found` 就是
    false —— 与「点了名没有那份样式」同一种「没说到」，不混进「写了空」。
    """
    props = {}
    part = None
    if named is not None:
        for cand in ("content.xml", "styles.xml"):
            if cand not in parts:
                continue
            root = ET.fromstring(parts[cand])
            nsmap = _ns_prefixes(parts[cand])
            reached = False
            for one in root.iter():
                if xml_local(one.tag) != "style":
                    continue
                written = _written_attrs(one, nsmap)
                if _local_in(written, "family") != "drawing-page":
                    continue
                if _local_in(written, "name") != named:
                    continue
                kids = [k for k in one.iter() if xml_local(k.tag) == "drawing-page-properties"]
                if kids:
                    for key, value in _written_attrs(kids[0], nsmap).items():
                        local = key.rsplit(":", 1)[-1]
                        if key == "xmlns" or key.startswith("xmlns:"):
                            continue
                        if "transition" in local or local in ODF_EFFECT_LOCALS:
                            props[local] = value
                    part = cand
                reached = True
                break
            if reached:
                break
    nsmap = _ns_prefixes(parts["content.xml"])
    effects = [{"written": {k.rsplit(":", 1)[-1]: v
                            for k, v in _written_attrs(one, nsmap).items()
                            if not k.startswith("xmlns")}}
               for one in page.iter() if xml_local(one.tag) == "transitionFilter"]
    return {
        "page_style": named,
        "style_found": part is not None,
        "style_part": part,
        "written": props,
        "effects": effects,
        "timing_roots": len([one for one in page.iter()
                             if xml_local(one.tag) == "par"
                             and of_local(one, "node-type") == "timing-root"]),
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
    # 框的 autofit 也住在样式里（family=graphic），与格子样式同一类两跳
    graphic_styles = odp_graphic_styles_of(parts)

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
                # 同一个问题这一族写在框点名的那份 graphic 样式里
                "autofit": odp_autofit(page, graphic_styles),
                # 这一族的链接直接写在字上，且只走页上的 frame（备注那一块另算）
                "links": odp_slide_links(
                    [one for one in page if xml_local(one.tag) != "notes"]
                ),
                # 藏不藏要跳一跳：页只点名样式，那句话在那份样式里
                "hidden": odp_page_visibility(
                    page_styles, of_local(page, "style-name")
                )["hidden"],
                "visibility": odp_page_visibility(page_styles, of_local(page, "style-name")),
                "odp_transition": odp_transition(parts, of_local(page, "style-name"), page),
                "shape_tree": slide_shape_tree_odp(page),
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
            # 表头重复那份账（行上的一个无值元素）
            out["ooxml"]["table_headers"] = docx_repeat_headers(path)
            # 制表位：定义在段上，而段里的制表字符是另一本账
            out["ooxml"]["tab_stops"] = docx_tab_stops(path)
            # 批注那一份账：内容在部件、锚点在正文，两边按号配
            out["ooxml"]["comment_ledger"] = docx_comment_ledger(path)
            out["ooxml"]["comment_threads"] = docx_comment_threads(path)
            out["ooxml"]["section_starts"] = docx_section_starts(path)
            # 结构搬进 markdown 那一本（`office-text --markdown` 的对照）
            out["ooxml"]["markdown"] = docx_markdown_ledger(path)
            # 文档里的公式：OMML 挂在段上，行内与独立成行按文件写着的交
            out["ooxml"]["equations"] = docx_equations_ledger(path)
            # 分页那四个开关（段上；ODF 一跳在样式里）
            out["ooxml"]["keep_switches"] = docx_keep_switches(path)
            # 这张表套的是哪个样式：样式 id 与那枚 look 分开交
            out["ooxml"]["table_styles"] = docx_table_styles(path)
            # 这一段的行距：那个数的**单位**由 lineRule 决定，所以两枚分开各交
            out["ooxml"]["line_spacing"] = docx_line_spacing(path)
            # 段边框与底纹：壳在不在与里面写了几条边是两件事
            out["ooxml"]["para_borders"] = docx_para_borders(path)
            # 文本框：同一个框可以在两种容器里各写一遍，「几份格子」与「几句话」两个数
            out["ooxml"]["text_boxes"] = docx_text_boxes(path)
            # 书签配对：起写名字、止只写号，断的两个方向各数一本
            out["ooxml"]["bookmark_pairs"] = docx_bookmark_pairs(path)
            # 这一节的页码：元素在场、属性写了什么，两件事分开
            out["ooxml"]["page_numbering"] = docx_page_numbering(path)
            # 这份文档写了哪种语言：`w:lang` 三个属性分三路说话
            out["ooxml"]["languages"] = docx_languages(path)
            # 注的编号：settings 与 sectPr 两处各一份，内容可以不一样
            out["ooxml"]["note_settings"] = docx_note_settings(path)
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
            # 打印区域与重复标题行：不在表上，在 workbook.xml 那两条保留名上
            out["ooxml"]["print_ranges"] = xlsx_print_ranges(path)
            out["ooxml"]["formula_elems"] = xlsx_formula_elems(path)
        elif "ppt/presentation.xml" in parts:
            out["app"] = "powerpoint"
            out["ooxml"] = pptx_facts(path)
            # 「这框对应版式里哪一条」那一跳（重写那份会断）
            out["ooxml"]["placeholder_hops"] = pptx_placeholder_hops(path)
            # 一条式子是文本体里的 OMML，而同一个形状在 Fallback 里还写了一遍（挂着替身图）
            out["ooxml"]["equations"] = pptx_equations_ledger(path)
        elif "content.xml" in parts:
            out["app"] = "opendocument"
            out["odf"] = odt_facts(path)
            out["links"] = odf_links(path)
            out["page_text"] = odf_page_text(path)
            out["odt"] = odt_structure(path)
            # 同一问在 ODF 是表身上的两个数
            out["odt"]["table_headers"] = odt_repeat_headers(path)
            # 同一问在 ODF 一跳之外：段只点样式名，制表位在样式里
            out["odt"]["tab_stops"] = odf_tab_stops(path)
            # 同一问在 ODF 只有一处：批注坐在段里面
            out["odt"]["comment_ledger"] = odf_comment_ledger(path)
            out["odt"]["comment_threads"] = odf_comment_threads(path)
            out["odt"]["keep_switches"] = odf_keep_switches(path)
            out["odt"]["table_styles"] = odf_table_styles(path)
            # 同一问在 ODF 一跳在样式里，而单位是写在串上的
            out["odt"]["line_spacing"] = odf_line_spacing(path)
            # 同一问在 ODF 一跳在样式里：一条 shorthand 顶四条边，「这边没有」是明写的
            out["odt"]["para_borders"] = odf_para_borders(path)
            # 同一问在 ODF 是一个 frame 套一个 text-box，尺寸是自带单位的串
            out["odt"]["text_boxes"] = odf_text_boxes(path)
            # 同一问在 ODF 是三种记号：一枚点，或一对按名字配的 start/end
            out["odt"]["bookmark_pairs"] = odf_bookmark_pairs(path)
            # 同一问在 ODF 写在页版式上，两份件都走
            out["odt"]["page_numbering"] = odf_page_numbering(path)
            # 同一问在 ODF 拆成 language + country（外加 script）
            out["odt"]["languages"] = odf_languages(path)
            # 同一问在 ODF 是一类注一份 configuration
            out["odt"]["note_settings"] = odf_note_settings(path)
            # 结构搬进 markdown：与 docx 那一本同一个键形状，只是层级与记号是另一族的写法
            out["odt"]["markdown"] = odf_markdown_ledger(path)
            # 同一问在 ODF 要跳进另一个部件：一条式子一个 Object N/content.xml 的 MathML
            out["odt"]["equations"] = odf_equations_ledger(path)
            ledger = odt_revision_ledger(path)
            if ledger is not None:
                out["revisions"] = ledger
            out["protection"] = protection_for(path)
            sheets = ods_facts(path)
            if sheets is not None:
                out["ods"] = sheets
                # 页版式（母版页）那六格：office-sheet 的 ODS 分支也交这一份，读者同一入口
                sheets["page_styles"] = odf_page_styles(path)
                # 打印范围：这一族写在表自己身上，另有一份为与 Excel 来回而留的 named-*
                sheets["print_ranges"] = ods_print_ranges(path)
                sheets["formula_elems"] = ods_formula_elems(path)
                out["csv"] = csv_facts(path)
                out["ods_styles"] = ods_styles(path)
            deck = odp_facts(path)
            if deck is not None:
                # 每页那张表的账（frame 那一份 + ODF 表那一份），按页一对一挂上去
                groups = odp_slide_tables(path)
                for which, slide in enumerate(deck["slides"]):
                    slide["table_list"] = groups[which] if which < len(groups) else []
                # 一条式子一个部件；页缩略图也是 frame，所以两格分开数
                deck["equations"] = odp_equations_ledger(path)
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
