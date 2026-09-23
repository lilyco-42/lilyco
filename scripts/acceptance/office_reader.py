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
from lyco_rtf import rtf_text  # 独立 RTF 实现，与 lilyco-binfmt/src/rtf.rs 对账

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
        "sections": len([one for one in body.iter() if xml_local(one.tag) == "sectPr"]),
        "has_numbering": "word/numbering.xml" in parts,
        "has_settings": "word/settings.xml" in parts,
        "has_styles_part": "word/styles.xml" in parts,
        "has_font_table": "word/fontTable.xml" in parts,
        "footnotes": len([
            one
            for one in (ET.fromstring(parts["word/footnotes.xml"]).iter()
                        if "word/footnotes.xml" in parts else [])
            if xml_local(one.tag) == "footnote"
        ]),
        "endnotes": len([
            one
            for one in (ET.fromstring(parts["word/endnotes.xml"]).iter()
                        if "word/endnotes.xml" in parts else [])
            if xml_local(one.tag) == "endnote"
        ]),
        "text": "\n".join(one for one in paragraphs if one),
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
        rels = parts.get(f"{stem}.rels")
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


def odt_facts(path: Path) -> dict:
    parts = {}
    with zipfile.ZipFile(path) as box:
        names = [one.filename for one in box.infolist()]
        parts = {one.filename: box.read(one.filename) for one in box.infolist()}
    root = ET.fromstring(parts["content.xml"])
    paras = [one for one in root.iter() if xml_local(one.tag) == "p"]
    heads = [one for one in root.iter() if xml_local(one.tag) == "h"]
    tables = [one for one in root.iter() if xml_local(one.tag) == "table"]
    metas = {}
    if "meta.xml" in parts:
        m = ET.fromstring(parts["meta.xml"])
        for one in m.iter():
            name = xml_local(one.tag)
            if one.text and one.text.strip():
                metas.setdefault(name, one.text.strip())
    return {
        "paragraph_count": len(paras),
        "headings": ["".join(one.itertext()) for one in heads],
        "tables": len(tables),
        "text": "\n".join("".join(one.itertext()) for one in paras),
        "media": sorted(one for one in names if one.startswith("Pictures/")),
        "meta": metas,
        "parts": sorted(names),
    }


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

    返回 `{meta..., "entries": [...], "streams": {name: bytes}}` —— 元数据视图与字节视图
    出自**同一次**解析。写两份链遍历就是给自己留「两份不一致」的坑。
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
        out["container"] = "cfb"
        out["cfb"] = cfb
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
        elif "ppt/presentation.xml" in parts:
            out["app"] = "powerpoint"
            out["ooxml"] = pptx_facts(path)
        elif "content.xml" in parts:
            out["app"] = "opendocument"
            out["odf"] = odt_facts(path)
        else:
            out["app"] = "unknown-zip"
        return out
    if data[:5] == b"{\\rtf":
        out["container"] = "rtf"
        out["rtf"] = rtf_text(data)
        out["app"] = "word"
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
            ".doc", ".docx", ".xls", ".xlsx", ".ppt", ".pptx", ".odt", ".ods", ".odp", ".rtf", ".docm", ".dotx", ".xltm", ".xlsm", ".potx",
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
