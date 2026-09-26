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
    """一段的字：与 Rust 的 `office_text::paragraph_text` = `run_text` 同一条 ——
    按文档顺序拼每个节点的自有字与孩子的尾巴（xmlscan 把 `#text` 当字存，所以
    「前 `<t>中</t>` 后」两边都读成「前中后」），`w:tab` 是一个制表、`w:br` / `w:cr`
    是一个换行，最后**整段 trim**（Rust 那本每段都 trim，不是只把拼完的整格 trim）。
    段读者直接借 `office_reader.ooxml_para_text`：那是这条判据的第二份实现，不另写一遍。
    """
    from office_reader import ooxml_para_text  # 晚一点引：本模块是被它 import 的那一个

    return ooxml_para_text(node).strip()


def odf_para_text(node) -> str:
    """ODF 一段：与 Rust 的 `odf_paragraph_text` = `text_skipping(node, "annotation")` 同一条。

    三件事都照那本做，不照「页面上看到什么」自己补：
    - **`text:annotation` 整棵子树跳过**（注里的段不是这一格的字）；
    - `text:tab` 是一个制表（Rust 那边认的名字就是 `tab`），而 **`text:s` 与
      `text:line-break` 不展开** —— 网格这一本交的是文件写成字符的那些字，
      记号要展开是 `office-text --markdown` 那一族的事（`lyco_markdown.py` 那一份读者）；
    - 整段 **trim**（与 docx 那一条同一个待遇）。
    """
    out: list = []

    def keep(chunk: str) -> bool:
        """纯空白且带换行的是 pretty-print 的缩进噪声；不带换行的空白是文件写的字"""
        if not chunk:
            return False
        return not (chunk.strip() == "" and ("\n" in chunk or "\r" in chunk))

    def walk(one, top: bool = False):
        name = local(one.tag)
        if not top and name == "annotation":
            if one.tail and keep(one.tail):
                out.append(one.tail)  # 注后面那段尾巴字仍属于这一段
            return
        if name == "tab":
            out.append("\t")
        elif one.text and keep(one.text):
            out.append(one.text)
        for kid in one:
            walk(kid)
        if one.tail and keep(one.tail):
            out.append(one.tail)

    walk(node, top=True)
    return "".join(out).strip()



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


def odf_grid(tbl, limit: int = LIMIT) -> dict:
    """一张 ODF 表（`table:table`）→ 网格：行与格都走直接孩子，被盖住的那一格照样算一格"""
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
    return {"rows": rows, "cut": cut}


def odf_grids(path: Path, limit: int = LIMIT):
    with zipfile.ZipFile(path) as box:
        root = ET.fromstring(box.read("content.xml"))
    tables = [one for one in root.iter() if local(one.tag) == "table"]
    return [odf_grid(one, limit) for one in tables[:limit]]


def doc_csv(grids, pick: str = "", at: str = "这份文件里"):
    """把第 pick 张表铺成 CSV（与 `csv_of` 同一组键、同一个 pick 语义）

    `at` 是那句「没有第几张表」说清在哪儿数过：文档那一族是「这份文件里」，
    放映那一族是「这一页（部件名）里」——与 Rust 的 `csv_of(want, grids, pick, at)` 同一条。
    """
    pick = (pick or "").strip()
    if not pick:
        index = 0
    else:
        try:
            index = int(pick)
        except ValueError:
            return {"error": "--table 要的是从 0 起的序号，收到「%s」" % pick}
    if index >= len(grids) or index < 0:
        return {"error": "%s没有第 %d 张表（一共 %d 张）" % (at, index, len(grids))}
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


# ── 放映那一族：一页可以有几张表，页是挑的第一层 ──────────────────────────────────
def pptx_grid(tbl, limit: int = LIMIT) -> dict:
    """一张 `a:tbl` → 网格。这一族的合并是**第三种写法**（事实 60）：
    被盖住的那一格照样在场，身上写 `a:hMerge` / `a:vMerge` 而字是空的 ——
    所以 `covered` 按「身上写了那两条之一」判，不看字空不空（那是另一本账 `empty_cells`）"""
    from office_reader import ooxml_para_text  # 晚一点引：office_reader 顶层就 import 本模块

    rows = []
    cut = len(kids(tbl, "tr")) > limit
    for tr in kids(tbl, "tr")[:limit]:
        tcs = kids(tr, "tc")
        cut |= len(tcs) > limit
        cells = []
        for tc in tcs[:limit]:
            bodies = kids(tc, "txBody")
            parts = [ooxml_para_text(one).strip() for one in kids(bodies[0], "p")] if bodies else []
            merge = [one for one in ("hMerge", "vMerge")
                     if any(local(key) == one for key in tc.attrib)]
            cells.append({"text": text_of(parts), "covered": bool(merge)})
        rows.append(cells)
    return {"rows": rows, "cut": cut}


def pptx_page_grids(raw: bytes, limit: int = LIMIT) -> list:
    """一页部件（`ppt/slides/slideN.xml` 的字节）里那些表的网格，按文档顺序"""
    root = ET.fromstring(raw)
    tables = [one for one in root.iter() if local(one.tag) == "tbl"]
    return [pptx_grid(one, limit) for one in tables[:limit]]


def odp_page_grids(page, limit: int = LIMIT) -> list:
    """一页 `draw:page` 里那些表的网格：与 .ods / .odt 同一个 `odf_grid`（合并另写占位格）"""
    tables = [one for one in page.iter() if local(one.tag) == "table"]
    return [odf_grid(one, limit) for one in tables[:limit]]


def page_csv(grids, limit: int = LIMIT) -> list:
    """这一页每张表一份 CSV 账（按文档顺序）；一页没有表就是空表，不是缺键。

    `limit` 只影响上面那几位建网格的人（`pptx_page_grids` / `odp_page_grids`），
    这里只按已有的网格逐张铺 —— 与被挑中的那张表一共几张，是网格那一层决定的
    """
    return [doc_csv(grids, str(one)) for one in range(len(grids))]
