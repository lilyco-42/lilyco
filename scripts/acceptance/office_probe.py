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
import zipfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import office_reader  # noqa: E402  第二读者（标准库实现）
import lyco_pages  # noqa: E402  那张纸（尺寸与边距）的第二读者：docx 与 odt 两家的独立实现
import lyco_grid  # noqa: E402  表格网格的第二读者（每行几个格、哪格被合并）

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
    """按点号取值，支持 a.b[0].c，也支持一步两个下标 a.b[0][1] —— 表格的网格是
    「行的数组，每一行又是格的数组」，只认一个下标就只能取到整行，比对不了单个格子"""
    here = payload
    for chunk in dotted.split("."):
        if here is None:
            return None
        at = chunk.find("[")
        if at < 0:
            name, rest = chunk, ""
        else:
            name, rest = chunk[:at], chunk[at:]
        if name:
            if not isinstance(here, dict) or name not in here:
                return None
            here = here[name]
        while rest.startswith("["):
            stop = rest.find("]")
            if stop < 0:
                return None
            try:
                which = int(rest[1:stop])
            except ValueError:
                return None
            if not isinstance(here, list) or which >= len(here):
                return None
            here = here[which]
            rest = rest[stop + 1:]
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
        "toc.docx": ("ooxml", "word", "docx"),
        "toc.odt": ("opendocument", "word", "odt"),
        "toc.rtf": ("rtf", "word", "rtf"),
        "comments.docx": ("ooxml", "word", "docx"),
        "comments.odt": ("opendocument", "word", "odt"),
        "comments.rtf": ("rtf", "word", "rtf"),
        "tables.docx": ("ooxml", "word", "docx"),
        "tables.odt": ("opendocument", "word", "odt"),
        "tables.rtf": ("rtf", "word", "rtf"),
        "paper-a4.docx": ("ooxml", "word", "docx"),
        "paper-a4.odt": ("opendocument", "word", "odt"),
        "paper-a4.rtf": ("rtf", "word", "rtf"),
        "tables-merged.docx": ("ooxml", "word", "docx"),
        "tables-merged.odt": ("opendocument", "word", "odt"),
        "notes-end.odt": ("opendocument", "word", "odt"),
        "notes-hf.odt": ("opendocument", "word", "odt"),
        "notes-hf.rtf": ("rtf", "word", "rtf"),
        "notes-end.rtf": ("rtf", "word", "rtf"),
        "notes.docm": ("ooxml", "word", "docm"),
        "notes-en.docx": ("ooxml", "word", "docx"),
        "book.xlsx": ("ooxml", "excel", "xlsx"),
        "deck.pptx": ("ooxml", "powerpoint", "pptx"),
        "deck-lo.pptx": ("ooxml", "powerpoint", "pptx"),
        "deck-chart.pptx": ("ooxml", "powerpoint", "pptx"),
        "deck-chart-lo.pptx": ("ooxml", "powerpoint", "pptx"),
        "chart.xlsx": ("ooxml", "excel", "xlsx"),
        "chart-lo.xlsx": ("ooxml", "excel", "xlsx"),
        "rules.xlsx": ("ooxml", "excel", "xlsx"),
        "rules-lo.xlsx": ("ooxml", "excel", "xlsx"),
        "view.xlsx": ("ooxml", "excel", "xlsx"),
        "view-lo.xlsx": ("ooxml", "excel", "xlsx"),
        "size.xlsx": ("ooxml", "excel", "xlsx"),
        "size-lo.xlsx": ("ooxml", "excel", "xlsx"),
        "para.docx": ("ooxml", "word", "docx"),
        "para.odt": ("opendocument", "word", "odt"),
        "para.rtf": ("rtf", "word", "rtf"),
        "tables-lo.docx": ("ooxml", "word", "docx"),
        "lists.docx": ("ooxml", "word", "docx"),
        "lists-lo.docx": ("ooxml", "word", "docx"),
        "lists.odt": ("opendocument", "word", "odt"),
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
        "cell-notes-many.xlsx": ("ooxml", "excel", "xlsx"),
        "cell-notes-many.xls": ("compound", "excel", "xls"),
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

    # 目录这一问三家的存法毫无共同点：OOXML 的级别在域指令的文字里（外面那层 w:sdt
    # 还可能没有），ODF 的级别在 source 元素的 outline-level 属性上，RTF 没有壳只有域
    # —— 所以整份账按各自的形状比，不强行归一（OOXML 专属的那两键 RTF 这边不该出现）
    print("=== 2b) 有没有目录、收了几级（toc.docx / toc.odt / toc.rtf） ===")
    for name in ("toc.docx", "notes.docx", "toc.odt", "notes.odt", "toc.rtf", "notes.rtf"):
        # 这个循环外头还有一个 `want` 装着 notes.docx 的整份账（后面十几条检查在用），
        # 所以这里必须换个名字 —— 复用 `want` 会把那份账换成本条循环的小字典
        if name.endswith(".docx"):
            cwant = files[name]["ooxml"]["contents"]
        elif name.endswith(".odt"):
            cwant = files[name]["odt"]["contents"]
        else:
            cwant = files[name]["rtf"]["contents"]
        check("%s 目录那份账" % name, lbin("office-doc", fixture(name)).get("contents"), cwant)
    check("notes.doc 没读就不报目录", lbin("office-doc", fixture("notes.doc")).get("contents"), None)
    # 同一份文档的两种写法：RTF 解掉那一对反斜杠之后，级数与 docx 是同一个字 ——
    # 这不是两边凑出来的，是那把读取器本来就只有一把（office_doc.rs 里 docx / rtf 共用）
    check(
        "toc 的级数：docx 与 rtf 同一个字（两家共用一把开关读取器）",
        [
            dig(lbin("office-doc", fixture("toc.docx")), "contents.levels"),
            dig(lbin("office-doc", fixture("toc.rtf")), "contents.levels"),
        ],
        ["1-2", "1-2"],
    )
    check(
        "toc.rtf 里没有 OOXML 那两键（不造假）",
        [
            (lbin("office-doc", fixture("toc.rtf")).get("contents") or {}).get(key, "没有这个键")
            for key in ("galleries", "sdt")
        ],
        ["没有这个键", "没有这个键"],
    )

    # ── 2c) RTF 的结构这一问：它不是包，是一条流，能数清的才报 ─────────────
    print("=== 2c) office-doc 读 RTF（段、注、图、跳过与注的口袋数） ===")
    for name in ("notes.rtf", "notes-hf.rtf", "notes-end.rtf", "tables.rtf", "toc.rtf", "comments.rtf"):
        got = lbin("office-doc", fixture(name))
        rwant = files[name]["rtf"]
        check(
            "%s 段数（RTF 的段 = par 切出来的行）" % name,
            dig(got, "structure.paragraphs"),
            rwant["line_count"],
        )
        check(
            "%s 注按 kind 分（尾注靠 ftnalt）" % name,
            [got.get("footnotes"), got.get("endnotes")],
            [
                len([one for one in rwant["notes"] if one["kind"] == "footnote"]),
                len([one for one in rwant["notes"] if one["kind"] == "endnote"]),
            ],
        )
        check(
            "%s 图 / 嵌入对象 / 跳过的群 / 注的口袋数" % name,
            [
                dig(got, "structure.pictures"),
                dig(got, "structure.embedded_objects"),
                dig(got, "structure.skipped_destinations"),
                dig(got, "structure.note_destinations"),
            ],
            [
                rwant["pictures"],
                rwant["embedded_objects"],
                rwant["skipped_destinations"],
                rwant["note_destinations"],
            ],
        )
        check(
            "%s 自己数的那份字账" % name,
            got.get("statistics", {}).get("ours"),
            office_reader.tally_of(rwant["lines"]),
        )
        # 「没看」与「没有」不是一件事：这几项两边都必须是 null。
        # 目录（contents）从这一批里出去了 —— 那一条流里有没有 TOC 域是数得出的，
        # 2b 那条循环按各自的形状比那份账（present=false 也要交，不装看不见）
        check(
            "%s 没判的项交回 null（tables / sections）" % name,
            [
                dig(got, "structure.sections") is None,
                (got.get("structure") or {}).get("tables") is None,
            ],
            [True, True],
        )
        # 字体与样式：那两群照旧整群跳过，但里面的名字要交出来；
        # 名字是不是敢读，由每个条目自己声明的字符集说
        check(
            "%s 字体与样式的账" % name,
            [
                [one.get("index") for one in got.get("font_list", [])],
                [one.get("name") for one in got.get("font_list", [])],
                got.get("styles"),
                [
                    dig(got, "structure.font_definitions"),
                    dig(got, "structure.style_definitions"),
                ],
            ],
            [
                [one["index"] for one in rwant["fonts"]],
                [one["name"] for one in rwant["fonts"]],
                {
                    one["name"]: one["count"] for one in rwant["style_uses"]
                },
                [len(rwant["fonts"]), len(rwant["styles"])],
            ],
        )
        # 表那一份：行数与格子数是控制字的条数，两个读者各数一遍
        check(
            "%s 表的四个计数（row / cell / trowd / intbl）" % name,
            [
                dig(got, "structure.table_rows"),
                dig(got, "structure.table_cells"),
                dig(got, "structure.table_row_defines"),
                dig(got, "structure.table_cell_paras"),
                dig(got, "structure.nested_table_rows"),
                dig(got, "structure.nested_table_cells"),
            ],
            [
                rwant["table_rows"],
                rwant["table_cells"],
                rwant["table_row_defines"],
                rwant["table_cell_paras"],
                rwant["nested_table_rows"],
                rwant["nested_table_cells"],
            ],
        )

        # 标题：层级就写在样式名里，与同一批字的 docx 那本账同一个形状
        check(
            "%s 标题（样式名 heading N 给的层级）" % name,
            [[one.get("level"), one.get("text")] for one in got.get("headings", [])],
            [[one["level"], one["text"]] for one in rwant["headings"]],
        )

        # 链接与域：域指令那一群照旧跳过，但链接从这里读出来；显示文字仍在正文里
        check(
            "%s 链接与域的条数" % name,
            [
                dig(got, "structure.fields"),
                [[one.get("target"), one.get("text")] for one in got.get("hyperlinks", [])],
            ],
            [
                rwant["fields"],
                [[one["target"], one["text"]] for one in rwant["links"]],
            ],
        )

    # 两张表那份（3×2 与 2×2）：同一批字的三副账要在行与格子上对得上，
    # 而「几张表」只有包着的两家敢报 —— RTF 那条流里判不住（规则在两份件上试过）
    print("=== 2c2) 表：三副账同样、tables 只两家报 ===")
    trtf = lbin("office-doc", fixture("tables.rtf"))
    tdocx = lbin("office-doc", fixture("tables.docx"))
    todt = lbin("office-doc", fixture("tables.odt"))
    check(
        "tables 三副账的行数与格子数",
        [
            [dig(trtf, "structure.table_rows"), dig(trtf, "structure.table_cells")],
            [
                dig(tdocx, "structure.table_rows"),
                dig(tdocx, "structure.table_cells"),
            ],
            [
                dig(todt, "structure.table_rows"),
                dig(todt, "structure.table_cells"),
            ],
        ],
        [[5, 10], [5, 10], [5, 10]],
    )
    check(
        "tables 那两张表：docx 与 odt 报 2，rtf 留 null",
        [
            trtf.get("structure", {}).get("tables") is None,
            tdocx.get("structure", {}).get("tables"),
            todt.get("structure", {}).get("tables"),
        ],
        [True, 2, 2],
    )
    check(
        "tables.rtf 的表账与读者一致",
        [
            dig(trtf, "structure.table_rows"),
            dig(trtf, "structure.table_cells"),
            dig(trtf, "structure.table_row_defines"),
            dig(trtf, "structure.table_cell_paras"),
            dig(trtf, "structure.paragraphs"),
        ],
        [
            files["tables.rtf"]["rtf"][k]
            for k in (
                "table_rows",
                "table_cells",
                "table_row_defines",
                "table_cell_paras",
                "line_count",
            )
        ],
    )

    # ── 2d) 那张纸：三家各写各的单位（docx 与 RTF 写 twips，odt 写「21.59cm」），
    # 换成 0.01mm 的整数之后逐条与 `lyco_pages.py` 对；三家之间谁不一致也照实交 ──────
    print("=== 2d) office-doc 的那张纸（尺寸与边距，三家三种单位） ===")
    # 四边边距三家不同的那一份件：docx 上下写 1440 twips，LibreOffice 的 odt 与 rtf
    # 两个导出都写 720 / 1.27cm。这是生产者的不一致，单独在下面钉住，不在这里当一致要求
    MARGIN_DIFFERS = {"notes-hf"}
    for stem in ("notes", "notes-hf", "notes-end", "tables", "toc", "paper-a4", "comments"):
        ledger = {}
        for ext in ("docx", "odt", "rtf"):
            name = "%s.%s" % (stem, ext)
            if not (FIXTURES / name).exists():
                continue
            got = lbin("office-doc", fixture(name))
            if ext == "rtf":
                want = [lyco_pages.rtf_entry(files[name]["rtf"]["paper_writes"])]
            else:
                want = lyco_pages.pages_of(FIXTURES / name)["papers"]
            setup = got.get("page_setup") or {}
            check("%s 那张纸整份账与读者一致" % name, setup.get("papers"), want)
            check("%s 单位在壳上说一次" % name, setup.get("unit"), "0.01mm")
            ledger[ext] = want
        rows = min(len(one) for one in ledger.values())
        for i in range(rows):
            sizes = {ext: (one[i]["width"], one[i]["height"]) for ext, one in ledger.items()}
            # 一个数从三家出来：这是整条换算链的地基
            check("%s 第%d张纸的纸面尺寸三家同一个数：%s" % (stem, i, sizes), len(set(sizes.values())), 1)
            if stem in MARGIN_DIFFERS:
                continue
            sides = {
                ext: tuple(one[i]["margins"][key] for key in ("top", "right", "bottom", "left"))
                for ext, one in ledger.items()
            }
            check("%s 第%d张纸的四边三家同一个数：%s" % (stem, i, sides), len(set(sides.values())), 1)
    # notes-hf：三家各报各的，谁也不替谁合并（OOXML 还另外写出两节）
    hf = {ext: lbin("office-doc", fixture("notes-hf." + ext)) for ext in ("docx", "odt", "rtf")}
    check(
        "notes-hf 上下边距：docx 2540，odt 与 rtf 1270（0.01mm）",
        [dig(hf[ext], "page_setup.papers[0].margins.top") for ext in ("docx", "odt", "rtf")],
        [2540, 1270, 1270],
    )
    check(
        "notes-hf 的 OOXML 两节各一条，另两家只有一条文档默认",
        [len(dig(hf[ext], "page_setup.papers") or []) for ext in ("docx", "odt", "rtf")],
        [2, 1, 1],
    )
    check(
        "orient 只交文件写了的：docx 与 rtf 竖排时不写，odt 明写 portrait",
        [
            dig(hf["docx"], "page_setup.papers[0].orient") is None,
            dig(hf["rtf"], "page_setup.papers[0].orient") is None,
            dig(hf["odt"], "page_setup.papers[0].orient"),
        ],
        [True, True, "portrait"],
    )
    check(
        "notes.doc 没看就不报纸（null 而不是空表）",
        lbin("office-doc", fixture("notes.doc")).get("page_setup"),
        None,
    )
    # 第二份尺寸（A4 + 一节横排）：换一个尺寸才知道换算不是凑上 Letter 的。
    # 三家的文档默认那一份在 0.01mm 上完全一致（21001×29700 —— 注意不是整数 21000×29700：
    # OOXML 与 RTF 把 A4 的短边写作 11906 twips，LibreOffice 的 ODF 又照抄成 21.001cm，
    # 所以这里**不给尺寸起名**，「A4」那种查表会在这三份件上全部落空）
    a4 = {ext: lbin("office-doc", fixture("paper-a4." + ext)) for ext in ("docx", "odt", "rtf")}
    check(
        "paper-a4 的文档默认那一份：三家同一个数",
        [
            [dig(a4[ext], "page_setup.papers[0].width"), dig(a4[ext], "page_setup.papers[0].height")]
            for ext in ("docx", "odt", "rtf")
        ],
        [[21001, 29700], [21001, 29700], [21001, 29700]],
    )
    # 横过来的那一节在 OOXML 与 ODF 里都写着（第一次有真件走到 orient=landscape），
    # 而 LibreOffice 的 RTF 导出整份文件一个 `\landscape` 都没写 —— 那一条流里就只有
    # 文档默认的纵向，第二节的横排看不见。两份件的差是文件的差，不是读者的差
    check(
        "paper-a4 横排那一节：docx 与 odt 有第二条，rtf 只有一条",
        [len(dig(a4[ext], "page_setup.papers") or []) for ext in ("docx", "odt", "rtf")],
        [2, 2, 1],
    )
    check(
        "paper-a4 第二条：宽高对调、orient 写着 landscape",
        [
            [
                dig(a4[ext], "page_setup.papers[1].width"),
                dig(a4[ext], "page_setup.papers[1].height"),
                dig(a4[ext], "page_setup.papers[1].orient"),
            ]
            for ext in ("docx", "odt")
        ],
        [[29700, 21001, "landscape"], [29700, 21001, "landscape"]],
    )
    check(
        "paper-a4.rtf 全文没有 landscape 这个词（所以那一条只能给 null）",
        [
            fixture("paper-a4.rtf").read_bytes().count(b"\\landscape"),
            dig(a4["rtf"], "page_setup.papers[0].orient"),
        ],
        [0, None],
    )

    # ── 2e) 表格的那张网：这张表自己几行、每行几个格子、哪一格被合并掉了 ─────
    # 与 `structure.table_rows` / `table_cells` 是两本账：那两个用 descendants 数
    # （嵌套表算进来），网格里走的是直接孩子
    print("=== 2e) office-doc 的表格网格（两家把合并写得不一样） ===")
    for name in ("notes.docx", "notes.odt", "tables.docx", "tables.odt",
                 "tables-merged.docx", "tables-merged.odt"):
        got = lbin("office-doc", fixture(name))
        want = lyco_grid.grids_of(FIXTURES / name)
        check(
            "%s 每张表的网格与读者一致" % name,
            [one.get("grid") for one in got.get("tables", [])],
            want,
        )
    # 同一张视觉上 2×3 的表：OOXML 那一行只写 2 个格（合掉的那一格整个不在文件里），
    # ODF 那一行写 3 个格（被盖住的那一格照样在，只是空的）。所以「格子数」是存储的数
    mdocx = lbin("office-doc", fixture("tables-merged.docx"))
    modt = lbin("office-doc", fixture("tables-merged.odt"))
    check(
        "横向合并那一行：docx 2 个格、odt 3 个格",
        [
            len(dig(mdocx, "tables[0].grid.rows[0]") or []),
            len(dig(modt, "tables[0].grid.rows[0]") or []),
        ],
        [2, 3],
    )
    check(
        "合并的写法两家不同：vMerge 说两头，ODF 直接写跨几行",
        [
            dig(mdocx, "tables[1].grid.rows[0][0].row_merge"),
            dig(mdocx, "tables[1].grid.rows[1][0].row_merge"),
            dig(modt, "tables[1].grid.rows[0][0].row_span"),
            dig(modt, "tables[1].grid.rows[1][0].covered"),
        ],
        ["restart", "continue", 2, True],
    )
    check(
        "跨几列两家都写 2，被盖住的那一格只有 ODF 有",
        [
            dig(mdocx, "tables[0].grid.rows[0][0].col_span"),
            dig(modt, "tables[0].grid.rows[0][0].col_span"),
            [one.get("covered") for one in (dig(mdocx, "tables[0].grid.rows[0]") or [])],
            [one.get("covered") for one in (dig(modt, "tables[0].grid.rows[0]") or [])],
        ],
        [2, 2, [False, False], [False, True, False]],
    )
    # 没有合并的那两份件：两家给出的网格必须一字不差（有合并的上面刚说清哪里不一样）
    plain_docx = lbin("office-doc", fixture("tables.docx"))
    plain_odt = lbin("office-doc", fixture("tables.odt"))
    check(
        "没合并的那两份件：两家的网格一字不差",
        [one.get("grid") for one in plain_docx.get("tables", [])],
        [one.get("grid") for one in plain_odt.get("tables", [])],
    )
    check(
        "网格的截断旗标：全交出来就是 false",
        [one.get("grid", {}).get("cut") for one in plain_docx.get("tables", [])],
        [False, False],
    )

    # ── 2f) 批注：同一段字在三家的第三种存法（RTF 把它写在流里） ──────────────
    # docx 有 word/comments.xml 那个部件，odt 的注嵌在正文段里面，RTF 两条列表分着写：
    # `{\*\atnauthor 名字}` 在前、`{\*\annotation 正文}` 在后，注自己带一个号 `atnref`
    print("=== 2f) office-doc / office-text 的批注（三家三种存法） ===")
    counts = {
        "docx": lbin("office-doc", fixture("comments.docx")).get("comments"),
        "odt": lbin("office-doc", fixture("comments.odt")).get("comments"),
        "rtf": lbin("office-doc", fixture("comments.rtf")).get("comments"),
    }
    check("comments 三家都数到两条", counts, {"docx": 2, "odt": 2, "rtf": 2})
    check(
        "notes 那一份也数到（docx 一个部件、RTF 一条注）",
        [
            lbin("office-doc", fixture("notes.docx")).get("comments"),
            lbin("office-doc", fixture("notes.rtf")).get("comments"),
            lbin("office-doc", fixture("notes-end.rtf")).get("comments"),
        ],
        [
            files["notes.docx"]["ooxml"]["comments"],
            len(files["notes.rtf"]["rtf"]["annotations"]),
            len(files["notes-end.rtf"]["rtf"]["annotations"]),
        ],
    )
    rcomments = files["comments.rtf"]["rtf"]
    rtext = lbin("office-text", fixture("comments.rtf"))
    # RTF 的侧账逐条比：出处、作者、字、注自己的号，以及「日期解不出来 = null、
    # 原样在 date_written 里」这一条口径（reader 那边的 `ref` 就是这里的 `anchor`）
    check(
        "comments.rtf 侧账逐条（出处 / 作者 / 字 / 号 / 日期）",
        [
            [
                one.get("from"),
                one.get("part"),
                one.get("author"),
                one.get("text"),
                one.get("anchor"),
                one.get("date"),
                one.get("date_written"),
            ]
            for one in rtext.get("paragraphs", [])
            if one.get("from")
        ],
        [
            ["comment", "rtf", one["author"], one["text"], one["ref"], None, one["date_written"]]
            for one in rcomments["annotations"]
        ],
    )
    check(
        "两条列表的条数各交一份（配不上时看得出来）",
        [
            dig(lbin("office-doc", fixture("comments.rtf")), "structure.annotation_authors"),
            lbin("office-doc", fixture("comments.rtf")).get("comments"),
            [rcomments["annotation_authors"], len(rcomments["annotations"])],
        ],
        [2, 2, [2, 2]],
    )
    # 注的字一份都不许混进正文：三行正文里没有一条注的字
    check(
        "comments.rtf 的字不混进正文",
        [
            any(one["text"] in (line.get("text") or "") for one in rcomments["annotations"])
            for line in rtext.get("paragraphs", [])
            if not line.get("from")
        ],
        [False, False, False],
    )
    # docx 那一份：作者与日期都齐（ISO），存法换了一个部件
    dwant = [one for one in files["comments.docx"]["ooxml"]["side_texts"] if one["from"] == "comment"]
    check(
        "comments.docx 侧账逐条（出处 / 部件 / 作者 / 日期 / 字）",
        [
            [one.get("from"), one.get("part"), one.get("author"), one.get("date"), one.get("text")]
            for one in lbin("office-text", fixture("comments.docx")).get("paragraphs", [])
            if one.get("from") == "comment"
        ],
        [[one["from"], one["part"], one["author"], one["date"], one["text"]] for one in dwant],
    )
    # 生产者的差（不是读者的差）：同一批字，中文作者名在 RTF 里被写成两个问号，
    # 而 LibreOffice 自己的 docx 导出照抄「刘奇」；两边都按文件写的交，不互相冒充
    check(
        "同一批字的作者名两家不同（RTF 丢了中文）",
        [
            [one.get("author") for one in rtext.get("paragraphs", []) if one.get("from")],
            [one.get("author") for one in dwant],
        ],
        [["liuqi", "??"], ["liuqi", "刘奇"]],
    )
    check(
        "批注的字两家一字不差（丢的只是作者名）",
        [one.get("text") for one in rtext.get("paragraphs", []) if one.get("from")],
        [one["text"] for one in dwant],
    )

    # ── 2g) 断点：换页在这一族有三种写法，子串数还会把 \pard 当成 \par ──────────
    print("=== 2g) RTF 的断点词（par / line / page / pagebb / pbb / sect） ===")
    for name in ("notes.rtf", "notes-hf.rtf", "notes-end.rtf", "tables.rtf",
                 "toc.rtf", "paper-a4.rtf", "comments.rtf"):
        got = lbin("office-doc", fixture(name))
        words = files[name]["rtf"]["break_words"]
        check("%s 断点词六个键与读者一致" % name, dig(got, "structure.break_words"), words)
        check(
            "%s 换页与分节收尾符（合起来的那个数）" % name,
            [dig(got, "structure.page_breaks"), dig(got, "structure.section_breaks")],
            [words["page"] + words["pagebb"] + words["pbb"], words["sect"]],
        )
    # Word 的一条 `w:br w:type="page"` 被 LibreOffice 的 RTF 导出写成 `\pagebb`：
    # 两家各自的数法不同，但「这份文档有一处换页」这个答案必须一样
    check(
        "同一批字的换页：docx 与 rtf 都是 1 处（写法两种）",
        [
            dig(lbin("office-doc", fixture("notes.docx")), "structure.page_breaks"),
            dig(lbin("office-doc", fixture("notes.rtf")), "structure.page_breaks"),
            dig(lbin("office-doc", fixture("toc.docx")), "structure.page_breaks"),
            dig(lbin("office-doc", fixture("toc.rtf")), "structure.page_breaks"),
        ],
        [1, 1, 1, 1],
    )
    # `\sect` 是收尾符：两节的件只写一个，所以这里只交写了几个，`sections` 仍留 null
    check(
        "两份两节的件各写 1 个 sect（节数不推断）",
        [
            [
                dig(lbin("office-doc", fixture(name)), "structure.section_breaks"),
                dig(lbin("office-doc", fixture(name)), "structure.sections"),
            ]
            for name in ("notes-hf.rtf", "paper-a4.rtf")
        ],
        [[1, None], [1, None]],
    )
    check(
        "子串数会骗人：notes.rtf 里 \\page 一条也没有（pagebb 才是那条换页）",
        [
            fixture("notes.rtf").read_bytes().count(b"\\page"),
            dig(lbin("office-doc", fixture("notes.rtf")), "structure.break_words.page"),
            dig(lbin("office-doc", fixture("notes.rtf")), "structure.break_words.pagebb"),
        ],
        [1, 0, 1],
    )
    # 同一份文档里的那一处换页，三家写成三种东西：docx 是正文里的 `w:br type="page"`，
    # ODF 是段落样式上的 `fo:break-before="page"`（正文里什么元素都没有），
    # RTF 是段属性上的 `\pagebb`。三家必须报同一个数 —— 以前 ODF 报 0（只数了
    # text:soft-page-break，那是渲染时落下的位置，不是作者要的换页）
    for stem in ("notes", "toc"):
        check(
            "%s 的那一处换页三家同一个数（三种写法）" % stem,
            [
                dig(lbin("office-doc", fixture("%s.%s" % (stem, ext))), "structure.page_breaks")
                for ext in ("docx", "odt", "rtf")
            ],
            [1, 1, 1],
        )
    check(
        "没有换页的三份件三家都是 0（soft-page-break 另给一个键）",
        [
            [
                dig(lbin("office-doc", fixture("%s.%s" % (stem, ext))), "structure.page_breaks")
                for ext in ("docx", "odt", "rtf")
            ]
            + [dig(lbin("office-doc", fixture("%s.odt" % stem)), "structure.soft_page_breaks")]
            for stem in ("comments", "paper-a4", "tables")
        ],
        [[0, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0]],
    )

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
    # 这份账自己取一份，不借那个一路被循环复用的 `want`（be00afa 就是被它带崩的：
    # 目录那条循环把 `want` 换成了一本小字典，下面十几条检查还当它是 notes.docx 的整份账）
    dwant = files["notes.docx"]["ooxml"]
    check("notes.docx 表格数", dig(doc, "structure.tables"), dwant["tables"])
    # 「多少字」那份账有两边：自己数的与生产者自报的（python-docx 写的那份全是 0）
    check("notes.docx 自己数的字与读者一致", dig(doc, "statistics.ours"), dwant["statistics"]["ours"])
    check("notes.docx 生产者自报的字数", dig(doc, "statistics.producer.words"), 0)
    check("notes.docx 生产者自报的页数", dig(doc, "statistics.producer.pages"), 1)
    check("notes.docx 行数", dig(doc, "structure.table_rows"), dwant["table_rows"])
    check("notes.docx 格子数", dig(doc, "structure.table_cells"), dwant["table_cells"])
    check("notes.docx 节数", dig(doc, "structure.sections"), dwant["sections"])
    check("notes.docx 批注数", doc.get("comments"), dwant["comments"])
    check("notes.docx 标题", doc.get("headings"), [{"level": one["level"], "text": one["text"]} for one in dwant["headings"]])
    check("notes.docx 链接", [one["target"] for one in doc.get("hyperlinks", [])], [one["target"] for one in dwant["hyperlinks"]])
    check("notes.docx 图片", doc.get("images"), dwant["media"])

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

    # ── 2h) 段落的格式与分栏：docx 写在段自己身上，ODF 要跳到它点名的样式 ──────
    print("=== 2h) office-doc 的段落格式与分栏（docx 与 odt 两族） ===")
    for name in sorted(one.name for one in FIXTURES.glob("*.docx")):
        got = lbin("office-doc", fixture(name)).get("structure", {})
        want = files[name]["ooxml"]
        check("%s 段落格式与读者一致" % name, got.get("paragraph_formats"), want.get("paragraph_formats"))
        check("%s 的分栏与读者一致" % name, got.get("columns"), want.get("columns"))
    for name in sorted(one.name for one in FIXTURES.glob("*.odt")):
        got = lbin("office-doc", fixture(name)).get("structure", {})
        want = files[name]["odt"]
        check("%s 段落格式与读者一致" % name, got.get("paragraph_formats"), want.get("paragraph_formats"))
        check("%s 的分栏与读者一致" % name, got.get("columns"), want.get("columns"))
    para = {ext: lbin("office-doc", fixture("para." + ext)) for ext in ("docx", "odt", "rtf")}
    check(
        "同一件事的两种字面：两端对齐在 docx 叫 both、在 ODF 叫 justify",
        [dig(para["docx"], "structure.paragraph_formats.list[0].alignment"),
         dig(para["odt"], "structure.paragraph_formats.list[0].alignment"),
         dig(para["docx"], "structure.paragraph_formats.list[1].alignment"),
         dig(para["odt"], "structure.paragraph_formats.list[1].alignment")],
        ["both", "justify", "right", "end"],
    )
    check(
        "单位也各交各的：twips 的 1701 与 cm 的 3cm（不换算）",
        [dig(para["docx"], "structure.paragraph_formats.list[0].indent.left"),
         dig(para["odt"], "structure.paragraph_formats.list[0].indent.fo:margin-left"),
         dig(para["docx"], "structure.paragraph_formats.list[0].spacing.before"),
         dig(para["odt"], "structure.paragraph_formats.list[0].spacing.fo:margin-top")],
        ["1701", "3cm", "120", "0.212cm"],
    )
    check(
        "行距的两种写法：docx 换 lineRule，ODF 换单位（150% 与 0.635cm）",
        [dig(para["docx"], "structure.paragraph_formats.list[0].spacing.lineRule"),
         dig(para["docx"], "structure.paragraph_formats.list[0].spacing.line"),
         dig(para["odt"], "structure.paragraph_formats.list[0].written.fo:line-height"),
         dig(para["docx"], "structure.paragraph_formats.list[1].spacing.lineRule"),
         dig(para["odt"], "structure.paragraph_formats.list[1].written.fo:line-height")],
        ["auto", "360", "150%", "exact", "0.635cm"],
    )
    check(
        "缩进的第二种单位：docx 是 leftChars=200，ODF 换成 loext:margin-left=2ic",
        [dig(para["docx"], "structure.paragraph_formats.list[2].indent.leftChars"),
         dig(para["docx"], "structure.paragraph_formats.list[2].chars_written"),
         dig(para["docx"], "structure.paragraph_formats.list[0].chars_written"),
         dig(para["odt"], "structure.paragraph_formats.list[3].indent.loext:margin-left"),
         dig(para["odt"], "structure.paragraph_formats.list[3].indent.fo:margin-left"),
         dig(para["odt"], "structure.paragraph_formats.list[0].indent.fo:margin-left")],
        ["200", True, False, "2ic", None, "3cm"],
    )
    check(
        "「没写」上不了榜，而 ODF 每段都点名一个样式：两份账的条数不同",
        [dig(para["docx"], "structure.paragraph_formats.checked"),
         dig(para["docx"], "structure.paragraph_formats.listed"),
         dig(para["odt"], "structure.paragraph_formats.checked"),
         dig(para["odt"], "structure.paragraph_formats.listed")],
        [6, 4, 5, 5],
    )
    check(
        "点名到 styles.xml 里那个样式：这一跳不通就说 null，不当成「没格式」",
        [dig(para["odt"], "structure.paragraph_formats.list[2].style"),
         dig(para["odt"], "structure.paragraph_formats.list[2].resolved"),
         dig(para["odt"], "structure.paragraph_formats.list[2].written"),
         dig(para["odt"], "structure.paragraph_formats.resolved")],
        ["Standard", False, None, 4],
    )
    check(
        "段里只挂着节属性那一段也上榜（它确实写了 pPr）",
        [dig(para["docx"], "structure.paragraph_formats.list[3].index"),
         dig(para["docx"], "structure.paragraph_formats.list[3].elements"),
         dig(para["docx"], "structure.paragraph_formats.list[3].alignment")],
        [4, ["sectPr"], None],
    )
    check(
        "分栏：docx 一节一条 w:cols（一栏就是没有 num），ODF 一个内联区一条区样式",
        [dig(para["docx"], "structure.columns.sections"),
         dig(para["docx"], "structure.columns.multi"),
         dig(para["docx"], "structure.columns.list[0].written"),
         dig(para["docx"], "structure.columns.list[1].written"),
         dig(para["odt"], "structure.columns.sections"),
         dig(para["odt"], "structure.columns.written")],
        [2, 1, {"space": "720"}, {"space": "425", "num": "2"}, 1, 1],
    )
    check(
        "ODF 的两栏还各写一份相对宽度与自己的内缩",
        [dig(para["odt"], "structure.columns.list[0].written"),
         dig(para["odt"], "structure.columns.list[0].name"),
         [one.get("style:rel-width") for one in (dig(para["odt"], "structure.columns.list[0].parts") or [])],
         dig(para["odt"], "structure.columns.list[0].parts[1].fo:start-indent"),
         dig(para["odt"], "structure.columns.list[0].dont_balance")],
        [{"fo:column-count": "2", "fo:column-gap": "0.751cm"}, "TextSection",
         ["32767*", "32768*"], "0.375cm", "true"],
    )
    # RTF 这一族两份账都不交：LibreOffice 的导出在每一段前面把样式的默认重发一遍
    # （`\pard\plain\s0…\sa200` 段段都有），「这一段自己写了什么」与样式分不开；
    # 而全文没有一个 `\cols`。`.doc` 同理：段格式在表流里，这一族不判
    for name in ("para.rtf", "notes.rtf", "toc.rtf"):
        got = lbin("office-doc", fixture(name)).get("structure", {})
        check("%s 这一段不报段落格式与分栏：两个键整个不在" % name,
              [one for one in ("paragraph_formats", "columns") if got.get(one) is not None], [])
    for name in ("notes.doc", "notes-en.doc"):
        got = lbin("office-doc", fixture(name)).get("structure", {})
        check("%s 也不报：段格式在表流里" % name,
              [one for one in ("paragraph_formats", "columns") if got.get(one) is not None], [])

    # ODF 那句「这份文档有几段」有两个答案，两个都得看得见：注是坐在正文段**里面**的
    # （`text:p > text:note > text:note-body > text:p`），不落进段里数得 4、全树数得 7，
    # 而 LibreOffice 自己写在 meta.xml 的那个 `paragraph-count` 是 7。
    # 段格式那份账取的是 4 这一份清单（与 `structure.paragraphs` 同一份，index 才对得上）；
    # 这一条钉住的就是「两份账用的是同一份段清单」，别再让 7 与 4 在同一栏里各数各的
    endoff = lbin("office-doc", fixture("notes-end.odt"))
    check(
        "notes-end.odt 的「几段」：正文段 4、连注里的段 7（生产者自己写 7）",
        [dig(endoff, "structure.paragraphs"),
         dig(endoff, "structure.paragraph_formats.checked"),
         dig(endoff, "statistics.producer.paragraph-count"),
         files["notes-end.odt"]["odt"]["paragraphs"]],
        [4, 4, "7", 7],
    )

    # ── 2i) 列表与编号：三条来源、三跳、两家的级别数法 ────────────────────
    print("=== 2i) office-doc 的编号那份账（docx 那一路与 ODF 那一跳） ===")
    for name in sorted(one.name for one in FIXTURES.glob("*.docx")):
        got = lbin("office-doc", fixture(name)).get("structure", {})
        want = files[name]["ooxml"]
        check("%s 编号与读者一致" % name, got.get("numbering"), want.get("numbering"))
    for name in sorted(one.name for one in FIXTURES.glob("*.odt")):
        got = lbin("office-doc", fixture(name)).get("structure", {})
        want = files[name]["odt"]
        check("%s 编号与读者一致" % name, got.get("numbering"), want.get("numbering"))
    lists = {ext: lbin("office-doc", fixture("lists." + ext)) for ext in ("docx", "odt")}
    listdoc, listodt = lists["docx"], lists["odt"]
    # 那三段是样式替它们点的编号：段上一个编号属性都没有
    check(
        "编号有三个来源：段上、样式上、以及两家都写",
        [dig(listdoc, "structure.numbering.list[0].from"),
         dig(listdoc, "structure.numbering.list[3].from"),
         dig(listdoc, "structure.numbering.via_style"),
         dig(listdoc, "structure.numbering.on_paragraph"),
         dig(listdoc, "structure.numbering.both")],
        ["style", "paragraph", 3, 3, 0],
    )
    # numId 与 abstractNumId 是分开编号的：5 指的是 7，1 指的是 8
    check(
        "那两本号是分开编的，照着号跳就跳错了",
        [dig(listdoc, "structure.numbering.list[0].num_id"),
         dig(listdoc, "structure.numbering.list[0].abstract"),
         dig(listdoc, "structure.numbering.definitions[0].num_id"),
         dig(listdoc, "structure.numbering.definitions[0].abstract")],
        ["5", "7", "1", "8"],
    )
    # 圆点不是 •：那是 Symbol 字体里的私用区码位，字体名挂在同一级的 w:rPr 上
    check(
        "圆点那一级的字是私用区码位，字体写在 rPr 上",
        [dig(listdoc, "structure.numbering.list[3].level.written.numFmt"),
         dig(listdoc, "structure.numbering.list[3].level.written.lvlText"),
         dig(listdoc, "structure.numbering.list[3].level.fonts.ascii"),
         dig(listdoc, "structure.numbering.list[3].level.written.pStyle")],
        ["bullet", "\uf0b7", "Symbol", "ListBullet3"],
    )
    # 一份定义只有一级（multiLevelType=singleLevel）时，点第二级就是点空的
    check(
        "点了没有的那一级：交「没找到」而不是交第一级",
        [dig(listdoc, "structure.numbering.list[4].ilvl"),
         dig(listdoc, "structure.numbering.list[4].abstract"),
         dig(listdoc, "structure.numbering.list[4].level_found"),
         dig(listdoc, "structure.numbering.list[0].ilvl"),
         dig(listdoc, "structure.numbering.list[0].level_found")],
        ["1", "5", False, None, False],
    )
    # 点了一个不存在的 numId：解不开这一段要看得见，不能与「没编号」混成一谈
    check(
        "点名点空的那一段：numId 77 在 numbering.xml 里没有",
        [dig(listdoc, "structure.numbering.list[5].num_id"),
         dig(listdoc, "structure.numbering.list[5].resolved"),
         dig(listdoc, "structure.numbering.unresolved"),
         dig(listdoc, "structure.numbering.used")],
        ["77", False, 1, ["5", "1", "3", "77"]],
    )
    lstlo = lbin("office-doc", fixture("lists-lo.docx"))
    # 同一个格式重写一次：编号搬到段上也留在样式上（两份都说），级别补齐九级，
    # 缩进换了属性名（start 而不是 left），对齐词也换了（start 而不是 left），
    # 而那个不存在的 77 被改写成 0 —— 两家都没有 numId 那一条
    check(
        "重写一次之后：两份都写、级别补齐、属性换名",
        [dig(lstlo, "structure.numbering.both"),
         dig(lstlo, "structure.numbering.list[0].from"),
         dig(lstlo, "structure.numbering.list[0].ilvl"),
         dig(lstlo, "structure.numbering.list[0].level.written.lvlJc"),
         dig(lstlo, "structure.numbering.list[0].level.indent")],
        [3, "both", "0", "start", {"start": "360", "hanging": "360"}],
    )
    check(
        "重写那一家把 77 改写成了 0，也还是点空的",
        [dig(lstlo, "structure.numbering.used"),
         dig(lstlo, "structure.numbering.list[5].num_id"),
         dig(lstlo, "structure.numbering.list[5].resolved"),
         dig(lstlo, "structure.numbering.nums"),
         dig(lstlo, "structure.numbering.definitions[0].written")],
        [["4", "1", "3", "0"], "0", False, 7, {}],
    )
    # ODF：级别是嵌套层数（不是属性），而且那一份定义整个不在 content.xml 里
    check(
        "ODF 的级别是套出来的：四层 list、五段坐在 list-item 里",
        [dig(listodt, "structure.numbering.lists"),
         dig(listodt, "structure.numbering.items"),
         dig(listodt, "structure.numbering.in_list"),
         dig(listodt, "structure.numbering.max_depth")],
        [4, 5, 5, 2],
    )
    # 两家的级别数法不同：docx 写 0 基的 w:ilvl，ODF 写 1 基的 text:level
    check(
        "那一跳跨部件：段样式在 content.xml，定义全在 styles.xml",
        [dig(listodt, "structure.numbering.list[0].style_part"),
         dig(listodt, "structure.numbering.list[0].list_part"),
         dig(listodt, "structure.numbering.in_content"),
         dig(listodt, "structure.numbering.in_styles"),
         dig(listodt, "structure.numbering.styles")],
        ["content.xml", "styles.xml", 0, 10, 10],
    )
    check(
        "同一级在两家的号不同：docx 的 0 与 ODF 的 1",
        [dig(listdoc, "structure.numbering.list[3].ilvl"),
         dig(listodt, "structure.numbering.list[3].depth"),
         dig(listodt, "structure.numbering.list[3].level.level"),
         dig(listodt, "structure.numbering.list[0].level.kind"),
         dig(listodt, "structure.numbering.list[0].level.written.style:num-format")],
        ["0", 1, "1", "list-level-style-number", "1"],
    )
    # 套在里面那一层的 text:list 连样式名都不写；点空的那一段在 ODF 写成空串
    check(
        "嵌套那一层不点名，「不套列表」写成空串",
        [dig(listodt, "structure.numbering.list[4].chain"),
         dig(listodt, "structure.numbering.list[4].depth"),
         dig(listodt, "structure.numbering.list[5].list_style"),
         dig(listodt, "structure.numbering.list[5].depth"),
         dig(listodt, "structure.numbering.list[5].resolved")],
        [["WWNum3", None], 2, "", 0, False],
    )
    # 定义了但整份文档没用上，是常事（模板与 LibreOffice 都会留十份）
    plainodt = lbin("office-doc", fixture("notes.odt"))
    check(
        "定义了没人用：notes.odt 一份列表都没套，却带着十份定义",
        [dig(plainodt, "structure.numbering.listed"),
         dig(plainodt, "structure.numbering.styles"),
         dig(plainodt, "structure.numbering.in_list")],
        [0, 10, 0],
    )
    # RTF 与 .doc 这一族不交这份账：键整个不在（那一族的列表住在 \listtable 与
    # \pntext 那一套里，还没读到；.doc 的在表流里）
    for name in ("notes.rtf", "toc.rtf", "para.rtf"):
        got = lbin("office-doc", fixture(name)).get("structure", {})
        check("%s 不报编号：这个键整个不在" % name,
              [one for one in ("numbering",) if got.get(one) is not None], [])
    for name in ("notes.doc", "notes-en.doc"):
        got = (lbin("office-doc", fixture(name)).get("structure") or {})
        check("%s 也不报编号：那一份在表流里" % name,
              [one for one in ("numbering",) if got.get(one) is not None], [])

    # ── 2j) 这张表多宽：docx 三本账、ODF 一跳、两家换算是同一个数 ──────────
    print("=== 2j) office-doc 的表宽（三本账各数各的） ===")
    for name in sorted(one.name for one in FIXTURES.glob("*.docx")):
        got = lbin("office-doc", fixture(name)).get("structure", {})
        want = files[name]["ooxml"]
        check("%s 表宽与读者一致" % name, got.get("table_layouts"), want.get("table_layouts"))
    for name in sorted(one.name for one in FIXTURES.glob("*.odt")):
        got = lbin("office-doc", fixture(name)).get("structure", {})
        want = files[name]["odt"]
        check("%s 表宽与读者一致" % name, got.get("table_layouts"), want.get("table_layouts"))
    plain = lbin("office-doc", fixture("tables.docx"))
    wide = lbin("office-doc", fixture("tables-merged.docx"))
    relo = lbin("office-doc", fixture("tables-lo.docx"))
    odt = lbin("office-doc", fixture("tables.odt"))
    odtw = lbin("office-doc", fixture("tables-merged.odt"))
    # 「这张表多宽」在 OOXML 有三本账，而且第一本可以什么都没说
    check(
        "python-docx 那份：w:tblW 写的是 auto/0（说了等于没说）",
        [dig(plain, "structure.table_layouts.listed"),
         dig(plain, "structure.table_layouts.with_tblW"),
         dig(plain, "structure.table_layouts.auto"),
         dig(plain, "structure.table_layouts.list[0].w"),
         dig(plain, "structure.table_layouts.list[0].kind"),
         dig(plain, "structure.table_layouts.list[0].mm")],
        [2, 2, 2, "0", "auto", 0],
    )
    # 同一个格式重写一次：那一家把它换成实数，还补齐对齐/缩进/固定布局/单元格边距
    check(
        "LibreOffice 重写：auto/0 换成 8640 dxa，另外四样一并写出",
        [dig(relo, "structure.table_layouts.auto"),
         dig(relo, "structure.table_layouts.list[0].w"),
         dig(relo, "structure.table_layouts.list[0].mm"),
         dig(relo, "structure.table_layouts.list[0].align"),
         dig(relo, "structure.table_layouts.list[0].layout"),
         dig(relo, "structure.table_layouts.list[0].cell_mar"),
         dig(relo, "structure.table_layouts.list[0].indent")],
        [0, "8640", 15240, "start", "fixed", True, {"w": "108", "type": "dxa"}],
    )
    # 网格那本两家一字不差；横向合并那一格自己写的是两列之和
    check(
        "网格两样都对得上，合并格报的是那一格的宽",
        [dig(plain, "structure.table_layouts.list[0].grid[0].w"),
         dig(relo, "structure.table_layouts.list[0].grid[0].w"),
         dig(wide, "structure.table_layouts.list[0].cols"),
         dig(wide, "structure.table_layouts.list[0].cells[0].written.w"),
         dig(wide, "structure.table_layouts.list[0].cells[0].span"),
         dig(wide, "structure.table_layouts.list[0].cells[1].written.w")],
        ["4320", "4320", 3, "5760", "2", "2880"],
    )
    # ODF：列是**一条元素顶几列**，宽度一跳在列样式上，而那份样式在 content.xml
    check(
        "ODF 一条 table-column 顶两列，宽度在样式那一跳",
        [dig(odt, "structure.table_layouts.column_elements"),
         dig(odt, "structure.table_layouts.covered"),
         dig(odt, "structure.table_layouts.resolved"),
         dig(odt, "structure.table_layouts.list[0].columns[0].repeated"),
         dig(odt, "structure.table_layouts.list[0].columns[0].style_part"),
         dig(odt, "structure.table_layouts.list[0].columns[0].width")],
        [2, 4, 2, 2, "content.xml", {"style:column-width": "7.62cm"}],
    )
    # 跨家族的好 pin：8640 twips 与 15.24cm、4320 与 7.62cm 换成同一个 0.01mm 数
    check(
        "换算是同一个数：docx 的 twips 与 ODF 自带单位的串",
        [dig(relo, "structure.table_layouts.list[0].mm"),
         dig(odt, "structure.table_layouts.list[0].mm"),
         dig(relo, "structure.table_layouts.grid_sum"),
         dig(odt, "structure.table_layouts.list[0].columns[0].mm"),
         dig(plain, "structure.table_layouts.grid_sum")],
        [15240, 15240, 30480, 7620, 30480],
    )
    check(
        "合并那张表的 ODF 侧：三条列宽 5.08cm（= 2880 twips）",
        [dig(otdw, "structure.table_layouts.covered"),
         dig(otdw, "structure.table_layouts.list[0].columns[0].repeated"),
         dig(otdw, "structure.table_layouts.list[0].columns[0].mm"),
         dig(wide, "structure.table_layouts.list[0].grid[0].w")],
        [5, 3, 5080, "2880"],
    )
    for name in ("tables.rtf",):
        got = lbin("office-doc", fixture(name)).get("structure", {})
        check("%s 不报表宽：RTF 里没有「表宽」这个东西（只有 \intbl 与格分隔）" % name,
              [one for one in ("table_layouts",) if got.get(one) is not None], [])
    for name in ("notes.doc",):
        got = (lbin("office-doc", fixture(name)).get("structure") or {})
        check("%s 也不报表宽：那一份在表流里" % name,
              [one for one in ("table_layouts",) if got.get(one) is not None], [])

    # ── 2f) 演示稿的第二生产者与那两张图：同一份稿子中一家会重写什么 ──────
    print("=== 2f) 两家写的 pptx 与页上的图 ===")
    for name in ("deck.pptx", "deck-lo.pptx", "deck-chart.pptx", "deck-chart-lo.pptx"):
        want = files[name]["ooxml"]
        got = lbin("office-slide", fixture(name))
        check("%s 页数" % name, len(got.get("slides", [])), want["slide_count"])
        check("%s 每页的图整份账（部件、类型、系列与缓存）" % name,
              [one.get("chart_list") for one in got.get("slides", [])],
              [one.get("chart_list") for one in want["slides"]])
        check("%s 每页图的条数" % name, [one.get("charts") for one in got.get("slides", [])],
              [one.get("charts") for one in want["slides"]])
        check("%s 每页 run 的条数（a:t）" % name,
              [one.get("text_runs") for one in got.get("slides", [])],
              [one.get("text_runs") for one in want["slides"]])
        check("%s 那张纸的尺寸与 type（没写就是 null）" % name,
            [dig(got, "size.cx"), dig(got, "size.cy"), dig(got, "size.type")],
            [int(want["slide_size"].split("x")[0]), int(want["slide_size"].split("x")[1].split(":")[0]),
             want["slide_size_type"]])
        check("%s 版式与母版的条数" % name,
              [len(got.get("layouts", [])), len(got.get("masters", []))],
              [len(want["layouts"]), len(want["masters"])])
    plain = lbin("office-slide", fixture("deck.pptx"))
    again = lbin("office-slide", fixture("deck-lo.pptx"))
    deck1 = lbin("office-slide", fixture("deck-chart.pptx"))
    deck2 = lbin("office-slide", fixture("deck-chart-lo.pptx"))
    check(
        "同一段字在一家是一个 run、在另一家是三个：段落数不变",
        [[one.get("paragraph_total") for one in plain.get("slides", [])],
         [one.get("paragraph_total") for one in again.get("slides", [])],
         [one.get("text_runs") for one in plain.get("slides", [])],
         [one.get("text_runs") for one in again.get("slides", [])]],
        [[3, 1], [3, 1], [3, 5], [5, 5]],
    )
    check(
        "两家的段落文本逐条一致（run 拆分看不见了）",
        [[p2.get("text") for p2 in one.get("paragraphs", [])] for one in plain.get("slides", [])],
        [[p2.get("text") for p2 in one.get("paragraphs", [])] for one in again.get("slides", [])],
    )
    check(
        "尺寸两个数一致，type 只有 python-pptx 那份写了",
        [dig(plain, "size.cx"), dig(again, "size.cx"), dig(plain, "size.type"),
         dig(again, "size.type")],
        [9144000, 9144000, "screen4x3", None],
    )
    check(
        "一页两张图：柱形两条系列、饼图一条，第二页 0 张",
        [[one.get("charts") for one in deck1.get("slides", [])],
         [dig(deck1, "slides[0].chart_list[0].groups[0].kind"),
          dig(deck1, "slides[0].chart_list[0].groups[0].series"),
          dig(deck1, "slides[0].chart_list[1].groups[0].kind"),
          dig(deck1, "slides[0].chart_list[1].groups[0].series")]],
        [[2, 0], ["barChart", 2, "pieChart", 1]],
    )
    with zipfile.ZipFile(fixture("deck-chart-lo.pptx")) as box:
        in_chart_dir = sorted(one for one in box.namelist() if one.startswith("ppt/charts/"))
    check(
        "图只认页的关系表：LO 另塞进 ppt/charts/ 的 style 与 colors 部件不算图",
        [len((deck2.get("slides") or [{}])[0].get("chart_list", [])), len(in_chart_dir)],
        [2, 8],
    )
    check(
        "那个目录里确实有 style 与 colors 部件（按目录数就会数成六张图）",
        sorted(one.rsplit("/", 1)[-1] for one in in_chart_dir if one.endswith(".xml")),
        ["chart1.xml", "chart2.xml", "colors1.xml", "colors2.xml",
         "style1.xml", "style2.xml"],
    )
    check(
        "引用串照文件交：LO 重写后 c:f 里写的是 label 0 而不是 Sheet1!$B$1",
        [dig(deck1, "slides[0].chart_list[0].groups[0].series_list[0].name.ref"),
         dig(deck2, "slides[0].chart_list[0].groups[0].series_list[0].name.ref"),
         dig(deck1, "slides[0].chart_list[0].groups[0].series_list[0].cat.ref"),
         dig(deck2, "slides[0].chart_list[0].groups[0].series_list[0].cat.ref"),
         dig(deck1, "slides[0].chart_list[0].groups[0].series_list[0].val.ref"),
         dig(deck2, "slides[0].chart_list[0].groups[0].series_list[0].val.ref")],
        ["Sheet1!$B$1", "label 0", "Sheet1!$A$2:$A$3", "categories",
         "Sheet1!$B$2:$B$3", "0"],
    )
    check(
        "引用丢了缓存还在：两家画的数一模一样",
        [dig(deck1, "slides[0].chart_list[0].groups[0].series_list[0].val.cache.values"),
         dig(deck2, "slides[0].chart_list[0].groups[0].series_list[0].val.cache.values"),
         dig(deck1, "slides[0].chart_list[0].groups[0].series_list[0].val.cache.written"),
         dig(deck2, "slides[0].chart_list[0].groups[0].series_list[0].val.cache.written"),
         dig(deck2, "slides[0].chart_list[1].groups[0].series_list[0].val.cache.values")],
        [[10.0, 25.0], [10.0, 25.0], "2", "2", [124000.0, 18000.0]],
    )
    check(
        "轴 id 各排各的：柱形两条、饼图没有",
        [len(dig(deck1, "slides[0].chart_list[0].groups[0].axis_ids") or []),
         len(dig(deck2, "slides[0].chart_list[0].groups[0].axis_ids") or []),
         len(dig(deck1, "slides[0].chart_list[1].groups[0].axis_ids") or [])],
        [2, 2, 0],
    )
    check(
        "标题只有 LO 那一份给饼图写了",
        [dig(deck1, "slides[0].chart_list[1].title.via"), dig(deck1, "slides[0].chart_list[1].title.text"),
         dig(deck2, "slides[0].chart_list[1].title.via"), dig(deck2, "slides[0].chart_list[1].title.text")],
        [None, None, "text", "占比"],
    )
    # .ppt 这一家的图还没读：它住在二进制记录树里（.odp 已走嵌入对象那一跳，见 3a11）
    check("deck.ppt 这一族的图没读：没有一页带 charts 那份账",
          len([one for one in lbin("office-slide", fixture("deck.ppt")).get("slides", [])
               if one.get("charts") is not None]), 0)

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
    # 这一支现在也有侧账了（那一份流里有一条批注），所以正文行要挑「没有 from 的那些」——
    # 与页眉页脚、注那几条同一个口径
    check("notes.rtf 逐行正文", [one["text"] for one in rtf.get("paragraphs", []) if not one.get("from")],
          files["notes.rtf"]["rtf"]["lines"])
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
    # 第四种存法：.xls 的注在同一条流的两类记录里（一条给字、一条给格子与作者），
    # 这份件一次只改一个变量：ASCII 与中文的作者名、2 与 3 个字、带换行的字、
    # AA100 这种两位列名、以及分到两张表上
    xlsbook = lbin("office-sheet", fixture("cell-notes-many.xls"))
    xwant = files["cell-notes-many.xls"]["biff"]["comments"]
    check(
        "cell-notes-many.xls 每张表的批注（按格子对）",
        mine_notes(xlsbook),
        {
            key: {had["ref"]: (had["author"], had["text"], had["date"]) for had in value["list"]}
            for key, value in xwant.items()
        },
    )
    check("cell-notes-many.xls 批注总账", dig(xlsbook, "workbook.totals.comments"),
          sum(len(value["list"]) for value in xwant.values()))
    check(
        "cell-notes-many.xls 两类记录的条数一起交",
        {
            one.get("name"): (one.get("note_text_records"), one.get("note_cell_records"), one.get("comments"))
            for one in xlsbook.get("sheets", [])
        },
        {
            key: (value["text_records"], value["cell_records"], len(value["list"]))
            for key, value in xwant.items()
        },
    )
    check(
        "cell-notes-many.xls 每条都说自报的字数切满了",
        [had.get("whole") for one in xlsbook.get("sheets", []) for had in one.get("comment_list", [])],
        [had["whole"] for value in xwant.values() for had in value["list"]],
    )
    # 同一批字跨存法：LibreOffice 写的那份 .xls 与 openpyxl 写的那份 xlsx 必须一字不差
    check(
        "同一批字的 .xls 与 xlsx 两份批注账",
        sorted(
            (one.get("name"), had.get("ref"), had.get("author"), had.get("text"))
            for one in xlsbook.get("sheets", [])
            for had in one.get("comment_list", [])
        ),
        sorted(
            (key, had["ref"], had["author"], had["text"])
            for key, value in files["cell-notes-many.xlsx"]["comments"].items()
            for had in value
        ),
    )
    # 反面对照：没有注的那几份 .xls 报 0 条（键在、值为零），而不是缺键
    check(
        "book.xls 没有注就报 0 条",
        [dig(lbin("office-sheet", fixture(one)), "workbook.totals.comments") for one in
         ("book.xls", "hidden.xls", "formats.xls", "mulrk.xls")],
        [0, 0, 0, 0],
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

    # ── 3a7) 每张表的打印设置：三个元素各自在不在，缺的交 null，单位按各家原样 ──
    print("=== 3a7) office-sheet 的打印设置（xlsx 的两个生产者） ===")
    for name in sorted(one.name for one in FIXTURES.glob("*.xlsx")):
        want = files[name]["ooxml"]["print_setup"]
        got = lbin("office-sheet", fixture(name))
        mine = {
            (one.get("part") or "").rsplit("/", 1)[-1][: -len(".xml")]: one.get("print_setup")
            for one in got.get("sheets", [])
        }
        # 整份账逐属性对：边距的 0.5 与 0.511811023622047 那种差也必须留在两边
        check("%s 每张表的打印设置与读者一致" % name, mine, want)
    op = lbin("office-sheet", fixture("hidden.xlsx"))
    lo = lbin("office-sheet", fixture("hidden-lo.xlsx"))
    check(
        "同一张表：openpyxl 不写 pageSetup 与 printOptions（null，不是 false）",
        [dig(op, "sheets[0].print_setup.setup") is None,
         dig(op, "sheets[0].print_setup.options") is None],
        [True, True],
    )
    check(
        "同一张表：LibreOffice 连 paperSize 与两个 dpi 都不省",
        [dig(lo, "sheets[0].print_setup.setup.paperSize"),
         dig(lo, "sheets[0].print_setup.setup.orientation"),
         dig(lo, "sheets[0].print_setup.setup.horizontalDpi"),
         dig(lo, "sheets[0].print_setup.setup.verticalDpi"),
         dig(lo, "sheets[0].print_setup.setup.copies")],
        ["9", "portrait", "300", "300", "1"],
    )
    check(
        "边距按文件原样交：0.5 与 0.511811023622047 是两个生产者的差",
        [dig(op, "sheets[0].print_setup.margins.header"),
         dig(lo, "sheets[0].print_setup.margins.header"),
         dig(op, "sheets[0].print_setup.margins.left"),
         dig(lo, "sheets[0].print_setup.margins.left")],
        ["0.5", "0.511811023622047", "0.75", "0.75"],
    )
    check(
        "单位在壳上说一次：这一族是英寸（不是文档那一族的 0.01mm）",
        [dig(op, "sheets[0].print_setup.margin_unit"), dig(lo, "sheets[0].print_setup.margin_unit")],
        ["inch", "inch"],
    )
    # 另两族：量过才知道读不了。ODF 的表不点名自己的版式（五份 ods 件全没有那一跳，
    # 只有 LibreOffice 自己那套 PageStyle_表名 的命名约定 —— 不是规格里的链接）；
    # .xls 每条 SETUP(0x00A1) 记录都在（每张表一条，长 34），可字段位没有一个读者能核对
    for name in ("hidden.ods", "book.ods", "hidden.xls", "book.xls"):
        missing = [one.get("name") for one in lbin("office-sheet", fixture(name)).get("sheets", [])
                   if one.get("print_setup") is not None]
        check("%s 这一族的打印设置没读：键整个不在" % name, missing, [])

    # ── 3a8) 表格里的图：三跳才到那个部件，两个生产者的引用写法与缓存值各交各的 ──
    print("=== 3a8) office-sheet 的图（openpyxl 与 LibreOffice 两份件） ===")
    for name in ("chart.xlsx", "chart-lo.xlsx"):
        want = files[name]["ooxml"]["charts"]
        got = lbin("office-sheet", fixture(name))
        mine = {
            (one.get("part") or "").rsplit("/", 1)[-1][: -len(".xml")]: one.get("chart_list")
            for one in got.get("sheets", [])
        }
        check("%s 每张表上的图整份账" % name, mine, want)
        check("%s 图的总账" % name, dig(got, "workbook.totals.charts"),
              sum(len(value) for value in want.values()))
    hand = lbin("office-sheet", fixture("chart.xlsx"))
    lo = lbin("office-sheet", fixture("chart-lo.xlsx"))
    SER0 = "sheets[0].chart_list[0].groups[0].series_list[0]"
    check(
        "两张图挂在同一张表上，另一张表 0 张",
        [one.get("charts") for one in hand.get("sheets", [])],
        [2, 0],
    )
    check(
        "图要跳三跳才到：表 → 自己的关系表 → 画法部件 → 它的关系表 → 图",
        [dig(hand, "sheets[0].chart_list[0].part"), dig(hand, "sheets[0].chart_list[1].part"),
         dig(lo, "sheets[0].chart_list[0].part")],
        ["xl/charts/chart1.xml", "xl/charts/chart2.xml", "xl/charts/chart1.xml"],
    )
    check(
        "图类型、柱形方向与系列数（柱形两条、折线一条）",
        [[dig(hand, "sheets[0].chart_list[0].groups[0].kind"),
          dig(hand, "sheets[0].chart_list[0].groups[0].written.barDir"),
          dig(hand, "sheets[0].chart_list[0].groups[0].written.grouping"),
          dig(hand, "sheets[0].chart_list[0].groups[0].series")],
         [dig(hand, "sheets[0].chart_list[1].groups[0].kind"),
          dig(hand, "sheets[0].chart_list[1].groups[0].written.barDir"),
          dig(hand, "sheets[0].chart_list[1].groups[0].written.grouping"),
          dig(hand, "sheets[0].chart_list[1].groups[0].series")]],
        [["barChart", "col", "clustered", 2], ["lineChart", None, "standard", 1]],
    )
    check(
        "同一段格子在两个生产者手里是两种写法，引用串照文件交",
        [dig(hand, SER0 + ".name.ref"), dig(lo, SER0 + ".name.ref"),
         dig(hand, SER0 + ".val.ref"), dig(lo, SER0 + ".val.ref")],
        ["'数据'!B1", "数据!$B$1", "'数据'!$B$2:$B$3", "数据!$B$2:$B$3"],
    )
    check(
        "缓存这份差一个数量级：openpyxl 一条 pt 不写，LibreOffice 把数都缓存了",
        [dig(hand, "sheets[0].chart_list[0].cached"), dig(lo, "sheets[0].chart_list[0].cached"),
         dig(hand, SER0 + ".val.cache.points"), dig(lo, SER0 + ".val.cache.points"),
         dig(hand, SER0 + ".val.cache.written"), dig(lo, SER0 + ".val.cache.written"),
         dig(lo, SER0 + ".val.cache.values")],
        [False, True, 0, 2, None, "2", [10.0, 25.0]],
    )
    check(
        "类目的走法两家不同（numRef 与 strRef），字还是同一批字",
        [dig(hand, SER0 + ".cat.via"), dig(lo, SER0 + ".cat.via"),
         dig(lo, SER0 + ".cat.cache.values"), dig(lo, SER0 + ".name.cache.values")],
        ["numRef", "strRef", ["一月", "二月"], ["收入"]],
    )
    check(
        "标题两家都写成字面量（c:rich），一个字母也不落在引用上",
        [dig(hand, "sheets[0].chart_list[0].title.via"),
         dig(hand, "sheets[0].chart_list[0].title.text"),
         dig(lo, "sheets[0].chart_list[0].title.text"),
         dig(lo, "sheets[0].chart_list[1].title.text"),
         dig(lo, "sheets[0].chart_list[0].title.ref")],
        ["text", "逐月收支", "逐月收支", "收入折线", None],
    )
    check(
        "轴 id 是生产者自己编的号：个数一致、值本来就不可比",
        [len(dig(hand, "sheets[0].chart_list[0].groups[0].axis_ids") or []),
         len(dig(lo, "sheets[0].chart_list[0].groups[0].axis_ids") or []),
         dig(hand, "sheets[0].chart_list[0].groups[0].axis_ids")
         == dig(lo, "sheets[0].chart_list[0].groups[0].axis_ids")],
        [2, 2, False],
    )
    check(
        "没挂图的表报 0（数过了没有）",
        [one.get("charts") for one in lbin("office-sheet", fixture("book.xlsx")).get("sheets", [])],
        [0, 0, 0],
    )
    # ODS 的图是嵌入对象（见 3a11），那两份没挂图的表报 0；.xls 走 BIFF 的对象链，
    # 本机没有一个读者量过 —— 那一族的键整个不在，不交空对象
    for name in ("book.ods", "hidden.ods"):
        check(
            "%s 没挂图的表报 0（数过了没有）" % name,
            [one.get("charts") for one in lbin("office-sheet", fixture(name)).get("sheets", [])],
            [0] * len(files[name]["ods"]["sheets"]),
        )
    check("hidden.xls 这一族的图没读：键整个不在",
          [one.get("name") for one in lbin("office-sheet", fixture("hidden.xls")).get("sheets", [])
           if one.get("charts") is not None], [])

    # ── 3a9) 规则那两份账：条件格式的样式在 dxf 那一跳上，数据验证的开关两种拼法 ──
    print("=== 3a9) office-sheet 的条件格式与数据验证 ===")
    for name in sorted(one.name for one in FIXTURES.glob("*.xlsx")):
        want = files[name]["ooxml"]
        got = lbin("office-sheet", fixture(name))
        mine = {
            (one.get("part") or "").rsplit("/", 1)[-1][: -len(".xml")]: one.get("rules")
            for one in got.get("sheets", [])
        }
        check("%s 每张表的规则整份账（条件格式 + 数据验证）" % name, mine, want["rules"])
        check("%s dxfs 那一跳的自报数与实际条数" % name, dig(got, "workbook.dxfs"), want["dxfs"])
    hand = lbin("office-sheet", fixture("rules.xlsx"))
    lo = lbin("office-sheet", fixture("rules-lo.xlsx"))
    FIRST = "sheets[0].rules.conditional[0].rule_list[0]"
    check(
        "一张表三块范围、四条件格式规则与三条验证",
        [len(dig(hand, "sheets[0].rules.conditional") or []),
         dig(hand, "workbook.totals.conditional_rules"),
         dig(hand, "workbook.totals.validations"),
         len(dig(hand, "sheets[1].rules.conditional") or [])],
        [3, 4, 3, 0],
    )
    check(
        "dxfId 那一跳：规则只写下标，字与色住在 styles.xml 的 dxfs 里",
        [dig(hand, FIRST + ".dxf.written"), dig(hand, FIRST + ".dxf.found"),
         dig(hand, FIRST + ".dxf.kinds"), dig(lo, FIRST + ".dxf.kinds")],
        ["0", True, ["font/b", "font/color"],
         ["font/name", "font/family", "font/b", "font/color", "font/sz"]],
    )
    check(
        "同一条 sqref 里可以塞两段区间，照文件交",
        [dig(hand, "sheets[0].rules.conditional[0].sqref"),
         dig(hand, "sheets[0].rules.conditional[1].sqref"),
         dig(hand, "sheets[0].rules.conditional[2].sqref")],
        ["B2:B6", "A2:A6 B2:B4", "A2:A6"],
    )
    check(
        "色阶的三个 cfvo 与三个颜色：alpha 那两位是两个生产者的差",
        [dig(hand, "sheets[0].rules.conditional[1].rule_list[0].scale.colors"),
         dig(lo, "sheets[0].rules.conditional[1].rule_list[0].scale.colors"),
         len(dig(hand, "sheets[0].rules.conditional[1].rule_list[0].scale.cfvo") or [])],
        [["00FFFFFF", "00FFEB84", "00F8696B"],
         ["FFFFFFFF", "FFFFEB84", "FFF8696B"], 3],
    )
    check(
        "图标集把形状写在子元素里（iconSet 与每条 cfvo 的 type/val）",
        [dig(hand, "sheets[0].rules.conditional[0].rule_list[1].type"),
         dig(hand, "sheets[0].rules.conditional[0].rule_list[1].scale.written.iconSet"),
         dig(hand, "sheets[0].rules.conditional[0].rule_list[1].scale.cfvo[2]")],
        ["iconSet", "3Arrows", {"type": "percent", "val": "67"}],
    )
    check(
        "公式规则里那个 > 是实体，解掉才是文件写的那条式子",
        [dig(hand, "sheets[0].rules.conditional[2].rule_list[0].formulas"),
         dig(lo, "sheets[0].rules.conditional[2].rule_list[0].formulas")],
        [["$B2>200"], ["$B2>200"]],
    )
    check(
        "priority 是生产者自己排的号：同一批规则两家排得不一样",
        [[dig(hand, "sheets[0].rules.conditional[%d].rule_list[0].priority" % i) for i in (0, 1, 2)],
         [dig(lo, "sheets[0].rules.conditional[%d].rule_list[0].priority" % i) for i in (0, 1, 2)]],
        [["1", "2", "4"], ["2", "4", "5"]],
    )
    check(
        "数据验证：容器自报 count，属性一个也不替它补",
        [dig(hand, "sheets[0].rules.validations.written"),
         dig(hand, "sheets[0].rules.validations.found"),
         dig(hand, "sheets[0].rules.validations.whole"),
         dig(hand, "sheets[0].rules.validations.list[0].type"),
         dig(hand, "sheets[0].rules.validations.list[0].operator"),
         dig(hand, "sheets[0].rules.validations.list[0].formulas")],
        ["3", 3, True, "list", None, ['"红,黄,绿"']],
    )
    check(
        "同一条开关的两种拼法（1/0 与 true/false）与 LibreOffice 补的那三样",
        [dig(hand, "sheets[0].rules.validations.list[0].written.allowBlank"),
         dig(lo, "sheets[0].rules.validations.list[0].written.allowBlank"),
         dig(lo, "sheets[0].rules.validations.list[0].operator"),
         dig(lo, "sheets[0].rules.validations.list[0].written.errorStyle"),
         dig(lo, "sheets[0].rules.validations.list[0].formulas"),
         dig(lo, "sheets[0].rules.validations.list[2].formulas")],
        ["1", "true", "equal", "stop", ['"红,黄,绿"', "0"], ["ISNUMBER(B2)", "0"]],
    )
    check(
        "没挂规则的表：块数 0、验证是「没写」而不是空表",
        [len(dig(lo, "sheets[1].rules.conditional") or []),
         dig(lo, "sheets[1].rules.validations.written"),
         dig(lo, "sheets[1].rules.validations.found")],
        [0, None, 0],
    )
    # 另两家：ODS 的条件格式在 number:* 样式与 table:style 那一套上，.xls 是 BIFF 的
    # CONDFMT/DCON 记录 —— 都没有量过的第二个读者，所以 rules 这个键整个不在
    for name in ("book.ods", "hidden.ods", "book.xls", "hidden.xls"):
        check("%s 这一族的规则没读：键整个不在" % name,
              [one.get("name") for one in lbin("office-sheet", fixture(name)).get("sheets", [])
               if one.get("rules") is not None], [])


    # ── 3a11) ODF 这一族的图：draw:object 指到 Object N/，地址是第三种写法 ──
    print("=== 3a11) ODS 与 ODP 的图（嵌入对象） ===")
    for name, key, cmd, holder in (("chart.ods", "ods", "office-sheet", "sheets"),
                                   ("deck-chart.odp", "odp", "office-slide", "slides")):
        want = files[name][key][holder]
        got = lbin(cmd, fixture(name))
        mine = [one.get("chart_list") for one in got.get(holder, [])]
        check("%s 每一张表/每一页的图整份账" % name, mine, [one.get("charts") for one in want])
        check(
            "%s 图的条数按表/页归位" % name,
            [one.get("charts") for one in got.get(holder, [])],
            [len(one.get("charts")) for one in want],
        )
    ods = lbin("office-sheet", fixture("chart.ods"))
    xw = lbin("office-sheet", fixture("chart-lo.xlsx"))
    op = lbin("office-sheet", fixture("chart.xlsx"))
    check(
        "同一段格子在三种存法里是三种写法（都不归一化）",
        [dig(ods, "sheets[0].chart_list[0].series_list[0].values"),
         dig(xw, "sheets[0].chart_list[0].groups[0].series_list[0].val.ref"),
         dig(op, "sheets[0].chart_list[0].groups[0].series_list[0].val.ref")],
        ["数据.B2:数据.B3", "数据!$B$2:$B$3", "'数据'!$B$2:$B$3"],
    )
    check(
        "图的类型：ODF 写在每条系列上，OOXML 写在外层图组上",
        [dig(ods, "sheets[0].chart_list[0].class"),
         dig(ods, "sheets[0].chart_list[0].series_list[0].class"),
         dig(ods, "sheets[0].chart_list[1].series_list[0].class"),
         dig(xw, "sheets[0].chart_list[0].groups[0].kind")],
        [None, "chart:bar", "chart:line", "barChart"],
    )
    check(
        "跨存法同一份账：图的条数、系列数与标题三处一致",
        [[len([1 for _ in (one.get("chart_list") or [])]) for one in ods.get("sheets", [])],
         [dig(ods, "sheets[0].chart_list[0].series"), dig(ods, "sheets[0].chart_list[1].series")],
         [dig(ods, "sheets[0].chart_list[0].title"), dig(ods, "sheets[0].chart_list[1].title")],
         [dig(xw, "sheets[0].chart_list[0].groups[0].series"),
          dig(xw, "sheets[0].chart_list[1].groups[0].series")],
         [dig(xw, "sheets[0].chart_list[0].title.text"), dig(xw, "sheets[0].chart_list[1].title.text")]],
        [[2, 0], [2, 1], ["逐月收支", "收入折线"], [2, 1], ["逐月收支", "收入折线"]],
    )
    check(
        "画的数也一致：ODF 的 local-table 与 OOXML 的 numCache 是同一批格子",
        [dig(ods, "sheets[0].chart_list[0].local_table[1].cells[1].value"),
         dig(ods, "sheets[0].chart_list[0].local_table[2].cells[1].value"),
         dig(xw, "sheets[0].chart_list[0].groups[0].series_list[0].val.cache.values"),
         [dig(ods, "sheets[0].chart_list[0].series_list[0].point_elements"),
          dig(ods, "sheets[0].chart_list[0].series_list[0].points_written")]],
        ["10", "25", [10.0, 25.0], [1, 2]],
    )
    check(
        "那一族没挂图的件报 0：book.ods 三张表、deck.odp 两页都是空表",
        [len(one.get("chart_list") or []) for one in lbin("office-sheet", fixture("book.ods")).get("sheets", [])]
        + [len(one.get("chart_list") or []) for one in lbin("office-slide", fixture("deck.odp")).get("slides", [])],
        [0, 0, 0, 0, 0],
    )
    # .xls 的图走 BIFF 的 OBJ/CLID 那条链，本机没有一个读者量过 —— 键整个不在
    check(
        "book.xls 这一族的图没读：每页不带 charts 那份账",
        len([one for one in lbin("office-sheet", fixture("book.xls")).get("sheets", [])
             if one.get("charts") is not None]),
        0,
    )

    # ── 3a12) 每张表的窗口状态与页眉页脚：开关照文件交，「没写」与「写了空的」分开 ──
    print("=== 3a12) office-sheet 的 sheetView 与 headerFooter（xlsx 的两个生产者） ===")
    for name in sorted(one.name for one in FIXTURES.glob("*.xlsx")):
        got = lbin("office-sheet", fixture(name))
        mine = {
            (one.get("part") or "").rsplit("/", 1)[-1][: -len(".xml")]: one.get("view")
            for one in got.get("sheets", [])
        }
        check("%s 每张表的窗口状态与读者一致" % name, mine, files[name]["ooxml"]["views"])
        mine = {
            (one.get("part") or "").rsplit("/", 1)[-1][: -len(".xml")]: one.get("header_footer")
            for one in got.get("sheets", [])
        }
        check("%s 每张表的页眉页脚与读者一致" % name, mine, files[name]["ooxml"]["headers"])
    hand = lbin("office-sheet", fixture("view.xlsx"))
    lo = lbin("office-sheet", fixture("view-lo.xlsx"))
    check(
        "同一条开关两种拼法：openpyxl 的 0 与 LibreOffice 的 false",
        [dig(hand, "sheets[0].view.written.showGridLines"),
         dig(lo, "sheets[0].view.written.showGridLines"),
         dig(hand, "sheets[0].view.written.tabSelected"),
         dig(lo, "sheets[0].view.written.tabSelected")],
        ["0", "false", "1", "true"],
    )
    check(
        "只交文件写了的：一家四个属性，另一家把没写的也全补出来",
        [len(dig(hand, "sheets[0].view.written") or {}),
         len(dig(lo, "sheets[0].view.written") or {}),
         dig(hand, "sheets[0].view.written.zoomScaleNormal")],
        [4, 15, None],
    )
    check(
        "冻结与拆分是同一个 state 上的两种值，两份件都按文件写",
        [dig(hand, "sheets[0].view.pane.state"), dig(hand, "sheets[1].view.pane.state"),
         dig(hand, "sheets[2].view.pane"), dig(lo, "sheets[0].view.pane.state")],
        ["frozen", "split", None, "frozen"],
    )
    # LibreOffice 的 xlsx 导出把「拆分」那个 pane 整个丢了（同一份件冻住的那张留着）：
    # 这是量出来的重写损失，不是这一族本来就没有
    check(
        "重写会掉东西：LO 那份里 split 的 pane 不在了",
        [dig(lo, "sheets[1].view.pane"), dig(lo, "sheets[0].view.pane.xSplit"),
         dig(lo, "sheets[0].view.pane.topLeftCell")],
        [None, "1", "B3"],
    )
    check(
        "selection 条数与点名各交各的：一家三条不点 topLeft，另一家四条还带 activeCellId",
        [[len(one.get("selections") or []) for one in
          [dig(hand, "sheets[0].view") or {}, dig(lo, "sheets[0].view") or {}]],
         [one.get("pane") for one in (dig(hand, "sheets[0].view.selections") or [])],
         [one.get("pane") for one in (dig(lo, "sheets[0].view.selections") or [])],
         dig(lo, "sheets[0].view.selections[0].activeCellId")],
        [[3, 4], ["topRight", "bottomLeft", "bottomRight"],
         ["topLeft", "topRight", "bottomLeft", "bottomRight"], "0"],
    )
    check(
        "页眉写了字的段数两份一致，尽管一家补了字体码另一家没有",
        [dig(hand, "sheets[0].header_footer.written_slots"),
         dig(lo, "sheets[0].header_footer.written_slots")],
        [3, 3],
    )
    check(
        "同一件事的两种字面：&L 与 &C 是分段标记，&& 是一个真的 &",
        [dig(hand, "sheets[0].header_footer.slots[0].text"),
         [one.get("at") for one in (dig(hand, "sheets[0].header_footer.slots[0].segments") or [])],
         dig(hand, "sheets[0].header_footer.slots[1].fields"),
         [one.get("text") for one in (dig(hand, "sheets[0].header_footer.slots[1].segments") or [])]],
        ["&L第 &A 页&C冻结那张", ["left", "center"], ["&&", "&P", "&N"],
         ["打开 && 关闭", "第 &P 页，共 &N 页"]],
    )
    check(
        "LO 每段前面补一个字体码，分段照它的写法交",
        [dig(lo, "sheets[0].header_footer.slots[0].fields"),
         dig(lo, "sheets[0].header_footer.slots[0].segments[0].text"),
         dig(lo, "sheets[0].header_footer.slots[1].fields")],
        [['&"Calibri"', "&A", '&"Calibri"'], '&"Calibri"第 &A 页',
         ['&"Calibri"', "&&", '&"Calibri"', "&P", "&N"]],
    )
    check(
        "「没写这个元素」与「写了但是空的」是两件事",
        [dig(hand, "sheets[2].header_footer.present"),
         dig(hand, "sheets[2].header_footer.slots[0].text"),
         dig(lo, "sheets[2].header_footer.present"),
         dig(lo, "sheets[2].header_footer.slots[0].text"),
         dig(lo, "sheets[2].header_footer.slots[0].present"),
         dig(lo, "sheets[2].header_footer.slots[2].present")],
        [False, None, True, "", True, False],
    )
    check(
        "开关属性也各自交：一家不写 differentFirst，另一家写 false",
        [dig(hand, "sheets[0].header_footer.written"),
         dig(lo, "sheets[0].header_footer.written")],
        [{"differentOddEven": "1"}, {"differentFirst": "false", "differentOddEven": "true"}],
    )
    # ODF 的窗口状态在样式那一套里、.xls 在 BIFF 的 WINDOW1/WINDOW2 与 HEADER/FOOTER 记录里，
    # 两族都没读：键整个不在（与 print_setup 同一口径）
    for name in ("book.ods", "hidden.ods", "chart.ods", "book.xls", "hidden.xls"):
        missed = [one.get("name") for one in lbin("office-sheet", fixture(name)).get("sheets", [])
                  if one.get("view") is not None or one.get("header_footer") is not None]
        check("%s 这一族的窗口与页眉页脚没读：键整个不在" % name, missed, [])

    # ── 3a13) 列宽行高、筛选与表对象：两家的换算互不相等，重写一次就换一套数 ──
    print("=== 3a13) office-sheet 的尺寸、筛选与表对象 ===")
    for name in sorted(one.name for one in FIXTURES.glob("*.xlsx")):
        got = lbin("office-sheet", fixture(name))
        want = files[name]["ooxml"]
        by_part = {(one.get("part") or "").rsplit("/", 1)[-1][: -len(".xml")]: one
                   for one in got.get("sheets", [])}
        for key in ("layout", "filter"):
            check(
                "%s 每张表的 %s 与读者一致" % (name, key),
                {one: value.get(key) for one, value in by_part.items()},
                want[key + "s"],
            )
        check(
            "%s 每张表的表对象与读者一致" % name,
            {
                one: {
                    "tables": value.get("tables"),
                    "table_list": value.get("table_list"),
                    "parts": value.get("table_parts"),
                }
                for one, value in by_part.items()
            },
            want["sheet_tables"],
        )
    hand = lbin("office-sheet", fixture("size.xlsx"))
    lo = lbin("office-sheet", fixture("size-lo.xlsx"))
    check(
        "「默认列宽」两家写的不是同一个属性：baseColWidth 与 defaultColWidth",
        [dig(hand, "sheets[0].layout.format.baseColWidth"),
         dig(hand, "sheets[0].layout.format.defaultColWidth"),
         dig(lo, "sheets[0].layout.format.baseColWidth"),
         dig(lo, "sheets[0].layout.format.defaultColWidth")],
        ["8", None, None, "7.7734375"],
    )
    check(
        "同一列换一家就换一套数：22.5 与 20.47、4 与 3.64（照文件交，不折算）",
        [dig(hand, "sheets[0].layout.columns.list[0].width"),
         dig(lo, "sheets[0].layout.columns.list[0].width"),
         dig(hand, "sheets[0].layout.columns.list[1].width"),
         dig(lo, "sheets[0].layout.columns.list[1].width")],
        ["22.5", "20.47", "4", "3.64"],
    )
    check(
        "一条 col 顶几列按 min/max 加出来，几条与盖住几列分开交",
        [dig(hand, "sheets[0].layout.columns.written"),
         dig(hand, "sheets[0].layout.columns.covered"),
         dig(lo, "sheets[0].layout.columns.written"),
         dig(lo, "sheets[0].layout.columns.covered")],
        [2, 2, 2, 2],
    )
    check(
        "藏起来的那一列：同一条开关的两种拼法",
        [dig(hand, "sheets[0].layout.columns.list[1].hidden"),
         dig(lo, "sheets[0].layout.columns.list[1].hidden")],
        ["1", "true"],
    )
    check(
        "行高度：一家只给说过话的两行写，另一家三行全写（40 换成 39.75）",
        [dig(hand, "sheets[0].layout.rows.elements"), dig(hand, "sheets[0].layout.rows.with_height"),
         dig(hand, "sheets[0].layout.rows.spoken"), dig(hand, "sheets[0].layout.rows.list[0].ht"),
         dig(lo, "sheets[0].layout.rows.elements"), dig(lo, "sheets[0].layout.rows.with_height"),
         dig(lo, "sheets[0].layout.rows.list[0].ht"), dig(lo, "sheets[0].layout.rows.list[1].ht")],
        [3, 2, 2, "40", 3, 3, "18", "39.75"],
    )
    check(
        "第二张表什么都没收：几条与盖住几列分开（没有 col 时判不住）",
        [dig(hand, "sheets[1].layout.columns.written"),
         dig(hand, "sheets[1].layout.columns.covered"),
         dig(hand, "sheets[1].layout.rows.elements"),
         dig(lo, "sheets[1].layout.columns.covered")],
        [0, None, 0, None],
    )
    check(
        "筛选范围与表对象的范围是两件事，两个都交",
        [dig(hand, "sheets[0].filter.written.ref"),
         dig(hand, "sheets[0].tables"), dig(hand, "sheets[0].table_list[0].written.ref"),
         dig(lo, "sheets[0].filter.written.ref"), dig(lo, "sheets[0].table_list[0].written.ref")],
        ["A1:C3", 1, "A1:B3", "A1:C3", "A1:B3"],
    )
    check(
        "filterMode 只有一家写：筛着的那张 true、没筛的那张 false，另一家两样都没有",
        [dig(hand, "sheets[0].filter.mode"), dig(hand, "sheets[1].filter.mode"),
         dig(lo, "sheets[0].filter.mode"), dig(lo, "sheets[1].filter.mode")],
        [None, None, "true", "false"],
    )
    check(
        "筛选列上的开关也只有一家写；被筛掉的值两家一字不差",
        [dig(hand, "sheets[0].filter.columns[0].written"),
         dig(lo, "sheets[0].filter.columns[0].written"),
         dig(hand, "sheets[0].filter.columns[0].vals"),
         dig(lo, "sheets[0].filter.columns[0].vals"),
         dig(hand, "sheets[1].filter.columns")],
        [{"colId": "0", "hiddenButton": "0", "showButton": "1"}, {"colId": "0"},
         ["甲"], ["甲"], []],
    )
    check(
        "表对象的列名是文件自己写的（一家拿范围第一行的字当列名，第二列就叫 10）",
        [dig(hand, "sheets[0].table_list[0].columns.names"),
         dig(lo, "sheets[0].table_list[0].columns.names"),
         dig(hand, "sheets[0].table_list[0].written.name"),
         dig(hand, "sheets[0].table_list[0].written.displayName")],
        [["一月", "10"], ["一月", "10"], "台账", "台账"],
    )
    check(
        "自报的条数与实际条数一起交：tableColumns 两家都写，tableParts 的 count 只有一家写",
        [dig(hand, "sheets[0].table_list[0].columns.written"),
         dig(hand, "sheets[0].table_list[0].columns.found"),
         dig(hand, "sheets[0].table_list[0].columns.whole"),
         dig(hand, "sheets[0].table_parts.written"), dig(hand, "sheets[0].table_parts.found"),
         dig(lo, "sheets[0].table_parts.written"), dig(lo, "sheets[0].table_parts.found"),
         dig(lo, "sheets[0].table_parts.whole")],
        ["2", 2, True, "1", 1, None, 1, True],
    )
    check(
        "表样式那几个开关：一家写两个，另一家五个全写（等于默认的也不省）",
        [len(dig(hand, "sheets[0].table_list[0].style") or {}),
         len(dig(lo, "sheets[0].table_list[0].style") or {}),
         dig(hand, "sheets[0].table_list[0].style.showRowStripes"),
         dig(lo, "sheets[0].table_list[0].style.showColumnStripes")],
        [2, 5, "1", "0"],
    )
    for name in ("book.ods", "hidden.ods", "chart.ods", "book.xls", "hidden.xls"):
        missed = [one.get("name") for one in lbin("office-sheet", fixture(name)).get("sheets", [])
                  if one.get("layout") is not None or one.get("filter") is not None
                  or one.get("tables") is not None]
        check("%s 这一族的尺寸、筛选与表对象没读：键整个不在" % name, missed, [])

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
        "covered_cells", "sections", "breaks", "page_breaks", "soft_page_breaks",
        "drawings",
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
