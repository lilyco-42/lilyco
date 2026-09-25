"""文档里的表铺成 CSV 的第二读者：与 `lilyco-binfmt/src/table_grid.rs::csv_of` 同一条判据。

三件事必须一致，都在这里独立做一遍：
- **表是哪几张**：`w:tbl` / `table:table` 按 `descendants`（文档顺序，嵌套表也算一张）；
- **一行几格**：行与格都走**直接孩子**（`w:tr`/`w:tc`；ODF 是 `table-row` 与
  `table-cell` + `covered-table-cell`），一格的字 = 它自己那几个段用 `\\n` 拼起来再 strip，
  嵌在格子里的那张表的段不算这一格；
- **不补方格**：OOXML 把横向合并那一格整个不写，ODF 照样写一枚空占位格，所以同一张
  视觉上的表在两家的 `columns_per_row` 可以不一样长 —— 引法用 RFC4180，与 office-sheet
  那本同一条（带逗号/引号/换行就整体加引号，里面的引号翻倍）。
"""
import zipfile
from pathlib import Path
from xml.etree import ElementTree as ET

LIMIT = 100


def local(tag: str) -> str:
    return tag.rsplit("}", 1)[-1] if "}" in tag else tag


def kids(node, *want):
    """直接孩子里名字在 `want` 里的那些（Rust 的 `all` / `is` 同一条：全名或局部名都算）"""
    return [one for one in node if local(one.tag) in want]


def docx_para_text(node) -> str:
    """一段的字：`w:t` 的后代按文档顺序拼（与 `office_text.rs::paragraph_text` 同一条）"""
    return "".join(one.text or "" for one in node.iter() if local(one.tag) == "t")


def odf_para_text(node) -> str:
    """ODF 一段：空格与制表是**记号**不是字符，`text:s` 按自己的 `c` 展开（同 Rust 那本）"""
    out = []
    for one in node.iter():
        name = local(one.tag)
        if name == "s":
            count = one.get("c") or one.get("{urn:oasis:names:tc:opendocument:xmlns:text:1.0}c")
            out.append(" " * int(count) if count else " ")
        elif name == "tab":
            out.append("\t")
        elif name == "line-break":
            out.append("\n")
        elif one.text:
            out.append(one.text)
        if one.tail:
            out.append(one.tail)
    return "".join(out)


def csv_field(raw: str) -> str:
    if any(ch in raw for ch in ('"', ",", "\n", "\r")):
        return '"%s"' % raw.replace('"', '""')
    return raw


def text_of(parts):
    return "\n".join(parts).strip()


def docx_grids(path: Path, limit: int = LIMIT):
    with zipfile.ZipFile(path) as box:
        root = ET.fromstring(box.read("word/document.xml"))
    body = kids(root, "body")
    holder = body[0] if body else root
    out = []
    for tbl in [one for one in holder.iter() if local(one.tag) == "tbl"][:limit]:
        rows = []
        cut = len(kids(tbl, "tr")) > limit
        for tr in kids(tbl, "tr")[:limit]:
            tcs = kids(tr, "tc")
            cut |= len(tcs) > limit
            cells = []
            for tc in tcs[:limit]:
                parts = [docx_para_text(one) for one in kids(tc, "p")]
                cells.append({"text": text_of(parts), "covered": False})
            rows.append(cells)
        out.append({"rows": rows, "cut": cut})
    return out


def odf_grids(path: Path, limit: int = LIMIT):
    with zipfile.ZipFile(path) as box:
        root = ET.fromstring(box.read("content.xml"))
    tables = [one for one in root.iter() if local(one.tag) == "table"]
    out = []
    for tbl in tables[:limit]:
        rows = []
        cut = len(kids(tbl, "table-row")) > limit
        for tr in kids(tbl, "table-row")[:limit]:
            tcs = kids(tr, "table-cell", "covered-table-cell")
            cut |= len(tcs) > limit
            cells = []
            for tc in tcs[:limit]:
                parts = [odf_para_text(one) for one in kids(tc, "p", "h")]
                cells.append({"text": text_of(parts),
                              "covered": local(tc.tag) == "covered-table-cell"})
            rows.append(cells)
        out.append({"rows": rows, "cut": cut})
    return out


def doc_csv(grids, pick: str = ""):
    """把第 pick 张表铺成 CSV（与 `csv_of` 同一组键、同一个 pick 语义）"""
    pick = (pick or "").strip()
    if not pick:
        index = 0
    else:
        try:
            index = int(pick)
        except ValueError:
            return {"error": "--table 要的是从 0 起的序号，收到「%s」" % pick}
    if index >= len(grids) or index < 0:
        return {"error": "这份文件里没有第 %d 张表（一共 %d 张）" % (index, len(grids))}
    grid = grids[index]
    lines, widths, empty_cells, covered_cells = [], [], 0, 0
    for cells in grid["rows"]:
        fields = []
        for one in cells:
            if one["covered"]:
                covered_cells += 1
            if not one["text"]:
                empty_cells += 1
            fields.append(one["text"])
        widths.append(len(fields))
        lines.append(",".join(csv_field(one) for one in fields))
    text = "".join(one + "\n" for one in lines)
    columns = max(widths) if widths else 0
    return {
        "table": index,
        "tables_total": len(grids),
        "rows": len(widths),
        "columns": columns,
        "columns_per_row": widths,
        "ragged": any(one != columns for one in widths),
        "empty_cells": empty_cells,
        "covered_cells": covered_cells,
        "cut": grid["cut"],
        "line_end": "LF",
        "text": text,
    }
