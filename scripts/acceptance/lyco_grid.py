"""第二读者：一张 Word / ODT 表格的「网格」——每行几个格子、哪一格被合并掉了。

只用标准库。OOXML 走 `word/document.xml` 的 `w:tbl` → `w:tr` → `w:tc`（**直接孩子**），
ODF 走 `content.xml` 的 `table:table` → `table:table-row` →
`table:table-cell` / `table:covered-table-cell`（同样只走直接孩子）。

为什么只走直接孩子：`office-doc` 的 `tables[].rows` / `cells` 那两个人数是
`descendants` 数的（一张嵌在格子里的表会让外面那张把里面的行列一起算进去），
而「这张表自己长什么样」必须是另一本账 —— 两本账都交，才对得上「这张表几行」
与「这份文件里有几个行标记」是两个问题。

一格的字 = 它自己那几个段用换行拼起来（嵌在格子里的那张表的段不算这一格）。
合并与重复只交文件写了的：没写就是 null（不是 1）。
"""

import json
import pathlib
import zipfile
import xml.etree.ElementTree as ET

KEYS = ("text", "col_span", "row_span", "repeat", "row_merge", "covered", "paragraphs")


def local(tag: str) -> str:
    return tag.split("}")[-1]


def attr(node, want: str):
    for key, value in node.attrib.items():
        if local(key) == want:
            return value
    return None


def number(node, want: str):
    raw = attr(node, want)
    if raw is None:
        return None
    try:
        return int(str(raw).strip())
    except ValueError:
        # 写了但不是个数：不替文件猜一个
        return None


def direct(node, want):
    wants = (want,) if isinstance(want, str) else tuple(want)
    return [one for one in node if local(one.tag) in wants]


def docx_cell(tc) -> dict:
    props = None
    for one in tc.iter():
        if local(one.tag) == "tcPr":
            props = one
            break
    span = row_merge = None
    if props is not None:
        for one in props:
            if local(one.tag) == "gridSpan":
                span = number(one, "val")
            elif local(one.tag) == "vMerge":
                # OOXML 里 vMerge 不写值就是「接上面那一格」，这是文件的规矩不是我们的推断
                row_merge = attr(one, "val") or "continue"
    paras = direct(tc, "p")
    return {
        "text": docx_cell_text(paras),
        "col_span": span,
        "row_span": None,
        "repeat": None,
        "row_merge": row_merge,
        "covered": False,
        "paragraphs": len(paras),
    }


def docx_cell_text(paras) -> str:
    parts = []
    for one in paras:
        bits = []
        for kid in one.iter():
            name = local(kid.tag)
            if name == "t":
                bits.append(kid.text or "")
            elif name == "tab":
                bits.append("\t")
            elif name == "br":
                bits.append("\n")
        parts.append("".join(bits))
    return "\n".join(parts).strip()


def odf_cell(tc) -> dict:
    # ODF 一段的字就是它那棵子树里的文本拼起来（`text:span` 只是分块，不改变字）
    paras = direct(tc, ("p", "h"))
    parts = ["".join(one.itertext()) for one in paras]
    return {
        "text": "\n".join(parts).strip(),
        "col_span": number(tc, "number-columns-spanned"),
        "row_span": number(tc, "number-rows-spanned"),
        "repeat": number(tc, "number-columns-repeated"),
        "row_merge": None,
        "covered": local(tc.tag) == "covered-table-cell",
        "paragraphs": len(paras),
    }


def docx(path: pathlib.Path, limit: int = 100) -> list:
    with zipfile.ZipFile(path) as z:
        root = ET.fromstring(z.read("word/document.xml"))
    body = [one for one in root.iter() if local(one.tag) == "body"][0]
    out = []
    for tbl in direct(body, "tbl"):
        rows = []
        cut = False
        trs = direct(tbl, "tr")
        cut |= len(trs) > limit
        for tr in trs[:limit]:
            tcs = direct(tr, "tc")
            cut |= len(tcs) > limit
            rows.append([docx_cell(one) for one in tcs[:limit]])
        out.append({"rows": rows, "cut": cut})
    return out


def odt(path: pathlib.Path, limit: int = 100) -> list:
    with zipfile.ZipFile(path) as z:
        root = ET.fromstring(z.read("content.xml"))
    out = []
    for tbl in [one for one in root.iter() if local(one.tag) == "table"]:
        rows = []
        cut = False
        trs = direct(tbl, "table-row")
        cut |= len(trs) > limit
        for tr in trs[:limit]:
            tcs = direct(tr, ("table-cell", "covered-table-cell"))
            cut |= len(tcs) > limit
            rows.append([odf_cell(one) for one in tcs[:limit]])
        out.append({"rows": rows, "cut": cut})
    return out


def grids_of(path: pathlib.Path) -> list:
    name = path.name.lower()
    if name.endswith((".docx", ".docm")):
        return docx(path)
    if name.endswith((".odt", ".odp")):
        # odp 的表与 odt 的是同一棵树（table:table / table-cell / covered-table-cell），
        # 只有页容器不一样 —— 页归位由 office-slide 管，这里只管这张表自己长什么样
        return odt(path)
    return []


def odp_table_sizes(path: pathlib.Path) -> list:
    """一张 odp 页表的列宽与行高，换成 0.01mm。

    这一族与 .ods 同一类一跳：`table:table-column` 只点名样式，尺寸在
    `family=table-column` 的那份 `style:column-width` 上。这一份存在的理由是**对照**：
    同一张表，LibreOffice 在 odp 里把列宽写成 `7.62cm`，在 pptx 里写成 `2743200`（EMU），
    两边换算到 0.01mm 都是 7620 —— pptx 那本 EMU 账因此有一个不是自己的读者能对。
    """
    from lyco_pages import convert

    with zipfile.ZipFile(path) as z:
        root = ET.fromstring(z.read("content.xml"))
    sized = {}
    for st in [one for one in root.iter() if local(one.tag) == "style"]:
        family = attr(st, "family")
        if family not in ("table-column", "table-row"):
            continue
        want = "column-width" if family == "table-column" else "row-height"
        holder = "table-column-properties" if family == "table-column" else "table-row-properties"
        props = direct(st, (holder,))
        sized[attr(st, "name")] = (
            convert(attr(props[0], want), None)[0] if props else None
        )
    out = []
    for tbl in [one for one in root.iter() if local(one.tag) == "table"]:
        out.append(
            {
                "columns": [sized.get(attr(one, "style-name")) for one in direct(tbl, "table-column")],
                "rows": [sized.get(attr(one, "style-name")) for one in direct(tbl, "table-row")],
            }
        )
    return out


if __name__ == "__main__":
    import sys

    for arg in sys.argv[1:]:
        print(arg, json.dumps(grids_of(pathlib.Path(arg)), ensure_ascii=False))
