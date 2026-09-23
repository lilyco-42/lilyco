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
        "notes.docm": ("ooxml", "word", "docm"),
        "notes-en.docx": ("ooxml", "word", "docx"),
        "book.xlsx": ("ooxml", "excel", "xlsx"),
        "deck.pptx": ("ooxml", "powerpoint", "pptx"),
        "notes.odt": ("opendocument", "word", "odt"),
        "book.ods": ("opendocument", "excel", "ods"),
        "deck.odp": ("opendocument", "powerpoint", "odp"),
        "notes.doc": ("compound", "word", "doc"),
        "notes-en.doc": ("compound", "word", "doc"),
        "book.xls": ("compound", "excel", "xls"),
        "deck.ppt": ("compound", "powerpoint", "ppt"),
        "notes.rtf": ("rtf", "word", "rtf"),
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
    check("notes.docx 段落数", docx.get("total_paragraphs"), want["paragraph_count"])
    check("notes.docx 非空段文本", [one["text"] for one in docx.get("paragraphs", []) if one["text"]],
          [one for one in want["paragraphs"] if one])
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
    check("deck.pptx 幻灯片部件", sorted({one["part"] for one in deck.get("paragraphs", [])}),
          [one["part"] for one in deck_want["slides"]])
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
    check("notes.odt 段落数", odt.get("total_paragraphs"), odt_want["paragraph_count"])

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
    check("notes.doc OLE 标题", dig(lm, "legacy.SummaryInformation.title.value"), props.get("2"))
    check("notes.doc OLE 作者", dig(lm, "legacy.SummaryInformation.author.value"), props.get("4"))
    check("notes.doc OLE CodePage", dig(lm, "legacy.SummaryInformation.codepage.value"), 65001)
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

    failed = [one for one in RESULTS if not one[1]]
    print(f"=== 合计 {len(RESULTS)} 项，失败 {len(failed)} 项 ===")
    for name, _, detail in failed:
        print(f"  FAIL {name}: {detail}")
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
