"""修订（tracked changes）这一份账的第二读者：只用标准库 ElementTree。

与 `lilyco-binfmt/src/revise.rs` 是同一套规则的两份实现，但走法不同：
Rust 那边是自己那棵容错 XML 树（`#text` 子节点）按顺序走，这里用 ElementTree 的
`.text` / `.tail` —— 尾巴文本天然带顺序，正好是「插入的字夹在两个标记之间」那一形态。

规则（两边都一样，且是有出处实测的，见 fixture README）：
* 元素计数照实数；**相邻**且同（类型 + 作者 + 时间 + 所在段 + 是否段落标记）的元素
  合成一条逻辑改动 —— LibreOffice 写 OOXML 会把「124000 元」拆成两个 `w:ins`，
  而它自己导出的 ODF 把同一次编辑写回一个 `text:changed-region`。
* 段落标记的修订（`w:pPr/w:rPr/w:ins`）与段里正文那条不是一回事，不合并。
* ODF 里 region 自己带着删掉的字，插入的字在正文 `change-start` 与 `change-end` 之间。
"""

from __future__ import annotations

import xml.etree.ElementTree as ET

DOCX_KINDS = {
    "ins": "insertion",
    "cellIns": "insertion",
    "rowIns": "insertion",
    "del": "deletion",
    "cellDel": "deletion",
    "rowDel": "deletion",
    "moveFrom": "move",
    "moveTo": "move",
    "rPrChange": "format-change",
    "pPrChange": "format-change",
    "tblPrChange": "format-change",
    "trPrChange": "format-change",
    "tcPrChange": "format-change",
    "sectPrChange": "format-change",
    "numberingChange": "format-change",
    "cellMerge": "format-change",
}
ODF_KINDS = {
    "insertion": "insertion",
    "deletion": "deletion",
    "format-change": "format-change",
    "move-from": "move",
    "move-to": "move",
}


def _local(tag: str) -> str:
    return tag.split("}")[1] if "}" in tag else tag


def _attr(el, name: str) -> str:
    """按局部名取属性（OOXML 的 `w:` 与 ODF 的 `text:` 前缀都是文件自己声明的）"""
    for key, value in el.attrib.items():
        if _local(key) == name:
            return value
    return ""


def _revision_text(el) -> str:
    """一段修订带进来的字：只收 w:t 与 w:delText，按文档顺序"""
    out = []
    for node in el.iter():
        if _local(node.tag) in ("t", "delText") and node.text:
            out.append(node.text)
    return "".join(out)


def _own_text(el, skip: str) -> str:
    """region 里自己的字，但要跳过 change-info（作者与时间坐在那段字中间）"""
    out = []
    for node in el.iter():
        if _local(node.tag) == skip:
            continue
        if _local(node.tag) in ("p", "span", "s") and node.text:
            out.append(node.text)
    return "".join(out)


def docx_revisions(document: ET.Element, settings: ET.Element | None) -> dict:
    body = next((one for one in document.iter() if _local(one.tag) == "body"), None)
    paragraphs = [one for one in (body or document).iter() if _local(one.tag) == "p"]
    raws: list = []
    for index, para in enumerate(paragraphs):
        # 段落自己的子树，按文档顺序走，但不进嵌套的 w:p（它有自己的序号）
        ordered: list = []

        def walk(node, mark: bool) -> None:
            for child in node:
                name = _local(child.tag)
                if name == "p":
                    continue
                kind = DOCX_KINDS.get(name)
                if kind is not None:
                    ordered.append(
                        (
                            kind,
                            _attr(child, "author"),
                            _attr(child, "date"),
                            "" if kind == "format-change" else _revision_text(child),
                            mark,
                        )
                    )
                walk(child, mark or name == "pPr")

        walk(para, False)
        for kind, author, date, text, mark in ordered:
            raws.append(
                {
                    "kind": kind,
                    "author": author,
                    "date": date,
                    "text": text,
                    "paragraph": index,
                    "paragraph_mark": mark,
                }
            )
    ledger = _ledger(raws)
    if settings is None:
        ledger["track_changes"] = None
    else:
        ledger["track_changes"] = any(
            _local(one.tag) == "trackChanges" for one in settings.iter()
        )
    return ledger


def odt_revisions(content: ET.Element) -> dict | None:
    """ODF：region 是账，正文里的标记定位置；插入的字取 change-start 到 change-end 的尾巴

    不是文字文档（ods / odp 的 content.xml 没有 office:text）就交回 None。
    """
    text_body = None
    for one in content.iter():
        if _local(one.tag) != "body":
            continue
        text_body = next((kid for kid in one if _local(kid.tag) == "text"), None)
        break
    if text_body is None:
        return None
    tracked = next(
        (one for one in text_body if _local(one.tag) == "tracked-changes"), None
    )
    paragraphs = []
    skip = {"tracked-changes", "annotation"}

    def collect(node) -> None:
        for child in node:
            if _local(child.tag) in skip:
                continue
            if _local(child.tag) == "p":
                paragraphs.append(child)
                continue
            collect(child)

    collect(text_body)

    at: dict = {}
    ranges: dict = {}

    def walk_body(node, index: int, open_ids: list) -> None:
        name = _local(node.tag)
        if name in ("change", "change-start", "change-end"):
            change_id = _attr(node, "change-id")
            if change_id:
                at.setdefault(change_id, index)
                if name == "change-start":
                    ranges.setdefault(change_id, [])
                    open_ids.append(change_id)
                elif name == "change-end" and change_id in open_ids:
                    open_ids.remove(change_id)
        if node.text:
            for change_id in open_ids:
                ranges[change_id].append(node.text)
        if node.tail:
            for change_id in open_ids:
                ranges[change_id].append(node.tail)
        for child in node:
            walk_body(child, index, open_ids)

    for index, para in enumerate(paragraphs):
        walk_body(para, index, [])

    raws: list = []
    for region in text_body.iter():
        if _local(region.tag) != "changed-region":
            continue
        body = next(
            (one for one in region if _local(one.tag) in ODF_KINDS), None
        )
        if body is None:
            continue
        change_id = _attr(region, "id")
        info = next((one for one in region.iter() if _local(one.tag) == "change-info"), None)
        author = ""
        date = ""
        if info is not None:
            creator = next((one for one in info.iter() if _local(one.tag) == "creator"), None)
            when = next((one for one in info.iter() if _local(one.tag) == "date"), None)
            author = (creator.text or "") if creator is not None else ""
            date = (when.text or "") if when is not None else ""
        own = _own_text(body, "change-info")
        raws.append(
            {
                "kind": ODF_KINDS[_local(body.tag)],
                "author": author,
                "date": date,
                "text": own if own else "".join(ranges.get(change_id, [])),
                "paragraph": at.get(change_id),
                "paragraph_mark": False,
            }
        )
    raws.sort(key=lambda one: (one["paragraph"] if one["paragraph"] is not None else 1 << 40))
    out = _ledger(raws)
    out["track_changes"] = (
        None if tracked is None else _attr(tracked, "track-changes") == "true"
    )
    return out


def _ledger(raws: list) -> dict:
    counts = {"insertion": 0, "deletion": 0, "format-change": 0, "move": 0}
    marks = 0
    changes: list = []
    for one in raws:
        counts[one["kind"]] += 1
        if one["paragraph_mark"]:
            marks += 1
        last = changes[-1] if changes else None
        if (
            last is not None
            and last["kind"] == one["kind"]
            and last["author"] == one["author"]
            and last["date"] == one["date"]
            and last["paragraph"] == one["paragraph"]
            and last["paragraph_mark"] == one["paragraph_mark"]
        ):
            last["elements"] += 1
            last["text"] += one["text"]
            continue
        changes.append(
            {
                "kind": one["kind"],
                "author": one["author"],
                "date": one["date"],
                "text": one["text"],
                "paragraph": one["paragraph"],
                "elements": 1,
                "paragraph_mark": one["paragraph_mark"],
            }
        )
    return {
        "changes": changes,
        "elements": {
            "insertions": counts["insertion"],
            "deletions": counts["deletion"],
            "format_changes": counts["format-change"],
            "moves": counts["move"],
        },
        "paragraph_marks": marks,
        "track_changes": None,
        "notes": [],
    }
