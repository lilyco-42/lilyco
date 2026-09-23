#!/usr/bin/env python3
"""`lbin office-*` 的两读者对账探针（只在 CI 上跑：本机不编译，二进制由 Actions 造）。

它验的不是「命令跑通了」，而是一件更硬的事：**同一批字节，两套独立实现必须给出同样的答案**。
一边是 Rust（`lilyco-binfmt/src/office_*.rs` 与它用的 cfb / props / zipread / xmlscan / rtf /
word / biff），另一边是只用标准库的 Python 读者（`office_reader.py` + `lyco_rtf.py` +
`lyco_legacy.py`）。两边各自从 fixture 里算，这里逐字段比对。

比的是**具体值**（表名、可见性、段落数、标题文本、格子引用与值、部件数），不是
「都退出 0」。差一个字段就打印两边的值并失败 —— 只报「不匹配」等于让人重新去查一遍。

用法：`python3 office_probe.py <lbin 路径> [fixture 目录]`
"""

from __future__ import annotations

import csv
import io
import json
import os
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import office_reader  # noqa: E402  第二读者（标准库实现）

BIN = Path(os.path.abspath(sys.argv[1])) if len(sys.argv) > 1 else Path("target/debug/lbin")
FIXTURES = Path(
    sys.argv[2]
    if len(sys.argv) > 2
    else Path(__file__).resolve().parents[2] / "lilyco-binfmt/tests/fixtures/office"
)
RESULTS: list = []


def record(name: str, ok: bool, detail: str = "") -> None:
    RESULTS.append((name, ok, detail))
    print("  %s %-52s %s" % ("PASS" if ok else "FAIL", name, detail[:120]))


def lbin(command: str, fixture: Path, *extra: str) -> dict:
    """跑一条命令，返回解析后的 JSON（解析不了就把原文留在失败信息里）"""
    argv = [str(BIN), command, "--path", str(fixture), "--json", *extra]
    proc = subprocess.run(argv, capture_output=True, text=True, timeout=120, encoding="utf-8", errors="replace")
    try:
        return json.loads(proc.stdout)
    except Exception as why:  # noqa: BLE001
        return {"__parse_error__": f"{why}: {proc.stdout[:200]} / {proc.stderr[:200]}"}


def dig(payload: dict, dotted: str):
    """按点号取值，支持 a.b[0].c 这种一步下标"""
    here = payload
    for chunk in dotted.split("."):
        if here is None:
            return None
        name, _, index = chunk.partition("[")
        if name:
            if not isinstance(here, dict) or name not in here:
                return None
            here = here[name]
        if index:
            which = int(index.rstrip("]"))
            if not isinstance(here, list) or which >= len(here):
                return None
            here = here[which]
    return here


def check(name: str, got, want) -> None:
    ok = got == want
    record(name, ok, "" if ok else f"lbin={json.dumps(got, ensure_ascii=False)[:90]} 读者={json.dumps(want, ensure_ascii=False)[:90]}")


def fixture(name: str) -> Path:
    path = FIXTURES / name
    assert path.exists(), f"缺 fixture：{path}"
    return path


def main() -> int:
    assert BIN.exists(), f"二进制不在：{BIN}（CI 里应先 cargo build -p lilyco-binfmt）"
    files = {}
    for one in sorted(FIXTURES.iterdir()):
        if one.is_file() and not one.name.startswith("."):
            files[one.name] = office_reader.facts(one)
    print(f"=== 对账 {len(files)} 份真实生产者文件（{BIN}） ===")

    # ── 识别：每条命令都得把文件认成同一个东西 ──────────────────────
    expect = {
        "notes.docx": ("ooxml", "word", "docx"),
        "notes-hf.docx": ("ooxml", "word", "docx"),
        "notes-foot.docx": ("ooxml", "word", "docx"),
        "notes-hf.odt": ("opendocument", "word", "odt"),
        "notes-hf.rtf": ("rtf", "word", "rtf"),
        "notes.docm": ("ooxml", "word", "docm"),
        "notes-en.docx": ("ooxml", "word", "docx"),
        "book.xlsx": ("ooxml", "excel", "xlsx"),
        "deck.pptx": ("ooxml", "powerpoint", "pptx"),
        "notes.odt": ("opendocument", "word", "odt"),
        "book.ods": ("opendocument", "excel", "ods"),
        "deck.odp": ("opendocument", "powerpoint", "odp"),
        "notes.doc": ("compound", "word", "doc"),
        "notes-en.doc": ("compound", "word", "doc"),
        "formats.xlsx": ("ooxml", "excel", "xlsx"),
        "formats.ods": ("opendocument", "excel", "ods"),
        "book.xls": ("compound", "excel", "xls"),
        "deck.ppt": ("compound", "powerpoint", "ppt"),
        "notes.rtf": ("rtf", "word", "rtf"),
        "hidden.xlsx": ("ooxml", "excel", "xlsx"),
        "hidden-lo.xlsx": ("ooxml", "excel", "xlsx"),
        "hidden.ods": ("opendocument", "excel", "ods"),
    }
    print("=== 1) office-info：识别与包账 ===")
    for name, (family, app, fmt) in expect.items():
        out = lbin("office-info", fixture(name))
        one = files[name]
        record(f"{name}: 认得出来", out.get("family") == family and out.get("app") == app and out.get("format") == fmt,
               "" if out.get("format") == fmt else json.dumps({k: out.get(k) for k in ("family", "app", "format", "notes")}, ensure_ascii=False))
        check(f"{name}: 部件数与读者一致", out.get("parts"), one.get("opc", {}).get("count", 0))
        for one_check in out.get("checks", []):
            record(f"{name}: 「{one_check.get('claim', '')[:20]}」通过", one_check.get("ok") is True, str(one_check.get("note", ""))[:110])
        # 宏/加密这类"要不要小心"的判断也要两边一致：读者看的是部件名
        parts = set(one.get("opc", {}).get("parts", []))
        has_macro_part = any(one_name.endswith("vbaProject.bin") for one_name in parts)
        check(f"{name}: 宏检测一致", bool(dig(out, "signals.has_macros")), has_macro_part)

    # ── 正文：段落/行列/单元格，逐行比 ─────────────────────────────
    print("=== 2) office-text：读出来的文字 ===")
    docx = lbin("office-text", fixture("notes.docx"))
    want = files["notes.docx"]["ooxml"]
    # 这条命令交出来的是「有字的段」：正文之外还要加上批注 / 脚注 / 尾注 / 页眉页脚那几类部件
    side = want.get("side_texts", [])
    want_texts = [one for one in want["paragraphs"] if one] + [one["text"] for one in side if one["text"]]
    check("notes.docx 段落数（正文 + 正文之外）", docx.get("total_paragraphs"), len(want_texts))
    check("notes.docx 非空段文本", [one["text"] for one in docx.get("paragraphs", []) if one["text"]],
          want_texts)
    for one in side:
        if not one["text"]:
            continue
        got = [had for had in docx.get("paragraphs", []) if had.get("text") == one["text"]]
        pair = (one.get("from"), one.get("author"))
        got_pair = (got[0].get("from"), got[0].get("author")) if got else (None, None)
        check("notes.docx 正文之外的出处与作者", got_pair, pair)
    # 页眉页脚那份样本：这条分支此前从未被真件走过，这里连顺序一起比
    hf = lbin("office-text", fixture("notes-hf.docx"))
    hwant = files["notes-hf.docx"]["ooxml"]
    hside = [one for one in hwant["side_texts"] if one["text"]]
    check(
        "notes-hf.docx 正文+页眉页脚条数",
        hf.get("total_paragraphs"),
        len([one for one in hwant["paragraphs"] if one]) + len(hside),
    )
    check(
        "notes-hf.docx 页眉页脚逐条出处",
        [
            (one.get("from"), one.get("part"), one.get("text"))
            for one in hf.get("paragraphs", [])
            if one.get("from")
        ],
        [(one["from"], one["part"], one["text"]) for one in hside],
    )

    # 脚注那份样本（LibreOffice 从 RTF 导入写出）：部件里白坐着两条分隔符，
    # 「有几条注」与「有几个 w:footnote 元素」不是一回事
    foot = lbin("office-text", fixture("notes-foot.docx"))
    fwant = files["notes-foot.docx"]["ooxml"]
    fside = [one for one in fwant["side_texts"] if one["text"]]
    check("notes-foot.docx 分隔符不交成正文那样的注", len(fside), 2)
    check(
        "notes-foot.docx 正文+脚注条数",
        foot.get("total_paragraphs"),
        len([one for one in fwant["paragraphs"] if one]) + len(fside),
    )
    check(
        "notes-foot.docx 脚注逐条出处",
        [
            (one.get("from"), one.get("part"), one.get("text"))
            for one in foot.get("paragraphs", [])
            if one.get("from")
        ],
        [(one["from"], one["part"], one["text"]) for one in fside],
    )
    record(
        "notes-foot.docx 注的字不混进正文",
        all("gross" not in (one.get("text") or "") for one in foot.get("paragraphs", []) if not one.get("from")),
        json.dumps([one.get("text") for one in foot.get("paragraphs", [])], ensure_ascii=False)[:120],
    )

    footdoc = lbin("office-doc", fixture("notes-foot.docx"))
    check("notes-foot.docx 脚注数", footdoc.get("footnotes"), fwant["footnotes"])
    check("notes-foot.docx 尾注数（部件不在包里就是零）", footdoc.get("endnotes"), fwant["endnotes"])

    # 同一批字换 ODF 的存法：页眉页脚在 styles.xml 的 master-page 里，一节一个 master-page
    hfodt = lbin("office-text", fixture("notes-hf.odt"))
    hwant2 = files["notes-hf.odt"]["page_text"]
    check(
        "notes-hf.odt 页眉页脚逐条出处",
        [
            (one.get("from"), one.get("part"), one.get("master"), one.get("slot"), one.get("text"))
            for one in hfodt.get("paragraphs", [])
            if one.get("from")
        ],
        [(one["from"], one["part"], one["master"], one["slot"], one["text"]) for one in hwant2],
    )
    check("notes-hf.odt 两个 master-page 各一套", len(hwant2), 4)
    check(
        "notes.odt 没有页眉页脚时不凭空多条目",
        [one.get("from") for one in lbin("office-text", fixture("notes.odt")).get("paragraphs", []) if one.get("from") == "header"],
        [],
    )

    doc = lbin("office-doc", fixture("notes.docx"))
    check("notes.docx 表格数", dig(doc, "structure.tables"), want["tables"])
    check("notes.docx 行数", dig(doc, "structure.table_rows"), want["table_rows"])
    check("notes.docx 格子数", dig(doc, "structure.table_cells"), want["table_cells"])
    check("notes.docx 节数", dig(doc, "structure.sections"), want["sections"])
    check("notes.docx 批注数", doc.get("comments"), want["comments"])
    check("notes.docx 标题", doc.get("headings"), [{"level": one["level"], "text": one["text"]} for one in want["headings"]])
    check("notes.docx 链接", [one["target"] for one in doc.get("hyperlinks", [])], [one["target"] for one in want["hyperlinks"]])
    check("notes.docx 图片", doc.get("images"), want["media"])

    deck = lbin("office-text", fixture("deck.pptx"))
    deck_want = files["deck.pptx"]["ooxml"]
    # office-text 连备注页一起读（那是这份演示稿写了字的地方），要账就得把 notes 部件算进来
    check("deck.pptx 幻灯片部件", sorted({one["part"] for one in deck.get("paragraphs", [])}),
          sorted([one["part"] for one in deck_want["slides"]] + deck_want["notesSlides"]))
    slide = lbin("office-slide", fixture("deck.pptx"))
    check("deck.pptx 页数", len(slide.get("slides", [])), deck_want["slide_count"])
    check("deck.pptx 每页标题", [one["title"] for one in slide.get("slides", [])], [one["title"] for one in deck_want["slides"]])
    check("deck.pptx 每页备注", [one["notes"] for one in slide.get("slides", [])], [one["notes"] for one in deck_want["slides"]])
    check("deck.pptx 母版数", len(slide.get("masters", [])), len(deck_want["masters"]))
    check("deck.pptx 版式数", len(slide.get("layouts", [])), len(deck_want["layouts"]))

    sheet = lbin("office-text", fixture("book.xlsx"))
    check("book.xlsx 有文字内容", sheet.get("kind") in ("cells", "shared-strings"), True)
    odt = lbin("office-text", fixture("notes.odt"))
    odt_want = files["notes.odt"]["odf"]
    check("notes.odt 文本非空", [one["text"] for one in odt.get("paragraphs", []) if one["text"]] != [], True)
    check("notes.odt 段落数（批注里的字不算正文段）", odt_want["paragraph_count"], 9)
    want_odt = [one for one in odt_want["paragraphs"] if one] + [
        one["text"] for one in odt_want["annotations"] if one["text"]
    ]
    check("notes.odt 段数（正文 + 批注）", odt.get("total_paragraphs"), len(want_odt))
    check("notes.odt 逐条文本", [one["text"] for one in odt.get("paragraphs", []) if one["text"]],
          want_odt)
    for one in odt_want["annotations"]:
        got = [had for had in odt.get("paragraphs", []) if had.get("text") == one["text"]]
        check("notes.odt 批注的出处与作者", [(had.get("from"), had.get("author")) for had in got],
              [("annotation", one["author"])])

    legacy = lbin("office-text", fixture("notes.doc"))
    legacy_want = files["notes.doc"]["legacy_text"]
    check("notes.doc 逐行文本", [one["text"] for one in legacy.get("paragraphs", [])], legacy_want["lines"])
    legacy_en = lbin("office-text", fixture("notes-en.doc"))
    check("notes-en.doc 逐行文本", [one["text"] for one in legacy_en.get("paragraphs", [])],
          files["notes-en.doc"]["legacy_text"]["lines"])

    ppt97 = lbin("office-text", fixture("deck.ppt"))
    ppt_want = files["deck.ppt"]["ppt_text"]
    check("deck.ppt 记录树走满", dig(ppt97, "kind"), "record-tree")
    check("deck.ppt 逐行文本", [one["text"] for one in ppt97.get("paragraphs", [])], ppt_want["lines"])
    check("deck.ppt 行数一致", ppt97.get("total_paragraphs"), len(ppt_want["lines"]))
    slide97 = lbin("office-slide", fixture("deck.ppt"))
    check("deck.ppt office-slide 也说记录树", dig(slide97, "record_tree.text_atoms"), len(ppt_want["text_atoms"]))
    rtf = lbin("office-text", fixture("notes.rtf"))
    check("notes.rtf 逐行文本", [one["text"] for one in rtf.get("paragraphs", [])], files["notes.rtf"]["rtf"]["lines"])
    # RTF 的页眉页脚与正文在同一个流里，只靠目标群分开：正文不许带上页眉的字
    hf_rtf = lbin("office-text", fixture("notes-hf.rtf"))
    rwant = files["notes-hf.rtf"]["rtf"]
    check(
        "notes-hf.rtf 正文行",
        [one["text"] for one in hf_rtf.get("paragraphs", []) if not one.get("from")],
        rwant["lines"],
    )
    check(
        "notes-hf.rtf 页眉页脚逐条",
        [
            (one.get("from"), one.get("slot"), one.get("text"))
            for one in hf_rtf.get("paragraphs", [])
            if one.get("from")
        ],
        [("header", one["slot"], one["text"]) for one in rwant["headers"]]
        + [("footer", one["slot"], one["text"]) for one in rwant["footers"]],
    )
    check("notes-hf.rtf 目标群数", rwant["page_destinations"], 6)

    # ── 表格结构：表名、可见性、范围、格子 ──────────────────────────
    print("=== 3) office-sheet：布局 ===")
    x = lbin("office-sheet", fixture("book.xlsx"))
    xw = files["book.xlsx"]["ooxml"]
    check("book.xlsx 表名与状态", [(one["name"], one["state"]) for one in x.get("sheets", [])],
          [(one["name"], one["state"] if one["state"] != "hidden" else "hidden") for one in xw["sheets"]])
    check("book.xlsx 格子总数", dig(x, "workbook.totals.cells"), xw["cells"])
    check("book.xlsx 公式数", dig(x, "workbook.totals.formulas"), xw["formula_cells"])
    check("book.xlsx 合并格", dig(x, "sheets[0].merged"), xw["merged"])
    check("book.xlsx 命名区域", [one["text"] for one in x.get("defined_names", [])] != [], True)
    b = lbin("office-sheet", fixture("book.xls"))
    bw = files["book.xls"]["biff"]
    check("book.xls 表名与状态", [(one["name"], one["state"]) for one in b.get("sheets", [])],
          [(one["name"], one["state"]) for one in bw["sheets"]])
    check("book.xls 字符串表", dig(b, "workbook.shared_strings"), len(bw["shared_strings"]))
    check("book.xls 格子数", dig(b, "workbook.totals.cells"), len(bw["cells"]))
    check("book.xls 每格归位", [one.get("sheet") for one in b.get("cells", [])],
          [one.get("sheet") for one in bw["cells"][: int(dig(b, "workbook.totals.cells") or 0)]])
    check("book.xls 按表计数", {str(one.get("name")): int(one.get("cells") or 0) for one in b.get("sheets", [])},
          {str(k): int(v) for k, v in bw["cells_per_sheet"].items()})

    # ── 3b) 数字格式：格子写的是 cellXfs 的下标，日期藏在样式里 ──────────
    print("=== 3b) formats.xlsx：格式号、判定与换算出来的日期 ===")
    fx = lbin("office-sheet", fixture("formats.xlsx"))
    fw = files["formats.xlsx"]["formats"]
    flat = {}
    for one_sheet in fx.get("sheets", []):
        for cell in one_sheet.get("cell_list", []):
            flat["%s!%s" % (one_sheet.get("name"), cell.get("ref"))] = cell
    want = {("%s!%s" % (one["sheet"], one["ref"])): one for one in fw["cells"]}
    check("formats.xlsx 格子数", len(flat), len(want))
    check("formats.xlsx 1904 基准", dig(fx, "workbook.date1904"), fw["date1904"])
    for key in sorted(want):
        got = flat.get(key, {})
        mine = {k: got.get(k) for k in ("format_kind", "num_fmt", "format", "as_date")}
        theirs = {
            "format_kind": want[key].get("kind"),
            "num_fmt": want[key].get("num_fmt_id"),
            "format": want[key].get("format_code"),
            "as_date": want[key].get("as_date"),
        }
        # 日期格两边都要有 as_date；非日期格两边都不许有
        record("formats %s 判成 %s" % (key, theirs["format_kind"]), mine == theirs,
               json.dumps({"got": mine, "want": theirs}, ensure_ascii=False)[:130])

    # ── 3c) ODS：另一套脾气的表格 ────────────────────────────────────
    print("=== 3c) ODS：重复计数、覆盖格、值类型与隐藏表 ===")
    for name in ("book.ods", "formats.ods"):
        want = files[name]["ods"]
        got = lbin("office-sheet", fixture(name))
        check("%s 表的顺序与名字" % name, [one.get("name") for one in got.get("sheets", [])],
              [one["name"] for one in want["sheets"]])
        check("%s 表的可见性" % name, [one.get("state") for one in got.get("sheets", [])],
              ["visible" if one["visible"] else "hidden" for one in want["sheets"]])
        check("%s 格子总数" % name, dig(got, "workbook.cell_total"), want["cell_total"])
        # 第三方的账：LibreOffice 自己往 meta.xml 写了 cell-count
        check("%s 与生产者自报的 cell-count" % name, dig(got, "workbook.cell_total"),
              int(want["statistic"]["cell-count"]))
        mine_flat = {}
        for one_sheet in got.get("sheets", []):
            for cell in one_sheet.get("cell_list", []):
                mine_flat["%s!%s" % (one_sheet.get("name"), cell.get("ref"))] = cell
        theirs_flat = {
            "%s!%s" % (one["name"], cell["ref"]): cell for one in want["sheets"] for cell in one["cell_list"]
        }
        smap = files[name].get("ods_styles", {})

        def fmt(d):
            return {
                k: (tuple(d.get("format_tokens") or []) if k == "format_tokens" else d.get(k))
                for k in (
                    "data_style",
                    "format_kind",
                    "decimals",
                    "currency_symbol",
                    "format_tokens",
                )
            }
        check("%s 格子位置清单" % name, sorted(mine_flat), sorted(theirs_flat))
        for key in sorted(theirs_flat):
            mine = mine_flat.get(key, {})
            theirs = theirs_flat[key]
            pair = {
                "kind": (mine.get("kind"), theirs["value_type"]),
                "text": (mine.get("text"), theirs["text"]),
                "value": (mine.get("value"), float(theirs["value"]) if theirs["value"] else None),
                "date": (mine.get("date_value"), theirs["date_value"]),
                "bool": (mine.get("boolean_value"), theirs["boolean_value"]),
                "formula": (mine.get("formula"), theirs["formula"]),
                "span": (mine.get("columns_spanned"), theirs["columns_spanned"]),
                # 格式那一跳：格子引的样式名，以及顺着 样式→data-style→number:*-style 抄出来的账
                "样式": (mine.get("cell_style"), theirs.get("style")),
                "格式": (fmt(mine), fmt(smap.get(theirs.get("style")) or {})),
            }
            bad = [label for label, (a, b) in pair.items() if a != b]
            record("%s %s 的账一致" % (name, key), not bad,
                   json.dumps({"字段": bad, "got": [pair[one][0] for one in bad]}, ensure_ascii=False)[:130])
        check("%s 每表行列" % name,
              [(one.get("rows"), one.get("columns")) for one in got.get("sheets", [])],
              [(one["rows"], one["columns"]) for one in want["sheets"]])
        check("%s 覆盖格与合并" % name,
              [(one.get("covered"), one.get("merged")) for one in got.get("sheets", [])],
              [(one["covered"], one["merged"]) for one in want["sheets"]])
        txt = lbin("office-text", fixture(name))
        check("%s office-text 走的是格子口径" % name, txt.get("kind"), "cells")
        check("%s office-text 的格子清单" % name,
              [(one.get("sheet"), one.get("ref"), one.get("text")) for one in txt.get("paragraphs", [])],
              [(one["name"], cell["ref"], cell["text"]) for one in want["sheets"] for cell in one["cell_list"]])

    # ── 3d) ODT：文字文档的结构账 ───────────────────────────────────
    print("=== 3d) notes.odt：office-doc 的 ODF 分支 ===")
    odtstruct = lbin("office-doc", fixture("notes.odt"))
    dwant = files["notes.odt"]["odt"]
    for field in (
        "paragraphs", "empty_paragraphs", "tables", "table_rows", "table_cells",
        "covered_cells", "sections", "breaks", "page_breaks", "drawings",
        "annotations", "lists", "list_styles", "bookmarks", "sequences",
        "tracked_changes",
    ):
        check("notes.odt structure.%s" % field, dig(odtstruct, "structure." + field), dwant.get(field))
    # structure 里这两个是计数，清单在别的键上：别拿清单去比计数
    check("notes.odt structure.hyperlinks", dig(odtstruct, "structure.hyperlinks"),
          len(dwant["hyperlinks"]))
    check("notes.odt structure.images", dig(odtstruct, "structure.images"), len(dwant["images"]))
    for field in ("footnotes", "endnotes"):
        check("notes.odt %s" % field, odtstruct.get(field), dwant.get(field))
    check("notes.odt 样式用量", odtstruct.get("styles"), dwant["styles"])
    check(
        "notes.odt 标题与层级",
        odtstruct.get("headings"),
        [{"level": int(one["level"]), "text": one["text"]} for one in dwant["headings"]],
    )
    check("notes.odt 超链接清单", [one.get("target") for one in odtstruct.get("hyperlinks", [])],
          [one["target"] for one in dwant["hyperlinks"]])
    check("notes.odt 图的出处", odtstruct.get("images"), dwant["images"])
    check("notes.odt 表格清单", [(one.get("name"), one.get("rows"), one.get("cells"), one.get("covered"))
                                 for one in odtstruct.get("tables", [])],
          [(one["name"], one["rows"], one["cells"], one["covered"]) for one in dwant["table_list"]])
    # 生产者自己写在 meta.xml 的那份账：照原样交出来，口径不同就说清口径
    check("notes.odt 与生产者自报的页数", dig(odtstruct, "producer_statistics.page-count"),
          dwant["statistic"]["page-count"])
    record(
        "notes.odt 段数两份账的口径差说得出来",
        dig(odtstruct, "structure.paragraphs") == dwant["paragraphs"]
        and int(dwant["statistic"]["paragraph-count"]) == dwant["paragraphs"]
        + len(dwant["annotation_texts"]),
        json.dumps(
            {
                "lbin": dig(odtstruct, "structure.paragraphs"),
                "读者(不含批注)": dwant["paragraphs"],
                "生产者自报": dwant["statistic"]["paragraph-count"],
                "批注里的段": len(dwant["annotation_texts"]),
            },
            ensure_ascii=False,
        )[:160],
    )

    # ── 3e) ODP：页、备注与母版那一跳 ────────────────────────────────
    print("=== 3e) deck.odp：office-slide 的 ODF 分支 ===")
    slide = lbin("office-slide", fixture("deck.odp"))
    pw = files["deck.odp"]["odp"]
    check("deck.odp 页数", len(slide.get("slides", [])), len(pw["slides"]))
    check("deck.odp 母版引用", sorted(slide.get("masters", [])), pw["masters"])
    check("deck.odp 版式名", sorted(slide.get("layouts", [])), pw["layouts"])
    check("deck.odp 文件里的版式定义数", pw["page_layout_defs"], 0)
    for index, one in enumerate(pw["slides"]):
        got = slide.get("slides", [])[index]
        label = "deck.odp 第 %d 页" % (index + 1)
        check(label + " 页名", got.get("name"), one["name"])
        check(label + " 标题", got.get("title"), one["title"])
        check(label + " 母版", got.get("master"), one["master"])
        check(label + " 版式名", got.get("layout"), one["layout"])
        check(label + " 页面上的字", got.get("texts"), one["texts"])
        check(label + " 备注", got.get("notes"), one["notes"])
        check(label + " 占位类别", got.get("placeholders"), one["placeholders"])
        check(label + " 图与表", (got.get("pictures"), got.get("tables")),
              (one["pictures"], one["tables"]))
    check("deck.odp 尺寸来自母版那一跳", dig(slide, "size.page_width"),
          pw["slides"][0]["size"]["page_width"])
    check("deck.odp 尺寸的方向", dig(slide, "size.orientation"),
          pw["slides"][0]["size"]["orientation"])
    blob = json.dumps(slide, ensure_ascii=False)
    record("deck.odp 不许把页码占位的样字当正文", "<编号>" not in blob, blob[:120])

    # ── 3f) --csv：把一张表铺平成 RFC4180（期望文本逐字来自 csv_facts/biff_csv）──
    print("=== 3f) office-sheet --csv：铺平一张表 ===")
    for name in ("book.xlsx", "formats.xlsx", "book.xls", "book.ods", "formats.ods"):
        want = {one["name"]: one["csv"] for one in files[name]["csv"]["sheets"]}
        plain = lbin("office-sheet", fixture(name))
        record(
            "%s 不开 --csv 就不交那份账" % name,
            plain.get("csv") is None,
            json.dumps(plain.get("csv"), ensure_ascii=False)[:80],
        )
        got = lbin("office-sheet", fixture(name), "--csv")
        order = [one["name"] for one in got.get("sheets", [])]
        check("%s --csv 的表序与命令报的一致" % name, order, list(want))
        check("%s --csv 默认第一张" % name, dig(got, "csv.text"), want.get(order[0]))
        for at, one in enumerate(got.get("sheets", [])):
            nm = one["name"]
            body = want.get(nm) or ""
            shape = list(csv.reader(io.StringIO(body)))
            by_name = lbin("office-sheet", fixture(name), "--csv", "--sheet", nm)
            check("%s --csv --sheet %s（按表名）" % (name, nm), dig(by_name, "csv.text"), body)
            by_index = lbin("office-sheet", fixture(name), "--csv", "--sheet", str(at))
            check("%s --csv --sheet %d（按序号）" % (name, at), dig(by_index, "csv.text"), body)
            check(
                "%s --csv %s 声明的行列与 csv 模块解出来的一致" % (name, nm),
                [dig(by_name, "csv.rows"), dig(by_name, "csv.columns")],
                [len(shape), max((len(row) for row in shape), default=0)],
            )
        bad = lbin("office-sheet", fixture(name), "--csv", "--sheet", "没这张表")
        check("%s --csv 认错表名就把候选说清楚" % name, isinstance(dig(bad, "csv.error"), str), True)

    # ── 3g) 隐藏的行与列：藏起来的是「看不看得到」，不是「在不在」────────
    print("=== 3g) 隐藏行/隐藏列：三种存法，同一个数 ===")
    for name in ("hidden.xlsx", "hidden-lo.xlsx", "hidden.ods"):
        if "hidden" in files[name]:
            want = files[name]["hidden"]
        else:
            want = {one["name"]: one for one in files[name]["ods"]["sheets"]}
        got = lbin("office-sheet", fixture(name))
        for one in got.get("sheets", []):
            nm = one["name"]
            mine = {k: one.get(k) for k in ("hidden_rows", "hidden_cols")}
            theirs = {k: (want.get(nm) or {}).get(k) for k in ("hidden_rows", "hidden_cols")}
            check("%s %s 隐藏几行几列" % (name, nm), mine, theirs)
        check(
            "%s 合计的隐藏行数" % name,
            dig(got, "workbook.totals.hidden_rows"),
            sum(int(one.get("hidden_rows") or 0) for one in want.values()),
        )
        blob = json.dumps(lbin("office-sheet", fixture(name), "--csv"), ensure_ascii=False)
        record(
            "%s 隐藏列里的字仍然要看得见" % name,
            "第一列批注" in blob and "第二列批注" in blob,
            blob[:90],
        )
    check(
        "隐藏列按 min/max 展开：LibreOffice 并成一条也报 3 列",
        dig(lbin("office-sheet", fixture("hidden-lo.xlsx")), "sheets[0].hidden_cols"),
        dig(lbin("office-sheet", fixture("hidden.xlsx")), "sheets[0].hidden_cols"),
    )

    # ── 属性：三份账 ───────────────────────────────────────────────
    print("=== 4) office-meta：属性 ===")
    m = lbin("office-meta", fixture("notes.docx"))
    core = files["notes.docx"]["docprops"]["core"]
    check("notes.docx 标题", dig(m, "core.title"), core.get("title"))
    check("notes.docx 作者", dig(m, "core.creator"), core.get("creator"))
    check("notes.docx 关键字", dig(m, "core.keywords"), core.get("keywords"))
    check("notes.docx 生产者", dig(m, "application.Application"), files["notes.docx"]["docprops"]["app"].get("Application"))
    check("notes.docx 自定义属性", dig(m, "custom.口径"), "含税")
    lm = lbin("office-meta", fixture("notes.doc"))
    props = {}
    for one in files["notes.doc"].get("summary", {}).get("sets", []):
        if one["fmtid"] == "e0859ff2f94f6810ab9108002b27b3d9":
            props = one["properties"]
    # 属性集表里的 PID 是**整数**键（json.dumps 才把它们写成字符串）
    check("notes.doc OLE 标题", dig(lm, "legacy.SummaryInformation.title.value"), props.get(2))
    check("notes.doc OLE 作者", dig(lm, "legacy.SummaryInformation.author.value"), props.get(4))
    check("notes.doc OLE CodePage", dig(lm, "legacy.SummaryInformation.codepage.value"), 65001)
    rm = lbin("office-meta", fixture("notes.rtf"))
    rinfo = files["notes.rtf"]["rtf"]["info"]["fields"]
    for key, value in sorted(rinfo.items()):
        check("notes.rtf \\info." + key, dig(rm, "core." + key), value)
    rprops = {
        str(one.get("name")): one.get("value")
        for one in files["notes.rtf"]["rtf"]["info"]["user_props"]
    }
    check("notes.rtf 自定义属性条数", len(rm.get("custom", {})), len(rprops))
    for key, value in sorted(rprops.items()):
        check("notes.rtf 自定义属性 " + key, dig(rm, "custom." + key + ".value"), value)
    om = lbin("office-meta", fixture("notes.odt"))
    ometa = files["notes.odt"]["odf"]["meta"]
    check("notes.odt 标题", dig(om, "core.title"), ometa.get("title"))
    check("notes.odt 生成者", str(dig(om, "core.generator") or "").startswith("LibreOffice/"), True)

    # ── 嵌入物与风险面 ─────────────────────────────────────────────
    print("=== 5) office-objects：里面还装了什么 ===")
    o = lbin("office-objects", fixture("notes.docx"))
    check("notes.docx 图片数", len(o.get("media", [])), len(files["notes.docx"]["ooxml"]["media"]))
    check("notes.docx 站外目标", [one["target"] for one in o.get("external", [])],
          [one["target"] for one in files["notes.docx"]["ooxml"]["hyperlinks"]])
    check("notes.docx 无宏", bool(dig(o, "risk_signals.has_macros")), False)
    dm = lbin("office-objects", fixture("notes.docm"))
    check("notes.docm 有宏", bool(dig(dm, "risk_signals.has_macros")), True)
    # ODF 没有 OPC 关系表：引用坐在 xlink:href 上，「在不在包外」只看它有没有 scheme
    for name in ("notes.odt", "deck.odp", "book.ods", "hidden.ods", "formats.ods"):
        want = files[name]["links"]
        got = lbin("office-objects", fixture(name))

        def shape(one):
            return json.dumps(one, ensure_ascii=False, sort_keys=True)

        check(
            "%s 引用清单" % name,
            sorted(shape(one) for one in got.get("links", [])),
            sorted(shape(one) for one in want["links"]),
        )
        check(
            "%s 站外目标" % name,
            [(one.get("target"), one.get("source")) for one in got.get("external", [])],
            [(one["target"], one["part"]) for one in want["external"]],
        )
        check("%s 站外目标计数" % name, dig(got, "risk_signals.external_targets"), len(want["external"]))
    check("notes.odt 那份确实有一个站外超链接", len(files["notes.odt"]["links"]["external"]), 1)
    check(
        "deck.odp 的图与表预览都是包内引用（不算站外）",
        [one["target"] for one in files["deck.odp"]["links"]["links"]],
        ["Pictures/1000000100000008000000088E4DF5D4.png", "Pictures/TablePreview1.svm"],
    )

    failed = [one for one in RESULTS if not one[1]]
    print(f"=== 合计 {len(RESULTS)} 项，失败 {len(failed)} 项 ===")
    for name, _, detail in failed:
        print(f"  FAIL {name}: {detail}")
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
