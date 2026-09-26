"""放映那两族的 markdown 第二读者：`office-text --markdown` 的 pptx 与 odp 两支。

与 `lilyco-binfmt/src/markdown.rs` 的那两支同一条判据。三条要紧的口径都写在下面，
两条读者各按自己文件写着的做，不互相借：

- **一页一个 `#`**：标题来自那一族自己写的那句话 —— pptx 是 `p:ph/@type=title|ctrTitle`，
  odp 是 `presentation:class=title`；一页没写标题就整个不出这一行，只记 `titles_missing`；
- **条目标不标是文件说的**：python-pptx 连 `a:pPr` 都不写（实测两份件各 0 条），
  LibreOffice 重写同一份稿子时给两条写了 `a:buChar`、给两条写了 `a:buNone`；
  odp 走另一套 —— 条目住在 `text:list` / `text:list-item` 里，而标题那一格不在表格里。
  于是 `bullets_written` / `bullets_denied` / `bullets_silent` 三本分开数；
- **备注不进 markdown**：那不是页面上给观众看的字，条数在 `notes_pages` 里，
  这一族另有 `office-slide` 那份账管它。
"""
import zipfile
from pathlib import Path
from xml.etree import ElementTree as ET

from lyco_markdown import (  # 同一条渲染规矩只写一遍：转义、拼接、空白处理
    HARD,
    LEAD,
    OFF,
    kids,
    local,
    odf_segments_collect,
    odf_styles,
    odf_table_md,
    rels_of,
    render,
)

REL_NS = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"


def attr_ns(node, want: str):
    """按局部名取属性：前缀是文件自己声明的，不假设是谁的"""
    for key, value in node.attrib.items():
        if local(key) == want:
            return value
    return None


# ── pptx ─────────────────────────────────────────────────────────────────────
def pptx_segments(para, rels: dict) -> list:
    """DrawingML 一段的片段。与 docx 那一本不同处的三件事都按这一族写的来：

    - 粗体与斜体是 `a:rPr` **身上的属性**（`b="1"`），不是 docx 那种孩子元素；
    - `a:br` 与 `a:tab` 是段里的**独立元素**（不在 run 里也要还原，不然一个字都读不出）；
    - 链接在 `a:rPr/a:hlinkClick/@r:id`，地址仍在这一页自己的关系表里（两跳）。
    """
    out: list = []

    def on(props, key):
        value = props.attrib.get(key)
        return value is not None and str(value).lower() not in OFF

    for one in para:
        name = local(one.tag)
        if name == "pPr":
            continue
        if name == "br":
            out.append({"text": HARD, "bold": False, "italic": False, "link": None})
            continue
        if name == "tab":
            out.append({"text": " ", "bold": False, "italic": False, "link": None})
            continue
        if name != "r":
            continue
        props = kids(one, "rPr")
        holder = props[0] if props else None
        link = None
        if holder is not None:
            hit = [kid for kid in holder if local(kid.tag) == "hlinkClick"]
            if hit:
                rid = None
                for key, value in hit[0].attrib.items():
                    # 号是 `r:id`：名字对、namespace 也要对（这一族的 id 各家自己编）
                    if local(key) == "id" and key.startswith("{") and "relationships" in key:
                        rid = value
                link = rels.get(rid, "") if rid else ""
        text = "".join(node.text or "" for node in one.iter() if local(node.tag) == "t")
        if text:
            out.append({
                "text": text,
                "bold": bool(holder is not None and on(holder, "b")),
                "italic": bool(holder is not None and on(holder, "i")),
                "link": link,
            })
    return out


def pptx_cell_text(tc, rels: dict) -> str:
    """一个格子的字：多段用 <br> 连，竖线在这里才转义（与 docx 那一条同一个待遇）"""
    bits = []
    for body in kids(tc, "txBody"):
        for par in kids(body, "p"):
            flat = render(pptx_segments(par, rels), pipe=True).strip().replace("\n", "<br>")
            if flat:
                bits.append(flat)
    return "<br>".join(bits)


def pptx_table_md(tbl, rels: dict) -> str:
    rows = [[pptx_cell_text(one, rels) for one in kids(tr, "tc")] for tr in kids(tbl, "tr")]
    if not rows:
        return ""
    width = max(len(one) for one in rows)
    for one in rows:
        one.extend([""] * (width - len(one)))
    lines = ["| " + " | ".join(rows[0]) + " |", "| " + " | ".join(["---"] * width) + " |"]
    for one in rows[1:]:
        lines.append("| " + " | ".join(one) + " |")
    return "\n".join(lines)


def show_order(parts: dict) -> list:
    """`presentation.xml` 的 sldId 顺序：部件文件名不是放映顺序"""
    if "ppt/presentation.xml" not in parts:
        return []
    pres = ET.fromstring(parts["ppt/presentation.xml"])
    rels_root = ET.fromstring(parts.get("ppt/_rels/presentation.xml.rels", b"<a/>"))
    id2part = {}
    for one in rels_root.iter():
        if local(one.tag) == "Relationship":
            target = one.get("Target", "")
            # Target 是**相对于 `ppt/` 这个部件所在目录**写的：本语料两家都写
            # `slides/slideN.xml`（这一条是刚踩出来的：只把 `../` 那种接上 `ppt/`，
            # 解出来的名字指不到任何部件，于是整份放映一页也读不到）。
            # 绝对写法与本仓没见过的 `../` 写法一并接着解，认不出的仍按原样交
            if target.startswith("/"):
                resolved = target[1:]
            elif target.startswith("../"):
                resolved = "ppt/" + target[3:]
            else:
                resolved = "ppt/" + target
            id2part[one.get("Id")] = resolved
    order = []
    for one in pres.iter():
        if local(one.tag) != "sldId":
            continue
        for key, value in one.attrib.items():
            if "relationships" in key and local(key) == "id":
                order.append(id2part.get(value))
    return [one for one in order if one]


def pptx_deck_markdown(path: Path, budget: int = 20000) -> dict:
    with zipfile.ZipFile(path) as box:
        parts = {one.filename: box.read(one.filename) for one in box.infolist()}
    if not any(one.startswith("ppt/slides/slide") for one in parts):
        return {"family": "pptx", "available": False}
    order = show_order(parts) or sorted(
        one for one in parts if one.startswith("ppt/slides/slide") and one.endswith(".xml"))
    blocks: list = []
    stats = {
        "pages": 0, "titles": 0, "titles_missing": 0, "paragraphs": 0, "headings": 0,
        "bullets_written": 0, "bullets_denied": 0, "bullets_silent": 0,
        "tables": 0, "table_rows": 0, "empty_dropped": 0, "notes_pages": 0,
        "links": 0, "pictures": 0, "levels_written": 0,
    }
    for name in order:
        if name not in parts:
            continue
        root = ET.fromstring(parts[name])
        page_rels = rels_of(parts, name)
        stats["pages"] += 1
        note_part = name.replace("/slides/slide", "/notesSlides/notesSlide")
        if note_part in parts:
            stats["notes_pages"] += 1
        title_done = False
        for shape in [one for one in root.iter()
                      if local(one.tag) in ("sp", "graphicFrame", "pic")]:
            for one in shape.iter():
                tag = local(one.tag)
                if tag == "hlinkClick":
                    stats["links"] += 1
                elif tag == "blip":
                    stats["pictures"] += 1
            if local(shape.tag) == "pic":
                continue
            ph = None
            for one in shape.iter():
                if local(one.tag) == "ph":
                    ph = attr_ns(one, "type")
                    break
            bodies = kids(shape, "txBody")
            for tbl in [one for one in shape.iter() if local(one.tag) == "tbl"]:
                made = pptx_table_md(tbl, page_rels)
                if made:
                    stats["tables"] += 1
                    stats["table_rows"] += len(kids(tbl, "tr"))
                    blocks.append((False, made))
            for body in bodies:
                for par in kids(body, "p"):
                    text = render(pptx_segments(par, page_rels)).strip()
                    ppr = kids(par, "pPr")
                    if ppr and attr_ns(ppr[0], "lvl") is not None:
                        stats["levels_written"] += 1
                    if not text:
                        stats["empty_dropped"] += 1
                        continue
                    if ph in ("title", "ctrTitle") and not title_done:
                        title_done = True
                        stats["titles"] += 1
                        blocks.append((False, "# " + text))
                        continue
                    marker = None
                    if ppr:
                        if kids(ppr[0], "buChar"):
                            marker = "char"
                        elif kids(ppr[0], "buAutoNum"):
                            marker = "auto"
                        elif kids(ppr[0], "buNone"):
                            marker = "none"
                    if marker == "none":
                        stats["bullets_denied"] += 1
                    elif marker:
                        stats["bullets_written"] += 1
                    else:
                        stats["bullets_silent"] += 1
                    lead = {"char": "- ", "auto": "1. "}.get(marker, "")
                    blocks.append((bool(lead),
                                   lead + ("\\" + text if LEAD.match(text) else text)))
                    stats["paragraphs"] += 1
        if not title_done:
            stats["titles_missing"] += 1
    pieces = []
    for index, (is_list, block) in enumerate(blocks):
        if index:
            pieces.append("\n" if is_list and blocks[index - 1][0] else "\n\n")
        pieces.append(block)
    text = ("".join(pieces) + "\n") if blocks else ""
    chars = len(text)
    return {
        "family": "pptx", "available": True, "text": text[:budget], "chars": chars,
        "cut": chars > budget, "blocks": len(blocks), **stats,
    }


# ── odp ──────────────────────────────────────────────────────────────────────
def _class_of(node) -> str:
    """`presentation:class` 按局部名取（这一族的前缀是文件自己起的）"""
    return attr_ns(node, "class") or ""


def _md_local(tag: str) -> str:
    return tag.rsplit("}", 1)[-1]


def odp_deck_markdown(path: Path, budget: int = 20000) -> dict:
    """一份 odp 的页级大纲。层级与条目标记在这一族是**元素**不是属性：

    - 标题：`draw:frame` 上的 `presentation:class=title`，里面那一段的第一句就是标题；
      一页里可以一个都没有（实测 `deck-tr.odp`、`eqs.odp` 那几页），那就整页不出 `#`；
    - 条目：段住在 `text:list` > `text:list-item` 里，深度是**嵌套层数**
      （`a:pPr/@lvl` 那一族在这两份件里一个字都没写，见 pptx 那一支的三本账）；
    - 表与 .odt 是同一棵树，直接借 `odf_table_md`（被盖住那一格照样是一格）；
    - 备注住在 `presentation:notes` 里那一页 —— 那不是给观众看的字，整棵跳过，
      只记 `notes_pages`（与 `office-slide` 那条同一个判据：框里的 `<编号>` 也不算正文）。
    """
    with zipfile.ZipFile(path) as box:
        parts = {one.filename: box.read(one.filename) for one in box.infolist()}
    if "content.xml" not in parts:
        return {"family": "odp", "available": False}
    root = ET.fromstring(parts["content.xml"])
    chars, lists = odf_styles(parts)
    ctx = {"rels": {}, "styles": {}, "chars": chars, "lists": lists,
           "stats": {"covered_cells": 0, "repeated_spans": 0, "spans_unresolved": 0,
                     "annotations_dropped": 0, "notes_dropped": 0, "space_markers": 0,
                     "links": 0, "images": 0}}
    blocks: list = []
    stats = {
        "pages": 0, "titles": 0, "titles_missing": 0, "paragraphs": 0, "headings": 0,
        "bullets_written": 0, "bullets_denied": 0, "bullets_silent": 0,
        "tables": 0, "table_rows": 0, "empty_dropped": 0, "notes_pages": 0,
        "levels_written": 0, "covered_cells": 0, "repeated_spans": 0,
        "links": 0, "pictures": 0,
    }
    pages = [one for one in root.iter() if _md_local(one.tag) == "page"]
    for page in pages:
        inside_notes = {id(x) for holder in page
                        if _md_local(holder.tag) == "notes" for x in holder.iter()}
        if any(_md_local(kid.tag) == "notes" for kid in page):
            stats["notes_pages"] += 1
        stats["pages"] += 1
        title_done = False
        for kid in page:
            if _md_local(kid.tag) not in ("frame", "custom-shape", "image", "group"):
                continue
            klass = ""
            for one in kid.iter():
                if _md_local(one.tag) in ("frame", "custom-shape"):
                    klass = klass or _class_of(one)
                if _md_local(one.tag) == "image":
                    stats["pictures"] += 1
            for holder in kid.iter():
                if _md_local(holder.tag) == "table":
                    made = odf_table_md(holder, ctx)
                    if made:
                        stats["tables"] += 1
                        stats["table_rows"] += len([x for x in holder
                                                    if _md_local(x.tag) == "table-row"])
                        blocks.append((False, made))
            # 表里的段、嵌入对象与图里的字都不再当页上的段交一遍：
            # 表已经由 `odf_table_md` 整张交出去了，重复交会把同一句字排两次
            skip = {id(inner)
                    for holder in kid.iter()
                    if _md_local(holder.tag) in ("table", "object", "image", "control")
                    for inner in holder.iter()}
            paragraphs = [one for one in kid.iter()
                          if _md_local(one.tag) in ("p", "h")
                          and id(one) not in inside_notes and id(one) not in skip]
            listed = {id(one) for lst in kid.iter() if _md_local(lst.tag) == "list"
                      for one in lst.iter() if _md_local(one.tag) == "p"}
            for par in paragraphs:
                text = render(odf_segments_collect(par, ctx)).strip()
                if not text:
                    stats["empty_dropped"] += 1
                    continue
                if _class_of(kid) == "title" and not title_done:
                    title_done = True
                    stats["titles"] += 1
                    blocks.append((False, "# " + text))
                    continue
                if _md_local(par.tag) == "h":
                    stats["headings"] += 1
                    level = attr_ns(par, "outline-level")
                    if level is not None:
                        stats["levels_written"] += 1
                    depth = int(level) if (level or "").isdigit() else 1
                    blocks.append((False, "#" * min(depth + 1, 6) + " " + text))
                    continue
                in_list = id(par) in listed
                if in_list:
                    stats["bullets_written"] += 1
                else:
                    stats["bullets_silent"] += 1
                lead = "- " if in_list else ""
                blocks.append((in_list,
                               lead + ("\\" + text if LEAD.match(text) else text)))
                stats["paragraphs"] += 1
        if not title_done:
            stats["titles_missing"] += 1
    stats["covered_cells"] = ctx["stats"]["covered_cells"]
    stats["repeated_spans"] = ctx["stats"]["repeated_spans"]
    stats["links"] = ctx["stats"]["links"]
    pieces = []
    for index, (is_list, block) in enumerate(blocks):
        if index:
            pieces.append("\n" if is_list and blocks[index - 1][0] else "\n\n")
        pieces.append(block)
    text = ("".join(pieces) + "\n") if blocks else ""
    chars = len(text)
    return {
        "family": "odp", "available": True, "text": text[:budget], "chars": chars,
        "cut": chars > budget, "blocks": len(blocks), **stats,
    }
