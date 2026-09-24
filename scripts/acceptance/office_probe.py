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
    # 两边各留 400 字：留少了就只看得到共同的前缀，差的偏偏在后头（修订那一条就这样）
    record(name, ok, "" if ok else f"lbin={json.dumps(got, ensure_ascii=False)[:400]} 读者={json.dumps(want, ensure_ascii=False)[:400]}")


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
        "notes-end.docx": ("ooxml", "word", "docx"),
        "notes-end.odt": ("opendocument", "word", "odt"),
        "notes-hf.odt": ("opendocument", "word", "odt"),
        "notes-hf.rtf": ("rtf", "word", "rtf"),
        "notes-end.rtf": ("rtf", "word", "rtf"),
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
        "hidden.xls": ("compound", "excel", "xls"),
        "deck.ppt": ("compound", "powerpoint", "ppt"),
        "notes.rtf": ("rtf", "word", "rtf"),
        "hidden.xlsx": ("ooxml", "excel", "xlsx"),
        "hidden-lo.xlsx": ("ooxml", "excel", "xlsx"),
        "hidden.ods": ("opendocument", "excel", "ods"),
        "cell-notes.xlsx": ("ooxml", "excel", "xlsx"),
        "cell-notes-lo.xlsx": ("ooxml", "excel", "xlsx"),
        "cell-notes.ods": ("opendocument", "excel", "ods"),
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

    # 尾注那一条分支第一次有真件：notes-end.docx 的 word/endnotes.xml 是 LibreOffice 的
    # docx 导出器写的（它把两条分隔符写成 <w:separator/> 那一族），
    # 于是「有几条真尾注」这一半也有了对证，不再只是「部件不在=0」
    end = lbin("office-text", fixture("notes-end.docx"))
    ewant = files["notes-end.docx"]["ooxml"]
    eside = [one for one in ewant["side_texts"] if one["text"]]
    check(
        "notes-end.docx 尾注与脚注逐条出处（两边同按部件名排）",
        [
            (one.get("from"), one.get("part"), one.get("text"))
            for one in end.get("paragraphs", [])
            if one.get("from")
        ],
        [(one["from"], one["part"], one["text"]) for one in eside],
    )
    check(
        "notes-end.docx 正文+尾注+脚注条数",
        end.get("total_paragraphs"),
        len([one for one in ewant["paragraphs"] if one]) + len(eside),
    )
    endoc = lbin("office-doc", fixture("notes-end.docx"))
    check("notes-end.docx 尾注数（office-doc 那一支）", endoc.get("endnotes"), ewant["endnotes"])
    check("notes-end.docx 脚注数（同一份文件两条腿）", endoc.get("footnotes"), ewant["footnotes"])
    check("notes-end.docx 正文段数", endoc.get("structure", {}).get("paragraphs"), ewant["paragraph_count"])

    # 同一批字换 ODF 的存法：`text:note-class` 那一条分支也第一次有真尾注可走
    # （LibreOffice 的 ODT 导出器写 `endnote` 那一类，编号换成罗马数字）
    endodt = lbin("office-doc", fixture("notes-end.odt"))
    ewant2 = files["notes-end.odt"]["odt"]
    check("notes-end.odt 尾注数（按 note:class 分）", endodt.get("endnotes"), ewant2["endnotes"])
    check("notes-end.odt 脚注数", endodt.get("footnotes"), ewant2["footnotes"])

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
    # 「多少字」那份账有两边：自己数的与生产者自报的（python-docx 写的那份全是 0）
    check("notes.docx 自己数的字与读者一致", dig(doc, "statistics.ours"), want["statistics"]["ours"])
    check("notes.docx 生产者自报的字数", dig(doc, "statistics.producer.words"), 0)
    check("notes.docx 生产者自报的页数", dig(doc, "statistics.producer.pages"), 1)
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
    # 按页归位：两位读者对同一棵树分组，逐页比行、原子数、偏移与那条版式名
    check("deck.ppt 按页归位的行数", [one["texts"] for one in slide97.get("slides", [])],
          [one["lines"] for one in ppt_want["slides"]])
    check("deck.ppt 每页原子数", [one["text_atoms"] for one in slide97.get("slides", [])],
          [one["atoms"] for one in ppt_want["slides"]])
    check("deck.ppt 每页记录偏移", [one["record_offset"] for one in slide97.get("slides", [])],
          [one["record_offset"] for one in ppt_want["slides"]])
    check("deck.ppt 每页版式名", [one["layout_name"] for one in slide97.get("slides", [])],
          [one["name"] for one in ppt_want["slides"]])
    # 归属的出处在这里：同一份文档的 pptx 那副面孔，每页标题逐张相同
    check("deck.ppt 与 deck.pptx 页数相同", len(slide97.get("slides", [])), len(slide.get("slides", [])))
    check("deck.ppt 与 deck.pptx 每页标题", [one["title"] for one in slide97.get("slides", [])],
          [one["title"] for one in slide.get("slides", [])])
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

    # RTF 的注：LibreOffice 把脚注与尾注都写进 `{\*\footnote …}`，尾注只多一个 `\ftnalt`。
    # `\*` 的语义是「不认识那个群才跳」—— 一见 `\*` 就跳会把整条注丢掉
    endrtf = lbin("office-text", fixture("notes-end.rtf"))
    rwant2 = files["notes-end.rtf"]["rtf"]
    check(
        "notes-end.rtf 正文行（注的字不留正文）",
        [one["text"] for one in endrtf.get("paragraphs", []) if not one.get("from")],
        rwant2["lines"],
    )
    check(
        "notes-end.rtf 注逐条（kind 与字）",
        [
            (one.get("from"), one.get("text"))
            for one in endrtf.get("paragraphs", [])
            if one.get("from")
        ],
        [(one["kind"], one["text"]) for one in rwant2["notes"]],
    )
    check("notes-end.rtf 注的目标群数", endrtf.get("total_paragraphs"), len(rwant2["lines"]) + len(rwant2["notes"]))
    # 跨格式同形：同一批字在 OOXML 那份里的三条注必须一模一样
    footdoc2 = lbin("office-text", fixture("notes-end.docx"))
    check(
        "同一批字的 RTF 与 docx 两份注账",
        sorted(one.get("text") for one in footdoc2.get("paragraphs", []) if one.get("from")),
        sorted(one["text"] for one in rwant2["notes"]),
    )

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

    # ── 3a2) .xls 的表级保护：那三条记录住在**被锁那张表自己的子流**里 ─────
    # 两份件唯一的差别是锁在第一张还是第二张表，所以「按表归位」是量出来的；
    # 「位 0 为 1 就是锁上了」另有一证：LibreOffice 自己 import 回 .ods 时
    # 只在同一张表上写 table:protected（见 fixtures README）
    print("=== 3a2) .xls 的锁按子流归位 ===")
    for name in ("book.xls", "locked-sheet.xls", "locked-second.xls"):
        got = lbin("office-sheet", fixture(name))
        want = files[name]["biff"]["locks"]
        mine = {
            str(one.get("name")): {
                str(k).replace("0x", "").lower(): int(v)
                for k, v in (one.get("records") or {}).items()
            }
            for one in (got.get("protection") or {}).get("sheets", [])
            if one.get("records")
        }
        theirs = {
            str(k): {str(kk): int(vv.get("grbit") or 0) for kk, vv in v.items()}
            for k, v in want.items()
        }
        check("%s 哪张表带着锁" % name, mine, theirs)
        check(
            "%s 锁上的表、开没开与口令哈希" % name,
            [
                (one.get("name"), bool(one.get("protected")), one.get("password_hash"))
                for one in (got.get("protection") or {}).get("sheets", [])
                if one.get("protected")
            ],
            [
                (
                    str(k),
                    bool((v.get("0012") or {}).get("grbit", 0) & 1),
                    "%04x" % ((v.get("0013") or {}).get("grbit") or 0) if v.get("0013") else None,
                )
                for k, v in want.items()
                if (v.get("0012") or {}).get("grbit", 0) & 1
            ],
        )

    # ── 3a3) MULRK：一行连续的数在 .xls 里共用一条记录 ────────────────────
    # 两边的读者都曾把 rkmac 读早两个字节（把每格的 ixfe 当成了数），而手上的
    # .xls 样本里没有 MULRK —— 这一族要靠专门一份件才走得到
    print("=== 3a3) mulrk：一行连续的数共用一条记录 ===")
    got = lbin("office-sheet", fixture("mulrk.xls"))
    want = files["mulrk.xls"]["biff"]
    mine = [one.get("number") for one in got.get("cells", []) if one.get("kind") == "mulrk"]
    theirs = [one.get("value") for one in want["cells"] if one.get("type") == "mulrk"]
    check("mulrk.xls 每条里的数逐个对", mine, theirs)
    check(
        "mulrk.xls 位置按列走",
        [one.get("ref") for one in got.get("cells", []) if one.get("kind") == "mulrk"],
        ["%s%d" % (chr(65 + one["col"]), one["row"] + 1) for one in want["cells"] if one.get("type") == "mulrk"],
    )
    # 这条不看键名，只看「一份件里那 11 个数是不是那几个」：与 LibreOffice 自己把
    # mulrk.xls 读回 .ods 交出来的 A2:H2 / A5:C5 同一批数（README 记着那一转）
    check("mulrk.xls 那 11 个数", sorted(mine), sorted([1000.5, 2000.5, 3000.5, 4000.5, 5000.5, 6000.5, 7000.5, 8000.5, 7.0, 14.0, 21.0]))

    # ── 3b2) .xls 的数字格式那一跳：ixfe → XF 表 → FORMAT 记录 ─────────────
    print("=== 3b2) .xls 的数字格式那一跳 ===")

    def a1(one: dict) -> str:
        col, row = int(one["col"]), int(one["row"])
        letters = ""
        col += 1
        while col:
            col, back = divmod(col - 1, 26)
            letters = chr(65 + back) + letters
        return "%s%d" % (letters, row + 1)

    for name in ("book.xls", "formats.xls", "mulrk.xls"):
        got = lbin("office-sheet", fixture(name))
        want = files[name]["biff"]
        check("%s XF 表（按出现顺序的格式号）" % name, dig(got, "workbook.xfs"), want["xfs"])
        check(
            "%s 自定义号的格式串" % name,
            dig(got, "workbook.formats"),
            {str(key): value for key, value in want["formats"].items()},
        )
        check("%s 日期基准来自 DATEMODE" % name, dig(got, "workbook.date1904"), want["date1904"])
        mine = {
            "%s!%s" % (one.get("sheet"), one.get("ref")): one.get("num_fmt")
            for one in got.get("cells", [])
            if one.get("num_fmt") is not None
        }
        theirs = {
            "%s!%s" % (one["sheet"], a1(one)): want["xfs"][one["ixfe"]]
            for one in want["cells"]
            if one.get("ixfe") is not None and one["ixfe"] < len(want["xfs"])
        }
        check("%s 每格查到的格式号" % name, mine, theirs)

    # ── 3a5) .xls 的隐藏行与隐藏列：ROW 的 0x20 位与 COLINFO 的第 0 位 ────────
    print("=== 3a5) .xls 的隐藏行/隐藏列 ===")
    for name in ("book.xls", "formats.xls", "mulrk.xls", "hidden.xls"):
        got = lbin("office-sheet", fixture(name))
        want = files[name]["biff"]["hidden"]
        mine = {
            one.get("name"): (one.get("hidden_rows"), one.get("hidden_cols"))
            for one in got.get("sheets", [])
        }
        theirs = {
            key: (len(value["rows"]), len(value["cols"])) for key, value in want.items()
        }
        check("%s 每张表藏了几行几列" % name, mine, theirs)
    # 跨格式同形：同一批字在四种存法下必须报同一个数（少一写法就会少报，早先栽过）
    ledger = {}
    for name in ("hidden.xlsx", "hidden-lo.xlsx", "hidden.ods", "hidden.xls"):
        only = lbin("office-sheet", fixture(name)).get("sheets", [{}])[0]
        ledger[name] = (only.get("hidden_rows"), only.get("hidden_cols"))
    check(
        "隐藏行/列：四种写法报出同一个数",
        sorted(set(ledger.values())),
        [(2, 3)],
    )

    # ── 3a6) 表格批注：两跳才找得到那个部件，两个生产者放在两个地方 ──────────
    print("=== 3a6) 表格批注（openpyxl / LibreOffice / ODF 三种写法） ===")

    def mine_notes(book: dict) -> dict:
        return {
            one.get("name"): {
                had.get("ref"): (had.get("author"), had.get("text"), had.get("date"))
                for had in one.get("comment_list", [])
            }
            for one in book.get("sheets", [])
        }

    for name in ("cell-notes.xlsx", "cell-notes-lo.xlsx"):
        got = lbin("office-sheet", fixture(name))
        want = files[name]["comments"]
        check(
            "%s 每张表的批注（按格子对）" % name,
            mine_notes(got),
            {
                key: {
                    had["ref"]: (had["author"], had["text"], had["date"]) for had in value
                }
                for key, value in want.items()
            },
        )
        check(
            "%s 批注总账" % name,
            dig(got, "workbook.totals.comments"),
            sum(len(value) for value in want.values()),
        )
    odsbook = lbin("office-sheet", fixture("cell-notes.ods"))
    check(
        "cell-notes.ods 每张表的批注（按格子对）",
        mine_notes(odsbook),
        {
            one.get("name"): {
                had["ref"]: (had["author"], had["text"], had["date"])
                for had in one.get("comments", [])
            }
            for one in files["cell-notes.ods"]["ods"]["sheets"]
        },
    )
    # ODF 的注就坐在格子里面：一锅端取字就会把注当成这一格的内容
    mixed = [
        one.get("ref")
        for one in odsbook.get("sheets", [{}])[0].get("cell_list", [])
        if "不含税" in (one.get("text") or "")
    ]
    record("cell-notes.ods 批注的字不混进格子", mixed == [], json.dumps(mixed, ensure_ascii=False))
    # 反面对照：没批注的件报 0，而不是缺这个键
    for name in ("book.xlsx", "hidden.xlsx"):
        check(
            "%s 没批注就是 0" % name,
            dig(lbin("office-sheet", fixture(name)), "workbook.totals.comments"),
            sum(len(value) for value in files[name]["comments"].values()),
        )

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
    check("notes.odt 与生产者自报的页数", dig(odtstruct, "statistics.producer.page-count"),
          dwant["statistic"]["page-count"])
    # 「多少字」两边对得上：我们数的字符数与 LibreOffice 自报的完全一致，
    # 词数不一致是口径（它按词切中文，我们只按空白切）
    check("notes.odt 自己数的字与读者一致", dig(odtstruct, "statistics.ours"), dwant["statistics"]["ours"])
    check(
        "notes.odt 的字符数与生产者自报的一致",
        [dig(odtstruct, "statistics.ours.characters"),
         dig(odtstruct, "statistics.ours.characters_no_spaces")],
        [int(dwant["statistic"]["character-count"]), int(dwant["statistic"]["non-whitespace-character-count"])],
    )
    check(
        "notes.odt 的词数口径与生产者不同（说明写在文档里）",
        dig(odtstruct, "statistics.ours.words_by_space") != int(dwant["statistic"]["word-count"]),
        True,
    )
    hfodtstruct = lbin("office-doc", fixture("notes-hf.odt"))
    hfwant = files["notes-hf.odt"]["odt"]
    check(
        "notes-hf.odt 只数正文：页眉页脚不在我们的账里",
        dig(hfodtstruct, "statistics.ours"),
        hfwant["statistics"]["ours"],
    )
    record(
        "notes-hf.odt 生产者的字符数含页眉页脚",
        int(hfwant["statistic"]["character-count"]) > hfwant["statistics"]["ours"]["characters"],
        json.dumps({"生产者": hfwant["statistic"]["character-count"],
                    "我们": hfwant["statistics"]["ours"]["characters"]}),
    )
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

    # ── 3ez) 修订这份账：谁在什么时候改了哪一段（OOXML 与 ODF 两套走法、一份账）
    print("=== 3ez) revisions.*：office-doc 的修订账 ===")
    ledger_of = {}
    for name in ("revisions.docx", "revisions-lo.docx", "revisions.odt"):
        whole = lbin("office-doc", fixture(name))
        got = whole.get("revisions") or {}
        want = files[name]["revisions"]
        key = ("kind", "author", "date", "paragraph", "text", "elements", "paragraph_mark")
        ledger_of[name] = [(one["kind"], one["author"], one["text"]) for one in want["changes"]]
        check(
            "%s 逐条修订" % name,
            [{field: one.get(field) for field in key} for one in got.get("changes", [])],
            [{field: one.get(field) for field in key} for one in want["changes"]],
        )
        check("%s 修订元素计数" % name, got.get("elements"), want["elements"])
        check("%s 段落标记数" % name, got.get("paragraph_marks"), want["paragraph_marks"])
        check("%s 有没有开着记录修订" % name, got.get("track_changes"), want["track_changes"])
        check("%s 条数" % name, got.get("changes_total"), len(want["changes"]))
    # 分水岭一：一次编辑被生产者拆成几个 run 时，元素数与逻辑条数必须分开
    hand = lbin("office-doc", fixture("revisions.docx")).get("revisions", {})
    lo = lbin("office-doc", fixture("revisions-lo.docx")).get("revisions", {})
    check(
        "revisions-lo.docx 元素 6 而改动 4",
        (sum(lo.get("elements", {}).values()), lo.get("changes_total")),
        (6, 4),
    )
    check(
        "revisions.docx 元素 5 而改动 5（没被拆开）",
        (sum(hand.get("elements", {}).values()), hand.get("changes_total")),
        (5, 5),
    )
    # 分水岭二：OOXML 合成后的四条与 ODF 的四个 region 逐字相同 —— 两个格式、两套走法，
    # 同一份账。日期不比对（OOXML 写 Z，ODF 不写），段落序号也不比对（标题在 ODF 里是 text:h）
    check(
        "docx 与 odt 的修订逐条对上",
        ledger_of["revisions-lo.docx"],
        ledger_of["revisions.odt"],
    )
    # 修订表里那份被删掉的段落不是正文：office-doc 的段落数与 office-text 的行都要证明这点
    check(
        "revisions.odt 正文段数（删掉的那段不算）",
        dig(lbin("office-doc", fixture("revisions.odt")), "structure.paragraphs"),
        files["revisions.odt"]["odt"]["paragraphs"],
    )
    revtext = lbin("office-text", fixture("revisions.odt"))
    check(
        "revisions.odt 正文行",
        [one["text"] for one in revtext.get("paragraphs", [])],
        files["revisions.odt"]["odf"]["paragraphs"],
    )
    check(
        "revisions.odt 正文里没有被删掉的那笔",
        any("89000" in one["text"] for one in revtext.get("paragraphs", [])),
        False,
    )

    # ── 3ez2) 保护这份账：docx / xlsx / odt / ods 四家各存各的
    print("=== 3ez2) protected.* / locked-sheet.*：还能动吗 ===")
    for name in ("protected.docx", "protected-lo.docx"):
        got = lbin("office-doc", fixture(name)).get("protection") or {}
        want = files[name]["protection"]
        check("%s 保护逐字段" % name,
              {key: got.get(key) for key in ("element", "protected", "edit", "enforcement", "password")},
              {key: want.get(key) for key in ("element", "protected", "edit", "enforcement", "password")})
    # 分水岭：同一份内容转成 .odt 之后，LO 并没有把 docx 的编辑限制搬过去
    check("protected.odt 带着那份限制", files["protected.odt"]["protection"]["protected"], False)
    check("protected.odt 与 protected-lo.docx 结论不同",
          (bool((lbin("office-doc", fixture("protected.docx")).get("protection") or {}).get("protected")),
           bool((lbin("office-doc", fixture("protected.odt")).get("protection") or {}).get("protected"))),
          (True, False))
    for name in ("locked-sheet.xlsx", "locked-sheet-lo.xlsx"):
        got = lbin("office-sheet", fixture(name)).get("protection") or {}
        want = files[name]["protection"]
        check("%s 工作簿一层" % name,
              {key: (got.get("workbook") or {}).get(key) for key in ("element", "lock_structure", "book_password")},
              {key: (want.get("workbook") or {}).get(key) for key in ("element", "lock_structure", "book_password")})
        check("%s 每张表一层" % name,
              [(one.get("name"), one.get("element"), one.get("protected"), one.get("password"), one.get("written"))
               for one in got.get("sheets", [])],
              # 没有 sheetProtection 的那张表，两边都是「这一项没写」—— 拿 .get(..., False)
              # 一补就成了「写了没锁」，那是读者没说的话
              [(one["name"], one["element"], one["protected"], one.get("password"), one.get("written"))
               for one in want["sheets"]])
    # 拼法这一条是分水岭：openpyxl 写 1/0，LibreOffice 重写同一份东西写 true/false
    check("两种拼法读出同一个结论",
          [bool((lbin("office-sheet", fixture(one)).get("protection") or {}).get("sheets", [{}])[0].get("protected"))
           for one in ("locked-sheet.xlsx", "locked-sheet-lo.xlsx")],
          [True, True])
    ods = lbin("office-sheet", fixture("locked-sheet.ods")).get("protection") or {}
    check("locked-sheet.ods 每张表",
          [(one.get("name"), one.get("protected"), one.get("password"), one.get("digest"))
           for one in ods.get("sheets", [])],
          [(one["name"], one["protected"], one["password"], one["digest"])
           for one in files["locked-sheet.ods"]["protection"]["sheets"]])
    # 没写的东西不许被报成写了：book.xlsx 里 openpyxl 留了一个空的 workbookProtection
    plain = lbin("office-sheet", fixture("book.xlsx")).get("protection") or {}
    check("book.xlsx 空元素不等于锁上",
          (bool((plain.get("workbook") or {}).get("element")), (plain.get("workbook") or {}).get("lock_structure")),
          (True, None))
    check("book.xlsx 没有一张表被锁",
          [one.get("protected") for one in plain.get("sheets", [])],
          [one["protected"] for one in files["book.xlsx"]["protection"]["sheets"]])

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

    # ── 6) office-pdf：PDF 这张对象表 ───────────────────────────────
    # PDF 不是容器，是「对象表 + 若干流」。这一族的三条分水岭都单独钉住：
    # 对象流里那 51 个对象、没有 trailer 这个词的文件、以及加密时不许把密文当元数据
    print("=== 6) office-pdf：对象表、页树、加密与风险面 ===")
    # 去哪儿那一层：书签 / 页内链接 / 权限位（三份都是同一份读者的另一段代码）
    for name in ("notes.pdf", "deck.pdf", "objstm.pdf", "risk.pdf", "locked.pdf", "perms.pdf"):
        want = files[name]["pdf"]
        got = lbin("office-pdf", fixture(name))
        check("%s 书签树在不在" % name, dig(got, "outline.present"), want["outline"]["present"])
        check("%s 书签条数" % name, dig(got, "outline.total"), len(want["outline"]["items"]))
        check(
            "%s 书签逐条" % name,
            [(one.get("depth"), one.get("title"), one.get("page"), one.get("target_object"),
              one.get("form"), one.get("children"), one.get("closed"))
             for one in (got.get("outline") or {}).get("items", [])],
            [(one["depth"], one["title"], one["page"], one["page_object"],
              one["target"], one["children"], one["closed"])
             for one in want["outline"]["items"]],
        )
        check("%s 书签自报的 /Count" % name, dig(got, "outline.declared_count"), want["outline"]["declared_count"])
        check("%s 链接条数" % name, dig(got, "links.total"), len(want["links"]["internal"]) + len(want["links"]["external"]) + len(want["links"]["other"]))
        check(
            "%s 链接逐条" % name,
            # 加密件里 URI 是密文（两边都给 null），所以「算不算站外」要按类别看，
            # 不能只看有没有解出地址 —— 只看 uri 就会把 locked/perms 那条整个漏掉
            [(one.get("page"), one.get("to_object"), one.get("to_page"), one.get("uri"), one.get("via"))
             for one in got.get("links", {}).get("items", [])
             if one.get("uri") or one.get("to_object") or one.get("via") == "uri"],
            [(one["page"], one.get("target_object"), one.get("target_page"), None, one["via"])
             for one in want["links"]["internal"]]
            + [(one["page"], None, None, one["uri"], "uri") for one in want["links"]["external"]],
        )
        check("%s 外往条数" % name, dig(got, "links.external"), len(want["links"]["external"]))
        perm = want["permissions"]
        if perm.get("permissions") is None:
            check("%s 没权限这份账" % name, got.get("permissions"), None)
        else:
            check("%s /P 原值" % name, dig(got, "permissions.raw"), perm["raw"])
            check(
                "%s 权限逐位" % name,
                {key: dig(got, "permissions." + key) for key in
                 ("print", "modify", "copy", "annotate", "forms", "assemble", "print_high_quality")},
                {key: perm["permissions"][key] for key in
                 ("print", "modify", "copy", "annotate", "forms", "assemble", "print_high_quality")},
            )
    # 分水岭：加密的件里字符串是密文 —— 书签标题与 URI 必须是 null，而页号与位必须报得出
    sealed = lbin("office-pdf", fixture("perms.pdf"))
    check("perms.pdf 加密件的标题是 null 而页号有值",
          [(one.get("title"), one.get("page")) for one in sealed.get("outline", {}).get("items", [])],
          [(None, 1), (None, 1)])
    check("perms.pdf 加密件的 URI 是 null",
          [one.get("uri") for one in sealed.get("links", {}).get("items", [])], [None])
    # pdfinfo 读同一份：Encrypted: yes (print:no copy:no change:yes addNotes:no)
    check("perms.pdf 与 pdfinfo 的四个词一致",
          [dig(sealed, "permissions." + key) for key in ("print", "copy", "modify", "annotate")],
          [False, False, True, False])

    for name in ("notes.pdf", "deck.pdf", "objstm.pdf", "locked.pdf", "risk.pdf"):
        got = lbin("office-pdf", fixture(name))
        want = files[name]["pdf"]
        check("%s 版本号" % name, dig(got, "version"), want["version"])
        check("%s 二进制注释行" % name, dig(got, "binary_comment"), want["binary_comment"])
        check("%s 明写的对象数" % name, dig(got, "objects.plain"), want["objects"]["plain"])
        check("%s 对象流里的对象数" % name, dig(got, "objects.in_object_streams"),
              want["objects"]["in_object_streams"])
        check("%s 看得见的对象总数" % name, dig(got, "objects.total_seen"), want["objects"]["total_seen"])
        check("%s 重复对象号" % name, dig(got, "objects.duplicated_ids"), want["objects"]["duplicated_ids"])
        check("%s trailer 关键字次数" % name, dig(got, "xref.trailer_keyword"), want["xref"]["trailer_keyword"])
        check("%s 交叉引用流的号" % name, dig(got, "xref.xref_streams"), want["xref"]["xref_streams"])
        check("%s 页数" % name, dig(got, "pages.page_objects"), want["pages"]["page_objects"])
        check("%s 页树节点数" % name, dig(got, "pages.tree_nodes"), want["pages"]["pages_tree_nodes"])
        check("%s /Count 自报" % name, dig(got, "pages.counts"), want["pages"]["counts"])
        check("%s 页面尺寸" % name, dig(got, "pages.distinct_boxes"), want["pages"]["distinct_sizes"])
        check("%s 继承来的 MediaBox" % name, dig(got, "pages.inherited_boxes"),
              want["pages"]["inherited_mediabox"])
        check("%s 每页旋转" % name, dig(got, "pages.rotations"),
              [int(one or 0) for one in want["pages"]["rotations"]])
        ge, we = got.get("encryption"), want.get("encryption")
        check("%s 加密与否" % name, ge is None, we is None)
        if we is not None:
            check("%s 加密 Filter" % name, dig(got, "encryption.filter"), we["filter"])
            check("%s 加密 V" % name, dig(got, "encryption.v"), we["v"])
            check("%s 加密 R" % name, dig(got, "encryption.revision"), we["revision"])
            check("%s 密钥位数" % name, dig(got, "encryption.key_bits"), we["length_bits"])
            # 密文不当元数据：两边都不给
            check("%s 加密时不给元数据" % name, got.get("metadata"), None)
            check("%s 加密时 /Lang 不猜" % name, dig(got, "tags.lang"), None)
        else:
            check("%s 元数据逐项一致" % name, got.get("metadata") or {}, want["info"])
            check("%s /Lang" % name, dig(got, "tags.lang"), want["tags"]["lang"] or None)
        check("%s tagged 标志" % name, bool(dig(got, "tags.marked")), want["tags"]["marked"])
        check("%s StructTreeRoot" % name, bool(dig(got, "tags.struct_tree_root")),
              want["tags"]["struct_tree_root"])
        check(
            "%s 字体清单" % name,
            [(one["object"], one["base_font"], one["subtype"], one["encoding"],
              one["to_unicode"], one["from_object_stream"]) for one in got.get("fonts", [])],
            [(one["id"], one["base_font"], one["subtype"], one["encoding"],
              one["to_unicode"], one["in_object_stream"]) for one in want["fonts"]],
        )
        check(
            "%s 图片清单" % name,
            [(one["object"], one["width"], one["height"], one["filter"], one["color_space"],
              one["bits_per_component"]) for one in got.get("images", [])],
            [(one["id"], one["width"], one["height"], one["filter"], one["color_space"], one["bits"])
             for one in want["images"]],
        )
        for key in ("javascript", "launch", "uri_actions", "attachments", "acroform",
                    "fields", "open_action"):
            check("%s 风险项 %s" % (name, key), dig(got, "features.%s" % key), want["features"][key])

    # 三条分水岭各自的具体数：这三条只要有一条没做，上面的合计就会歪
    stm = lbin("office-pdf", fixture("objstm.pdf"))
    check("objstm.pdf 那个对象流自报 /N", dig(stm, "objects.object_streams[0].declared_n"), 51)
    check("objstm.pdf 解出来的对象数", dig(stm, "objects.object_streams[0].unpacked"), 51)
    check("objstm.pdf 对象流没有毛病", dig(stm, "objects.object_streams[0].problem"), None)
    check("objstm.pdf /Info 只在 XRef 流里指得出", dig(stm, "xref.info"), 52)
    # 51 不是抄来的：那份件唯一的 XRef 流写着 /Root 51 0 R，51 号对象的 /Type 就是
    # /Catalog，pikepdf 读同一份也报 (51, 0)。上一版这里写 9 —— 那是把注释里举的例子
    # 当成了量到的数，9 号在那份件里是一个 /StructElem
    check("objstm.pdf /Root 也只在 XRef 流里", dig(stm, "xref.root"), 51)
    plain = lbin("office-pdf", fixture("notes.pdf"))
    check("notes.pdf 的 /Root 从 trailer 指", dig(plain, "xref.root"), 74)
    check("notes.pdf 明写的对象比 objstm 多", dig(plain, "objects.plain") > dig(stm, "objects.plain"), True)
    lock = lbin("office-pdf", fixture("locked.pdf"))
    check("locked.pdf 的 watch 第一条是加密", dig(lock, "watch[0].kind"), "encrypted")
    check("locked.pdf 声明没解密", dig(lock, "encryption.decrypted"), False)
    risk = lbin("office-pdf", fixture("risk.pdf"))
    check("risk.pdf 的 Launch 动作进了 watch",
          [one["kind"] for one in risk.get("watch", [])].count("launch-action"), 1)

    # ── 6b) PDF 的正文：两套读者逐页对字 ───────────────────────────
    # 这一层的价值全在「顺序对」上：字都认得、顺序排错，输出看着像读通了其实没有
    for name in ("notes.pdf", "deck.pdf", "objstm.pdf", "locked.pdf", "risk.pdf"):
        got = lbin("office-pdf", fixture(name), "--text")
        want = files[name]["pdf"]["text"]
        check("%s 每页正文逐字一致" % name,
              [one["text"] for one in got.get("text", {}).get("pages", [])], want)
        check("%s 正文不是解不出来就当没有" % name,
              bool(dig(got, "text.order_from_page_tree")), True)
    body = lbin("office-text", fixture("notes.pdf"))
    check("office-text 也答得出 PDF 的正文",
          [one["text"] for one in body.get("paragraphs", [])][:2],
          ["一级标题：预算口径", "第三季度服务器预算为十二万四千元"])
    check("office-text 那份 PDF 的口径写清楚", dig(body, "kind"), "pages")
    locked_body = lbin("office-text", fixture("locked.pdf"))
    check("加密的 PDF 不报正文，只说明为什么", len(locked_body.get("paragraphs", [])), 0)

    failed = [one for one in RESULTS if not one[1]]
    print(f"=== 合计 {len(RESULTS)} 项，失败 {len(failed)} 项 ===")
    for name, _, detail in failed:
        print(f"  FAIL {name}: {detail}")
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
