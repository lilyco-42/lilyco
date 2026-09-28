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
from collections import Counter
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


def flat(val):
    """失败详情里不许因为「键是元组」就把整趟对账打断（json.dumps 对元组键直接抛）"""
    if isinstance(val, dict):
        return {one if isinstance(one, (str, int, float, bool)) or one is None else str(one):
                flat(had) for one, had in val.items()}
    if isinstance(val, (list, tuple)):
        return [flat(one) for one in val]
    return val


def diff_paths(got, want, at: str = "") -> str:
    """两边第一个不一样的**路径**与各自的值 —— 整本比对失败时唯一省时间的东西

    两边都是 dict / list 就一路往下走（dict 的键按名排序，这样两边的报告在同一处停住），
    长度不同先说长度，叶子不同就说那一条路径。找到第一处就返回，不堆全表（全表太长）。
    """
    miss = "〈缺键〉"
    if isinstance(got, dict) and isinstance(want, dict):
        for key in sorted(set(got) | set(want), key=str):
            hit = diff_paths(got[key] if key in got else miss,
                             want[key] if key in want else miss,
                             "%s.%s" % (at, key))
            if hit:
                return hit
        return ""
    if isinstance(got, list) and isinstance(want, list):
        if len(got) != len(want):
            return "%s 条数 lbin=%d 读者=%d" % (at, len(got), len(want))
        for index, (one, two) in enumerate(zip(got, want)):
            hit = diff_paths(one, two, "%s[%d]" % (at, index))
            if hit:
                return hit
        return ""
    if got == want:
        return ""
    return "%s lbin=%s 读者=%s" % (
        at, json.dumps(flat(got), ensure_ascii=False, default=str)[:120],
        json.dumps(flat(want), ensure_ascii=False, default=str)[:120])


def check(name: str, got, want, hint: str = "") -> None:
    ok = got == want
    # 两边各留 4000 字：留少了就只看得到共同的前缀，差的偏偏在后头（修订那一条就这样）。
    # 上一档是 400：语料级聚合（主题那三本 @name、手指按族拆开的合计）差的常在后半截，
    # 日志里抄不回完整新值，就得再跑一条 68 分钟的闸门去要一个数。
    # 整本比对失败时再补一句「第一个不一样的路径」（diff_paths）—— 两边的共同前缀
    # 一直相同，截断里永远看不见差在哪一格；这一句只对 dict / list 算，标量不必。
    if not ok and not hint and isinstance(got, (dict, list)) and isinstance(want, (dict, list)):
        hint = diff_paths(got, want)
    record(name, ok, "" if ok else f"lbin={json.dumps(flat(got), ensure_ascii=False, default=str)[:4000]} "
                                   f"读者={json.dumps(flat(want), ensure_ascii=False, default=str)[:4000]}"
                                   + (f" ▸ {hint}" if hint else ""))


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
    # 语料指纹先打一行：语料级的聚合钉值一失败，第一眼就要能判断是「件数变了」还是「读者变了」。
    # 上一轮 13 条失败全是前者（word 88→92、odt 47→49），而日志里没有这份指纹，只能回头数件。
    by_ext = {}
    for one in sorted(files):
        tail = one.rsplit(".", 1)[-1] if "." in one else "(无后缀)"
        by_ext[tail] = by_ext.get(tail, 0) + 1
    print("=== 语料指纹：" + " ".join(
        "%s=%d" % pair for pair in sorted(by_ext.items(), key=lambda x: (-x[1], x[0]))) + " ===")

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
        "deck-links.pptx": ("ooxml", "powerpoint", "pptx"),
        "deck-links-lo.pptx": ("ooxml", "powerpoint", "pptx"),
        "deck-hidden.pptx": ("ooxml", "powerpoint", "pptx"),
        "deck-hidden-lo.pptx": ("ooxml", "powerpoint", "pptx"),
        "deck-hidden.odp": ("opendocument", "powerpoint", "odp"),
        "deck-pictures.pptx": ("ooxml", "powerpoint", "pptx"),
        "deck-pictures-lo.pptx": ("ooxml", "powerpoint", "pptx"),
        "deck-pictures.odp": ("opendocument", "powerpoint", "odp"),
        "deck-lo.pptx": ("ooxml", "powerpoint", "pptx"),
        "deck-chart.pptx": ("ooxml", "powerpoint", "pptx"),
        "deck-chart-lo.pptx": ("ooxml", "powerpoint", "pptx"),
        "errors.xlsx": ("ooxml", "excel", "xlsx"),
        "errors-lo.xlsx": ("ooxml", "excel", "xlsx"),
        "errors.ods": ("opendocument", "excel", "ods"),
        "epoch.xlsx": ("ooxml", "excel", "xlsx"),
        "epoch-lo.xlsx": ("ooxml", "excel", "xlsx"),
        "rich.xlsx": ("ooxml", "excel", "xlsx"),
        "rich-lo.xlsx": ("ooxml", "excel", "xlsx"),
        "rich.ods": ("opendocument", "excel", "ods"),
        "styled.xlsx": ("ooxml", "excel", "xlsx"),
        "styled-lo.xlsx": ("ooxml", "excel", "xlsx"),
        "chart.xlsx": ("ooxml", "excel", "xlsx"),
        "chart-lo.xlsx": ("ooxml", "excel", "xlsx"),
        "rules.xlsx": ("ooxml", "excel", "xlsx"),
        "rules-lo.xlsx": ("ooxml", "excel", "xlsx"),
        "view.xlsx": ("ooxml", "excel", "xlsx"),
        "view-lo.xlsx": ("ooxml", "excel", "xlsx"),
        "size.xlsx": ("ooxml", "excel", "xlsx"),
        "size-lo.xlsx": ("ooxml", "excel", "xlsx"),
        "para.docx": ("ooxml", "word", "docx"),
        "images.docx": ("ooxml", "word", "docx"),
        "images-lo.docx": ("ooxml", "word", "docx"),
        "images-float.docx": ("ooxml", "word", "docx"),
        "images.odt": ("opendocument", "word", "odt"),
        "images-float.odt": ("opendocument", "word", "odt"),
        "images.rtf": ("rtf", "word", "rtf"),
        # 字符格式那四份：一段只点一个开关，另有点「明确不粗」的、一串字里两个孩子的、
        # 以及一段里三种字各一串的（见 office_fixtures.write_runs_docx）
        "styled-text.docx": ("ooxml", "word", "docx"),
        "styled-text-lo.docx": ("ooxml", "word", "docx"),
        "styled-text.odt": ("opendocument", "word", "odt"),
        "styled-text.rtf": ("rtf", "word", "rtf"),
        # 字符样式那四份：段上只写一个样式号，那句话住在另一份部件里（一处一半）
        "charstyles.docx": ("ooxml", "word", "docx"),
        "charstyles-lo.docx": ("ooxml", "word", "docx"),
        "charstyles.odt": ("opendocument", "word", "odt"),
        "charstyles.rtf": ("rtf", "word", "rtf"),
        "fields.docx": ("ooxml", "word", "docx"),
        "fields-lo.docx": ("ooxml", "word", "docx"),
        "fields.odt": ("opendocument", "word", "odt"),
        "fields.rtf": ("rtf", "word", "rtf"),
        "sections.docx": ("ooxml", "word", "docx"),
        "para.odt": ("opendocument", "word", "odt"),
        "para.rtf": ("rtf", "word", "rtf"),
        "tables-lo.docx": ("ooxml", "word", "docx"),
        "shaded.docx": ("ooxml", "word", "docx"),
        "shaded-lo.docx": ("ooxml", "word", "docx"),
        "shaded.odt": ("opendocument", "word", "odt"),
        "lists.docx": ("ooxml", "word", "docx"),
        "lists-lo.docx": ("ooxml", "word", "docx"),
        "lists.odt": ("opendocument", "word", "odt"),
        "lists.rtf": ("rtf", "word", "rtf"),
        "notes.odt": ("opendocument", "word", "odt"),
        "book.ods": ("opendocument", "excel", "ods"),
        "deck.odp": ("opendocument", "powerpoint", "odp"),
        "notes.doc": ("compound", "word", "doc"),
        "eq.doc": ("compound", "word", "doc"),
        "notes-en.doc": ("compound", "word", "doc"),
        "formats.xlsx": ("ooxml", "excel", "xlsx"),
        "formats.ods": ("opendocument", "excel", "ods"),
        "book.xls": ("compound", "excel", "xls"),
        "hidden.xls": ("compound", "excel", "xls"),
        "deck.ppt": ("compound", "powerpoint", "ppt"),
        "deck-ph-lo.ppt": ("compound", "powerpoint", "ppt"),
        "deck-tables-lo.ppt": ("compound", "powerpoint", "ppt"),
        "notes.rtf": ("rtf", "word", "rtf"),
        "hidden.xlsx": ("ooxml", "excel", "xlsx"),
        "hidden-lo.xlsx": ("ooxml", "excel", "xlsx"),
        "hidden.ods": ("opendocument", "excel", "ods"),
        "cell-notes.xlsx": ("ooxml", "excel", "xlsx"),
        "cell-notes-lo.xlsx": ("ooxml", "excel", "xlsx"),
        "cell-notes.ods": ("opendocument", "excel", "ods"),
        "cell-notes-many.xlsx": ("ooxml", "excel", "xlsx"),
        "cell-notes-many.xls": ("compound", "excel", "xls"),
        # 页上那张表那三件：一张表的三种写法（python-pptx、LibreOffice 的 pptx、LibreOffice 的 odp）
        "deck-tables.pptx": ("ooxml", "powerpoint", "pptx"),
        "deck-tables-lo.pptx": ("ooxml", "powerpoint", "pptx"),
        "deck-tables.odp": ("opendocument", "powerpoint", "odp"),
        # 框与字那两件：一个框里的字装不下怎么办（python-pptx 的 pptx 与 LibreOffice 的 odp）
        # 字体那三份：一次只改一个变量的五种点法（表里没有的名、主题那一路、只点东亚）
        "fonts.docx": ("ooxml", "word", "docx"),
        "fonts-lo.docx": ("ooxml", "word", "docx"),
        "fonts.odt": ("opendocument", "word", "odt"),
        "deck-autofit.pptx": ("ooxml", "powerpoint", "pptx"),
        "deck-autofit.odp": ("opendocument", "powerpoint", "odp"),
        # 打印范围那三份：同一个选择在两家是两处地方
        "print-area.xlsx": ("ooxml", "excel", "xlsx"),
        "print-area-lo.xlsx": ("ooxml", "excel", "xlsx"),
        "print-area.ods": ("opendocument", "excel", "ods"),
        "deck-ph.pptx": ("ooxml", "powerpoint", "pptx"),
        "deck-ph-lo.pptx": ("ooxml", "powerpoint", "pptx"),
        "deck-ph.odp": ("opendocument", "powerpoint", "odp"),
        # 表头重复那三份：同一个要求，一家写在行上（一个元素），一家写在表身上（两个数）
        "table-header.docx": ("ooxml", "word", "docx"),
        "table-header-lo.docx": ("ooxml", "word", "docx"),
        "table-header.odt": ("opendocument", "word", "odt"),
        # 制表位那三份里的两份：同一个定义在两家手里是两个串（twip / cm）—— `.rtf` 那一族
        # 这一支还没读（它写 `\tx`，见 3aa），所以不进这一张识别表
        "tabs.docx": ("ooxml", "word", "docx"),
        "tabs.odt": ("opendocument", "word", "odt"),
        # 批注那三份：内容在部件、锚点在正文，两家把「第几条」排成两种先后
        "doc-comments.docx": ("ooxml", "word", "docx"),
        "doc-comments-lo.docx": ("ooxml", "word", "docx"),
        "doc-comments.odt": ("opendocument", "word", "odt"),
        # 分页开关那三份：在场不等于开着，一跳不等于没有
        "keep.docx": ("ooxml", "word", "docx"),
        "keep-lo.docx": ("ooxml", "word", "docx"),
        "keep.odt": ("opendocument", "word", "odt"),
        # 表样式那三份：一家的样式 id 与那枚缓存值是两本账，可以互相不一致
        "table-style.docx": ("ooxml", "word", "docx"),
        "table-style-lo.docx": ("ooxml", "word", "docx"),
        "table-style.odt": ("opendocument", "word", "odt"),
        # 行距那三份：同一个数在两种单位下长得一模一样，只有紧跟的那枚属性说得清
        "line.docx": ("ooxml", "word", "docx"),
        "line-lo.docx": ("ooxml", "word", "docx"),
        "line.odt": ("opendocument", "word", "odt"),
        # 段边框与底纹那三份：壳在不在、里面写了几条边，两件事
        "pborder.docx": ("ooxml", "word", "docx"),
        "pborder-lo.docx": ("ooxml", "word", "docx"),
        "pborder.odt": ("opendocument", "word", "odt"),
        # 文本框那三份：一个框可以写两份，框里的段不是正文的段
        "tbox.odt": ("opendocument", "word", "odt"),
        "tbox.docx": ("ooxml", "word", "docx"),
        "tbox-lo.odt": ("opendocument", "word", "odt"),
        # 书签配对那三份：止只写号，断的两个方向都造一条
        "bkmks.docx": ("ooxml", "word", "docx"),
        "bkmks-lo.docx": ("ooxml", "word", "docx"),
        "bkmks.odt": ("opendocument", "word", "odt"),
        # 页码起始那五份：三个属性三种待遇，跨族走一趟两头各丢一次
        "pnum.odt": ("opendocument", "word", "odt"),
        "pnum.docx": ("ooxml", "word", "docx"),
        "restart.docx": ("ooxml", "word", "docx"),
        "restart-lo.docx": ("ooxml", "word", "docx"),
        "restart.odt": ("opendocument", "word", "odt"),
        # 语言那三份：一路、另一路、三路全说，跨族之后只剩一格
        "lang.docx": ("ooxml", "word", "docx"),
        "lang-lo.docx": ("ooxml", "word", "docx"),
        "lang.odt": ("opendocument", "word", "odt"),
        # 形状清单那三份：一页一个散框加一个三件的组合，另一页什么都没有
        "deck-gr.pptx": ("ooxml", "powerpoint", "pptx"),
        "deck-gr-lo.pptx": ("ooxml", "powerpoint", "pptx"),
        "deck-gr.odp": ("opendocument", "powerpoint", "odp"),
        # 页底那三份：实色 / 显式 noFill / 渐变 / 什么都不写，四页三种存法
        "deck-bg.pptx": ("ooxml", "powerpoint", "pptx"),
        "deck-bg-lo.pptx": ("ooxml", "powerpoint", "pptx"),
        "deck-bg.odp": ("opendocument", "powerpoint", "odp"),
        "crep.docx": ("ooxml", "word", "docx"),
        "crep-lo.docx": ("ooxml", "word", "docx"),
        "crep-r.docx": ("ooxml", "word", "docx"),
        "crep.odt": ("opendocument", "word", "odt"),
        "crep-r.odt": ("opendocument", "word", "odt"),
        "shared.xlsx": ("ooxml", "excel", "xlsx"),
        "shared-lo.xlsx": ("ooxml", "excel", "xlsx"),
        "shared.ods": ("opendocument", "excel", "ods"),
        "sstart.docx": ("ooxml", "word", "docx"),
        "sstart-lo.docx": ("ooxml", "word", "docx"),
        # 结构搬进 markdown 那两份：一家把列表号写在样式上、另一家写在段上
        "md.docx": ("ooxml", "word", "docx"),
        "md-lo.docx": ("ooxml", "word", "docx"),
        "md.odt": ("opendocument", "word", "odt"),
        # 公式那四份：OMML 原文、LibreOffice 重写、转成 odt（一条式子一个部件）、再回转
        "eq.docx": ("ooxml", "word", "docx"),
        "eq-lo.docx": ("ooxml", "word", "docx"),
        "eq-od.docx": ("ooxml", "word", "docx"),
        "eq.odt": ("opendocument", "word", "odt"),
        # 放映里的公式：手写外壳 + LibreOffice 的同格式重写（pptx 那一转也留了件，但这一本不读它）
        "eqs.odp": ("opendocument", "powerpoint", "odp"),
        "eqs-lo.odp": ("opendocument", "powerpoint", "odp"),
        "eqs.pptx": ("ooxml", "powerpoint", "pptx"),
        "eqs-pp.pptx": ("ooxml", "powerpoint", "pptx"),
        # 注的编号那三份：同一句话在两处说，两处说的不一样
        "nset.docx": ("ooxml", "word", "docx"),
        "nset-lo.docx": ("ooxml", "word", "docx"),
        "nset.odt": ("opendocument", "word", "odt"),
        # 表上那张位图那四份：四族各写各的来路，两个生产者把同四张图写成两套明细
        "sheet-pictures.xlsx": ("ooxml", "excel", "xlsx"),
        "sheet-pictures-lo.xlsx": ("ooxml", "excel", "xlsx"),
        "sheet-pictures.ods": ("opendocument", "excel", "ods"),
        "sheet-pictures.xls": ("compound", "excel", "xls"),
        # 文字走向那六份：一家五处写在身上（含节），一家四处全在样式上；rtf 那一族这一支不读
        "dir-cell.docx": ("ooxml", "word", "docx"),
        "dir-cell-lo.docx": ("ooxml", "word", "docx"),
        "dir-sect.docx": ("ooxml", "word", "docx"),
        "dir-cell.odt": ("opendocument", "word", "odt"),
        "dir-sect.odt": ("opendocument", "word", "odt"),
        "dir-cell.rtf": ("rtf", "word", "rtf"),
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
    for name in ("toc.docx", "notes.docx", "toc.odt", "notes.odt", "toc.rtf", "notes.rtf",
                 "toc-full.docx", "toc-full.odt", "toc-full.rtf"):
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
        "toc.rtf 里没有 OOXML 那两键（`galleries` / `sdt` 造不得），而条目那一本从这一批起"
        "有了：整份件没有目录域时那一个键也是数过了的零，不是没看",
        [
            (lbin("office-doc", fixture("toc.rtf")).get("contents") or {}).get(key, "没有这个键")
            for key in ("galleries", "sdt")
        ]
        + [
            lbin("office-doc", fixture("notes.rtf")).get("contents", {}).get("entries", {}).get("paras", "没有这个键"),
            lbin("office-doc", fixture("toc.rtf")).get("contents", {}).get("entries", {}).get("scope", "没有这个键"),
        ],
        ["没有这个键", "没有这个键", 0, "fldrslt"],
    )

    # ── 3b2) 目录里那几条排出来的条目：两家三处，级别取处也不同 ────────────────────
    print("=== 3b2) office-doc contents.entries：让软件自己排的目录，条目与页码是文件写的 ===")
    for name in sorted(one.name for one in FIXTURES.glob("*.docx")):
        check("%s 的条目那一本与读者一致" % name,
              dig(lbin("office-doc", fixture(name)), "contents.entries"),
              files[name]["ooxml"]["contents"]["entries"])
    for name in sorted(one.name for one in FIXTURES.glob("*.odt")):
        check("%s 的条目那一本与读者一致" % name,
              dig(lbin("office-doc", fixture(name)), "contents.entries"),
              files[name]["odt"]["contents"]["entries"])
    # 第三族：哪些段算条目只能按「那个 `\par` 落在不在目录域的字节跨度里」判 ——
    # 这一族既没有 `w:sdt` 壳也没有 `text:index-body` 块，域本身就是壳
    for name in sorted(one.name for one in FIXTURES.glob("*.rtf")):
        check("%s 的条目那一本与读者一致（第三族：域跨度里的段才算）" % name,
              dig(lbin("office-doc", fixture(name)), "contents.entries"),
              files[name]["rtf"]["contents"]["entries"])
    r_full = lbin("office-doc", fixture("toc-full.rtf"))
    d_full = lbin("office-doc", fixture("toc-full.docx"))
    o_full = lbin("office-doc", fixture("toc-full.odt"))
    check(
        "「容器里有几段」同问三个答案：docx 3 / ODF 2 / RTF 2，而条目都是 2。RTF 这一族"
        "把「目录」那一行标题写在 `\\s139`（样式表里那个名字就叫 `TOC Heading`）上、**在开域之前**就 `\\par`，"
        "所以它压根不在域跨度里 —— 与 docx 把标题写成 `sdtContent` 的直接孩子、ODF 把它嵌在 "
        "`text:index-title` 里是三种不同的形状。级别这一问 RTF 与 docx 同一取处（段自己点的样式名，"
        "只不过这一族写的是 `toc 1` / `toc 2` 那种小写带空格的样式表名），页码还是 `\\tab` 之后的字面。"
        "两条都在 `\\fldrslt` 的跨度里，第一条的 `\\s140` 写在**开域那一段**上（`{\\field` 之前），"
        "第二条自己写 `\\pard\\plain \\s141` —— 段号跟着 `\\par` 收，所以两条都拿得到自己的号",
        [dig(d_full, "contents.entries.paras"), dig(o_full, "contents.entries.paras"),
         dig(r_full, "contents.entries.paras"),
         dig(r_full, "contents.entries.entries"), dig(r_full, "contents.entries.with_page"),
         [one["text"] for one in dig(r_full, "contents.entries.list")],
         [one["page_written"] for one in dig(r_full, "contents.entries.list")],
         [one["style"] for one in dig(r_full, "contents.entries.list")],
         [one["level"] for one in dig(r_full, "contents.entries.list")],
         [one["level_from"] for one in dig(r_full, "contents.entries.list")],
         [one["paragraph"] for one in dig(r_full, "contents.entries.list")],
         [one["style_index"] for one in dig(r_full, "contents.entries.list")]],
        [3, 2, 2, 2, 2, ['结构：一级', '结构：二级'], ['1', '2'],
         ['toc 1', 'toc 2'], [1, 2], ['paragraph-style', 'paragraph-style'], [1, 2], [140, 141]],
    )
    check(
        "`toc-full.docx` / `toc-full.odt` 是 LibreOffice **自己排过**的两份（生产者那条路见 README 事实 119）："
        "两家都把页码写成制表符之后的一段**字面**（不是 PAGEREF 域、也不是 `text:page-number` 元素），"
        "两条条目的页码是 '1' 与 '2'（交回的是一整列，与条目文字那一列同长）。docx 那个容器还多写一段「目录」标题（`paras` 3 而 `entries` 2，而条目表里的第一行就是它、`page_written` 为 null），"
        "ODF 把标题嵌在 `text:index-title` 里所以 `paras` 2 —— 同问两个答案，两本账都交，不折成一个数",
        [dig(d_full, "contents.entries.paras"), dig(d_full, "contents.entries.entries"),
         dig(d_full, "contents.entries.with_page"), dig(d_full, "contents.entries.with_anchor"),
         dig(o_full, "contents.entries.paras"), dig(o_full, "contents.entries.entries"),
         dig(o_full, "contents.entries.with_page"),
         [one["text"] for one in dig(d_full, "contents.entries.list")],
         [one["page_written"] for one in dig(d_full, "contents.entries.list")],
         [one["text"] for one in dig(o_full, "contents.entries.list")],
         [one["page_written"] for one in dig(o_full, "contents.entries.list")]],
        [3, 2, 2, 2, 2, 2, 2,
         ['目录', '结构：一级', '结构：二级'], [None, '1', '2'],
         ['结构：一级', '结构：二级'], ['1', '2']],
    )
    check(
        "级别那一问两族取处不同，所以每条都带 `level_from`：docx 写在段自己点的样式名上"
        "（['TOC1', 'TOC2'] → [1, 2]），ODF 段上那个 ['P1', 'P2'] 是**自动样式**、那两个号与级别无关，于是顺锚点两跳去看"
        "被指那一段写的 `text:outline-level`（被指那一段自己写的 outline-level 是 ['1', '2']，解出级别 [1, 2]）。两家的锚点都指得到（`targets_found` 2），"
        "指到的段两家写法也不同：OOXML 那边是被指段的样式名（['Heading1', 'Heading2']），ODF 那边是元素名 `h` 加它自己的"
        "级别，而第二个标题点的样式叫 ['Heading_20_1', 'P3'] —— 照文件交，不替它改成看起来该叫的名字",
        [[one["style"] for one in dig(d_full, "contents.entries.list")][1:],
         [one["level"] for one in dig(d_full, "contents.entries.list")][1:],
         [one["level_from"] for one in dig(d_full, "contents.entries.list")][1:],
         [one["style"] for one in dig(o_full, "contents.entries.list")],
         [one["level"] for one in dig(o_full, "contents.entries.list")],
         [one["level_from"] for one in dig(o_full, "contents.entries.list")],
         dig(d_full, "contents.entries.targets_found"), dig(o_full, "contents.entries.targets_found"),
         [one["target_style"] for one in dig(d_full, "contents.entries.list")][1:],
         [one["target_element"] for one in dig(o_full, "contents.entries.list")],
         [one["target_outline_level"] for one in dig(o_full, "contents.entries.list")],
         [one["target_style"] for one in dig(o_full, "contents.entries.list")]],
        [['TOC1', 'TOC2'], [1, 2], ['paragraph-style', 'paragraph-style'], ['P1', 'P2'], [1, 2], ['target-outline-level', 'target-outline-level'], 2, 2, ['Heading1', 'Heading2'], ['h', 'h'], ['1', '2'], ['Heading_20_1', 'P3']],
    )
    check(
        "`w:pPr/w:tabs/w:tab` 是**制表位定义**不是段里那一下：这一份的第一条条目自己就写着两条定义"
        "加一枚真记号，把定义当记号数就会在第一个字之前先撞到一个，整条被判成「制表符之后」——"
        "读回来条目文字是 ['结构：一级', '结构：二级'] 的第一串、页码是第二串，而 ODF 那一份的页码挂在 `text:tab` **之后的一段裸文字**上"
        "（只收元素的直接文字就一个字也读不到）。老那两份 `toc.docx` / `toc.odt` 里目录是**注进去的壳**，"
        "`entries` 因此是 0 / 0（数过了没有，不是没读）；`md.docx` 根本没有目录（`paras` 0）。"
        "最后那一格是第三族：RTF 也交这一本了，整份账拿读者的当期望（它的形状见下面「同问三个答案」那条）",
        [[one["text"] for one in dig(d_full, "contents.entries.list")][1],
         [one["page_written"] for one in dig(d_full, "contents.entries.list")][1],
         [one["text"] for one in dig(o_full, "contents.entries.list")][0],
         [one["page_written"] for one in dig(o_full, "contents.entries.list")][0],
         dig(lbin("office-doc", fixture("toc.docx")), "contents.entries.entries"),
         dig(lbin("office-doc", fixture("toc.docx")), "contents.entries.paras"),
         dig(lbin("office-doc", fixture("toc.odt")), "contents.entries.entries"),
         dig(lbin("office-doc", fixture("toc.odt")), "contents.entries.paras"),
         dig(lbin("office-doc", fixture("md.docx")), "contents.entries.paras"),
         dig(lbin("office-doc", fixture("md.docx")), "contents.entries.scope"),
         lbin("office-doc", fixture("toc.rtf")).get("contents", {}).get("entries")],
        ['结构：一级', '1', '结构：一级', '1', 0, 2, 0, 1, 0, "sdt-content",
         files["toc.rtf"]["rtf"]["contents"]["entries"]],
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
                 "tables-merged.docx", "tables-merged.odt", "shaded.odt"):
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
        # 字符格式这一本对**每一份 docx** 都比：段里的每一串字各交一份自己的 rPr
        check("%s 字符格式与读者一致（每一串字自己的 rPr）" % name,
              got.get("run_formats"), want.get("run_formats"))
        check("%s 的分栏与读者一致" % name, got.get("columns"), want.get("columns"))
    for name in sorted(one.name for one in FIXTURES.glob("*.odt")):
        got = lbin("office-doc", fixture(name)).get("structure", {})
        want = files[name]["odt"]
        check("%s 段落格式与读者一致" % name, got.get("paragraph_formats"), want.get("paragraph_formats"))
        # 这一族还要多比一本：span 有几个、夹在中间不包起来的字有几处
        check("%s 字符格式与读者一致（span 那一跳与不包起来的字）" % name,
              got.get("run_formats"), want.get("run_formats"))
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
    # RTF 这一族现在也交这份账：号写在段上（`\ilvl` + `\ls`），定义在 `{\*\listtable`
    # 那个星号群里，而 `listoverridetable` 在同一份文件里却是**不带星号**写的 ——
    # 只认一条路径就会一份读到、一份读不到，所以两边都要走
    for name in sorted(one.name for one in FIXTURES.glob("*.rtf")):
        got = lbin("office-doc", fixture(name)).get("structure", {})
        want = files[name]["rtf"]
        check("%s 编号与读者一致" % name, got.get("numbering"), want.get("numbering"))
    listrtf = lbin("office-doc", fixture("lists.rtf"))
    check(
        "RTF 的三本账：七份定义、七条号本、一份九级",
        [dig(listrtf, "structure.numbering.list_definitions"),
         dig(listrtf, "structure.numbering.overrides"),
         dig(listrtf, "structure.numbering.levels"),
         dig(listrtf, "structure.numbering.checked"),
         dig(listrtf, "structure.numbering.listed"),
         dig(listrtf, "structure.numbering.with_ls"),
         dig(listrtf, "structure.numbering.label_words")],
        [7, 7, 63, 7, 5, 5, 5],
    )
    check(
        "段上的号先落号本，号本再点名定义（三跳各自一个布尔）",
        [dig(listrtf, "structure.numbering.override_list[3]"),
         dig(listrtf, "structure.numbering.list[0].ls"),
         dig(listrtf, "structure.numbering.list[0].list_id"),
         dig(listrtf, "structure.numbering.list[0].template_id"),
         dig(listrtf, "structure.numbering.list[0].definition_found"),
         dig(listrtf, "structure.numbering.list[0].level_found")],
        [{"ls": "4", "list_id": "4", "override_count": "0"}, "4", "4", "4", True, True],
    )
    # 这一族不在级上写 `\ilvl`（一份也没有），所以级别号是「这份 list 里第几个
    # {\listlevel」—— 号是读者按顺序给的，这一点要看得见
    check(
        "级别号是第几条 {\\listlevel，号型按写的交",
        [dig(listrtf, "structure.numbering.definitions[0].levels"),
         dig(listrtf, "structure.numbering.definitions[0].nfc[0]"),
         dig(listrtf, "structure.numbering.definitions[3].nfc[0]"),
         dig(listrtf, "structure.numbering.definitions[6].nfc[0]"),
         dig(listrtf, "structure.numbering.list[4].level.at")],
        [9, "23", "0", "255", 1],
    )
    # 圆点那一级：定义里点 `\f1`（字体表里那一个就是 Symbol），
    # 而文件自己算出来的那一句标签点的是 `\f7` —— 两个号各按各的交，不替它对齐
    check(
        "同一枚圆点：定义里的字体号与标签里的字体号不是一号",
        [dig(listrtf, "structure.numbering.list[2].level.nfc"),
         dig(listrtf, "structure.numbering.list[2].level.font"),
         dig(listrtf, "structure.numbering.list[2].label_font"),
         dig(listrtf, "structure.numbering.list[2].level.level_text"),
         dig(listrtf, "structure.numbering.list[2].label_written"),
         dig(listrtf, "structure.numbering.list[2].label_tab")],
        ["23", "1", "7", "\\'01\\u-3913 ?;", "\\pard\\plain \\f7 \\u-3913\\'3f\\tab", True],
    )
    # 那一句标签是**生产者算好写进流的**，不是我们数出来的：原样与解出来的字一起交
    check(
        "标签那一跳：原样一句与解出来的字并存",
        [dig(listrtf, "structure.numbering.list[0].label_written"),
         dig(listrtf, "structure.numbering.list[0].label"),
         dig(listrtf, "structure.numbering.list[0].level.level_text"),
         dig(listrtf, "structure.numbering.list[0].indent.li"),
         dig(listrtf, "structure.numbering.list[0].level.indent")],
        ["\\pard\\plain  1.\\tab", "1.", "\\'02\\'00.;", "360", "360"],
    )
    # 跨家族最狠的一条：同一段（第二级）在 docx 手里那一级没定义，
    # 在 LibreOffice 的 RTF 导出里九级都写全了 —— 于是 level_found 一边 false 一边 true
    check(
        "同一段落在 docx 与 RTF 手里的两级答案",
        [dig(listdoc, "structure.numbering.list[4].level_found"),
         dig(listrtf, "structure.numbering.list[4].level_found"),
         dig(listdoc, "structure.numbering.list[4].ilvl"),
         dig(listrtf, "structure.numbering.list[4].ilvl")],
        [False, True, "1", "1"],
    )
    check(
        "样式名与段号：段上的 `\\sN` 到样式表里查名字",
        [dig(listrtf, "structure.numbering.list[0].style_index"),
         dig(listrtf, "structure.numbering.list[0].style_name"),
         dig(listrtf, "structure.numbering.list[2].style_name"),
         dig(listrtf, "structure.numbering.list[0].at"),
         dig(listrtf, "structure.numbering.list[4].at")],
        [70, "List Number", "List Bullet", 1, 5],
    )
    # 「定义了没人用」在这一族更夸张：六份 RTF 都带着整个模板的七份定义，
    # 而榜上一条都没有（`notes-end.rtf` 那份只带一份）
    empty_rtf = {}
    for name in ("para.rtf", "toc.rtf", "notes.rtf", "tables.rtf", "comments.rtf",
                 "notes-hf.rtf", "notes-end.rtf"):
        had = lbin("office-doc", fixture(name)).get("structure", {}).get("numbering") or {}
        empty_rtf[name] = [had.get("list_definitions"), had.get("levels"), had.get("listed")]
    check(
        "带着定义没人用：六份 RTF 七份定义九级一段也没套",
        [one for name, one in sorted(empty_rtf.items()) if name != "notes-end.rtf"],
        [[7, 63, 0]] * 6,
    )
    check(
        "少带一份的那条：notes-end.rtf 只有一份定义",
        empty_rtf["notes-end.rtf"], [1, 9, 0],
    )
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
        [dig(odtw, "structure.table_layouts.covered"),
         dig(odtw, "structure.table_layouts.list[0].columns[0].repeated"),
         dig(odtw, "structure.table_layouts.list[0].columns[0].mm"),
         dig(wide, "structure.table_layouts.list[0].grid[0].w")],
        [5, 3, 5080, "2880"],
    )
    shade = lbin("office-doc", fixture("shaded.docx"))
    shade_lo = lbin("office-doc", fixture("shaded-lo.docx"))
    # 格子自己的三样各在一格，两格什么都没设
    check(
        "格子的底色、边框与垂直对齐（python-docx 那一份）",
        [dig(shade, "structure.table_layouts.shade_cells"),
         dig(shade, "structure.table_layouts.border_cells"),
         dig(shade, "structure.table_layouts.align_cells"),
         dig(shade, "structure.table_layouts.empty_border_cells"),
         dig(shade, "structure.table_layouts.list[0].cells[0].shading"),
         dig(shade, "structure.table_layouts.list[0].cells[1].borders"),
         dig(shade, "structure.table_layouts.list[0].cells[2].valign"),
         dig(shade, "structure.table_layouts.list[0].cells[3].borders_present")],
        [1, 1, 1, 0, {"val": "clear", "color": "auto", "fill": "FFFF00"},
         {"top": {"val": "double", "sz": "6", "space": "0", "color": "FF0000"}}, "bottom",
         False],
    )
    # 重写那一家给每一格都补一个空的 tcBorders：「元素在而一条边都没有」不能与「整个不写」混
    check(
        "同一个格式重写：五格补了空的 tcBorders，值本身一个没改",
        [dig(shade_lo, "structure.table_layouts.empty_border_cells"),
         dig(shade_lo, "structure.table_layouts.border_cells"),
         dig(shade_lo, "structure.table_layouts.list[0].cells[3].borders"),
         dig(shade_lo, "structure.table_layouts.list[0].cells[3].borders_present"),
         dig(shade_lo, "structure.table_layouts.list[0].cells[0].shading"),
         dig(shade_lo, "structure.table_layouts.list[0].cells[2].valign")],
        [5, 1, {}, True, {"val": "clear", "color": "auto", "fill": "FFFF00"}, "bottom"],
    )
    # ODF 那一侧同样三样，但值一律在一跳之外的自动样式上（名字按地址拼）
    shodt = lbin("office-doc", fixture("shaded.odt"))
    check(
        "ODF 的格子三样：六格六份样式，全在 content.xml",
        [dig(shodt, "structure.table_layouts.cell_styles"),
         dig(shodt, "structure.table_layouts.cell_styles_in_content"),
         dig(shodt, "structure.table_layouts.cell_styles_in_styles"),
         dig(shodt, "structure.table_layouts.cell_elements"),
         dig(shodt, "structure.table_layouts.cells_unresolved"),
         dig(shodt, "structure.table_layouts.shade_cells"),
         dig(shodt, "structure.table_layouts.align_cells"),
         dig(shodt, "structure.table_layouts.lined_cells"),
         dig(shodt, "structure.table_layouts.padded_cells")],
        [6, 6, 0, 6, 0, 1, 1, 1, 6],
    )
    check(
        "ODF 的格子逐条：点名在格上，值在样式里",
        [dig(shodt, "structure.table_layouts.list[0].cells[0].style"),
         dig(shodt, "structure.table_layouts.list[0].cells[0].style_part"),
         dig(shodt, "structure.table_layouts.list[0].cells[0].attrs.table:style-name"),
         dig(shodt, "structure.table_layouts.list[0].cells[0].shading"),
         dig(shodt, "structure.table_layouts.list[0].cells[0].borders"),
         dig(shodt, "structure.table_layouts.list[0].cells[0].lined"),
         dig(shodt, "structure.table_layouts.list[0].cells[1].borders.border-top"),
         dig(shodt, "structure.table_layouts.list[0].cells[1].lined"),
         dig(shodt,
             "structure.table_layouts.list[0].cells[1].written.style:border-line-width-top"),
         dig(shodt, "structure.table_layouts.list[0].cells[2].valign")],
        ["表格1.A1", "content.xml", "表格1.A1", "#ffff00", {"border": "none"}, False,
         "2.25pt double #ff0000", True, "0.026cm 0.026cm 0.026cm", "bottom"],
    )
    # 同一份格式两族各写一遍：底色与双线两种说法，谁也没替谁归一化
    check(
        "同一格两种说法：docx 大写不带 #，ODF 小写带 #；双线一边记每根一边记合起来",
        [dig(shade, "structure.table_layouts.list[0].cells[0].shading.fill"),
         dig(shodt, "structure.table_layouts.list[0].cells[0].shading"),
         dig(shade, "structure.table_layouts.list[0].cells[1].borders.top.sz"),
         dig(shodt, "structure.table_layouts.list[0].cells[1].written.style:border-line-width-top")],
        ["FFFF00", "#ffff00", "6", "0.026cm 0.026cm 0.026cm"],
    )
    check(
        "ODF 的占位格两种写法：一种连样式名都没点，跨几列写在格子上",
        [dig(odtw, "structure.table_layouts.cell_elements"),
         dig(odtw, "structure.table_layouts.covered_cells"),
         dig(odtw, "structure.table_layouts.padded_cells"),
         dig(odtw, "structure.table_layouts.list[0].cells[0].attrs.table:number-columns-spanned"),
         dig(odtw, "structure.table_layouts.list[0].cells[1].covered"),
         dig(odtw, "structure.table_layouts.list[0].cells[1].attrs"),
         dig(odtw, "structure.table_layouts.list[0].cells[1].style"),
         dig(odtw, "structure.table_layouts.list[1].cells[0].attrs.table:number-rows-spanned"),
         dig(odtw, "structure.table_layouts.list[1].cells[2].covered"),
         dig(odtw, "structure.table_layouts.list[1].cells[2].style")],
        [10, 2, 9, "2", True, {}, None, "2", True, "表格2.A1"],
    )
    check(
        "什么都没设的那张 ODF 表：三本都是 0，而 padding 十格全写",
        [dig(odt, "structure.table_layouts.shade_cells"),
         dig(odt, "structure.table_layouts.align_cells"),
         dig(odt, "structure.table_layouts.lined_cells"),
         dig(odt, "structure.table_layouts.cell_elements"),
         dig(odt, "structure.table_layouts.cell_styles"),
         dig(odt, "structure.table_layouts.padded_cells")],
        [0, 0, 0, 10, 2, 10],
    )

    for name in ("tables.rtf",):
        got = lbin("office-doc", fixture(name)).get("structure", {})
        check("%s 不报表宽：RTF 里没有「表宽」这个东西（只有 \\intbl 与格分隔）" % name,
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
    # 每一块文字前面那条四字记录写的数值：两份读者各走一遍树，逐块比「数值 + 整截字」。
    # 而 0 是不是「这一页的标题那块」，拿同一份稿子的 pptx 那一头逐页对（不是背规范）
    for stem in ("deck", "deck-ph-lo", "deck-tables-lo"):
        name = stem + ".ppt"
        got = lbin("office-slide", fixture(name))
        want = files[name]["ppt_text"]["slides"]
        check("%s 每页那几块（文件写的数值与整截字）与读者一致" % name,
              [one["blocks"] for one in got.get("slides", [])],
              [one["blocks"] for one in want])
        check("%s 每页块数与 blocks 那一本同一问" % name,
              [one["blocks_total"] for one in got.get("slides", [])],
              [len(one["blocks"]) for one in want])
        peer = lbin("office-slide", fixture(stem + ".pptx"))
        check("%s 写着 0 的那一块就是 %s.pptx 每页的标题（两副面孔逐页对）" % (name, stem),
              [[one["text"] for one in page["blocks"] if one["type_written"] == 0]
               for page in got.get("slides", [])],
              [[one["title"]] if one["title"] else [] for one in peer.get("slides", [])])
        check("%s 除了 0 就是 4：这一族没有第三种数在这几份件里出现过" % name,
              sorted({one["type_written"] for page in got.get("slides", [])
                      for one in page["blocks"]}),
              [0, 4])
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

    # ── 2k) 页上那张表：一张表的三种写法（pptx 两家 + odp 那一转） ────────────
    print("=== 2k) office-slide 的表：网格、行高、tcPr 与合并的三本账 ===")
    for name in ("deck.pptx", "deck-lo.pptx", "deck-tables.pptx", "deck-tables-lo.pptx"):
        want = files[name]["ooxml"]
        got = lbin("office-slide", fixture(name))
        check(
            "%s 每页那张表的整份账与读者一致" % name,
            [one.get("table_list") for one in got.get("slides", [])],
            [one.get("table_list") for one in want["slides"]],
        )
    first = lbin("office-slide", fixture("deck-tables.pptx"))
    second = lbin("office-slide", fixture("deck-tables-lo.pptx"))
    T = "slides[0].table_list[0]"
    check(
        "同一张表：一家把三个开关写在 tblPr 上并点名一条 tableStyleId，另一家这个元素在场但一个字没说",
        [dig(first, T + ".pr_present"), dig(first, T + ".written"), dig(first, T + ".style_present"),
         dig(first, T + ".style_id"),
         dig(second, T + ".pr_present"), dig(second, T + ".written"), dig(second, T + ".style_present"),
         dig(second, T + ".style_id")],
        [True, {"firstRow": "1", "bandRow": "1"}, True,
         "{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}",
         True, {}, False, None],
    )
    check(
        "列宽两家一字不差（EMU 与换算出来的 0.01mm 都是同一批数）",
        [dig(first, T + ".column_elements"), dig(first, T + ".grid_sum"), dig(first, T + ".grid_sum_mm"),
         [dig(first, T + ".grid[%d].mm" % i) for i in range(3)],
         dig(second, T + ".grid_sum"), dig(second, T + ".grid_sum_mm")],
        [3, 6400800, 17780, [7620, 5080, 5080], 6400800, 17780],
    )
    check(
        "重写不是无损的：没说过话的那一行一家写 609600、另一家写 609480，而换算都是 1693",
        [dig(first, T + ".rows[0].written.h"), dig(second, T + ".rows[0].written.h"),
         dig(first, T + ".rows[0].h"), dig(second, T + ".rows[0].h"),
         dig(first, T + ".rows[0].mm"), dig(second, T + ".rows[0].mm"),
         dig(first, T + ".rows[1].h"), dig(second, T + ".rows[1].h"), dig(second, T + ".rows[1].mm")],
        ["609600", "609480", 609600, 609480, 1693, 1693, 914400, 914400, 2540],
    )
    check(
        "几个格、跨度之和、网格几列是三本账：9 个格、跨度之和 10、三列",
        [dig(first, T + ".cell_elements"), dig(first, T + ".span_sum"), dig(first, T + ".column_elements"),
         dig(first, T + ".rows[0].cells"), dig(first, T + ".rows[0].span_sum"),
         dig(second, T + ".cell_elements"), dig(second, T + ".span_sum")],
        [9, 10, 3, 3, 4, 9, 10],
    )
    check(
        "被合掉的那一格照样在场：字是空的、一个 run 也没有，两家倒是一样",
        [dig(first, T + ".merged_from"), dig(second, T + ".merged_from"),
         dig(first, T + ".spanning"), dig(second, T + ".spanning"),
         [dig(first, "%s.rows[0].list[%d].%s" % (T, i, k))
          for i in range(3) for k in ("merge_from", "text", "runs")],
         dig(second, "slides[0].table_list[0].rows[0].list[1].merge_from"),
         dig(second, "slides[0].table_list[0].rows[0].list[1].text")],
        [2, 2, 2, 2, [False, "科目\n金额", 2, True, "", 0, False, "备注", 1], True, ""],
    )
    check(
        "起点的跨度照写的交：横向两列、纵向两行，没写的按 1",
        [dig(first, T + ".rows[0].list[0].written"), dig(first, T + ".rows[0].list[0].span_cols"),
         dig(first, T + ".rows[1].list[2].written"), dig(first, T + ".rows[1].list[2].span_rows"),
         dig(first, T + ".rows[0].list[2].span_cols"), dig(first, T + ".rows[0].list[2].span_rows")],
        [{"gridSpan": "2"}, 2, {"rowSpan": "2"}, 2, 1, 1],
    )
    check(
        "一格两段：段的字用换行拼起来，段数与 run 数各交一个",
        [dig(first, T + ".rows[2].list[0].text"), dig(first, T + ".rows[2].list[0].paragraphs"),
         dig(first, T + ".rows[2].list[0].runs"), dig(second, T + ".rows[2].list[0].text"),
         dig(second, T + ".rows[2].list[0].paragraphs"), dig(second, T + ".rows[2].list[0].runs")],
        ["网络\n设备", 2, 2, "网络\n设备", 2, 2],
    )
    check(
        "tcPr 里有什么也是一家的事：一家每格都有四道边加一个填充，另一家的格子是空的",
        [dig(first, T + ".rows[0].list[0].tcpr_present"), dig(first, T + ".rows[0].list[0].tcpr"),
         dig(first, T + ".rows[0].list[0].tcpr_paths"),
         dig(second, T + ".rows[0].list[0].tcpr_paths"),
         dig(second, T + ".rows[0].list[0].tcpr")],
        [True, {}, [], ["lnL", "lnR", "lnT", "lnB", "solidFill"],
         {"anchor": "t", "marL": "91440", "marR": "91440", "marT": "45720", "marB": "45720"}],
    )
    check(
        "同一件事写在两个地方：LibreOffice 把边距又往 a:bodyPr 里抄了一遍（两家分开交）",
        [dig(first, T + ".rows[1].list[0].body"), dig(second, T + ".rows[1].list[0].body"),
         dig(second, T + ".rows[1].list[0].tcpr.marR"), dig(second, T + ".rows[0].list[1].body")],
        [{}, {"rIns": "45720", "anchor": "b"}, "45720",
         {"lIns": "90000", "tIns": "45000", "rIns": "90000", "bIns": "45000", "anchor": "t"}],
    )
    check(
        "有字的格数两家一样（合掉的那两格没字）",
        [dig(first, T + ".with_text"), dig(second, T + ".with_text"),
         dig(first, T + ".rows[0].with_text"), dig(second, T + ".rows[2].with_text")],
        [7, 7, 2, 2],
    )
    # 第三种写法：LibreOffice 自己转出来的 odp。尺寸换成 cm、合并换成「另写一格 covered」
    odp_sizes = lyco_grid.odp_table_sizes(fixture("deck-tables.odp"))
    check(
        "同一张表的第三副账（odp）：列宽与行高换算成 0.01mm 与 pptx 两家一模一样",
        [[one["columns"] for one in odp_sizes], [one["rows"] for one in odp_sizes],
         [dig(first, T + ".grid[%d].mm" % i) for i in range(3)],
         [dig(second, T + ".rows[%d].mm" % i) for i in range(3)],
         [dig(first, T + ".rows[%d].mm" % i) for i in range(3)]],
        [[[7620, 5080, 5080]], [[1693, 2540, 1693]], [7620, 5080, 5080],
         [1693, 2540, 1693], [1693, 2540, 1693]],
    )
    odp_grid = lyco_grid.grids_of(fixture("deck-tables.odp"))
    check(
        "合并的第三种写法：ODF 把被盖住的那格另写成 covered-table-cell（不是 hMerge）",
        [[(cell["text"], cell["col_span"], cell["row_span"], cell["covered"])
          for row in odp_grid[0]["rows"] for cell in row],
         [dig(first, "%s.rows[%d].list[%d].text" % (T, r, c))
          for r in range(3) for c in range(3)]],
        [[("科目\n金额", 2, None, False), ("", None, None, True), ("备注", None, None, False),
          ("服务器", None, None, False), ("124000", None, None, False),
          ("含税", None, 2, False), ("网络\n设备", None, None, False),
          ("8000", None, None, False), ("", None, None, True)],
         ["科目\n金额", "", "备注", "服务器", "124000", "含税", "网络\n设备", "8000", ""]],
    )
    # odp 是第三种写法：表在 `draw:frame` 里，而**表名与位置只在 frame 上**
    # （`table:table` 自己实测一个字都不写），读表的那一份与 .ods 是同一条路
    for name in ("deck-tables.odp", "deck.odp", "deck-chart.odp"):
        want = files[name].get("odp") or {}
        got = lbin("office-slide", fixture(name))
        check(
            "%s 每页那些表整份账与读者一致" % name,
            [one.get("table_list") for one in got.get("slides", [])],
            [one.get("table_list") for one in want.get("slides", [])],
        )
    odp_slide = lbin("office-slide", fixture("deck-tables.odp"))
    # 这张表在第 0 页上（`deck.odp` 那张在第 1 页，两条钉各按各的页号写）
    O = "slides[0].table_list[0]"
    check(
        "deck-tables.odp 的表住在第 0 页：先钉页号，下面那几条路径才有意义",
        [len(dig(odp_slide, "slides") or []),
         [len(one.get("table_list") or []) for one in odp_slide.get("slides", [])],
         dig(odp_slide, "slides[0].tables")],
        [1, [1], 1],
    )
    check(
        "odp 的表名与位置在 frame 上，而 `table:table` 自己一个字都不写",
        [dig(lbin("office-slide", fixture("deck.odp")), "slides[1].tables"),
         dig(lbin("office-slide", fixture("deck.odp")), "slides[1].table_list[0].frame.name"),
         dig(lbin("office-slide", fixture("deck.odp")), "slides[1].table_list[0].written"),
         dig(lbin("office-slide", fixture("deck.odp")), "slides[1].table_list[0].name")],
        [1, "Table 2", {}, ""],
    )
    check(
        "同一张表的第三副账：frame 的宽 17.779cm 与两副 pptx 的 6400800 EMU 是同一个数",
        [dig(odp_slide, O + ".frame.width"), dig(odp_slide, O + ".frame.x"),
         dig(odp_slide, O + ".frame.y"), dig(odp_slide, O + ".frame.height"),
         [dig(odp_slide, "%s.layout.columns.list[%d].size_mm" % (O, i)) for i in range(3)],
         [dig(odp_slide, "%s.layout.rows.list[%d].size_mm" % (O, i)) for i in range(3)]],
        ["17.779cm", "2.54cm", "5.08cm", "6.097cm", [7620, 5080, 5080],
         [1693, 2540, 1693]],
    )
    check(
        "Impress 不把列补齐：三张列元素就盖三列（.ods 那边每张表都补到 16384 列）",
        [dig(odp_slide, O + ".layout.columns.elements"),
         dig(odp_slide, O + ".layout.columns.spans"),
         dig(odp_slide, O + ".layout.rows.elements"),
         dig(odp_slide, O + ".layout.rows.spans"),
         dig(odp_slide, O + ".rows"), dig(odp_slide, O + ".columns")],
        [3, 3, 3, 3, 3, 3],
    )
    check(
        "同一个生产者换个应用，连「写不写 use-optimal」都不一样：odp 的列上写 false，.ods 的列上根本不写",
        [dig(odp_slide, "%s.layout.columns.list[0].optimal" % O),
         dig(odp_slide, "%s.layout.rows.list[0].optimal" % O),
         dig(lbin("office-sheet", fixture("book.ods")), "sheets[0].layout.columns.list[0].optimal"),
         dig(lbin("office-sheet", fixture("book.ods")), "sheets[0].layout.rows.list[0].optimal")],
        ["false", "false", None, "true"],
    )
    check(
        "合并的第三种写法：起头那格写 number-*-spanned，被盖住的那格照样在（covered 另数一本）",
        [dig(odp_slide, O + ".cells"), dig(odp_slide, O + ".covered"),
         dig(odp_slide, O + ".merged"),
         dig(odp_slide, O + ".cell_list[0].span_cols"),
         dig(odp_slide, O + ".cell_list[0].text"),
         dig(odp_slide, O + ".cell_list[4].span_rows")],
        [7, 2, 2, 2, "科目\n金额", 2],
    )
    check(
        "格子点到的样式与 .ods 同一类（实测四格没点名、两格是占位格叫 standard）",
        [dig(odp_slide, O + ".cell_list[2].style"),
         dig(odp_slide, O + ".cell_list[6].style"),
         dig(odp_slide, O + ".cell_list[0].style"),
         dig(odp_slide, O + ".layout.stated.number-columns"),
         dig(odp_slide, O + ".layout.unit")],
        ["ce2", "ce5", None, None, "0.01mm"],
    )
    # 同一批字，两家对「这格是什么类型」的说法完全不同：.ods 每格都写 office:value-type，
    # 而 Impress 九个格子元素一个都没写（其中七个有字）。没写就交 null，不替它推
    check(
        "格子的类型只交文件写了的：odp 全是 null，.ods 那边同样有字的格子写着 string",
        [dig(odp_slide, O + ".cell_list[0].kind"),
         dig(odp_slide, O + ".cell_list[2].kind"),
         dig(odp_slide, O + ".cell_list[3].kind"),
         dig(lbin("office-sheet", fixture("book.ods")), "sheets[0].cell_list[0].kind"),
         dig(lbin("office-sheet", fixture("book.ods")), "sheets[0].cell_list[1].kind")],
        [None, None, None, "string", "string"],
    )
    # 那一跳到样式文件里去拿：实测演示稿把底色与垂直对齐写在 `loext:graphic-properties`
    # （LibreOffice 自己的实验命名空间），边写在 `style:paragraph-properties`，
    # 而 odt 表格用的 `style:table-cell-properties` 这一族一个都没有
    props = dig(odp_slide, O + ".cell_list[2].style_props") or {}
    check(
        "格子样式那一跳：底色住在 loext:graphic-properties，边住在 paragraph-properties",
        [props.get("style"), props.get("found"), props.get("part"), props.get("family"),
         props.get("parent"), props.get("graphic_element"),
         (props.get("graphic") or {}).get("draw:fill"),
         (props.get("graphic") or {}).get("draw:fill-color"),
         (props.get("graphic") or {}).get("draw:textarea-vertical-align"),
         (props.get("graphic") or {}).get("fo:padding-left"),
         props.get("paragraph_element"),
         (props.get("paragraph") or {}).get("fo:border"),
         props.get("table_cell_element")],
        ["ce2", True, "content.xml", "table-cell", None, "loext:graphic-properties",
         "solid", "#d0d8e7", "bottom", "0.254cm",
         "style:paragraph-properties", "0.48pt solid #ffffff", None],
    )
    check(
        "这张表上的样式账：三格点了名、三格解开、四格根本没点，properties 只两处",
        dig(odp_slide, O + ".cell_styles"),
        {"named": 3, "resolved": 3, "unwritten": 4, "with_graphic": 3, "with_paragraph": 3,
         "with_table_cell_properties": 0,
         "elements": ["loext:graphic-properties", "style:paragraph-properties"]},
    )
    check(
        "没点样式的格子说「没点」，不是「点了找不到」：style 与 found 分两笔",
        [dig(odp_slide, O + ".cell_list[0].style_props.style"),
         dig(odp_slide, O + ".cell_list[0].style_props.found"),
         dig(odp_slide, O + ".cell_list[0].style_props.graphic"),
         dig(odp_slide, O + ".cell_list[6].style_props.style"),
         dig(odp_slide, O + ".cell_list[6].style_props.found"),
         dig(odp_slide, O + ".cell_list[6].style_props.graphic.draw:fill-color")],
        [None, False, None, "ce5", True, "#ffff00"],
    )


    # ── 2l) 页上那几条链接：两家的差别在「要不要再跳一跳」 ────────
    for name in ("deck-links.pptx", "deck-links-lo.pptx", "deck-links.odp"):
        deck = files[name].get("ooxml") or files[name].get("odp") or {}
        got = lbin("office-slide", fixture(name))
        check(
            "%s 每页那些链接整份账与读者一致" % name,
            [one.get("links") for one in got.get("slides", [])],
            [one.get("links") for one in deck.get("slides", [])],
        )
    link_deck = lbin("office-slide", fixture("deck-links.pptx"))
    # 那一页的关系表整份账：Rust 侧曾因部件名少了中间那段 `_rels/` 而全是空表，
    # 键在、值也自洽，只有跟读者对一遍才露出来
    for name in ("deck-links.pptx", "deck-links-lo.pptx", "deck.pptx", "deck-chart.pptx"):
        deck = files[name].get("ooxml") or {}
        check(
            "%s 每页那张关系表与读者一致" % name,
            [one.get("relationships") for one in lbin("office-slide", fixture(name)).get("slides", [])],
            [one.get("relationships") for one in deck.get("slides", [])],
        )
    check(
        "关系表里内、外两种 Target：内部的解成包内全名，外部的照原样",
        [dig(link_deck, "slides[0].relationships[0].kind"),
         dig(link_deck, "slides[0].relationships[0].target"),
         dig(link_deck, "slides[0].relationships[0].external"),
         dig(link_deck, "slides[0].relationships[1].kind"),
         dig(link_deck, "slides[0].relationships[1].target"),
         dig(link_deck, "slides[0].relationships[1].external"),
         len(dig(link_deck, "slides[0].relationships")),
         len(dig(link_deck, "slides[1].relationships"))],
        ["slideLayout", "ppt/slideLayouts/slideLayout6.xml", False,
         "hyperlink", "https://example.com/budget", True, 4, 1],
    )
    check(
        "一条链都指不到的那页，关系表仍然把图片与备注页记着（两本账不是一回事）",
        [dig(lbin("office-slide", fixture("deck.pptx")), "slides[0].links.total"),
         dig(lbin("office-slide", fixture("deck.pptx")), "slides[0].relationships[1].kind"),
         dig(lbin("office-slide", fixture("deck.pptx")), "slides[0].relationships[1].target"),
         dig(lbin("office-slide", fixture("deck.pptx")), "slides[0].relationships[2].kind"),
         dig(lbin("office-slide", fixture("deck.pptx")), "slides[0].relationships[2].target")],
        [0, "notesSlide", "ppt/notesSlides/notesSlide1.xml", "image", "ppt/media/image1.png"],
    )
    check(
        "OOXML 那一族要跳两跳：run 里只有一个号，地址在这一页自己的关系表里",
        [dig(link_deck, "slides[0].links.total"), dig(link_deck, "slides[0].links.external"),
         dig(link_deck, "slides[0].links.unresolved"),
         dig(link_deck, "slides[0].links.list[0].text"),
         dig(link_deck, "slides[0].links.list[0].target"),
         dig(link_deck, "slides[0].links.list[0].scheme"),
         dig(link_deck, "slides[0].links.list[0].hop"),
         dig(link_deck, "slides[0].links.list[1].scheme"),
         dig(link_deck, "slides[0].links.list[2].target"),
         dig(link_deck, "slides[1].links.total")],
        [3, 3, 0, "第三季度的说明", "https://example.com/budget", "https", "rels",
         "mailto", "https://example.com/raw", 0],
    )
    check(
        "重写不换地址只换号：一家排 rId2、一家排 rId1 —— 号只交出来，不拿来比",
        [dig(link_deck, "slides[0].links.list[0].id"),
         dig(lbin("office-slide", fixture("deck-links-lo.pptx")), "slides[0].links.list[0].id"),
         dig(lbin("office-slide", fixture("deck-links-lo.pptx")), "slides[0].links.total"),
         dig(lbin("office-slide", fixture("deck-links-lo.pptx")), "slides[0].links.list[0].target")],
        ["rId2", "rId1", 3, "https://example.com/budget"],
    )
    odp_links = lbin("office-slide", fixture("deck-links.odp"))
    check(
        "ODF 那一族地址就写在字上：没有第二跳，也没有「站内 / 站外」那个开关",
        [dig(odp_links, "slides[0].links.list[0].hop"),
         dig(odp_links, "slides[0].links.list[0].external"),
         dig(odp_links, "slides[0].links.list[0].id"),
         dig(odp_links, "slides[0].links.list[0].target"),
         dig(odp_links, "slides[0].links.list[0].text"),
         dig(odp_links, "slides[0].links.external"),
         dig(odp_links, "slides[0].links.total"),
         dig(odp_links, "slides[1].links.total")],
        ["inline", None, None, "https://example.com/budget", "第三季度的说明", 0, 3, 0],
    )
    check(
        "文本框被 Impress 改写成了 custom-shape：链接不因此不见，页上的 frame 却少一个",
        [dig(odp_links, "slides[0].frames"), dig(odp_links, "slides[0].links.total")],
        [2, 3],
    )

    # ── 2m) 放映时隐藏这一页：三家各写一处，ODF 那一处还要跳一跳 ──────
    hidden_pptx = lbin("office-slide", fixture("deck-hidden.pptx"))
    hidden_lo = lbin("office-slide", fixture("deck-hidden-lo.pptx"))
    hidden_odp = lbin("office-slide", fixture("deck-hidden.odp"))
    for name, got in (
        ("deck-hidden.pptx", hidden_pptx),
        ("deck-hidden-lo.pptx", hidden_lo),
        ("deck-hidden.odp", hidden_odp),
    ):
        deck = files[name].get("ooxml") or files[name].get("odp") or {}
        check(
            "%s 每页藏不藏与读者一致" % name,
            [one.get("hidden") for one in got.get("slides", [])],
            [one.get("hidden") for one in deck.get("slides", [])],
        )
    check(
        "deck-hidden.odp 那一跳整份账与读者一致（页只点名样式）",
        [one.get("visibility") for one in hidden_odp.get("slides", [])],
        [one.get("visibility") for one in (files["deck-hidden.odp"].get("odp") or {}).get("slides", [])],
    )
    check(
        "同一件事三处写法：pptx 在根上的 show，odp 在页点名的那份样式里",
        [dig(hidden_pptx, "slides[0].hidden"), dig(hidden_pptx, "slides[1].hidden"),
         dig(hidden_lo, "slides[1].hidden"),
         dig(hidden_odp, "slides[0].hidden"), dig(hidden_odp, "slides[1].hidden"),
         dig(hidden_odp, "slides[0].visibility.page_style"),
         dig(hidden_odp, "slides[1].visibility.page_style"),
         dig(hidden_odp, "slides[1].visibility.visibility_written"),
         dig(hidden_odp, "slides[0].visibility.visibility_written"),
         dig(hidden_odp, "slides[0].visibility.style_found")],
        [False, True, True, False, True, "dp1", "dp3", "hidden", None, True],
    )
    check(
        "藏起来那页照样在账上：两页都在，标题与字一条不少（dp2 那份没人点名的样式不替它说话）",
        [len(hidden_pptx.get("slides", [])), len(hidden_odp.get("slides", [])),
         dig(hidden_pptx, "slides[1].title"),
         dig(hidden_odp, "slides[1].title"),
         dig(hidden_odp, "slides[0].hidden")],
        [2, 2, "第二页：放映时藏起来", "第二页：放映时藏起来", False],
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

    # ── 3a6c) 格子里的链接：四种存法各交各的账（rels 一跳 / 元素上直接写地址 /
    #         段里的字挂地址 / 同一条流的记录），来路不同就不硬并成一个形状 ──
    print("=== 3a6c) office-sheet 的链接（四种存法） ===")
    LINK_KEYS = (
        "total", "external", "internal", "unresolved", "with_id",
        "with_location", "with_display", "with_tooltip", "formula_cells",
    )
    LINK_ROW = ("ref", "id", "target", "scheme", "external",
                "location", "display", "tooltip", "hop")
    for name in ("cell-links.xlsx", "cell-links-lo.xlsx"):
        got = lbin("office-sheet", fixture(name))
        theirs = files[name]["ooxml"]["links"]
        mine = {}
        for index, one in enumerate(got.get("sheets", []), start=1):
            ledger = one.get("links") or {}
            mine["sheet%d" % index] = (
                {key: ledger.get(key) for key in LINK_KEYS},
                [{key: had.get(key) for key in LINK_ROW} for had in ledger.get("list", [])],
            )
        check(
            "%s 每张表的链接账（逐条 + 九个数）" % name,
            mine,
            {
                key: (
                    {one: value[one] for one in LINK_KEYS},
                    [{one: had.get(one) for one in LINK_ROW} for had in value["list"]],
                )
                for key, value in theirs.items()
            },
        )
        check(
            "%s 链接总账两条（元素与 HYPERLINK 公式各一本）" % name,
            [dig(got, "workbook.totals.links"), dig(got, "workbook.totals.hyperlink_formulas")],
            [
                sum(value["total"] for value in theirs.values()),
                sum(value["formula_cells"] for value in theirs.values()),
            ],
        )
    odsbook = lbin("office-sheet", fixture("cell-links.ods"))
    ods_theirs = {
        one["name"]: one for one in files["cell-links.ods"]["ods"]["sheets"]
    }
    check(
        "cell-links.ods 每张表的链接逐条（地址挂在字上，没有第二跳）",
        {
            one.get("name"): [
                {key: had.get(key) for key in ("ref", "text", "target", "scheme",
                                               "external", "id", "hop", "via")}
                for had in (one.get("links") or {}).get("list", [])
            ]
            for one in odsbook.get("sheets", [])
        },
        {
            key: [
                {one: had.get(one) for one in ("ref", "text", "target", "scheme",
                                               "external", "id", "hop", "via")}
                for had in value["links"]
            ]
            for key, value in ods_theirs.items()
        },
    )
    check(
        "cell-links.ods 这一族把链接写在哪（text:a 对 table:hyperlink）",
        {
            one.get("name"): (
                (one.get("links") or {}).get("via_text_a"),
                (one.get("links") or {}).get("via_table_hyperlink"),
                (one.get("links") or {}).get("formula_cells"),
            )
            for one in odsbook.get("sheets", [])
        },
        {
            key: (
                sum(1 for had in value["links"] if had["via"] == "a"),
                sum(1 for had in value["links"] if had["via"] != "a"),
                value["hyperlink_formulas"],
            )
            for key, value in ods_theirs.items()
        },
    )
    check(
        "cell-links.ods 链接总账两条",
        [dig(odsbook, "workbook.totals.links"), dig(odsbook, "workbook.totals.hyperlink_formulas")],
        [
            sum(len(value["links"]) for value in ods_theirs.values()),
            sum(value["hyperlink_formulas"] for value in ods_theirs.values()),
        ],
    )
    # 第四种存法：0x01B8 记录里的两条分支，判据是第二个 GUID 在不在
    xlsbook = lbin("office-sheet", fixture("cell-links.xls"))
    xls_theirs = files["cell-links.xls"]["biff"]["links"]
    check(
        "cell-links.xls 每张表的链接逐条（两种地址 + 那两个自证数）",
        {
            one.get("name"): [
                (had.get("ref"), had.get("target"), had.get("location"),
                 had.get("external"), had.get("display"), had.get("whole"),
                 had.get("guid_first"), had.get("at24"), had.get("at28"))
                for had in (one.get("links") or {}).get("list", [])
            ]
            for one in xlsbook.get("sheets", [])
        },
        {
            key: [
                (had["ref"], had["target"], had["location"], had["guid_second"],
                 had["friendly"], had["whole"], had["guid_first"], had["at24"], had["at28"])
                for had in value["list"]
            ]
            for key, value in xls_theirs.items()
        },
    )
    check(
        "cell-links.xls 每条记录都切到自己报的长度",
        {
            one.get("name"): (
                (one.get("links") or {}).get("total"),
                (one.get("links") or {}).get("external"),
                (one.get("links") or {}).get("records"),
                (one.get("links") or {}).get("whole"),
            )
            for one in xlsbook.get("sheets", [])
        },
        {
            key: (len(value["list"]), value["external"], len(value["list"]), value["whole"])
            for key, value in xls_theirs.items()
        },
    )
    # 文件自己写的那个数照它交（实测 0，而流里有六条记录）：交 null 或改成 6 都是替文件说话
    check(
        "cell-links.xls 0x01B7 自报的那个数按写的交",
        dig(xlsbook, "workbook.links_written"),
        files["cell-links.xls"]["biff"]["link_counts"],
    )
    check(
        "cell-links.xls 这一族的 HYPERLINK 公式判不住就交 null",
        [(one.get("links") or {}).get("formula_cells") for one in xlsbook.get("sheets", [])],
        [None] * len(xlsbook.get("sheets", [])),
    )
    # 反面对照：没有链接的那几份件里键照样在、值为 0（缺键与零条是两件事）
    for name in ("book.xlsx", "hidden.xlsx"):
        check(
            "%s 没有链接就是 0 条" % name,
            dig(lbin("office-sheet", fixture(name)), "workbook.totals.links"),
            sum(value["total"] for value in files[name]["ooxml"]["links"].values()),
        )
    check(
        "book.xls 没有链接也是 0 条，而那条自报的记录照样在",
        [dig(lbin("office-sheet", fixture("book.xls")), "workbook.totals.links"),
         dig(lbin("office-sheet", fixture("book.xls")), "workbook.links_written")],
        [0, files["book.xls"]["biff"]["link_counts"]],
    )
    # 跨存法的同一条链接：站内地址三种写法各写各的，不替它们归一
    check(
        "同一格 C3 的站内地址：三种写法各交各的",
        {
            name: [
                had.get("location") or had.get("target")
                for one in lbin("office-sheet", fixture(name)).get("sheets", [])
                for had in (one.get("links") or {}).get("list", [])
                if had.get("ref") == "C3"
            ]
            for name in ("cell-links.xlsx", "cell-links.ods", "cell-links.xls")
        },
        {
            "cell-links.xlsx": ["'数据'!A1"],
            "cell-links.ods": ["#'数据'.A1"],
            "cell-links.xls": ["'数据'!A1"],
        },
    )

    # ── 3a6d) 表上那张位图：OOXML 跳三跳、ODF 只有一跳、XLS 的字节根本不在表子流里。
    #         两个生产者把同样四张图写成两套明细（openpyxl 用三种锚块元素名分表示
    #         跨格/单格/绝对，LibreOffice 一律写 twoCellAnchor 而把区别放进 editAs），
    #         所以整份明细一起比：只看总数什么都看不见 ──
    print("=== 3a6d) office-sheet 的位图（四种包形状，两个生产者） ===")
    PIC_KEYS = (
        "drawings", "total", "distinct_media", "unresolved", "missing_media",
        "other_anchors", "listed", "cut",
    )
    XDR_KEYS = (
        "drawing", "placed", "anchor_attrs", "from", "to", "pos", "ext",
        "shape_id", "shape_name", "descr", "shape_attrs", "blip_attrs", "blip_id",
        "external", "target", "media_bytes", "xfrm",
    )
    ODS_KEYS = (
        "in_cell", "written", "name", "x", "y", "width", "height",
        "end_cell_address", "href", "also_object", "mime", "image_written",
        "alt", "media_bytes",
    )
    BLIP_KEYS = ("offset", "instance", "cb", "magic_at", "kind", "inline_bytes")

    def pic_counts(ledger):
        return {one: ledger.get(one) for one in PIC_KEYS}

    def pic_view(ledger, row_keys):
        return (
            pic_counts(ledger),
            [{one: had.get(one) for one in row_keys} for had in ledger.get("list", [])],
        )

    for name in ("sheet-pictures.xlsx", "sheet-pictures-lo.xlsx"):
        got = lbin("office-sheet", fixture(name))
        theirs = files[name]["ooxml"]["pictures"]
        check(
            "%s 每张表的位图整份账（八个数 + 每个锚块的明细）" % name,
            {
                "sheet%d" % index: pic_view(one.get("pictures") or {}, XDR_KEYS)
                for index, one in enumerate(got.get("sheets", []), start=1)
            },
            {
                key: pic_view(value, XDR_KEYS)
                for key, value in theirs.items()
            },
        )
        check(
            "%s 位图总账（四张表加起来的条数）" % name,
            dig(got, "workbook.totals.pictures"),
            sum(value["total"] for value in theirs.values()),
        )
    openpyxl = lbin("office-sheet", fixture("sheet-pictures.xlsx"))
    losheet = lbin("office-sheet", fixture("sheet-pictures-lo.xlsx"))
    # 摆法这个名字在两族手里完全不是一套：元素名与属性名各说各的
    check(
        "openpyxl 用三种锚块元素名，一个 anchor 属性都不写",
        [
            [had.get("placed") for had in (dig(openpyxl, "sheets[0].pictures.list") or [])],
            [had.get("anchor_attrs") for had in (dig(openpyxl, "sheets[0].pictures.list") or [])],
        ],
        [
            ["twoCellAnchor", "oneCellAnchor", "oneCellAnchor", "oneCellAnchor",
             "absoluteAnchor"],
            [{}, {}, {}, {}, {}],
        ],
    )
    check(
        "LibreOffice 一律写 twoCellAnchor，区别全在 editAs 那四个值里",
        [
            [had.get("placed") for had in (dig(losheet, "sheets[0].pictures.list") or [])],
            [had.get("anchor_attrs") for had in (dig(losheet, "sheets[0].pictures.list") or [])],
        ],
        [
            ["twoCellAnchor"] * 4,
            [{"editAs": "twoCell"}, {"editAs": "oneCell"}, {"editAs": "oneCell"},
             {"editAs": "absolute"}],
        ],
    )
    check(
        "同四张图的两种尺寸写法：一家在锚块的 ext，一家在 spPr/xfrm 的 off+ext",
        [
            dig(openpyxl, "sheets[0].pictures.list[1].ext"),
            dig(openpyxl, "sheets[0].pictures.list[1].xfrm"),
            dig(losheet, "sheets[0].pictures.list[1].ext"),
            dig(losheet, "sheets[0].pictures.list[1].xfrm"),
        ],
        [
            {"cx": "381000", "cy": "228600"}, None,
            None,
            {"attrs": {}, "off": {"x": "601920", "y": "190440"},
             "ext": {"cx": "380520", "cy": "228240"}},
        ],
    )
    # 单位都写 EMU，但两家的数就不相等：跨格右下角那一个偏移
    check(
        "同一条跨格图的两个角：EMU 数不等，两家各按自己那次换算交",
        [
            dig(openpyxl, "sheets[0].pictures.list[0].to"),
            dig(losheet, "sheets[0].pictures.list[0].to"),
            dig(losheet, "sheets[0].pictures.list[0].xfrm.off"),
        ],
        [
            {"col": 8.0, "col_off": 95250.0, "row": 8.0, "row_off": 66675.0},
            {"col": 8.0, "col_off": 95040.0, "row": 8.0, "row_off": 66240.0},
            {"x": "3009960", "y": "952560"},
        ],
    )
    check(
        "absoluteAnchor 那一支：两个角都不在，pos 与 ext 各一对坐标",
        [
            dig(openpyxl, "sheets[0].pictures.list[4].from"),
            dig(openpyxl, "sheets[0].pictures.list[4].to"),
            dig(openpyxl, "sheets[0].pictures.list[4].pos"),
            dig(openpyxl, "sheets[0].pictures.list[4].ext"),
        ],
        [None, None, {"x": "2857500", "y": "1905000"}, {"cx": "571500", "cy": "285750"}],
    )
    # 关系被删掉而锚块还在：那一条既算一张图，又跳不到任何字节
    check(
        "rId5 那条关系被删掉了：锚块还在，target 交 null 而不编一个",
        [
            dig(openpyxl, "sheets[0].pictures.list[3].blip_id"),
            dig(openpyxl, "sheets[0].pictures.list[3].target"),
            dig(openpyxl, "sheets[0].pictures.list[3].media_bytes"),
            dig(openpyxl, "sheets[0].pictures.unresolved"),
            dig(openpyxl, "sheets[0].pictures.missing_media"),
        ],
        ["rId5", None, None, 1, 0],
    )
    check(
        "blip 的属性两家不同：openpyxl 写了 cstate=print，LibreOffice 没有那一样",
        [dig(openpyxl, "sheets[0].pictures.list[0].blip_attrs"),
         dig(losheet, "sheets[0].pictures.list[0].blip_attrs")],
        [{"cstate": "print", "embed": "rId3"}, {"embed": "rId1"}],
    )
    check(
        "形状名与替代文字：改过的那一条两家都留着，其余是生产者自己编的 Image N",
        [
            [had.get("shape_name") for had in (dig(openpyxl, "sheets[0].pictures.list") or [])],
            [had.get("descr") for had in (dig(openpyxl, "sheets[0].pictures.list") or [])],
            [had.get("shape_name") for had in (dig(losheet, "sheets[0].pictures.list") or [])],
        ],
        [
            ["Image 3", "Image 1", "第二张", "Image 5", "Image 4"],
            ["Picture", "Picture", "一个蓝点", "Picture", "Picture"],
            ["Image 3", "Image 1", "第二张", "Image 4"],
        ],
    )
    # 同一张位图被两个锚块引用：去重数小于条数，这一本是分得开的
    check(
        "去重按这一条自己写的地址：LibreOffice 那份 image1.png 被引用两次",
        [
            dig(losheet, "sheets[0].pictures.total"),
            dig(losheet, "sheets[0].pictures.distinct_media"),
            [had.get("target") for had in (dig(losheet, "sheets[0].pictures.list") or [])][:2],
            dig(openpyxl, "sheets[0].pictures.total"),
            dig(openpyxl, "sheets[0].pictures.distinct_media"),
        ],
        [4, 3, ["xl/media/image1.png", "xl/media/image1.png"], 5, 4],
    )
    check(
        "每张表有自己的画法部件：drawing2/drawing3 各挂一张，第四张表一个都没挂",
        [
            [dig(one, "pictures.drawings") for one in openpyxl.get("sheets", [])],
            [had.get("drawing") for one in openpyxl.get("sheets", [])
             for had in (one.get("pictures") or {}).get("list", []) if had.get("drawing") != "xl/drawings/drawing1.xml"],
            [dig(one, "pictures.drawings") for one in losheet.get("sheets", [])],
        ],
        [
            [1, 1, 1, 0],
            ["xl/drawings/drawing2.xml", "xl/drawings/drawing3.xml"],
            [1, 1, 1, 0],
        ],
    )
    # 画法部件里那些不是位图的摆位（图表走 graphicFrame）：另记一本，不算坏图
    for name in ("chart.xlsx", "chart-lo.xlsx"):
        theirs = files[name]["ooxml"]["pictures"]
        check(
            "%s 两张图挂在表上而一张位图也没有：other_anchors 记着那两个锚块" % name,
            {
                "sheet%d" % index: pic_counts(one.get("pictures") or {})
                for index, one in enumerate(lbin("office-sheet", fixture(name)).get("sheets", []), start=1)
            },
            {key: pic_counts(value) for key, value in theirs.items()},
        )
    check(
        "chart.xlsx 那一份的 other_anchors 是 2 而 unresolved 是 0（不是两张坏图）",
        [dig(lbin("office-sheet", fixture("chart.xlsx")), "sheets[0].pictures.other_anchors"),
         dig(lbin("office-sheet", fixture("chart.xlsx")), "sheets[0].pictures.unresolved"),
         dig(lbin("office-sheet", fixture("chart.xlsx")), "sheets[0].pictures.total"),
         dig(lbin("office-sheet", fixture("chart.xlsx")), "sheets[0].pictures.drawings")],
        [2, 0, 0, 1],
    )
    # ODF 那一族：一跳、没有 drawings 那一层、尺寸是自带单位的串
    odsbook = lbin("office-sheet", fixture("sheet-pictures.ods"))
    ods_theirs = {one["name"]: one["pictures"] for one in files["sheet-pictures.ods"]["ods"]["sheets"]}
    check(
        "sheet-pictures.ods 每张表的位图整份账（八个数 + 每个 frame 的明细）",
        {
            one.get("name"): pic_view(one.get("pictures") or {}, ODS_KEYS)
            for one in odsbook.get("sheets", [])
        },
        {key: pic_view(value, ODS_KEYS) for key, value in ods_theirs.items()},
    )
    check(
        "sheet-pictures.ods 位图总账 + drawings 这一族整个交 null",
        [dig(odsbook, "workbook.totals.pictures"),
         {one.get("name"): dig(one, "pictures.drawings") for one in odsbook.get("sheets", [])}],
        [7, {key: None for key in ods_theirs}],
    )
    check(
        "「坐在格子里」是结构事实：四条里只有一条不在，而它带着 svg:x/svg:y",
        [
            [had.get("in_cell") for had in (dig(odsbook, "sheets[0].pictures.list") or [])],
            [had.get("x") for had in (dig(odsbook, "sheets[0].pictures.list") or [])],
            [had.get("end_cell_address") for had in (dig(odsbook, "sheets[0].pictures.list") or [])],
        ],
        [
            [False, True, True, True, True],
            ["7.938cm", "0cm", "0cm", "0cm", "0cm"],
            [None, None, None, "图与格.I9", None],
        ],
    )
    check(
        "ODF 那一族的「配不上号」是压根没写 href：frame 与空的 draw:image 都还在",
        [
            dig(odsbook, "sheets[0].pictures.list[4].name"),
            dig(odsbook, "sheets[0].pictures.list[4].href"),
            dig(odsbook, "sheets[0].pictures.list[4].mime"),
            dig(odsbook, "sheets[0].pictures.list[4].image_written"),
            dig(odsbook, "sheets[0].pictures.unresolved"),
            dig(odsbook, "sheets[0].pictures.distinct_media"),
        ],
        ["Image 5", None, None, {}, 1, 3],
    )
    # 隐藏的表照样有自己的那份账：字节在包里，看不见不等于没有
    check(
        "隐藏的表那张图照数（藏着 1 条、只有字 0 条）",
        {
            one.get("name"): [dig(one, "pictures.total"), dig(one, "pictures.listed"),
                              dig(one, "pictures.drawings")]
            for one in odsbook.get("sheets", [])
        },
        {
            "图与格": [5, 5, None], "另一张": [1, 1, None],
            "藏着": [1, 1, None], "只有字": [0, 0, None],
        },
    )
    check(
        "chart.ods 那两张是图表的预览缓存位图：also_object 全为 true，另有一份嵌入对象账",
        [
            {one.get("name"): pic_counts(one.get("pictures") or {})
             for one in lbin("office-sheet", fixture("chart.ods")).get("sheets", [])},
            [had.get("also_object") for had in
             (dig(lbin("office-sheet", fixture("chart.ods")), "sheets[0].pictures.list") or [])],
            [had.get("href") for had in
             (dig(lbin("office-sheet", fixture("chart.ods")), "sheets[0].pictures.list") or [])],
        ],
        [
            {key: pic_counts(value)
             for key, value in {one["name"]: one["pictures"]
                                for one in files["chart.ods"]["ods"]["sheets"]}.items()},
            [True, True],
            ["ObjectReplacements/Object 1", "ObjectReplacements/Object 2"],
        ],
    )
    # 第四族：位图字节只住在 global 流的那一条 OfficeArt 记录里，表子流一个字节都没有
    xlsbook = lbin("office-sheet", fixture("sheet-pictures.xls"))
    xls_theirs = files["sheet-pictures.xls"]["biff"]
    xls_blips = [
        {one: had.get(one) for one in BLIP_KEYS}
        for had in (dig(xlsbook, "workbook.pictures.blips") or [])
    ]
    check(
        "sheet-pictures.xls 三条 0x00EB 的字节整份账（偏移、instance、cb、字签位置都照文件交）",
        xls_blips,
        [{one: had.get(one) for one in BLIP_KEYS}
         for had in xls_theirs["pictures"]["blips"]],
    )
    check(
        # 那三个 instance 与「字签到正文末尾」的字节数是生产者自己写的数，逐条钉住
        "sheet-pictures.xls 那三个 instance（6/6/5）与三条 inline_bytes（117/110/661）",
        xls_blips,
        [
            {"offset": 1130, "instance": 6, "cb": 178, "magic_at": 61, "kind": "png",
             "inline_bytes": 117},
            {"offset": 1316, "instance": 6, "cb": 171, "magic_at": 61, "kind": "png",
             "inline_bytes": 110},
            {"offset": 1495, "instance": 5, "cb": 722, "magic_at": 61, "kind": "jpeg",
             "inline_bytes": 661},
        ],
    )
    check(
        "sheet-pictures.xls 那两个自证数：每条画法记录与图形状各几笔",
        [
            {one.get("name"): [dig(one, "pictures.shapes"), dig(one, "pictures.drawing_records")]
             for one in xlsbook.get("sheets", [])},
            [dig(xlsbook, "workbook.pictures.drawing_groups[0]")],
            [dig(xlsbook, "workbook.totals.picture_shapes"), dig(xlsbook, "workbook.totals.drawing_records")],
        ],
        [
            {one["name"]: [one["pictures"]["shapes"], one["pictures"]["drawing_records"]]
             for one in xls_theirs["sheets"]},
            [xls_theirs["pictures"]["drawing_groups"][0]],
            [7, 8],
        ],
    )
    # 跨存法的自证：同一批源图片在四族里的字节数一样，这一本不是自说自话
    check(
        "同一批源图片在四族里的字节：117 / 110 / 661 三个数对得上",
        [
            {had.get("media_bytes") for one in openpyxl.get("sheets", [])
             for had in (one.get("pictures") or {}).get("list", [])},
            {had.get("media_bytes") for one in odsbook.get("sheets", [])
             for had in (one.get("pictures") or {}).get("list", [])},
            {one.get("inline_bytes") for one in dig(xlsbook, "workbook.pictures.blips") or []},
        ],
        [{117, 110, 661, None}] * 2 + [{117, 110, 661}],
    )
    # 反面对照：没有图的件里键照样在、八个数全交（缺键与零条是两件事）
    for name in ("book.xlsx", "hidden.xlsx", "book.ods", "print-area.ods"):
        got = lbin("office-sheet", fixture(name))
        theirs = (
            files[name]["ooxml"]["pictures"] if name.endswith(".xlsx")
            else {one["name"]: one["pictures"] for one in files[name]["ods"]["sheets"]}
        )
        mine = {
            ("sheet%d" % (index + 1)) if name.endswith(".xlsx") else one.get("name"):
            pic_counts(one.get("pictures") or {})
            for index, one in enumerate(got.get("sheets", []))
        }
        check(
            "%s 没有位图就是 0 条（drawings 那一族各交各的：OOXML 数出 0，ODF 没有那一层）" % name,
            mine,
            {key: pic_counts(value) for key, value in theirs.items()},
        )
    check(
        "book.xls 一条 blip 也没有，而那条画法记录照样数得到",
        [dig(lbin("office-sheet", fixture("book.xls")), "workbook.pictures.total"),
         dig(lbin("office-sheet", fixture("book.xls")), "workbook.totals.picture_shapes"),
         dig(lbin("office-sheet", fixture("book.xls")), "workbook.totals.drawing_records")],
        [0, 0, 3],
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
    FRAME_KEYS = ("legend_found", "legend_pos", "legend_overlay", "legend_delete",
                  "auto_title_deleted", "disp_blanks_as")

    def frames_of(ledger):
        """一份包里所有图的 chartSpace 层摊成一列（表与页两种容器都走这一条）"""
        out = []
        rows = ledger.get("sheets")
        if rows is None:
            rows = ledger.get("slides")
        for one in rows or []:
            for chart in one.get("chart_list") or []:
                if not chart.get("present"):
                    continue
                had = chart["chart_space"]
                out.append([chart["part"].rsplit("/", 1)[-1][: -len(".xml")],
                            [had[key] for key in FRAME_KEYS]])
        return out

    sheet_frames = [frames_of(hand), frames_of(lo),
                    frames_of(lbin("office-slide", fixture("deck-chart.pptx"))),
                    frames_of(lbin("office-slide", fixture("deck-chart-lo.pptx")))]
    check("chartSpace 那一层四件对照：两份 xlsx 都写图例（`r`）而不写 `delete`，LibreOffice 那一份多写一枚 "
          "`overlay=\"0\"` 与 `autoTitleDeleted=\"0\"`；两份 pptx 里**图例整个不见**（导出时丢了），"
          "而 LibreOffice 那份 pptx 同一页的两张图 `autoTitleDeleted` 一张 `1` 一张 `0` —— 同一次重写里逐图不同，"
          "所以这一格不能按「这份稿子删过标题没有」折成一个布尔",
          sheet_frames,
          [[["chart1", [True, "r", None, None, None, "gap"]],
            ["chart2", [True, "r", None, None, None, "gap"]]],
           [["chart1", [True, "r", "0", None, "0", "gap"]],
            ["chart2", [True, "r", "0", None, "0", "gap"]]],
           [["chart1", [False, None, None, None, "0", "gap"]],
            ["chart2", [False, None, None, None, "0", "gap"]]],
           [["chart1", [False, None, None, None, "1", "gap"]],
            ["chart2", [False, None, None, None, "0", "gap"]]]],
    )
    check("图例那一格四件里两份写两份不写：写图例的只有 xlsx 那两家（各 2 张），"
          "pptx 那两家四张全不写；`dispBlanksAs` 八张全写 `gap`（本库没有一件写 zero 或 span）",
          [[sum(1 for row in ones for one in row if one[1][0]),
            sum(1 for row in ones for one in row if one[1][2] is not None),
            sum(1 for row in ones for one in row if one[1][4] is not None),
            sorted({one[1][5] for row in ones for one in row})]
           for ones in ([sheet_frames[0], sheet_frames[1]], [sheet_frames[2], sheet_frames[3]])],
          [[4, 2, 2, ["gap"]], [0, 0, 4, ["gap"]]],
    )
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
    for name in ("book.xls", "hidden.xls"):
        missed = [one.get("name") for one in lbin("office-sheet", fixture(name)).get("sheets", [])
                  if one.get("layout") is not None or one.get("filter") is not None
                  or one.get("tables") is not None]
        check("%s 这一族的尺寸、筛选与表对象没读：键整个不在" % name, missed, [])
    # ODS 的尺寸这一族读到了（见 3a14），筛选与表对象则是 OOXML 才有的东西
    for name in ("book.ods", "hidden.ods", "chart.ods"):
        missed = [one.get("name") for one in lbin("office-sheet", fixture(name)).get("sheets", [])
                  if one.get("filter") is not None or one.get("tables") is not None]
        check("%s 这一族的筛选与表对象没读：键整个不在" % name, missed, [])

    # ── 3a14) ODS 的列宽与行高：尺寸不在元素上，一跳在它点名的自动样式里 ──
    print("=== 3a14) office-sheet 的 ODS 尺寸（16384 那一本账） ===")
    for name in sorted(one.name for one in FIXTURES.glob("*.ods")):
        got = lbin("office-sheet", fixture(name))
        want = files[name]["ods"]
        check(
            "%s 每张表的列宽账与读者一致" % name,
            {one.get("name"): (one.get("layout") or {}).get("columns")
             for one in got.get("sheets", [])},
            {one["name"]: one["layout"]["columns"] for one in want["sheets"]},
        )
        check(
            "%s 每张表的行高账与读者一致" % name,
            {one.get("name"): (one.get("layout") or {}).get("rows")
             for one in got.get("sheets", [])},
            {one["name"]: one["layout"]["rows"] for one in want["sheets"]},
        )
        check(
            "%s 表元素自己写的那四个数与读者一致" % name,
            {one.get("name"): (one.get("layout") or {}).get("stated")
             for one in got.get("sheets", [])},
            {one["name"]: one["layout"]["stated"] for one in want["sheets"]},
        )
    # 「几条元素」「盖住几列」「有内容的最右一列」是三本账：LibreOffice 每张表都补到 16384 列
    booked = lbin("office-sheet", fixture("book.ods"))
    check(
        "一张表 2 条列元素、盖住 16384 列、有内容的最右一格在第 2 列",
        [dig(booked, "sheets[0].layout.columns.elements"),
         dig(booked, "sheets[0].layout.columns.spans"), dig(booked, "sheets[0].columns")],
        [2, 16384, 2],
    )
    check(
        "九份 .ods 里 17 张表补到 16384 列，只有一张例外：`print-area.ods` 的 `什么都没给` "
        "（只给了重复**列**的那一张）写了 3 条列元素，补齐就到 **16383** —— 「补到整 16384」"
        "不是这一族的恒等式，是生产者的写法，所以两个数都按看到的交",
        sorted(
            {dig(one, "layout.columns.spans")
             for name in sorted(item.name for item in FIXTURES.glob("*.ods"))
             for one in lbin("office-sheet", fixture(name)).get("sheets", [])}
        ),
        [16383, 16384],
    )
    check(
        "一条列元素顶几列写在 repeated 里：2 + 16382，两个数各自交",
        [dig(booked, "sheets[0].layout.columns.list[0].repeated"),
         dig(booked, "sheets[0].layout.columns.list[1].repeated"),
         dig(booked, "sheets[0].layout.columns.list[0].style"),
         dig(booked, "sheets[0].layout.columns.list[0].size"),
         dig(booked, "sheets[0].layout.columns.list[0].size_mm")],
        [2, 16382, "co1", "1.672cm", 1672],
    )
    # 行也一样：一份件里 5 条行元素可以盖住 20 行
    charted = lbin("office-sheet", fixture("chart.ods"))
    check(
        "行也一样：5 条行元素盖住 20 行（其中一条 repeated=16）",
        [dig(charted, "sheets[0].layout.rows.elements"),
         dig(charted, "sheets[0].layout.rows.spans"),
         dig(charted, "sheets[0].layout.rows.list[3].repeated"),
         dig(charted, "sheets[0].layout.rows.list[3].size"),
         dig(charted, "sheets[0].layout.rows.list[3].size_mm")],
        [5, 20, 16, "0.529cm", 529],
    )
    check(
        "高度与「按最优」是同时写的两句，宽度这一族却没有 use-optimal-column-width",
        [dig(booked, "sheets[0].layout.rows.optimal"),
         dig(booked, "sheets[0].layout.rows.with_size"),
         dig(booked, "sheets[0].layout.rows.list[0].optimal"),
         dig(booked, "sheets[0].layout.columns.optimal"),
         dig(booked, "sheets[0].layout.columns.list[0].optimal")],
        [5, 5, "true", 0, None],
    )
    hidden = lbin("office-sheet", fixture("hidden.ods"))
    check(
        "隐藏写在元素自己身上（不是样式里）：那一条顶 3 列，样式叫 co2",
        [dig(hidden, "sheets[0].layout.columns.elements"),
         dig(hidden, "sheets[0].layout.columns.spoken_visibility"),
         dig(hidden, "sheets[0].layout.columns.list[1].element_visibility"),
         dig(hidden, "sheets[0].layout.columns.list[1].style"),
         dig(hidden, "sheets[0].layout.columns.list[1].repeated"),
         dig(hidden, "sheets[0].layout.columns.list[1].size_mm")],
        [3, 1, "collapse", "co2", 3, 2545],
    )
    check(
        "两条来路分开交：元素上的 visibility 数出来的与样式里那条（这批件里样式都没写）",
        [dig(hidden, "sheets[0].layout.rows.spoken_visibility"),
         dig(hidden, "sheets[0].layout.rows.list[2].element_visibility"),
         dig(hidden, "sheets[0].layout.rows.list[2].style_visibility"),
         dig(hidden, "sheets[0].layout.columns.list[1].style_visibility")],
        [2, "collapse", None, None],
    )
    check(
        "点了名又找着了样式才算报得出尺寸：这批件里每一条都 resolved，所以 with_size 与 elements 相等",
        [dig(booked, "sheets[0].layout.columns.resolved"),
         dig(booked, "sheets[0].layout.columns.with_size"),
         dig(booked, "sheets[0].layout.columns.list[0].resolved"),
         dig(booked, "sheets[0].layout.columns.list[0].style_parent")],
        [2, 2, True, None],
    )
    check(
        "那四个表级自报的数 LibreOffice 一个都不写：交回四个 null，而不是四个 0",
        [dig(booked, "sheets[0].layout.stated.number-columns"),
         dig(booked, "sheets[0].layout.stated.number-rows"),
         dig(booked, "sheets[0].layout.stated.default-column-width"),
         dig(booked, "sheets[0].layout.stated.default-row-height")],
        [None, None, None, None],
    )
    check(
        "单位说一次：0.01mm（1672 就是 1.672cm）",
        [dig(booked, "sheets[0].layout.unit"), dig(booked, "sheets[0].layout.columns.listed"),
         dig(booked, "sheets[0].layout.columns.shown"),
         len(dig(booked, "sheets[0].layout.columns.list") or [])],
        ["0.01mm", 2, 2, 2],
    )

    # ── 3b) 数字格式：格子写的是 cellXfs 的下标，日期藏在样式里 ──────────
    print("=== 3b) formats.xlsx 与 epoch 那两件：格式号、判定与换算出来的日期 ===")
    for what in ("formats.xlsx", "epoch.xlsx", "epoch-lo.xlsx"):
        fx = lbin("office-sheet", fixture(what))
        fw = files[what]["formats"]
        flat = {}
        for one_sheet in fx.get("sheets", []):
            for cell in one_sheet.get("cell_list", []):
                flat["%s!%s" % (one_sheet.get("name"), cell.get("ref"))] = cell
        want = {("%s!%s" % (one["sheet"], one["ref"])): one for one in fw["cells"]}
        check("%s 格子数" % what, len(flat), len(want))
        check("%s 1904 基准" % what, dig(fx, "workbook.date1904"), fw["date1904"])
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
            record("%s %s 判成 %s" % (what, key, theirs["format_kind"]), mine == theirs,
                   json.dumps({"got": mine, "want": theirs}, ensure_ascii=False)[:130])
    epoch_one = lbin("office-sheet", fixture("epoch.xlsx"))
    epoch_two = lbin("office-sheet", fixture("epoch-lo.xlsx"))
    check(
        "同一批序列数在 1904 基准下是另一套日子（两家写 date1904 的拼法不同而数相同）",
        [dig(epoch_one, "workbook.date1904"), dig(epoch_two, "workbook.date1904"),
         dig(epoch_one, "sheets[0].cell_list[0].as_date"),
         dig(epoch_two, "sheets[0].cell_list[0].as_date"),
         dig(epoch_one, "sheets[0].cell_list[2].as_date"),
         dig(epoch_two, "sheets[0].cell_list[2].as_date")],
        [True, True, "2013-12-23", "2013-12-23",
         "2020-01-02T03:04:05", "2020-01-02T03:04:05"],
    )
    check(
        "序列号 60 的那个闰年 bug 只属于 1900 基准：1904 这里它是 1904-03-01",
        [dig(epoch_one, "sheets[0].cell_list[3].value"),
         dig(epoch_one, "sheets[0].cell_list[3].as_date"),
         dig(epoch_two, "sheets[0].cell_list[3].as_date")],
        [60, "1904-03-01", "1904-03-01"],
    )
    check(
        "长得像日期的那一格本来就是字：两种存法（inlineStr 与共享字符串）都不许换算成日子",
        [dig(epoch_one, "sheets[0].cell_list[4].kind"),
         dig(epoch_one, "sheets[0].cell_list[4].value"),
         dig(epoch_one, "sheets[0].cell_list[4].as_date"),
         dig(epoch_two, "sheets[0].cell_list[4].kind"),
         dig(epoch_two, "sheets[0].cell_list[4].value"),
         dig(epoch_two, "sheets[0].cell_list[4].as_date")],
        ["inlineStr", "12/23/2013", None, "s", "12/23/2013", None],
    )

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

    # ── 3b3) office-text --markdown 的第五族：RTF（整本账与读者对，pinned 那几条从 15 份真件量的）──
    print("=== 3b3) RTF → markdown：第五族整本对账 ===")
    for name in sorted(one.name for one in FIXTURES.glob("*.rtf")):
        got = lbin("office-text", fixture(name), "--markdown")
        check("%s 的 markdown 整本与读者一致" % name, got.get("markdown"), files[name]["rtf_markdown"])
        record(
            "%s 不开 --markdown 就不交那一格" % name,
            lbin("office-text", fixture(name)).get("markdown") is None,
            json.dumps(got.get("markdown"), ensure_ascii=False)[:60],
        )
    # 三条从真件量出来的规矩，各钉一处（不是规范推的）
    lists = lbin("office-text", fixture("lists.rtf"), "--markdown").get("markdown") or {}
    check(
        "lists.rtf：段里自带文件写的那枚标签，摘掉之后才加 markdown 的记号",
        [lists.get("list_items"), lists.get("bullet_items"),
         lists.get("ordered_items"), lists.get("labels_stripped")],
        [5, 2, 3, 5],
    )
    check(
        "lists.rtf：正文里没有一枚残留的标签或制表记号",
        [lists.get("tabs"), "\uf0b7" in (lists.get("text") or ""), "1.\t" in (lists.get("text") or "")],
        [5, False, False],
    )
    tabs_ledger = lbin("office-text", fixture("tables.rtf"), "--markdown").get("markdown") or {}
    check(
        "tables.rtf：表在这一族是一整段带制表记号的字，所以 tables 交 null 而不是 0",
        [tabs_ledger.get("tables"), tabs_ledger.get("tabs"), tabs_ledger.get("empty_dropped")],
        [None, 5, 0],
    )
    toc = lbin("office-text", fixture("toc.rtf"), "--markdown").get("markdown") or {}
    check(
        "toc.rtf：目录那几条缓存条目按正文排（这一族的 entries 还没读，见事实 119 最后一条）",
        [toc.get("paragraphs"), toc.get("headings"), toc.get("list_items")],
        [9, 2, 0],
    )
    check(
        "toc.rtf：同一段字既在目录条目里也在正文标题里，两行都在、不折成一行",
        [(toc.get("text") or "").count("一级标题：预算口径"),
         (toc.get("text") or "").count("# 一级标题：预算口径")],
        [2, 1],
    )

    # ── 3f) --csv：把一张表铺平成 RFC4180（期望文本逐字来自 csv_facts/biff_csv）──
    print("=== 3f) office-sheet --csv：铺平一张表 ===")
    for name in (
        "book.xlsx",
        "formats.xlsx",
        "book.xls",
        "book.ods",
        "formats.ods",
        # 「结果不是数」那三副：除零的字、拼出来的字、没重算的空
        "errors.xlsx",
        "errors-lo.xlsx",
        "errors.ods",
        # 1904 基准那两件：同一批序列数换一套基准就是另一套日子
        "epoch.xlsx",
        "epoch-lo.xlsx",
        # 分段写的字与首尾那两个空格：三家三种摆法，铺平之后都得还在原处
        "rich.xlsx",
        "rich-lo.xlsx",
        "rich.ods",
    ):
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

    # ── 3f1) --markdown：同一张方格的第二个出口（全文期望逐字来自 md_render）──
    print("=== 3f1) office-sheet --markdown：一张方格两个出口 ===")
    for name in (
        "book.xlsx",
        "book.xls",
        "book.ods",
        "formats.xlsx",
        "formats.ods",
        "errors.xlsx",
        "errors.ods",
        "epoch.xlsx",
        "rich.xlsx",
        "rich-lo.xlsx",
        "rich.ods",
        # 竖线那一支：三家生产者各一副（库里原本没有一格带竖线，这副是为此做的）
        "pipes.xlsx",
        "pipes-lo.xlsx",
        "pipes.ods",
    ):
        led = files[name]["csv"]["sheets"]
        want = {one["name"]: one["markdown"] for one in led}
        shape = {one["name"]: [one["rows"], one["columns"]] for one in led}
        plain = lbin("office-sheet", fixture(name))
        record(
            "%s 不开 --markdown 就不交那份账" % name,
            plain.get("markdown") is None,
            json.dumps(plain.get("markdown"), ensure_ascii=False)[:80],
        )
        both = lbin("office-sheet", fixture(name), "--csv", "--markdown")
        order = [one["name"] for one in both.get("sheets", [])]
        check("%s --markdown 的表序与命令报的一致" % name, order, list(want))
        check("%s --markdown 默认第一张" % name, dig(both, "markdown.text"), want.get(order[0]))
        md_text = dig(both, "markdown.text") or ""
        record(
            "%s --markdown 的正文里没有裸 CR（行尾按 §2.11 折过）" % name,
            "\r" not in md_text,
            repr(md_text)[:70],
        )
        for at, one in enumerate(both.get("sheets", [])):
            nm = one["name"]
            got = lbin("office-sheet", fixture(name), "--markdown", "--sheet", nm, "--csv")
            check("%s --markdown %s 全文" % (name, nm), dig(got, "markdown.text"), want.get(nm))
            check("%s --markdown %s 的序号" % (name, nm), dig(got, "markdown.index"), at)
            # 两个出口读的是同一张方格：那两个数不许各算一遍
            check(
                "%s --markdown %s 的行列与 --csv 同源" % (name, nm),
                [dig(got, "markdown.rows"), dig(got, "markdown.columns")],
                [dig(got, "csv.rows"), dig(got, "csv.columns")],
            )
            # 而镜像也算了一遍行列（一条是格子坐标、一条是解出来的表）
            check("%s --markdown %s 的行列对得上镜像" % (name, nm),
                  [dig(got, "markdown.rows"), dig(got, "markdown.columns")], shape.get(nm))
            rows = shape.get(nm, [0])[0]
            check(
                "%s --markdown %s 那行分隔线在第 0 行之后" % (name, nm),
                dig(got, "markdown.separator_after_row"),
                0 if rows else None,
            )
        bad = lbin("office-sheet", fixture(name), "--markdown", "--sheet", "没这张表")
        check(
            "%s --markdown 认错表名就把候选说清楚" % name,
            isinstance(dig(bad, "markdown.error"), str),
            True,
        )

    # 竖线与格内换行：markdown 的表格装不下原样，躲法是钉死的（全文上面已与镜像逐字比过）
    for name in ("pipes.xlsx", "pipes-lo.xlsx", "pipes.ods"):
        text = dig(lbin("office-sheet", fixture(name), "--markdown"), "markdown.text") or ""
        record("%s 的竖线躲成反斜杠竖线" % name, "a\\|b" in text, text[:70])
        record("%s 的格内换行铺成 <br>" % name, "第一行<br>带" in text, text[:70])
        line = next((one for one in text.split("\n") if "a\\|b" in one), "")
        # 把躲过的竖线按占位符收回来再分列：那一行仍然只有两格，说明 `\|` 没被当成分列符
        marked = line.replace("\\|", "@@")
        cells = [one.strip().replace("@@", "|") for one in marked.split("|")][1:-1]
        check("%s 躲过的竖线不再被当分列符" % name, cells, ["a|b", "1|2|3"])

    # ── 3f2) 结果不是数的那几种：文件写着什么就交什么，一家没重算就什么都没有 ──
    print("=== 3f2) 错误格、文本结果与「没重算」 ===")
    for name in ("errors.xlsx", "errors-lo.xlsx"):
        got = lbin("office-sheet", fixture(name))
        want = files[name]["ooxml"]
        check(
            "%s 那三本新账与读者一致（几个格算错、几个格是字、几个格写了 t）" % name,
            [dig(got, "workbook.totals.error_cells"),
             dig(got, "workbook.totals.string_result_cells"),
             dig(got, "workbook.totals.cells_with_written_type"),
             dig(got, "workbook.totals.cells")],
            [want["error_cells"], want["string_result_cells"],
             want["cells_with_written_type"], want["cells"]],
        )
    errs = lbin("office-sheet", fixture("errors-lo.xlsx"))
    check(
        "错误格交文件自己写的那一串：#DIV/0! 而不是读者替它加的一句「#错误」",
        [dig(errs, "sheets[0].cell_list[1].kind"), dig(errs, "sheets[0].cell_list[1].value"),
         dig(errs, "sheets[0].cell_list[4].kind"), dig(errs, "sheets[0].cell_list[4].value"),
         dig(errs, "sheets[0].cell_list[2].kind"), dig(errs, "sheets[0].cell_list[2].value"),
         dig(errs, "sheets[0].error_cells"), dig(errs, "sheets[0].string_result_cells")],
        ["e", "#DIV/0!", "str", "甲乙", "b", "TRUE", 3, 1],
    )
    check(
        "「文件写了 t」与「按规范默认 n」分两键：一家每格都写，另一家公式格一个不写",
        [dig(errs, "sheets[0].cells_with_written_type"),
         dig(errs, "sheets[0].cell_list[1].kind_written"),
         dig(lbin("office-sheet", fixture("errors.xlsx")), "sheets[0].cells_with_written_type"),
         dig(lbin("office-sheet", fixture("errors.xlsx")), "sheets[0].cell_list[1].kind"),
         dig(lbin("office-sheet", fixture("errors.xlsx")), "sheets[0].cell_list[1].kind_written"),
         dig(lbin("office-sheet", fixture("errors.xlsx")), "sheets[0].error_cells")],
        [9, True, 3, "n", False, 0],
    )
    check(
        "同一批格子两种生产者：重算过才看得见错误，没重算全是空",
        [dig(lbin("office-sheet", fixture("errors-lo.xlsx"), "--csv"), "csv.text"),
         dig(lbin("office-sheet", fixture("errors.xlsx"), "--csv"), "csv.text")],
        ["7,#DIV/0!,,TRUE\n5,甲乙,,\n,#N/A,,\n,#VALUE!,,\n,14,10,\n",
         "7,,,TRUE\n5,,,\n,,,\n,,,\n,,,\n"],
    )
    ods_err = lbin("office-sheet", fixture("errors.ods"))
    their_cells = files["errors.ods"]["ods"]["sheets"][0]["cell_list"]
    check(
        "errors.ods 每一格的类型与字整串与读者一致（错误那几格文件写的是 string）",
        [[one.get("kind"), one.get("text")] for one in dig(ods_err, "sheets[0].cell_list") or []],
        [[one.get("value_type"), one.get("text")] for one in their_cells],
    )
    check(
        "同一个错误在三副件里是三个名字：按各自文件写的那个交，不替它们对上",
        [dig(errs, "sheets[0].cell_list[6].value"),
         dig(ods_err, "sheets[0].cell_list[6].text"),
         dig(ods_err, "sheets[0].cell_list[1].kind"),
         dig(ods_err, "sheets[0].cell_list[1].value")],
        ["#VALUE!", "错误:502", "string", None],
    )

    # ── 3f3) 一个格子的字分成几段：行内串、字符串表与 .ods 的记号 ────────
    # openpyxl 把富文本写成 `is`（整个不写 sharedStrings），LibreOffice 重写时全搬进表里，
    # .ods 又把空格与制表符写成 `text:s` / `text:tab` 记号 —— 三份件三种摆法，一家一份账
    print("=== 3f3) 分段写的字：runs、xml:space 与 ODF 的那三种记号 ===")
    for name in ("rich.xlsx", "rich-lo.xlsx"):
        got = lbin("office-sheet", fixture(name))
        want = files[name]["ooxml"]
        check(
            "%s 那张表自报的两个数与实际条数（count 是引用次数，uniqueCount 是条数）" % name,
            [dig(got, "workbook.shared_strings"), dig(got, "workbook.sst_count_written"),
             dig(got, "workbook.sst_unique_written"), dig(got, "workbook.sst_unique_matches"),
             dig(got, "workbook.sst_with_runs"), dig(got, "workbook.sst_with_preserved_space")],
            [want["strings"]["entries"], want["strings"]["count_written"],
             want["strings"]["unique_written"], want["strings"]["unique_matches"],
             want["strings"]["with_runs"], want["strings"]["with_preserved_space"]],
        )
        check(
            "%s 两本合计：几格是分段写的、几格的文件说了保留空格" % name,
            [dig(got, "workbook.totals.cells_with_runs"),
             dig(got, "workbook.totals.cells_with_preserved_space"),
             dig(got, "sheets[0].cells_with_runs"),
             dig(got, "sheets[0].cells_with_preserved_space")],
            [want["cells_with_runs"], want["cells_with_preserved_space"],
             want["cells_with_runs"], want["cells_with_preserved_space"]],
        )
        mine = {one.get("ref"): one for one in dig(got, "sheets[0].cell_list") or []}
        theirs = {one["ref"]: one for one in want["cell_strings"]}
        refs = sorted(theirs)
        check(
            "%s 每一格的分段账整份与读者一致（几段、每段的字与 rPr）" % name,
            [[(mine.get(ref) or {}).get(key) for key in
              ("value", "run_total", "rich_string", "space_preserved", "runs")] for ref in refs],
            [[theirs[ref]["text"], theirs[ref]["run_total"], theirs[ref]["rich"],
              theirs[ref]["preserved"], theirs[ref]["runs"]] for ref in refs],
        )
    rich_op = lbin("office-sheet", fixture("rich.xlsx"))
    rich_lo = lbin("office-sheet", fixture("rich-lo.xlsx"))
    check(
        "同一段字在两家文件里是两种写法：粗体一家写 1、另一家写 true，而一段没 rPr 与 rPr 是空的也分两件事",
        [dig(rich_op, "sheets[0].cell_list[2].runs[0].format[1].attrs.val"),
         dig(rich_lo, "sheets[0].cell_list[2].runs[0].format[0].attrs.val"),
         dig(rich_op, "sheets[0].cell_list[6].runs[0].props_written"),
         dig(rich_lo, "sheets[0].cell_list[6].runs[0].props_written"),
         dig(rich_op, "sheets[0].cell_list[6].runs[0].format"),
         dig(rich_lo, "sheets[0].cell_list[7].run_total")],
        ["1", "true", False, True, None, 2],
    )
    check(
        "空格不许被吃掉：三副件里那一格交回来的都是文件写的那一串",
        [dig(rich_op, "sheets[0].cell_list[3].value"),
         dig(rich_lo, "sheets[0].cell_list[3].value"),
         dig(lbin("office-sheet", fixture("rich.ods")), "sheets[0].cell_list[3].text")],
        ["  两头有空格  ", "  两头有空格  ", "  两头有空格  "],
    )
    check(
        "一家整个没写字符串表（八个格子的字全在页上），另一家写了 count 8 配 uniqueCount 7",
        [dig(rich_op, "workbook.shared_strings"), dig(rich_op, "workbook.sst_count_written"),
         dig(rich_lo, "workbook.shared_strings"), dig(rich_lo, "workbook.sst_count_written"),
         dig(rich_lo, "workbook.sst_unique_written"), dig(rich_lo, "workbook.sst_unique_matches")],
        [0, None, 7, "8", "7", True],
    )
    rich_ods = lbin("office-sheet", fixture("rich.ods"))
    mine = {
        one.get("ref"): one for one in dig(rich_ods, "sheets[0].cell_list") or []
    }
    theirs = {one["ref"]: one for one in files["rich.ods"]["ods"]["sheets"][0]["cell_list"]}
    refs = sorted(theirs)
    check(
        "rich.ods 每一格的字、几段样式与几个记号，两边整份一致",
        [[(mine.get(ref) or {}).get(key) for key in ("text", "spans", "specials")]
         for ref in refs],
        [[theirs[ref]["text"], theirs[ref]["spans"], theirs[ref]["specials"]] for ref in refs],
    )
    check(
        "ODF 把空格与制表符写成记号：展开之后字面都对，几段样式与几个记号各交一本",
        [dig(rich_ods, "sheets[0].cell_list[3].specials"),
         dig(rich_ods, "sheets[0].cell_list[7].specials"),
         dig(rich_ods, "sheets[0].cell_list[2].spans"),
         dig(rich_ods, "sheets[0].cell_list[6].spans"),
         dig(rich_ods, "sheets[0].cell_list[7].text"),
         dig(rich_ods, "sheets[0].cell_list[0].specials")],
        [2, 1, 2, 1, "\ttab 开头", 0],
    )

    # ── 3h) 一个格子「长什么样」：cellXfs 之外的三张表 ─────────────────
    print("=== 3h) 格子的长相：fontId / fillId / borderId 那一跳")
    LOOK = ("style_written", "style", "style_found", "style_attrs", "style_font_id",
            "style_font", "style_fill_id", "style_fill", "style_border_id", "style_border",
            "style_alignment", "style_bold", "style_filled", "style_wrapped")
    for name in ("styled.xlsx", "styled-lo.xlsx"):
        got = lbin("office-sheet", fixture(name))
        want = files[name]["ooxml"]
        check("%s 那四张表自报的 count 与实际条数" % name, dig(got, "workbook.styles"),
              want["style_tables"])
        mine = {one.get("ref"): one for one in dig(got, "sheets[0].cell_list") or []}
        theirs = {one["ref"]: one for one in want["cell_styles"]}
        refs = sorted(theirs)
        check(
            "%s 每一格的长相整份与读者一致（三个号、三行表内容、三个开关）" % name,
            [[(mine.get(ref) or {}).get(key) for key in LOOK] for ref in refs],
            [[theirs[ref].get(key) for key in LOOK] for ref in refs],
        )
        check(
            "%s 四本合计：几格粗体、几格有底、几格换行、几格连 s 都没写" % name,
            [dig(got, "sheets[0].cells_bold"), dig(got, "sheets[0].cells_filled"),
             dig(got, "sheets[0].cells_wrapped"),
             dig(got, "sheets[0].cells_without_style_written"),
             dig(got, "workbook.totals.cells_bold")],
            [want["cells_bold"], want["cells_filled"], want["cells_wrapped"],
             want["cells_without_style_written"], want["cells_bold"]],
        )
    styled = lbin("office-sheet", fixture("styled.xlsx"))
    styled_lo = lbin("office-sheet", fixture("styled-lo.xlsx"))
    check(
        "同一份稿子两家的写法差：占位那一条一家写空的 patternFill、另一家写明 none",
        [dig(styled, "sheets[0].cell_list[9].style_fill.parts[0].attrs"),
         dig(styled_lo, "sheets[0].cell_list[9].style_fill.parts[0].attrs"),
         dig(styled, "sheets[0].cell_list[0].style_font.parts[1].attrs.val"),
         dig(styled_lo, "sheets[0].cell_list[0].style_font.parts[0].attrs.val")],
        [{}, {"patternType": "none"}, "1", "true"],
    )
    check(
        "「没写」与「写了关」分两件事：alignment 整段没写是 null，写了 wrapText=false 是 false",
        [dig(styled, "sheets[0].cell_list[0].style_alignment"),
         dig(styled, "sheets[0].cell_list[0].style_wrapped"),
         dig(styled_lo, "sheets[0].cell_list[0].style_wrapped"),
         dig(styled, "sheets[0].cell_list[4].style_wrapped"),
         dig(styled, "sheets[0].cell_list[9].style_written"),
         dig(styled_lo, "sheets[0].cell_list[9].style_written")],
        [None, None, False, True, False, True],
    )
    check(
        "点状网格底被重写成了实心底：颜色与 pattern 各按各的文件交，不替它们对上",
        [dig(styled, "sheets[0].cell_list[7].style_fill.parts[0].attrs.patternType"),
         dig(styled_lo, "sheets[0].cell_list[7].style_fill.parts[0].attrs.patternType"),
         dig(styled, "sheets[0].cell_list[7].style_fill.parts[0].parts[0].attrs.rgb"),
         dig(styled_lo, "sheets[0].cell_list[7].style_fill.parts[0].parts[0].attrs.rgb"),
         dig(styled, "sheets[0].cell_list[5].style_font.parts[0].attrs.indexed"),
         dig(styled_lo, "sheets[0].cell_list[5].style_font.parts[1].attrs.rgb")],
        ["lightGrid", "solid", "FF00B050", "FF90DDB3", "64", "FF000000"],
    )

    # ── 3j) 页上那张图：alt 只有一处，而那一处写的可以是文件名 ─────────
    print("=== 3j) 页上的图：一处尺寸一处位置，alt 只有一处而它可以不是描述 ===")
    for name in ("deck-pictures.pptx", "deck-pictures-lo.pptx"):
        got = lbin("office-slide", fixture(name))
        want = {one["part"]: one["picture_rows"] for one in files[name]["ooxml"]["slides"]}
        mine = {one["part"]: one for one in (dig(got, "slides") or [])}
        check("%s 每页那张图的整份账与读者一致（尺寸、位置、锁、拉伸、alt）" % name,
              {k: (mine.get(k) or {}).get("picture_list") for k in want}, want)
        check("%s 每页几张图、几张写着非空的 descr" % name,
              {k: [(mine.get(k) or {}).get("pictures"),
                   (mine.get(k) or {}).get("pictures_with_alt_text")] for k in want},
              {k: [len(v), sum(1 for one in v if one["descr"])] for k, v in want.items()})
    odpic = lbin("office-slide", fixture("deck-pictures.odp"))
    want = {one["name"]: one["picture_rows"] for one in files["deck-pictures.odp"]["odp"]["slides"]}
    mine = {one["name"]: one for one in (dig(odpic, "slides") or [])}
    check("deck-pictures.odp 每页那张图的整份账与读者一致（frame 那份账与 odt 同一条）",
          {k: (mine.get(k) or {}).get("picture_list") for k in want}, want)
    check("deck-pictures.odp 每页几张图、几张有替代文字",
          {k: [(mine.get(k) or {}).get("pictures"),
               (mine.get(k) or {}).get("pictures_with_alt_text")] for k in want},
          {k: [len(v), sum(1 for one in v if one["alt"])] for k, v in want.items()})

    pics = lbin("office-slide", fixture("deck-pictures.pptx"))
    pics_lo = lbin("office-slide", fixture("deck-pictures-lo.pptx"))
    check(
        "没给替代文字那一张：python-pptx 把源文件名写进了 descr —— 数出来「有 alt」而那句不是描述",
        [dig(pics, "slides[1].picture_list[0].name"),
         dig(pics, "slides[1].picture_list[0].descr"),
         dig(pics, "slides[1].picture_list[0].descr_written"),
         dig(pics, "slides[1].pictures_with_alt_text")],
        ["Picture 1", "dot.png", True, 1],
    )
    check(
        "来回一趟换掉的三样：号被重排、锁整个不见、尺寸从 4000 变成 3999",
        [dig(pics, "slides[0].picture_list[0].blip_id"),
         dig(pics_lo, "slides[0].picture_list[0].blip_id"),
         dig(pics, "slides[0].picture_list[0].locks"),
         dig(pics_lo, "slides[0].picture_list[0].locks"),
         dig(pics, "slides[0].picture_list[0].ext.mm_w"),
         dig(pics_lo, "slides[0].picture_list[0].ext.mm_w"),
         dig(pics, "slides[0].picture_list[0].stretch"),
         dig(pics_lo, "slides[0].picture_list[0].stretch")],
        ["rId2", "rId1", {"noChangeAspect": "1"}, None, 4000, 3999, ["fillRect"], []],
    )
    check(
        "位置与尺寸同层（off 360000 = 1cm），而 odp 那一族的图框不写 anchor-type，摆法交 null",
        [dig(pics, "slides[0].picture_list[0].off.mm_x"),
         dig(pics, "slides[0].picture_list[0].off.mm_y"),
         dig(pics, "slides[1].picture_list[0].off.mm_x"),
         dig(pics, "slides[0].picture_list[0].target"),
         dig(odpic, "slides[0].picture_list[0].placed"),
         dig(odpic, "slides[0].picture_list[0].mm_w"),
         dig(odpic, "slides[0].picture_list[0].alt")],
        [1000, 1000, 10000, "ppt/media/image1.png", None, 3999, "一个红点"],
    )
    check(
        "不写宽高那一张：python-pptx 按 72 DPI 把 40px 换成 508000 EMU，LO 重写又换成 507600",
        [dig(pics, "slides[1].picture_list[0].ext.cx"),
         dig(pics, "slides[1].picture_list[0].ext.mm_w"),
         dig(pics_lo, "slides[1].picture_list[0].ext.cx"),
         dig(pics_lo, "slides[1].picture_list[0].ext.mm_w"),
         dig(odpic, "slides[1].picture_list[0].mm_h"),
         dig(odpic, "slides[1].picture_list[0].style")],
        ["508000", 1411, "507600", 1410, 846, "gr1"],
    )

    # ── 3l) 字符样式：段上只写一个号，那句话住在另一份部件里 ────────────────
    print("=== 3l) 字符样式那一跳：一处一半、号与名不同、span 会套 span ===")
    style = {one: lbin("office-doc", fixture("charstyles." + one)) for one in ("docx", "odt", "rtf")}
    style["lo"] = lbin("office-doc", fixture("charstyles-lo.docx"))
    check("charstyles.docx 逐串账（含样式那一跳）与读者一致",
          dig(style["docx"], "structure.run_formats"),
          files["charstyles.docx"]["ooxml"]["run_formats"])
    check("charstyles-lo.docx 逐串账与读者一致（重写也留着 rStyle 的那一份）",
          dig(style["lo"], "structure.run_formats"),
          files["charstyles-lo.docx"]["ooxml"]["run_formats"])
    check("charstyles.odt 逐串账与读者一致（span 套 span 与显示名）",
          dig(style["odt"], "structure.run_formats"),
          files["charstyles.odt"]["odt"]["run_formats"])
    check("charstyles.rtf 逐串账与读者一致（cs 号跳样式表解出名字）",
          [dig(style["rtf"], "structure.run_formats.list"),
           dig(style["rtf"], "structure.run_formats.words_outside_groups")],
          [files["charstyles.rtf"]["rtf"]["run_rows"],
           files["charstyles.rtf"]["rtf"]["run_words_stray"]])
    check(
        "段上只写一个号：三段都点样式、三条都跳得到，而 bold_on 只数段上自己说的那一次",
        [dig(style["docx"], "structure.run_formats.with_style"),
         dig(style["docx"], "structure.run_formats.style_found"),
         dig(style["docx"], "structure.run_formats.bold_on"),
         dig(style["docx"], "structure.run_formats.bold_from_style"),
         dig(style["docx"], "structure.run_formats.italic_from_style"),
         dig(style["docx"], "structure.run_formats.where_both_spoke"),
         dig(style["lo"], "structure.run_formats.props_empty"),
         dig(style["lo"], "structure.run_formats.style_found")],
        [3, 3, 1, 1, 2, 0, 4, 3],
    )
    check(
        "一处一半：Emphasis 那一段说斜、段上自己写 b 说粗，两处各半，不合成一句「又粗又斜」",
        [dig(style["docx"], "structure.run_formats.list[3].elements"),
         dig(style["docx"], "structure.run_formats.list[3].switches.bold"),
         dig(style["docx"], "structure.run_formats.list[3].switches.italic"),
         dig(style["docx"], "structure.run_formats.list[3].style_switches.italic"),
         dig(style["docx"], "structure.run_formats.list[3].style_switches.bold")],
        [["rStyle", "b"], True, None, True, None],
    )
    check(
        "样式号与样式名不是一回事：SubtleEmphasis 这个号的名字里带一个空格",
        [dig(style["docx"], "structure.run_formats.list[5].style"),
         dig(style["docx"], "structure.run_formats.list[5].style_name"),
         dig(style["docx"], "structure.run_formats.list[1].style_parent"),
         dig(style["odt"], "structure.run_formats.list[1].style"),
         dig(style["odt"], "structure.run_formats.list[1].display"),
         dig(style["odt"], "structure.run_formats.list[1].found_in"),
         dig(style["rtf"], "structure.run_formats.list[0].values.character_style.index"),
         dig(style["rtf"], "structure.run_formats.list[0].values.character_style.name")],
        ["SubtleEmphasis", "Subtle Emphasis", "DefaultParagraphFont",
         "Strong_20_Emphasis", "Strong Emphasis", "styles", "34", "Strong"],
    )
    check(
        "ODF 的 span 会套 span：外层只点样式、里层才写粗，深度与「自己那半句字」各交一条",
        [dig(style["odt"], "structure.run_formats.nested_spans"),
         dig(style["odt"], "structure.run_formats.checked"),
         dig(style["odt"], "structure.run_formats.list[3].depth"),
         dig(style["odt"], "structure.run_formats.list[3].text"),
         dig(style["odt"], "structure.run_formats.list[3].style"),
         dig(style["odt"], "structure.run_formats.list[4].depth"),
         dig(style["odt"], "structure.run_formats.list[4].text"),
         dig(style["odt"], "structure.run_formats.list[4].found_in"),
         dig(style["odt"], "structure.run_formats.list[4].switches.bold")],
        [1, 8, 1, "", "Emphasis", 2, "又粗又斜", "content", True],
    )
    check(
        "跨家不许对成一个数：同一句「这几串字是粗的」，ODF 从样式里数到 2，OOXML 从段上数到 1",
        [dig(style["docx"], "structure.run_formats.bold_on"),
         dig(style["odt"], "structure.run_formats.bold_on"),
         dig(style["odt"], "structure.run_formats.italic_on"),
         dig(style["docx"], "structure.run_formats.italic_on")],
        [1, 2, 2, 0],
    )
    check(
        "RTF 的群头把样式自己那份 \\b 也抄了一遍 —— 号与话同时在场，两处都交",
        [dig(style["rtf"], "structure.run_formats.list[0].words"),
         dig(style["rtf"], "structure.run_formats.list[0].switches.bold"),
         dig(style["rtf"], "structure.run_formats.list[1].words[0][1]"),
         dig(style["rtf"], "structure.run_formats.list[2].values.character_style.name")],
        [[["cs", "34"], ["ab", ""], ["ab", ""], ["ab", ""], ["b", ""]], True,
         "35", "Subtle Emphasis"],
    )

    # ── 3k) 那几个字自己说了什么：三家把同一句格式话说在三个地方 ────────────
    print("=== 3k) 字符格式：rPr 的孩子、span 点名的样式、群头上的控制字 ===")
    runs = {one: lbin("office-doc", fixture("styled-text." + one)) for one in ("docx", "odt", "rtf")}
    runs["lo"] = lbin("office-doc", fixture("styled-text-lo.docx"))
    check("styled-text.docx 逐串字符格式整份账与读者一致",
          dig(runs["docx"], "structure.run_formats"),
          files["styled-text.docx"]["ooxml"]["run_formats"])
    check("styled-text-lo.docx 逐串字符格式整份账与读者一致（每一串都补一个空 rPr 的那一份）",
          dig(runs["lo"], "structure.run_formats"),
          files["styled-text-lo.docx"]["ooxml"]["run_formats"])
    check("styled-text.odt 逐串字符格式整份账与读者一致（含夹在 span 中间不包起来的那些字）",
          dig(runs["odt"], "structure.run_formats"),
          files["styled-text.odt"]["odt"]["run_formats"])
    check("styled-text.rtf 逐串字符格式与读者一致（号已按文件自己那两张表跳过一跳）",
          [dig(runs["rtf"], "structure.run_formats.list"),
           dig(runs["rtf"], "structure.run_formats.words_outside_groups")],
          [files["styled-text.rtf"]["rtf"]["run_rows"],
           files["styled-text.rtf"]["rtf"]["run_words_stray"]])
    check(
        "同一句话的三种存法：三家数出来同一条答案（三串粗、一串明确不粗、14 串说过话）",
        [dig(runs["docx"], "structure.run_formats.bold_on"),
         dig(runs["odt"], "structure.run_formats.bold_on"),
         dig(runs["rtf"], "structure.run_formats.bold_on"),
         dig(runs["docx"], "structure.run_formats.bold_off"),
         dig(runs["odt"], "structure.run_formats.bold_off"),
         dig(runs["rtf"], "structure.run_formats.bold_off"),
         dig(runs["docx"], "structure.run_formats.with_format"),
         dig(runs["odt"], "structure.run_formats.with_format"),
         dig(runs["rtf"], "structure.run_formats.with_format")],
        [3, 3, 3, 1, 1, 1, 14, 14, 14],
    )
    check(
        "「有这一格而里面是空的」与「压根没有这一格」：两家生产者正好一边一种",
        [dig(runs["docx"], "structure.run_formats.with_props"),
         dig(runs["docx"], "structure.run_formats.props_empty"),
         dig(runs["lo"], "structure.run_formats.with_props"),
         dig(runs["lo"], "structure.run_formats.props_empty")],
        [14, 0, 30, 16],
    )
    check(
        "ODF 的第三种情况：夹在 span 中间的字文件没给它立元素，而 14 个 span 全在 content.xml 查到",
        [dig(runs["odt"], "structure.run_formats.spans"),
         dig(runs["odt"], "structure.run_formats.bare_text"),
         dig(runs["odt"], "structure.run_formats.checked"),
         dig(runs["odt"], "structure.run_formats.resolved"),
         dig(runs["odt"], "structure.run_formats.list[2].found_in"),
         dig(runs["odt"], "structure.run_formats.list[2].style"),
         dig(runs["odt"], "structure.run_formats.list[0].element"),
         dig(runs["odt"], "structure.run_formats.list[0].resolved")],
        [14, 16, 30, 14, "content", "T1", "#text", None],
    )
    check(
        "「明确不粗」有四家拼法：重写一次就从 0 变成 false，读成「有 b 这个孩子」就把话说反了",
        [dig(runs["docx"], "structure.run_formats.list[20].elements"),
         dig(runs["docx"], "structure.run_formats.list[20].switches.bold"),
         dig(runs["lo"], "structure.run_formats.list[20].format[0].written.val"),
         dig(runs["lo"], "structure.run_formats.list[20].switches.bold"),
         dig(runs["odt"], "structure.run_formats.list[20].written.fo:font-weight"),
         dig(runs["odt"], "structure.run_formats.list[20].switches.bold"),
         dig(runs["rtf"], "structure.run_formats.list[9].format[0].digits"),
         dig(runs["rtf"], "structure.run_formats.list[9].switches.bold")],
        [["b"], False, "false", False, "normal", False, "0", False],
    )
    check(
        "那一个红点要跳一次表才看得到：docx 直接写颜色，RTF 写号，号在文件自己那张 colortbl 上",
        [dig(runs["docx"], "structure.run_formats.list[12].values.color"),
         dig(runs["odt"], "structure.run_formats.list[12].written.fo:color"),
         dig(runs["rtf"], "structure.run_formats.list[5].values.color.index"),
         dig(runs["rtf"], "structure.run_formats.list[5].values.color.rgb"),
         dig(runs["rtf"], "structure.run_formats.list[5].values.color.resolved")],
        ["C00000", "#c00000", "23", "C00000", True],
    )
    check(
        "9 磅在 OOXML 与 RTF 是同一个半磅数，在 ODF 是自带单位的串 —— 三个都交原样，不折成一个",
        [dig(runs["docx"], "structure.run_formats.list[16].values.size"),
         dig(runs["rtf"], "structure.run_formats.list[7].values.size"),
         dig(runs["odt"], "structure.run_formats.list[16].written.fo:font-size")],
        ["18", "18", "9pt"],
    )
    check(
        "一串字里两个孩子：docx 按文件顺序交 b、i，RTF 的群头写的是 i、b —— 顺序是文件的，不重排",
        [dig(runs["docx"], "structure.run_formats.list[24].elements"),
         dig(runs["rtf"], "structure.run_formats.list[11].format[0].element"),
         dig(runs["rtf"], "structure.run_formats.list[11].format[1].element"),
         dig(runs["rtf"], "structure.run_formats.list[11].switches.bold"),
         dig(runs["rtf"], "structure.run_formats.list[11].switches.italic")],
        [["b", "i"], "i", "b", True, True],
    )
    check(
        "下划线这一族写在「日文下划线」那个口袋上：词先交出来，开关才算一次「要」",
        [dig(runs["docx"], "structure.run_formats.list[6].elements"),
         dig(runs["docx"], "structure.run_formats.list[6].format[0].written.val"),
         dig(runs["odt"], "structure.run_formats.list[6].written.style:text-underline-style"),
         dig(runs["rtf"], "structure.run_formats.list[2].values.underline_word"),
         dig(runs["rtf"], "structure.run_formats.list[2].switches.underline")],
        [["u"], "single", "solid", "aul", True],
    )
    check(
        "RTF 没有「一串字」这个元素：有串而没说格式这一问在这里判不住（null），"
        "段前缀那些控制字另记一条数",
        [dig(runs["rtf"], "structure.run_formats.with_props"),
         dig(runs["rtf"], "structure.run_formats.props_empty"),
         dig(runs["rtf"], "structure.run_formats.checked"),
         dig(runs["rtf"], "structure.run_formats.words_outside_groups"),
         dig(runs["rtf"], "structure.run_formats.list[8].values.asian_font.index"),
         dig(runs["rtf"], "structure.run_formats.list[8].values.asian_font.name")],
        [None, None, 14, 135, "9", None],
    )

    # ── 3m) 一串字里到底有什么：字、制表、分页、图、注的号与域指令 ────────────
    print("=== 3m) 一串字里的每一块：text 只算 w:t，其余按文件顺序整串交出来 ===")
    toc = lbin("office-doc", fixture("toc.docx"))
    ne = lbin("office-doc", fixture("notes-end.docx"))
    check(
        "域指令不是页面上的字：这一串 text 交空串，而它带的那一串指令原样交出来",
        [dig(toc, "structure.run_formats.list[2].text"),
         dig(toc, "structure.run_formats.list[2].contents[0].element"),
         dig(toc, "structure.run_formats.list[2].contents[0].written.space"),
         dig(toc, "structure.run_formats.list[2].instructions[0]"),
         dig(toc, "structure.run_formats.list[1].field_chars"),
         dig(toc, "structure.run_formats.field_runs"),
         dig(toc, "structure.run_formats.runs_with_text"),
         dig(toc, "structure.run_formats.checked")],
        ['', "instrText", "preserve", ' TOC \\o "1-2" \\h', ["begin"], 4, 12, 21],
    )
    check(
        "一串字里没有字不等于没有这一串：图、分页符与批注的号各是一条 contents",
        [dig(toc, "structure.run_formats.list[15].contents[0].element"),
         dig(toc, "structure.run_formats.list[15].text"),
         dig(toc, "structure.run_formats.list[17].breaks"),
         dig(toc, "structure.run_formats.list[17].contents[0].written.type"),
         dig(toc, "structure.run_formats.list[19].refs.comment"),
         dig(toc, "structure.run_formats.list[19].note"),
         dig(toc, "structure.run_formats.runs_with_ref"),
         dig(toc, "structure.run_formats.ref_found")],
        ["drawing", "", ["page"], "page", "0", None, 1, 0],
    )
    check(
        "脚注的 2 与尾注的 2 是两条注：号同一个而两本账，只按号对就都落在部件第 0 条",
        [dig(ne, "structure.run_formats.list[2].refs.footnote"),
         dig(ne, "structure.run_formats.list[2].note.kind"),
         dig(ne, "structure.run_formats.list[2].note.found"),
         dig(ne, "structure.run_formats.list[2].note.at"),
         dig(ne, "structure.run_formats.list[3].refs.endnote"),
         dig(ne, "structure.run_formats.list[3].note.kind"),
         dig(ne, "structure.run_formats.list[3].note.at"),
         dig(ne, "structure.run_formats.list[6].note.at")],
        ["2", "footnote", True, 0, "2", "endnote", 2, 1],
    )
    check(
        "注那两份部件与正文的号是双向的一本账：部件三条、正文引用三条、没被引用的 0 条",
        [dig(ne, "structure.run_formats.notes_in_parts"),
         dig(ne, "structure.run_formats.notes_referenced"),
         dig(ne, "structure.run_formats.notes_unreferenced"),
         dig(ne, "structure.run_formats.ref_found"),
         dig(ne, "structure.run_formats.runs_with_ref"),
         dig(toc, "structure.run_formats.notes_in_parts")],
        [3, 3, 0, 3, 3, 0],
    )
    for name in ("notes-end.docx", "notes-foot.docx", "notes.docx", "protected.docx"):
        got = lbin("office-doc", fixture(name))
        want = files[name]["ooxml"]["run_formats"]
        check("%s 每一串字里有什么（text / contents / refs / breaks / instructions）与读者一致" % name,
              [dig(got, "structure.run_formats.list"),
               dig(got, "structure.run_formats.runs_with_text"),
               dig(got, "structure.run_formats.runs_with_ref"),
               dig(got, "structure.run_formats.ref_found"),
               dig(got, "structure.run_formats.field_runs"),
               dig(got, "structure.run_formats.notes_in_parts"),
               dig(got, "structure.run_formats.notes_referenced"),
               dig(got, "structure.run_formats.notes_unreferenced")],
              [want["list"], want["runs_with_text"], want["runs_with_ref"], want["ref_found"],
               want["field_runs"], want["notes_in_parts"], want["notes_referenced"],
               want["notes_unreferenced"]])

    # ── 3n) 这一串字被谁包着：超链接与修订那三个壳上的字，串自己一个都没有 ────
    print("=== 3n) 壳上那半句：hyperlink / ins / del 各自的作者、时间与号 ===")
    rev = lbin("office-doc", fixture("revisions.docx"))
    revlo = lbin("office-doc", fixture("revisions-lo.docx"))
    notes = lbin("office-doc", fixture("notes.docx"))
    check(
        "同一次插入：一家写一条壳（id 11、一句「124000 元」），"
        "LibreOffice 重写成两条壳（id 0 与 1、把数与单位拆开）",
        [dig(rev, "structure.run_formats.list[3].wrapped"),
         dig(rev, "structure.run_formats.list[3].wrapped_written.id"),
         dig(rev, "structure.run_formats.list[3].text"),
         dig(rev, "structure.run_formats.runs_wrapped"),
         dig(rev, "structure.run_formats.wrapped_ins"),
         dig(rev, "structure.run_formats.wrapped_del"),
         dig(revlo, "structure.run_formats.list[3].wrapped_written.id"),
         dig(revlo, "structure.run_formats.list[3].text"),
         dig(revlo, "structure.run_formats.list[4].wrapped_written.id"),
         dig(revlo, "structure.run_formats.list[4].text"),
         dig(revlo, "structure.run_formats.runs_wrapped"),
         dig(revlo, "structure.run_formats.wrapped_ins"),
         dig(revlo, "structure.run_formats.wrapped_del")],
        ["ins", "11", "124000 元", 3, 2, 1, "0", "124000 ", "1", "元", 5, 3, 2],
    )
    check(
        "删掉的字不写在 `w:t` 上：这一串 text 是空串，而那些字照原样在 delText 里，"
        "作者与时间是壳上写的（两家一字不差）",
        [dig(rev, "structure.run_formats.list[4].wrapped"),
         dig(rev, "structure.run_formats.list[4].text"),
         dig(rev, "structure.run_formats.list[4].contents[0].element"),
         dig(rev, "structure.run_formats.list[4].wrapped_written.author"),
         dig(rev, "structure.run_formats.list[4].wrapped_written.date"),
         dig(revlo, "structure.run_formats.list[5].wrapped"),
         dig(revlo, "structure.run_formats.list[5].text"),
         dig(revlo, "structure.run_formats.list[5].contents[0].element"),
         dig(revlo, "structure.run_formats.list[5].wrapped_written.author")],
        ["del", "", "delText", "李四", "2026-03-06T11:45:00Z",
         "del", "", "delText", "李四"],
    )
    check(
        "同一句「预算制度」是一条链接：三副件三个号（rId2 / rId9），号是生产者自己排的",
        [dig(toc, "structure.run_formats.list[14].wrapped"),
         dig(toc, "structure.run_formats.list[14].wrapped_written.id"),
         dig(toc, "structure.run_formats.list[14].text"),
         dig(notes, "structure.run_formats.list[8].wrapped_written.id"),
         dig(runs["lo"], "structure.run_formats.wrapped_hyperlink"),
         dig(toc, "structure.run_formats.runs_wrapped"),
         dig(toc, "structure.run_formats.wrapped_ins"),
         dig(notes, "structure.run_formats.wrapped_hyperlink")],
        ["hyperlink", "rId2", "预算制度", "rId9", 0, 1, 0, 1],
    )
    check(
        "没壳的那些串交 null，而不是空串：一份件里 21 串只有 1 串是包起来的",
        [dig(toc, "structure.run_formats.list[13].wrapped"),
         dig(toc, "structure.run_formats.list[13].wrapped_written"),
         dig(rev, "structure.run_formats.list[2].wrapped"),
         dig(notes, "structure.run_formats.runs_wrapped"),
         dig(notes, "structure.run_formats.checked")],
        [None, None, None, 1, 13],
    )

    # ── 3o) 这一串字里有一个「要算的东西」：域、书签与站内跳转的两种地址 ──────
    print("=== 3o) 域与跳转：instrText 那一条链，与 anchor 对着书签名的那一跳 ===")
    fld = lbin("office-doc", fixture("fields.docx"))
    fldlo = lbin("office-doc", fixture("fields-lo.docx"))
    check(
        "四条域链（SEQ 编号 / DATE / 正文 PAGE，页脚那一条不在正文里）：指令原样交，链上那几串字自己没字",
        [dig(fld, "structure.run_formats.checked"),
         dig(fld, "structure.run_formats.field_runs"),
         dig(fld, "structure.run_formats.runs_with_text"),
         dig(fld, "structure.run_formats.list[2].field_chars"),
         dig(fld, "structure.run_formats.list[2].contents[0].written.fldCharType"),
         dig(fld, "structure.run_formats.list[3].instructions[0]"),
         dig(fld, "structure.run_formats.list[4].field_chars"),
         dig(fld, "structure.run_formats.list[6].field_chars")],
        [23, 12, 11, ["begin"], "begin", ' SEQ 表 \\* ARABIC', ["separate"], ["end"]],
    )
    check(
        "同一句「这里要算」，重写之后连指令都不再逐字相同：多一个尾空格、把日期格式里的连字符反斜杠转义",
        [dig(fld, "structure.run_formats.list[3].instructions[0]"),
         dig(fldlo, "structure.run_formats.list[3].instructions[0]"),
         dig(fld, "structure.run_formats.list[13].instructions[0]"),
         dig(fldlo, "structure.run_formats.list[13].instructions[0]"),
         dig(fldlo, "structure.run_formats.field_runs"),
         dig(fldlo, "structure.run_formats.checked")],
        [' SEQ 表 \\* ARABIC', ' SEQ 表 \\* ARABIC ',
         ' DATE \\@ "yyyy-MM-dd"', ' DATE \\@"yyyy\\-MM\\-dd" ', 12, 23],
    )
    check(
        "站内跳转的地址写在 `w:anchor` 上（外部链接写的是 `r:id`），而它对的是书签的**名字**："
        "一条对得到、一条对不到",
        [dig(fld, "structure.run_formats.list[7].wrapped"),
         dig(fld, "structure.run_formats.list[7].link_anchor"),
         dig(fld, "structure.run_formats.list[7].link_found"),
         dig(fld, "structure.run_formats.list[8].link_anchor"),
         dig(fld, "structure.run_formats.list[8].link_found"),
         dig(fld, "structure.run_formats.bookmark_names"),
         dig(fld, "structure.run_formats.runs_with_anchor"),
         dig(fld, "structure.run_formats.anchors_found"),
         dig(fld, "structure.run_formats.anchors_missing"),
         dig(toc, "structure.run_formats.list[14].link_anchor"),
         dig(toc, "structure.run_formats.list[14].link_found")],
        ["hyperlink", "表锚点", True, "没这个书签", False, 1, 2, 1, 1, None, None],
    )
    check(
        "那条「脏了、下次要重算」的开关只有写的那一份有：重写把 `w:dirty` 整个丢了，"
        "并把缓存值换成它自己算出来的那两个数",
        [dig(fld, "structure.run_formats.list[12].contents[0].written"),
         dig(fldlo, "structure.run_formats.list[12].contents[0].written"),
         dig(fld, "structure.run_formats.list[15].text"),
         dig(fldlo, "structure.run_formats.list[15].text"),
         dig(fld, "structure.run_formats.list[20].text"),
         dig(fldlo, "structure.run_formats.list[20].text"),
         dig(fld, "structure.run_formats.list[5].contents[0].written"),
         dig(fldlo, "structure.run_formats.list[5].contents[0].written")],
        [{"fldCharType": "begin", "dirty": "true"}, {"fldCharType": "begin"},
         "2026-09-24", "2026-09-25", "2", "1", {"space": "preserve"}, {}],
    )

    # ── 3p) ODF 那一族：链接与域也是段里的一条元素，anchor 对的还是书签名 ─────────
    print("=== 3p) ODF 的链接与域：同一个 anchor 问题，两族两处存法 ===")
    fodt = lbin("office-doc", fixture("fields.odt"))
    nodt = lbin("office-doc", fixture("notes.odt"))
    cotd = lbin("office-doc", fixture("toc.odt"))
    check(
        "同一个 anchor 问题两族各问一次：ODF 对 `text:bookmark-start/@text:name`，"
        "OOXML 对 `w:bookmarkStart/@w:name`，两份都是 1 对 1 错",
        [dig(fodt, "structure.run_formats.links"),
         dig(fodt, "structure.run_formats.list[2].element"),
         dig(fodt, "structure.run_formats.list[2].link_anchor"),
         dig(fodt, "structure.run_formats.list[2].link_found"),
         dig(fodt, "structure.run_formats.list[3].link_anchor"),
         dig(fodt, "structure.run_formats.list[3].link_found"),
         dig(fodt, "structure.run_formats.bookmarks_written"),
         dig(fodt, "structure.run_formats.anchors_found"),
         dig(fodt, "structure.run_formats.anchors_missing"),
         dig(fld, "structure.run_formats.bookmark_names"),
         dig(fld, "structure.run_formats.anchors_found"),
         dig(fld, "structure.run_formats.anchors_missing")],
        [2, "a", "表锚点", True, "没这个书签", False, 1, 1, 1, 1, 1, 1],
    )
    check(
        "站外的地址在 ODF 里没有第二跳（href 就是地址）→ anchor 与 found 都交 null；"
        "而它照样点着一个字符样式，三家各起各的名",
        [dig(nodt, "structure.run_formats.list[6].element"),
         dig(nodt, "structure.run_formats.list[6].link_href"),
         dig(nodt, "structure.run_formats.list[6].link_anchor"),
         dig(nodt, "structure.run_formats.list[6].link_found"),
         dig(nodt, "structure.run_formats.list[6].style"),
         dig(nodt, "structure.run_formats.list[6].resolved"),
         dig(cotd, "structure.run_formats.list[8].style"),
         dig(nodt, "structure.run_formats.links")],
        ["a", "https://example.com/budget", None, None, "ListLabel_20_5", True,
         "ListLabel_20_2", 1],
    )
    check(
        "域在 ODF 是「元素自己说算了什么」：SEQ 那条被拆成名字与算式，"
        "日期另点一份数据样式，页码的缓存值写着 0",
        [dig(fodt, "structure.run_formats.field_pieces"),
         dig(fodt, "structure.run_formats.list[1].element"),
         dig(fodt, "structure.run_formats.list[1].own_written.text:formula"),
         dig(fodt, "structure.run_formats.list[1].own_written.text:name"),
         dig(fodt, "structure.run_formats.list[1].text"),
         dig(fodt, "structure.run_formats.list[7].own_written.style:data-style-name"),
         dig(fodt, "structure.run_formats.list[8].own_written.text:select-page"),
         dig(fodt, "structure.run_formats.list[8].text"),
         dig(fodt, "structure.run_formats.checked")],
        [3, "sequence", "ooow:表+1", "表", "1", "N10049", "current", "0", 10],
    )

    # ── 3q) 三族答同一个 anchor 问题：书签的名与那一跳落不落得地 ───────────────
    print("=== 3q) 书签与站内跳转：一份稿子、三种存法、同一个坏名 ===")
    for name in sorted(one.name for one in FIXTURES.glob("*.rtf")):
        got = lbin("office-doc", fixture(name))
        rwant = files[name]["rtf"]
        check("%s 书签与跳转那八本账与读者一致" % name,
              [dig(got, "structure.bookmark_names"),
               dig(got, "structure.bookmark_starts"),
               dig(got, "structure.bookmark_ends"),
               dig(got, "structure.anchors"),
               dig(got, "structure.anchors_found"),
               dig(got, "structure.anchors_missing"),
               dig(got, "structure.links_external"),
               dig(got, "structure.bookmarks")],
              [rwant["bookmarks"], rwant["bookmark_starts"], rwant["bookmark_ends"],
               rwant["anchors"], rwant["anchors_found"], rwant["anchors_missing"],
               rwant["links_external"], rwant["bookmark_starts"]])
    fldrtf = lbin("office-doc", fixture("fields.rtf"))
    check(
        "三族各 1 条指得到、1 条指不到：docx 对 w:bookmarkStart 的 name、"
        "odt 对 text:bookmark-start 的 name、rtf 对书签那一群里解过转义的那一个名",
        [dig(fld, "structure.run_formats.anchors_found"),
         dig(fld, "structure.run_formats.anchors_missing"),
         dig(fodt, "structure.run_formats.anchors_found"),
         dig(fodt, "structure.run_formats.anchors_missing"),
         dig(fldrtf, "structure.anchors_found"),
         dig(fldrtf, "structure.anchors_missing"),
         dig(fldrtf, "structure.bookmark_names"),
         dig(fldrtf, "structure.anchors")],
        [1, 1, 1, 1, 1, 1, ["表锚点"], ["表锚点", "没这个书签"]],
    )
    check(
        "读名字不改跳过：书签那一群仍然整群跳过（`skipped_destinations` 数得出这一条）、"
        "六个书签名与那两行字一个也没多进正文，而解出来的名照交",
        [any("表锚点" in one for one in files["fields.rtf"]["rtf"]["lines"]),
         dig(fldrtf, "structure.skipped_destinations") > 0,
         dig(fldrtf, "structure.bookmark_names"),
         dig(fldrtf, "structure.bookmarks")],
        [False, True, ["表锚点"], 1],
    )

    # ── 3r) 分节的页眉页脚：没写那一格是「沿用上一节」，不是「没有」 ────────────
    print("=== 3r) 节的六格：自己写的、沿用上一节的，与一条指着没有的号 ===")
    for name in sorted(one.name for one in FIXTURES.glob("*.docx")):
        got = lbin("office-doc", fixture(name))
        check("%s 分节六格与读者一致（每节六格：own / earlier-section / null）" % name,
              dig(got, "structure.header_footers"),
              files[name]["ooxml"]["header_footers"])
    sec = lbin("office-doc", fixture("sections.docx"))
    plain = lbin("office-doc", fixture("para.docx"))
    check(
        "第二节自己一个字都没写：页眉与页脚都沿用第一节那两份部件（Word 界面上叫「与上一节相同」）",
        [dig(sec, "structure.header_footers.sections_total"),
         dig(sec, "structure.header_footers.slots_written"),
         dig(sec, "structure.header_footers.slots_inherited"),
         dig(sec, "structure.header_footers.sections[0].slots.header:default.from"),
         dig(sec, "structure.header_footers.sections[0].slots.header:default.part"),
         dig(sec, "structure.header_footers.sections[1].slots.header:default.from"),
         dig(sec, "structure.header_footers.sections[1].slots.header:default.part"),
         dig(sec, "structure.header_footers.sections[1].slots.footer:default.id"),
         dig(sec, "structure.header_footers.sections[1].written_refs")],
        [2, 3, 2, "own", "word/header1.xml",
         "earlier-section", "word/header1.xml", "rId10", 1],
    )
    check(
        "一条指着不存在的号：part 与 external 都交 null（连是不是站外都不知道），"
        "而那一个号照交，refs_unresolved 数得出这一条",
        [dig(sec, "structure.header_footers.sections[1].slots.header:even.id"),
         dig(sec, "structure.header_footers.sections[1].slots.header:even.part"),
         dig(sec, "structure.header_footers.sections[1].slots.header:even.part_exists"),
         dig(sec, "structure.header_footers.sections[1].slots.header:even.external"),
         dig(sec, "structure.header_footers.refs_total"),
         dig(sec, "structure.header_footers.refs_unresolved"),
         dig(sec, "structure.header_footers.refs_unresolved")],
        ["rId999", None, False, None, 3, 1, 1],
    )
    check(
        "首尾页与奇偶页那两个开关按写的交：`titlePg` 在两节都写着，"
        "`evenAndOddHeaders` 写了而没写值；两节都不点名任何部件的那一份，六格全 null",
        [dig(sec, "structure.header_footers.sections[0].title_pg_written"),
         dig(sec, "structure.header_footers.sections[1].title_pg_written"),
         dig(sec, "structure.header_footers.even_and_odd_headers.written"),
         dig(sec, "structure.header_footers.even_and_odd_headers.val"),
         dig(plain, "structure.header_footers.sections_total"),
         dig(plain, "structure.header_footers.slots_written"),
         dig(plain, "structure.header_footers.sections[0].slots.header:default"),
         dig(plain, "structure.header_footers.even_and_odd_headers.written")],
        [True, True, True, None, 2, 0, None, False],
    )

    # ── 3s) 一个框里的字装不下怎么办：pptx 写独子元素，odp 写框点的那份样式 ──────
    print("=== 3s) 框与字：a:bodyPr 的独子元素 vs 框点名的那份 graphic 样式 ===")
    for name in sorted(one.name for one in FIXTURES.glob("*.pptx")):
        got = lbin("office-slide", fixture(name))
        by_part = {one["part"]: one for one in files[name]["ooxml"]["slides"]}
        for one in got.get("slides", []):
            check("%s 每页「框与字」整份账与读者一致（独子元素名、它写的属性、框的位置尺寸）" % name,
                  one.get("autofit"), (by_part.get(one.get("part")) or {}).get("autofit"))
    for name in sorted(one.name for one in FIXTURES.glob("*.odp")):
        got = lbin("office-slide", fixture(name))
        mine = files[name]["odp"]["slides"]
        for at, one in enumerate(got.get("slides", [])):
            check("%s 第 %d 页「框与字」整份账与读者一致（一跳：框点名的样式 → 那条属性）"
                  % (name, at + 1),
                  one.get("autofit"), (mine[at] if at < len(mine) else {}).get("autofit"))
    af = lbin("office-slide", fixture("deck-autofit.pptx"))
    od = lbin("office-slide", fixture("deck-autofit.odp"))
    check(
        "pptx 三档各一条，且都写在 a:bodyPr 的独子元素上；第四档（一个子元素都没有）这份件里没有出现",
        [dig(af, "slides[0].autofit.shapes"),
         dig(af, "slides[0].autofit.says_nothing"),
         dig(af, "slides[0].autofit.no_bodyPr"),
         dig(af, "slides[0].autofit.by_element"),
         dig(af, "slides[1].autofit.by_element"),
         dig(af, "slides[0].autofit.bodyPr_total")],
        [3, 0, 0,
         {"a:noAutofit": 1, "a:spAutoFit": 1, "a:normAutofit": 1},
         {"a:spAutoFit": 1}, 3],
    )
    check(
        "算出来的缩放比按写的交：normAutofit 那一条带着 fontScale 与 lnSpcReduction，"
        "另两条一个字都不写（{} 是「看了，没说」）",
        [dig(af, "slides[0].autofit.rows[2].autofit_written"),
         dig(af, "slides[0].autofit.rows[0].autofit_written"),
         dig(af, "slides[0].autofit.rows[0].written"),
         dig(af, "slides[1].autofit.rows[0].written")],
        [{"fontScale": "75000", "lnSpcReduction": "20000"}, {},
         {"wrap": "square"}, {"wrap": "none"}],
    )
    check(
        "odp 那一跳解得开：三框各点一份 graphic 样式，那条 graphic-properties 键带着文件自己写的前缀",
        [dig(od, "slides[0].autofit.shapes"),
         dig(od, "slides[0].autofit.style_found"),
         dig(od, "slides[0].autofit.style_missing"),
         dig(od, "slides[0].autofit.props_written"),
         dig(od, "slides[0].autofit.rows[0].style"),
         dig(od, "slides[0].autofit.rows[0].props_element"),
         dig(od, "slides[0].autofit.frames")],
        [3, 3, 0, 3, "gr1", "style:graphic-properties", 1],
    )
    check(
        "LibreOffice 把 pptx 的「什么都不做」与「框随字长」写成一模一样的一条："
        "odp 这一族解不出 spAutoFit 那一档，这两行就是那件丢掉的事",
        [dig(od, "slides[0].autofit.rows[0].written")
         == dig(od, "slides[0].autofit.rows[1].written"),
         dig(od, "slides[0].autofit.rows[0].written"),
         dig(od, "slides[0].autofit.shrink_to_fit"),
         dig(od, "slides[0].autofit.fit_to_size")],
        [True,
         {"draw:fit-to-size": "false", "style:shrink-to-fit": "false", "fo:wrap-option": "wrap"},
         {"false": 2, "true": 1}, {"false": 3}],
    )
    check(
        "跨家同一个数：哪几框明说「字缩进框」（pptx 的 normAutofit / odp 的 shrink-to-fit=true）",
        [[one["shrinks_text"] for one in dig(af, "slides[0].autofit.rows")],
         [one["shrinks_text"] for one in dig(od, "slides[0].autofit.rows")],
         [dig(af, "slides[0].autofit.shrinks_text"), dig(od, "slides[0].autofit.shrinks_text")]],
        [[False, False, True], [False, False, True], [1, 1]],
    )
    check(
        "同一句问题两家的字面：会不会折行 —— pptx 写 square / none，odp 写 wrap / no-wrap，各交各的",
        [dig(af, "slides[0].autofit.rows[0].written.wrap"),
         dig(af, "slides[1].autofit.rows[0].written.wrap"),
         dig(od, "slides[0].autofit.rows[0].wrap_option"),
         dig(od, "slides[1].autofit.rows[0].wrap_option"),
         dig(od, "slides[1].autofit.wrap_option")],
        ["square", "none", "wrap", "no-wrap", {"no-wrap": 1}],
    )
    check(
        "框自己的位置尺寸：pptx 是 EMU（另给一份 0.01mm），odp 是自带单位的原样串，谁都不换算成对方",
        [dig(af, "slides[0].autofit.rows[0].off"),
         dig(af, "slides[0].autofit.rows[0].off_mm"),
         dig(od, "slides[0].autofit.rows[0].box"),
         dig(af, "slides[0].autofit.box_written"), dig(od, "slides[0].autofit.box_written")],
        [{"x": "457200", "y": "914400"}, {"x": 1270, "y": 2540},
         {"x": "1.27cm", "y": "2.54cm", "width": "6.349cm", "height": "2.539cm"}, 3, 3],
    )

    # ── 3t) 这份文档要点哪些字体：一张表、四种指针、主题那一跳 ──────────────────
    print("=== 3t) 字体：表在 fontTable / style:font-face，点它的地方两家四处 ===")
    for name in sorted(one.name for one in FIXTURES.glob("*.docx")):
        got = lbin("office-doc", fixture(name))
        check("%s 字体那份整账与读者一致（表、每一格 rFonts、主题那一跳、嵌入两本）" % name,
              dig(got, "structure.fonts"), files[name]["ooxml"]["fonts"])
    for name in sorted(one.name for one in FIXTURES.glob("*.odt")):
        got = lbin("office-doc", fixture(name))
        check("%s 字体那份整账与读者一致（face 表两种指针各数一遍，两份件都走）" % name,
              dig(got, "structure.fonts"), files[name]["odt"]["fonts"])
    fnt = lbin("office-doc", fixture("fonts.docx"))
    fntlo = lbin("office-doc", fixture("fonts-lo.docx"))
    fntodt = lbin("office-doc", fixture("fonts.odt"))
    check(
        "python-docx 一次把同一个选择写两遍（ascii 与 hAnsi）：那一框字是一格 rFonts、两条指针",
        [dig(fnt, "structure.fonts.declared_total"),
         dig(fnt, "structure.fonts.rows[0].written"),
         dig(fnt, "structure.fonts.rows[0].points"),
         dig(fnt, "structure.fonts.pointer_elements"),
         dig(fnt, "structure.fonts.rows[3].points")],
        [8, {"ascii": "Courier", "hAnsi": "Courier"},
         [{"attr": "ascii", "value": "Courier", "declared": True},
          {"attr": "hAnsi", "value": "Courier", "declared": True}],
         78, [{"attr": "eastAsia", "value": "ＭＳ 明朝", "declared": True}]],
    )
    check(
        "点了而表里没有 —— 这一问的答案是一个名：Courier New（东亚那一路点的名字在表里）",
        [dig(fnt, "structure.fonts.undeclared"),
         dig(fnt, "structure.fonts.undeclared_total"),
         dig(fnt, "structure.fonts.declared_unused_total"),
         dig(fnt, "structure.fonts.embedded_refs"),
         dig(fnt, "structure.fonts.embedded_parts")],
        [["Courier New"], 1, 6, 0, []],
    )
    check(
        "主题那一路不是字面名：minorHAnsi 要到 theme1.xml 的 minorFont/latin 才落到 Cambria，"
        "而 cstheme 那个拼法与另外三个不一致（按写的交）",
        [dig(fnt, "structure.fonts.rows[2].themes[0].value"),
         dig(fnt, "structure.fonts.rows[2].themes[0].slot"),
         dig(fnt, "structure.fonts.rows[2].themes[0].which"),
         dig(fnt, "structure.fonts.rows[2].themes[0].typeface"),
         dig(fnt, "structure.fonts.theme_refs"), dig(fnt, "structure.fonts.theme_resolved"),
         dig(fntlo, "structure.fonts.rows[3].themes[1].attr"),
         dig(fntlo, "structure.fonts.rows[3].themes[1].which"),
         dig(fntlo, "structure.fonts.rows[3].themes[1].typeface")],
        ["minorHAnsi", "minorFont", "latin", "Cambria", 290, 290,
         "cstheme", "cs", ""],
    )
    check(
        "LibreOffice 重写那份把主题就地解开了（同一条既写 ascii=Cambria 又留 asciiTheme），"
        "还把表里没的名字写进正文，另附 26 条 `cs=\"\"` —— 「写了空话」与「没写」分两笔",
        [dig(fntlo, "structure.fonts.rows[2].written"),
         dig(fntlo, "structure.fonts.empty_written"),
         dig(fntlo, "structure.fonts.undeclared"),
         dig(fntlo, "structure.fonts.declared_total"),
         dig(fntlo, "structure.fonts.pointer_elements")],
        [{"ascii": "Cambria", "hAnsi": "Cambria",
          "asciiTheme": "minorHAnsi", "hAnsiTheme": "minorHAnsi"},
         26, ["Lucida Sans", "Noto Sans SC", "ＭＳ ゴシック", "ＭＳ 明朝"], 9, 81],
    )
    check(
        "ODF 这一族的两种指针不能并成一数：按 style:font-name 全落到了表里，"
        "而按 style:font-family 有 `'Courier New'` 那一条根本没出现在族名里",
        [dig(fntodt, "structure.fonts.faces_total"),
         dig(fntodt, "structure.fonts.faces_duplicated"),
         dig(fntodt, "structure.fonts.families_quoted"),
         dig(fntodt, "structure.fonts.faces_with_charset"),
         dig(fntodt, "structure.fonts.undeclared_names"),
         dig(fntodt, "structure.fonts.undeclared_families"),
         dig(fntodt, "structure.fonts.by_font_name").get("(没写)"),
         dig(fntodt, "structure.fonts.rows[1].written")],
        [11, 11, 6, 1, [], ["'Courier New'"], 1,
         {"fo:font-family": "'Courier New'", "style:font-family-generic": "roman", "style:font-pitch": "variable"}],
    )
    check(
        "同一条族名在两张表里写法不一致是文件的事实：Cambria 与 Cambria1 指着同一个族名，"
        "只靠 style:font-charset 分开；另有一条 name=F 的族名写成空串",
        [dig(fntodt, "structure.fonts.faces[1].written"),
         dig(fntodt, "structure.fonts.faces[2].written"),
         dig(fntodt, "structure.fonts.theme")],
        [{"style:name": "Cambria", "svg:font-family": "Cambria",
          "style:font-family-generic": "roman", "style:font-pitch": "variable",
          "style:font-charset": "x-symbol"},
         {"style:name": "Cambria1", "svg:font-family": "Cambria",
          "style:font-family-generic": "roman", "style:font-pitch": "variable"},
         None],
    )

    # ── 3u) ODF 的页眉页脚在母版页上：六格、字段，与「有没有一节点它的名」───────
    print("=== 3u) 母版页那六格：写了的、没写的，与没人点的第二份母版页 ===")
    for name in sorted(one.name for one in FIXTURES.glob("*.odt")):
        got = lbin("office-doc", fixture(name))
        check("%s 母版页六格整份账与读者一致（格在不在、写的字、里面有没有自己算的）" % name,
              dig(got, "structure.header_footers"),
              files[name]["odt"]["header_footers"])
    hfodt = lbin("office-doc", fixture("notes-hf.odt"))
    hfpaper = lbin("office-doc", fixture("paper-a4.odt"))
    check(
        "两节的 docx 转成 odt 之后没有 text:section：LO 造出第二份母版页把第二节的页眉搬进去，"
        "而全文没有任何一节点过它的名 —— 那一句还在，可它「生效不生效」这份账不猜",
        [dig(hfodt, "structure.header_footers.masters_total"),
         dig(hfodt, "structure.header_footers.sections_total"),
         dig(hfodt, "structure.header_footers.masters_named_by_section"),
         dig(hfodt, "structure.header_footers.masters_unnamed"),
         dig(hfodt, "structure.header_footers.slots_written"),
         dig(hfodt, "structure.header_footers.masters[0].slots.footer:default.text"),
         dig(hfodt, "structure.header_footers.masters[1].slots.header:default.text"),
         dig(hfodt, "structure.header_footers.masters[1].used_by_sections")],
        [2, 0, 0, 2, 4, "第 1 页 / 共 3 页", "第二节的页眉不一样", []],
    )
    check(
        "格子的三种状态分得开：这一格写了（present + 写的字）、整个没有这一格（null）；"
        "页码那一路另数：`text:page-number` 在脚格里有几个",
        [dig(hfodt, "structure.header_footers.masters[0].slots.header:left"),
         dig(hfodt, "structure.header_footers.masters[0].slots.header:default.present"),
         dig(hfodt, "structure.header_footers.field_slots"),
         dig(lbin("office-doc", fixture("fields.odt")),
             "structure.header_footers.masters[0].slots.footer:default.fields"),
         dig(hfpaper, "structure.header_footers.slots_written"),
         dig(hfpaper, "structure.header_footers.masters[0].slots.header:default")],
        [None, True, 0, {"page-number": 1}, 0, None],
    )

    # ── 3v) 表格的母版页：字住在左右两半里，而没有任何一条属性把表连到页版式 ────
    print("=== 3v) .ods 的页版式六格：两半、缓存的三个问号，与没人点它的名 ===")
    for name in sorted(one.name for one in FIXTURES.glob("*.ods")):
        got = lbin("office-sheet", fixture(name))
        check("%s 页版式那六格整份账与读者一致（段住在 region 两半里也算得到）" % name,
              dig(got, "page_styles"),
              files[name]["ods"]["page_styles"])
    hfbook = lbin("office-sheet", fixture("book.ods"))
    check(
        "LibreOffice 每份 .ods 自带两份有字的母版页：`Report` 的页眉那一段**住在 "
        "style:region-left / -right 两半里**（只走直接孩子就读成 0 段、空串），"
        "`text:sheet-name` 与 `text:title` 带的是文件自己缓存的三个问号，按原样交",
        [dig(hfbook, "page_styles.masters_total"),
         dig(hfbook, "page_styles.masters[1].name"),
         dig(hfbook, "page_styles.masters[1].slots.header:default.paragraphs"),
         dig(hfbook, "page_styles.masters[1].slots.header:default.text"),
         [one["element"] for one in
          dig(hfbook, "page_styles.masters[1].slots.header:default.regions")],
         [one["paragraphs"] for one in
          dig(hfbook, "page_styles.masters[1].slots.header:default.regions")],
         dig(hfbook, "page_styles.masters[1].slots.header:default.fields"),
         dig(hfbook, "page_styles.masters[1].slots.footer:default.text")],
        [5, "Report", 2, "???(???)\n0000/00/00, 00:00:00",
         ["style:region-left", "style:region-right"], [1, 1],
         {"date": 1, "time": 1}, "页 1/ 99"],
    )
    check(
        "六格全写了而每一格都自己写着 style:display=\"false\"：这一格写了、只是它自己说不显示，"
        "与整个没有这一格（null）是两份不同的文件 —— 「所以打不打得出来」不归读者判。"
        "最后一问是这份账为什么按页版式交：全文没有一条写着的属性把某张表连到某份页版式",
        [dig(hfbook, "page_styles.masters[2].name"),
         dig(hfbook, "page_styles.masters[2].slots.header:default.present"),
         dig(hfbook, "page_styles.masters[2].slots.header:default.paragraphs"),
         dig(hfbook, "page_styles.masters[2].slots.header:default.display_written"),
         dig(hfbook, "page_styles.masters[0].slots.header:default.display_written"),
         dig(hfbook, "page_styles.masters[2].used_by_sections"),
         dig(hfbook, "page_styles.masters_named_by_section"),
         dig(hfbook, "page_styles.masters_unnamed"),
         dig(hfbook, "page_styles.sections_total"),
         dig(hfbook, "page_styles.available")],
        ["PageStyle_5f_说明", True, 0, "false", None, [], 0, 5, 0, True],
    )

    # ── 3w) 打印范围：一家写成两条保留名（序号归属），一家写成表自己身上的一条属性 ──
    print("=== 3w) 打哪几行几列、每页重复哪一行：两族两处，各自交账 ===")
    for name in sorted(one.name for one in FIXTURES.glob("*.xlsx")):
        got = lbin("office-sheet", fixture(name))
        check("%s 打印范围那份账与读者一致（保留名、序号归属、范围原样）" % name,
              dig(got, "print_ranges"),
              files[name]["ooxml"]["print_ranges"])
    for name in sorted(one.name for one in FIXTURES.glob("*.ods")):
        got = lbin("office-sheet", fixture(name))
        check("%s 打印范围那份账与读者一致（表上那条属性与 Excel 来回那一份）" % name,
              dig(got, "print_ranges"),
              files[name]["ods"]["print_ranges"])
    pa = lbin("office-sheet", fixture("print-area.xlsx"))
    pa_lo = lbin("office-sheet", fixture("print-area-lo.xlsx"))
    pa_ods = lbin("office-sheet", fixture("print-area.ods"))
    check(
        "OOXML 把这件事写成两条保留名，归属是 `localSheetId` —— 那个序号数的是 `<sheets>` 里的"
        "先后，不是 `sheetId` 也不是 `r:id`（四张表 0..3 全落得地）；一条 definedName 里可以塞"
        "两段（逗号分隔），而「只给重复列、没给区域」的那一张交 area 0 条 / titles 1 条",
        [dig(pa, "print_ranges.sheets_total"),
         dig(pa, "print_ranges.defined_total"),
         dig(pa, "print_ranges.print_entries"),
         dig(pa, "print_ranges.unresolved"),
         dig(pa, "print_ranges.by_name"),
         [one["name"] for one in dig(pa, "print_ranges.sheets")],
         dig(pa, "print_ranges.sheets[2].area_ranges"),
         dig(pa, "print_ranges.sheets[3].area_entries"),
         dig(pa, "print_ranges.sheets[3].titles_entries")],
        [4, 5, 5, 0, {"_xlnm.Print_Area": 3, "_xlnm.Print_Titles": 2},
         ["区域与标题", "区域加标题", "两段区域", "什么都没给"],
         ["'两段区域'!$A$1:$B$6", "'两段区域'!$C$8:$C$12"], 0, 1],
    )
    check(
        "同一条稿子换 LibreOffice 重写：五名字、五归属、条数一字不差，而 sheet 名的引号全没了 "
        "（5 条带引号 → 0 条）—— 引号是生产者的写法，不是文档的说法，所以两边各按各的交；"
        "「重复的是行还是列」这一家干脆看不出来（`$1:$1` 与 `$B:$B` 同一条名）",
        [dig(pa_lo, "print_ranges.print_entries"),
         dig(pa_lo, "print_ranges.quoted_entries"),
         dig(pa, "print_ranges.quoted_entries"),
         [one["text"] for one in dig(pa_lo, "print_ranges.entries")
          if one["name"] == "_xlnm.Print_Titles"],
         dig(pa_lo, "print_ranges.available")],
        [5, 0, 5,
         ["区域加标题!$1:$1", "什么都没给!$B:$B"], True],
    )
    check(
        "ODF 是两处：`table:print-ranges` 坐在表自己身上（分隔符是空白不是逗号，地址是 "
        "`表名.A1:表名.C10` 这种点号写法），另有一份为与 Excel 来回而写的 `table:named-*` —— "
        "五样里四样是 `named-range`、两段那一样是 `named-expression`（同一个选择在一种文件里"
        "两种元素），而那五样的 `base-cell-address` 全是同一个（第一张表的 A1）："
        "「这是哪张表的」只在地址串里，不在这条指针上",
        [dig(pa_ods, "print_ranges.tables_total"),
         dig(pa_ods, "print_ranges.with_print_ranges"),
         dig(pa_ods, "print_ranges.tables[2].ranges"),
         dig(pa_ods, "print_ranges.tables[3].print_ranges_written"),
         dig(pa_ods, "print_ranges.named_total"),
         dig(pa_ods, "print_ranges.named_by_element"),
         dig(pa_ods, "print_ranges.distinct_base_addresses"),
         dig(pa_ods, "print_ranges.usable_as_written")],
        [4, 3,
         ["两段区域.A1:两段区域.B6", "两段区域.C8:两段区域.C12"], None, 5,
         {"range": 4, "expression": 1}, 1,
         {"print-range": 2, "repeat-column repeat-row": 2}],
    )
    check(
        "两族能对齐的只有「几张表上有打印范围」这一问（三家都是三张），而重复行那一半在 ODF 的"
        "那条属性里根本不存在 —— 只读 `table:print-ranges` 就把「每页重复第 1 行」读丢了；"
        "没写过这件事的件交 0，不是缺键",
        [dig(pa, "print_ranges.sheets_total"),
         dig(pa_ods, "print_ranges.tables_total"),
         sum(1 for one in dig(pa, "print_ranges.sheets") if one["area_entries"] > 0),
         dig(pa_ods, "print_ranges.with_print_ranges"),
         any("$1" in one or ".1:" in one for one in dig(pa_ods, "print_ranges.tables[1].ranges")),
         dig(lbin("office-sheet", fixture("book.xlsx")), "print_ranges.print_entries"),
         dig(lbin("office-sheet", fixture("book.xlsx")), "print_ranges.defined_total"),
         dig(lbin("office-sheet", fixture("book.ods")), "print_ranges.with_print_ranges"),
         dig(lbin("office-sheet", fixture("book.xls")), "print_ranges")],
        [4, 4, 3, 3, False, 0, 1, 0, None],
    )

    # ── 3x) 页上每个框自己说的那句话：占位符的 type 按写的交，没写就是 null ──────
    print("=== 3x) 这个角色是谁说的：写了的、没写的，与两家同一个不兜 ===")

    def word_key(raw):
        return (raw is not None, raw or "")

    for name in sorted(one.name for one in FIXTURES.glob("*.pptx")):
        got = lbin("office-slide", fixture(name))
        rwant = files[name]["ooxml"]["slides"]
        mine = [one for slide in got.get("slides", []) for one in slide.get("placeholder_words", [])]
        yours = [one for slide in rwant for one in slide["placeholders"]]
        check("%s 每页各形状的角色账与读者一致（两家都不把没写的兜成 title 或 other）" % name,
              [sorted(mine, key=word_key), len(mine)],
              [sorted(yours, key=word_key), len(yours)])
    ph = lbin("office-slide", fixture("deck-ph.pptx"))
    ph_lo = lbin("office-slide", fixture("deck-ph-lo.pptx"))
    check(
        "python-pptx 的正文占位符只写 `idx=\"1\"` 而没有 `type`：这一格交 null。"
        "自制文本框连 `p:ph` 元素都没有，也交 null —— 两种「没说」在这里同一个值，"
        "而形状数另有一本账（第 2 页 3 个形状：标题、内容、文本框）",
        [dig(ph, "slides[0].placeholder_words"),
         dig(ph, "slides[1].placeholder_words"),
         dig(ph, "slides[1].shapes"),
         dig(ph, "slides[3].placeholder_words"),
         dig(ph, "slides[3].title"),
         files["deck-ph.pptx"]["ooxml"]["slides"][0]["placeholders"]],
        [["title", None], ["title", None, None], 3, [None], "", ["title", None]],
    )
    check(
        "LibreOffice 重写同一份：把 `idx` 整个丢掉（`<p:ph/>`）而角色账一字不变 —— "
        "两家在这件事上写的不是一样多，而**都不写就是都不写**；形状名换了（Title 1 → "
        "PlaceHolder 1）可那不是角色，所以四页的角色账两副件逐页相等",
        [dig(ph_lo, "slides[0].placeholder_words"),
         [dig(ph_lo, "slides[%d].placeholder_words" % index) ==
          dig(ph, "slides[%d].placeholder_words" % index) for index in range(4)],
         dig(ph_lo, "slides[0].title"),
         dig(ph_lo, "slides[0].paragraphs[0].placeholder"),
         sum(1 for slide in files["deck-ph-lo.pptx"]["ooxml"]["slides"]
             for one in slide["placeholders"] if one is None)],
        [["title", None], [True, True, True, True], "预算评审", None, 5],
    )

    # ── 3y) 占位符对版式那一跳：同一份稿子，重写之后这一跳会断 ─────────────────
    print("=== 3y) 这一框对应版式里哪一条：号写了才对得上，号丢了就断 ===")
    for name in sorted(one.name for one in FIXTURES.glob("*.pptx")):
        got = lbin("office-slide", fixture(name))
        check("%s 占位符对版式那一跳整份账与读者一致" % name,
              dig(got, "placeholder_hops"),
              files[name]["ooxml"]["placeholder_hops"])
    hops = lbin("office-slide", fixture("deck-ph.pptx"))
    hops_lo = lbin("office-slide", fixture("deck-ph-lo.pptx"))
    check(
        "python-pptx 那份：正文占位符写了 `idx=\"1\"`，版式里也写了 `idx=\"1\"` —— 六个形状"
        "全对得上（三个按号、三个按名），一条也没断",
        [dig(hops, "placeholder_hops.slide_total"),
         dig(hops, "placeholder_hops.shape_total"),
         dig(hops, "placeholder_hops.with_ph"),
         dig(hops, "placeholder_hops.hop_found"),
         dig(hops, "placeholder_hops.hop_missing"),
         dig(hops, "placeholder_hops.slides[0].layout_part"),
         [one["hop"] for one in dig(hops, "placeholder_hops.slides[0].shapes")],
         dig(hops, "placeholder_hops.slides[0].shapes[1].idx_written"),
         dig(hops, "placeholder_hops.slides[0].shapes[1].layout_matched")["idx"],
         dig(hops, "placeholder_hops.no_idx_written")],
        [4, 8, 6, 6, 0, "ppt/slideLayouts/slideLayout2.xml",
         ["by_type", "by_idx"], "1", "1", 3],
    )
    check(
        "LibreOffice 重写同一份：页上那条写成空元素 `<p:ph/>`（号与名都没有），版式那一条"
        "却写着 `type=\"body\"` —— 六个形状只对上三个，断了三条。这不是读者偷懒：按规范补一个"
        "默认值就能全「对上」，那是替文件说话，所以交 hop_missing",
        [dig(hops_lo, "placeholder_hops.hop_found"),
         dig(hops_lo, "placeholder_hops.hop_missing"),
         dig(hops_lo, "placeholder_hops.no_idx_written"),
         dig(hops_lo, "placeholder_hops.shape_total"),
         [one["hop"] for one in dig(hops_lo, "placeholder_hops.slides[0].shapes")],
         dig(hops_lo, "placeholder_hops.slides[0].shapes[1].layout_matched"),
         [one["type"] for one in dig(hops_lo, "placeholder_hops.slides[0].layout_placeholders")]],
        [3, 3, 6, 8, ["by_type", "by_type"], None,
         ["title", "body", "dt", "ftr", "sldNum"]],
    )

    # ── 3z) 每页重复哪几行：一家写成行上的一个无值元素，一家写成表身上的两个数 ──
    print("=== 3z) 这张表的哪几行每页重复：两处存法，形状不折算 ===")
    for name in sorted(one.name for one in FIXTURES.glob("*.docx")):
        got = lbin("office-doc", fixture(name))
        check("%s 表头重复那份账与读者一致（哪几行标了、几枚 trPr、连不连着第一行）" % name,
              dig(got, "structure.table_headers"),
              files[name]["ooxml"]["table_headers"])
    for name in sorted(one.name for one in FIXTURES.glob("*.odt")):
        got = lbin("office-doc", fixture(name))
        check("%s 表头重复那份账与读者一致（表身上那四个属性按写的交，没写是 null）" % name,
              dig(got, "structure.table_headers"),
              files[name]["odt"]["table_headers"])
    th = lbin("office-doc", fixture("table-header.docx"))
    th_lo = lbin("office-doc", fixture("table-header-lo.docx"))
    th_odt = lbin("office-doc", fixture("table-header.odt"))
    check(
        "python-docx 那份：四张表里三张标了、一共 4 行，而其中一张标在**第二行**上 "
        "（`contiguous_from_first` 因此是 false）—— 「哪几行」只能逐行交，只数一个总数就把"
        "这件事抹平了；`tr_pr_elements` 与标记的行数也不是一回事（第三张 0 枚、第四张 1 枚却没标第一行）",
        [dig(th, "structure.table_headers.tables_total"),
         dig(th, "structure.table_headers.marked"),
         dig(th, "structure.table_headers.header_rows_total"),
         dig(th, "structure.table_headers.non_leading"),
         [one["header_rows"] for one in dig(th, "structure.table_headers.tables")],
         [one["tr_pr_elements"] for one in dig(th, "structure.table_headers.tables")],
         [one["contiguous_from_first"] for one in dig(th, "structure.table_headers.tables")]],
        [4, 3, 4, 1,
         [[True, False, False], [True, True, False],
          [False, False, False], [False, True, False]],
         [1, 2, 0, 1], [True, True, False, False]],
    )
    check(
        "LibreOffice 重写同一份：那枚「不是从第一行起」的标记整个不见了（4 行 → 3 行、非 leading "
        "1 张 → 0 张），而它给**每一行**都补了一枚**空的** `w:trPr`（1/2/0/1 → 3/3/3/3）—— "
        "「有几枚 trPr」不能当「有几行是表头」用，这两本账必须分开交",
        [dig(th_lo, "structure.table_headers.marked"),
         dig(th_lo, "structure.table_headers.header_rows_total"),
         dig(th_lo, "structure.table_headers.non_leading"),
         [one["tr_pr_elements"] for one in dig(th_lo, "structure.table_headers.tables")],
         [one["header_rows"] for one in dig(th_lo, "structure.table_headers.tables")]],
        [2, 3, 0, [3, 3, 3, 3],
         [[True, False, False], [True, True, False],
          [False, False, False], [False, False, False]]],
    )
    check(
        "同一条稿子转成 odt：表身上那四个属性**一个都没写** —— 交的是 null（这份文件没说），"
        "不是 0（那才是「说了不重复」）。这里所有 .odt 一件都没写过这件事，所以 ODF 那一支"
        "只证得到「按写的交、不替文件兜」，那张网是另一本账",
        [dig(th_odt, "structure.table_headers.tables_total"),
         dig(th_odt, "structure.table_headers.with_header_rows"),
         dig(th_odt, "structure.table_headers.with_repeated"),
         [one["header_rows"] for one in dig(th_odt, "structure.table_headers.tables")],
         [one["header_rows_repeated"] for one in dig(th_odt, "structure.table_headers.tables")],
         [one["name"] for one in dig(th_odt, "structure.table_headers.tables")]],
        [4, 0, 0, [None, None, None, None], [None, None, None, None],
         ["表格1", "表格2", "表格3", "表格4"]],
    )
    check(
        "两族能对上的只有「几张表」这一问（同一份稿子两份都是 4 张），形状本身不折算；"
        "没写过这件事的件交 0 与空数组，不是缺键，而 RTF 这一族**没读**（它写 `\\trhdr`，"
        "这一支还没做）—— 缺键就是「这一族没看」",
        [dig(th, "structure.table_headers.tables_total"),
         dig(th_odt, "structure.table_headers.tables_total"),
         dig(th, "structure.table_headers.available"),
         dig(lbin("office-doc", fixture("paper-a4.docx")),
             "structure.table_headers.tables_total"),
         dig(lbin("office-doc", fixture("paper-a4.docx")), "structure.table_headers.marked"),
         dig(lbin("office-doc", fixture("paper-a4.docx")), "structure.table_headers.tables"),
         dig(lbin("office-doc", fixture("notes.odt")),
             "structure.table_headers.tables_total"),
         dig(lbin("office-doc", fixture("notes.rtf")), "structure.table_headers")],
        [4, 4, True, 0, 0, [], 1, None],
    )

    # ── 3aa) 制表位：一家写在段上，一家一跳在段点的样式里，而位置是两个不同的串 ──
    print("=== 3aa) 这一段上有哪几个制表位：两处存法，定义与字符两本账 ===")
    for name in sorted(one.name for one in FIXTURES.glob("*.docx")):
        got = lbin("office-doc", fixture(name))
        check("%s 制表位那份账与读者一致（段上的定义与 run 里的字符）" % name,
              dig(got, "structure.tab_stops"),
              files[name]["ooxml"]["tab_stops"])
    for name in sorted(one.name for one in FIXTURES.glob("*.odt")):
        got = lbin("office-doc", fixture(name))
        check("%s 制表位那份账与读者一致（一跳在样式里，default-style 另交一份）" % name,
              dig(got, "structure.tab_stops"),
              files[name]["odt"]["tab_stops"])
    tb = lbin("office-doc", fixture("tabs.docx"))
    tbo = lbin("office-doc", fixture("tabs.odt"))
    check(
        "python-docx 那份（四条段各改一个变量：左无引导 / 右点引导 / 居中划引导 / 小数点）："
        "定义 8 条、制表字符也 8 个，两本账分开交；`w:val` 八条全写了，而 `w:leader` 有 2 条"
        "整个属性不落（生产者那一档叫 `SPACES`）—— 没写不等于「无引导」，那是规范的默认值，"
        "所以这一格交 null",
        [dig(tb, "structure.tab_stops.paragraphs_total"),
         dig(tb, "structure.tab_stops.with_stops"),
         dig(tb, "structure.tab_stops.stops_total"),
         dig(tb, "structure.tab_stops.tab_chars_total"),
         dig(tb, "structure.tab_stops.stops_without_val"),
         dig(tb, "structure.tab_stops.stops_without_leader"),
         dig(tb, "structure.tab_stops.vals_written"),
         dig(tb, "structure.tab_stops.leaders_written"),
         dig(tb, "structure.tab_stops.distinct_positions")],
        [5, 4, 8, 8, 0, 2,
         {"left": 5, "right": 1, "center": 1, "decimal": 1},
         {"underscore": 4, "dot": 1, "hyphen": 1}, ["1701", "5102"]],
    )
    check(
        "同一条稿子转 ODF：那四个数一模一样（5 / 4 / 8 / 8），可位置换成两个带单位的串，而"
        "**9cm 写成 `8.999cm`**（转一趟少 0.001cm —— 谁换算的谁负责，读者不替它平），"
        "对齐那一格有 5 条没写（左对齐这一家压根不写），「引导符」是 `leader-style` 与 "
        "`leader-text` **两个**属性合起来的，小数点那一样换成 `type=char` 配 `char=.` —— "
        "与 OOXML 的 `decimal` 是两种说法，不折算",
        [dig(tbo, "structure.tab_stops.paragraphs_total"),
         dig(tbo, "structure.tab_stops.with_stops"),
         dig(tbo, "structure.tab_stops.stops_total"),
         dig(tbo, "structure.tab_stops.tab_chars_total"),
         dig(tbo, "structure.tab_stops.distinct_positions"),
         dig(tbo, "structure.tab_stops.stops_without_type"),
         dig(tbo, "structure.tab_stops.types_written"),
         dig(tbo, "structure.tab_stops.leader_styles_written"),
         dig(tbo, "structure.tab_stops.paragraphs[4].stops[0]")],
        [5, 4, 8, 8, ["3cm", "8.999cm"], 5,
         {"right": 1, "center": 1, "char": 1}, {"solid": 5, "dotted": 1},
         {"position": "3cm", "type": "char", "char": ".",
          "leader_style": None, "leader_text": None}],
    )
    check(
        "ODF 这一问要走一跳才看得见：段自己只写一个样式名（`P1`…`P4`，父名 `Standard`），"
        "而这份件里 44 份具名段落样式有 7 份写了制表位 —— 其中 3 份**没有任何段点它**"
        "（`Header` / `Footer` / `macro`：前两份是页眉页脚自己的样式，第三份没人用）；"
        "`style:default-style` 没有名字可点而照样落到每一段上，所以单独交一份（这里是空的）",
        [dig(tbo, "structure.tab_stops.styles_total"),
         dig(tbo, "structure.tab_stops.styles_with_stops"),
         dig(tbo, "structure.tab_stops.unpointed_styles"),
         dig(tbo, "structure.tab_stops.default_style_stops"),
         dig(tbo, "structure.tab_stops.paragraphs[1].style_written"),
         dig(tbo, "structure.tab_stops.paragraphs[1].style_found"),
         dig(tbo, "structure.tab_stops.paragraphs[1].parent_style_written"),
         dig(tbo, "structure.tab_stops.paragraphs[0].style_written"),
         dig(tbo, "structure.tab_stops.paragraphs[0].stops")],
        [44, 7, ["Header", "Footer", "macro"], [], "P1", True, "Standard", "Standard", []],
    )
    check(
        "两族能对齐的就是那四个数，形状与单位不对齐；有段而一条没定义的件交 0 与空数组，"
        "不是缺键。第三家见 3ab：RTF 也写这份账，位置又回到 twip",
        [dig(tb, "structure.tab_stops.paragraphs_total"),
         dig(tbo, "structure.tab_stops.paragraphs_total"),
         dig(tb, "structure.tab_stops.distinct_positions"),
         dig(tbo, "structure.tab_stops.distinct_positions"),
         dig(lbin("office-doc", fixture("paper-a4.docx")), "structure.tab_stops.with_stops"),
         dig(lbin("office-doc", fixture("paper-a4.docx")), "structure.tab_stops.stops_total"),
         dig(lbin("office-doc", fixture("paper-a4.odt")), "structure.tab_stops.paragraphs_total"),
         dig(lbin("office-doc", fixture("paper-a4.odt")), "structure.tab_stops.styles_with_stops")],
        [5, 5, ["1701", "5102"], ["3cm", "8.999cm"], 0, 0, 2, 3],
    )

    # ── 3ab) 第三家：RTF 把制表位写成一条扁平流，前缀只管紧跟的那一个位置 ─────────
    print("=== 3ab) RTF 的制表位：`\\tx` 逐条与前缀配对 ===")
    for name in sorted(one.name for one in FIXTURES.glob("*.rtf")):
        got = lbin("office-doc", fixture(name))
        check("%s 制表位那份账与读者一致（位置、前缀、制表字符各一本）" % name,
              dig(got, "structure.tab_stops"),
              files[name]["rtf"]["tab_stops"])
    tbr = lbin("office-doc", fixture("tabs.rtf"))
    check(
        "同一条稿子的第三种存法：八个位置、八个制表字符，与前两**同一个数**，而位置又回到 "
        "twip（`1701` / `5102` —— 与 OOXML 同一种单位，ODF 那个 `8.999cm` 是另一家的写法）；"
        "对齐前缀只有 3 条（左对齐这一族不写词），引导前缀 6 条 —— 四个 9cm 那一条共用一个 "
        "`\\tlul`，而「前缀比位置多/少」正是这一族的形状，所以两个数各交各的",
        [dig(tbr, "structure.tab_stops.positions"),
         dig(tbr, "structure.tab_stops.chars"),
         dig(tbr, "structure.tab_stops.without_position"),
         dig(tbr, "structure.tab_stops.align_words"),
         dig(tbr, "structure.tab_stops.leader_words"),
         [one["position_written"] for one in dig(tbr, "structure.tab_stops.rows")],
         [one["align_written"] for one in dig(tbr, "structure.tab_stops.rows")],
         [one["leader_written"] for one in dig(tbr, "structure.tab_stops.rows")]],
        [8, 8, 0, 3, 6,
         ["1701", "5102", "1701", "5102", "1701", "5102", "1701", "5102"],
         [None, None, "tqr", None, "tqc", None, "tqdec", None],
         [None, "tlul", "tldot", "tlul", "tlth", "tlul", None, "tlul"]],
    )
    check(
        "三家答同一个问的三份凭据（一条稿子、三种写法）：位置 8 条与制表字符 8 个三家都一样，"
        "而「对齐」这件事三家各有词表 —— OOXML `w:val=left/right/center/decimal`、"
        "ODF `style:type=right/center/char`（左不写）、RTF `\\tqr/\\tqc/\\tqdec`（左不写）；"
        "有制表字符而一个位置没定义的件也真存在（`lists.rtf`：5 个字符、0 条定义）",
        [dig(tb, "structure.tab_stops.stops_total"),
         dig(tbo, "structure.tab_stops.stops_total"),
         dig(tbr, "structure.tab_stops.positions"),
         dig(tb, "structure.tab_stops.tab_chars_total"),
         dig(tbo, "structure.tab_stops.tab_chars_total"),
         dig(tbr, "structure.tab_stops.chars"),
         dig(lbin("office-doc", fixture("lists.rtf")), "structure.tab_stops.chars"),
         dig(lbin("office-doc", fixture("lists.rtf")), "structure.tab_stops.positions"),
         dig(lbin("office-doc", fixture("tables.rtf")), "structure.tab_stops.positions")],
        [8, 8, 8, 8, 8, 8, 5, 0, 0],
    )

    # ── 3ac) 文档里那几条批注：内容在部件、锚点在正文，两边按号配 ───────────────
    print("=== 3ac) 这几条批注是谁写的、锚在哪一段：两族两处，两个方向都数 ===")
    for name in sorted(one.name for one in FIXTURES.glob("*.docx")):
        got = lbin("office-doc", fixture(name))
        check("%s 批注那份账与读者一致（部件里的内容、正文里的三个锚点）" % name,
              dig(got, "structure.comment_ledger"),
              files[name]["ooxml"]["comment_ledger"])
    for name in sorted(one.name for one in FIXTURES.glob("*.odt")):
        got = lbin("office-doc", fixture(name))
        check("%s 批注那份账与读者一致（批注坐在段里面，作者是孩子元素）" % name,
              dig(got, "structure.comment_ledger"),
              files[name]["odt"]["comment_ledger"])
    cm = lbin("office-doc", fixture("doc-comments.docx"))
    cm_lo = lbin("office-doc", fixture("doc-comments-lo.docx"))
    cm_odt = lbin("office-doc", fixture("doc-comments.odt"))
    check(
        "python-docx 那份（四条段、三条批注，前两条锚在同一段）：内容与锚点分两处，"
        "按 `w:id` 配上 —— 三个锚点种类各数一遍（`commentRangeStart` / `End` / `commentReference`），"
        "而「哪一段指着哪条」是正文那一路的答案：第 0 段带了 **两条**（号 1 在前、号 0 在后）",
        [dig(cm, "structure.comment_ledger.comments_total"),
         dig(cm, "structure.comment_ledger.part_written"),
         dig(cm, "structure.comment_ledger.anchor_starts"),
         dig(cm, "structure.comment_ledger.anchor_ends"),
         dig(cm, "structure.comment_ledger.anchor_references"),
         dig(cm, "structure.comment_ledger.range_asymmetric"),
         dig(cm, "structure.comment_ledger.orphans_without_anchor"),
         dig(cm, "structure.comment_ledger.anchors_without_comment"),
         dig(cm, "structure.comment_ledger.distinct_authors"),
         dig(cm, "structure.comment_ledger.comments[0]"),
         dig(cm, "structure.comment_ledger.hosts[0]")],
        [3, True, 3, 3, 3, False, 0, 0, ["刘奇", "审稿人", "编辑"],
         {"part_index": 0, "id": "0", "author": "刘奇", "initials": "LQ",
          "date": "2026-09-25T05:45:49Z", "paragraphs": 1,
          "text": "第一条批注：请核对数字"},
         {"paragraph": 0, "ids": ["1", "0"]}],
    )
    check(
        "LibreOffice 重写同一份：三个数与六个数一字不差，而 `comments.xml` 里那三条的**先后**"
        "换了（部件第一条现在是号 1 的那条、作者是审稿人），正文里的九个锚点没动 —— "
        "所以「第几条批注」不说清是按部件还是按正文，就是两个不同的答案，两份都交",
        [dig(cm_lo, "structure.comment_ledger.comments_total"),
         dig(cm_lo, "structure.comment_ledger.anchor_references"),
         dig(cm_lo, "structure.comment_ledger.distinct_authors"),
         dig(cm_lo, "structure.comment_ledger.comments[0].id"),
         dig(cm_lo, "structure.comment_ledger.comments[0].author"),
         dig(cm_lo, "structure.comment_ledger.comments[1].id"),
         dig(cm_lo, "structure.comment_ledger.hosts[0]"),
         dig(cm_lo, "structure.comment_ledger.orphans_without_anchor"),
         dig(cm_lo, "structure.comment_ledger.anchors_without_comment")],
        [3, 3, ["审稿人", "刘奇", "编辑"], "1", "审稿人", "0",
         {"paragraph": 0, "ids": ["1", "0"]}, 0, 0],
    )
    check(
        "同一问在 ODF 是一处：`text:annotation` 就坐在它所属的那一段里，作者是孩子元素 "
        "`dc:creator`、时间是 `dc:date` —— 而那个时间**没有 Z**（OOXML 那份写 `…Z`），"
        "时区是文件自己写的，不替它补。「全文几段」在这一族是两个数：6 个 `text:p` 里 "
        "3 个住在批注里，正文只有 3 段",
        [dig(cm_odt, "structure.comment_ledger.annotations_total"),
         dig(cm_odt, "structure.comment_ledger.paragraphs_total"),
         dig(cm_odt, "structure.comment_ledger.paragraphs_in_annotations"),
         dig(cm_odt, "structure.comment_ledger.paragraphs_body_only"),
         dig(cm_odt, "structure.comment_ledger.hosted_in"),
         dig(cm_odt, "structure.comment_ledger.distinct_creators"),
         dig(cm_odt, "structure.comment_ledger.annotations[0].date"),
         dig(cm_odt, "structure.comment_ledger.annotations[0].host_paragraph"),
         dig(cm_odt, "structure.comment_ledger.annotations[1].creator")],
        [3, 6, 3, 3, 2, ["刘奇", "审稿人", "编辑"],
         "2026-09-25T05:45:49", 0, "审稿人"],
    )
    check(
        "两家能对齐的是条数与作者数（3 / 3）；没写过批注的件交 `part_written: false` 与一串 0"
        "（不是缺键），有批注而没人锚的份也存在（`notes.docx` 一条、作者 `liuqi`）；"
        "RTF 那一族这一格不交（它的批注另有 `annotations` 那一本账，带作者、日期与字）",
        [dig(cm, "structure.comment_ledger.comments_total"),
         dig(cm_odt, "structure.comment_ledger.annotations_total"),
         dig(cm, "structure.comment_ledger.distinct_authors") ==
         dig(cm_odt, "structure.comment_ledger.distinct_creators"),
         dig(lbin("office-doc", fixture("paper-a4.docx")),
             "structure.comment_ledger.part_written"),
         dig(lbin("office-doc", fixture("paper-a4.docx")),
             "structure.comment_ledger.comments_total"),
         dig(lbin("office-doc", fixture("paper-a4.docx")),
             "structure.comment_ledger.anchor_references"),
         dig(lbin("office-doc", fixture("notes.docx")), "structure.comment_ledger.comments_total"),
         dig(lbin("office-doc", fixture("notes.docx")), "structure.comment_ledger.distinct_authors"),
         dig(lbin("office-doc", fixture("tabs.rtf")), "structure.comment_ledger")],
        [3, 3, True, False, 0, 0, 1, ["liuqi"], None],
    )

    # ── 3ad) 这一段与下一页怎么接：一家坐在段上（元素在场），一家一跳在样式（且是两个数） ──
    print("=== 3ad) 分页那四个开关：在场不等于开着，一跳不等于没有 ===")
    for name in sorted(one.name for one in FIXTURES.glob("*.docx")):
        got = lbin("office-doc", fixture(name))
        check("%s 分页开关那份账与读者一致（在场、写的值、算出来的状态）" % name,
              dig(got, "structure.keep_switches"),
              files[name]["ooxml"]["keep_switches"])
    for name in sorted(one.name for one in FIXTURES.glob("*.odt")):
        got = lbin("office-doc", fixture(name))
        check("%s 分页开关那份账与读者一致（一跳在段点的那份样式上）" % name,
              dig(got, "structure.keep_switches"),
              files[name]["odt"]["keep_switches"])
    kp = lbin("office-doc", fixture("keep.docx"))
    kp_lo = lbin("office-doc", fixture("keep-lo.docx"))
    kp_odt = lbin("office-doc", fixture("keep.odt"))
    check(
        "python-docx 那份（五段各开一个开关）：前三枚写出来是**空元素**（在场=开着，值整个没有），"
        "第四枚反过来写 `w:val=0` 表示关 —— 所以 `present` / `val` / 算出来的 `on_written`、"
        "`off_written` 四个键都要交，把「在场」当「开着」就会把那枚关着的数成开着。"
        "基线那段连 `w:pPr` 都没有（`has_pPr` false，不是「写了 false」）",
        [dig(kp, "structure.keep_switches.paragraphs_total"),
         dig(kp, "structure.keep_switches.p_pr_elements"),
         dig(kp, "structure.keep_switches.paragraphs_with_any"),
         dig(kp, "structure.keep_switches.paragraphs_indexed"),
         dig(kp, "structure.keep_switches.states_written"),
         dig(kp, "structure.keep_switches.paragraphs[0].has_pPr"),
         dig(kp, "structure.keep_switches.paragraphs[1].keep_next"),
         dig(kp, "structure.keep_switches.paragraphs[4].widow_control"),
         dig(kp, "structure.keep_switches.paragraphs[3].page_break_before")],
        [5, 4, 4, [1, 2, 3, 4],
         {"keepNext bare": 1, "keepLines bare": 1, "pageBreakBefore bare": 1,
          "widowControl with_value": 1},
         False,
         {"present": True, "val": None, "on_written": True, "off_written": False},
         {"present": True, "val": "0", "on_written": False, "off_written": True},
         {"present": True, "val": None, "on_written": True, "off_written": False}],
    )
    check(
        "LibreOffice 重写同一份：每段都被补了一个 `w:pPr`（4 枚 → 5 枚），值换成 true/false 这一种"
        "拼法（`keepNext` 现在写着 true、`widowControl` 写着 false），而**带 `w:pageBreakBefore` "
        "的那一段整个不再有这一格** —— 交着开关的段从 4 段掉到 3 段（第 3 段那一格没了）",
        [dig(kp_lo, "structure.keep_switches.p_pr_elements"),
         dig(kp_lo, "structure.keep_switches.paragraphs_with_any"),
         dig(kp_lo, "structure.keep_switches.paragraphs_indexed"),
         dig(kp_lo, "structure.keep_switches.states_written"),
         dig(kp_lo, "structure.keep_switches.paragraphs[1].keep_next"),
         dig(kp_lo, "structure.keep_switches.paragraphs[4].widow_control"),
         sum(1 for one in dig(kp_lo, "structure.keep_switches.paragraphs")
             if one["page_break_before"]["present"])],
        [5, 3, [1, 2, 4],
         {"keepNext with_value": 1, "keepLines bare": 1, "widowControl with_value": 1},
         {"present": True, "val": "true", "on_written": True, "off_written": False},
         {"present": True, "val": "false", "on_written": False, "off_written": True},
         0],
    )
    check(
        "同一问转 ODF：段身上一个字都没写，四枚开关一跳在段点的样式上（`P1` keep-with-next=always、"
        "`P2` keep-together=always、`P3` break-before=page、`P4` widows=0 配 orphans=0）——"
        "**孤行控制在这一族是两个数，不是一枚开关**；而基线那段点的 `Standard` 自己写着 2/2，"
        "于是「有开关的段」是 5 段（比 OOXML 那份的 4 还多），这两个数不能互相对账",
        [dig(kp_odt, "structure.keep_switches.paragraphs_total"),
         dig(kp_odt, "structure.keep_switches.resolved"),
         dig(kp_odt, "structure.keep_switches.paragraphs_with_any"),
         dig(kp_odt, "structure.keep_switches.styles_total"),
         dig(kp_odt, "structure.keep_switches.words_written"),
         dig(kp_odt, "structure.keep_switches.paragraphs[0].widows"),
         dig(kp_odt, "structure.keep_switches.paragraphs[1].keep_with_next"),
         dig(kp_odt, "structure.keep_switches.paragraphs[3].break_before"),
         [dig(kp_odt, "structure.keep_switches.paragraphs[4].widows.written"),
          dig(kp_odt, "structure.keep_switches.paragraphs[4].orphans.written")]],
        [5, 5, 5, 44,
         {"widows=2": 1, "orphans=2": 1, "keep-with-next=always": 1,
          "keep-together=always": 1, "break-before=page": 1, "widows=0": 1, "orphans=0": 1},
         {"written": "2", "present": True},
         {"written": "always", "present": True},
         {"written": "page", "present": True},
         ["0", "0"]],
    )
    check(
        "两族形状不同：同一份稿子「有开关的段」是 4（OOXML）与 5（ODF，因为样式自己写了默认值），"
        "单位与词表也不同（`w:val=0` 对 `fo:widows=0`），所以各交各的不折算；RTF 那一族"
        "**不交这个键** —— 它写 `\\keepn` 与 `\\nowidctlpar`，可实测 11 条 `\\keepn` 里只有 1 条"
        "落在正文段上、其余在样式表里（归属判不住），缺键就是「这一族没看」",
        [dig(kp, "structure.keep_switches.paragraphs_with_any"),
         dig(kp_odt, "structure.keep_switches.paragraphs_with_any"),
         dig(kp, "structure.keep_switches.paragraphs_with_any")
         != dig(kp_odt, "structure.keep_switches.paragraphs_with_any"),
         dig(lbin("office-doc", fixture("keep.docx")),
             "structure.keep_switches.paragraphs[0].keep_next.present"),
         dig(lbin("office-doc", fixture("tabs.rtf")), "structure.keep_switches")],
        [4, 5, True, False, None],
    )

    # ── 3ae) 这张表套的是哪个样式：一家的样式 id 与那枚缓存值是两本账 ─────────────
    print("=== 3ae) 表样式：样式 id、那枚 tblLook 的六个位与一个缓存值 ===")
    for name in sorted(one.name for one in FIXTURES.glob("*.docx")):
        got = lbin("office-doc", fixture(name))
        check("%s 表样式那份账与读者一致（样式 id 与 tblLook 各按写的交）" % name,
              dig(got, "structure.table_styles"),
              files[name]["ooxml"]["table_styles"])
    for name in sorted(one.name for one in FIXTURES.glob("*.odt")):
        got = lbin("office-doc", fixture(name))
        check("%s 表样式那份账与读者一致（只有一个名字，look 在这一族不存在）" % name,
              dig(got, "structure.table_styles"),
              files[name]["odt"]["table_styles"])
    ts = lbin("office-doc", fixture("table-style.docx"))
    ts_lo = lbin("office-doc", fixture("table-style-lo.docx"))
    ts_odt = lbin("office-doc", fixture("table-style.odt"))
    check(
        "python-docx 那份（四张表各改一个变量）：只有两张写了样式 id，四张都带着 `w:tblLook`；"
        "第三张把 `w:firstRow` 改成 0 之后**那个十六进制缓存没重算**（还是 `04A0`）—— "
        "两本账都按写的交，不拿一个去修另一个",
        [dig(ts, "structure.table_styles.tables_total"),
         dig(ts, "structure.table_styles.with_style_written"),
         dig(ts, "structure.table_styles.with_look"),
         dig(ts, "structure.table_styles.distinct_styles"),
         dig(ts, "structure.table_styles.tables[2].look_written.firstRow"),
         dig(ts, "structure.table_styles.tables[2].look_written.val"),
         dig(ts, "structure.table_styles.tables[3].style_written")],
        [4, 2, 4, ["LightGrid-Accent1"], "0", "04A0", None],
    )
    check(
        "LibreOffice 重写同一份：四张表的样式与那六个位一个没变，而它**把缓存值重算了** —— 第三张的 "
        "`04A0` 变成 `0480`（首行那位清掉了），另外几张也从大写换成小写（`04A0` → `04a0`）—— "
        "写法与算过的结果都是文件自己的话，两份读者只照着交",
        [dig(ts_lo, "structure.table_styles.with_style_written"),
         dig(ts_lo, "structure.table_styles.tables[2].look_written.val"),
         dig(ts_lo, "structure.table_styles.tables[0].look_written.val"),
         dig(ts_lo, "structure.table_styles.tables[2].look_written.firstRow"),
         dig(ts, "structure.table_styles.tables[0].look_written.val")
         != dig(ts_lo, "structure.table_styles.tables[0].look_written.val")],
        [2, "0480", "04a0", "0", True],
    )
    check(
        "转成 ODF 之后这个问只剩一个名字：四张表各点一份自动样式（`表格1`…`表格4`，都找得到、"
        "都没有父样式），而 OOXML 那个 `LightGrid-Accent1` 在这一族的账本里**看不见** —— "
        "样式那一路的信息在这一转里丢了，交看到的，不替它认回来",
        [dig(ts_odt, "structure.table_styles.tables_total"),
         dig(ts_odt, "structure.table_styles.with_style_written"),
         dig(ts_odt, "structure.table_styles.style_found_total"),
         dig(ts_odt, "structure.table_styles.styles_total"),
         [one["style_written"] for one in dig(ts_odt, "structure.table_styles.tables")],
         [one["parent_style_written"] for one in dig(ts_odt, "structure.table_styles.tables")],
         any("look_written" in one for one in dig(ts_odt, "structure.table_styles.tables")),
         "LightGrid" in json.dumps(dig(ts_odt, "structure.table_styles"), ensure_ascii=False)],
        [4, 4, 4, 4, ["表格1", "表格2", "表格3", "表格4"], [None, None, None, None], False, False],
    )
    check(
        "两族能对齐的只有「几张表」这一问（同一份稿子两份都是 4 张）；没套样式的件交 0 与空数组"
        "而不是缺键（`tables.docx` 两张表都有 `w:tblLook`，却一张样式名都没写），"
        "RTF 那一族不交这个键（缺键 = 这一族没看）",
        [dig(ts, "structure.table_styles.tables_total"),
         dig(ts_odt, "structure.table_styles.tables_total"),
         dig(lbin("office-doc", fixture("tables.docx")),
             "structure.table_styles.with_style_written"),
         dig(lbin("office-doc", fixture("tables.docx")), "structure.table_styles.with_look"),
         dig(lbin("office-doc", fixture("tables.docx")), "structure.table_styles.distinct_styles"),
         dig(lbin("office-doc", fixture("tables.odt")),
             "structure.table_styles.style_found_total"),
         dig(lbin("office-doc", fixture("tabs.rtf")), "structure.table_styles")],
        [4, 4, 0, 2, [], 2, None],
    )

    # ── 3af) 这一段的行距：那个数的单位由谁说了算，两家两种答法 ───────────────────
    print("=== 3af) 行距：`w:line` 的单位由 `w:lineRule` 决定，ODF 把单位写在串上 ===")
    for name in sorted(one.name for one in FIXTURES.glob("*.docx")):
        got = lbin("office-doc", fixture(name))
        check("%s 行距那份账与读者一致（那个数与它的单位分开交）" % name,
              dig(got, "structure.line_spacing"),
              files[name]["ooxml"]["line_spacing"])
    for name in sorted(one.name for one in FIXTURES.glob("*.odt")):
        got = lbin("office-doc", fixture(name))
        check("%s 行距那份账与读者一致（一跳在样式里，单位在串上）" % name,
              dig(got, "structure.line_spacing"),
              files[name]["odt"]["line_spacing"])
    ls = lbin("office-doc", fixture("line.docx"))
    ls_lo = lbin("office-doc", fixture("line-lo.docx"))
    ls_odt = lbin("office-doc", fixture("line.odt"))
    check(
        "python-docx 那份（五段各改一个变量）：四条写了数、四条写了单位，两条各占一种 —— "
        "`auto` 两条（1.5 倍、2 倍）、`exact` 与 `atLeast` 各一条；段零那个格整块没写（null 不是 0）",
        [dig(ls, "structure.line_spacing.paragraphs_total"),
         dig(ls, "structure.line_spacing.with_line_written"),
         dig(ls, "structure.line_spacing.with_rule_written"),
         dig(ls, "structure.line_spacing.with_both"),
         dig(ls, "structure.line_spacing.rules_written"),
         [one["line_written"] for one in dig(ls, "structure.line_spacing.paragraphs")],
         [one["rule_written"] for one in dig(ls, "structure.line_spacing.paragraphs")],
         dig(ls, "structure.line_spacing.paragraphs[0].has_spacing")],
        [5, 4, 4, 4, {"auto": 2, "exact": 1, "atLeast": 1},
         [None, "360", "480", "440", "360"],
         [None, "auto", "auto", "exact", "atLeast"], False],
    )
    check(
        "「1.5 倍」与「至少 18 磅」在文件里是**同一个数**（都是 `360`）：只交那个数就把两种单位"
        "读成一种，分开交才看得见是 `lineRule` 在选单位 —— 这一条是这份样本存在的理由",
        [dig(ls, "structure.line_spacing.paragraphs[1].line_written")
         == dig(ls, "structure.line_spacing.paragraphs[4].line_written"),
         dig(ls, "structure.line_spacing.paragraphs[1].rule_written"),
         dig(ls, "structure.line_spacing.paragraphs[4].rule_written")],
        [True, "auto", "atLeast"],
    )
    check(
        "LibreOffice 重写同一份：四个数与其单位一个都没改口，而段零被补了一份 `w:pPr`"
        "（里面**没有** `w:spacing`）—— 补壳子不补内容，两样都得按写的交",
        [dig(ls_lo, "structure.line_spacing.with_line_written"),
         dig(ls_lo, "structure.line_spacing.rules_written"),
         dig(ls_lo, "structure.line_spacing.paragraphs[3].line_written"),
         dig(ls_lo, "structure.line_spacing.paragraphs[4].rule_written"),
         dig(ls_lo, "structure.line_spacing.paragraphs[0].has_pPr"),
         dig(ls_lo, "structure.line_spacing.paragraphs[0].has_spacing")],
        [4, {"auto": 2, "exact": 1, "atLeast": 1}, "440", "atLeast", True, False],
    )
    check(
        "转成 ODF 后同一问跳一跳在样式里，单位换成写在串上的：1.5 倍 → `150%`、2 倍 → `200%`、"
        "22 磅 → `0.776cm`（长度串，不换算不约分），而 `atLeast` 那一段**四个相关属性一个都没写** —— "
        "这是转换丢的，交看到的、不替它接回去",
        [dig(ls_odt, "structure.line_spacing.paragraphs_total"),
         dig(ls_odt, "structure.line_spacing.with_line_height"),
         dig(ls_odt, "structure.line_spacing.unit_forms"),
         [one["line_height_written"] for one in dig(ls_odt, "structure.line_spacing.paragraphs")],
         [one["line_height_style"] for one in dig(ls_odt, "structure.line_spacing.paragraphs")]],
        [5, 4, {"%": 3, "cm": 1}, ["115%", "150%", "200%", "0.776cm", None],
         [None, None, None, None, None]],
    )
    check(
        "两族对同一个问的答法不同到**连「没写」都不对应**：docx 段零那个格整块没写，"
        "odt 段零点的 `Standard` 样式里却写着 `115%` —— 同一份稿子两个答案，谁也不替谁圆；"
        "没写行距的件交 0 与空表而不是缺键，RTF 那一族不交这个键（缺键 = 这一族没看）",
        [dig(ls, "structure.line_spacing.paragraphs[0].line_written"),
         dig(ls_odt, "structure.line_spacing.paragraphs[0].line_height_written"),
         dig(lbin("office-doc", fixture("keep.docx")),
             "structure.line_spacing.with_line_written"),
         dig(lbin("office-doc", fixture("keep.docx")), "structure.line_spacing.rules_written"),
         dig(lbin("office-doc", fixture("tabs.rtf")), "structure.line_spacing")],
        [None, "115%", 0, {}, None],
    )

    # ── 3ag) 这一段自己有没有说画个框、铺个底：壳与边是两件事，shorthand 与四条边也是 ──
    print("=== 3ag) 段边框与底纹：OOXML 一枚壳加几条边，ODF 一跳而一条 shorthand 顶四条 ===")
    for name in sorted(one.name for one in FIXTURES.glob("*.docx")):
        got = lbin("office-doc", fixture(name))
        check("%s 段边框与底纹那份账与读者一致（壳在不在与几条边分开数）" % name,
              dig(got, "structure.para_borders"),
              files[name]["ooxml"]["para_borders"])
    for name in sorted(one.name for one in FIXTURES.glob("*.odt")):
        got = lbin("office-doc", fixture(name))
        check("%s 段边框与底纹那份账与读者一致（一跳在样式里，名字留着前缀）" % name,
              dig(got, "structure.para_borders"),
              files[name]["odt"]["para_borders"])
    pb = lbin("office-doc", fixture("pborder.docx"))
    pb_lo = lbin("office-doc", fixture("pborder-lo.docx"))
    pb_odt = lbin("office-doc", fixture("pborder.odt"))
    check(
        "python-docx 那份（五段各改一个变量）：三枚 `w:pBdr` 壳、其中一枚**整个是空的**，"
        "五条边一共在另两枚壳里；底纹两枚（`clear` 与 `solid` 各一枚），两个 fill 按写的顺序交",
        [dig(pb, "structure.para_borders.paragraphs_total"),
         dig(pb, "structure.para_borders.p_pr_elements"),
         dig(pb, "structure.para_borders.with_border_element"),
         dig(pb, "structure.para_borders.border_element_empty"),
         dig(pb, "structure.para_borders.edges_total"),
         dig(pb, "structure.para_borders.with_shading"),
         dig(pb, "structure.para_borders.shading_vals"),
         dig(pb, "structure.para_borders.distinct_fills")],
        [5, 4, 3, 1, 5, 2, {"clear": 1, "solid": 1}, ["FFFF00", "00B050"]],
    )
    check(
        "一枚边自己的四个属性按写的交（`sz` 是 1/8 磅、`space` 是边离字多远，都不换算）；"
        "空壳那一段 `border_element` 是 true 而 `edge_count` 是 0 —— 这是文件说过的话，"
        "不能读成「这段没边框」；底纹那枚还可以点一个主题色（`themeFill`）",
        [dig(pb, "structure.para_borders.paragraphs[1].edges.top"),
         dig(pb, "structure.para_borders.paragraphs[2].edges.top.color"),
         dig(pb, "structure.para_borders.paragraphs[2].edge_count"),
         dig(pb, "structure.para_borders.paragraphs[4].border_element"),
         dig(pb, "structure.para_borders.paragraphs[4].edge_count"),
         dig(pb, "structure.para_borders.paragraphs[4].shading.themeFill"),
         dig(pb, "structure.para_borders.paragraphs[0].shading")],
        [{"val": "single", "sz": "6", "space": "1", "color": "FF0000"},
         "auto", 1, True, 0, "accent6", None],
    )
    check(
        "LibreOffice 重写同一份：每段都补了 `w:pPr`（4 → 5 枚），而那个**空壳整个被丢掉**"
        "（3 枚 → 2 枚、empty 归 0），并且把「自动」这个颜色折成一个具体色（`auto` → `000000`）"
        "—— 在场与有内容是两件事，改口与丢掉都在账上看得见",
        [dig(pb_lo, "structure.para_borders.p_pr_elements"),
         dig(pb_lo, "structure.para_borders.with_border_element"),
         dig(pb_lo, "structure.para_borders.border_element_empty"),
         dig(pb_lo, "structure.para_borders.edges_total"),
         dig(pb_lo, "structure.para_borders.paragraphs[2].edges.top.color"),
         dig(pb_lo, "structure.para_borders.paragraphs[4].border_element"),
         dig(pb_lo, "structure.para_borders.paragraphs[4].shading.fill")],
        [5, 2, 0, 5, "000000", False, "00B050"],
    )
    check(
        "转成 ODF 后两样都在段点的那份样式上，而形状换了：四边单线合成**一条 shorthand**"
        "（`fo:border=\"0.74pt solid #ff0000\"`，`sz=6` 那条在这里是 0.74pt），只有上面一条双线那段"
        "则四条各写、其中三条**明写着 `none`**，还多出一份逐根的 `style:border-line-width-top`；"
        "docx 的 `w:space` 在这一族搬成 `fo:padding`",
        [dig(pb_odt, "structure.para_borders.with_shorthand"),
         dig(pb_odt, "structure.para_borders.with_side_elements"),
         dig(pb_odt, "structure.para_borders.sides_written"),
         dig(pb_odt, "structure.para_borders.sides_none"),
         dig(pb_odt, "structure.para_borders.paragraphs[1].border_shorthand"),
         dig(pb_odt, "structure.para_borders.paragraphs[1].padding_written"),
         dig(pb_odt, "structure.para_borders.paragraphs[2].sides_written"),
         dig(pb_odt, "structure.para_borders.paragraphs[2].line_widths")],
        [1, 1, 4, 3, "0.74pt solid #ff0000", "0.035cm",
         {"fo:border-left": "none", "fo:border-right": "none",
          "fo:border-top": "6.75pt double #000000", "fo:border-bottom": "none"},
         {"top": "0.079cm 0.079cm 0.079cm"}],
    )
    check(
        "底纹那一枚最要紧：同一个 `w:fill` 在两种 `w:val` 下不是同一个角色 —— "
        "`clear` + `fill=FFFF00` 那一段转过去是 `#ffff00`，而 `solid` + `fill=00B050`（另点着主题色）"
        "那一段转过去成了 `#ffffff`。两份读者都把串原样交出来，不猜哪个才对",
        [dig(pb, "structure.para_borders.paragraphs[3].shading.val"),
         dig(pb_odt, "structure.para_borders.paragraphs[3].background_written"),
         dig(pb, "structure.para_borders.paragraphs[4].shading.val"),
         dig(pb_odt, "structure.para_borders.paragraphs[4].background_written")],
        ["clear", "#ffff00", "solid", "#ffffff"],
    )
    check(
        "没框没底的件交一串 0 与空表而不是缺键（`keep.docx` 五段一枚壳都没有、"
        "`keep.odt` 也没有一份段落样式写着边框），RTF 那一族不交这个键（缺键 = 这一族没看："
        "那一族的段边框住在样式表里，归属判不住）",
        [dig(lbin("office-doc", fixture("keep.docx")), "structure.para_borders.with_border_element"),
         dig(lbin("office-doc", fixture("keep.docx")), "structure.para_borders.edges_total"),
         dig(lbin("office-doc", fixture("keep.docx")), "structure.para_borders.shading_vals"),
         dig(lbin("office-doc", fixture("keep.odt")), "structure.para_borders.with_background"),
         dig(lbin("office-doc", fixture("keep.odt")), "structure.para_borders.sides_written"),
         dig(lbin("office-doc", fixture("tabs.rtf")), "structure.para_borders")],
        [0, 0, {}, 0, 0, None],
    )

    # ── 3ah) 文本框：一个框可以写两份、框里的段不是正文的段 ──────────────────────
    print("=== 3ah) 文本框：OOXML 两种容器各写一份，ODF 一个 frame 套一个 text-box ===")
    for name in sorted(one.name for one in FIXTURES.glob("*.docx")):
        got = lbin("office-doc", fixture(name))
        check("%s 文本框那份账与读者一致（几份格子与几句话分两个数）" % name,
              dig(got, "structure.text_boxes"),
              files[name]["ooxml"]["text_boxes"])
    for name in sorted(one.name for one in FIXTURES.glob("*.odt")):
        got = lbin("office-doc", fixture(name))
        check("%s 文本框那份账与读者一致（frame 的名字、锚与尺寸按写的交）" % name,
              dig(got, "structure.text_boxes"),
              files[name]["odt"]["text_boxes"])
    tb = lbin("office-doc", fixture("tbox.docx"))
    tb_odt = lbin("office-doc", fixture("tbox.odt"))
    tb_lo = lbin("office-doc", fixture("tbox-lo.odt"))
    check(
        "LibreOffice 的 docx 把一个框写成两份：`w:drawing`（DrawingML，尺寸在 `wp:extent` 的 EMU 上）"
        "与 `w:pict`（VML）各带一份 `w:txbxContent`，两份的字一模一样 —— 所以格子是 2 份而话只有 1 句，"
        "合成一个数就把同一句话读成两个框",
        [dig(tb, "structure.text_boxes.boxes_total"),
         dig(tb, "structure.text_boxes.drawings_with_boxes"),
         dig(tb, "structure.text_boxes.picts_with_boxes"),
         dig(tb, "structure.text_boxes.distinct_text_count"),
         dig(tb, "structure.text_boxes.distinct_texts"),
         dig(tb, "structure.text_boxes.boxes[0].inline_element"),
         dig(tb, "structure.text_boxes.boxes[0].anchor_element"),
         dig(tb, "structure.text_boxes.boxes[0].extent_written"),
         dig(tb, "structure.text_boxes.boxes[1].kind"),
         dig(tb, "structure.text_boxes.boxes[1].shape_style")],
        [2, 1, 1, 1, ["框里的第一段框里的第二段，长一点的字"],
         "inline", None, {"cx": "1800225", "cy": "864235"}, "pict", None],
    )
    check(
        "框里自己带段，所以「有几段」在这份件上有两个答案：`w:body` 的直接孩子 3 段、整棵树 7 段，"
        "差的那 4 段就是两份副本各带两段 —— 与批注、脚注同一族（合并成一个数就说不清谁是谁）",
        [dig(tb, "structure.text_boxes.paragraphs_direct_of_body"),
         dig(tb, "structure.text_boxes.paragraphs_anywhere"),
         dig(tb, "structure.text_boxes.paragraphs_in_boxes_direct"),
         dig(tb, "structure.text_boxes.paragraphs_in_boxes_anywhere"),
         dig(tb, "structure.text_boxes.boxes[0].paragraphs_direct"),
         dig(tb, "structure.text_boxes.boxes[0].text")
         == dig(tb, "structure.text_boxes.boxes[1].text")],
        [3, 7, 4, 4, 2, True],
    )
    check(
        "ODF 这一族是一个 `draw:frame` 套一个 `draw:text-box`：名字、锚、尺寸与坐标都写在框自己身上，"
        "而尺寸是自带单位的串（`5cm` / `2.4cm`），坐标 `1.2cm` / `0.5cm` 与层号也都在",
        [dig(tb_odt, "structure.text_boxes.frames_total"),
         dig(tb_odt, "structure.text_boxes.frames_with_boxes"),
         dig(tb_odt, "structure.text_boxes.text_box_elements"),
         dig(tb_odt, "structure.text_boxes.paragraphs_direct_of_text"),
         dig(tb_odt, "structure.text_boxes.paragraphs_anywhere"),
         dig(tb_odt, "structure.text_boxes.boxes[0].name_written"),
         dig(tb_odt, "structure.text_boxes.boxes[0].anchor_written"),
         dig(tb_odt, "structure.text_boxes.boxes[0].width_written"),
         dig(tb_odt, "structure.text_boxes.boxes[0].x_written"),
         dig(tb_odt, "structure.text_boxes.boxes[0].style_written")],
        [1, 1, 1, 3, 5, "框一", "as-char", "5cm", "1.2cm", None],
    )
    check(
        "LibreOffice 重写同一份 odt 那一遍：挂上帧样式 `Frame`、**两个坐标与层号整个没了**、"
        "尺寸从 `5cm` 换成 `5.001cm`（换算串），而那一句话一字未改也还是只有一份 —— "
        "「丢了哪几格」与「换了写法」都在账上，不替它接回去",
        [dig(tb_lo, "structure.text_boxes.boxes[0].style_written"),
         dig(tb_lo, "structure.text_boxes.boxes[0].width_written"),
         dig(tb_lo, "structure.text_boxes.boxes[0].height_written"),
         dig(tb_lo, "structure.text_boxes.boxes[0].x_written"),
         dig(tb_lo, "structure.text_boxes.boxes[0].y_written"),
         dig(tb_lo, "structure.text_boxes.boxes[0].z_index_written"),
         dig(tb_lo, "structure.text_boxes.distinct_texts"),
         dig(tb_lo, "structure.text_boxes.text_box_elements")],
        ["Frame", "5.001cm", "2.401cm", None, None, None,
         ["框里的第一段框里的第二段，长一点的字"], 1],
    )
    check(
        "没有框的件交一串 0 与空表而不是缺键；**有一个帧但那不是文本框**的件（`notes.odt` 那张图）"
        "两个数分开：`frames_total` 1 而 `frames_with_boxes` 0 —— 有帧不等于有框，"
        "RTF 那一族不交这个键（缺键 = 这一族没看：那份流里既没有 SHAPPIE 也没有 `\\pict`）",
        [dig(lbin("office-doc", fixture("keep.docx")), "structure.text_boxes.boxes_total"),
         dig(lbin("office-doc", fixture("keep.docx")), "structure.text_boxes.distinct_texts"),
         dig(lbin("office-doc", fixture("keep.docx")),
             "structure.text_boxes.paragraphs_direct_of_body"),
         dig(lbin("office-doc", fixture("notes.odt")), "structure.text_boxes.frames_total"),
         dig(lbin("office-doc", fixture("notes.odt")), "structure.text_boxes.frames_with_boxes"),
         dig(lbin("office-doc", fixture("notes.odt")), "structure.text_boxes.text_box_elements"),
         dig(lbin("office-doc", fixture("tabs.rtf")), "structure.text_boxes")],
        [0, [], 5, 1, 0, 0, None],
    )

    # ── 3ai) 书签配对：止只写号、断的两个方向、重名的怎么办、Word 自己塞的那条 ──────
    print("=== 3ai) 书签配对：OOXML 按号配、ODF 按名字配，而同段的一对在那一族是一枚点 ===")
    for name in sorted(one.name for one in FIXTURES.glob("*.docx")):
        got = lbin("office-doc", fixture(name))
        check("%s 书签配对那份账与读者一致（起写名字、止只写号）" % name,
              dig(got, "structure.bookmark_pairs"),
              files[name]["ooxml"]["bookmark_pairs"])
    for name in sorted(one.name for one in FIXTURES.glob("*.odt")):
        got = lbin("office-doc", fixture(name))
        check("%s 书签配对那份账与读者一致（三种记号，配对按名字）" % name,
              dig(got, "structure.bookmark_pairs"),
              files[name]["odt"]["bookmark_pairs"])
    bp = lbin("office-doc", fixture("bkmks.docx"))
    bp_lo = lbin("office-doc", fixture("bkmks-lo.docx"))
    bp_odt = lbin("office-doc", fixture("bkmks.odt"))
    check(
        "python-docx 那份（八段各造一种情形）：5 起 5 止、闭 4 对，剩下 1 条起没有止（`断了`）"
        "与 1 条止没有起（号 `9`）—— 两个方向各数一本；`_GoBack` 那一条按名字的前缀另数一笔，"
        "重名的 `口径` 有两条而 distinct 只有 4 个",
        [dig(bp, "structure.bookmark_pairs.starts_total"),
         dig(bp, "structure.bookmark_pairs.ends_total"),
         dig(bp, "structure.bookmark_pairs.pairs_closed"),
         dig(bp, "structure.bookmark_pairs.starts_without_end"),
         dig(bp, "structure.bookmark_pairs.ends_without_start"),
         dig(bp, "structure.bookmark_pairs.names_total"),
         dig(bp, "structure.bookmark_pairs.distinct_names"),
         dig(bp, "structure.bookmark_pairs.duplicate_names"),
         dig(bp, "structure.bookmark_pairs.hidden_starts")],
        [5, 5, 4, 1, 1, 5, ["口径", "跨段", "断了", "_GoBack"], ["口径"], 1],
    )
    check(
        "`w:bookmarkEnd` 上根本没有 `w:name` 这个属性（五条止全交 null）—— 所以「这条书签闭没闭」"
        "只能按 `w:id` 问，而号是生产者自己排的：跨段那一对起在第 1 段、止在第 2 段，"
        "两头没有任何共同的名字可查",
        [dig(bp, "structure.bookmark_pairs.ends[0].name_written"),
         dig(bp, "structure.bookmark_pairs.ends[4].name_written"),
         dig(bp, "structure.bookmark_pairs.starts[1].paragraph"),
         dig(bp, "structure.bookmark_pairs.ends[1].paragraph"),
         dig(bp, "structure.bookmark_pairs.starts[1].has_end"),
         dig(bp, "structure.bookmark_pairs.starts[2].has_end"),
         dig(bp, "structure.bookmark_pairs.ends[2].id_written"),
         dig(bp, "structure.bookmark_pairs.ends[2].has_start")],
        [None, None, 1, 2, True, False, "9", False],
    )
    check(
        "LibreOffice 重写同一份：两个**断的整个被删掉**（5 起 5 止 → 4 起 4 止、两个孤本都归 0）、"
        "号从 1..5 整批重排成 0..3、第二条重名的它不报错而是**改名** `口径_副本_1` —— "
        "删、排、改都是文件自己的事，读者按现在这份件交",
        [dig(bp_lo, "structure.bookmark_pairs.starts_total"),
         dig(bp_lo, "structure.bookmark_pairs.ends_total"),
         dig(bp_lo, "structure.bookmark_pairs.pairs_closed"),
         dig(bp_lo, "structure.bookmark_pairs.starts_without_end"),
         dig(bp_lo, "structure.bookmark_pairs.ends_without_start"),
         dig(bp_lo, "structure.bookmark_pairs.duplicate_names"),
         dig(bp_lo, "structure.bookmark_pairs.distinct_names"),
         dig(bp_lo, "structure.bookmark_pairs.starts[0].id_written")],
        [4, 4, 4, 0, 0, [], ["口径", "跨段", "_GoBack", "口径_副本_1"], "0"],
    )
    check(
        "转成 ODF 后记号换了三种：闭在同段的那两条与 Word 那条光标都变成**一枚** `text:bookmark`"
        "（3 枚），只有跨段那一对还是 `bookmark-start`/`-end` 一条对（两头都写名字，配对按名字）；"
        "而那个改名的副本在这一族写成带空格的「口径 副本 1」—— 与 docx 那面的下划线是两个不同的串",
        [dig(bp_odt, "structure.bookmark_pairs.points_total"),
         dig(bp_odt, "structure.bookmark_pairs.spans_start"),
         dig(bp_odt, "structure.bookmark_pairs.spans_end"),
         dig(bp_odt, "structure.bookmark_pairs.spans_closed"),
         dig(bp_odt, "structure.bookmark_pairs.names_total"),
         dig(bp_odt, "structure.bookmark_pairs.distinct_names"),
         dig(bp_odt, "structure.bookmark_pairs.span_starts[0].name_written"),
         dig(bp_odt, "structure.bookmark_pairs.span_ends[0].paragraph"),
         dig(bp_odt, "structure.bookmark_pairs.points[2].name_written")],
        [3, 1, 1, 1, 4, ["口径", "_GoBack", "口径 副本 1", "跨段"], "跨段", 2, "口径 副本 1"],
    )
    check(
        "没有书签的件交一串 0 与空表而不是缺键（`keep.docx` 与 `keep.odt` 都是 0），"
        "RTF 不交这一份配对账（缺键 = 这一支不再交一次：那一族的 `\\bkmkstart` / `\\bkmkend` "
        "条数早就在 `structure.bookmarks` 那本账上，两份数不互相顶替）",
        [dig(lbin("office-doc", fixture("keep.docx")), "structure.bookmark_pairs.starts_total"),
         dig(lbin("office-doc", fixture("keep.docx")), "structure.bookmark_pairs.distinct_names"),
         dig(lbin("office-doc", fixture("keep.odt")), "structure.bookmark_pairs.points_total"),
         dig(lbin("office-doc", fixture("keep.odt")), "structure.bookmark_pairs.spans_start"),
         dig(lbin("office-doc", fixture("tabs.rtf")), "structure.bookmark_pairs")],
        [0, [], 0, 0, None],
    )

    # ── 3aj) 放映切换：一页可以写两条，属性可以缺，效果孩子自己带属性（pptx 一支）──
    print("=== 3aj) 放映切换：`p:transition` 的条数、属性与效果孩子 ===")

    def tr_multiset(rows):
        """按内容排序的多重集：两支读者的页序不同（一支按放映序、一支按部件名），
        所以这一问不按位置比，按「这套页一共交出了什么」比。"""
        return sorted(json.dumps(one.get("transition_detail"), sort_keys=True,
                                 ensure_ascii=False) for one in rows)

    for name in sorted(one.name for one in FIXTURES.glob("*.pptx")):
        got = lbin("office-slide", fixture(name))
        want = files[name]["ooxml"]["slides"]
        check("%s 每页的切换账合起来与读者一致（多重集，不比页序）" % name,
              tr_multiset(got.get("slides", [])), tr_multiset(want))
        check("%s 全篇几条 `p:transition` 与读者一致" % name,
              sum((one.get("transition_detail") or {}).get("elements", 0)
                  for one in got.get("slides", [])),
              sum((one.get("transition_detail") or {}).get("elements", 0) for one in want))
    tr = lbin("office-slide", fixture("deck-tr.pptx"))
    tr_lo = lbin("office-slide", fixture("deck-tr-lo.pptx"))
    check(
        "python-pptx 那份（三页各改一个变量）：第一页三个属性都写（`spd=med`、`advClick=1`、"
        "`advTm=5000`）带一个孩子 `fade`；第二页只写 `spd=fast` 而方向在孩子自己身上（`wipe dir=l`）；"
        "第三页一个字都没写（0 条）—— 所以「几条」2 与「几页有」2 这次刚好相同，是因为每页最多一条",
        [dig(tr, "slides[0].transition_detail.elements"),
         dig(tr, "slides[0].transition_detail.list[0].written"),
         dig(tr, "slides[0].transition_detail.list[0].effects"),
         dig(tr, "slides[1].transition_detail.list[0].written"),
         dig(tr, "slides[1].transition_detail.list[0].effects"),
         dig(tr, "slides[2].transition_detail.elements"),
         sum((one.get("transition_detail") or {}).get("elements", 0) for one in tr["slides"])],
        [1, {"spd": "med", "advClick": "1", "advTm": "5000"},
         [{"element": "fade", "written": {}}],
         {"spd": "fast"}, [{"element": "wipe", "written": {"dir": "l"}}], 0, 2],
    )
    check(
        "LibreOffice 重写同一份：第一页的 `advClick` 没了（只剩 `spd` 与 `advTm`）、第二页连 `spd` "
        "都没了（属性表是空的，而孩子的 `dir=l` 留着），第三页**原本什么都没写，它补了两条** —— "
        "于是一篇里 4 条元素而只有 3 页有：一页两条是真会发生的，两个数不能互推",
        [dig(tr_lo, "slides[0].transition_detail.list[0].written"),
         dig(tr_lo, "slides[1].transition_detail.list[0].written"),
         dig(tr_lo, "slides[1].transition_detail.list[0].effects"),
         dig(tr_lo, "slides[2].transition_detail.elements"),
         [one["written"] for one in dig(tr_lo, "slides[2].transition_detail.list")],
         [one["effects"] for one in dig(tr_lo, "slides[2].transition_detail.list")],
         sum((one.get("transition_detail") or {}).get("elements", 0) for one in tr_lo["slides"]),
         len([one for one in tr_lo["slides"]
              if (one.get("transition_detail") or {}).get("elements", 0) > 0])],
        [{"spd": "med", "advTm": "5000"}, {}, [{"element": "wipe", "written": {"dir": "l"}}],
         2, [{"spd": "slow", "dur": "2000"}, {"spd": "slow"}], [[], []], 4, 3],
    )
    check(
        "同一份转成 odp 之后这一族不交这个键（缺键 = 这一支只读 OOXML）：切换在那一族没丢，"
        "而是搬到 `style:drawing-page-properties` 与一棵 SMIL 动画树里、词表整个换了"
        "（`presentation:transition-type` / `anim:transitionFilter`），那是另一问、另一次测量",
        [dig(lbin("office-slide", fixture("deck-tr.odp")), "slides[0].transition_detail")],
        [None],
    )

    # ── 3ak) odp 那一面的放映切换：样式里一份 + 页体内一棵动画树，两处都交 ─────────
    print("=== 3ak) odp 放映切换：dp1 写满、dp3 半句、dp4 一个字不写（与 pptx 那一面正相反）===")

    def odp_tr_multiset(rows):
        return sorted(json.dumps(one.get("odp_transition"), sort_keys=True,
                                 ensure_ascii=False) for one in rows)

    for name in sorted(one.name for one in FIXTURES.glob("*.odp")):
        got = lbin("office-slide", fixture(name))
        want = files[name].get("odp", {}).get("slides", [])
        check("%s 每页的 odp 切换账合起来与读者一致（多重集，不比页序）" % name,
              odp_tr_multiset(got.get("slides", [])), odp_tr_multiset(want))
        check("%s 全篇 odp 效果条数与读者一致" % name,
              sum(len((one.get("odp_transition") or {}).get("effects", []))
                  for one in got.get("slides", [])),
              sum(len((one.get("odp_transition") or {}).get("effects", [])) for one in want))
    odp = lbin("office-slide", fixture("deck-tr.odp"))
    check(
        "第一页那份 dp1 样式写满了一句半：`transition-type=\"automatic\"`、`transition-speed=\"fast\"`、"
        "`duration=\"PT5S``，另带效果自己的 `type` / `subtype` / `fadeColor`；而页体内那棵动画树"
        "**又写了一遍**效果（`smil:dur=\"0.75s\"` + fade/crossfade）—— 一份件两处，两处都交、不互证",
        [dig(odp, "slides[0].odp_transition.page_style"),
         dig(odp, "slides[0].odp_transition.style_found"),
         dig(odp, "slides[0].odp_transition.style_part"),
         dig(odp, "slides[0].odp_transition.written"),
         dig(odp, "slides[0].odp_transition.effects"),
         dig(odp, "slides[0].odp_transition.timing_roots")],
        ["dp1", True, "content.xml",
         {"transition-type": "automatic", "transition-speed": "fast", "duration": "PT5S",
          "type": "fade", "subtype": "crossfade", "fadeColor": "#000000"},
         [{"written": {"dur": "0.75s", "type": "fade", "subtype": "crossfade"}}], 1],
    )
    check(
        "第二页只写半句：样式里有 `transition-speed` 与 `type=barWipe` / `subtype=leftToRight` / "
        "`direction=reverse` 而**没有 `transition-type`、没有 duration**（那一族没有「默认就是 fast」这回事，"
        "没写就交没有）；第三页那份 dp4 找到了、可一个切换属性都不写 —— `written` 是空表而不是缺键，"
        "而 pptx 那一面 LibreOffice 给同一页**补了两条** `p:transition`：同一个生产者的两个导出方向相反",
        [dig(odp, "slides[1].odp_transition.written"),
         dig(odp, "slides[1].odp_transition.effects"),
         dig(odp, "slides[2].odp_transition.page_style"),
         dig(odp, "slides[2].odp_transition.style_found"),
         dig(odp, "slides[2].odp_transition.written"),
         dig(odp, "slides[2].odp_transition.effects"),
         dig(odp, "slides[2].odp_transition.timing_roots"),
         dig(lbin("office-slide", fixture("deck-tr-lo.pptx")),
             "slides[2].transition_detail.elements")],
        [{"transition-speed": "fast", "type": "barWipe", "subtype": "leftToRight",
          "direction": "reverse"},
         [{"written": {"dur": "0.5s", "type": "barWipe", "subtype": "leftToRight",
                       "direction": "reverse"}}],
         "dp4", True, {}, [], 0, 2],
    )
    check(
        "另一份 odp（`deck.odp`，两页都点 `dp1`）那一份样式什么切换都没写：两页的 `written` 都是空表、"
        "`style_found` 都是 true —— 「跳到了那份样式而它没说」与「跳不到那份样式」是两件事",
        [dig(lbin("office-slide", fixture("deck.odp")), "slides[0].odp_transition.written"),
         dig(lbin("office-slide", fixture("deck.odp")), "slides[0].odp_transition.style_found"),
         dig(lbin("office-slide", fixture("deck.odp")), "slides[1].odp_transition.written"),
         dig(lbin("office-slide", fixture("deck.odp")), "slides[1].odp_transition.effects"),
         dig(lbin("office-slide", fixture("deck.odp")), "slides[0].transition_detail")],
        [{}, True, {}, [], None],
    )



    # ── 3an) 脚注与尾注怎么编号：OOXML 两处各说各的，ODF 一类注一份且两类不对称 ──────
    print("=== 3an) 注的编号：settings 那份说了从几开始，sectPr 那份没说 ===")
    for name in sorted(one.name for one in FIXTURES.glob("*.docx")):
        got = lbin("office-doc", fixture(name))
        check("%s 注的编号那份账与读者一致（settings 一份 + 每条节一份）" % name,
              dig(got, "structure.note_settings"),
              files[name]["ooxml"]["note_settings"])
    for name in sorted(one.name for one in FIXTURES.glob("*.odt")):
        got = lbin("office-doc", fixture(name))
        check("%s 注的编号那份账与读者一致（一类注一份 configuration）" % name,
              dig(got, "structure.note_settings"),
              files[name]["odt"]["note_settings"])
    nset = lbin("office-doc", fixture("nset.docx"))
    check(
        "`nset.docx`：settings 里那份 `w:footnotePr` 说了 `numFmt=decimal` / `numStart=5` / "
        "`numRestart=eachPage` / `pos=sectEnd`，而 `w:sectPr` 里那一份**只有 `pos` 与 `numFmt`** —— "
        "「从几开始、每页重来吗」只在一处说过话，所以 `attrs_only_in_settings` 是那两格、"
        "`attrs_in_both` 只有 `numFmt` 与 `pos`；两处各交一份，不合成",
        [dig(nset, "structure.note_settings.settings_part"),
         dig(nset, "structure.note_settings.footnote_written"),
         dig(nset, "structure.note_settings.endnote_written"),
         dig(nset, "structure.note_settings.footnote.written"),
         dig(nset, "structure.note_settings.footnote.note_refs"),
         dig(nset, "structure.note_settings.sections[0].footnote.written"),
         dig(nset, "structure.note_settings.attrs_only_in_settings"),
         dig(nset, "structure.note_settings.attrs_only_in_sections"),
         dig(nset, "structure.note_settings.attrs_in_both")],
        [True, True, True,
         {"numFmt": "decimal", "numStart": "5", "numRestart": "eachPage", "pos": "sectEnd"},
         ["0", "1"], {"pos": "sectEnd", "numFmt": "decimal"},
         ["numRestart", "numStart"], [], ["numFmt", "pos"]],
    )
    check(
        "那两个 `w:footnote w:id` / `w:endnote w:id` 孩子是分隔符与延续分隔符的引用"
        "（注部件里那两条空正文的占位，注那一条 lane 已经量过）—— 它们住在 `w:footnotePr` "
        "**里面**，所以「settings 那份有引用、sectPr 那份没有」也是两处不一样的地方：`note_refs` 两份各交",
        [dig(nset, "structure.note_settings.endnote.note_refs"),
         dig(nset, "structure.note_settings.sections[0].footnote.note_refs"),
         dig(nset, "structure.note_settings.sections[0].endnote.note_refs"),
         dig(nset, "structure.note_settings.sections_with_footnote_pr"),
         dig(nset, "structure.note_settings.sections_with_endnote_pr"),
         dig(nset, "structure.note_settings.sections_total")],
        [["0", "1"], [], [], 1, 1, 1],
    )
    nset_lo = lbin("office-doc", fixture("nset-lo.docx"))
    check(
        "LibreOffice 把同一份重写一遍：两处的那两格**都没了**（只剩 `pos` 与 `numFmt`），"
        "`attrs_only_in_settings` 因此变空、`attrs_in_both` 还是那两格 —— 「谁丢了起点」在账上看得见，"
        "而编号格式与分隔符引用一字未动",
        [dig(nset_lo, "structure.note_settings.footnote.written"),
         dig(nset_lo, "structure.note_settings.endnote.written"),
         dig(nset_lo, "structure.note_settings.attrs_only_in_settings"),
         dig(nset_lo, "structure.note_settings.attrs_in_both"),
         dig(nset_lo, "structure.note_settings.footnote.note_refs")],
        [{"pos": "sectEnd", "numFmt": "decimal"},
         {"pos": "sectEnd", "numFmt": "lowerRoman"},
         [], ["numFmt", "pos"], ["0", "1"]],
    )
    check(
        "反面凭据：`notes.docx`（python-docx 的原件，**有脚注**但两处都没写过这一格）—— "
        "两个 `*_written` 与两条 `sections_with_*_pr` 全是 false / 0；`sections.docx` 两节也一样。"
        "「这份件有注」与「这份件说了注怎么编号」是两件事",
        [dig(lbin("office-doc", fixture("notes.docx")), "structure.note_settings.footnote_written"),
         dig(lbin("office-doc", fixture("notes.docx")), "structure.note_settings.footnote"),
         dig(lbin("office-doc", fixture("notes.docx")), "structure.note_settings.sections_with_footnote_pr"),
         dig(lbin("office-doc", fixture("sections.docx")), "structure.note_settings.sections_total"),
         dig(lbin("office-doc", fixture("sections.docx")), "structure.note_settings.sections_with_endnote_pr"),
         dig(lbin("office-doc", fixture("sections.docx")), "structure.note_settings.attrs_in_both")],
        [False, None, 0, 2, 0, []],
    )
    check(
        "ODF 那一面一类注一份 configuration（两份都在 styles.xml）：footnote 那份写了 "
        "`num-format=\"1\"` + `start-value=\"0\"` + `footnotes-position=\"page\"` + "
        "`start-numbering-at=\"document\"`，endnote 那份**只有前两个** —— 没写的交 false，"
        "不拿另一类的写法替它接；而 LibreOffice 对这份带 `numStart=5` 的 docx 转出来的 odt "
        "写的还是它自己的默认 `start-value=\"0\"`（不是 5）",
        [dig(lbin("office-doc", fixture("nset.odt")), "structure.note_settings.configs_total"),
         dig(lbin("office-doc", fixture("nset.odt")), "structure.note_settings.classes_written"),
         dig(lbin("office-doc", fixture("nset.odt")), "structure.note_settings.distinct_num_formats"),
         dig(lbin("office-doc", fixture("nset.odt")), "structure.note_settings.with_position"),
         dig(lbin("office-doc", fixture("nset.odt")), "structure.note_settings.with_start_numbering"),
         dig(lbin("office-doc", fixture("nset.odt")), "structure.note_settings.parts_seen"),
         dig(lbin("office-doc", fixture("nset.odt")), "structure.note_settings.configs[0].written"),
         dig(lbin("office-doc", fixture("nset.odt")), "structure.note_settings.configs[1].written"),
         dig(lbin("office-doc", fixture("nset.odt")), "structure.note_settings.configs[1].position_written")],
        [2, ["footnote", "endnote"], ["1", "i"], 1, 1, ["styles.xml"],
         {"note-class": "footnote", "num-format": "1", "start-value": "0",
          "footnotes-position": "page", "start-numbering-at": "document"},
         {"note-class": "endnote", "num-format": "i", "start-value": "0"}, False],
    )
    check(
        "RTF 与遗留 .doc **不交这个键**（缺键 = 这一支没看）：RTF 的注编号写在 `\\ftrprops` 那一路"
        "控制字上、没有节级对应物，归属判不住；.doc 的注设置住在 table stream，这一族读者不走那里",
        [dig(lbin("office-doc", fixture("tabs.rtf")), "structure.note_settings"),
         dig(lbin("office-doc", fixture("notes-en.doc")), "structure.note_settings")],
        [None, None],
    )
    # ── 3am) 这份文档写了哪种语言：OOXML 一枚元素三路文字，ODF 只有一格还要拆两段 ────
    print("=== 3am) 语言：`w:lang` 三属性四层 vs `fo:language` + `fo:country`，含字面 none ===")
    for name in sorted(one.name for one in FIXTURES.glob("*.docx")):
        got = lbin("office-doc", fixture(name))
        check("%s 语言那份账与读者一致（四层各交各的）" % name,
              dig(got, "structure.languages"),
              files[name]["ooxml"]["languages"])
    for name in sorted(one.name for one in FIXTURES.glob("*.odt")):
        got = lbin("office-doc", fixture(name))
        check("%s 语言那份账与读者一致（宿主两趟，先 style 后 default-style）" % name,
              dig(got, "structure.languages"),
              files[name]["odt"]["languages"])
    check(
        "模板的说法不是作者的说法：`notes.docx` 全文只有 styles.xml 里那一条 `w:lang`，"
        "它在 `docDefaults` 上同时写 `val=\"en-US\"` / `eastAsia=\"en-US\"` / `bidi=\"ar-SA\"` —— "
        "一份全中文稿子在文件级默认上声明「复杂脚本是阿拉伯语」，正文一个字都没说（`in_document` 0）",
        [dig(lbin("office-doc", fixture("notes.docx")), "structure.languages.elements_total"),
         dig(lbin("office-doc", fixture("notes.docx")), "structure.languages.in_document"),
         dig(lbin("office-doc", fixture("notes.docx")), "structure.languages.in_styles"),
         dig(lbin("office-doc", fixture("notes.docx")), "structure.languages.doc_defaults_written"),
         dig(lbin("office-doc", fixture("notes.docx")), "structure.languages.doc_defaults"),
         dig(lbin("office-doc", fixture("notes.docx")), "structure.languages.runs_with_lang"),
         dig(lbin("office-doc", fixture("notes.docx")), "structure.languages.levels_seen")],
        [1, 0, 1, True, {"val": "en-US", "eastAsia": "en-US", "bidi": "ar-SA"}, 0, ["doc_defaults"]],
    )
    lang = lbin("office-doc", fixture("lang.docx"))
    check(
        "`lang.docx` 第一次让段层与 run 层有非零凭据：段上 `val=\"es-ES\"`，三串字分别只写 "
        "`val=\"fr-FR\"`、**只写 `eastAsia=\"ja-JP\"`**、三路全写 `de-DE / zh-CN / ar-SA` —— "
        "三个属性各说一路文字，所以「这份文档几种语言」要看问的是哪一路（`distinct_vals` 与 "
        "`distinct_east_asia` 是两个清单，不并成一个）",
        [dig(lang, "structure.languages.elements_total"),
         dig(lang, "structure.languages.in_document"),
         dig(lang, "structure.languages.paragraphs_with_lang"),
         dig(lang, "structure.languages.runs_with_lang"),
         dig(lang, "structure.languages.paragraphs[0]"),
         dig(lang, "structure.languages.runs[1]"),
         dig(lang, "structure.languages.runs[2].attrs"),
         dig(lang, "structure.languages.distinct_vals"),
         dig(lang, "structure.languages.distinct_east_asia"),
         dig(lang, "structure.languages.distinct_bidi")],
        [5, 4, 1, 3,
         {"index": 7, "attrs": {"val": "es-ES"}},
         {"run": 14, "attrs": {"eastAsia": "ja-JP"}},
         {"val": "de-DE", "eastAsia": "zh-CN", "bidi": "ar-SA"},
         ["es-ES", "fr-FR", "de-DE", "en-US"], ["ja-JP", "zh-CN", "en-US"], ["ar-SA"]],
    )
    check(
        "LibreOffice 重写同一份：正文那四条一字未动，另外**给三个样式各补了一条** "
        "（`Normal` / `NoSpacing` / `MacroText`，值都是 en-US / en-US / ar-SA）—— "
        "元素 5 条变 8 条、`levels_seen` 多出一层；补的是它自己的手笔，不是这份稿子说过的话",
        [dig(lbin("office-doc", fixture("lang-lo.docx")), "structure.languages.elements_total"),
         dig(lbin("office-doc", fixture("lang-lo.docx")), "structure.languages.in_styles"),
         dig(lbin("office-doc", fixture("lang-lo.docx")), "structure.languages.styles_with_lang"),
         dig(lbin("office-doc", fixture("lang-lo.docx")), "structure.languages.runs_with_lang"),
         dig(lbin("office-doc", fixture("lang-lo.docx")), "structure.languages.paragraphs_with_lang"),
         dig(lbin("office-doc", fixture("lang-lo.docx")), "structure.languages.styles[0]"),
         dig(lbin("office-doc", fixture("lang-lo.docx")), "structure.languages.levels_seen")],
        [8, 4, 3, 3, 1,
         {"style_id": "Normal", "style_type": "paragraph",
          "attrs": {"val": "en-US", "eastAsia": "en-US", "bidi": "ar-SA"}},
         ["doc_defaults", "styles", "paragraphs", "runs"]],
    )
    check(
        "同一份转成 odt 只留一格：`distinct_languages` 是 `de / en / es / fr` —— "
        "**只写 `eastAsia=\"ja-JP\"` 那一串字在 ODF 一个字都没落**（没有 ja），三路全写那串只剩 "
        "`de` + `DE`（zh 与 ar 都不见），而 `en-US` 在这一族拆成 `language=\"en\"` + `country=\"US\"` "
        "两个属性 —— 词汇与格数都不是一套，不折算",
        [dig(lbin("office-doc", fixture("lang.odt")), "structure.languages.elements_total"),
         dig(lbin("office-doc", fixture("lang.odt")), "structure.languages.under_style"),
         dig(lbin("office-doc", fixture("lang.odt")), "structure.languages.not_under_style"),
         dig(lbin("office-doc", fixture("lang.odt")), "structure.languages.distinct_languages"),
         dig(lbin("office-doc", fixture("lang.odt")), "structure.languages.distinct_countries"),
         dig(lbin("office-doc", fixture("lang.odt")), "structure.languages.distinct_scripts"),
         dig(lbin("office-doc", fixture("lang.odt")), "structure.languages.parts_seen"),
         dig(lbin("office-doc", fixture("lang.odt")), "structure.languages.entries[0]")],
        [5, 5, 0, ["de", "en", "es", "fr"], ["DE", "ES", "FR", "US"], [],
         ["content.xml", "styles.xml"],
         {"part": "content.xml", "holder": "style", "style_name": "T1", "family": "text",
          "attrs": {"language": "es", "country": "ES"}}],
    )
    check(
        "「说了没有」与「一个字不说」是两件事：`tbox-lo.odt` 有一条 `style:text-properties` 写 "
        "**`fo:language=\"none\"`**（`none_written` 1，而它的 `country` 也写着 `none`）；"
        "`tbox.odt` 与 `pnum.odt`（zipfile 写的最小件）整族零条 —— `elements_total` 是 0、"
        "`entries` 是空表、`parts_seen` 也是空表，不替它补 `en`",
        [dig(lbin("office-doc", fixture("tbox-lo.odt")), "structure.languages.none_written"),
         dig(lbin("office-doc", fixture("tbox-lo.odt")), "structure.languages.elements_total"),
         dig(lbin("office-doc", fixture("tbox-lo.odt")), "structure.languages.distinct_languages"),
         dig(lbin("office-doc", fixture("tbox-lo.odt")), "structure.languages.distinct_countries"),
         dig(lbin("office-doc", fixture("tbox.odt")), "structure.languages.elements_total"),
         dig(lbin("office-doc", fixture("tbox.odt")), "structure.languages.entries"),
         dig(lbin("office-doc", fixture("tbox.odt")), "structure.languages.distinct_languages"),
         dig(lbin("office-doc", fixture("pnum.odt")), "structure.languages.parts_seen")],
        [1, 2, ["en", "none"], ["US", "none"], 0, [], [], []],
    )
    check(
        "RTF 与遗留 .doc **不交这个键**（缺键 = 这一支没看）：RTF 写的是 `\\lang` 加一个 LCID 数字"
        "（另有 `\\langfe` 那一路），整名比对与归属判据还没量完，不拿「数得出条数」当「说得清归属」；"
        "而「这份文档是哪国语言」这个属性级的问句早就在 `office-meta` 的 `dc:language` 那一份账上，"
        "两份数不互相顶替",
        [dig(lbin("office-doc", fixture("tabs.rtf")), "structure.languages"),
         dig(lbin("office-doc", fixture("notes-en.doc")), "structure.languages")],
        [None, None],
    )

    # ── 3ar) 这张字体字典自己说了什么：子集前缀、/FontDescriptor、里面有没有 FontFile* ──
    print("=== 3ar) PDF 字体嵌没嵌入：字典自己写的三样，外加 pdffonts 那三列的对质 ===")
    for name in sorted(one.name for one in FIXTURES.glob("*.pdf")):
        got = lbin("office-pdf", fixture(name))
        check("%s 字体字典那份账与读者一致（descriptor / FontFile* / 子集前缀）" % name,
              got.get("font_embedding"), files[name]["pdf"]["font_embedding"])
    dk = lbin("office-pdf", fixture("deck.pdf"))
    check(
        "`deck.pdf` 六张字体每张都**自己写了** `/FontDescriptor`，descriptor 里都带 `/FontFile2`，"
        "名字前还各有一截生产者自己截的子集前缀（`EAAAAA+Calibri` → `subset_prefix` `EAAAAA`）；"
        "六张都带 `/ToUnicode` —— `pdffonts` 那三列 emb/sub/uni 全是 yes，对象号逐个对得上。"
        "同一份输出里 `fonts` 与 `font_embedding` 是**两个问句**：前者列名字与编码，"
        "后者只回答「这一层说没说自己带了字面数据」，键互不重叠",
        [dig(dk, "font_embedding.fonts_total"),
         dig(dk, "font_embedding.with_descriptor"),
         dig(dk, "font_embedding.descriptor_missing"),
         dig(dk, "font_embedding.with_font_file"),
         dig(dk, "font_embedding.subsets"),
         dig(dk, "font_embedding.with_to_unicode"),
         dig(dk, "font_embedding.file_kinds"),
         dig(dk, "font_embedding.subtypes"),
         dig(dk, "font_embedding.fonts[0].object"),
         dig(dk, "font_embedding.fonts[0].base_font"),
         dig(dk, "font_embedding.fonts[0].subset_prefix"),
         dig(dk, "font_embedding.fonts[0].font_file"),
         dig(dk, "font_embedding.fonts[0].from_object_stream")],
        [6, 6, 0, 6, 6, 6, ["FontFile2"], ["TrueType"], 40, "EAAAAA+Calibri",
         "EAAAAA", "FontFile2", None],
    )
    rk = lbin("office-pdf", fixture("risk.pdf"))
    check(
        "`risk.pdf` 是这一问的反面凭据：一张 `Helvetica`（Type1）**什么都没有写** —— "
        "`descriptor` 与 `font_file` 都是 null（不是 0），`subsets` 0、`with_to_unicode` 0。"
        "标准 14 字体本来就从不嵌入，所以这里没有「嵌入失败」可读；"
        "而 `pdffonts` 对它那一行给的是 emb=no、sub=no —— 两边各自的答案都留下",
        [dig(rk, "font_embedding.fonts_total"),
         dig(rk, "font_embedding.with_descriptor"),
         dig(rk, "font_embedding.descriptor_missing"),
         dig(rk, "font_embedding.with_font_file"),
         dig(rk, "font_embedding.subsets"),
         dig(rk, "font_embedding.file_kinds"),
         dig(rk, "font_embedding.subtypes"),
         dig(rk, "font_embedding.fonts[0].base_font"),
         dig(rk, "font_embedding.fonts[0].descriptor"),
         dig(rk, "font_embedding.fonts[0].font_file")],
        [1, 0, 1, 0, 0, [], ["Type1"], "Helvetica", None, None],
    )
    check(
        "字体字典也可以整个住在**对象流**里：`objstm.pdf` 那份明文只有 17 个对象，"
        "五张字体都在 `/Type /ObjStm` 里 —— 不拆第二层就会报「这张 PDF 一张字体也没有」。"
        "加密的 `locked.pdf` 是另一种分工：`pdffonts` 一个字都不列（它解不开），"
        "而字体字典是明文对象，这里数得出 5 张全带 `/FontFile2` —— 两份答案各说各的，不互相顶替",
        [dig(lbin("office-pdf", fixture("objstm.pdf")), "font_embedding.fonts_total"),
         dig(lbin("office-pdf", fixture("objstm.pdf")), "font_embedding.with_font_file"),
         dig(lbin("office-pdf", fixture("objstm.pdf")), "font_embedding.with_to_unicode"),
         dig(lbin("office-pdf", fixture("locked.pdf")), "font_embedding.fonts_total"),
         dig(lbin("office-pdf", fixture("locked.pdf")), "font_embedding.subsets"),
         dig(lbin("office-pdf", fixture("locked.pdf")), "font_embedding.fonts[0].subset_prefix")],
        [5, 5, 5, 5, 5, "BAAAAA"],
    )
    # ── 3ao) 这一页有哪些形状：pptx 的 spTree 直接孩子就是叠放序，ODF 的分组是 svg:g ──
    print("=== 3ao) 形状清单：组合、层级、两处坐标，以及一页零个形状 ===")

    def shape_pages_multiset(rows):
        return sorted(json.dumps(one.get("shape_tree"), sort_keys=True, ensure_ascii=False)
                      for one in rows)

    for name in sorted(one.name for one in FIXTURES.glob("*.pptx")):
        got = lbin("office-slide", fixture(name))
        want = files[name]["ooxml"]["slides"]
        check("%s 每页形状清单合起来与读者一致（多重集，不比页序）" % name,
              shape_pages_multiset(got.get("slides", [])), shape_pages_multiset(want))
        check("%s 全篇形状条数之和与读者一致" % name,
              sum((one.get("shape_tree") or {}).get("shapes_total", 0)
                  for one in got.get("slides", [])),
              sum((one.get("shape_tree") or {}).get("shapes_total", 0) for one in want))
    for name in sorted(one.name for one in FIXTURES.glob("*.odp")):
        got = lbin("office-slide", fixture(name))
        want = files[name].get("odp", {}).get("slides", [])
        check("%s 每页形状清单合起来与读者一致（多重集，不比页序）" % name,
              shape_pages_multiset(got.get("slides", [])), shape_pages_multiset(want))
        check("%s 全篇嵌套条数之和与读者一致（frame 套 image 也算一层）" % name,
              sum((one.get("shape_tree") or {}).get("nested", 0)
                  for one in got.get("slides", [])),
              sum((one.get("shape_tree") or {}).get("nested", 0) for one in want))
    gr = lbin("office-slide", fixture("deck-gr.pptx"))
    check(
        "`deck-gr.pptx` 第 1 页：5 条形状 = 顶层 2（1 个 `sp` + 1 个 `grpSp`）+ 组合里 3，"
        "`groups` 1、`max_depth` 1、`placeholder` 全 false —— 叠放序就是 `spTree` 的孩子顺序",
        [dig(gr, "slides[0].shape_tree.shapes_total"),
         dig(gr, "slides[0].shape_tree.top_level"),
         dig(gr, "slides[0].shape_tree.nested"),
         dig(gr, "slides[0].shape_tree.groups"),
         dig(gr, "slides[0].shape_tree.max_depth"),
         dig(gr, "slides[0].shape_tree.placeholders"),
         dig(gr, "slides[0].shape_tree.kinds_seen"),
         dig(gr, "slides[0].shape_tree.distinct_names")],
        [5, 2, 3, 1, 1, 0, ["sp", "grpSp"],
         ["散着的框", "三个框的组合", "组合里的第1个", "组合里的第2个", "组合里的第3个"]],
    )
    check(
        "组合那一条自己写的 `a:xfrm` **四份都在**：`off={0,0}` 而 `ext` 与 `chExt` 一模一样 —— "
        "外面那份是页坐标、`ch*` 那份是子坐标系，两份单位一样、语义不同，按写的交不换算；"
        "里面那三条的 `depth` 是 1 而 `parent` 指回组合那一条的序号 1",
        [dig(gr, "slides[0].shape_tree.shapes[1].kind"),
         dig(gr, "slides[0].shape_tree.shapes[1].id"),
         dig(gr, "slides[0].shape_tree.shapes[1].xfrm"),
         dig(gr, "slides[0].shape_tree.shapes[1].children"),
         dig(gr, "slides[0].shape_tree.shapes[2].depth"),
         dig(gr, "slides[0].shape_tree.shapes[2].parent"),
         dig(gr, "slides[0].shape_tree.shapes[2].paragraphs_direct")],
        ["grpSp", "3",
         {"off": {"x": "0", "y": "0"}, "ext": {"cx": "2900000", "cy": "900000"},
          "chOff": {"x": "0", "y": "0"}, "chExt": {"cx": "2900000", "cy": "900000"}},
         3, 1, 1, 1],
    )
    check(
        "第 2 页整份清单是空的：`shapes_total` 0、`kinds_seen` 与 `distinct_names` 都是空表、"
        "`max_depth` 0 —— 「这一页一个形状都没有」交 0 而不是缺键（那页的 `spTree` 只剩一个 `grpSpPr`）",
        [dig(gr, "slides[1].shape_tree.available"),
         dig(gr, "slides[1].shape_tree.shapes_total"),
         dig(gr, "slides[1].shape_tree.kinds_seen"),
         dig(gr, "slides[1].shape_tree.shapes"),
         dig(gr, "slides[1].shape_tree.groups")],
        [True, 0, [], [], 0],
    )
    check(
        "LibreOffice 重写同一份：形状、组合、`chOff` / `chExt` 都保住，`id` 从 2..6 整批重排成 "
        "61..65，坐标走那条老换算（`100000` → `100080`、`2900000` → `2899800`），五个名字一字未动",
        [dig(lbin("office-slide", fixture("deck-gr-lo.pptx")), "slides[0].shape_tree.shapes_total"),
         dig(lbin("office-slide", fixture("deck-gr-lo.pptx")), "slides[0].shape_tree.groups"),
         dig(lbin("office-slide", fixture("deck-gr-lo.pptx")), "slides[0].shape_tree.shapes[0].id"),
         dig(lbin("office-slide", fixture("deck-gr-lo.pptx")), "slides[0].shape_tree.shapes[1].id"),
         dig(lbin("office-slide", fixture("deck-gr-lo.pptx")),
             "slides[0].shape_tree.shapes[0].xfrm.off"),
         dig(lbin("office-slide", fixture("deck-gr-lo.pptx")),
             "slides[0].shape_tree.shapes[1].xfrm.chExt")],
        [5, 1, "61", "62", {"x": "100080", "y": "100080"},
         {"cx": "2899800", "cy": "899640"}],
    )
    gr_odp = lbin("office-slide", fixture("deck-gr.odp"))
    check(
        "转成 odp：同一页还是 5 条、层级一样、五个名字都在 —— 但分组在这里叫 `svg:g`"
        "（`draw:group` 这份件里一次都没出现），坐标换成 `5.555cm` / `0.278cm` 这种自带单位的串，"
        "而 `id` 这一族**根本没有**（全 null）",
        [dig(gr_odp, "slides[0].shape_tree.shapes_total"),
         dig(gr_odp, "slides[0].shape_tree.top_level"),
         dig(gr_odp, "slides[0].shape_tree.nested"),
         dig(gr_odp, "slides[0].shape_tree.groups"),
         dig(gr_odp, "slides[0].shape_tree.kinds_seen"),
         dig(gr_odp, "slides[0].shape_tree.shapes[1].kind"),
         dig(gr_odp, "slides[0].shape_tree.shapes[0].id"),
         dig(gr_odp, "slides[0].shape_tree.shapes[0].size_written")],
        [5, 2, 3, 1, ["custom-shape", "g"], "g", None,
         {"x": "0.278cm", "y": "0.278cm", "width": "5.555cm", "height": "1.11cm"}],
    )
    check(
        "「字装在哪一层」在两族各有岔路：pptx 这边组合与框一律 `txBody`（`deck-pictures.pptx` "
        "第 1 页只有一张 `pic`，`carriers_seen` 是**空表**）；ODF 同一页里两种并存 —— "
        "`draw:frame` 装在 `draw:text-box` 里、`draw:custom-shape` 的 `text:p` 直接挂在形状自己身上，"
        "所以 `deck.odp` 第 1 页是 `[\"text-box\", \"self\"]`。按「有没有 text-box」数段，"
        "`deck.odp` 那一页从 4 段掉到 3 段，而 `deck-gr.odp` 那一页 4 段全没（那里三个框都是 "
        "custom-shape，`size_written` 之外一个口袋也不写）",
        [dig(gr, "slides[0].shape_tree.carriers_seen"),
         dig(gr, "slides[0].shape_tree.paragraphs_in_shapes"),
         dig(gr, "slides[0].shape_tree.shapes[1].text_carrier"),
         dig(gr, "slides[0].shape_tree.shapes[1].paragraphs_direct"),
         dig(lbin("office-slide", fixture("deck-pictures.pptx")),
             "slides[0].shape_tree.carriers_seen"),
         dig(lbin("office-slide", fixture("deck.odp")), "slides[0].shape_tree.carriers_seen"),
         dig(lbin("office-slide", fixture("deck.odp")),
             "slides[0].shape_tree.paragraphs_in_shapes")],
        [["txBody"], 4, None, 0, [], ["text-box", "self"], 4],
    )
    check(
        "`nested` 在两族不是同一个问：`deck.odp` 那一页里 `draw:image` 坐在 `draw:frame` 里面，"
        "于是第 1 页是 4 条、`nested` 1、`groups` 0，且那两条 unnamed 就是图 —— 与 pptx 的"
        "「深度 >0 必在组合里」不同义，两个数不互相解释；而 `.ppt` 那一族**不交这个键**"
        "（它的记录树没有形状树这一层，按 0x03EE 归页的账另在 `records`）",
        [dig(lbin("office-slide", fixture("deck.odp")), "slides[0].shape_tree.shapes_total"),
         dig(lbin("office-slide", fixture("deck.odp")), "slides[0].shape_tree.nested"),
         dig(lbin("office-slide", fixture("deck.odp")), "slides[0].shape_tree.groups"),
         dig(lbin("office-slide", fixture("deck.odp")), "slides[0].shape_tree.unnamed"),
         dig(lbin("office-slide", fixture("deck.pptx")), "slides[0].shape_tree.placeholders"),
         dig(lbin("office-slide", fixture("deck.ppt")), "slides[0].shape_tree")],
        [4, 1, 0, 1, 2, None],
    )
    # ── 3ap) 批注的回复与「已解决」：值在另外两份部件里，靠段号连，两跳各数断口 ──────
    print("=== 3ap) 批注的回复与已解决：一问四份数据、两跳，没写与写了 0 是两件事 ===")
    for name in sorted(one.name for one in FIXTURES.glob("*.docx")):
        got = lbin("office-doc", fixture(name))
        check("%s 回复/已解决那份账与读者一致（三份部件连两跳）" % name,
              dig(got, "structure.comment_threads"),
              files[name]["ooxml"]["comment_threads"])
    for name in sorted(one.name for one in FIXTURES.glob("*.odt")):
        got = lbin("office-doc", fixture(name))
        check("%s 已解决那一份账与读者一致（注自己身上，两份件都走）" % name,
              dig(got, "structure.comment_threads"),
              files[name]["odt"]["comment_threads"])
    cr = lbin("office-doc", fixture("crep.docx"))
    check(
        "`crep.docx` 把三种情形写全：2 条批注各有段号，`commentsExtended.xml` 三条里"
        "（`ext_total` 3）**只连上 2 条**（`ext_orphans` 1）、`commentsIds.xml` 同理 3 条里 1 条孤儿；"
        "已解决 1 条、明确写「没解决」1 条、回复 1 条且指得回第 0 条 —— "
        "「几条批注」与「几条线连上了」不是一件事",
        [dig(cr, "structure.comment_threads.comments_total"),
         dig(cr, "structure.comment_threads.paras_with_para_id"),
         dig(cr, "structure.comment_threads.ext_total"),
         dig(cr, "structure.comment_threads.ext_matched"),
         dig(cr, "structure.comment_threads.ext_orphans"),
         dig(cr, "structure.comment_threads.ids_total"),
         dig(cr, "structure.comment_threads.ids_orphans"),
         dig(cr, "structure.comment_threads.done_true"),
         dig(cr, "structure.comment_threads.done_false"),
         dig(cr, "structure.comment_threads.replies_total"),
         dig(cr, "structure.comment_threads.replies_dangling"),
         dig(cr, "structure.comment_threads.threads[1].replies_to"),
         dig(cr, "structure.comment_threads.threads[0].durable_id")],
        [2, 2, 3, 2, 1, 3, 1, 1, 1, 1, 0, 0, "1000"],
    )
    check(
        "同一份走 LibreOffice 的 docx→odt：**回复整条没有对应物**，而「已解决」换了地方也换了词 —— "
        "`loext:resolved` 两条都写 `false`，源件里那条 `done=\"1\"` **没落过来**"
        "（`resolved_true` 0）—— 生产者在导入时不看那一格，是这份件的事实",
        [dig(lbin("office-doc", fixture("crep.odt")),
           "structure.comment_threads.annotations_total"),
         dig(lbin("office-doc", fixture("crep.odt")),
            "structure.comment_threads.with_resolved_written"),
         dig(lbin("office-doc", fixture("crep.odt")), "structure.comment_threads.resolved_true"),
         dig(lbin("office-doc", fixture("crep.odt")), "structure.comment_threads.resolved_false"),
         dig(lbin("office-doc", fixture("crep.odt")),
            "structure.comment_threads.annotations[0].name_written") is not None,
         dig(lbin("office-doc", fixture("crep.odt")), "structure.comment_threads.parts_seen")],
        [2, 2, 0, 2, True, ["content.xml"]],
    )
    check(
        "反向证明这套词汇不是我编的：把 odt 那两格改成 `true`/`false` 再让 LibreOffice 导成 docx，"
        "它**自己写出** `word/commentsExtended.xml` —— 2 条批注里只给已解决那条写记录"
        "（`ext_total` 1、`done_true` 1），另一条是 `ex_found: false` 而**不是** `done_written: \"0\"`；"
        "段号也是它新排的（`01000000`），而 `commentsIds.xml` 整个不写",
        [dig(lbin("office-doc", fixture("crep-r.docx")), "structure.comment_threads.ext_total"),
         dig(lbin("office-doc", fixture("crep-r.docx")),
            "structure.comment_threads.ext_orphans"),
         dig(lbin("office-doc", fixture("crep-r.docx")),
            "structure.comment_threads.done_written_total"),
         dig(lbin("office-doc", fixture("crep-r.docx")), "structure.comment_threads.done_true"),
         dig(lbin("office-doc", fixture("crep-r.docx")), "structure.comment_threads.done_false"),
         dig(lbin("office-doc", fixture("crep-r.docx")),
            "structure.comment_threads.threads[1].ex_found"),
         dig(lbin("office-doc", fixture("crep-r.docx")),
            "structure.comment_threads.threads[1].done_written"),
         dig(lbin("office-doc", fixture("crep-r.docx")),
            "structure.comment_threads.threads[0].para_id"),
         dig(lbin("office-doc", fixture("crep-r.docx")),
            "structure.comment_threads.ids_part_written")],
        [1, 0, 1, 1, 0, False, None, "01000000", False],
    )
    check(
        "同一份件在 LibreOffice 手里走 docx→docx：两份部件**整个不见**，连批注体内那个 "
        "`w14:paraId` 也没了（`paras_with_para_id` 从 2 掉到 0）—— 于是回复与已解决两头都读不出来，"
        "交一串 0 与 `null` 而不是缺键；而 python-docx 那份（`comments.docx`）本来就是这个形状："
        "**有批注而这一格一个字都没写**",
        [dig(lbin("office-doc", fixture("crep-lo.docx")),
             "structure.comment_threads.comments_total"),
         dig(lbin("office-doc", fixture("crep-lo.docx")),
             "structure.comment_threads.paras_with_para_id"),
         dig(lbin("office-doc", fixture("crep-lo.docx")), "structure.comment_threads.ext_total"),
         dig(lbin("office-doc", fixture("crep-lo.docx")),
             "structure.comment_threads.ext_part_written"),
         dig(lbin("office-doc", fixture("crep-lo.docx")),
             "structure.comment_threads.threads[0].done"),
         dig(lbin("office-doc", fixture("comments.docx")),
             "structure.comment_threads.comments_total"),
         dig(lbin("office-doc", fixture("comments.docx")), "structure.comment_threads.ids_total")],
        [2, 0, 0, False, None, 2, 0],
    )
    check(
        "一份 odt 里两种答案并存（`crep-r.odt` 是我把第一格改成 `true` 的那份）："
        "`resolved_true` 1 而 `resolved_false` 1 —— 「这份文档的批注解决了几条」在 ODF 这一族"
        "数得出来；而回复那一问这一族没有位置，账上就不交那几格（不是 0）",
        [dig(lbin("office-doc", fixture("crep-r.odt")), "structure.comment_threads.resolved_true"),
         dig(lbin("office-doc", fixture("crep-r.odt")), "structure.comment_threads.resolved_false"),
         dig(lbin("office-doc", fixture("crep-r.odt")),
             "structure.comment_threads.without_resolved"),
         dig(lbin("office-doc", fixture("crep-r.odt")),
            "structure.comment_threads.annotations[1].resolved")],
        [1, 1, 0, False],
    )
    # ── 3as) 这一节从哪儿开始：默认值「另起一页」可以根本不写在文件里 ──────────────
    print("=== 3as) 分节起始类型：没说、说了默认、说了奇偶，是三种不同的文件 ===")
    for name in sorted(one.name for one in FIXTURES.glob("*.docx")):
        got = lbin("office-doc", fixture(name))
        check("%s 分节起始那份账与读者一致（元素在不在、写了哪个值）" % name,
              dig(got, "structure.section_starts"),
              files[name]["ooxml"]["section_starts"])
    ss = lbin("office-doc", fixture("sstart.docx"))
    check(
        "`sstart.docx` 三节：第一节把「另起一页」**设了却等于没说** —— 那是 Word 的默认值，"
        "python-docx 因此一个 `w:type` 都不写（`element_present` false、`type_written` null、"
        "`written` 空表），第二节 `continuous`、第三节 `evenPage` 才是写出来的；"
        "`with_element` 2 而 `sections_total` 3、`type_missing` 1",
        [dig(ss, "structure.section_starts.sections_total"),
         dig(ss, "structure.section_starts.with_element"),
         dig(ss, "structure.section_starts.type_missing"),
         dig(ss, "structure.section_starts.distinct_types"),
         dig(ss, "structure.section_starts.sections[0].element_present"),
         dig(ss, "structure.section_starts.sections[0].type_written"),
         dig(ss, "structure.section_starts.sections[0].written"),
         dig(ss, "structure.section_starts.sections[1].written"),
         dig(ss, "structure.section_starts.sections[2].type_written")],
        [3, 2, 1, ["continuous", "evenPage"], False, None, {},
         {"val": "continuous"}, "evenPage"],
    )
    check(
        "LibreOffice 重写同一份（`sstart-lo.docx`）：**第一节那一句被写出来了** —— "
        "`<w:type w:val=\"nextPage\"/>`，于是 `with_element` 从 2 变 3、`type_missing` 从 1 变 0、"
        "`distinct_types` 多出一个 nextPage；而 continuous 与 evenPage 两个值一字未变 —— "
        "这一族生产者改的是「说没说」，不是「说了什么」",
        [dig(lbin("office-doc", fixture("sstart-lo.docx")),
             "structure.section_starts.with_element"),
         dig(lbin("office-doc", fixture("sstart-lo.docx")),
            "structure.section_starts.type_missing"),
         dig(lbin("office-doc", fixture("sstart-lo.docx")),
            "structure.section_starts.distinct_types"),
         dig(lbin("office-doc", fixture("sstart-lo.docx")),
             "structure.section_starts.sections[0].element_present"),
         dig(lbin("office-doc", fixture("sstart-lo.docx")),
             "structure.section_starts.sections[0].written")],
        [3, 0, ["nextPage", "continuous", "evenPage"], True, {"val": "nextPage"}],
    )
    check(
        "反面凭据：`restart.docx`（写过页码起点的那一份）与 `notes.docx` 都只有一节而**一个 "
        "`w:type` 都没写** —— `sections_total` 1、`with_element` 0、`distinct_types` 空表，"
        "「这份文档分了几节」与「它说清每节怎么起头」是两问；"
        "ODF 那一族**不交这个键**（缺键 = 这一族没看）：LibreOffice 把「连续」转成一枚 "
        "`text:section`，而另起一页 / 偶数页那两节在 odt 里连一处写着起始类型的地方都没有，"
        "分页换成段落属性上的 `style:master-page-name` 承担 —— 一句问话被拆两处还有一半没落纸，"
        "所以不硬凑一个键（页版式与母版页另在 `page_numbering` / `header_footers` 记账）",
        [dig(lbin("office-doc", fixture("restart.docx")),
             "structure.section_starts.sections_total"),
         dig(lbin("office-doc", fixture("restart.docx")),
             "structure.section_starts.with_element"),
         dig(lbin("office-doc", fixture("restart.docx")),
             "structure.section_starts.distinct_types"),
         dig(lbin("office-doc", fixture("restart.odt")), "structure.section_starts"),
         dig(lbin("office-doc", fixture("tabs.rtf")), "structure.section_starts")],
        [1, 0, [], None, None],
    )
    # ── 3at) 结构搬进 markdown：两个生产者的渲染一字不差，而号写在哪一处不一样 ──────
    print("=== 3at) markdown：docx 的结构搬过去，逐字与第二读者对，两家的账各交各的 ===")
    for name in sorted(one.name for one in FIXTURES.glob("*.docx")):
        got = lbin("office-text", fixture(name), "--markdown")
        check("%s 的 markdown 那一本与读者一致（渲染逐字、计数逐格）" % name,
              got.get("markdown"), files[name]["ooxml"]["markdown"])
    hand = lbin("office-text", fixture("md.docx"), "--markdown")
    back = lbin("office-text", fixture("md-lo.docx"), "--markdown")
    hand_text = dig(hand, "markdown.text") or ""
    back_text = dig(back, "markdown.text") or ""
    check(
        "`md.docx` 每段只管一件事（标题 / 粗斜 / markdown 记号 / 两个空格的段 / 行首像记号的字 / "
        "两级列表 / 有序列表 / 带竖线与星号且有一格两段的表 / 站外链接 / 硬换行 / 图 / 空段 / 分页符），"
        "渲染出来的数是 18 块、10 段、2 标题、5 个列表项（3 圆点 + 2 编号）、1 张表 3 行、"
        "丢掉 2 个空段 —— `chars` 359 与 `cut` false 一起交，截没截由它自己说",
        [dig(hand, "markdown.blocks"), dig(hand, "markdown.paragraphs"),
         dig(hand, "markdown.headings"), dig(hand, "markdown.list_items"),
         dig(hand, "markdown.bullet_items"), dig(hand, "markdown.ordered_items"),
         dig(hand, "markdown.tables"), dig(hand, "markdown.table_rows"),
         dig(hand, "markdown.empty_dropped"), dig(hand, "markdown.chars"),
         dig(hand, "markdown.cut")],
        [18, 10, 2, 5, 3, 2, 1, 3, 2, 359, False],
    )
    check(
        "同一份稿子的两副件**渲染一字不差**，而账本说得出这一族改了什么：列表号在 python-docx 那份"
        "写在**样式**上（`list_from_style` 5），LibreOffice 重写时抄到**段上**（0）—— "
        "搬进 markdown 之后看不出来，因为它只问「这一段是不是列表项」",
        [hand_text == back_text, dig(hand, "markdown.list_from_style"),
         dig(back, "markdown.list_from_style"), len(hand_text), len(back_text)],
        [True, 5, 0, 359, 359],
    )
    check(
        "转义与不转义是分开的两件事：表外的竖线照字交（`|`）、表里的补一个反斜杠；"
        "行首长得像记号的那两句也补（`#` 与 `1.`），不然文件里写着的字会被读成标题与编号。"
        "还有一件容易被 trim 掉的：句子中间那两个空格（docx 靠 `xml:space=\"preserve\"` 存着）"
        "照字交回，一个都不缩",
        [hand_text.count("竖线 |"), hand_text.count("\\| 带竖线"),
         hand_text.startswith("# 结构：一级"), "\n\\# 这不是标题" in hand_text,
         "\n\\1. 这不是编号" in hand_text, hand_text.count("服务器 \\* 两台"),
         hand_text.count("两处空格  之间是一个记号"), back_text.count("两处空格  之间是一个记号")],
        [2, 1, True, True, True, 1, 1, 1],
    )
    lst = lbin("office-text", fixture("lists.docx"), "--markdown")
    lst_text = dig(lst, "markdown.text") or ""
    check(
        "`lists.docx` 是「层级与解不到」那一份凭据：直接挂在段上的第二级缩进两格（文件写了 "
        "`ilvl=1`），点了一个不存在的号与 LibreOffice 重排出来的 `numId=\"0\"` 都解不到格式 —— "
        "渲染挑了 `- ` 当最保守的标记，而 `unresolved_fmt` 2 把这件事说在账上（不藏进字符串里）",
        [dig(lst, "markdown.list_items"), dig(lst, "markdown.unresolved_fmt"),
         dig(lst, "markdown.bullet_items"), dig(lst, "markdown.ordered_items"),
         "  - 直接挂在段上的第二级" in lst_text, dig(lst, "markdown.list_from_style")],
        [6, 2, 4, 2, True, 3],
    )
    # 同一本账在 ODF 那一面：字在 `text:span` 上（一跳字符样式）、列表是嵌套元素、
    # 层级写在 `text:outline-level` 上、空格与换行是**记号**不是字
    for name in sorted(one.name for one in FIXTURES.glob("*.odt")):
        got = lbin("office-text", fixture(name), "--markdown")
        check("%s 的 markdown 那一本与读者一致（ODF：一跳样式、记号还原、块序一致）" % name,
              got.get("markdown"), files[name]["odt"]["markdown"])
    odt = lbin("office-text", fixture("md.odt"), "--markdown")
    odt_text = dig(odt, "markdown.text") or ""
    check(
        "`md.odt` 是同一份稿子的第三副样子（LibreOffice 的 docx → odt）：块数与两份 docx 一样是 18，"
        "而账上换了三格 —— 列表号一律来自 `text:list-style`（`lists_named` 3）、字要一跳字符样式"
        "（`spans_unresolved` 0 才算粗斜真落到字上）、空格记号要展开（`space_markers` 1）",
        [dig(odt, "markdown.blocks"), dig(odt, "markdown.paragraphs"),
         dig(odt, "markdown.headings"), dig(odt, "markdown.list_items"),
         dig(odt, "markdown.lists_named"), dig(odt, "markdown.spans_unresolved"),
         dig(odt, "markdown.space_markers"), dig(odt, "markdown.tables"),
         dig(odt, "markdown.empty_dropped"), dig(odt, "markdown.chars")],
        [18, 10, 2, 5, 3, 0, 1, 1, 2, 388],
    )
    check(
        "**跨族同形**：同一份稿子从 docx 与从 odt 搬进 markdown，35 行里**只有第 32 行不同** —— "
        "那一行是图片地址（各按自己文件写的交：`media/image1.png` 与 "
        "`Pictures/1000000100000008000000088E4DF5D4.png`，LibreOffice 在 ODF 里按内容哈希命名）；"
        "连「两处空格」那一句都一模一样（docx 写 `xml:space=\"preserve\"`、odt 写 `text:s` 记号，"
        "还原之后同一个串）",
        [len(hand_text.splitlines()), len(odt_text.splitlines()),
         [i for i, (x, y) in enumerate(zip(hand_text.splitlines(), odt_text.splitlines()))
          if x != y],
         hand_text.count("两处空格  之间是一个记号"),
         odt_text.count("两处空格  之间是一个记号"),
         hand_text.count("![](media/image1.png)"),
         odt_text.count("![](Pictures/")],
        [35, 35, [32], 1, 1, 1, 1],
    )
    ends = lbin("office-text", fixture("notes-end.odt"), "--markdown")
    ends_text = dig(ends, "markdown.text") or ""
    nso = lbin("office-text", fixture("notes.odt"), "--markdown")
    nso_text = dig(nso, "markdown.text") or ""
    tocs = lbin("office-text", fixture("toc.odt"), "--markdown")
    check(
        "ODF 那一族的两处跳过，都有真件数着：批注（LibreOffice 写作 `office:annotation`，"
        "就嵌在正文段**里面**）的字不进正文 —— `notes.odt` 里那句「这里要补上不含税口径」在渲染里"
        "一个都不剩（`annotations_dropped` 1）；注（`text:note`，脚注两枚 + 尾注一枚）同理，"
        "`notes-end.odt` 三条注正文一句都没落而所在段自己的字照旧（`notes_dropped` 3）。"
        "docx 那侧这些东西住在别的部件里、本来就不在正文，两族同一口径",
        [dig(nso, "markdown.annotations_dropped"), dig(nso, "markdown.notes_dropped"),
         dig(ends, "markdown.notes_dropped"), dig(ends, "markdown.annotations_dropped"),
         dig(tocs, "markdown.headings"), nso_text.count("这里要补上不含税口径"),
         ends_text.count("Footnote: the numbers are gross."),
         ends_text.count("Endnote: the totals"),
         ends_text.count("carries a footnote")],
        [1, 0, 3, 0, 2, 0, 0, 0, 1],
    )
    quiet = lbin("office-text", fixture("md.docx"))
    elsewhere = lbin("office-text", fixture("deck.ppt"), "--markdown")
    check(
        "没开 `--markdown` 就整个键都不给（不给一份空串装作渲染过）；开了而这一族还没搬的那一份，"
        "键也不在，只在 notes 里说一句（那句话点名交的是哪五族 —— RTF 从 37e702a 起在列，"
        "所以这里换成 `.ppt` 来当「还没搬的那一族」）",
        [quiet.get("markdown"), elsewhere.get("markdown"),
         any("markdown" in str(one) for one in (dig(elsewhere, "notes") or [])),
         any("这份件不是那五族" in str(one) for one in (dig(elsewhere, "notes") or []))],
        [None, None, True, True],
    )
    ported = lbin("office-text", fixture("notes.rtf"), "--markdown")
    check(
        "同一句「缺键 = 这一族还没搬」的反面：RTF 已经搬完，`notes.rtf` 开着 `--markdown` "
        "就有那一个键，notes 里也就没有那一句了（那一本整份账在 3b3 那条 lane 上与读者对过）",
        [ported.get("markdown") is not None,
         any("这份件不是那五族" in str(one) for one in (dig(ported, "notes") or []))],
        [True, False],
    )
    # ── 3b4) RTF 的段流水：一段一行整份列（headings / numbering.list / entries.list 都筛过）──
    print("=== 3b4) office-doc RTF structure.para_flow：逐行与读者的 para_rows 对 ===")
    for name in sorted(one.name for one in FIXTURES.glob("*.rtf")):
        got = lbin("office-doc", fixture(name))
        rows = files[name]["rtf"]["para_rows"]
        flow = dig(got, "structure.para_flow") or {}
        listed = flow.get("listed") or 0
        check("%s 的段流水逐行与读者一致（按 --limit 截之后）" % name,
              flow.get("rows"), rows[:listed])
        check("%s 段流水那几本计数都数得回来" % name,
              [flow.get("family"), flow.get("available"), flow.get("paragraphs"),
               flow.get("empty"), flow.get("headings"), flow.get("in_list"),
               flow.get("in_index"), flow.get("labelled"),
               flow.get("styles_unresolved"), flow.get("cut")],
              ["rtf", True, len(rows),
               len([one for one in rows if not one["text"]]),
               len([one for one in rows if one["heading_level"] is not None]),
               len([one for one in rows if one["in_list"]]),
               len([one for one in rows if one["in_index"]]),
               len([one for one in rows if one["label"] is not None]),
               len([one for one in rows
                    if one["style_index"] is not None and one["style_name"] is None]),
               len(rows) > listed])
        # 这道减法就是这一本存在的理由：流水整份数 = 老那本（不带空段的那本）+ 空段
        check("%s 的流水段数 = structure.paragraphs + empty" % name,
              [flow.get("paragraphs")
               == dig(got, "structure.paragraphs") + flow.get("empty"),
               flow.get("paragraphs") >= dig(got, "structure.paragraphs")],
              [True, True])
    few = lbin("office-doc", fixture("lists.rtf"), "--limit", "3")
    check("截断这一格是真截：lists.rtf 交 3 行、paragraphs 仍是整份的 7、cut 是 true",
          [len(dig(few, "structure.para_flow.rows")), dig(few, "structure.para_flow.paragraphs"),
           dig(few, "structure.para_flow.listed"), dig(few, "structure.para_flow.cut")],
          [3, 7, 3, True])
    pin = dig(lbin("office-doc", fixture("toc-full.rtf")), "structure.para_flow") or {}
    check("toc-full.rtf 那三格钉住：目录那一段的样式号 140 与名字 toc 1 是一起交的，"
          "而正文里那句「结构：一级」是另一个 in_index 为 false 的段（同一个字两处都在）",
          [[one["at"], one["text"], one["style_index"], one["style_name"], one["in_index"],
            one["heading_level"]] for one in pin.get("rows", [])[:6]],
          [[0, "目录", 139, "TOC Heading", False, None],
           [1, "结构：一级\t1", 140, "toc 1", True, None],
           [2, "结构：二级\t2", 141, "toc 2", True, None],
           [3, "结构：一级", 1, "heading 1", False, 1],
           [4, "", 0, "Normal", False, None],
           [5, "结构：二级", 2, "heading 2", False, 2]])

    # ── 3b6) 域那一份账：复杂式那条链、简单式那一行、ODF 的元素名、RTF 的那一群 ─────
    print("=== 3b6) office-doc structure.field_ledger：三家各写各的，逐行与第二读者对 ===")
    for name in sorted(one.name for one in FIXTURES.glob("*.docx")):
        check("%s 的域账整本与读者一致（链的断口、指令段数、标记与游离标记分开交）" % name,
              dig(lbin("office-doc", fixture(name)), "structure.field_ledger"),
              files[name]["ooxml"]["field_ledger"])
    for name in sorted(one.name for one in FIXTURES.glob("*.odt")):
        check("%s 的域账整本与读者一致（种类就是元素名，序列声明另交一本）" % name,
              dig(lbin("office-doc", fixture(name)), "structure.field_ledger"),
              files[name]["odt"]["field_ledger"])
    for name in sorted(one.name for one in FIXTURES.glob("*.rtf")):
        check("%s 的域账整本与读者一致（一枚 `\\field` 群一行，指令与结果各交各的）" % name,
              dig(lbin("office-doc", fixture(name)), "structure.field_ledger"),
              files[name]["rtf"]["field_ledger"])

    mix = dig(lbin("office-doc", fixture("fields-mix.docx")), "structure.field_ledger")
    lo = dig(lbin("office-doc", fixture("fields-mix-lo.docx")), "structure.field_ledger")
    check(
        "同一份稿子两家写：Word 那份 15 行 = 13 枚复杂式 + 2 枚 `w:fldSimple`，"
        "LibreOffice 重写那份只有 13 行 —— 两枚简单式被摊平成复杂式（`forms.simple` 2 → 0），"
        "`\\*` 从两枚剩一枚（`MERGEFORMAT` 不写了），`\\r` 从一枚变两枚（同一个开关写了两遍），"
        "`w:dirty` 整族不再写（`markers.dirty` 1 → 0、`dirty_on` 1 → 0）；"
        "反过来 LO 给每一枚都补齐了 separate/end（`unclosed` 1 → 0），却有一枚 begin 什么指令都没写"
        "（`no_instruction` 1，而 `markers.instrText` 12 比 `markers.begin` 13 少一根）",
        [mix["fields_total"], mix["forms"], lo["fields_total"], lo["forms"],
         mix["switch_tokens"], lo["switch_tokens"],
         [mix["markers"]["dirty"], lo["markers"]["dirty"]],
         [mix["unclosed"], lo["unclosed"]],
         [mix["no_instruction"], lo["no_instruction"]],
         [mix["markers"]["instrText"], lo["markers"]["instrText"],
          mix["markers"]["begin"], lo["markers"]["begin"]],
         [mix["dirty_on"], lo["dirty_on"]],
         [one["switches"] for one in mix["rows"] if one["kind"] == "REF"],
         [one["switches"] for one in lo["rows"] if one["kind"] == "REF"]],
        [15, {"simple": 2, "complex": 13}, 13, {"simple": 0, "complex": 13},
         {"\\*": 2, "\\r": 1, "\\h": 2}, {"\\*": 1, "\\r": 2, "\\h": 2},
         [1, 0], [1, 0], [0, 1], [13, 12, 13, 13], [1, 0],
         [["\\r", "\\h"]], [["\\r", "\\r", "\\h"]]],
    )
    check(
        "这一本的两条减法：复杂式的行数就是 `w:fldChar begin` 的枚数（没有别的形状能开出复杂式），"
        "而「闭合」的枚数 = `end` 减去游离在外的那几枚 —— 游离的 end 不算闭合，"
        "断在哪一行就报在哪一行（`closed` 交 false 而不是替它补一句）。"
        "「没带 separate」与「没闭合」是两行两回事：前者第 10 行，后者第 11 行",
        [mix["forms"]["complex"] == mix["markers"]["begin"],
         mix["markers"]["end"] - mix["loose"]["end"] == mix["forms"]["complex"] - mix["unclosed"],
         [one["index"] for one in mix["rows"] if one["closed"] is False],
         [one["index"] for one in mix["rows"] if one["has_separate"] is False]],
        [True, True, [11], [10]],
    )
    check(
        "没闭合的那一枚不是「少一句」而已：它的链还开着，后面每一段都被算进它的缓存字里"
        "（第 11 行 depth 仍是 0，可缓存串把第 12 行那句「空指令」的字也吞了；"
        "第 12 行是开在这条没闭合的链里，所以 depth 1）。"
        "而第 12 行自己指令为空 —— 空白指令与没指令是两格，`empty_instruction` 数它",
        [mix["rows"][11]["depth"], mix["rows"][11]["cached"], mix["rows"][11]["closed"],
         mix["rows"][12]["depth"], mix["rows"][12]["instruction"], mix["rows"][12]["kind"],
         mix["empty_instruction"], mix["nested"]],
        [0, "9空指令：书签所指：被指的那一句简单式那两枚：", False,
         1, "  ", None, 1, 3],
    )
    foot = dig(lbin("office-doc", fixture("fields.docx")), "structure.field_ledger")
    check(
        "这一本与老那一格（`structure.fields`）不是一个问：老那格数正文里出现过的域标记（9 枚，"
        "三枚域 × begin/separate/end），这一本按域逐行列、并且把跨部件那一跳也算进来 —— "
        "`parts` 里除 `word/document.xml` 那 3 行外还有 `word/footer1.xml` 的 1 行，"
        "页脚里那枚 PAGE 在正文那棵树里根本看不见",
        [foot["fields_total"], dig(lbin("office-doc", fixture("fields.docx")), "structure.fields"),
         foot["parts"], foot["kinds"], [one["part"] for one in foot["rows"]]],
        [4, 9, {"word/document.xml": 3, "word/footer1.xml": 1},
         {"SEQ": 1, "DATE": 1, "PAGE": 2},
         ["word/document.xml", "word/document.xml", "word/document.xml", "word/footer1.xml"]],
    )
    few = dig(lbin("office-doc", fixture("fields-mix.docx"), "--limit", "3"),
              "structure.field_ledger")
    check(
        "截断这一格是真截：`rows` 交 3 行、`listed` 3、`cut` true，"
        "可 `fields_total` 与三本簿（kinds / switch_tokens / parts）仍是整份的账 —— "
        "限额只管列多少行，不管这份文件里有几枚域",
        [len(few["rows"]), few["listed"], few["cut"], few["fields_total"],
         few["kinds"]["PAGE"], few["parts"]["word/document.xml"]],
        [3, 3, True, 15, 5, 15],
    )
    odt = dig(lbin("office-doc", fixture("fields-mix.odt")), "structure.field_ledger")
    check(
        "ODF 那一族没有「域指令」这回事：种类就是元素名，`instruction` 与 `switches` 整本为空；"
        "docx 的 MERGEFIELD 在这里叫 `text:database-display`，"
        "而链接是 `text:a` —— 它不是一门域，所以 `kinds` 里查不到 hyperlink；"
        "两枚 `text:bookmark-ref` 靠 `text:reference-format` 分成 number / page 两种读法，"
        "所以种类那一本数 2 而格式那一本各数 1；序列号还要先有声明（`sequence_declarations` 6 条）",
        [odt["fields_total"],
         [one["index"] for one in odt["rows"] if one["instruction"] is not None],
         [one["index"] for one in odt["rows"] if one["switches"]],
         "hyperlink" in odt["kinds"], odt["kinds"]["bookmark-ref"],
         odt["kinds"]["database-display"], odt["reference_formats"],
         odt["sequence_declarations"], odt["sequence_declared"],
         odt["kinds"]["page-number"]],
        [11, [], [], False, 2, 1, {"number": 1, "page": 1}, 6,
         ["Drawing", "Figure", "Illustration", "Table", "Text", "图"], 4],
    )
    odt_page = dig(lbin("office-doc", fixture("fields.odt")), "structure.field_ledger")
    check(
        "跨部件那一跳在 ODF 这里是 styles.xml：页码那枚域写在页版式的样式里，不在正文，"
        "所以 `parts` 两格分得清清楚楚（content.xml 3、styles.xml 1）—— "
        "只读正文的读者会少报这一枚，而它正是「第几页」那一个字",
        [odt_page["parts"], odt_page["fields_total"],
         [(one["part"], one["kind"]) for one in odt_page["rows"]]],
        [{"content.xml": 3, "styles.xml": 1}, 4,
         [("content.xml", "sequence"), ("content.xml", "date"),
          ("content.xml", "page-number"), ("styles.xml", "page-number")]],
    )
    rtf = dig(lbin("office-doc", fixture("fields-mix.rtf")), "structure.field_ledger")
    check(
        "RTF 一本 14 行 = 流里 14 枚 `\\field` 群（`control_words` 同数，一个群一行）；"
        "「空指令」那一枚连群里的字都解不出来，`instruction` 与 `kind` 一起交 null，"
        "可它照样有一行 —— 因为群在场；显示文字取 `\\fldrslt`，页码域的缓存就是页上的那个数。"
        "同一个 REF 种子在这一族解完转义顺手 trim（`REF _RefMix1 \\r \\r \\h`），"
        "docx 那本交的是原样带前后空格的一串，所以比开关不比整串",
        [rtf["fields_total"], rtf["control_words"],
         [one["index"] for one in rtf["rows"] if one["instruction"] is None],
         rtf["rows"][6]["switches"], lo["rows"][6]["switches"],
         rtf["rows"][6]["instruction"], lo["rows"][6]["instruction"],
         rtf["rows"][13]["cached"], rtf["kinds"]["PAGE"], rtf["cached_empty"],
         rtf["rows"][11]["cached"]],
        [14, 14, [11], ["\\r", "\\r", "\\h"], ["\\r", "\\r", "\\h"],
         "REF _RefMix1 \\r \\r \\h", " REF _RefMix1 \\r \\r \\h ",
         "站内字样", 4, 1, "空"],
    )
    toc_rtf = dig(lbin("office-doc", fixture("toc.rtf")), "structure.field_ledger")
    no_field = dig(lbin("office-doc", fixture("lists.rtf")), "structure.field_ledger")
    check(
        "`toc` 那一格与目录那本账共用一把尺子：解出来的指令第一个字是 TOC 才算一行（2 行里 1 行是），"
        "而一份全是列表、一枚域都没有的流两本账都是零 —— 账本仍然 available，"
        "「读了，没有」与「没读这一族」是两个答案",
        [toc_rtf["fields_total"], toc_rtf["toc"], toc_rtf["kinds"], toc_rtf["switch_tokens"],
         no_field["available"], no_field["fields_total"], no_field["rows"],
         no_field["control_words"]],
        [2, 1, {"TOC": 1, "HYPERLINK": 1}, {"\\o": 1, "\\h": 1},
         True, 0, [], 0],
    )

    # ── 3b5) 这一串字是横着走还是竖着走：一家五处写在身上，一家四处全在样式上 ──────
    print("=== 3b5) 文字走向：五处各说各的，两种词法、三处样式列表，一处也不合并 ===")
    for name in sorted(one.name for one in FIXTURES.glob("*.docx")):
        got = lbin("office-doc", fixture(name))
        check("%s 走向那份账与读者一致（格、表、段、字、节五处分开交）" % name,
              dig(got, "structure.text_direction"),
              files[name]["ooxml"]["text_direction"])
    for name in sorted(one.name for one in FIXTURES.glob("*.odt")):
        got = lbin("office-doc", fixture(name))
        check("%s 走向那份账与读者一致（四处全在样式上，词法与样式列表各交各的）" % name,
              dig(got, "structure.text_direction"),
              files[name]["odt"]["text_direction"])
    dc = lbin("office-doc", fixture("dir-cell.docx"))
    dc_lo = lbin("office-doc", fixture("dir-cell-lo.docx"))
    ds = lbin("office-doc", fixture("dir-sect.docx"))
    do = lbin("office-doc", fixture("dir-cell.odt"))
    ds_odt = lbin("office-doc", fixture("dir-sect.odt"))
    check(
        "python-docx 那份（一格一个枚举值）：五格各写 `w:textDirection` 的一个值，五个枚举**一个都不折算**；"
        "`w:bidiVisual` 写出来是**空元素**（在场=开着，值整个没有），而段上那枚 `w:bidi` 写着 `1`；"
        "节上两处都没有 —— `text_direction` 是 null 而 `text_direction_present` 是 false，"
        "这两个键一起交才不会把「没写」当成「写了空」",
        [dig(dc, "structure.text_direction.tables_total"),
         dig(dc, "structure.text_direction.cells_total"),
         dig(dc, "structure.text_direction.cells_written"),
         [one["val"] for one in dig(dc, "structure.text_direction.cells")],
         dig(dc, "structure.text_direction.tables[1].bidi_visual"),
         dig(dc, "structure.text_direction.paragraphs_total"),
         dig(dc, "structure.text_direction.paragraphs_written"),
         dig(dc, "structure.text_direction.paragraphs_indexed"),
         dig(dc, "structure.text_direction.sections[0]"),
         dig(dc, "structure.text_direction.values_written")],
        [2, 10, 5, ["lrTb", "tbRl", "btLr", "lrTbV", "tbRlV"],
         {"present": True, "val": None, "on_written": True, "off_written": False},
         14, 2, [1, 2],
         {"index": 0, "bidi": {"present": False, "val": None, "on_written": False,
                               "off_written": False},
          "text_direction": None, "text_direction_present": False},
         {"textDirection=lrTb": 1, "textDirection=tbRl": 1, "textDirection=btLr": 1,
          "textDirection=lrTbV": 1, "textDirection=tbRlV": 1, "bidiVisual bare": 1,
          "bidi with_value": 1, "rtl with_value": 1}],
    )
    check(
        "LibreOffice 重写同一份：**说了等于没说的那两格整个没了**（`lrTb`、`lrTbV` 不写，"
        "`tbRlV` 被换成 `tbRl`，所以 `tbRl` 那一枚数是 2 而不是 1）；反过来给每段各补一句 "
        "`w:bidi w:val=\"0\"`（关掉也要写出来，交着话的段 2 段 → 11 段），"
        "而 `w:bidiVisual` 这次带着值、节上多了一枚 `w:textDirection w:val=\"lrTb\"` —— "
        "同一个意思同一份稿子，两处一处多一处少，所以没有「这份文档是不是竖排」这么一个数",
        [dig(dc_lo, "structure.text_direction.cells_written"),
         [one["val"] for one in dig(dc_lo, "structure.text_direction.cells")],
         dig(dc_lo, "structure.text_direction.tables[1].bidi_visual.val"),
         dig(dc_lo, "structure.text_direction.paragraphs_written"),
         dig(dc_lo, "structure.text_direction.paragraphs_indexed"),
         dig(dc_lo, "structure.text_direction.sections[0].text_direction"),
         dig(dc_lo, "structure.text_direction.sections[0].text_direction_present"),
         dig(dc_lo, "structure.text_direction.values_written")],
        [3, ["tbRl", "btLr", "tbRl"], "true", 11, [1, 3, 4, 5, 6, 7, 8, 10, 11, 12, 13],
         "lrTb", True,
         {"textDirection=tbRl": 2, "textDirection=btLr": 1, "bidiVisual with_value": 1,
          "bidi with_value": 11, "rtl with_value": 1, "sectPr textDirection=lrTb": 1}],
    )
    check(
        "只在节上说的一句话（`dir-sect.docx`）：格、表、段三处全是空的（`cells_written` 0、"
        "`paragraphs_written` 0），只有 `w:sectPr/w:bidi` 在场且 `on_written` —— "
        "只看段与格的读者会把这份件报成「没有走向这回事」",
        [dig(ds, "structure.text_direction.cells_written"),
         dig(ds, "structure.text_direction.paragraphs_written"),
         dig(ds, "structure.text_direction.sections_total"),
         dig(ds, "structure.text_direction.sections[0].bidi"),
         dig(ds, "structure.text_direction.values_written")],
        [0, 0, 1,
         {"present": True, "val": "1", "on_written": True, "off_written": False},
         {"sectPr bidi with_value": 1}],
    )
    check(
        "同一问转 ODF：正文里**一个走向字都不写**（十格全靠样式名 `表格1.B1` 这类地址式自动样式），"
        "而且**两种词法**：`bt-lr` 那一格走的是 LibreOffice 扩展的 `loext:writing-mode`，"
        "局部名与 `style:writing-mode` 一模一样 —— 只按局部名收就会互相盖掉，所以每行都带 `vocabulary`；"
        "枚举值也是这一族自己的写法（`tb-rl`/`bt-lr`/`rl-tb`/`lr-tb`/`page`），与 OOXML 那五个不通用",
        [dig(do, "structure.text_direction.cells_total"),
         dig(do, "structure.text_direction.cells_named"),
         dig(do, "structure.text_direction.cells_found"),
         dig(do, "structure.text_direction.cells_written"),
         dig(do, "structure.text_direction.distinct_cell_styles"),
         dig(do, "structure.text_direction.cells_from"),
         [(one["style"], one["value"], one["vocabulary"])
          for one in dig(do, "structure.text_direction.cells")],
         dig(do, "structure.text_direction.values_written")],
        [10, 10, 10, 8, 4, {"automatic": 8, "named": 0, "other": 0},
         [("表格1.B1", "tb-rl", "style"), ("表格1.C1", "bt-lr", "loext"),
          ("表格1.D1", "lr-tb", "style"), ("表格1.B1", "tb-rl", "style"),
          ("表格2.A1", "rl-tb", "style"), ("表格2.A1", "rl-tb", "style"),
          ("表格2.A1", "rl-tb", "style"), ("表格2.A1", "rl-tb", "style")],
         {"style:writing-mode=page": 1, "style:writing-mode=tb-rl": 2,
          "loext:writing-mode=bt-lr": 1, "style:writing-mode=lr-tb": 15,
          "style:writing-mode=rl-tb": 6}],
    )
    check(
        "样式分两处住，同一枚值来自哪一处是两个问题：14 段里 13 段是靠**命名样式** `Standard` "
        "那枚默认值才「说了话」（`declared_in` 是 `styles`、`style_part` 是 `styles.xml`），"
        "只有第 2 段是自己那份自动样式 `P1` 写着 `rl-tb`（在 `content.xml` 里）—— "
        "合并成「14 段都竖排」就把「文件说了」与「默认值替它说了」混成一个数",
        [dig(do, "structure.text_direction.paragraphs_total"),
         dig(do, "structure.text_direction.paragraphs_written"),
         dig(do, "structure.text_direction.paragraphs_from"),
         [(one["style"], one["value"], one["declared_in"], one["style_part"])
          for one in [dig(do, "structure.text_direction.paragraphs[0]"),
                      dig(do, "structure.text_direction.paragraphs[1]")]],
         sum(1 for one in dig(do, "structure.text_direction.paragraphs")
             if one["declared_in"] == "styles")],
        [14, 14, {"automatic": 1, "named": 13, "other": 0},
         [("Standard", "lr-tb", "styles", "styles.xml"),
          ("P1", "rl-tb", "automatic-styles", "content.xml")],
         13],
    )
    check(
        "页面那一处挂的**不是** `style:style`：它坐在 `style:page-layout` 的 "
        "`style:page-layout-properties` 上（实测 `Mpm1`），母版页那一跳没量过所以不判落在哪一页，"
        "只交「哪些定义写了它」；`dir-sect.odt` 的 `Mpm1` 写着 `rl-tb` —— 这一枚就是 OOXML 那份"
        "写在**节**上的 `w:bidi` 的去处（同一句「整份文档倒过来」在两族落在不同的层）",
        [dig(do, "structure.text_direction.page_definitions_total"),
         [(one["style"], one["value"], one["part"])
          for one in dig(do, "structure.text_direction.page_definitions")],
         dig(ds_odt, "structure.text_direction.page_definitions[0].value"),
         dig(ds_odt, "structure.text_direction.cells_written"),
         dig(ds_odt, "structure.text_direction.paragraphs_written"),
         dig(ds_odt, "structure.text_direction.values_written"),
         dig(ds, "structure.text_direction.sections[0].bidi.on_written")],
        [1, [("Mpm1", "lr-tb", "styles.xml")], "rl-tb", 0, 4,
         {"style:writing-mode=page": 1, "style:writing-mode=lr-tb": 4,
          "style:writing-mode=rl-tb": 1},
         True],
    )
    check(
        "跨族对照里最值钱的两条，两族各按自己写的词交、不互相翻译：表上那句在 OOXML 是 "
        "`w:tblPr/w:bidiVisual`（值可以整个没有），在 ODF 是表样式那枚 `rl-tb`；"
        "而 ODF 多出来的那个枚举 `page` 在 OOXML 那边根本没有 —— 它是「这张表自己不说、由页面定」",
        [dig(dc, "structure.text_direction.tables[1].bidi_visual.present"),
         dig(do, "structure.text_direction.tables[1].value"),
         dig(do, "structure.text_direction.tables[0].value"),
         dig(do, "structure.text_direction.tables_written"),
         dig(do, "structure.text_direction.tables_from")],
        [True, "rl-tb", "page", 2, {"automatic": 2, "named": 0, "other": 0}],
    )
    rtf_dir = fixture("dir-cell.rtf").read_bytes().decode("latin-1")
    check(
        "RTF 与遗留 .doc 这一格**不交**：那一族把同一件事写成 `\\cltxtbrl`（格，实测 ×2）、"
        "`\\cltxbtlr`（格，×1）、`\\rtlrow`（行，×2 —— OOXML 写在表上的一句在这里落到**每一行**）、"
        "`\\rtlpar`（段，×1）与 `\\ltrpar`（×27，默认值被逐段重发），而 `\\rtlcol` 一个都没有；"
        "归属要按行群与格群切开才判得住，本读者在这一族连「几张表」都判不住（fact 100），"
        "所以规则记在模块头上、交回来的是缺键而不是一个猜的数",
        [rtf_dir.count("\\cltxtbrl"), rtf_dir.count("\\cltxbtlr"), rtf_dir.count("\\rtlrow"),
         rtf_dir.count("\\rtlpar"), rtf_dir.count("\\ltrpar") - rtf_dir.count("\\rtlpar"),
         rtf_dir.count("\\rtlcol"),
         dig(lbin("office-doc", fixture("dir-cell.rtf")), "structure.text_direction"),
         dig(lbin("office-doc", fixture("eq.doc")), "structure.text_direction")],
        [2, 1, 2, 1, 26, 0, None, None],
    )
    # ── 3au) 文档里的公式：行内与独立成行会被生产者改，ODF 一条式子住在另一个部件 ──────
    print("=== 3au) 公式：OMML 的挂法与 MathML 的部件，两家各按自己文件写的交 ===")
    for name in sorted(one.name for one in FIXTURES.glob("*.docx")):
        got = lbin("office-doc", fixture(name))
        check("%s 的公式那份账与读者一致（OMML：挂法、对齐、结构名、式子里的字）" % name,
              dig(got, "structure.equations"), files[name]["ooxml"]["equations"])
    for name in sorted(one.name for one in FIXTURES.glob("*.odt")):
        got = lbin("office-doc", fixture(name))
        check("%s 的公式那份账与读者一致（ODF：draw:object 那一跳、MathML 元素与字）" % name,
              dig(got, "structure.equations"), files[name]["odt"]["equations"])
    hand = lbin("office-doc", fixture("eq.docx"))
    rewrote = lbin("office-doc", fixture("eq-lo.docx"))
    check(
        "`eq.docx` 六条式子：三条行内（`m:oMath` 直接挂 `w:p`）、三条独立成行（在 `m:oMathPara` 里），"
        "六段各有至少一条；只有一条自己写了对齐（`align_written_total` 1，值是 `centerGroup`），"
        "点名成普通字的 `m:nor` 一枚，`m:t` 里的字一共 13 个码位 —— 结构名按文档顺序交"
        "（分数 `f`、上标 `sSup`、根号 `rad` 带 `degHide`、括号 `d` 带 `begChr`/`endChr`）",
        [dig(hand, "structure.equations.equations_total"),
         dig(hand, "structure.equations.inline_total"),
         dig(hand, "structure.equations.display_total"),
         dig(hand, "structure.equations.paragraphs_with"),
         dig(hand, "structure.equations.paragraphs_total"),
         dig(hand, "structure.equations.align_written_total"),
         dig(hand, "structure.equations.nor_runs"),
         dig(hand, "structure.equations.lit_runs"),
         dig(hand, "structure.equations.math_runs"),
         dig(hand, "structure.equations.text_chars"),
         dig(hand, "structure.equations.o_math_para_total"),
         dig(hand, "structure.equations.items[0].placement"),
         dig(hand, "structure.equations.items[0].host"),
         dig(hand, "structure.equations.items[0].paragraph"),
         dig(hand, "structure.equations.items[0].structures"),
         dig(hand, "structure.equations.items[0].text"),
         dig(hand, "structure.equations.items[3].align_written")],
        [6, 3, 3, 6, 7, 1, 1, 0, 12, 13, 3,
         "inline", "w:p", 0, ["f", "num", "den"], "ab", "centerGroup"],
    )
    check(
        "LibreOffice 重写同一份（`eq-lo.docx`）改了三处，账本一处都不遮：行内 3 → 2、独立 3 → 4"
        "（两条行内式被升级成 `m:oMathPara`）；对齐从 1 条变 4 条 —— 作者写的 `centerGroup` 变成 "
        "`center`，另外三条原本一个字没写的都补上了值；`m:nor` 那一条被补了一枚 `m:lit`。"
        "而式子里的字一个都没动（`text_chars` 仍 13、`math_runs` 仍 12）",
        [dig(rewrote, "structure.equations.inline_total"),
         dig(rewrote, "structure.equations.display_total"),
         dig(rewrote, "structure.equations.align_written_total"),
         dig(rewrote, "structure.equations.o_math_para_total"),
         dig(rewrote, "structure.equations.items[3].align_written"),
         dig(rewrote, "structure.equations.lit_runs"),
         dig(rewrote, "structure.equations.nor_runs"),
         dig(rewrote, "structure.equations.text_chars"),
         dig(rewrote, "structure.equations.math_runs"),
         dig(rewrote, "structure.equations.equations_total")],
        [2, 4, 4, 4, "center", 1, 1, 13, 12, 6],
    )
    odt = lbin("office-doc", fixture("eq.odt"))
    check(
        "同一份转成 odt 之后，一条式子搬进**另一份部件**：六枚 `draw:frame`"
        "（`text:anchor-type` 写 `as-char`、尺寸写成 `0.314cm` 这种自带单位的串）里 "
        "`draw:object` 的 `xlink:href` 指向 `./Object N`，部件 `Object N/content.xml` 全在"
        "（`parts_found` 6 / `parts_missing` 0），里面都有 `<math>` 根（`math_found` 6）；"
        "另有六枚替位图地址（`replacements_written` 6）也按写的交。"
        "**行内与独立在这一族分不出来**：六条的 `display` 一律写 `block`"
        "（`inline_written` 0），所以只交「按写的几个 block」，不猜原本是哪种",
        [dig(odt, "structure.equations.equations_total"),
         dig(odt, "structure.equations.frames_seen"),
         dig(odt, "structure.equations.objects_total"),
         dig(odt, "structure.equations.math_found"),
         dig(odt, "structure.equations.objects_without_math"),
         dig(odt, "structure.equations.parts_found"),
         dig(odt, "structure.equations.parts_missing"),
         dig(odt, "structure.equations.block_written"),
         dig(odt, "structure.equations.inline_written"),
         dig(odt, "structure.equations.annotations_found"),
         dig(odt, "structure.equations.replacements_written"),
         dig(odt, "structure.equations.paragraphs_total"),
         dig(odt, "structure.equations.items[0].anchor_written"),
         dig(odt, "structure.equations.items[0].width_written"),
         dig(odt, "structure.equations.items[0].object_target"),
         dig(odt, "structure.equations.items[0].object_part"),
         dig(odt, "structure.equations.items[0].display_written"),
         dig(odt, "structure.equations.items[0].annotation_source")],
        [6, 6, 6, 6, 0, 6, 0, 6, 0, 6, 6, 7,
         "as-char", "0.314cm", "./Object 1", "Object 1/content.xml", "block",
         "{a} over {b}"],
    )
    omml_text = [dig(hand, "structure.equations.items[%d].text" % i) for i in range(6)]
    math_text = [dig(odt, "structure.equations.items[%d].text" % i) for i in range(6)]
    check(
        "**跨族同问，两族的「字」不一样长**：六条式子在 OMML 里是 ab / x2 / 12 / n / 合计=12 / n，"
        "在 MathML 里是 ab / x2 / 12 / **[n]** / 合计=12 / **[n]** —— 括号在 OMML 是 `m:d` 的属性"
        "（`m:begChr` / `m:endChr`），到 MathML 成了 `mo` **元素**，于是第 3、5 条差出来；"
        "两边 `text_chars` 因此是 13 与 17，各按自己文件写着的交，不拿一边补另一边",
        [omml_text, math_text,
         [i for i, (x, y) in enumerate(zip(omml_text, math_text)) if x != y],
         dig(hand, "structure.equations.text_chars"),
         dig(odt, "structure.equations.text_chars")],
        [["ab", "x2", "12", "n", "合计=12", "n"],
         ["ab", "x2", "12", "[n]", "合计=12", "[n]"],
         [3, 5], 13, 17],
    )
    round_trip = lbin("office-doc", fixture("eq-od.docx"))
    check(
        "从这个 odt 再回转成 docx（`eq-od.docx`）：账与 LibreOffice 那次 docx → docx 的重写**一格不差**"
        "（MathML 回到 OMML 之后挂法、对齐、结构名与字都落在同一份账上）—— "
        "两条路走出来的两副件在这一本上同形，所以不需要替文件合并任何一格",
        dig(round_trip, "structure.equations"),
        dig(rewrote, "structure.equations"),
    )
    quiet = lbin("office-doc", fixture("md.docx"))
    check(
        "没有公式的件给 0（数过了没有），不是缺键；而 `.doc` / RTF 这些族**整个不交这个键**"
        "（缺键 = 这一族还没读，不是 0）：那几族把式子内嵌成字段/对象是另一套记号，"
        "本机没有能写出这些件的生产者",
        [dig(quiet, "structure.equations.equations_total"),
         dig(quiet, "structure.equations.available"),
         dig(lbin("office-doc", fixture("tabs.rtf")), "structure.equations"),
         dig(lbin("office-doc", fixture("notes.odt")), "structure.equations.frames_seen"),
         dig(lbin("office-doc", fixture("images.odt")), "structure.equations.objects_total")],
        [0, True, None, 1, 0],
    )
    # ── 3av) 放映里的公式：一条式子一个部件，而页缩略图也是 frame，两格必须分开数 ──────
    print("=== 3av) odp 的公式：frame → object → Object N/content.xml，重写改了哪几格 ===")
    for name in sorted(one.name for one in FIXTURES.glob("*.odp")):
        got = lbin("office-slide", fixture(name))
        check("%s 的公式那份账与读者一致（每页的 frame/object/部件、缩略图另记一格）" % name,
              got.get("equations"), files[name]["odp"]["equations"])
    hand = lbin("office-slide", fixture("eqs.odp"))
    rewrote = lbin("office-slide", fixture("eqs-lo.odp"))
    check(
        "`eqs.odp` 两页、三条 frame、两枚 `draw:object`：每页一条式子，各自住在 "
        "`Object N/content.xml` 的 MathML 里（`parts_found` 2 / `parts_missing` 0，"
        "里面都有 `<math>` 根 —— `math_found` 2、`objects_without_math` 0）；"
        "frame 写的都是 `text:anchor-type=\"as-char\"`（`anchors_written` 2），"
        "尺寸是自带单位的串（`3.261cm`），第一条还有一枚替位图地址在包里（`replacement_found` true）",
        [dig(hand, "equations.pages_total"), dig(hand, "equations.frames_seen"),
         dig(hand, "equations.frames_in_notes"), dig(hand, "equations.page_thumbnails"),
         dig(hand, "equations.objects_total"), dig(hand, "equations.math_found"),
         dig(hand, "equations.objects_without_math"), dig(hand, "equations.parts_found"),
         dig(hand, "equations.anchors_written"), dig(hand, "equations.replacements_written"),
         dig(hand, "equations.block_written"), dig(hand, "equations.inline_written"),
         dig(hand, "equations.annotations_found"), dig(hand, "equations.text_chars"),
         dig(hand, "equations.equations_total"),
         dig(hand, "equations.pages"),
         dig(hand, "equations.items[0].frame_name"),
         dig(hand, "equations.items[0].width_written"),
         dig(hand, "equations.items[0].object_part"),
         dig(hand, "equations.items[0].annotation_source")],
        [2, 3, 0, 0, 2, 2, 0, 2, 2, 1, 2, 0, 2, 4, 2,
         [{"page": 0, "frames": 1, "frames_in_notes": 0, "thumbnails": 0, "formulas": 1},
          {"page": 1, "frames": 2, "frames_in_notes": 0, "thumbnails": 0, "formulas": 1}],
         "对象1", "3.261cm", "Object 1/content.xml", "{a} over {b}"],
    )
    check(
        "LibreOffice 把同一份 odp 重写一遍，**式子一条不少、字一字不变**，而三格变了："
        "`text:anchor-type` 全被丢掉（`anchors_written` 2 → 0）、每页补一枚 `draw:frame` 装 "
        "`draw:page-thumbnail`（`frames_in_notes` 0 → 2、`page_thumbnails` 0 → 2 —— 这一族页缩略图"
        "挂在 `presentation:notes` 里，若不分开数，「页上有几枚 frame」就从 3 变 5），"
        "frame 的样式名从 `fr1` 换成 `gr1`；还给第二枚对象写了 `./ObjectReplacements/Object 2`，"
        "**可这个部件既不在包里、清单里也没有这一条**（`replacements_written` 1 → 2 而 "
        "`replacements_missing` 1）。页上的 frame 数本身没动（3）—— 所以两处都得交",
        [dig(rewrote, "equations.frames_seen"), dig(rewrote, "equations.frames_in_notes"),
         dig(rewrote, "equations.page_thumbnails"), dig(rewrote, "equations.anchors_written"),
         dig(rewrote, "equations.replacements_written"), dig(rewrote, "equations.replacements_missing"),
         dig(rewrote, "equations.items[0].style_written"),
         dig(rewrote, "equations.items[1].replacement_target"),
         dig(rewrote, "equations.items[1].replacement_found"),
         dig(rewrote, "equations.objects_total"), dig(rewrote, "equations.math_found")],
        [3, 2, 2, 0, 2, 1, "gr1", "./ObjectReplacements/Object 2", False, 2, 2],
    )
    check(
        "跨件同形：两份件的「式子里的字」与线性源一模一样（`ab` / `12`、"
        "`{a} over {b}` / `sqrt {1 2}`）—— 重写改的是挂法与引用，不是内容；"
        "而 `elements_seen` 两副件也一致（MathML 那 8 个元素名）",
        [[dig(hand, "equations.items[%d].text" % i) for i in (0, 1)],
         [dig(rewrote, "equations.items[%d].text" % i) for i in (0, 1)],
         [dig(hand, "equations.items[%d].annotation_source" % i) for i in (0, 1)],
         dig(hand, "equations.elements_seen") == dig(rewrote, "equations.elements_seen")],
        [["ab", "12"], ["ab", "12"], ["{a} over {b}", "sqrt {1 2}"], True],
    )
    check(
        "没有公式的放映也给「数过了没有」而不是缺键，但要紧的是**同一个数在另一份件上不是 0**："
        "`deck.odp` 一份公式都没有（`objects_total` 0、`math_found` 0、式子里的字 0 个码位），"
        "可它页上仍有 5 枚 `draw:frame`、注块里 3 枚、页缩略图 2 枚 —— 把「frame 数」当「公式数」"
        "就会在这里说谎。`*.pptx` 那一份另起一本（见 3aw）：LibreOffice 把 odp 的式子写成"
        "文本体里的 OMML（`a14:m` 套 `m:oMath`）外加 `mc:Fallback` 里的一张 EMF，"
        "那一族的形状、段号与页级三格都在 3aw 那一本上对。`.ppt` 仍旧不交这个键，但这是量过的："
        "odp → ppt 那一转把式子变成 `Pictures` 里的一笔位图，容器里连 `ObjectPool` 都没有"
        "（见事实 114）",
        [dig(lbin("office-slide", fixture("deck.odp")), "equations.equations_total"),
         dig(lbin("office-slide", fixture("deck.odp")), "equations.frames_seen"),
         dig(lbin("office-slide", fixture("deck.odp")), "equations.frames_in_notes"),
         dig(lbin("office-slide", fixture("deck.odp")), "equations.page_thumbnails"),
         dig(lbin("office-slide", fixture("deck.odp")), "equations.objects_total"),
         dig(lbin("office-slide", fixture("deck.pptx")), "equations.equations_total"),
         dig(lbin("office-slide", fixture("deck.pptx")), "equations.slides_total"),
         dig(lbin("office-slide", fixture("eqs.pptx")), "equations.equations_total"),
         dig(lbin("office-slide", fixture("eqs.pptx")), "equations.slides[0].shapes_total"),
         lbin("office-slide", fixture("deck.ppt")).get("equations")],
        [0, 5, 3, 2, 0, 0, 2, 2, 2, None],
    )
    chart = lbin("office-slide", fixture("deck-chart.odp"))
    check(
        "**图表走的是同一扇门**：`deck-chart.odp` 页上有 2 枚 `draw:frame`、2 枚 `draw:object`"
        "（`./Object 1` / `./Object 2`，两枚部件都在包里），可那两份 `content.xml` 的根不是 "
        "`<math>` —— 于是 `math_found` 0、`objects_without_math` 2、`equations_total` 0，"
        "`elements_seen` 与式子里的字都是空的。这一格存在的理由就是这份件：拿「有 `draw:object`」"
        "或「部件解析得开」当公式，就会把两张图报成两条式子（第二读者第一版正是这样，"
        "在这份件上才对出来）。两处替位图地址也都不在包里（`replacements_missing` 2）",
        [dig(chart, "equations.frames_seen"), dig(chart, "equations.objects_total"),
         dig(chart, "equations.parts_found"), dig(chart, "equations.math_found"),
         dig(chart, "equations.objects_without_math"),
         dig(chart, "equations.equations_total"), dig(chart, "equations.elements_seen"),
         dig(chart, "equations.replacements_written"),
         dig(chart, "equations.replacements_missing"),
         [dig(chart, "equations.items[%d].math_found" % i) for i in (0, 1)]],
        [2, 2, 2, 0, 2, 0, [], 2, 2, [False, False]],
    )
    # ── 3b0) 放映的大纲搬进 markdown：页、标题、条目标记与表 ────────────────────────
    print("=== 3b0) office-text --markdown 的 pptx 支：标不标条目是文件自己说的 ===")
    for name in sorted(one.name for one in FIXTURES.glob("*.pptx")):
        got = lbin("office-text", fixture(name), "--markdown")
        check("%s 的大纲整份与读者一致（渲染逐字、计数逐格）" % name,
              got.get("markdown"), files[name]["ooxml"]["markdown"])
    hand = lbin("office-text", fixture("deck.pptx"), "--markdown")
    back = lbin("office-text", fixture("deck-lo.pptx"), "--markdown")
    check(
        "同一份稿子在两家手里：python-pptx 一个 `a:pPr` 都不写（那两段是普通段），"
        "LibreOffice 给它们写了 `a:buChar`（于是两行前面有 `- `）—— "
        "两处渲染相差 3 个码位：两个 `- ` 的标记，加上列表项之间不再空一行；"
        "块数与页数一根不多一根不少，沉默的那一段不替它补标记，"
        "`bullets_silent` 与 `bullets_written` 两本账各数各的",
        [dig(hand, "markdown.chars"), dig(hand, "markdown.blocks"),
         dig(hand, "markdown.pages"), dig(hand, "markdown.titles"),
         dig(hand, "markdown.bullets_written"), dig(hand, "markdown.bullets_silent"),
         dig(hand, "markdown.text"),
         dig(back, "markdown.chars"), dig(back, "markdown.blocks"),
         dig(back, "markdown.bullets_written"), dig(back, "markdown.bullets_denied"),
         dig(back, "markdown.text")],
        [84, 5, 2, 2, 0, 2, '# 预算评审\n\n新增两台 64 核应用服务器\n\n第二条要点\n\n# 第二页：数字\n\n| 科目 | 金额 |\n| --- | --- |\n| 服务器 | 124000 |\n', 87, 5, 2, 0, '# 预算评审\n\n- 新增两台 64 核应用服务器\n- 第二条要点\n\n# 第二页：数字\n\n| 科目 | 金额 |\n| --- | --- |\n| 服务器 | 124000 |\n'],
    )
    tab = lbin("office-text", fixture("deck-tables.pptx"), "--markdown")
    tab_lo = lbin("office-text", fixture("deck-tables-lo.pptx"), "--markdown")
    check(
        "一张 `a:tbl` 在两家手里铺成同一份 markdown：页身份、行数与那一格两段的 `<br>` 都一样"
        "（与 3az 那本 CSV 同一批件，两种出口各按各的规矩）",
        [dig(tab, "markdown.tables"), dig(tab, "markdown.table_rows"),
         dig(tab, "markdown.chars"), dig(tab, "markdown.text"),
         tab_lo.get("markdown") == tab.get("markdown")],
        [1, 3, 95, '# 表格那一页\n\n| 科目<br>金额 |  | 备注 |\n| --- | --- | --- |\n| 服务器 | 124000 | 含税 |\n| 网络<br>设备 | 8000 |  |\n', True],
    )
    check(
        "没有标题形状的那份件不替它编标题：`deck-tr.pptx` 三页全是 `titles_missing`，"
        "整篇没有一行 `# `；备注不进 markdown（那不是给观众看的字），只数几页有备注部件；"
        "没搬的是 .ods —— 它按格子交字，没有页级大纲那棵树，所以那个键整个不在，"
        "`--markdown` 不开时也一样（odp 这一族已经搬进来了，上面逐份对账）",
        [dig(lbin("office-text", fixture("deck-tr.pptx"), "--markdown"), "markdown.titles"),
         dig(lbin("office-text", fixture("deck-tr.pptx"), "--markdown"), "markdown.titles_missing"),
         dig(back, "markdown.notes_pages"), dig(back, "markdown.links"),
         dig(back, "markdown.pictures"),
         lbin("office-text", fixture("book.ods"), "--markdown").get("markdown"),
         lbin("office-text", fixture("deck-lo.pptx")).get("markdown")],
        [0, 3, 1, 0, 1, None, None],
    )
    link = lbin("office-text", fixture("deck-links.pptx"), "--markdown")
    link_lo = lbin("office-text", fixture("deck-links-lo.pptx"), "--markdown")
    check(
        "页上那三条链在 markdown 里必须是 `[字](地址)`：`a:rPr/a:hlinkClick/@r:id` 只写一个号，"
        "地址在**这一页自己的**关系表里（`Rel.source` 存的是源部件 `ppt/slides/slide1.xml`，"
        "不是成员名 `ppt/slides/_rels/slide1.xml.rels` —— 拿后者去比，三条链全成「指不到」，"
        "而 `links` 那一格照样是 3，只有整串文本对得上才看得见这种错）；"
        "两家写的号不一样（一家 rId2 起、重写那份 rId1 起）而地址一字未变",
        [dig(link, "markdown.links"), dig(link_lo, "markdown.links"),
         "[第三季度的说明](https://example.com/budget)" in str(dig(link, "markdown.text")),
         "[发邮件问预算](mailto:liuqi@example.com)" in str(dig(link, "markdown.text")),
         "[https://example.com/raw](https://example.com/raw)" in str(dig(link, "markdown.text")),
         dig(link, "markdown.chars"), dig(link_lo, "markdown.chars"),
         dig(link, "markdown.text"), dig(link_lo, "markdown.text")],
        [3, 3, True, True, True, 169, 169, '# 链接那一页\n\n[第三季度的说明](https://example.com/budget)\n\n（口径见附页，这一段没链）\n\n[发邮件问预算](mailto:liuqi@example.com)\n\n[https://example.com/raw](https://example.com/raw)\n\n# 第二页\n\n这一页一条链接也没有\n', '# 链接那一页\n\n[第三季度的说明](https://example.com/budget)\n\n（口径见附页，这一段没链）\n\n[发邮件问预算](mailto:liuqi@example.com)\n\n[https://example.com/raw](https://example.com/raw)\n\n# 第二页\n\n这一页一条链接也没有\n'],
    )
    # ── 3b1) 同一本大纲的 odp 那一面：条目是元素，标题在框的 class 上 ──────────────
    print("=== 3b1) office-text --markdown 的 odp 支：两族把「条目」说在两处，渲染却可以一字不差 ===")
    for name in sorted(one.name for one in FIXTURES.glob("*.odp")):
        got = lbin("office-text", fixture(name), "--markdown")
        check("%s 的大纲整份与读者一致（渲染逐字、计数逐格）" % name,
              got.get("markdown"), files[name]["odp"]["markdown"])
    odp_deck = lbin("office-text", fixture("deck.odp"), "--markdown")
    odp_tab = lbin("office-text", fixture("deck-tables.odp"), "--markdown")
    odp_tr = lbin("office-text", fixture("deck-tr.odp"), "--markdown")
    odp_eqs = lbin("office-text", fixture("eqs.odp"), "--markdown")
    odp_eqs_lo = lbin("office-text", fixture("eqs-lo.odp"), "--markdown")
    check(
        "同一份稿子从 LibreOffice 出去的两族，渲染可以一字不差：`deck-lo.pptx` 与 `deck.odp`"
        "同为 87 码位 5 块、整串相等 —— pptx 把条目写在 `a:pPr/a:buChar` 上，odp 直接把段装进"
        "`text:list-item` 元素，两处各说各的，读者不替对方翻译；`deck-tables` 那张表在两家手里也"
        "是同一份 markdown（95 码位，1 张表 2 个被盖住的占位格）。可其余账目一家一份，"
        "谁也不替谁补齐：odp 每页都写备注块（notes_pages 2 对 pptx 的 1）、页上的图也多数一枚"
        "（2 对 1），而这一族没有 `a:pPr/@lvl` 那样的层级属性（levels_written 0）",
        [dig(odp_deck, "markdown.chars"), dig(odp_deck, "markdown.blocks"),
         dig(odp_deck, "markdown.text") == dig(back, "markdown.text"),
         dig(odp_deck, "markdown.notes_pages"), dig(back, "markdown.notes_pages"),
         dig(odp_deck, "markdown.pictures"), dig(back, "markdown.pictures"),
         dig(odp_deck, "markdown.levels_written"),
         dig(odp_tab, "markdown.text") == dig(tab, "markdown.text"),
         dig(odp_tab, "markdown.chars"), dig(odp_tab, "markdown.tables"),
         dig(odp_tab, "markdown.covered_cells")],
        [87, 5, True, 2, 1, 2, 1, 0, True, 95, 1, 2],
    )
    check(
        "备注块不是第二张 `draw:page`：LibreOffice 把 `presentation:notes` 写成 "
        "`draw:page-thumbnail` 加两个 `draw:frame`（那块里一枚 `draw:image` 也没有），按局部名数 "
        "`page` 只数到真页 —— `deck.odp` 2 页 2 个备注块，而 `eqs.odp` 2 页一个备注块都没有"
        "（0：那一份的外壳是手写的，LibreOffice 没有 odt → odp 的导出过滤器），同一份字被它重写成 "
        "`eqs-lo.odp` 后备注块变成 2、页上的图也多一枚（2 对 1）—— 两家对「一页该不该有备注块」答案"
        "不同，两本账分开。`deck-tr.odp` 3 页没有一个 `presentation:class=title`，整篇一行 "
        "`# ` 也不写、3 页全记 `titles_missing`；`text:h` 在这一批 odp 里一个都没有，所以"
        "`headings` 与 `levels_written` 全 0（0 = 数过了没有，不是没看）",
        [dig(odp_deck, "markdown.pages"), dig(odp_deck, "markdown.notes_pages"),
         dig(odp_eqs, "markdown.pages"), dig(odp_eqs, "markdown.notes_pages"),
         dig(odp_eqs_lo, "markdown.notes_pages"), dig(odp_eqs_lo, "markdown.pictures"),
         dig(odp_eqs, "markdown.pictures"),
         dig(odp_tr, "markdown.titles"), dig(odp_tr, "markdown.titles_missing"),
         "# " in str(dig(odp_tr, "markdown.text")),
         dig(odp_tr, "markdown.headings"), dig(odp_tr, "markdown.levels_written"),
         dig(odp_deck, "markdown.headings")],
        [2, 2, 2, 0, 2, 2, 1, 0, 3, False, 0, 0, 0],
    )
    # ── 3az) 放映里那张表铺成 CSV：挑页有三层，两家都留着被盖住那一格 ──────────────
    print("=== 3az) office-slide --csv：一页一张表一份账，页身份与页号一起交 ===")
    for name in sorted(one.name for one in FIXTURES.glob("*.pptx")):
        deck = lbin("office-slide", fixture(name))
        want = files[name]["ooxml"]["slides"]
        check("%s 不给 --csv 时这一格在场而值是 null —— 这一族读了这个开关、只是没被要求；"
              "「这一族没读」是 .ppt 那种缺席（下面最后一条钉住）" % name,
              [("csv" in deck), deck.get("csv")], [True, None])
        check("%s 两读者的页序（部件名）要一致，否则下面按号比的就是两页" % name,
              [one.get("part") for one in deck.get("slides") or []],
              [one.get("part") for one in want])
        for at, entry in enumerate(deck.get("slides") or []):
            part = entry.get("part")
            ledgers = (want[at] or {}).get("csv") or []
            for give in (str(at), part):
                got = lbin("office-slide", fixture(name), "--csv", "--page", give)
                check("%s 第 %d 页按「%s」挑，页身份要指到同一页" % (name, at, give),
                      [got.get("csv", {}).get("page"), got.get("csv", {}).get("page_index")],
                      [part, at])
            for ti, one_csv in enumerate(ledgers):
                got = dict(lbin("office-slide", fixture(name), "--csv", "--page", part,
                                "--table", str(ti)).get("csv") or {})
                got.pop("page", None)
                got.pop("page_index", None)
                check("%s 第 %d 页第 %d 张表铺成 CSV 与读者整份一致" % (name, at, ti),
                      got, one_csv)
            n = len(ledgers)
            got = lbin("office-slide", fixture(name), "--csv", "--page", part,
                       "--table", str(n)).get("csv") or {}
            check("%s 那一页只有 %d 张表：再说第 %d 张要说清是哪一页，不能拿「这份文件」顶"
                  % (name, n, n),
                  [got.get("error"), got.get("page")],
                  ["这一页（%s）里没有第 %d 张表（一共 %d 张）" % (part, n, n), part])
    for name in sorted(one.name for one in FIXTURES.glob("*.odp")):
        deck = lbin("office-slide", fixture(name))
        want = files[name]["odp"]["slides"]
        check("%s 不给 --csv 时这一格在场而值是 null —— 这一族读了这个开关、只是没被要求；"
              "「这一族没读」是 .ppt 那种缺席（下面最后一条钉住）" % name,
              [("csv" in deck), deck.get("csv")], [True, None])
        check("%s 两读者的页序（页名）要一致" % name,
              [one.get("name") for one in deck.get("slides") or []],
              [one.get("name") for one in want])
        for at, entry in enumerate(deck.get("slides") or []):
            pname = entry.get("name") or ""
            ledgers = (want[at] or {}).get("csv") or []
            for give in (str(at), pname):
                got = lbin("office-slide", fixture(name), "--csv", "--page", give)
                check("%s 第 %d 页按「%s」挑，页身份要指到同一页" % (name, at, give),
                      [got.get("csv", {}).get("page"), got.get("csv", {}).get("page_index")],
                      [pname, at])
            for ti, one_csv in enumerate(ledgers):
                got = dict(lbin("office-slide", fixture(name), "--csv", "--page", pname,
                                "--table", str(ti)).get("csv") or {})
                got.pop("page", None)
                got.pop("page_index", None)
                check("%s 第 %d 页第 %d 张表铺成 CSV 与读者整份一致" % (name, at, ti),
                      got, one_csv)
    dt_p = lbin("office-slide", fixture("deck-tables.pptx"), "--csv")
    dt_lo = lbin("office-slide", fixture("deck-tables-lo.pptx"), "--csv")
    dt_o = lbin("office-slide", fixture("deck-tables.odp"), "--csv")
    check(
        "同一张表的三种合并写法在这里露出两面：pptx 在被盖住那一格身上写 `hMerge` / `vMerge`、"
        "ODF 另写一枚 `covered-table-cell` —— 两家的字段数都是齐的 [3, 3, 3]、`covered_cells` 与 "
        "`empty_cells` 各 2，而**铺出来的串一字不差**（文档那一族的 OOXML 才是整个不写那一格： "
        "`[2, 3]` 且 `ragged`，见 3ay）。一格里两个段的那两格进 CSV 都按 RFC4180 加了引号；"
        "两家 pptx 那一支连 `page` 都同名，odp 那一支页身份是页名",
        [dig(dt_p, "csv.columns_per_row"), dig(dt_p, "csv.ragged"),
         dig(dt_p, "csv.empty_cells"), dig(dt_p, "csv.covered_cells"),
         dig(dt_p, "csv.rows"), dig(dt_p, "csv.columns"), dig(dt_p, "csv.text"),
         dig(dt_p, "csv.page"), dig(dt_p, "csv.page_index"),
         dig(dt_lo, "csv"), dig(dt_o, "csv.columns_per_row"), dig(dt_o, "csv.text"),
         dig(dt_o, "csv.page"), dig(dt_o, "csv.covered_cells"),
         dig(dt_o, "csv.tables_total")],
        [[3, 3, 3], False, 2, 2, 3, 3, '"科目\n金额",,备注\n服务器,124000,含税\n"网络\n设备",8000,\n', 'ppt/slides/slide1.xml', 0, {'table': 0, 'tables_total': 1, 'rows': 3, 'columns': 3, 'columns_per_row': [3, 3, 3], 'ragged': False, 'empty_cells': 2, 'covered_cells': 2, 'cut': False, 'line_end': 'LF', 'text': '"科目\n金额",,备注\n服务器,124000,含税\n"网络\n设备",8000,\n', 'page': 'ppt/slides/slide1.xml', 'page_index': 0}, [3, 3, 3], '"科目\n金额",,备注\n服务器,124000,含税\n"网络\n设备",8000,\n', '表格那一页', 2, 1],
    )
    deck_p = lbin("office-slide", fixture("deck.pptx"), "--csv", "--page", "1")
    deck_first = lbin("office-slide", fixture("deck.pptx"), "--csv")
    deck_o = lbin("office-slide", fixture("deck.odp"), "--csv", "--page", '第二页：数字')
    check(
        "表不在第一页的那份件：不给号挑到的是标题页，它交的是「这一页没有表」那句话而不是空串；"
        "按号挑到第 1 页才拿得到那张 2×2。odp 那一支同一张表的身份是页名（%s），字段数一样"
        % '第二页：数字',
        [dig(deck_p, "csv.page"), dig(deck_p, "csv.columns_per_row"), dig(deck_p, "csv.text"),
         dig(deck_p, "csv.page_index"), dig(deck_first, "csv.error"),
         dig(deck_first, "csv.page"), dig(deck_o, "csv.page"),
         dig(deck_o, "csv.columns_per_row"), dig(deck_o, "csv.text")],
        ['ppt/slides/slide2.xml', [2, 2], '科目,金额\n服务器,124000\n', 1, '这一页（ppt/slides/slide1.xml）里没有第 0 张表（一共 0 张）', 'ppt/slides/slide1.xml', '第二页：数字', [2, 2], '科目,金额\n服务器,124000\n'],
    )
    check(
        "页与表两层各说各的失败：`--page 9` 说这份放映一共几页，`--page 没这页` 说这既不是序号"
        "也不是这份件里点得到的名字；遗留 .ppt **不交这个键**（那一族到不了页部件这一层，"
        "与它不交 equations 同一个边界，见事实 114）",
        [dig(lbin("office-slide", fixture("deck.pptx"), "--csv", "--page", "9"), "csv.error"),
         dig(lbin("office-slide", fixture("deck.pptx"), "--csv", "--page", "没这页"),
             "csv.error"),
         ["csv" in lbin("office-slide", fixture("deck.ppt"), "--csv"),
          lbin("office-slide", fixture("deck.ppt"), "--csv").get("csv")]],
        ['这份放映里没有第 9 页（一共 2 页）', '--page 要的是从 0 起的序号、部件名或页名，收到「没这页」（一共 2 页）', [False, None]],
    )
    # ── 3ay) 文档里的表铺成 CSV：两家把「合并」写得不一样，行数一样而字段数不一样 ──────
    print("=== 3ay) office-doc --csv：一行就是文件自己写着的几格，不补方格 ===")
    for name in sorted(one.name for one in FIXTURES.glob("*.docx")):
        got = lbin("office-doc", fixture(name), "--csv")
        check("%s 的第一张表铺成 CSV 与读者一致" % name,
              got.get("csv"), files[name]["ooxml"]["csv"])
    for name in sorted(one.name for one in FIXTURES.glob("*.odt")):
        got = lbin("office-doc", fixture(name), "--csv")
        check("%s 的第一张表铺成 CSV 与读者一致" % name,
              got.get("csv"), files[name]["odt"]["csv"])
    for name in sorted(one.name for one in FIXTURES.glob("*.docx")):
        total = (files[name]["ooxml"].get("csv") or {}).get("tables_total", 0)
        for at in range(total):
            got = lbin("office-doc", fixture(name), "--csv", "--table", str(at))
            check("%s 第 %d 张表按号取（这一份一共 %s 张）" % (name, at, total),
                  [got.get("csv", {}).get("table"), got.get("csv", {}).get("tables_total")],
                  [at, total])
    plain = lbin("office-doc", fixture("tables.docx"), "--csv")
    merged = lbin("office-doc", fixture("tables-merged.docx"), "--csv")
    odt_merged = lbin("office-doc", fixture("tables-merged.odt"), "--csv")
    rich = lbin("office-doc", fixture("md.docx"), "--csv")
    check(
        "同一张表，两家铺出来不一样长：`tables-merged.docx` 把横向合掉那一格**整个不写** → "
        "`columns_per_row` 与 `ragged` 跟着变；`tables-merged.odt` 照样写一枚空的 "
        "`covered-table-cell` → 每行字段数齐了，而 `covered_cells`（文件写了占位格）与 "
        "`empty_cells`（这格没有字）各说各的。账本不替任何一边补方格",
        [dig(merged, "csv.columns_per_row"), dig(merged, "csv.ragged"),
         dig(merged, "csv.covered_cells"), dig(merged, "csv.empty_cells"),
         dig(merged, "csv.rows"), dig(merged, "csv.columns"), dig(merged, "csv.text"),
         dig(odt_merged, "csv.columns_per_row"), dig(odt_merged, "csv.ragged"),
         dig(odt_merged, "csv.covered_cells"), dig(odt_merged, "csv.empty_cells"),
         dig(odt_merged, "csv.rows"), dig(odt_merged, "csv.columns"),
         dig(odt_merged, "csv.text")],
        [[2, 3], True, 0, 0, 2, 3, "跨两列,第三列\na,b,c\n",
         [3, 3], False, 1, 1, 2, 3, "跨两列,,第三列\na,b,c\n"],
    )
    check(
        "一格里有几个段就用换行连着，进了 CSV 按 RFC4180 整格加引号、里面的引号翻倍："
        "`md.docx` 那三行的文本原样在下面（第三行第一格是两个段拼的，第二格带竖线 —— 竖线不是引用触发符）；"
        "一格的字先按 `text_of` 拼那几个段、再去首尾空白，与 `tables[].grid` 同一个口径，"
        "`tables.docx` 三行两格没有要引号的，就是裸字段",
        [dig(rich, "csv.rows"), dig(rich, "csv.columns"), dig(rich, "csv.empty_cells"),
         dig(rich, "csv.covered_cells"), dig(rich, "csv.text"),
         dig(plain, "csv.rows"), dig(plain, "csv.columns"), dig(plain, "csv.text")],
        [3, 2, 0, 0, "科目,金额\n服务器 * 两台,124000\n\"这一格有\n两段字\",尾格 | 带竖线\n", 3, 2, "R0C0,R0C1\nR1C0,R1C1\nR2C0,R2C1\n"],
    )
    check(
        "号只认从 0 起的序号：`--table 9` 说没有第 9 张表（并报一共几张），"
        "`--table 没这个号` 说这不是个序号；不给号就是第一张，`line_end` 说行尾。"
        "RTF 与遗留 .doc **不交这个键**：RTF 数得清 `\\row` 与 `\\cell` 却归不到某一张表"
        "（量过，见事实 100），.doc 只有 piece 表里的格子标记 —— 两处都做不出这张 CSV",
        [dig(lbin("office-doc", fixture("tables.docx"), "--csv", "--table", "9"), "csv.error"),
         dig(lbin("office-doc", fixture("tables.docx"), "--csv", "--table", "没这个号"), "csv.error"),
         dig(plain, "csv.table"), dig(plain, "csv.tables_total"),
         dig(plain, "csv.cut"), dig(plain, "csv.line_end"),
         lbin("office-doc", fixture("tabs.rtf"), "--csv").get("csv"),
         lbin("office-doc", fixture("notes.doc"), "--csv").get("csv")],
        ["这份文件里没有第 9 张表（一共 2 张）", "--table 要的是从 0 起的序号，收到「没这个号」", 0, 2, False, "LF", None, None],
    )
    # ── 3ax) 遗留 .doc 的公式对象：一条式子是一枚内嵌 OLE 对象，字在 MTEF 里不读 ──────
    print("=== 3ax) .doc 的公式对象：ObjectPool 一物一 storage，正文流叫 Equation Native ===")
    for name in sorted(one.name for one in FIXTURES.glob("*.doc")):
        got = lbin("office-doc", fixture(name))
        check("%s 的公式对象那份账与读者一致（走目录树，MTEF 的字不解）" % name,
              dig(got, "structure.equations"), files[name]["ole_equations"])
    eqdoc = lbin("office-doc", fixture("eq.doc"))
    check(
        "`eq.doc`（`eq.docx` 经 LibreOffice 的 MS Word 97 那一转）六条式子变成**六枚内嵌对象**："
        "`ObjectPool` 下一物一 storage，名字从 `_2147483647` 起往下排（账里按目录树的中序走，"
        "所以是 `_2147483642` 在前）；每枚带 `\x01Ole` + `\x01CompObj` + 一条正文流，"
        "正文流**自己叫什么就交什么**（`Equation Native`），六条 MTEF 一共 362 字节。"
        "`\x01CompObj` 后半那三枚串读出来是 `Microsoft Equation 3.0` / `DS Equation` / "
        "`Equation.3`。**两个地方各数一次、数到同一个数**：piece 表里正文的嵌入对象锚 "
        "（U+0001）是 6 枚，容器目录里 `ObjectPool` 的孩子也是 6 枚 —— 这一本交两格，"
        "不拿一格去圆另一格",
        [dig(eqdoc, "structure.equations.objects_total"), dig(eqdoc, "structure.equations.equations_total"),
         dig(eqdoc, "structure.equations.pool_found"), dig(eqdoc, "structure.equations.payload_stream_seen"),
         dig(eqdoc, "structure.equations.native_bytes_total"),
         [dig(eqdoc, "structure.equations.items[%d].payload_size" % i) for i in range(6)],
         [dig(eqdoc, "structure.equations.items[%d].pool_name" % i) for i in range(6)],
         dig(eqdoc, "structure.equations.items[0].label"),
         dig(eqdoc, "structure.equations.items[0].user_type"),
         dig(eqdoc, "structure.equations.items[0].prog_id"),
         dig(eqdoc, "structure.equations.items[0].streams"),
         dig(eqdoc, "structure.object_marks")],
        [6, 6, True, ["Equation Native"], 362,
         [59, 71, 59, 56, 58, 59],
         ["_2147483642", "_2147483643", "_2147483644", "_2147483645", "_2147483646",
          "_2147483647"],
         "Microsoft Equation 3.0", "DS Equation", "Equation.3",
         ["\x01Ole", "\x01CompObj", "Equation Native"], 6],
    )
    check(
        "同一份的 docx 那一边是 6 条 OMML（`equations.equations_total` 6，见 3au），"
        "转成 97 的 .doc 之后还是 6 枚 —— 只是**字从可读的 `m:t` 变成不可读的 MTEF**，"
        "所以这一本不交 `text`：整个键不在，而不是空串。"
        "两份没有 `ObjectPool` 的 .doc 各交 0 与 `pool_found` false（数过了没有）",
        [dig(lbin("office-doc", fixture("eq.docx")), "structure.equations.equations_total"),
         "text" in (dig(eqdoc, "structure.equations.items[0]") or {}),
         dig(lbin("office-doc", fixture("notes.doc")), "structure.equations.pool_found"),
         dig(lbin("office-doc", fixture("notes.doc")), "structure.equations.objects_total"),
         dig(lbin("office-doc", fixture("notes-en.doc")), "structure.equations.pool_found"),
         dig(lbin("office-doc", fixture("notes-en.doc")), "structure.equations.objects_total")],
        [6, False, False, 0, False, 0],
    )
    # ── 3aw) pptx 的公式：一条式子挂在文本体里，同一个形状在 Fallback 里还写了一遍 ────
    print("=== 3aw) pptx 的公式：a14:m 里的 OMML 与 Fallback 里那张替身图 ===")
    for name in sorted(one.name for one in FIXTURES.glob("*.pptx")):
        got = lbin("office-slide", fixture(name))
        check("%s 的公式那份账与读者一致（挂法、替身图、页级三格各数各的）" % name,
              got.get("equations"), files[name]["ooxml"]["equations"])
    lo = lbin("office-slide", fixture("eqs.pptx"))
    pp = lbin("office-slide", fixture("eqs-pp.pptx"))
    check(
        "`eqs.pptx`（`eqs.odp` → pptx 那一转）两条式子，每条是 `a:p → a14:m → m:oMath`："
        "字在 `m:t` 里（`ab` / `12`），结构名按文档顺序（`f`/`num`/`den`、`rad`/`radPr`/"
        "`degHide`/`deg`/`e`），一条式子两个数学 run。要紧的是**同一个形状写了两遍** —— "
        "式子在 `mc:Choice Requires=\"a14\"` 里那一遍，`mc:Fallback` 里那一遍没有 txBody、"
        "改挂一张 EMF：`fallback_blip` 交文件写着的号、`fallback_target` 解成包里的部件、"
        "`fallback_found` 回答它在不在包里，而 `fallback_shape_id` 与 `shape_id` 一字不差"
        "（9 与 10 各一枚 → `duplicated_shapes` 2）。所以页级三格必须分开：slide1 有 "
        "**2 枚 `p:sp` 却只有 1 条式子、1 个段**",
        [dig(lo, "equations.equations_total"), dig(lo, "equations.slides_with"),
         dig(lo, "equations.alternates_total"), dig(lo, "equations.fallbacks_written"),
         dig(lo, "equations.duplicated_shapes"), dig(lo, "equations.rasters_written"),
         dig(lo, "equations.rasters_found"), dig(lo, "equations.rasters_missing"),
         dig(lo, "equations.paragraphs_total"), dig(lo, "equations.text_chars"),
         dig(lo, "equations.math_runs"), dig(lo, "equations.slides"),
         dig(lo, "equations.items[0].holder"),
         dig(lo, "equations.items[0].choice_requires"),
         dig(lo, "equations.items[0].fallback_blip"),
         dig(lo, "equations.items[0].fallback_target"),
         dig(lo, "equations.items[0].fallback_shape_id"),
         dig(lo, "equations.items[0].shape_id"),
         dig(lo, "equations.items[0].shape_name"),
         dig(lo, "equations.items[1].structures"), dig(lo, "equations.structures_seen")],
        [2, 2, 4, 2, 2, 2, 2, 0, 3, 4, 4,
         [{"part": "ppt/slides/slide1.xml", "show_index": "256", "paragraphs_total": 1,
           "shapes_total": 2, "formulas": 1, "alternates": 2},
          {"part": "ppt/slides/slide2.xml", "show_index": "257", "paragraphs_total": 2,
           "shapes_total": 3, "formulas": 1, "alternates": 2}],
         "a14:m", "a14", "rId1", "ppt/media/image1.emf", "9", "9", "对象1",
         ["rad", "radPr", "degHide", "deg", "e"],
         ["f", "num", "den", "rad", "radPr", "degHide", "deg", "e"]],
    )
    check(
        "同一句问题的第二种写法：`eqs-pp.pptx` 由 python-pptx 手挂，`a14:m` **裸挂在 `a:p` 里**"
        "（与正文那个 `a:r` 并列），没有 `mc:AlternateContent`、没有替身图 —— "
        "`alternates_total` 0、`fallbacks_written` 0、`rasters_written` 0，三条 `fallback_*` "
        "全是 **null（这一族没写这一格）** 而不是 false；两条式子的字与结构名与 LO 那份一模一样，"
        "而每页只有 1 枚 `p:sp`（LO 那份是 2 枚）—— 形状数与式子数的两倍关系是**挂法**带来的，"
        "不是内容",
        [dig(pp, "equations.equations_total"), dig(pp, "equations.alternates_total"),
         dig(pp, "equations.fallbacks_written"), dig(pp, "equations.duplicated_shapes"),
         dig(pp, "equations.rasters_written"),
         [dig(pp, "equations.items[%d].%s" % (i, k)) for i in (0, 1)
          for k in ("holder", "in_alternate", "choice_requires", "fallback_written",
                    "fallback_blip", "fallback_found")],
         [dig(pp, "equations.items[%d].text" % i) for i in (0, 1)],
         [dig(pp, "equations.slides[%d].shapes_total" % i) for i in (0, 1)],
         [dig(lo, "equations.slides[%d].shapes_total" % i) for i in (0, 1)]],
        [2, 0, 0, 0, 0,
         ["a14:m", False, None, None, None, None,
          "a14:m", False, None, None, None, None],
         ["ab", "12"], [1, 1], [2, 3]],
    )
    check(
        "跨生产者同形：两家的 `structures_seen`、`text_chars`、`math_runs` 与两条式子的字"
        "全部一致，所以「哪一格不一样」才看得见 —— 差的是**外壳**与**引用**，不是内容。"
        "另外记一条生产者边界：这份裸挂的 `eqs-pp.pptx` 转 odp 时 LibreOffice **把整条式子丢了**"
        "（转出的件里 `draw:object` 0 枚、没有任何公式部件），所以这一族不能拿重写当凭据",
        [dig(pp, "equations.structures_seen") == dig(lo, "equations.structures_seen"),
         dig(pp, "equations.text_chars"), dig(pp, "equations.math_runs"),
         [dig(pp, "equations.items[%d].text" % i) for i in (0, 1)]],
        [True, 4, 4, ["ab", "12"]],
    )
    # ── 3aq) 公式那枚 <f> 自己写了什么：共享组的跟随格在文件里没有公式正文 ──────────
    print("=== 3aq) 公式元素自己：共享组、空正文带缓存值、两个生产者三种写法 ===")
    for name in sorted(one.name for one in FIXTURES.glob("*.xlsx")):
        got = lbin("office-sheet", fixture(name))
        check("%s 公式元素那份账与读者一致（属性分布、共享组、空正文带缓存）" % name,
              got.get("formula_elems"), files[name]["ooxml"]["formula_elems"])
    for name in sorted(one.name for one in FIXTURES.glob("*.ods")):
        got = lbin("office-sheet", fixture(name))
        check("%s 公式那份账与读者一致（格子身上的属性，每条都带正文）" % name,
              dig(got, "formula_elems"),
              files[name]["ods"]["formula_elems"])
    sh = lbin("office-sheet", fixture("shared.xlsx"))
    check(
        "`shared.xlsx` 一列八格一个共享组：16 枚 `<f>` 里 **8 枚带属性**（`t` / `ref` / `si` 三种，"
        "按文件写的顺序列在 `attrs_seen`）、主格那一条写着 `ref=\"B1:B8\"`，"
        "**7 枚正文是空的**（`empty_text` 7）而这 7 枚**全都有缓存值** —— "
        "「几格有公式」16、「几格写了正文」9 是两个数，合成一个就把「文件没写公式」读没了",
        [dig(sh, "formula_elems.formula_elems"),
         dig(sh, "formula_elems.sheets_seen"),
         dig(sh, "formula_elems.with_attrs"),
         dig(sh, "formula_elems.attrs_seen"),
         dig(sh, "formula_elems.text_written"),
         dig(sh, "formula_elems.empty_text"),
         dig(sh, "formula_elems.empty_text_with_cached"),
         dig(sh, "formula_elems.shared_elems"),
         dig(sh, "formula_elems.shared_masters"),
         dig(sh, "formula_elems.shared_followers"),
         dig(sh, "formula_elems.ref_written_elems"),
         dig(sh, "formula_elems.cached_elems"),
         dig(sh, "formula_elems.attr_values")],
        [16, 1, 8, ["t", "ref", "si"], 9, 7, 7, 8, 1, 7, 1, 16,
         {"t": {"shared": 8}, "si": {"0": 8}, "ref": {"B1:B8": 1}}],
    )
    check(
        "清单按**文档序**（一行里先 B 后 C），所以「第几条」不是「第几行」：第 0 条是主格 B1"
        "（正文 `A1*2`、`ref_written` 是 `B1:B8`），第 1 条是同一行的 C1（普通公式，不带属性），"
        "第 2 条才是跟随格 B2 —— 正文空串、`shared` true、`si` 还是 `0` 而 `ref_written` 是 null。"
        "它带一枚 `<v>` 标签，可那标签里**没有字**（openpyxl 没算过），"
        "所以「有 `<v>`」与「有缓存值」还要分两格看（`cached_written` true 而 `cached` 是空串）",
        [dig(sh, "formula_elems.cells[0].cell"),
         dig(sh, "formula_elems.cells[0].text"),
         dig(sh, "formula_elems.cells[0].ref_written"),
         dig(sh, "formula_elems.cells[1].cell"),
         dig(sh, "formula_elems.cells[1].shared"),
         dig(sh, "formula_elems.cells[2].cell"),
         dig(sh, "formula_elems.cells[2].text"),
         dig(sh, "formula_elems.cells[2].text_written"),
         dig(sh, "formula_elems.cells[2].si"),
         dig(sh, "formula_elems.cells[2].ref_written"),
         dig(sh, "formula_elems.cells[2].cached")],
        ["B1", "A1*2", "B1:B8", "C1", False, "B2", "", False, "0", None, ""],
    )
    lo = lbin("office-sheet", fixture("shared-lo.xlsx"))
    check(
        "LibreOffice 重写同一份：**不用共享组** —— 16 枚 `<f>` 各写自己的正文"
        "（`shared_elems` 0、`empty_text` 0），而它给每一枚都写了 `aca=\"false\"`；"
        "openpyxl 那一份（`book.xlsx`）一枚属性都不写 —— 三个生产者三种写法，"
        "`attrs_seen` 与 `attr_values` 各按各的文件交",
        [dig(lo, "formula_elems.formula_elems"),
         dig(lo, "formula_elems.with_attrs"),
         dig(lo, "formula_elems.attrs_seen"),
         dig(lo, "formula_elems.attr_values"),
         dig(lo, "formula_elems.text_written"),
         dig(lo, "formula_elems.shared_elems"),
         dig(lbin("office-sheet", fixture("book.xlsx")), "formula_elems.formula_elems"),
         dig(lbin("office-sheet", fixture("book.xlsx")), "formula_elems.with_attrs"),
         dig(lbin("office-sheet", fixture("book.xlsx")), "formula_elems.attrs_seen")],
        [16, 16, ["aca"], {"aca": {"false": 16}}, 16, 0, 1, 0, []],
    )
    ods = lbin("office-sheet", fixture("shared.ods"))
    check(
        "同一问在 ODF 是第三种写法：公式是**格子身上的一个属性**，16 条全带正文"
        "（`empty_text` 0），`si` / `ref` / `shared` 那几格这一族**整个不交**（缺键 = 没有那个位置，"
        "不是 0）；正文前缀 `of:` 按写的留着（`ooo:` 是另一族写的），交在 `formula_prefixes` 这一格。"
        "带公式的是格子自己，所以 `attrs` 是那一格写着的属性全表、`sheet` 是它所在那张 "
        "`table:table` 写的名字（不是样式名），而这一族**不写格子地址** —— `cell` 逐条 null",
        [dig(ods, "formula_elems.formula_elems"),
         dig(ods, "formula_elems.text_written"),
         dig(ods, "formula_elems.empty_text"),
         dig(ods, "formula_elems.tables_seen"),
         dig(ods, "formula_elems.attrs_seen"),
         dig(ods, "formula_elems.formula_prefixes"),
         dig(ods, "formula_elems.shared_elems"),
         dig(ods, "formula_elems.cells[0].sheet"),
         dig(ods, "formula_elems.cells[0].cell"),
         dig(ods, "formula_elems.cells[1].text"),
         dig(ods, "formula_elems.cells[1].paragraphs"),
         dig(ods, "formula_elems.cells[1].cached")],
        [16, 16, 0, 1, ["formula", "value-type", "value"], {"of": 16}, None,
         "表一", None, "of:=SUM([.$A$1:.A1])", 1, "3"],
    )
    # ── 3al) 这一节的页码：OOXML 一节一条三个属性，ODF 一页版式一条，跨族各丢一次 ────
    print("=== 3al) 页码：元素在场、三个属性各写各的，而「从 7 开始」两头都不是同一种丢法 ===")
    for name in sorted(one.name for one in FIXTURES.glob("*.docx")):
        got = lbin("office-doc", fixture(name))
        check("%s 页码那份账与读者一致（一节一条，写了哪几个属性）" % name,
              dig(got, "structure.page_numbering"),
              files[name]["ooxml"]["page_numbering"])
    for name in sorted(one.name for one in FIXTURES.glob("*.odt")):
        got = lbin("office-doc", fixture(name))
        check("%s 页码那份账与读者一致（一页版式一条，两份件都走）" % name,
              dig(got, "structure.page_numbering"),
              files[name]["odt"]["page_numbering"])
    rs = lbin("office-doc", fixture("restart.docx"))
    rs_lo = lbin("office-doc", fixture("restart-lo.docx"))
    check(
        "`w:pgNumType` 三个属性全写的那一份：`start=\"7\"` / `fmt=\"upperRoman\"` / `chpNum=\"none\"` "
        "一条不落 —— 而 `w:fmt` 与 `w:start` 是两件事：这一节说了用什么数、也说了从几起",
        [dig(rs, "structure.page_numbering.sections_total"),
         dig(rs, "structure.page_numbering.with_element"),
         dig(rs, "structure.page_numbering.start_written_total"),
         dig(rs, "structure.page_numbering.distinct_fmts"),
         dig(rs, "structure.page_numbering.sections[0].start_written"),
         dig(rs, "structure.page_numbering.sections[0].fmt_written"),
         dig(rs, "structure.page_numbering.sections[0].chpnum_written"),
         dig(rs, "structure.page_numbering.sections[0].written")],
        [1, 1, 1, ["upperRoman"], "7", "upperRoman", "none",
         {"start": "7", "fmt": "upperRoman", "chpNum": "none"}],
    )
    check(
        "LibreOffice 把同一份件重写一遍：`start` 与 `fmt` 都活着，**`chpNum` 整格没了** —— "
        "三个属性不是同一个待遇，所以「写了哪几个」按现在这份件交，不替上一版接回来",
        [dig(rs_lo, "structure.page_numbering.sections[0].start_written"),
         dig(rs_lo, "structure.page_numbering.sections[0].fmt_written"),
         dig(rs_lo, "structure.page_numbering.sections[0].chpnum_written"),
         dig(rs_lo, "structure.page_numbering.sections[0].written"),
         dig(rs_lo, "structure.page_numbering.start_written_total"),
         dig(rs_lo, "structure.page_numbering.with_element")],
        ["7", "upperRoman", None, {"start": "7", "fmt": "upperRoman"}, 1, 1],
    )
    check(
        "同一份 docx 转成 odt：这一问换了地方（页版式上）也换了词汇 —— `upperRoman` 这一族写成"
        "一个字母 `I`，而**「从 7 开始」在 ODF 侧一个字都没落**（`page_number_written` 是 null、"
        "`with_page_number` 是 0）。null 是「这一格没写」，不是「从 1 开始」",
        [dig(lbin("office-doc", fixture("restart.odt")), "structure.page_numbering.layouts_total"),
         dig(lbin("office-doc", fixture("restart.odt")), "structure.page_numbering.masters_total"),
         dig(lbin("office-doc", fixture("restart.odt")), "structure.page_numbering.with_num_format"),
         dig(lbin("office-doc", fixture("restart.odt")), "structure.page_numbering.with_page_number"),
         dig(lbin("office-doc", fixture("restart.odt")), "structure.page_numbering.distinct_formats"),
         dig(lbin("office-doc", fixture("restart.odt")), "structure.page_numbering.layouts[0].layout_name"),
         dig(lbin("office-doc", fixture("restart.odt")), "structure.page_numbering.layouts[0].part"),
         dig(lbin("office-doc", fixture("restart.odt")), "structure.page_numbering.layouts[0].written")],
        [1, 1, 1, 0, ["I"], "Mpm1", "styles.xml", {"num-format": "I"}],
    )
    check(
        "反方向也丢：`pnum.odt` 在段落属性上明写 `style:page-number=\"7\"` + `use-page-numbering=\"true\"`，"
        "LibreOffice 导成 docx 后那一节的 `w:pgNumType` **只带 `fmt=\"decimal\"`** —— `w:start` 没写；"
        "而源件那个「另起一页」也没换出第二节（`sections_total` 是 1）。跨族走一趟，这一问两头各丢一次",
        [dig(lbin("office-doc", fixture("pnum.docx")), "structure.page_numbering.sections_total"),
         dig(lbin("office-doc", fixture("pnum.docx")), "structure.page_numbering.with_element"),
         dig(lbin("office-doc", fixture("pnum.docx")), "structure.page_numbering.start_written_total"),
         dig(lbin("office-doc", fixture("pnum.docx")), "structure.page_numbering.distinct_fmts"),
         dig(lbin("office-doc", fixture("pnum.docx")), "structure.page_numbering.sections[0].start_written"),
         dig(lbin("office-doc", fixture("pnum.docx")), "structure.page_numbering.sections[0].written")],
        [1, 1, 0, ["decimal"], None, {"fmt": "decimal"}],
    )
    check(
        "而 `pnum.odt` 自己那份账说：页版式一条、母版页两条（`layouts_total` 1 / `masters_total` 2，"
        "两份母版页共用同一份版式），版式上 `num-format=\"1\"` 与 `page-number=\"1\"` 都写了 —— "
        "「几条版式」与「几份母版页」是两个数，不拿一个顶另一个；这一族的版式住在 styles.xml，"
        "所以 `part` 交的是那一份件名",
        [dig(lbin("office-doc", fixture("pnum.odt")), "structure.page_numbering.layouts_total"),
         dig(lbin("office-doc", fixture("pnum.odt")), "structure.page_numbering.masters_total"),
         dig(lbin("office-doc", fixture("pnum.odt")), "structure.page_numbering.with_num_format"),
         dig(lbin("office-doc", fixture("pnum.odt")), "structure.page_numbering.with_page_number"),
         dig(lbin("office-doc", fixture("pnum.odt")), "structure.page_numbering.layouts[0].layout_name"),
         dig(lbin("office-doc", fixture("pnum.odt")), "structure.page_numbering.layouts[0].part"),
         dig(lbin("office-doc", fixture("pnum.odt")), "structure.page_numbering.layouts[0].written")],
        [1, 2, 1, 1, "pl1", "styles.xml", {"num-format": "1", "page-number": "1"}],
    )
    check(
        "没有这一格的件：docx 那边 `w:pgNumType` 根本不在（`with_element` 0、`written` 空表、"
        "`element_present` false），ODF 那边连版式都没有的 `tbox.odt` 交 0 而不是缺键；"
        "RTF 与遗留 .doc **不交这个键**（缺键 = 这一支不再交一次，不是「数过了没有」）",
        [dig(lbin("office-doc", fixture("notes.docx")), "structure.page_numbering.sections_total"),
         dig(lbin("office-doc", fixture("notes.docx")), "structure.page_numbering.with_element"),
         dig(lbin("office-doc", fixture("notes.docx")), "structure.page_numbering.sections[0].element_present"),
         dig(lbin("office-doc", fixture("notes.docx")), "structure.page_numbering.sections[0].written"),
         dig(lbin("office-doc", fixture("tbox.odt")), "structure.page_numbering.layouts_total"),
         dig(lbin("office-doc", fixture("tbox.odt")), "structure.page_numbering.masters_total"),
         dig(lbin("office-doc", fixture("tabs.rtf")), "structure.page_numbering"),
         dig(lbin("office-doc", fixture("notes-en.doc")), "structure.page_numbering")],
        [1, 0, False, {}, 0, 0, None, None],
    )
    # ── 3i) 文档里那几张图：两处尺寸、两处替代文字、两处锁，摆法三家各处 ──
    print("=== 3i) 文档里的图：一处号一次跳，两处答案各按各的文件交 ===")
    for name in ("images.docx", "images-lo.docx", "images-float.docx",
                 "notes.docx", "protected.docx", "protected-lo.docx", "toc.docx"):
        got = lbin("office-doc", fixture(name))
        want = files[name]["ooxml"]["picture_rows"]
        with_alt = sum(1 for one in want if (one["alt"] or {}).get("descr"))
        check("%s 每张图整份账与读者一致（尺寸两处、替代文字两处、锁两处）" % name,
              dig(got, "structure.picture_list"), want)
        check("%s 几张图、几张有替代文字、几个 drawing" % name,
              [dig(got, "structure.pictures"), dig(got, "structure.pictures_with_alt_text"),
               dig(got, "structure.pictures_without_alt_text"), dig(got, "structure.drawings")],
              [len(want), with_alt, len(want) - with_alt, files[name]["ooxml"]["drawings"]])
    for name in ("images.odt", "images-float.odt", "notes.odt", "protected.odt", "toc.odt"):
        got = lbin("office-doc", fixture(name))
        want = files[name]["odf"]["picture_rows"]
        with_alt = sum(1 for one in want if one["alt"])
        check("%s 每张图整份账与读者一致（尺寸自带单位、替代文字是孩子元素）" % name,
              dig(got, "structure.picture_list"), want)
        check("%s 几张图、几张有替代文字" % name,
              [dig(got, "structure.pictures"), dig(got, "structure.pictures_with_alt_text"),
               dig(got, "structure.pictures_without_alt_text")],
              [len(want), with_alt, len(want) - with_alt])

    plain = lbin("office-doc", fixture("images.docx"))
    lo = lbin("office-doc", fixture("images-lo.docx"))
    floaty = lbin("office-doc", fixture("images-float.docx"))
    odt = lbin("office-doc", fixture("images.odt"))
    odtf = lbin("office-doc", fixture("images-float.odt"))
    check(
        "同一条 4cm 在两家手里是两个数：两处尺寸一起跟着重写换，ODF 那个串落到同一个 0.01mm",
        [dig(plain, "structure.picture_list[0].extent.cx"),
         dig(plain, "structure.picture_list[0].extent.mm_w"),
         dig(lo, "structure.picture_list[0].extent.mm_w"),
         dig(lo, "structure.picture_list[0].pic_extent.mm_w"),
         dig(odt, "structure.picture_list[0].mm_w"),
         dig(odt, "structure.picture_list[0].mm_h"),
         dig(lo, "structure.picture_list[0].extent.mm_h")],
        ["1440000", 4000, 4001, 4001, 4001, 2401, 2401],
    )
    check(
        "替代文字两处、锁两处：一家只写外头那处，重写那份把两句都抄进图里而且自己多补一份锁",
        [dig(plain, "structure.picture_list[0].alt.descr"),
         dig(plain, "structure.picture_list[0].alt_in_picture.name"),
         dig(plain, "structure.picture_list[0].alt_in_picture.descr_written"),
         dig(plain, "structure.picture_list[0].pic_locks"),
         dig(lo, "structure.picture_list[0].alt_in_picture.descr"),
         dig(lo, "structure.picture_list[0].pic_locks")],
        ["一个红点", "dot.png", False, None, "一个红点",
         {"noChangeAspect": "1", "noChangeArrowheads": "1"}],
    )
    check(
        "号是生产者自己排的而解出来的部件是同一个：rId9 与 rId2 都到 word/media/image1.png",
        [dig(plain, "structure.picture_list[0].blip_id"), dig(lo, "structure.picture_list[0].blip_id"),
         dig(plain, "structure.picture_list[0].target"), dig(lo, "structure.picture_list[0].target")],
        ["rId9", "rId2", "word/media/image1.png", "word/media/image1.png"],
    )
    check(
        "浮起来那一种：摆法写在元素名上，绕排写在元素名与属性上，位置一半在属性一半在字里",
        [dig(plain, "structure.picture_list[0].placed"),
         dig(floaty, "structure.picture_list[0].placed"),
         dig(floaty, "structure.picture_list[0].wrap"),
         dig(floaty, "structure.picture_list[0].wrap_written"),
         dig(floaty, "structure.picture_list[0].position_h"),
         dig(floaty, "structure.picture_list[0].position_v"),
         dig(floaty, "structure.picture_list[0].simple_pos"),
         dig(plain, "structure.picture_list[0].wrap")],
        ["inline", "anchor", "wrapSquare", {"wrapText": "largest"},
         {"written": {"relativeFrom": "column"}, "element": "align", "value": "center"},
         {"written": {"relativeFrom": "paragraph"}, "element": "posOffset", "value": "635"},
         {"x": "0", "y": "0"}, None],
    )
    check(
        "同一种摆法换到 ODF 是另一个词：as-char 与 char 各按各的文件交，不折成一个词",
        [dig(odt, "structure.picture_list[0].placed"),
         dig(odtf, "structure.picture_list[0].placed"),
         dig(odt, "structure.picture_list[0].alt"),
         dig(odtf, "structure.picture_list[0].href"),
         dig(odt, "structure.picture_list[0].mime")],
        ["as-char", "char", "一个红点",
         "Pictures/100000000000002800000018CB9DEC0B.png", "image/png"],
    )
    for name in ("images.rtf", "notes.rtf", "toc.rtf"):
        got = lbin("office-doc", fixture(name))
        want = files[name]["rtf"]["picture_rows"]
        with_alt = sum(1 for one in want if one["alt"])
        check("%s 每张图整份账与读者一致（三种单位、格式的两份凭据、形状属性那格）" % name,
              dig(got, "structure.picture_list"), want)
        check("%s 几张图、几张有替代文字" % name,
              [dig(got, "structure.pictures"), dig(got, "structure.pictures_with_alt_text"),
               dig(got, "structure.pictures_without_alt_text")],
              [len(want), with_alt, len(want) - with_alt])
    rtf = lbin("office-doc", fixture("images.rtf"))
    check(
        "RTF 把「多大」拆成三种单位写：像素、twips 目标与缩放百分比都在，而这一族没有 DPI 可换算像素",
        [dig(rtf, "structure.picture_list[0].pixels.w"),
         dig(rtf, "structure.picture_list[0].goal.w"),
         dig(rtf, "structure.picture_list[0].goal.mm_w"),
         dig(rtf, "structure.picture_list[0].goal.unit"),
         dig(rtf, "structure.picture_list[0].scale.x"),
         dig(rtf, "structure.picture_list[0].blip"),
         dig(rtf, "structure.picture_list[0].sig"),
         dig(rtf, "structure.picture_list[0].sig_agrees"),
         dig(rtf, "structure.picture_list[0].head_hex")],
        ["40", "480", 847, "twips", "472", "pngblip", "png", True, "89504e470d0a1a0a"],
    )
    logo_rtf = lbin("office-doc", fixture("notes.rtf"))
    check(
        "那一格形状属性写了而值是空的：alt_written 是 true 而「有替代文字」是 0 —— 说了与说了字是两件事",
        [dig(logo_rtf, "structure.picture_list[0].props_written"),
         dig(logo_rtf, "structure.picture_list[0].alt_written"),
         dig(logo_rtf, "structure.picture_list[0].alt"),
         dig(logo_rtf, "structure.picture_list[0].props[1].name"),
         dig(logo_rtf, "structure.pictures"),
         dig(logo_rtf, "structure.pictures_with_alt_text")],
        [True, True, "", "wzName", 1, 0],
    )
    check(
        "同一句替代文字在三家写在三个地方，而字一字不差：docx 的属性、odt 的孩子元素、rtf 的形状属性表",
        [dig(plain, "structure.picture_list[0].alt.descr"),
         dig(odt, "structure.picture_list[0].alt"),
         dig(rtf, "structure.picture_list[0].alt"),
         dig(rtf, "structure.picture_list[0].props[0].name")],
        ["一个红点", "一个红点", "一个红点", "wzDescription"],
    )
    # 模板自带的另一枚小图（手上本来就有的三份件）：零替代文字与跨家换算的第二个凭据
    logo = lbin("office-doc", fixture("notes.docx"))
    logo_lo = lbin("office-doc", fixture("notes.odt"))
    check(
        "模板那枚 101600 EMU 的小图：docx 与 odt 两边换算撞在同一个 282 上，而替代文字整个没写",
        [dig(logo, "structure.picture_list[0].extent.mm_w"),
         dig(logo_lo, "structure.picture_list[0].mm_w"),
         dig(logo, "structure.picture_list[0].alt.descr_written"),
         dig(logo_lo, "structure.picture_list[0].alt_written"),
         dig(logo, "structure.pictures"),
         dig(logo, "structure.pictures_with_alt_text")],
        [282, 282, False, False, 1, 0],
    )

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
    # 注记整本账：`/Annots` 里不止链接。一条批注带着自己的弹出框，而那个框也在同一个
    # 数组里 —— 所以「几条注记」与「几条批注」是两个数
    ANNOT_KEYS = ("total", "notes", "popups", "links", "no_subtype", "by_subtype", "items")
    for name in ("pdf-comments.pdf", "notes.pdf", "deck.pdf", "risk.pdf"):
        mine = lbin("office-pdf", fixture(name)).get("annotations") or {}
        theirs = files[name]["pdf"].get("annotations") or {}
        check("%s 注记整本账与读者一致" % name,
              {k: mine.get(k) for k in ANNOT_KEYS}, {k: theirs.get(k) for k in ANNOT_KEYS})
    noted = lbin("office-pdf", fixture("pdf-comments.pdf"))
    check(
        "一条批注三条注记：作者与日期在文件里本来就是一条串，那个全零的 /M 原样交",
        [dig(noted, "annotations.total"), dig(noted, "annotations.notes"),
         dig(noted, "annotations.popups"), dig(noted, "annotations.links"),
         dig(noted, "annotations.no_subtype"),
         dig(noted, "annotations.items[1].page"), dig(noted, "annotations.items[1].object"),
         dig(noted, "annotations.items[1].subtype"),
         dig(noted, "annotations.items[1].author"),
         dig(noted, "annotations.items[1].contents"),
         dig(noted, "annotations.items[1].modified"),
         dig(noted, "annotations.items[1].popup"),
         dig(noted, "annotations.items[1].parent"),
         dig(noted, "annotations.items[2].parent")],
        [3, 1, 1, 1, 0, 2, 11, "Text", "liuqi, 09/23/26, ", "这里要补上不含税口径",
         "D:00000000000000Z", 12, None, 11],
    )
    check(
        "同一份 docx 不带生产者那个开关导出去，批注就整个不见：notes 交 0 而不是缺键",
        [dig(lbin("office-pdf", fixture("notes.pdf")), "annotations.total"),
         dig(lbin("office-pdf", fixture("notes.pdf")), "annotations.notes"),
         dig(lbin("office-pdf", fixture("deck.pdf")), "annotations.total"),
         dig(lbin("office-pdf", fixture("deck.pdf")), "annotations.by_subtype")],
        [1, 0, 0, []],
    )
    # 去哪儿那一层：书签 / 页内链接 / 权限位（三份都是同一份读者的另一段代码）
    for name in ("notes.pdf", "deck.pdf", "objstm.pdf", "risk.pdf", "locked.pdf", "perms.pdf",
                 "forms-hier.pdf", "pdf-comments.pdf"):
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

    for name in ("notes.pdf", "deck.pdf", "objstm.pdf", "locked.pdf", "risk.pdf",
                 "forms-hier.pdf"):
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
    # 表单那一份账：`/AcroForm` → `/Fields` → `/Kids`。值只交文件写的，不算 appearance、
    # 不验签名（签名那一件与「加密件不解密」同一个说法：只认 `/FT` 是 Sig 与 `/Sig` 在不在）
    for name in ("notes.pdf", "deck.pdf", "objstm.pdf", "perms.pdf", "locked.pdf", "risk.pdf",
                 "forms-hier.pdf"):
        check("%s 表单那份账与读者一致" % name,
              lbin("office-pdf", fixture(name)).get("form"),
              files[name]["pdf"].get("form"))
    rform = risk.get("form") or {}
    check(
        "risk.pdf 那一条字段：名字、类型与值都按写的交",
        [rform.get("present"), rform.get("object"), rform.get("roots"), rform.get("total"),
         rform.get("with_v"), (rform.get("by_type") or {}).get("text"), rform.get("widgets"),
         dig(risk, "form.items[0].partial"), dig(risk, "form.items[0].qualified"),
         dig(risk, "form.items[0].type"), dig(risk, "form.items[0].value"),
         dig(risk, "form.items[0].value_present"), dig(risk, "form.items[0].widget")],
        [True, 12, 1, 1, 1, 1, 1, "name", "name", "Tx", "x", True, True],
    )
    check(
        "没表单的三份：present 是 false，不是整个没有这个键",
        [(lbin("office-pdf", fixture(name)).get("form") or {}).get("present")
         for name in ("notes.pdf", "deck.pdf", "perms.pdf")],
        [False, False, False],
    )
    # 分层那一件：/FT 与 /Ff 只写在祖父上，两级孩子各自往上走一跳、两跳才拿到。
    # 这一份是 pikepdf 挂出来的（编辑器没一个肯在父字段上写 /FT），第三个读者 pypdf 数过
    hier = lbin("office-pdf", fixture("forms-hier.pdf"))
    check(
        "forms-hier.pdf 那三层字段：七条根、十二条账、最深第三层",
        [dig(hier, "form.roots"), dig(hier, "form.total"), dig(hier, "form.deepest"),
         dig(hier, "form.inherited_type"), dig(hier, "form.inherited_flags"),
         dig(hier, "form.with_v"), dig(hier, "form.widgets"),
         dig(hier, "form.by_type.choice"), dig(hier, "form.by_type.button"),
         dig(hier, "form.by_type.unknown"), dig(hier, "form.need_appearances"),
         dig(hier, "form.sig_flags")],
        [7, 12, 2, 4, 4, 7, 6, 2, 5, 1, True, 1],
    )
    check(
        "继承来的类型与开关：City 自己一个字都没写",
        [dig(hier, "form.items[3].qualified"), dig(hier, "form.items[3].type"),
         dig(hier, "form.items[3].type_inherited"), dig(hier, "form.items[3].flags"),
         dig(hier, "form.items[3].flags_inherited"), dig(hier, "form.items[3].value"),
         dig(hier, "form.items[1].flags"), dig(hier, "form.items[1].max_len")],
        ["Person.Address.City", "Tx", True, 4, True, "杭州", 1, 4],
    )
    check(
        "/Opt 的两种合法写法：成对与摊平各是一份账，键整个没有不是空数组",
        [dig(hier, "form.items[4].options"), dig(hier, "form.items[4].options_shape"),
         dig(hier, "form.items[5].options"), dig(hier, "form.items[5].options_shape"),
         dig(hier, "form.items[0].options_shape")],
        [["1", "一", "2", "二"], "pairs", ["甲", "乙", "丙"], "flat", None],
    )
    check(
        "/V 的四件事：写空串、写数组、写名字、整个没写（形状与几段值都按写的交）",
        [dig(hier, "form.items[11].value"), dig(hier, "form.items[11].value_shape"),
         dig(hier, "form.items[11].value_parts"),
         dig(hier, "form.items[5].value"), dig(hier, "form.items[5].value_shape"),
         dig(hier, "form.items[5].value_parts"),
         dig(hier, "form.items[4].value_shape"), dig(hier, "form.items[4].value_parts"),
         dig(hier, "form.items[0].value_shape"), dig(hier, "form.items[0].value_present")],
        ["", "string", [], None, "array", ["甲", "丙"], "string", [], None, False],
    )
    check(
        "勾选框那三处：/V 是名字、当前显示在控件的 /AS、可显示的几种在 /AP 的 /N 键上",
        [dig(hier, "form.items[6].value_shape"), dig(hier, "form.items[6].value"),
         dig(hier, "form.items[6].value_name"), dig(hier, "form.items[6].as_state"),
         dig(hier, "form.items[6].ap_states"),
         dig(hier, "form.items[7].value_present"), dig(hier, "form.items[7].value_name"),
         dig(hier, "form.items[7].as_state"),
         dig(hier, "form.items[9].qualified"), dig(hier, "form.items[9].as_state"),
         dig(hier, "form.items[9].type_inherited"),
         dig(hier, "form.items[10].as_state"), dig(hier, "form.items[10].ap_states")],
        ["other", None, "Yes", "Yes", ["Off", "Yes"],
         False, None, "Off",
         "Pick.On", "One", True,
         "Two", []],
    )
    check(
        "六条控件同时挂在页的 /Annots 上：字段树只从 /Fields 走，一条没数两遍",
        [dig(hier, "form.total"), dig(hier, "form.widgets"), dig(hier, "features.fields")],
        [12, 6, 7],
    )
    check(
        "没表单的那几份：一条也没数出来",
        [(lbin("office-pdf", fixture(name)).get("form") or {}).get("total")
         for name in ("notes.pdf", "deck.pdf", "perms.pdf", "locked.pdf", "objstm.pdf")],
        [0, 0, 0, 0, 0],
    )

    # ── 6b) PDF 的正文：两套读者逐页对字 ───────────────────────────
    # 这一层的价值全在「顺序对」上：字都认得、顺序排错，输出看着像读通了其实没有
    for name in ("notes.pdf", "deck.pdf", "objstm.pdf", "locked.pdf", "risk.pdf",
                 "forms-hier.pdf"):
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

    # ── 3bj) office-sheet 的合并区间：哪一块、跨几格、底下有没有字、与自报的 count 对账
    print("=== 3bj) office-sheet 的合并区间：两份读者逐表逐键对整本账 ===")
    MERGE_KEYS = ("total", "listed", "cut", "declared", "declared_matches", "distinct",
                  "duplicated", "overlapping", "solo", "covered_cells",
                  "anchors_with_text", "bad_refs")
    # 全库反查：xlsx 按部件名归位、ods 按表名归位，整本账（含逐行几何）逐键对
    for name in sorted(one.name for one in FIXTURES.glob("*.xlsx")):
        got = lbin("office-sheet", fixture(name))
        mine = {
            (one.get("part") or "").rsplit("/", 1)[-1][: -len(".xml")]: one.get("merges")
            for one in got.get("sheets", [])
        }
        check("%s 每张表的区间账与读者一致" % name, mine, files[name]["ooxml"]["merges"])
    for name in sorted(one.name for one in FIXTURES.glob("*.ods")):
        got = lbin("office-sheet", fixture(name))
        mine = [one.get("merges") for one in got.get("sheets", [])]
        check("%s 每张表的区间账与读者一致" % name, mine,
              [one["merges"] for one in files[name]["ods"]["sheets"]])
    # 自证一：OOXML 那本里区间条数与老的 merged 同一次走查，两本不许各数各的
    for name in ("book.xlsx", "merges.xlsx", "merges-lo.xlsx", "locked-sheet.xlsx"):
        got = lbin("office-sheet", fixture(name))
        check("%s 区间条数与那个只数条数的老数是同一个来源" % name,
              [(one.get("merged"), dig(one, "merges.total")) for one in got.get("sheets", [])],
              [(one.get("merged"), one.get("merged")) for one in got.get("sheets", [])])
    m = lbin("office-sheet", fixture("merges.xlsx"))
    mlo = lbin("office-sheet", fixture("merges-lo.xlsx"))
    mods = lbin("office-sheet", fixture("merges.ods"))
    ms = files["merges.xlsx"]["ooxml"]["merges"]
    mlos = files["merges-lo.xlsx"]["ooxml"]["merges"]
    mw = ms["sheet1"]
    modw = {one["name"]: one for one in files["merges.ods"]["ods"]["sheets"]}
    check(
        "merges.xlsx 第一张表的区间账（重叠只数后来那条、单格合并算 solo）",
        [dig(m, "sheets[0].merges.%s" % key) for key in MERGE_KEYS],
        [mw[key] for key in MERGE_KEYS],
    )
    check(
        "五条形状各异的区间都按文件原样交：顺序是生产者自己的，不是排过序的",
        [one.get("written") for one in dig(m, "sheets[0].merges.rows")],
        ["A1:D1", "A12", "B7:C9", "A3:A5", "A3:C5"],
    )
    check(
        "每一块的几何各自算清：锚点、止点、跨几行几列、盖住几格",
        [(one.get("anchor"), one.get("end"), one.get("rows"), one.get("cols"),
          one.get("cells"), one.get("covered"), one.get("solo"),
          one.get("anchor_has_text"), one.get("overlaps_earlier"))
         for one in dig(m, "sheets[0].merges.rows")],
        [(one["anchor"], one["end"], one["rows"], one["cols"], one["cells"], one["covered"],
          one["solo"], one["anchor_has_text"], one["overlaps_earlier"])
         for one in mw["rows"]],
    )
    check(
        "LibreOffice 重写同一张表：单格那条与重叠里较小的一条一起丢掉，count 跟着改口",
        [dig(mlo, "sheets[0].merges.total"), dig(mlo, "sheets[0].merges.declared"),
         dig(mlo, "sheets[0].merges.overlapping"), dig(mlo, "sheets[0].merges.solo"),
         [one.get("written") for one in dig(mlo, "sheets[0].merges.rows")]],
        [3, 3, 0, 0, ["A1:D1", "A3:C5", "B7:C9"]],
    )
    check(
        "整册那三本合计就是逐表之和（两个 xlsx 生产者各一份）",
        [dig(m, "workbook.totals.merges_ranges"), dig(m, "workbook.totals.merges_covered"),
         dig(m, "workbook.totals.merges_overlapping"),
         dig(mlo, "workbook.totals.merges_ranges"), dig(mlo, "workbook.totals.merges_covered"),
         dig(mlo, "workbook.totals.merges_overlapping")],
        [sum(one["total"] for one in ms.values()),
         sum(one["covered_cells"] for one in ms.values()),
         sum(one["overlapping"] for one in ms.values()),
         sum(one["total"] for one in mlos.values()),
         sum(one["covered_cells"] for one in mlos.values()),
         sum(one["overlapping"] for one in mlos.values())],
    )
    check(
        "整册与那张表各守各的数：整本 6 条区间、21 格被盖、1 对重叠，第一张表自己 1 条单格、count 也对得上",
        [dig(m, "workbook.totals.merges_ranges"), dig(m, "workbook.totals.merges_covered"),
         dig(m, "workbook.totals.merges_overlapping"),
         dig(m, "sheets[0].merges.solo"), dig(m, "sheets[0].merges.declared_matches")],
        [6, 21, 1, 1, True],
    )
    check(
        "ODF 没有区间串也没有 count：那一本照样有三条，锚点与止点从跨度加出来",
        [dig(mods, "sheets[0].merges.declared"), dig(mods, "sheets[0].merges.total"),
         [(one.get("anchor"), one.get("end"), one.get("written"))
          for one in dig(mods, "sheets[0].merges.rows")]],
        [None, 3, [("A1", "D1", None), ("A3", "C5", None), ("B7", "C9", None)]],
    )
    check(
        "合并块底下一个字都没有：这一本数得到，老的那个只数有字合并格的数是 0",
        [dig(mods, "sheets[2].merged"), dig(mods, "sheets[2].merges.total"),
         dig(mods, "sheets[2].merges.anchors_with_text"), dig(mods, "sheets[2].merges.solo"),
         dig(mods, "sheets[2].merges.covered_cells")],
        [modw["只有合并块"]["merged"], 1, 0, 0, 3],
    )
    modws = {one["name"]: one["merges"] for one in files["merges.ods"]["ods"]["sheets"]}
    check(
        "ods 那一族的整册合计也是逐表之和",
        [dig(mods, "workbook.totals.merges_ranges"),
         dig(mods, "workbook.totals.merges_covered"),
         dig(mods, "workbook.totals.merges_overlapping")],
        [sum(one["total"] for one in modws.values()),
         sum(one["covered_cells"] for one in modws.values()),
         sum(one["overlapping"] for one in modws.values())],
    )
    check(
        "两边同一个口径：整张表没并过格的是全零的一份账，不是缺键",
        [dig(m, "sheets[1].merges"), dig(mods, "sheets[1].merges")],
        [files["merges.xlsx"]["ooxml"]["merges"]["sheet2"], modw["一格也没并"]["merges"]],
    )

    # ── 3bk) 主题那一本：十二格有两种写法、「写了空串」与「没这个槽」是两件事、一份包可以有很多本
    print("=== 3bk) theme：主题部件那本账逐件与第二读者对（OOXML 三家读内容，ODF 三家交零条）===")
    theme_rows: list = []
    for name in sorted(one.name for one in list(FIXTURES.glob("*.docx"))
                       + list(FIXTURES.glob("*.docm"))):
        got = dig(lbin("office-doc", fixture(name), "--limit", "400"), "structure.theme")
        check("%s 的主题账整本与读者一致（两种写法、三个 @name、字体那三槽各交各的）" % name,
              got, files[name]["ooxml"]["theme"])
        theme_rows.extend(got["parts"])
    for name in sorted(one.name for one in FIXTURES.glob("*.xlsx")):
        got = dig(lbin("office-sheet", fixture(name), "--limit", "400"), "theme")
        check("%s 的主题账整本与读者一致（这一族按序号点主题，格序就是答案的一半）" % name,
              got, files[name]["ooxml"]["theme"])
        theme_rows.extend(got["parts"])
    for name in sorted(one.name for one in FIXTURES.glob("*.pptx")):
        got = dig(lbin("office-slide", fixture(name), "--limit", "400"), "theme")
        check("%s 的主题账整本与读者一致（一个母版一个部件，所以逐件记账）" % name,
              got, files[name]["ooxml"]["theme"])
        theme_rows.extend(got["parts"])
    # ODF 三家一件主题部件都没有：那一格交零条的账，而不是缺这个键
    for pattern, command, key, mine in (
        ("*.odt", "office-doc", "structure.theme", "odt"),
        ("*.ods", "office-sheet", "theme", "ods"),
        ("*.odp", "office-slide", "theme", "odp"),
    ):
        for name in sorted(one.name for one in FIXTURES.glob(pattern)):
            check("%s 的 ODF 那一面没有主题这个概念（零条）" % name,
                  dig(lbin(command, fixture(name), "--limit", "400"), key),
                  files[name][mine]["theme"])
    # 语料级：整库 196 个部件（来自上面三份 OOXML 家族的真件，不是读者算的）
    dk1_sys = [one["part"] for one in theme_rows
               if one["slots"] and one["slots"][0]["kind"] == "sysClr"]
    with_extra = [one["part"] for one in theme_rows
                  if "extraClrSchemeLst" in one["root_children"]]
    roles = [row for one in theme_rows for row in one["fonts"]]
    fmt_shapes = sorted({tuple((x["list"], x["entries"]) for x in one["fmt"])
                         for one in theme_rows})
    check(
        "整库 235 个主题部件的三条自证：十二格的名字与顺序全对（235/235 canonical、每本 12 格）、"
        "fmtScheme 全是四列各三条（数出来的一致，不是照规范抄的）、"
        "而 dk1 用 sysClr 的那一批与写了 extraClrSchemeLst 的那一批是同一批（88 = 88，一份不差）—— "
        "「MS 那一路」在这两个记号上同进同出，所以这一路认得出",
        [len(theme_rows), sum(1 for one in theme_rows if one["unread"]),
         sorted({one["slot_total"] for one in theme_rows}),
         sum(1 for one in theme_rows if one["canonical"]), fmt_shapes,
         len(dk1_sys), len(with_extra), sorted(dk1_sys) == sorted(with_extra)],
        [235, 0, [12], 235, [(('fillStyleLst', 3), ('lnStyleLst', 3), ('effectStyleLst', 3), ('bgFillStyleLst', 3))], 88, 88, True],
    )
    check(
        "一个部件里的三个 @name 各说各的：theme 两种（Office Theme 226 / Office 9）、"
        "clrScheme 两种（Office 224 / LibreOffice 11，LibreOffice 重写时改的就是这一个）、"
        "fontScheme 一种（235 个全写 Office），而 fmtScheme 只在那 88 个里点名",
        [dict(Counter(one["theme_name"] for one in theme_rows)),
         dict(Counter(one["scheme_name"] for one in theme_rows)),
         dict(Counter(one["font_name"] for one in theme_rows)),
         dict(Counter("写了" if one["fmt_name"] is not None else "没写" for one in theme_rows))],
        [{'Office Theme': 226, 'Office': 9}, {'Office': 224, 'LibreOffice': 11}, {'Office': 235}, {'没写': 147, '写了': 88}]
    )
    check(
        "字体那三槽的待遇：latin 470 个角色全写了名字（没有一个空串），"
        "ea 与 cs 各是 174 写了值、296 写了空串、0 个没这个槽 —— "
        "空串与不在场是两件事，这一本分列而不是并成一格；"
        "按书写系统分的那一批 8802 条，29 或 30 条一套（差一枚 Geor），"
        "而 script=Hans 那一条整库只有一个答案",
        [len(roles),
         [sum(1 for one in roles if isinstance(one[which], str) and one[which] != "")
          for which in ("latin", "ea", "cs")],
         [sum(1 for one in roles if one[which] == "") for which in ("latin", "ea", "cs")],
         [sum(1 for one in roles if one[which] is None) for which in ("latin", "ea", "cs")],
         sum(len(one["faces"]) for one in roles),
         sorted(Counter(len(one["faces"]) for one in roles).items()),
         sorted({one["typeface"] for role in roles for one in role["faces"]
                 if one["script"] == "Hans"})],
        [470, [470, 174, 174], [0, 296, 296], [0, 0, 0], 8802, [(0, 174), (29, 78), (30, 218)], ['宋体']]
    )
    d = dig(lbin("office-slide", fixture("deck-lo.pptx"), "--limit", "400"), "theme")
    dl = dig(lbin("office-slide", fixture("deck-lo.pptx"), "--limit", "5"), "theme")
    check(
        "限额这一格是真截：`parts` 交 5 本、`listed` 5、`cut` true，"
        "可 `total` 与合计那一本仍是 12 本的账 —— 限额只管列几本，不管这份包里有几个部件",
        [d["total"], d["listed"], d["cut"], dl["listed"], dl["cut"],
         dl["totals"]["theme_parts"], dl["totals"]["slots"], dl["totals"]["by_scheme_name"]],
        [12, 12, False, 5, True, 12, 144, {"Office": 11, "LibreOffice": 1}],
    )
    w = dig(lbin("office-doc", fixture("bkmks.docx"), "--limit", "400"), "structure.theme")
    wlo = dig(lbin("office-doc", fixture("bkmks-lo.docx"), "--limit", "400"), "structure.theme")

    def slot_of(book: dict, which: str) -> dict:
        return next(one for one in book["parts"][0]["slots"] if one["slot"] == which)

    check(
        "两家写的不是同一本：Word 那份 dk1 是 `sysClr`（`lastClr=000000` 加 `val=windowText` 两格），"
        "LibreOffice 重写那份换成 `srgbClr` 且 `system` 那一格随之没有 —— 同一个黑换了写法，"
        "而 accent1 两家都是 4F81BD（原样留着，没被重写）；这一族的颜色还有第二种存法："
        "themeElements 之外那一跳（objectDefaults）只有 MS 那一路写",
        [[slot_of(w, "dk1")[key] for key in ("kind", "written", "system")],
         [slot_of(wlo, "dk1")[key] for key in ("kind", "written", "system")],
         [slot_of(w, "accent1")["written"], slot_of(wlo, "accent1")["written"]],
         [w["parts"][0]["root_children"], wlo["parts"][0]["root_children"]]],
        [["sysClr", "000000", "windowText"], ["srgbClr", "000000", None],
         ["4F81BD", "4F81BD"],
         [["themeElements", "objectDefaults", "extraClrSchemeLst"], ["themeElements"]]],
    )
    check(
        "重打那一本与照搬那一本不是一本账：LibreOffice 重写 docx 时把 MS 的主题照搬"
        "（60 条 `a:font`、两个空串槽都还在），自己重打的 pptx 一份 `a:font` 也不写、"
        "ea/cs 全交 DejaVu Sans —— 所以 faces 60 对 0、ea_blank 2 对 0 是两族的实测差",
        [wlo["totals"]["faces"], wlo["totals"]["ea_blank"], wlo["parts"][0]["fonts"][0]["ea"],
         d["totals"]["faces"], d["totals"]["ea_written"], d["totals"]["ea_blank"],
         d["parts"][0]["fonts"][0]["ea"], d["parts"][0]["fonts"][0]["kids"]],
        [60, 2, "", 0, 24, 0, "DejaVu Sans", ["latin", "ea", "cs"]],
    )

    def no_theme_key(command: str, name: str, key: str = "theme") -> bool:
        """整份输出里找 `key` 这个键（遗留那三家还没读，缺键要说得出口）"""
        stack = [lbin(command, fixture(name))]
        while stack:
            one = stack.pop()
            if isinstance(one, dict):
                if key in one:
                    return True
                stack.extend(one.values())
            elif isinstance(one, list):
                stack.extend(one)
        return False

    check(
        "遗留那三家（.doc / .ppt / .xls）与 RTF 现在还没有这一本：那一份主题数据住在 CFB 的 "
        "`theme` 流里（RTF 是 `{\\*\\themedata}` 那一段 base64），实测这一批件里都没有 `theme` 这个键 —— "
        "说得出的才交，交不出的别交一个零条的账冒充读过了",
        [no_theme_key("office-doc", "eq.doc"), no_theme_key("office-slide", "deck.ppt"),
         no_theme_key("office-sheet", "book.xls"), no_theme_key("office-doc", "comments.rtf")],
        [False, False, False, False],
    )

    # ── 3bl) 正文那些手指：主题那一本问「这一格坐着什么」，这一本反过来问「谁点了这一格」。
    #         三种点法各数各的（Word 的名字+影子实色、DrawingML 的名字+孩子修饰符、Excel 的序号），
    #         名字→格三条来路都记在 via 上，解不出交 null；带修饰符的一律不判（不算色）。
    print("=== 3bl) color_refs：手指账逐件与第二读者对（三条点法、三条来路、序号两读、ODF 交零条）===")
    ref_rows: list = []
    odf_groups: dict = {}
    ref_groups: dict = {}
    for pattern, command, key, mine in (
        ("*.docx", "office-doc", "structure.color_refs", "ooxml"),
        ("*.docm", "office-doc", "structure.color_refs", "ooxml"),
        ("*.xlsx", "office-sheet", "color_refs", "ooxml"),
        ("*.pptx", "office-slide", "color_refs", "ooxml"),
        ("*.odt", "office-doc", "structure.color_refs", "odt"),
        ("*.ods", "office-sheet", "color_refs", "ods"),
        ("*.odp", "office-slide", "color_refs", "odp"),
    ):
        for name in sorted(one.name for one in FIXTURES.glob(pattern)):
            got = dig(lbin(command, fixture(name), "--limit", "400"), key)
            check("%s 的手指账整本与读者一致（名字、来路、影子、修饰符、两读，一格都不许差）" % name,
                  got, files[name][mine]["color_refs"])
            if pattern.startswith("*.od"):
                odf_groups.setdefault(pattern, []).append(got)
            else:
                ref_rows.append(got)
                ref_groups.setdefault(pattern, []).append(got)

    ref_counters = [
        "refs", "wml_color", "scheme_clr", "theme_index", "parts_scanned", "parts_unread",
        "parts_with_refs", "theme_parts", "clr_map_written", "alias_names", "alias_conflict",
        "in_slots", "off_slots", "resolved", "unresolved", "matched", "mismatched",
        "skip_modified", "skip_no_literal", "skip_no_slot", "skip_multi_value",
        "index_agree", "index_disagree",
    ]
    summed = {one: sum(row["totals"][one] for row in ref_rows) for one in ref_counters}
    via: Counter = Counter()
    for row in ref_rows:
        via.update(row["totals"]["by_via"])
    check(
        "171 个 OOXML 包 40695 条手指：三条点法各自数得回来（Word 32943 + DrawingML 7616 + Excel 序号 136），"
        "走过 2424 个 `.xml` 部件、`parts_unread` 4 个读不开（三件在 `customxml-lo.docx`、一件在 `sdt-lo.docx`，都是 LibreOffice 清空的 0 字节 `customXml/itemN.xml` —— 件在而里面没字，不是解析器不行），其中 235 个是主题部件（与主题那本同一数）、"
        "731 个部件里手指在场；名字→格的三条来路之和也是 40695 —— 名字本身就是一格 8600、"
        "Word 那一族的别名 28392、文件自己写的 `a:clrMap` 897、序号 136，剩下 2670 条交 null 而不照着规范替文件补",
        [len(ref_rows), summed["refs"], summed["wml_color"], summed["scheme_clr"],
         summed["theme_index"], summed["parts_scanned"], summed["parts_unread"],
         summed["theme_parts"], summed["parts_with_refs"],
         [summed["in_slots"], summed["off_slots"], summed["resolved"], summed["unresolved"]],
         [via["name"], via["wml-alias"], via["clrMap"], via["index"], via["(没写)"],
          sum(via.values())],
         [summed["alias_names"], summed["alias_conflict"], summed["clr_map_written"],
          summed["skip_multi_value"]]],
        [171, 40695, 32943, 7616, 136, 2424, 4, 235, 731, [8600, 32095, 38025, 2670], [8600, 28392, 897, 136, 2670, 40695], [396, 0, 97, 0]],
    )
    check(
        "解不出那 2670 条不是「读不到」而是文件自己没说，而且数目能拆开对：按名字 "
        "`phClr` 2605 条（主题占位色，压根不是那十二格之一）+ `dark2` 12 条（整批带 shade，别名表不收）"
        "剩 53 条；`tx1` / `bg1` 一共 950 条，走 `a:clrMap` 解出的 897 条，"
        "两本一减也是 53 —— 两个方向算出同一个数，那 53 条就是落在没写对照的包里的那些",
        [via["(没写)"],
         sum(row["totals"]["by_name"].get("schemeClr", {}).get("phClr", 0) for row in ref_rows),
         sum(row["totals"]["by_name"].get("wmlColor", {}).get("dark2", 0) for row in ref_rows),
         sum(row["totals"]["by_name"].get("schemeClr", {}).get(one, 0) for one in ("tx1", "bg1")
             for row in ref_rows),
         via["clrMap"],
         sum(row["totals"]["by_name"].get("schemeClr", {}).get(one, 0) for one in ("tx1", "bg1")
             for row in ref_rows) - via["clrMap"],
         via["(没写)"]
         - sum(row["totals"]["by_name"].get("schemeClr", {}).get("phClr", 0)
               for row in ref_rows)
         - sum(row["totals"]["by_name"].get("wmlColor", {}).get("dark2", 0)
               for row in ref_rows)],
        [2670, 2605, 12, 950, 897, 53, 53],
    )
    doc_rows = ref_groups["*.docx"] + ref_groups["*.docm"]
    check(
        "Word 那一路是自己跟自己核对的：每一条 `w:color` 都另写了一遍六位实色当影子，"
        "无修饰符的 29465 条与本包主题那一格逐条对上、`mismatched` 0 条，"
        "剩下 4333 条带 `themeTint` / `themeShade`（不算色）、315 条连影子都没写、381 条点不出格 —— "
        "四种判不住分列，加起来正好是那一路的 34494 条，一条也没被揉成一格",
        [len(doc_rows), sum(one["totals"]["wml_color"] for one in doc_rows),
         sum(one["totals"]["matched"] for one in doc_rows),
         sum(one["totals"]["mismatched"] for one in doc_rows),
         sum(one["totals"]["skip_modified"] for one in doc_rows),
         sum(one["totals"]["skip_no_literal"] for one in doc_rows),
         sum(one["totals"]["skip_no_slot"] for one in doc_rows),
         sum(one["totals"]["refs"] for one in doc_rows),
         sum(one["totals"]["matched"] + one["totals"]["mismatched"]
             + one["totals"]["skip_modified"] + one["totals"]["skip_no_literal"]
             + one["totals"]["skip_no_slot"] + one["totals"]["skip_multi_value"]
             for one in doc_rows)],
        [95, 32943, 29465, 0, 4333, 315, 381, 34494, 34494],
    )
    idx_rows = [one for one in ref_rows if one["totals"]["theme_index"]]
    check(
        "序号那一族两读都交：110 条点的是 `1` 或 `4` 两个数字，"
        "`1` 那 102 条规范顺序说 lt1 而 Excel 实际用的那张表说 dk1（`index_disagree`），"
        "`4` 那 8 条两边说的是同一格 accent1（`index_agree`）—— 谁胜出不是这本的活，"
        "两格并排放着才是答案",
        [len(idx_rows), sum(one["totals"]["theme_index"] for one in idx_rows),
         sum(one["totals"]["index_agree"] for one in idx_rows),
         sum(one["totals"]["index_disagree"] for one in idx_rows),
         sorted({one for row in idx_rows for one in row["totals"]["by_name"]["themeIndex"]}),
         sorted({(one["name"], one["slot"], one["alt_slot"], one["via"])
                 for row in idx_rows for one in row["refs"] if one["kind"] == "themeIndex"})],
        [43, 136, 8, 128, ['1', '4'], [('1', 'lt1', 'dk1', 'index'), ('4', 'accent1', 'accent1', 'index')]],
    )
    deck_tx = [one for one in files["deck.pptx"]["ooxml"]["color_refs"]["refs"]
               if one["name"] in ("tx1", "bg1")]
    chart_tx = [one for one in files["chart-lo.xlsx"]["ooxml"]["color_refs"]["refs"]
                if one["name"] in ("tx1", "bg1")]
    check(
        "`a:clrMap` 只有幻灯片这一族写（23 份 pptx 全写、Word 与 Excel 那 121 个包一个都不写），"
        "而 85 份对照说的是同一套十二对（`alias_conflict` 0）：于是同名的一指在两种包里两个答案 —— "
        "deck.pptx 里 62 条 tx1/bg1 由文件自己解到 dk1/lt1，chart-lo.xlsx 那 8 条同一名字落在"
        "没写对照的包里就交解不出（slot null、via null），不是猜一个补上",
        [sum(1 for one in ref_rows if one["totals"]["clr_map_written"]),
         sum(1 for one in ref_rows if not one["totals"]["clr_map_written"]),
         sum(one["totals"]["clr_map_written"] for one in ref_rows),
         sum(one["totals"]["by_via"].get("clrMap", 0) for one in ref_rows),
         len(deck_tx), sorted({(one["slot"], one["via"]) for one in deck_tx}),
         len(chart_tx), sorted({(one["slot"], one["via"], one["part"]) for one in chart_tx})],
        [33, 138, 97, 897, 62, [('dk1', 'clrMap'), ('lt1', 'clrMap')], 8, [(None, None, 'xl/charts/style1.xml'), (None, None, 'xl/charts/style2.xml')]],
    )
    crefs = dig(lbin("office-doc", fixture("bkmks.docx"), "--limit", "400"),
               "structure.color_refs")
    crefs5 = dig(lbin("office-doc", fixture("bkmks.docx"), "--limit", "5"),
                 "structure.color_refs")
    check(
        "限额这一格只管列几条，不管这份包里的账：`--limit 5` 交 5 条而 `total` 与合计仍是 543 条的账"
        "（`parts_scanned` 13、`parts_with_refs` 3、`matched` 466 一个都不动）—— "
        "整库被 400 截住的只有那 39 个 Word 包，`cut` 就是说给你听的",
        [crefs["total"], crefs["listed"], crefs["cut"], crefs5["total"], crefs5["listed"],
         crefs5["cut"], crefs5["totals"]["refs"], crefs5["totals"]["parts_scanned"],
         crefs5["totals"]["parts_with_refs"], crefs5["totals"]["matched"],
         sorted(one["part"] for one in crefs5["refs"])[:2],
         sum(1 for one in ref_rows if one["cut"])],
        [543, 400, True, 543, 5, True, 543, 13, 3, 466,
         ["word/styles.xml", "word/styles.xml"], 42],
    )
    check(
        "第一条就是这样写的：`word/styles.xml` 里 `rPr` 上那枚 `w:color`，名字 accent1 本身就是一格"
        "（`via = name`），影子实色另写了一遍 `365F91`，而这一格带着 `themeShade=BF`"
        "（`shade` 交的就是文件写的 BF），于是 `matches` 交 null —— 带修饰符的不判，判它就得先算色；"
        "Word 那一路 520 条全坐在 `color/rPr` 这一个座位上",
        [crefs["refs"][0], crefs["totals"]["by_holder"]["wmlColor"]],
        [{"part": "word/styles.xml", "kind": "wmlColor", "at": "color", "holder": "rPr",
          "name": "accent1", "slot": "accent1", "via": "name", "alt_slot": None,
          "literal": "365F91", "tint": None, "shade": "BF", "mods": ["themeShade"],
          "in_slots": True, "matches": None},
         {"color/rPr": 520}],
    )
    odf_counters = ["refs", "parts_scanned", "parts_unread", "theme_parts",
                    "clr_map_written", "wml_color", "scheme_clr", "theme_index",
                    "resolved", "unresolved", "matched"]
    for pattern, describe, want_packages, want_scanned in (
        ("*.odt", "文字", 51, 259),
        ("*.ods", "表格", 15, 81),
        ("*.odp", "演示", 18, 98),
    ):
        rows = odf_groups[pattern]
        got = {one: sum(row["totals"][one] for row in rows) for one in odf_counters}
        check(
            "ODF 那 %s 份 %s件没有主题这个概念：手指一本零条，可部件照样数得到（%s 个 `.xml`，"
            "一个都没读不开）—— 「这一层不存在」与「我没读」是两件事："
            "键一个不少、十二格每格都空着交出去，`total` / `listed` / `cut` 也是零条的样子"
            % (want_packages, describe, want_scanned),
            [len(rows), got["refs"], got["parts_scanned"], got["parts_unread"],
             [got[one] for one in ("theme_parts", "clr_map_written", "wml_color",
                                   "scheme_clr", "theme_index", "resolved",
                                   "unresolved", "matched")],
             sorted({len(row["slots"]) for row in rows}),
             sorted({sum(len(one["values"]) for one in row["slots"]) for row in rows}),
             sorted({(row["total"], row["listed"], row["cut"]) for row in rows})],
            [want_packages, 0, want_scanned, 0, [0, 0, 0, 0, 0, 0, 0, 0], [12], [0],
             [(0, 0, False)]],
        )
    check(
        "遗留那三家与 RTF 连这一本都没有：`color_refs` 这个键在 .doc / .ppt / .xls / RTF 的整份输出里"
        "一次都没出现 —— 那一份主题数据坐在 CFB 的 `Theme` 流与 RTF 的 `{\\*\\themedata}` 群里，"
        "本机做不出凭据，所以那一本还没开；开不了就说没开，别交一本零条的账冒充读过了",
        [no_theme_key("office-doc", "eq.doc", "color_refs"),
         no_theme_key("office-slide", "deck.ppt", "color_refs"),
         no_theme_key("office-sheet", "book.xls", "color_refs"),
         no_theme_key("office-doc", "comments.rtf", "color_refs")],
        [False, False, False, False],
    )

    # ── 3bm) 排版兼容：OOXML 一种问话两种写法，ODF 把开关摊平在另一本账里 ──────────
    print("=== 3bm) 排版兼容：`<w:compat>` 两种写法各摊一本，ODF 那一格根本不存在 ===")
    for name in sorted(one.name for one in FIXTURES.glob("*.docx")):
        got = lbin("office-doc", fixture(name))
        check("%s 排版兼容那份账与读者一致（具名项一本 + 裸开关一本）" % name,
              dig(got, "structure.layout_compat"),
              files[name]["ooxml"]["layout_compat"])
    for name in sorted(one.name for one in FIXTURES.glob("*.odt")):
        got = lbin("office-doc", fixture(name))
        check("%s 排版兼容那份账与读者一致（ooo:configuration-settings 摊平的那几条）" % name,
              dig(got, "structure.layout_compat"),
              files[name]["odt"]["layout_compat"])
    for name in sorted(one.name for one in FIXTURES.glob("*.docm")):
        got = lbin("office-doc", fixture(name))
        check("%s 排版兼容那份账与读者一致（这一族的第 82 份，带这一格的是 .docx 之外还有宏文档）" % name,
              dig(got, "structure.layout_compat"),
              files[name]["ooxml"]["layout_compat"])
    dm = dig(lbin("office-doc", fixture("notes.docm")), "structure.layout_compat")
    check(
        "那一份 .docm 是带这一格的**第 82 份**（81 份 .docx 全有，宏文档也有），而它的账与那 38 份"
        " python-docx 模板件同形：`compatibilityMode=14` 之外还带三条具名项（`overrideTableStyleFontSizeAndJustification`"
        " / `enableOpenTypeFeatures` / `doNotFlipMirrorIndents`，三条的 `w:val` 都写着 1），裸开关仍然只有"
        " `useFELayout` 一枚。把宏文档算进来，具名项的名字总数**不增**（还是六个）、`w:uri` 也不增 —— "
        "第 74 份不是新形状，只是这一格不独属于 .docx",
        [dm["children_total"], dm["mode"], len(dm["named"]), dm["named_names"],
         dm["named"][1:], len(dm["switches"]), dm["switch_names"],
         dm["names_in_both_encodings"], dm["uris"], dm["mode_total"]],
        [5, "14", 4,
         ["compatibilityMode", "overrideTableStyleFontSizeAndJustification",
          "enableOpenTypeFeatures", "doNotFlipMirrorIndents"],
         [{"name": "overrideTableStyleFontSizeAndJustification",
           "uri": "http://schemas.microsoft.com/office/word", "val": "1"},
          {"name": "enableOpenTypeFeatures",
           "uri": "http://schemas.microsoft.com/office/word", "val": "1"},
          {"name": "doNotFlipMirrorIndents",
           "uri": "http://schemas.microsoft.com/office/word", "val": "1"}],
         1, ["useFELayout"], [], ["http://schemas.microsoft.com/office/word"], 1],
    )
    compat = dict((one, had) for one, had in files.items()
                  if one.endswith(".docx") and "ooxml" in had)
    shapes = {}
    for had in compat.values():
        book = had["ooxml"]["layout_compat"]
        key = "%d+%d" % (len(book["named"]), len(book["switches"]))
        shapes[key] = shapes.get(key, 0) + 1
    check(
        "两种写法在同一段里搭配出六种形状（81 份 .docx 按「几条具名项 + 几枚裸开关」数）："
        "`4+1` 是那 38 份模板件，`4+0` 是 30 份 LibreOffice 重写且一个开关都不补，`1+2` 是 `nset` 那一家"
        "四份（只说 compatibilityMode 与两枚开关），`3+0` 五份、`3+2` 三份，而 `2+0` **只有一份** —— "
        "所以「具名项至少写四条」是生产者的习惯，不是这一格的规矩",
        [shapes.get("4+1"), shapes.get("4+0"), shapes.get("1+2"), shapes.get("3+0"),
         shapes.get("3+2"), shapes.get("2+0"), len(shapes)],
        [44, 36, 4, 5, 4, 1, 6],
    )
    bad = sorted(one for one, had in compat.items() if not (
        had["ooxml"]["layout_compat"]["settings_part"]
        and had["ooxml"]["layout_compat"]["compat_written"]
        and had["ooxml"]["layout_compat"]["compat_total"] == 1
        and had["ooxml"]["layout_compat"]["children_total"] > 0
        and had["ooxml"]["layout_compat"]["names_in_both_encodings"] == []
        and had["ooxml"]["layout_compat"]["uris"] == ["http://schemas.microsoft.com/office/word"]
        and all(not row["val_written"]
                for row in had["ooxml"]["layout_compat"]["switches"])))
    modes, switches = {}, {}
    for had in compat.values():
        book = had["ooxml"]["layout_compat"]
        modes[book["mode"]] = modes.get(book["mode"], 0) + 1
        for one in book["switch_names"]:
            switches[one] = switches.get(one, 0) + 1
    check(
        "全语料不变量（81 份 .docx）：`<w:compat>` 只在 `word/settings.xml`（218 份 zip 件的 2432 份 "
        "xml 部件里只此一名）、没有一份是空的、两种写法的名字互不重叠、`w:uri` 恒那一条、"
        "而**没有一枚裸开关写过 `w:val`** —— 违反的那几份交出来（这里应当是空表）",
        [len(compat), bad, modes, switches],
        [94, [], {'14': 79, '15': 10, '12': 5}, {'useFELayout': 44, 'adjustLineHeightInTable': 4, 'doNotUseHTMLParagraphAutoSpacing': 8, 'doNotBreakWrappedTables': 4}],
    )
    only_fe = sorted(one for one, had in compat.items() if had["ooxml"]["layout_compat"]["switch_names"]
                     == ["useFELayout"])
    with_break = sorted(one for one, had in compat.items() if "doNotBreakWrappedTables"
                        in had["ooxml"]["layout_compat"]["switch_names"])
    flat = dict((one, had) for one, had in files.items()
                if one.endswith(".odt") and "odt" in had)
    same_name = sorted(one for one, had in flat.items() if had["odt"]["layout_compat"]["same_name_rows"])
    no_block = sorted(one for one, had in flat.items()
                      if not had["odt"]["layout_compat"]["item_set_written"])
    check(
        "同一个 `<w:compat>` 两套笔迹（按 `docProps/app.xml` 的 Application 分）：python-docx 那份模板"
        "（写着 Microsoft Macintosh Word）38 份**都只带 `useFELayout`**，另外 43 份出自 LibreOffice、"
        "其中 30 份一个裸开关都不写。而跨到 ODF 那一头是**另一套丢法**：带 `doNotBreakWrappedTables` "
        "的 .docx 有 4 份，只有一条同名字段的 .odt 只有 2 份，另有 3 份 odt 整个没有 settings.xml",
        [len(only_fe), only_fe[:1], len(with_break), with_break, len(flat),
         same_name, no_block],
        [44, ['alternate.docx'], 4, ['notes-end.docx', 'notes-foot.docx', 'nset-lo.docx', 'nset.docx'], 51, ['notes-end.odt', 'nset.odt'], ['cjk-odf.odt', 'pnum.odt', 'tbox.odt', 'wrap.odt']],
    )
    lc_docx = lbin("office-doc", fixture("nset.docx"))
    lc_odt = lbin("office-doc", fixture("nset.odt"))
    check(
        "`nset.docx`：`<w:compat>` 三个孩子、一种写法一条 —— 具名项只写了 `compatibilityMode=12`，"
        "裸开关 `doNotUseHTMLParagraphAutoSpacing` 与 `doNotBreakWrappedTables` **身上什么都没有**"
        "（在场即开，`val_written` 是 false）。转成 odt 之后 `<w:compat>` 整格不复存在，"
        "同一条 `DoNotBreakWrappedTables` 变成 `ooo:configuration-settings` 123 格里的一条",
        [dig(lc_docx, "structure.layout_compat.children_total"),
         dig(lc_docx, "structure.layout_compat.mode"),
         dig(lc_docx, "structure.layout_compat.named"),
         dig(lc_docx, "structure.layout_compat.switches"),
         dig(lc_docx, "structure.layout_compat.names_in_both_encodings"),
         dig(lc_odt, "structure.layout_compat.items_total"),
         dig(lc_odt, "structure.layout_compat.same_name_rows")],
        [3, "12",
         [{"name": "compatibilityMode", "uri": "http://schemas.microsoft.com/office/word",
           "val": "12"}],
         [{"name": "doNotUseHTMLParagraphAutoSpacing", "val_written": False, "val": None,
           "on": True},
          {"name": "doNotBreakWrappedTables", "val_written": False, "val": None, "on": True}],
         [], 123, ["DoNotBreakWrappedTables"]],
    )
    check(
        "ODF 那一本的**大小本身**是一份账：这一组 123 条、其中 108 条是 `boolean`、"
        "63 条写着 true；类型直方图按类型名排序（缺 `config:type` 的自成一类，这里没有）",
        [dig(lc_odt, "structure.layout_compat.settings_part"),
         dig(lc_odt, "structure.layout_compat.item_set_written"),
         dig(lc_odt, "structure.layout_compat.booleans_total"),
         dig(lc_odt, "structure.layout_compat.booleans_true"),
         dig(lc_odt, "structure.layout_compat.types"),
         dig(lc_odt, "structure.layout_compat.compat_item_total")],
        [True, True, 108, 63,
         [{"type": "base64Binary", "count": 2}, {"type": "boolean", "count": 108},
          {"type": "int", "count": 4}, {"type": "short", "count": 3},
          {"type": "string", "count": 6}], 5],
    )
    check(
        "那四条名字 47 份全写（45 份就这四条、另 2 份多一条 `DoNotBreakWrappedTables`），**可值不是套话**：`MsWordUlTrailSpace` 47 份全 false，"
        "另外三条多数 true —— 而 `tbox-lo.odt` 四条全 false、`images-float.odt` 只错开一条。"
        "（每份按名字排序，所以第一格在同名那一条存在时是 `DoNotBreakWrappedTables`）",
        [dict((one, [row["value"] for row in
                     flat[one]["odt"]["layout_compat"]["compat_items"]])
              for one in ["nset.odt", "tabs.odt", "images-float.odt", "tbox-lo.odt"])],
        [{"nset.odt": ["true", "true", "true", "true", "false"],
          "tabs.odt": ["true", "true", "true", "false"],
          "images-float.odt": ["true", "false", "true", "false"],
          "tbox-lo.odt": ["false", "false", "false", "false"]}],
    )
    lim_docx = dig(lbin("office-doc", fixture("notes.docm"), "--limit", "2"), "structure.layout_compat")
    lim_odt = dig(lbin("office-doc", fixture("nset.odt"), "--limit", "1"), "structure.layout_compat")
    check(
        "`--limit` 只砍列表、砍不动算术（与主题那本同一条规矩）：宏文档限到 2 时 `named` 交按文档顺序的前两条，"
        "而 `named_names` 仍是四条、`children_total` 5 一格没动；odt 限到 1 时 `compat_items` 只剩一条"
        "（按名字排的第一条正是同名那条 `DoNotBreakWrappedTables`），而 `compat_item_total` 5、`items_total` 123、"
        "`booleans_true` 63 全按整本数",
        [len(lim_docx["named"]), lim_docx["named"], len(lim_docx["named_names"]),
         lim_docx["children_total"], lim_docx["switch_names"], len(lim_odt["compat_items"]),
         lim_odt["compat_items"], lim_odt["compat_item_total"], lim_odt["items_total"],
         lim_odt["booleans_true"], len(lim_odt["types"])],
        [2, [{"name": "compatibilityMode", "uri": "http://schemas.microsoft.com/office/word",
              "val": "14"},
             {"name": "overrideTableStyleFontSizeAndJustification",
              "uri": "http://schemas.microsoft.com/office/word", "val": "1"}],
         4, 5, ["useFELayout"], 1,
         [{"name": "DoNotBreakWrappedTables", "type": "boolean", "value": "true",
           "via": "same-name"}],
         5, 123, 63, 5],
    )
    check(
        "反面凭据：`pnum.odt` 与 `tbox.odt` **整个没有 `settings.xml`** —— 于是这一本交空账"
        "（`settings_part` / `item_set_written` 是 false，不是「有 0 条的组」）。"
        "另外两族写了同一组却没有那四条名字（.ods 那 15 份里 14 份 39 条、`workbook-settings.ods` 40 条（多的那格是 `CodeName`）、17 份 .odp 写 42 或 43 条（另 1 份零条）），"
        "而 RTF 与遗留 .doc 连这一本都没有 —— 缺键 = 这一族没这一层",
        [dig(lbin("office-doc", fixture("pnum.odt")), "structure.layout_compat.items_total"),
         dig(lbin("office-doc", fixture("pnum.odt")),
             "structure.layout_compat.item_set_written"),
         no_theme_key("office-doc", "tabs.rtf", "layout_compat"),
         no_theme_key("office-doc", "notes-en.doc", "layout_compat"),
         no_theme_key("office-sheet", "book.ods", "layout_compat"),
         no_theme_key("office-slide", "deck.odp", "layout_compat")],
        [0, False, False, False, False, False],
    )

    # ── 3bn) 文档默认值：OOXML 写在 `<w:docDefaults>` 两层（可能两个部件各一遍），
    #        ODF 摊成 `style:default-style` 一族一条 ──────────────────────────
    print("=== 3bn) 没写样式的字长什么样：docDefaults 两层 vs 一族一条 ===")
    for name in sorted(one.name for one in FIXTURES.glob("*.docx")):
        got = lbin("office-doc", fixture(name))
        check("%s 默认值那份账与读者一致（docDefaults 两层 + Normal 那一格）" % name,
              dig(got, "structure.doc_defaults"),
              files[name]["ooxml"]["doc_defaults"])
    for name in sorted(one.name for one in FIXTURES.glob("*.docm")):
        got = lbin("office-doc", fixture(name))
        check("%s 默认值那份账与读者一致（宏文档也写这一格，第 74 份不是新形状）" % name,
              dig(got, "structure.doc_defaults"),
              files[name]["ooxml"]["doc_defaults"])
    for name in sorted(one.name for one in FIXTURES.glob("*.odt")):
        got = lbin("office-doc", fixture(name))
        check("%s 默认值那份账与读者一致（一族一条，属性住在孩子的孩子上）" % name,
              dig(got, "structure.doc_defaults"),
              files[name]["odt"]["doc_defaults"])

    def tally(vals):
        out = {}
        for one in vals:
            key = tuple(one) if isinstance(one, list) else one
            out[key] = out.get(key, 0) + 1
        return out

    def rows_of(pool, key):
        return [row for had in pool.values() for row in had[key]]

    dd = dict((one, had["ooxml"]["doc_defaults"]) for one, had in files.items()
              if one.endswith((".docx", ".docm")))
    check(
        "95 份 word 件（94 份 .docx + 1 份 .docm）**全有这一层**，50 份只在 `word/styles.xml` 写一块、"
        "45 份在 `word/stylesWithEffects.xml` 又写了一遍（所以 `blocks_total` 是 1 或 2，没有一份是 3）；"
        "每一块的孩子恒 `rPrDefault + pPrDefault` 两条（没有一份是 1 或 3），而这一格在 43 份 .xlsx 与"
        "33 份 .pptx 里一份都没有 —— 所以账本交「几块、各在哪个部件、每块说了什么」，"
        "只在 office-doc 这一族交",
        [len(dd), tally([one["blocks_total"] for one in dd.values()]),
         tally([one["children_total"] for one in dd.values()]),
         tally([one["styles_effects_part"] == (one["blocks_total"] == 2) for one in dd.values()]),
         tally([one["parts_with_block"] for one in dd.values()]),
         tally([one["block_shapes"][0]["children"] == one["block_shapes"][-1]["children"]
                == ["rPrDefault", "pPrDefault"] for one in dd.values()])],
        [95, {1: 50, 2: 45}, {2: 95}, {True: 95}, {('word/styles.xml',): 50, ('word/styles.xml', 'word/stylesWithEffects.xml'): 45}, {True: 95}],
    )
    check(
        "`rPr` 三种形状（87 份就 `rFonts, sz, szCs, lang` 四条、4 份在 `rFonts` 后多插 `kern`、"
        "4 份多插 `color`；`extras` 交的是「四条常项以外还写了什么」），`pPr` 三档 "
        "（50 份 LibreOffice 写 `suppressAutoHyphens`、44 份 Word 写 `spacing`、1 份在 `spacing` 后"
        "多一条 `adjustRightInd`）",
        [tally([one["rpr_names"] for one in dd.values()]),
         tally([one["ppr_names"] for one in dd.values()]),
         tally([one["extras"] for one in dd.values()])],
        [{('rFonts', 'sz', 'szCs', 'lang'): 87, ('rFonts', 'color', 'sz', 'szCs', 'lang'): 4, ('rFonts', 'kern', 'sz', 'szCs', 'lang'): 4}, {('suppressAutoHyphens',): 50, ('spacing',): 44, ('spacing', 'adjustRightInd'): 1}, {(): 87, ('color',): 4, ('kern',): 4}],
    )
    check(
        "字体指针两列各交各的：45 份只写四条主题指针（末条是**小写开头**的 `cstheme`，"
        "按前缀 `theme` 认会一条不中，所以按结尾认）、36 份主题与字面名两套都写、14 份只写四条字面名；"
        "而字面名**可以是空串** —— `w:cs=\"\"` 在 35 份里在场，「写了这个属性」与「点了个字体名」是两件事",
        [tally([[one["wrote_theme"], one["wrote_literal"]] for one in dd.values()]),
         tally([one["font_blank_attrs"] for one in dd.values()]),
         tally([one for value in dd.values() for one in value["font_blank_attrs"]])],
        [{(True, True): 36, (True, False): 45, (False, True): 14}, {('cs',): 35, (): 60}, {'cs': 35}],
    )
    check(
        "`sz` 与 `szCs` 在 95 份里**恒等**（22 的 87 份、24 的 8 份）但两枚分开交；"
        "`w:lang` 三条属性名 95 份全写，值却只有两组（87 份 `en-US/en-US/ar-SA` 对 8 份 "
        "`en-US/zh-CN/hi-IN`）",
        [tally([one["size_written"] == one["size_cs_written"] for one in dd.values()]),
         tally([one["size_written"] for one in dd.values()]),
         tally([[one["lang_written"]["val"], one["lang_written"]["eastAsia"],
                 one["lang_written"]["bidi"]] for one in dd.values()]),
         tally([tuple(sorted(one["lang_written"])) for one in dd.values()])],
        [{True: 95}, {'22': 87, '24': 8}, {('en-US', 'en-US', 'ar-SA'): 87, ('en-US', 'zh-CN', 'hi-IN'): 8}, {('bidi', 'eastAsia', 'val'): 95}],
    )
    check(
        "第二层在 `<w:style w:styleId=\"Normal\">`（`styleId` 与 `type` 两个属性都对上才算，"
        "字符样式也叫 Normal）：95 份全找得到。45 份的 Normal **一个 `rPr`/`pPr` 都不写**"
        "（全靠 docDefaults），另 50 份两个都写、并把上面那套**摊平**进来"
        "（`rPr` 六条 rFonts/color/kern/sz/szCs/lang 的 46 份，另有 4 份只写 sz + lang）—— "
        "同一句话在不同生产者手里住在不同的格子里，所以两层的账都得交",
        [tally([one["normal_style"]["found"] for one in dd.values()]),
         tally([one["normal_style"]["children"] for one in dd.values()]),
         tally([one["normal_style"]["rpr_rows"] == [] for one in dd.values()]),
         tally([len(one["normal_style"]["rpr_rows"]) for one in dd.values()
                if one["normal_style"]["rpr_rows"]]),
         tally([one["normal_style"]["found"] and one["blocks_total"] > 0 for one in dd.values()])],
        [{True: 95}, {('name', 'qFormat', 'rsid', 'pPr', 'rPr'): 36, ('name', 'qFormat', 'rsid'): 45, ('name', 'qFormat', 'pPr', 'rPr'): 10, ('name', 'pPr', 'rPr'): 4}, {False: 50, True: 45}, {6: 46, 2: 4}, {True: 95}],
    )
    check(
        "两家各自同形（同一句话的两种写法在这一格里数得出）：`.docm` 与它的 .docx 原型逐键相同、"
        "`pnum.docx` 与 `tbox.docx` 也是；`crep.docx` 是那份把 `w:cs` 写成空串、"
        "Normal 的 `pPr` 第一条是个空壳 `widowControl`（有话说、没属性）的",
        [dd["notes.docm"] == dd["notes.docx"], dd["pnum.docx"] == dd["tbox.docx"],
         dd["notes.docx"]["normal_style"]["rpr_rows"],
         dd["crep.docx"]["font_blank_attrs"],
         dd["crep.docx"]["rpr_rows"][0]["attrs"]["cs"],
         dd["crep.docx"]["normal_style"]["ppr_rows"][0]["name"],
         dd["nset.docx"]["extras"], dd["pnum.docx"]["extras"],
         dd["nset.docx"]["size_written"], dd["pnum.docx"]["size_written"]],
        [True, True, [], ["cs"], "", "widowControl", ["kern"], ["color"], "24", "24"],
    )

    check(
        "四把各自独立的钥匙开同一批 44 份（生产者标记在这一格里合上）：`<w:docDefaults>` 在第二个部件"
        "又写一遍、Normal **一个 `rPr` 都不写**、`w:pPr` 那一条叫 `spacing`、字体名**只写主题指针** —— "
        "四个记号有 50 份逐份同假、44 份逐份同真，只有 1 份前三条真而第四条不是（它 `w:pPr` 写的是"
        "`spacing` 之外还多一条 `adjustRightInd`），所以这四把锁交的仍是四列而不是一个「Word 造」布尔"
        "（`extras` 也不在这把锁里：95 份里多写一条的那 8 份全在只写一块的 50 份里）",
        tally([[one["blocks_total"] == 2, one["normal_style"]["rpr_rows"] == [],
                one["ppr_names"] == ["spacing"],
                [one["wrote_theme"], one["wrote_literal"]] == [True, False]]
               for one in dd.values()]),
        {(False, False, False, False): 50, (True, True, True, True): 44, (True, True, False, True): 1},
    )

    of = dict((one, had["odt"]["doc_defaults"]) for one, had in files.items()
              if one.endswith(".odt"))
    check(
        "ODF 那一问一族一条：51 份 .odt 里 47 份恒四条（**写的序** graphic / paragraph / table / table-row，"
        "`families` 是排过序的同一组）、另 4 份（`cjk-odf.odt` / `pnum.odt` / `tbox.odt` / `wrap.odt`）"
        "**有 `styles.xml` 而一条都不写** —— 「零条」与「没有这个部件」两列分开交；51 份的 content.xml 里"
        "`default-style` 出现 **0 次**，但两列计数都留着（断在另一头也要数得出）",
        [len(of), tally([one["defaults_total"] for one in of.values()]),
         tally([one["styles_part"] for one in of.values()]),
         tally([one["defaults_in_content"] for one in of.values()]),
         tally([one["families"] for one in of.values()]),
         tally([one["rows"][0]["family"] if one["rows"] else None for one in of.values()]),
         tally([len(one["rows"]) == one["defaults_total"] for one in of.values()])],
        [51, {4: 47, 0: 4}, {True: 51}, {0: 51}, {('graphic', 'paragraph', 'table', 'table-row'): 47, (): 4}, {'graphic': 47, None: 4}, {True: 51}],
    )
    check(
        "字体名、字号、语言**各三格**（latin / asian / complex），和 docx 的 `rFonts` 四条与 `w:lang` 三条"
        "是同一句话的两种写法。.odt 出口里 94 条带 `text-properties` 的行**字号与语言三格全写满**（各 94 条），"
        "字体名却只有 94 / 93 / 94 —— 差的那一条正是 `tbox-lo.odt` 的 graphic 行 asian 格"
        "「只写号不写名」；而 table 与 table-row 那两条**从来一个字体都不点**（各只带一条属性）。"
        "整库 84 份 odf（51 .odt + 15 .ods + 18 .odp）摊开是 235 条行、141 条带 `text-properties`，"
        "字体名 126 / 125 / 126 对字号与语言各 141（差额 15 / 16 / 15 共 46 条就是「只写号不写名」——"
        "其中 15 份 .ods 的 graphic 行三格全不点名，剩下那 1 条就是 `tbox-lo.odt`）",
        [tally([row["slot"] for row in rows_of(of, "fonts_written")]),
         tally([row["slot"] for row in rows_of(of, "sizes_written")]),
         tally([row["slot"] for row in rows_of(of, "langs_written")]),
         tally([row["family"] for row in rows_of(of, "fonts_written")]),
         tally([row["props_attrs_total"] for one in of.values() for row in one["rows"][2:]]),
         tally([[row["language"], row["country"]] for row in rows_of(of, "langs_written")
                if row["language"] == "none"])],
        [{'latin': 94, 'asian': 93, 'complex': 94}, {'latin': 94, 'asian': 94, 'complex': 94}, {'latin': 94, 'asian': 94, 'complex': 94}, {'graphic': 140, 'paragraph': 141}, {1: 94}, {('none', 'none'): 3}],
    )
    check(
        "那串连字符设置（十三条名字一组，按局部名排序）**只出现在 paragraph 那一族**：51 份里 46 份写，"
        "`tbox-lo.odt` 有 paragraph 一条却一个都不写，另四份（`cjk-odf.odt` / `pnum.odt` / "
        "`tbox.odt` / `wrap.odt`）整本零条；table 一族只写 `table:border-model`、"
        "table-row 一族只写 `fo:keep-together`",
        [sum(1 for one in of.values() if one["hyphenation_names"]),
         tally([len(one["hyphenation_names"]) for one in of.values()]),
         sorted({row["family"] for one in of.values() for row in one["hyphenation_rows"]}),
         len(of["nset.odt"]["hyphenation_names"]),
         of["nset.odt"]["hyphenation_names"][:2],
         [row["props_attrs_total"] for row in of["nset.odt"]["rows"]]],
        [46, {13: 46, 0: 5}, ['paragraph'], 13, ['hyphenate', 'hyphenation-compound-push-char-count'], [31, 34, 1, 1]],
    )
    lim_docx = dig(lbin("office-doc", fixture("notes.docm"), "--limit", "1"), "structure.doc_defaults")
    lim_odt = dig(lbin("office-doc", fixture("nset.odt"), "--limit", "2"), "structure.doc_defaults")
    check(
        "`--limit` 只砍列表、砍不动算术（与主题、兼容那两本同一条规矩）：宏文档限到 1 时 `block_shapes` 只剩"
        "主的那一块、`rpr_rows` 只剩 `rFonts` 一条，而 `blocks_total` 2、`rpr_names` 四条、"
        "`parts_with_block` 两条、`size_written` 一格没动；odt 限到 2 时 `rows` 与三本各剩前两条"
        "（三本都按**行序 × 槽序**排，所以前两格同属 graphic 那一行），而 `defaults_total` 4、"
        "`families` 四条仍按整本数",
        [len(lim_docx["block_shapes"]), lim_docx["block_shapes"], lim_docx["rpr_rows"],
         lim_docx["blocks_total"], lim_docx["rpr_names"], lim_docx["parts_with_block"],
         lim_docx["size_written"], lim_docx["normal_style"],
         len(lim_odt["rows"]), lim_odt["rows"][1], lim_odt["defaults_total"],
         lim_odt["fonts_written"], lim_odt["sizes_written"], lim_odt["langs_written"],
         lim_odt["hyphenation_rows"], lim_odt["families"]],
        [1, [{"children": ["rPrDefault", "pPrDefault"], "part": "word/styles.xml",
              "ppr_names": ["spacing"], "rpr_names": ["rFonts", "sz", "szCs", "lang"]}],
         [{"attrs": {"asciiTheme": "minorHAnsi", "cstheme": "minorBidi",
                     "eastAsiaTheme": "minorEastAsia", "hAnsiTheme": "minorHAnsi"},
           "name": "rFonts"}],
         2, ["rFonts", "sz", "szCs", "lang"],
         ["word/styles.xml", "word/stylesWithEffects.xml"], "22",
         {"children": ["name", "qFormat", "rsid"], "found": True,
          "ppr_rows": [], "rpr_rows": []},
         2,
         {"children": ["paragraph-properties", "text-properties"], "country": "US",
          "country_asian": "CN", "country_complex": "IN", "family": "paragraph",
          "font_name": "Liberation Serif", "font_name_asian": "Noto Serif SC",
          "font_name_complex": "Lucida Sans", "font_size": "12pt", "font_size_asian": "12pt",
          "font_size_complex": "12pt", "language": "en", "language_asian": "zh",
          "language_complex": "hi", "part": "styles.xml", "props_attrs_total": 34},
         4,
         [{"family": "graphic", "part": "styles.xml", "slot": "latin", "value": "Liberation Serif"},
          {"family": "graphic", "part": "styles.xml", "slot": "asian", "value": "Noto Serif SC"}],
         [{"family": "graphic", "part": "styles.xml", "slot": "latin", "value": "12pt"},
          {"family": "graphic", "part": "styles.xml", "slot": "asian", "value": "12pt"}],
         [{"country": "US", "family": "graphic", "language": "en", "part": "styles.xml",
           "slot": "latin"},
          {"country": "CN", "family": "graphic", "language": "zh", "part": "styles.xml",
           "slot": "asian"}],
         [{"family": "paragraph", "name": "hyphenate", "part": "styles.xml", "value": "false"},
          {"family": "paragraph", "name": "hyphenation-compound-push-char-count",
           "part": "styles.xml", "value": "2"}],
         ["graphic", "paragraph", "table", "table-row"]],
    )
    check(
        "反面凭据：这一格只在 office-doc 交。RTF 把默认值混在 `{\\s0 …}` 那一条 Normal 样式里、"
        "归属判不住就不报；遗留 .doc 的住在 styles heap 里、本族料的 .doc 全出自 LibreOffice，"
        "没有第二个读者能核对就不照一个没核过的读法写。另两族也写这一层（15 份 .ods 恒两条、"
        "写的序 table-cell 在前；12 份 .odp 里 11 份一条 graphic、`eqs.odp` 零条）可 office-doc 不读它们 —— "
        "**这一本对那两族没有出口**，缺键 = 这一族没这一层",
        [no_theme_key("office-doc", "tabs.rtf", "doc_defaults"),
         no_theme_key("office-doc", "notes-en.doc", "doc_defaults"),
         no_theme_key("office-sheet", "book.ods", "doc_defaults"),
         no_theme_key("office-slide", "deck.odp", "doc_defaults")],
        [False, False, False, False],
    )

    # ── 3bo) 题注与交叉引用：目标那一刀、被点名的三本书、题注样式那一格 ────────────
    print("=== 3bo) office-doc structure.cross_refs：五种「指向别处」的域一份账，逐行与第二读者对 ===")
    for name in sorted(one.name for one in FIXTURES.glob("*.docx")):
        check("%s 的交叉引用整本与读者一致（目标切法、三本书、resolves 三态）" % name,
              dig(lbin("office-doc", fixture(name)), "structure.cross_refs"),
              files[name]["ooxml"]["cross_refs"])
    for name in sorted(one.name for one in FIXTURES.glob("*.docm")):
        check("%s 的交叉引用整本与读者一致（宏文档也写这一格，第 74 份不是新形状）" % name,
              dig(lbin("office-doc", fixture(name)), "structure.cross_refs"),
              files[name]["ooxml"]["cross_refs"])
    for name in sorted(one.name for one in FIXTURES.glob("*.odt")):
        check("%s 的交叉引用整本与读者一致（目标是属性不是指令串，声明那一层在场）" % name,
              dig(lbin("office-doc", fixture(name)), "structure.cross_refs"),
              files[name]["odt"]["cross_refs"])
    for name in sorted(one.name for one in FIXTURES.glob("*.rtf")):
        check("%s 的交叉引用整本与读者一致（目标解自 `\\fldinst` 那一群，样式号只按文件自己写的交）" % name,
              dig(lbin("office-doc", fixture(name)), "structure.cross_refs"),
              files[name]["rtf"]["cross_refs"])

    cx = dict((one, had["ooxml"]["cross_refs"]) for one, had in files.items()
              if one.endswith((".docx", ".docm")))
    co = dict((one, had["odt"]["cross_refs"]) for one, had in files.items() if one.endswith(".odt"))
    cr = dict((one, had["rtf"]["cross_refs"]) for one, had in files.items() if one.endswith(".rtf"))
    check(
        "同一套格子三族共有：164 份（95 word + 51 份 .odt + 18 份 .rtf）的键集一模一样（18 格），"
        "`books` 那六本也是同一套 —— 差别只在**哪几本填得出东西**：OOXML 与 RTF 的 `sequences` 恒 null"
        "（这一族没有序列声明那一层），ODF 的 `style_ids` 恒 null（样式只有名字，没有 `w:styleId` 那一格）。"
        "`caption_styles` 少的正是 `styles_part` 那一格：**只属于 OOXML** —— ODF 的题注样式住在哪个部件"
        "另有它的账，不在这儿重复一遍部件名",
        [len(cx), len(co), len(cr),
         tally([tuple(sorted(one)) for one in list(cx.values()) + list(co.values()) + list(cr.values())]),
         tally([tuple(sorted(one["books"])) for one in list(cx.values()) + list(co.values())
                + list(cr.values())]),
         tally([tuple(sorted(one["caption_styles"])) for one in cx.values()]),
         tally([tuple(sorted(one["caption_styles"])) for one in co.values()]),
         tally([tuple(sorted(one["caption_styles"])) for one in cr.values()])],
        [95, 51, 18, {('available', 'books', 'cache_missing', 'cache_values', 'cached_but_unresolved', 'caption_styles', 'cut', 'declarations', 'family', 'kinds', 'listed', 'notes', 'quoted', 'resolves', 'resolving_but_no_cache', 'rows', 'target_books', 'target_rows'): 164}, {('bookmark_marks', 'bookmarks', 'sequences', 'style_defs', 'style_ids', 'style_names'): 164}, {('declared', 'paragraphs_using_them', 'rows', 'styles_part'): 95}, {('declared', 'paragraphs_using_them', 'rows'): 51}, {('declared', 'paragraphs_using_them', 'rows'): 18}],
    )
    check(
        "95 份 word 件**每一份都交这一格**，可只有 4 份写着这五种域（`target_rows`：91 份 0 条、"
        "2 份 1 条、2 份 4 条）——「没有交叉引用」与「这一族没有这一层」是两件事，所以零行也整本交。"
        "`kinds` 的总和恒等于 `target_rows`；SEQ 那一种在 OOXML **判不出成不成立**（没有声明那本书可查），"
        "所以 95 份的 `resolves.null` 与 `kinds.SEQ` 全对得上，`declarations` 那一格根本不交，"
        "`notes` 里恒写着那一句解释",
        [tally([one["available"] for one in cx.values()]),
         tally([one["target_rows"] for one in cx.values()]),
         tally([one["books"]["sequences"] for one in cx.values()]),
         tally([one["declarations"] for one in cx.values()]),
         tally([sum(one["kinds"].values()) == one["target_rows"] for one in cx.values()]),
         tally([one["resolves"]["null"] == one["kinds"].get("SEQ", 0) for one in cx.values()]),
         tally([one["notes"][0] for one in cx.values()])],
        [{True: 95}, {0: 91, 1: 2, 4: 2}, {None: 95}, {None: 95}, {True: 95}, {True: 95}, {'这一族没有序列声明这一层：SEQ 的 resolves 一律 null': 95}]
    )
    check(
        "题注样式那两本各查各的：91 份的 `<w:style w:styleId=\"Caption\">` 两个名字都对上"
        "（`w:styleId` 是大写 C、`w:name` 是小写 caption，所以 `matched_on` 交两条），95 份全都"
        "**声明了没人用**（`paragraphs_using_them` 恒 0 —— 题注段用的是生产者摊出来的直接格式）。"
        "一条题注样式都没有的是四份（`cjk-odf-lo.docx`、`pnum.docx`、`tbox.docx`、`wrap-lo.docx`），交 `declared` 0 而不是缺格。"
        "样式那三本账（元素数、`w:styleId` 数、`w:name` 数）94 份恒等，164 与 169 只是两家存量不同 —— 只有 `levels.docx` 一份不恒等（`style_names` 167 对另两本 171，那四枚样式只有 id 没有名），"
        "`bookmarks` 是**名字**那本、`bookmark_marks` 是**元素**那本，名字数永不超过元素数",
        [tally([one["caption_styles"]["declared"] for one in cx.values()]),
         tally([one["caption_styles"]["paragraphs_using_them"] for one in cx.values()]),
         tally([one["caption_styles"]["styles_part"] for one in cx.values()]),
         tally(["|".join([str(r["style_id"]), str(r["type"]), ",".join(r["matched_on"])])
                for one in cx.values() for r in one["caption_styles"]["rows"]]),
         sorted(k for k, v in cx.items() if v["caption_styles"]["declared"] == 0),
         tally([one["books"]["style_defs"] == one["books"]["style_ids"]
                == one["books"]["style_names"] for one in cx.values()]),
         tally([one["books"]["style_defs"] for one in cx.values()]),
         tally([len(one["books"]["bookmarks"]) <= one["books"]["bookmark_marks"] for one in cx.values()])],
        [{1: 91, 0: 4}, {0: 95}, {True: 95}, {'Caption|paragraph|styleId,name': 91}, ['cjk-odf-lo.docx', 'pnum.docx', 'tbox.docx', 'wrap-lo.docx'], {True: 94, False: 1}, {168: 23, 164: 43, 169: 9, 1: 2, 170: 1, 166: 1, 171: 3, 71: 1, 68: 4, 16: 3, 11: 1, 3: 1, 69: 1, 2: 1, 172: 1}, {True: 95}],
    )
    check(
        "ODF 那一族多一本**声明**的账，四格都是数出来的：`elements` 是 `text:sequence-decl` 的枚数"
        "（45 份 5 枚、2 份 6 枚、4 份一枚不写），它与 `books.sequences` 的长度在 51 份里**恒等**；"
        "`wrappers` 交的是「哪个部件里写了几枚」（恒在 `content.xml`，与 `elements` 同数）。"
        "LO 每次存 .odt 都把 Drawing/Figure/Illustration/Table/Text 那五条**模板序列**写进来，"
        "47 份的 `declared_unused` 就是这五个名字 —— 声明了没人用在这一族是常态而不是缺陷；"
        "`used_without_decl` 51 份全空，`sequence_ref_elements` 也全 0（认字表里没有 `text:sequence-ref`）",
        [tally([one["available"] for one in co.values()]),
         tally([one["target_rows"] for one in co.values()]),
         tally([one["declarations"]["elements"] for one in co.values()]),
         tally([one["declarations"]["elements"] == len(one["books"]["sequences"]) for one in co.values()]),
         tally([one["declarations"]["declared_unused"] for one in co.values()]),
         tally([one["declarations"]["used_without_decl"] for one in co.values()]),
         tally([one["declarations"]["sequence_ref_elements"] for one in co.values()]),
         tally([one["declarations"]["sequence_ref_uses"] for one in co.values()]),
         tally([tuple((r["part"], r["count"]) for r in one["declarations"]["wrappers"])
                for one in co.values()]),
         tally([one["books"]["style_ids"] for one in co.values()]),
         tally([one["books"]["style_defs"] == one["books"]["style_names"] for one in co.values()]),
         tally([one["notes"] for one in co.values()]),
         sorted(k for k, v in co.items() if len(v["books"]["sequences"]) == 6),
         sorted(k for k, v in co.items() if v["caption_styles"]["declared"] == 0)],
        [{True: 51}, {0: 49, 3: 1, 1: 1}, {5: 45, 0: 4, 6: 2}, {True: 51}, {('Drawing', 'Figure', 'Illustration', 'Table', 'Text'): 47, (): 4}, {(): 51}, {0: 51}, {(): 51}, {(('content.xml', 5),): 45, (): 4, (('content.xml', 6),): 2}, {None: 51}, {True: 51}, {(): 51}, ['fields-mix.odt', 'fields.odt'], ['cjk-odf.odt', 'pnum.odt', 'tbox-lo.odt', 'tbox.odt', 'wrap.odt']],
    )
    check(
        "题注样式在 ODF 只有一本可查：`style:name` 那本命中（`matched_on` 一条 `name`），"
        "`style_id` 交 null 而不是 0 —— 这一族没有 id 那一格。46 份命中、5 份没有；"
        "`styles_part` 这一格在这一族根本不出现（缺键，不是 false）",
        [tally([tuple([str(r["style_id"]), r["part"], ",".join(r["matched_on"]),
                       str(r["paragraphs_using_it"])])
                for r in one["caption_styles"]["rows"]] for one in co.values()),
         tally(["styles_part" in one["caption_styles"] for one in co.values()])],
        [{(('None', 'styles.xml', 'name', '0'),): 46, (): 5}, {False: 51}],
    )
    check(
        "RTF 的 18 份全都有题注样式，而它的 `style_id` 是**样式号**（`\\s` 后面那个数），"
        "各家自己写的：54 的 12 份、55 的 3 份、24/109/110 各一份 —— 同一个名字在不同文件里号不同，"
        "所以号只按文件交、不折成 docx 的 `Caption`。`sequences` 与 `style_ids` 两本恒 null"
        "（这一族既没有序列声明层，样式表也只有号与名），`notes` 那一句与 OOXML 同义但措辞各自写",
        [tally([one["target_rows"] for one in cr.values()]),
         tally([one["books"]["sequences"] for one in cr.values()]),
         tally([one["books"]["style_ids"] for one in cr.values()]),
         tally([one["caption_styles"]["declared"] for one in cr.values()]),
         tally([one["caption_styles"]["paragraphs_using_them"] for one in cr.values()]),
         tally([r["part"] for one in cr.values() for r in one["caption_styles"]["rows"]]),
         tally([tuple([r["style_id"], r["type"], ",".join(r["matched_on"])])
                for one in cr.values() for r in one["caption_styles"]["rows"]]),
         tally([one["notes"][0] for one in cr.values()]),
         tally([one["resolves"]["null"] == one["kinds"].get("SEQ", 0) for one in cr.values()]),
         tally([tuple(sorted({o["part"] for o in one["rows"]})) for one in cr.values() if one["rows"]])],
        [{0: 16, 4: 1, 1: 1}, {None: 18}, {None: 18}, {1: 18}, {0: 18}, {"stylesheet": 18},
         {(54, "paragraph", "name"): 12, (55, "paragraph", "name"): 3, (24, "paragraph", "name"): 1,
          (110, "paragraph", "name"): 1, (109, "paragraph", "name"): 1},
         {"这一族也没有序列声明这一层：SEQ 的 resolves 一律 null": 18}, {True: 18},
         {("stream",): 2}],
    )
    x_written = dict((k, v) for pool in (cx, co, cr) for k, v in pool.items() if v["target_rows"])
    check(
        "整个语料只有 8 份写了这五种域（4 份 word、2 份 .odt、2 份 .rtf），合起来 19 条。"
        "行按 `target` × `resolves` × `book` 摊开看：`REF`/`PAGEREF` 三族都**判得出成不成立**"
        "（书签那本有名字可查，全 true），`STYLEREF` 两族都 false（点的是样式名那本，"
        "而 `标题 1` / `标题 1 (user)` 不在库里），`SEQ` 则是 word 与 RTF 交 null、ODF 交 true ——"
        "同一个「查不到」在两类账里是两回事：一类没有那本书，一类有书而没那个名字",
        [len(x_written), sorted(x_written), sum(v["target_rows"] for v in x_written.values()),
         tally([v["family"] for v in x_written.values()]),
         tally([tuple([one["kind"], one["target"], one["resolves"], one["book"]])
                for v in x_written.values() for one in v["rows"]]),
         tally([tuple(sorted(v["resolves"].items())) for v in x_written.values()]),
         tally([tuple(sorted(v["target_books"].items())) for v in x_written.values()]),
         tally([tuple(sorted(v["cache_values"].items())) for v in x_written.values()]),
         tally([tuple(sorted({o["part"] for o in v["rows"]})) for v in x_written.values()])],
        [8, ["fields-lo.docx", "fields-mix-lo.docx", "fields-mix.docx", "fields-mix.odt",
             "fields-mix.rtf", "fields.docx", "fields.odt", "fields.rtf"], 19,
         {"ooxml": 4, "odf": 2, "rtf": 2},
         {("SEQ", "表", None, "sequence"): 3, ("SEQ", "图", None, "sequence"): 3,
          ("REF", "_RefMix1", True, "bookmark"): 3, ("PAGEREF", "_RefMix1", True, "bookmark"): 3,
          ("STYLEREF", "标题 1 (user)", False, "style"): 2, ("STYLEREF", "标题 1", False, "style"): 1,
          ("sequence", "图", True, "sequence"): 1, ("sequence", "表", True, "sequence"): 1,
          ("bookmark-ref", "_RefMix1", True, "bookmark"): 2},
         {(("false", 0), ("null", 1), ("true", 0)): 3,
          (("false", 1), ("null", 1), ("true", 2)): 3,
          (("false", 0), ("null", 0), ("true", 3)): 1,
          (("false", 0), ("null", 0), ("true", 1)): 1},
         {(("sequence", 1),): 4, (("bookmark", 2), ("sequence", 1), ("style", 1)): 3,
          (("bookmark", 2), ("sequence", 1)): 1},
         {(("1", 1),): 4, (("1", 2), ("错误: 引用源未找到", 1)): 1,
          (("1", 3), ("标题一：给 STYLEREF 用", 1)): 1, (("1", 3),): 1,
          (("", 1), ("1", 2), ("错误: 引用源未找到", 1)): 1},
         {("word/document.xml",): 4, ("content.xml",): 2, ("stream",): 2}],
    )
    x_docx = cx["fields-mix.docx"]
    x_lo = cx["fields-mix-lo.docx"]
    x_rtf = cr["fields-mix.rtf"]
    check(
        "同一段稿子两个手：Word 那份与 LibreOffice 重写那份的 `target_rows` 4、`resolves`、`quoted` "
        "三格**完全一致**（切目标的刀不认生产者），差别全在缓存值那一列 —— "
        "Word 那份四行都写了结果（`cache_written` 全 true），LO 那份把 REF 的 `w:result` 留空"
        "（`cached` null 而 `cache_written` false，SEQ/PAGEREF/STYLEREF 照写）；"
        "于是「判成立却没缓存值」那一格 Word 0、LO 1，「引用不成立却有缓存」两族都 1 —— "
        "`cached` 为 null 与 `cached` 为空串也是两回事（RTF 那份 REF 写的是空串）。"
        "样式存量 164 对 169 是两家的账，书签那本两家都只有 `_RefMix1` 一枚",
        [x_docx["target_rows"], x_docx["resolves"], x_docx["quoted"], x_docx["cache_missing"],
         x_docx["resolving_but_no_cache"], x_docx["cached_but_unresolved"],
         [one["kind"] for one in x_docx["rows"]],
         [one["target"] for one in x_docx["rows"]],
         [one["resolves"] for one in x_docx["rows"]],
         [one["target_written_quoted"] for one in x_docx["rows"]],
         [one["next_token_is_switch"] for one in x_docx["rows"]],
         [one["target_unterminated"] for one in x_docx["rows"]],
         [one["cache_written"] for one in x_docx["rows"]],
         [one["cached"] for one in x_docx["rows"]],
         sorted(x_docx["cache_values"]),
         [x_docx["books"]["style_defs"], x_lo["books"]["style_defs"]],
         [x_docx["books"]["bookmarks"], x_lo["books"]["bookmarks"]],
         [x_lo["resolving_but_no_cache"], x_lo["cached_but_unresolved"], x_lo["cache_missing"],
          [one["cached"] for one in x_lo["rows"]], [one["cache_written"] for one in x_lo["rows"]],
          sorted(x_lo["cache_values"])]],
        [4, {"true": 2, "false": 1, "null": 1}, {"true": 1, "false": 3, "null": 0}, 0, 0, 1,
         ["SEQ", "REF", "PAGEREF", "STYLEREF"], ["图", "_RefMix1", "_RefMix1", "标题 1"],
         [None, True, True, False], [False, False, False, True],
         [False, False, False, False], [False, False, False, False],
         [True, True, True, True], ["1", "1", "1", "标题一：给 STYLEREF 用"],
         ["1", "标题一：给 STYLEREF 用"], [164, 169], [["_RefMix1"], ["_RefMix1"]],
         [1, 1, 1, ["1", None, "1", "错误: 引用源未找到"], [True, False, True, True],
          ["1", "错误: 引用源未找到"]]],
    )
    check(
        "ODF 的目标是**属性**不是指令串，所以那一行没有 `instruction` 可切：`target_written_quoted` 交 null"
        "（引号这一问在这一族不存在），另交 `target_written` 表示属性在不在场。"
        "`text:sequence` 那一行的目标就是 `text:name`，同时把 `text:formula`（`ooow:图+1`）、"
        "`text:ref-name`（`ref图0`）、`text:num-format` 与自己的 `own_name` 都分开交 —— "
        "「这一枚属于哪个序列」与「它指向哪个序列」是两格；"
        "两枚 `text:bookmark-ref` 指向同一条 `_RefMix1`，只有 `reference-format` 一条是 `number` 一条是 `page`，"
        "所以「引用了几次」与「引用了谁」不能只报一个数",
        [[one[k] for k in ("element", "kind", "target", "resolves", "target_written",
                           "target_written_quoted", "reference_format", "formula", "ref_name",
                           "own_name", "num_format", "seq_sub_formula", "cached", "cache_written")]
         for one in co["fields-mix.odt"]["rows"]],
        [["text:sequence", "sequence", "图", True, True, None, None, "ooow:图+1", "ref图0",
          "图", "1", None, "1", True],
         ["text:bookmark-ref", "bookmark-ref", "_RefMix1", True, True, None, "number", None,
          "_RefMix1", None, None, None, "1", True],
         ["text:bookmark-ref", "bookmark-ref", "_RefMix1", True, True, None, "page", None,
          "_RefMix1", None, None, None, "1", True]],
    )
    check(
        "RTF 的指令串解掉转义就是 docx 那一串，可样式号只按文件自己写的交（55 而不是 `Caption`），"
        "并且 REF 那行是这一族独有的形状：`\\fldrslt` 在场但**里面没字**，"
        "于是 `cached` 空串、`cache_written` true —— 「写了空结果」与「没写结果」分开记账，"
        "这一条同时进了 `resolving_but_no_cache` 为 0（引用成立、缓存也在场）",
        [x_rtf["rows"][0]["instruction"], x_rtf["rows"][1]["instruction"],
         x_rtf["rows"][2]["instruction"], x_rtf["rows"][3]["instruction"],
         x_rtf["rows"][1]["cached"], x_rtf["caption_styles"]["rows"][0]["style_id"],
         [one["resolves"] for one in x_rtf["rows"]],
         [one["part"] for one in x_rtf["rows"]],
         x_rtf["resolving_but_no_cache"], x_rtf["cached_but_unresolved"],
         [[one[k] for k in ("element", "own_name", "formula", "reference_format",
                            "num_format", "resolves")]
          for one in co["fields.odt"]["rows"]]],
        ["SEQ 图 \\* ARABIC", "REF _RefMix1 \\r \\r \\h", "PAGEREF _RefMix1 \\h",
         'STYLEREF "标题 1 (user)"', "", 55, [None, True, True, False],
         ["stream", "stream", "stream", "stream"], 1, 1,
         [["text:sequence", "表", "ooow:表+1", None, "1", True]]],
    )
    check(
        "`bookmarks` 与 `bookmark_marks` 两本账的差在 ODF 最清楚：`fields.odt` 只有「表锚点」一个名字，"
        "可它由 `text:bookmark` + `text:bookmark-end` 两枚元素写成，所以 marks 2 而 names 1；"
        "docx / RTF 那两份同一个名字只有一枚 `w:bookmarkStart`，两格同为 1。"
        "断链的那两份（`bkmks.docx`、`bkmks.odt`）各 5 枚标记、4 个名字 —— 与书签那一族的账对得上",
        [[k, cx.get(k, co.get(k, cr.get(k)))["books"]["bookmark_marks"],
          len(cx.get(k, co.get(k, cr.get(k)))["books"]["bookmarks"]),
          cx.get(k, co.get(k, cr.get(k)))["books"]["bookmarks"]]
         for k in ("fields.docx", "fields.odt", "fields.rtf", "bkmks.docx", "bkmks.odt",
                   "fields-mix.odt")],
        [["fields.docx", 1, 1, ["表锚点"]], ["fields.odt", 2, 1, ["表锚点"]],
         ["fields.rtf", 1, 1, ["表锚点"]], ["bkmks.docx", 5, 4, ["_GoBack", "口径", "断了", "跨段"]],
         ["bkmks.odt", 5, 4, ["_GoBack", "口径", "口径 副本 1", "跨段"]],
         ["fields-mix.odt", 1, 1, ["_RefMix1"]]],
    )
    x_cut = dig(lbin("office-doc", fixture("fields-mix.docx"), "--limit", "1"),
               "structure.cross_refs")
    check(
        "截行不截账：`--limit 1` 只砍 `rows`（`listed` 1、`cut` true），四条计数仍按全部行算 —— "
        "`target_rows` 4、`kinds` 四种各一枚、`cache_values` 两个值、`books.bookmarks` 那本也照全",
        [x_cut["target_rows"], x_cut["listed"], x_cut["cut"], len(x_cut["rows"]),
         x_cut["kinds"], sorted(x_cut["cache_values"]), x_cut["books"]["bookmarks"],
         x_cut["rows"][0]["kind"], [one["kind"] for one in x_docx["rows"]]],
        [4, 1, True, 1, {"SEQ": 1, "REF": 1, "PAGEREF": 1, "STYLEREF": 1},
         ["1", "标题一：给 STYLEREF 用"], ["_RefMix1"], "SEQ",
         ["SEQ", "REF", "PAGEREF", "STYLEREF"]],
    )
    check(
        "反面凭据：这一格只在 office-doc 交，三族都有出口（RTF 那 18 份也交）。"
        "遗留 .doc 的交叉引用住在 piece 流里的 field 指令，本机没有第二个读者可核对，"
        "所以那一家连「零条」都不报；.ods / .odp 的题注样式在 office-doc 没有出口 —— "
        "12 份 .odp 里 10 份各写 28 条 `presentation` 族样式（名字带 Caption 的那批**是母版样式不是题注**，"
        "另 2 份零条）；把它们算进这一族只会替文件编一本假账，缺键 = 这一族没这一层",
        [no_theme_key("office-doc", "fields-mix.rtf", "cross_refs"),
         no_theme_key("office-doc", "tabs.rtf", "cross_refs"),
         no_theme_key("office-doc", "notes-en.doc", "cross_refs"),
         no_theme_key("office-sheet", "book.ods", "cross_refs"),
         no_theme_key("office-slide", "deck.odp", "cross_refs")],
        [True, True, False, False, False],
    )

    # ── 3bp) 图自己那串字节 vs 文档说的那两个尺寸：一族一条链，五种「没有」各交各的 ──
    print("=== 3bp) office-doc structure.picture_bytes：声明、字节与摆放三份账，逐行与第二读者对 ===")
    for name in sorted(one.name for one in FIXTURES.glob("*.docx")):
        check("%s 的图账整本与读者一致（声明在 `[Content_Types].xml`、地址要顺着 rels 跳一跳）" % name,
              dig(lbin("office-doc", fixture(name)), "structure.picture_bytes"),
              files[name]["ooxml"]["picture_bytes"])
    for name in sorted(one.name for one in FIXTURES.glob("*.docm")):
        check("%s 的图账整本与读者一致（宏文档走同一条 OOXML 链，第 74 份不是新形状）" % name,
              dig(lbin("office-doc", fixture(name)), "structure.picture_bytes"),
              files[name]["ooxml"]["picture_bytes"])
    for name in sorted(one.name for one in FIXTURES.glob("*.odt")):
        check("%s 的图账整本与读者一致（声明是 `draw:mime-type` 属性、地址直接是包内路径）" % name,
              dig(lbin("office-doc", fixture(name)), "structure.picture_bytes"),
              files[name]["odt"]["picture_bytes"])
    for name in sorted(one.name for one in FIXTURES.glob("*.rtf")):
        check("%s 的图账整本与读者一致（声明就是控制字本身，尺寸写成目标 twips × 缩放百分比）" % name,
              dig(lbin("office-doc", fixture(name)), "structure.picture_bytes"),
              files[name]["rtf"]["picture_bytes"])

    pb_dx = dict((one, had["ooxml"]["picture_bytes"]) for one, had in files.items()
                 if one.endswith((".docx", ".docm")))
    pb_od = dict((one, had["odt"]["picture_bytes"]) for one, had in files.items()
                 if one.endswith(".odt"))
    pb_rt = dict((one, had["rtf"]["picture_bytes"]) for one, had in files.items()
                 if one.endswith(".rtf"))
    pb_all = {}
    for pool in (pb_dx, pb_od, pb_rt):
        pb_all.update(pool)
    pb_rows = [(n, one_row) for n in sorted(pb_all) for one_row in pb_all[n]["rows"]]

    def pb_sum(field, sub):
        return [sum(one[field][sub] for one in pool.values())
                for pool in (pb_dx, pb_od, pb_rt)]

    def pb_rows_where(key, want):
        return [(n, one_row["where"]) for n, one_row in pb_rows if one_row[key] is want]

    check(
        "同一套格子三族共有：164 份（94 份 .docx + 1 份 .docm + 51 份 .odt + 18 份 .rtf）"
        "账本 19 格、行 22 格，两家各只有一种取值。`family` 在 ODF 那一族叫 `odf` 而不是 `odt`；"
        "`read_cap` 记的是这一族最多读进图的前多少字节 —— OOXML 两本 65536（够走完 PNG 的块表与 "
        "TIFF 的第一个 IFD），RTF 只有 8192（群头扫 16KB、十六进制解出来的上限），"
        "两边封顶不同本身就是一条实测事实。164 份全都 `available`，没有一份被截过行",
        [len(pb_dx), len(pb_od), len(pb_rt), len(pb_all),
         sorted(set((one["family"], one["read_cap"]) for one in pb_all.values())),
         [sum(1 for one in pb_all.values() if one["available"]),
          sum(1 for one in pb_all.values() if one["cut"]),
          sum(1 for one in pb_all.values() if one["listed"] == one["total"])],
         tally([tuple(sorted(one)) for one in pb_all.values()]),
         tally([tuple(sorted(one_row)) for _, one_row in pb_rows])],
        [95, 51, 18, 164, [('docx', 65536), ('odf', 65536), ('rtf', 8192)], [164, 0, 164], {('addr', 'agrees', 'at_natural', 'available', 'cut', 'declared_pixels', 'density', 'detected', 'distinct_parts', 'ext_agrees', 'family', 'listed', 'natural', 'pixels', 'placed', 'read_cap', 'rows', 'stretched', 'total'): 164}, {('addr', 'agrees', 'aspect_permille', 'at_natural', 'declared_pixels', 'density', 'ext', 'ext_agrees', 'ext_name', 'head_hex', 'how', 'nat_mm100', 'note', 'pixels', 'placed_mm100', 'px_agrees', 'scale_permille', 'sig', 'stretched', 'where', 'word', 'word_name'): 81}],
    )
    check(
        "整库摊开：95 份 word 里 22 份写了图共 41 行、去重部件 34 个（九张框可以指向六个部件 —— "
        "LibreOffice 存件时按**像素内容**去重帧的部件，`distinct_parts` 就是为这件事交的一格）；"
        "51 份 .odt 里 12 份有图 27 行 22 个部件；18 份 .rtf 里 5 份有图 13 行 13 个部件 —— "
        "RTF 的图住在 `{\\pict}` 群里，没有包内路径可去重，所以那一族行数恒等于部件数。"
        "合起来 39 份有图、81 行，最多的一份 9 行；剩下 125 份交**零行的整本账** —— "
        "「这份文档没有图」与「这一族没这一层」是两件事，所以三族都恒开这一格",
        [[len(pool), sum(one["total"] for one in pool.values()),
          sum(one["distinct_parts"] for one in pool.values())]
         for pool in (pb_dx, pb_od, pb_rt)]
        + [[sum(1 for one in pb_all.values() if one["total"])],
           max(one["total"] for one in pb_all.values()), len(pb_rows)],
        [[95, 41, 34], [51, 27, 22], [18, 13, 13], [39], 9, 81],
    )
    check(
        "地址那一格四态：`read` 三族 40 / 27 / 13 全都落到了部件，`unresolved`（跳不到）与 "
        "`missing`（部件不在包里）本库各 0 —— 那两条分支由合成件守着，不是没人写。"
        "`none` 只有 `tbox.docx` 那一条：框里既没有 `a:blip` 也没有 `r:embed`，于是那一行连字节都没有，"
        "`head_hex` 交**空串**（说了「读过、没有」）而签名与像素交 null —— "
        "「没有字节可读」是这本账里的第五种「没有」，前四种在 `density.state` 上",
        [[pb_sum("addr", f) for f in ("read", "unresolved", "missing", "none")],
         [(n, one_row["where"], one_row["sig"], one_row["head_hex"], one_row["note"])
          for n, one_row in pb_rows if one_row["addr"] == "none"]],
        [[[40, 27, 13], [0, 0, 0], [0, 0, 0], [1, 0, 0]],
         [('tbox.docx', '(no-blip)', None, '', None)]],
    )
    check(
        "自带密度四态（`read` 写了可用单位 / `unitless` 写了那个字段但只说长宽比 / "
        "`absent` 这个格式有这一格而这份件没写或写零 / `none` 这个格式压根没有这一格）"
        "三族各 [12,6,6]、[2,1,1]、[16,9,4]、[3,8,2] —— 71 行里只有 24 行说得出自己该占多大地方，"
        "也就是说**三分之二的图只能靠文档那一侧**。单位只有三种真写得出来：ppm 17、dpi 7、"
        "aspect 4（那四条就是 Pillow 存 JPEG 时 JFIF 单位给 0 的那几张），剩下 43 行交 null",
        [[pb_sum("density", f) for f in ("read", "unitless", "absent", "none")],
         tally([one_row["density"]["unit"] for _, one_row in pb_rows])],
        [[[12, 6, 6], [2, 1, 1], [23, 12, 4], [3, 8, 2]],
         {None: 53, 'ppm': 17, 'dpi': 7, 'aspect': 4}],
    )
    check(
        "文件在引用处写下的类型名与图自己头里的签名**从不互相打脸**：`agrees` 64 真 / 0 假 / "
        "7 判不了（那 7 条是 `eq.odt` 的六条 SVM 加 `tbox.docx` 那条没字节的，两边都没有可比的名字）；"
        "扩展名那一本 `ext_agrees` 51 真 / 0 假 / 20 判不了 —— 其中 13 条是 RTF（`{\\pict}` 群里"
        "压根没有扩展名这一说，那一格恒 null），另 7 条同上。"
        "「打脸的行」这一问整库答案是**空清单**，两条都逐行交出来而不是只报一个 0",
        [[pb_sum("agrees", f) for f in ("yes", "no", "undecided")],
         [pb_sum("ext_agrees", f) for f in ("yes", "no", "undecided")],
         pb_rows_where("agrees", False) + pb_rows_where("ext_agrees", False)],
        [[[40, 18, 13], [0, 0, 0], [1, 9, 0]], [[40, 21, 0], [0, 0, 0], [1, 6, 13]], []],
    )
    check(
        "三种「它是什么格式」各交各的：`word` 是文件写在引用处的名字（OOXML 是 `image/png` 那种 MIME、"
        "RTF 是 `\\pngblip` / `\\jpegblip` / `\\wmetafile` 控制字、ODF 的 `draw:mime-type` 可以整条不写 "
        "所以 7 行 null），`ext` 是包内路径尾巴上那个扩展名，`sig` 是从头里认出来的。"
        "名字对得上不等于写法一样：`ext` 交的是 `tif` 4 条、`jpg` 4 条，而认出来是 `tiff` / `jpeg` —— "
        "认字表里那两行同名映射就是为它们写的；`word_name` 与 `ext_name` 是把两种写法归到同一个名字上"
        "的那两本（`png` 42 / `jpeg` 8 / `gif` 5 / `bmp` 3 / `tiff` 4），`how` 那一本记的是**从哪儿读出来的**"
        "（PNG 的 IHDR、JPEG 的 SOF0、BMP 的 LogicalScreenDescriptor 与 BITMAPHEADER40 两式、"
        "TIFF 的 IFD0、WMF 的 Header）",
        [tally([one_row["word"] for _, one_row in pb_rows]),
         tally([one_row["word_name"] for _, one_row in pb_rows]),
         tally([one_row["ext"] for _, one_row in pb_rows]),
         tally([one_row["ext_name"] for _, one_row in pb_rows]),
         tally([one_row["sig"] for _, one_row in pb_rows]),
         tally([one_row["how"] for _, one_row in pb_rows]),
         [(n, one_row["where"], one_row["ext"], one_row["sig"]) for n, one_row in pb_rows
          if one_row["ext"] and one_row["ext"] != one_row["sig"]]],
        [{'image/png': 40,
          None: 10,
          'image/jpeg': 6,
          'image/gif': 5,
          'image/bmp': 3,
          'image/tiff': 4,
          'pngblip': 9,
          'jpegblip': 2,
          'wmetafile': 2},
         {'png': 49, None: 10, 'jpeg': 8, 'gif': 5, 'bmp': 3, 'tiff': 4, 'wmf': 2},
         {'png': 43, None: 20, 'jpeg': 2, 'gif': 5, 'bmp': 3, 'tif': 4, 'jpg': 4},
         {'png': 43, None: 20, 'jpeg': 6, 'gif': 5, 'bmp': 3, 'tiff': 4},
         {'png': 52, 'svm': 6, 'jpeg': 8, 'gif': 5, 'bmp': 3, 'tiff': 4, 'wmf': 2, None: 1},
         {'IHDR': 52,
          None: 7,
          'SOF0': 8,
          'LogicalScreenDescriptor': 5,
          'BITMAPHEADER40': 3,
          'IFD0': 4,
          'Header': 2},
         [('images-dpi-lo.docx', 'word/media/image6.tif', 'tif', 'tiff'),
          ('images-dpi.docx', 'word/media/image3.jpg', 'jpg', 'jpeg'),
          ('images-dpi.docx', 'word/media/image4.jpg', 'jpg', 'jpeg'),
          ('images-dpi.docx', 'word/media/image7.tif', 'tif', 'tiff'),
          ('images-dpi.docx', 'word/media/image8.tif', 'tif', 'tiff'),
          ('images-dpi.odt', 'Pictures/100000000000003C0000001EF1C6D9F6.jpg', 'jpg', 'jpeg'),
          ('images-dpi.odt', 'Pictures/10000000000000300000001498F99A9F.jpg', 'jpg', 'jpeg'),
          ('images-dpi.odt', 'Pictures/100000010000002C000000166BE96C63.tif', 'tif', 'tiff')]],
    )
    check(
        "`head_hex` 是头八个字节本身，它与 `sig` 一一对得上：png 42 份同一个值、jpeg 8 份同一个 JFIF 前缀，"
        "SVM 六条 `56434c4d54460100`（`VCLMTF\\x01\\x00`），两张 WMF 各是 `010009000003120d` 与 "
        "`0100090000035a13`（同一族记录头、后面跟的字节不同）—— 只有 `tbox.docx` 那一条是空串。"
        "`detected` 那一本按族摊开：word 74 份 png 21 / jpeg 4 / gif 3 / tiff 3 / bmp 2 加一枚「(没读到)」，"
        "ODF 42 份 png 12 / svm 6 / gif 2 / jpeg 2 / bmp 1 / tiff 1，"
        "RTF 18 份只有 png 9 / jpeg 2 / wmf 2 —— LO 存 .rtf 时把 gif / bmp / tif 全转成了 png 或 wmf，"
        "于是那三种名字在这一族的账里一次都不出现",
        [tally([one_row["head_hex"] for _, one_row in pb_rows]),
         [tally([dn for one in pool.values() for dn, cnt in one["detected"].items()
                 for _ in range(cnt)]) for pool in (pb_dx, pb_od, pb_rt)]],
        [{'89504e470d0a1a0a': 52,
          '56434c4d54460100': 6,
          'ffd8ffe000104a46': 8,
          '4749463837612000': 5,
          '424d360600000000': 3,
          '49492a0008000000': 4,
          '010009000003120d': 1,
          '0100090000035a13': 1,
          '': 1},
         [{'png': 28, 'jpeg': 4, 'gif': 3, 'bmp': 2, 'tiff': 3, '(没读到)': 1},
          {'svm': 6, 'png': 15, 'jpeg': 2, 'gif': 2, 'bmp': 1, 'tiff': 1},
          {'png': 9, 'jpeg': 2, 'wmf': 2}]],
    )
    check(
        "两种「只有合成件才给」的形状逐行交出来：`eq.odt` 里那六条嵌入公式的替位图是 SVM，"
        "既没有 `draw:mime-type` 也没有扩展名，所以 `word` / `ext` 两本都交 null 而 `sig` 有答案，"
        "像素与自然尺寸也全 null（SVM 的头里只有一个魔数，没有尺寸格）；"
        "`images-dpi.rtf` 那两条是**普通** WMF —— 头里只有记录长度（`how` 交 `Header`），"
        "可它自己声明了 `\\picw` 32×16 与 44×22，于是 `declared_pixels` 有值而 `pixels` 没有、"
        "`px_agrees` 判不了，密度那一格落在 `none`（WMF 压根没有这一格）",
        [[(n, one_row["where"], one_row["how"], one_row["pixels"], one_row["declared_pixels"])
          for n, one_row in pb_rows if one_row["sig"] == "svm"],
         [(n, one_row["where"], one_row["how"], one_row["pixels"], one_row["declared_pixels"],
           one_row["px_agrees"], one_row["density"]["state"])
          for n, one_row in pb_rows if one_row["sig"] == "wmf"]],
        [[("eq.odt", "ObjectReplacements/Object 1", None, {"w": None, "h": None}, None),
          ("eq.odt", "ObjectReplacements/Object 2", None, {"w": None, "h": None}, None),
          ("eq.odt", "ObjectReplacements/Object 3", None, {"w": None, "h": None}, None),
          ("eq.odt", "ObjectReplacements/Object 4", None, {"w": None, "h": None}, None),
          ("eq.odt", "ObjectReplacements/Object 5", None, {"w": None, "h": None}, None),
          ("eq.odt", "ObjectReplacements/Object 6", None, {"w": None, "h": None}, None)],
         [("images-dpi.rtf", "pict#7", "Header", {"w": None, "h": None}, {"w": 32, "h": 16},
           None, "none"),
          ("images-dpi.rtf", "pict#8", "Header", {"w": None, "h": None}, {"w": 44, "h": 22},
           None, "none")]],
    )
    check(
        "三本尺寸各数各的：`pixels`（图自己头里的像素）62 条知道、9 条不知道（六条 SVM、两条 WMF "
        "加没字节那条）；`natural`（拿自带密度乘回去的自然尺寸）只有 24 条算得出；"
        "`placed`（页面上占的那块）71 条全有 —— 有框就有摆放。由这三本派生的两问：`stretched` "
        "长宽比变了 4 条、`at_natural` 恰好按原尺寸摆的 4 条，两批**没有一行重合**"
        "（`images-dpi` 那一份件的三家出口各一条，长宽比差 583‰ / 583‰ / 583‰ / 582‰，"
        "判据是 1‰ 的零头而不是相等 —— 生产者在最后一位上就不一致；原尺寸那四条的偏差恒 2‰）。"
        "看着像重合的两处各在 `images-dpi-lo.docx` 与 `images-dpi.odt`：LibreOffice 把两张图并成"
        "同一个部件名，被拉大的那一张与按原尺寸摆的那一张各顶一行 —— 「同一份字节」与"
        "「同一行」是两问；判不了的分别 9 条与 47 条",
        [[pb_sum("pixels", "known"), pb_sum("natural", "known"), pb_sum("placed", "known")],
         [pb_sum("pixels", "unknown"), pb_sum("natural", "unknown"), pb_sum("placed", "unknown")],
         [pb_sum("stretched", f) for f in ("yes", "no", "undecided")],
         [pb_sum("at_natural", f) for f in ("yes", "no", "undecided")],
         [(n, one_row["where"], one_row["aspect_permille"]) for n, one_row in pb_rows
          if one_row["stretched"] is True],
         [(n, one_row["where"], one_row["scale_permille"], one_row["aspect_permille"])
          for n, one_row in pb_rows if one_row["at_natural"] is True]],
        [[[40, 21, 11], [12, 6, 6], [39, 27, 13]],
         [[1, 6, 2], [29, 21, 7], [2, 0, 0]],
         [[4, 4, 1], [34, 17, 10], [3, 6, 2]],
         [[2, 1, 1], [10, 5, 5], [29, 21, 7]],
         [('images-dpi-lo.docx', 'word/media/image1.png', 583),
          ('images-dpi.docx', 'word/media/image2.png', 583),
          ('images-dpi.odt', 'Pictures/100000000000002800000018CB9DEC0B.png', 583),
          ('images-dpi.rtf', 'pict#2', 582),
          ('wrap-lo.docx', 'word/media/image1.png', 333),
          ('wrap-lo.docx', 'word/media/image1.png', 333),
          ('wrap.odt', 'Pictures/dot.png', 333),
          ('wrap.odt', 'Pictures/dot.png', 333),
          ('wrap.odt', 'Pictures/dot.png', 333)],
         [('images-dpi-lo.docx', 'word/media/image1.png', {'w': 1000, 'h': 1000}, 2),
          ('images-dpi.docx', 'word/media/image1.png', {'w': 1000, 'h': 1000}, 2),
          ('images-dpi.odt',
           'Pictures/100000000000002800000018CB9DEC0B.png',
           {'w': 1000, 'h': 1000},
           2),
          ('images-dpi.rtf', 'pict#0', {'w': 1000, 'h': 1000}, 2)]],
    )
    check(
        "RTF 独家那一格：`\\picw` / `\\pich` 是文件**自己声明**的像素数，13 行全写了、11 行与头里认出来的"
        "像素数一致、2 行判不了（那两张 WMF 头里没有像素），`disagrees` 恒 0 —— 而这一格在 OOXML 与 ODF "
        "两族根本没有可写的地方，所以那两族 58 行交 null 且计数全落在 `undecided`。"
        "`px_agrees` 因此只在 RTF 上有 true，另两族整本 null —— 缺这一问的族交 null，不交 false",
        [[pb_sum("declared_pixels", f)
          for f in ("written", "agrees", "disagrees", "undecided")],
         {k: tally([one_row["px_agrees"] for n, one_row in pb_rows
                    if pb_all[n]["family"] == k]) for k in ("docx", "odf", "rtf")},
         [sum(1 for _, one_row in pb_rows if one_row["declared_pixels"] is not None)]],
        [[[0, 0, 13], [0, 0, 11], [0, 0, 0], [41, 27, 2]],
         {'docx': {None: 41}, 'odf': {None: 27}, 'rtf': {True: 11, None: 2}},
         [13]],
    )
    check(
        "「页面占的那块」的来路三族三条：OOXML 从 `wp:extent` 的 EMU、ODF 从 "
        "`draw:frame/@svg:width` 的自带单位、RTF 从 `\\picwgoal × \\picscalex` —— 同一张 300 dpi 的 "
        "40×24 PNG，python-docx 写 `121920` EMU 而 LibreOffice 写 `0.339cm`、RTF 写 192 twips × 100%，"
        "换到 0.01mm 三边都是 339×203 而**串长得不一样**，所以 `from` 与 `written` 两样都交、不合并。"
        "`note` 那一格在整库 71 行里恒 null：这一族的含糊全有格子可放，不需要旁白",
        [tally([one_row["placed_mm100"]["from"] for _, one_row in pb_rows]),
         [(n, one_row["where"], one_row["note"]) for n, one_row in pb_rows if one_row["note"]]],
        [{'wp:extent': 41, 'draw:frame/@svg:width': 27, '\\picwgoal×\\picscalex': 13}, []],
    )
    x_pic = dig(lbin("office-doc", fixture("images-dpi.docx")), "structure.picture_bytes")
    pb_cut = dig(lbin("office-doc", fixture("images-dpi.docx"), "--limit", "1"),
                 "structure.picture_bytes")
    check(
        "截行不截账：`--limit 1` 只砍 `rows`（`listed` 1、`cut` true，交出来的那一行还是那张 40×24 的 PNG），"
        "九本计数仍按全部 9 行算 —— `addr` 9 条、`agrees` 九个 yes、`density` 四种各就各位、"
        "`pixels` / `placed` 九个 known、`natural` 六比三、`declared_pixels` 九个 undecided、"
        "`detected` 五种合起来九枚、`distinct_parts` 仍是 8（九个框指向八个部件）",
        [pb_cut["total"], pb_cut["listed"], pb_cut["cut"], len(pb_cut["rows"]),
         pb_cut["rows"][0]["where"], pb_cut["addr"], pb_cut["agrees"], pb_cut["density"],
         pb_cut["pixels"], pb_cut["natural"], pb_cut["placed"], pb_cut["declared_pixels"],
         pb_cut["detected"], pb_cut["distinct_parts"],
         [one["where"] for one in x_pic["rows"][:3]]],
        [9, 1, True, 1, "word/media/image1.png",
         {"read": 9, "unresolved": 0, "missing": 0, "none": 0},
         {"yes": 9, "no": 0, "undecided": 0},
         {"read": 6, "unitless": 1, "absent": 1, "none": 1},
         {"known": 9, "unknown": 0}, {"known": 6, "unknown": 3}, {"known": 9, "unknown": 0},
         {"written": 0, "agrees": 0, "disagrees": 0, "undecided": 9},
         {"png": 3, "jpeg": 2, "gif": 1, "bmp": 1, "tiff": 2}, 8,
         ["word/media/image1.png", "word/media/image1.png", "word/media/image2.png"]],
    )
    check(
        "反面凭据：这一格只在 office-doc 交，三族都有出口（RTF 那 18 份也交）。"
        "遗留 .doc 的图住在 piece 流里，本机没有第二个读者可核对，所以那一家连「零条」都不报；"
        ".ods / .odp 的图在 office-sheet / office-slide 各有各的账，在 office-doc 没有出口 —— "
        "把它们算进这一族只会替文件编一本假账，缺键 = 这一族没这一层",
        [no_theme_key("office-doc", "images-dpi.rtf", "picture_bytes"),
         no_theme_key("office-doc", "notes-en.doc", "picture_bytes"),
         no_theme_key("office-sheet", "book.ods", "picture_bytes"),
         no_theme_key("office-slide", "deck.odp", "picture_bytes")],
        [True, False, False, False],
    )

    # ── 3bq) 这一页的底色：pptx 写在页自己身上，odp 写在页点名的那份样式里，两边都能不写 ──
    print("=== 3bq) office-slide page_background：三族一份账，「写了 noFill」与「什么都没写」 ===")

    def bg_pages_multiset(rows):
        return sorted(json.dumps(one.get("page_background"), sort_keys=True, ensure_ascii=False)
                      for one in rows)

    for name in sorted(one.name for one in FIXTURES.glob("*.pptx")):
        got = lbin("office-slide", fixture(name))
        want = files[name]["ooxml"]["slides"]
        check("%s 每页底色记录合起来与读者一致（多重集，不比页序）" % name,
              bg_pages_multiset(got.get("slides", [])), bg_pages_multiset(want))
        check("%s 整册那本底色账（六类部件逐件）与读者一致" % name,
              dig(got, "backgrounds"), files[name]["ooxml"]["backgrounds"])
    for name in sorted(one.name for one in FIXTURES.glob("*.odp")):
        got = lbin("office-slide", fixture(name))
        want = files[name].get("odp", {})
        check("%s 每页底色（跳一跳在样式里）与读者一致（多重集，不比页序）" % name,
              bg_pages_multiset(got.get("slides", [])), bg_pages_multiset(want.get("slides", [])))
        check("%s 整册那份样式账（几页共用一份 dp）与读者一致" % name,
              dig(got, "backgrounds"), want.get("backgrounds"))
    bg = lbin("office-slide", fixture("deck-bg.pptx"))
    check(
        "`deck-bg.pptx` 第 1 页：python-pptx 把实色写在**页自己身上** —— `p:bg` → `p:bgPr` → "
        "`a:solidFill`，色是字面 `1A1A2E`（没有修饰），而它每条底色后面都跟一枚**空壳** "
        "`<a:effectLst/>`：于是「这枚元素在不在」是 true、「它肚子里有几个孩子」是 0，两个数各说各的",
        [dig(bg, "slides[0].page_background.written"),
         dig(bg, "slides[0].page_background.via"),
         dig(bg, "slides[0].page_background.fill"),
         dig(bg, "slides[0].page_background.fill_element"),
         dig(bg, "slides[0].page_background.colors[0]"),
         dig(bg, "slides[0].page_background.effect_lst_written"),
         dig(bg, "slides[0].page_background.effects")],
        [True, "bgPr", "solid", "solidFill",
         {"element": "srgbClr", "val": "1A1A2E", "modifiers": {},
          "modifier_elements": []}, True, 0],
    )
    check(
        "同一本里第 2、4 页是这一族最难的一对：第 2 页写了整枚 `<p:bg><p:bgPr><a:noFill/>`"
        "（`fill` 短名 `none`、`colors` 是空表），第 4 页**一枚 `p:bg` 都没有** —— "
        "「显式不填充」与「什么都不写、去看母版」是两件事，所以后者的每一格是 null 而不是 false / 0："
        "`written` false、`colors` null、`effect_lst_written` null",
        [dig(bg, "slides[1].page_background.fill"),
         dig(bg, "slides[1].page_background.colors"),
         dig(bg, "slides[1].page_background.stops"),
         dig(bg, "slides[3].page_background.written"),
         dig(bg, "slides[3].page_background.fill"),
         dig(bg, "slides[3].page_background.colors"),
         dig(bg, "slides[3].page_background.effect_lst_written")],
        ["none", [], 0, False, None, None, None],
    )
    check(
        "第 3 页那份渐变按文件写的交：两站各点一次主题色 `accent1`，站号是万分比原串 "
        "`0` / `100000`，方向元素自己带 `@scaled` —— 解成什么色、朝哪边都不替文件算。"
        "而这一族的修饰**不是属性、是孩子元素**（`a:tint` / `a:shade` / `a:satMod`），"
        "所以颜色那一行交四格：`modifiers` 是空表、`modifier_elements` 才是那三句；"
        "填充元素自己的属性（`rotWithShape`）与它的直接孩子照样整份交出来",
        [dig(bg, "slides[2].page_background.fill"),
         dig(bg, "slides[2].page_background.stop_positions"),
         dig(bg, "slides[2].page_background.color_names"),
         [one["val"] for one in dig(bg, "slides[2].page_background.colors")],
         [one["modifiers"] for one in dig(bg, "slides[2].page_background.colors")],
         [one["modifier_elements"] for one in dig(bg, "slides[2].page_background.colors")],
         dig(bg, "slides[2].page_background.fill_attrs"),
         dig(bg, "slides[2].page_background.fill_children")],
        ["gradient", ["0", "100000"], ["schemeClr"], ["accent1", "accent1"], [{}, {}],
         [[{"element": "tint", "attrs": {"val": "100000"}},
           {"element": "shade", "attrs": {"val": "100000"}},
           {"element": "satMod", "attrs": {"val": "130000"}}],
          [{"element": "tint", "attrs": {"val": "50000"}},
           {"element": "shade", "attrs": {"val": "100000"}},
           {"element": "satMod", "attrs": {"val": "350000"}}]],
         {"rotWithShape": "1"},
         [{"element": "gsLst", "attrs": {}}, {"element": "lin", "attrs": {"scaled": "0"}}]],
    )
    check(
        "整册那本账看得见页级记录看不见的事：这一本只有 4 件写过底色，其中一件是**母版** —— "
        "它走的是另一条路 `p:bgRef @idx=\"1001\"`，肚子里没有填充族（`fill` 是 null 而不是 "
        "\"没有填充\"），色直接挂在 `bgRef` 下面点 `bg1`。11 份版式一枚都不写",
        [dig(bg, "backgrounds.parts_scanned"),
         dig(bg, "backgrounds.parts_with_bg"),
         dig(bg, "backgrounds.layers"),
         dig(bg, "backgrounds.fills_seen"),
         dig(bg, "backgrounds.entries[0].layer"),
         dig(bg, "backgrounds.entries[0].via"),
         dig(bg, "backgrounds.entries[0].idx"),
         dig(bg, "backgrounds.entries[0].fill"),
         dig(bg, "backgrounds.entries[0].colors")],
        [16, 4, [{"layer": "master", "parts": 1}, {"layer": "slide", "parts": 3}],
         ["solid", "none", "gradient"], "master", "bgRef", "1001", None,
         [{"element": "schemeClr", "val": "bg1", "modifiers": {},
           "modifier_elements": []}]],
    )
    bglo = lbin("office-slide", fixture("deck-bg-lo.pptx"))
    check(
        "LibreOffice 重写同一份稿子，三处搬家：① 第 2 页那枚**显式 `noFill` 整条丢掉**，"
        "于是「写了不填充」与「什么都没写」在两份件里都不可分辨（方向还相反）；"
        "② 主题色它**替文件算完了** —— 两个 `srgbClr` 字面值 `3E7FCC` / `A4C1FF`，"
        "python-pptx 那三枚修饰（`tint` / `shade` / `satMod`）一个字不写；"
        "③ 同一枚渐变方向元素点的属性名都不一样（`@scaled` → `@ang`），空壳 `effectLst` 一枚也不写",
        [dig(bglo, "slides[1].page_background.written"),
         dig(bglo, "slides[3].page_background.written"),
         [one["element"] for one in dig(bglo, "slides[2].page_background.colors")],
         [one["val"] for one in dig(bglo, "slides[2].page_background.colors")],
         dig(bglo, "slides[2].page_background.colors[0].modifiers"),
         [one["modifier_elements"] for one in dig(bglo, "slides[2].page_background.colors")],
         dig(bglo, "slides[2].page_background.fill_children[1]"),
         dig(bglo, "slides[2].page_background.effect_lst_written"),
         dig(bglo, "slides[2].page_background.effects")],
        [False, False, ["srgbClr", "srgbClr"], ["3E7FCC", "A4C1FF"], {}, [[], []],
         {"element": "lin", "attrs": {"ang": "0"}}, False, None],
    )
    check(
        "同一本账里那次「层与层之间搬家」：母版那枚 `bgRef` 没了，LibreOffice 把它摊到 "
        "**11 份版式**上写成字面 `FFFFFF` —— 只看页部件会读成「底色丢了」，而页上写的两句还在。"
        "`parts_with_bg` 因此从 4 涨到 13，`layers` 从「母版 1 + 页 3」变成「版式 11 + 页 2」",
        [dig(bglo, "backgrounds.parts_scanned"),
         dig(bglo, "backgrounds.parts_with_bg"),
         dig(bglo, "backgrounds.layers"),
         dig(bglo, "backgrounds.fills_seen"),
         dig(bglo, "backgrounds.entries[0].layer"),
         dig(bglo, "backgrounds.entries[0].part"),
         dig(bglo, "backgrounds.entries[0].colors[0].val")],
        [16, 13, [{"layer": "layout", "parts": 11}, {"layer": "slide", "parts": 2}],
         ["solid", "gradient"], "layout", "ppt/slideLayouts/slideLayout1.xml", "FFFFFF"],
    )
    bgodp = lbin("office-slide", fixture("deck-bg.odp"))
    check(
        "同一份东西转成 odp，这句话换了地方：页只点一个样式名，底色在那份 `drawing-page` 样式里 —— "
        "第 1 页 `dp1` 写 `draw:fill=\"solid\"` + `draw:fill-color=\"#1a1a2e\"`，"
        "而 `props_written` 7 是「那份样式一共说了几句话」（`display-footer` 那些也算），"
        "底色那一堆只是子集，两个数不互相解释",
        [dig(bgodp, "slides[0].page_background.page_style"),
         dig(bgodp, "slides[0].page_background.style_found"),
         dig(bgodp, "slides[0].page_background.style_part"),
         dig(bgodp, "slides[0].page_background.written"),
         dig(bgodp, "slides[0].page_background.fill"),
         dig(bgodp, "slides[0].page_background.fill_attrs"),
         dig(bgodp, "slides[0].page_background.props_written"),
         dig(bgodp, "slides[0].page_background.background_attrs")],
        ["dp1", True, "content.xml", True, "solid",
         {"fill": "solid", "fill-color": "#1a1a2e"}, 7,
         {"background-objects-visible": "true", "background-visible": "true"}],
    )
    check(
        "这一族里「不填充」与「没写」也不可分辨，但露出来的地方不同：第 2、4 页**共用同一份 `dp3**`，"
        "那份样式的属性表里一条 `draw:fill` 都没有（`fill_written` false、`written` false、"
        "`props_written` 5 —— 那五句全是页脚/背景可见性），所以 `written` 是 false 而不是 null："
        "样式读到了，它没说。真正管色的是继承那一跳 —— 母版页 `Blank` → `Mdp1` → `solid #ffffff`",
        [dig(bgodp, "slides[1].page_background.page_style"),
         dig(bgodp, "slides[1].page_background.written"),
         dig(bgodp, "slides[1].page_background.fill"),
         dig(bgodp, "slides[1].page_background.fill_attrs"),
         dig(bgodp, "slides[1].page_background.props_written"),
         dig(bgodp, "slides[3].page_background.page_style"),
         dig(bgodp, "slides[1].page_background.inherited")],
        ["dp3", False, None, {}, 5, "dp3",
         {"master": "Blank", "master_style": "Mdp1", "found": True, "part": "styles.xml",
          "fill": "solid",
          "fill_attrs": {"fill": "solid", "fill-color": "#ffffff"},
          "background_attrs": {"background-size": "border"}}],
    )
    check(
        "渐变那一跳要两跳才解得开：样式里只有一个名字 `msFillGradient_20_1`，定义住在 "
        "styles.xml 的 `office:styles` 里，而元素名是 `draw:gradient`（不是 `style:gradient`）。"
        "解开了交那份定义自己写的属性（`style=\"linear\"` `angle=\"90deg\"` 两端色），"
        "解不开时 `found` 是 false —— 两边都是两串字面值，与 pptx 那两份的 `3E7FCC` / `A4C1FF` 对得上",
        [dig(bgodp, "slides[2].page_background.fill"),
         dig(bgodp, "slides[2].page_background.fill_attrs"),
         dig(bgodp, "slides[2].page_background.gradient.name"),
         dig(bgodp, "slides[2].page_background.gradient.found"),
         dig(bgodp, "slides[2].page_background.gradient.part"),
         dig(bgodp, "slides[2].page_background.gradient.element"),
         [dig(bgodp, "slides[2].page_background.gradient.attrs." + one)
          for one in ("style", "start-color", "end-color", "angle")]],
        ["gradient", {"fill": "gradient", "fill-gradient-name": "msFillGradient_20_1"},
         "msFillGradient_20_1", True, "styles.xml", "gradient",
         ["linear", "#3e7fcc", "#a4c1ff", "90deg"]],
    )
    check(
        "整册那份样式账在 ODF 顶的是「这一格被几页共用」：四页全点了样式名（`pages_unnamed` 0）、"
        "三份样式各 1 / 2 / 1 页，其中一份被两页共用、一份读了却说不了底色 —— "
        "「一份样式挂几页」与「几份样式沉默」是两个数，光看每页那份记录拼不出来",
        [dig(bgodp, "backgrounds.pages"),
         dig(bgodp, "backgrounds.pages_unnamed"),
         dig(bgodp, "backgrounds.shared_styles"),
         dig(bgodp, "backgrounds.silent_styles"),
         dig(bgodp, "backgrounds.unfound_styles"),
         [[one["style"], one["pages"], one["fill_written"]]
          for one in dig(bgodp, "backgrounds.styles")]],
        [4, 0, 1, 1, 0,
         [["dp1", 1, True], ["dp3", 2, False], ["dp4", 1, True]]],
    )
    check(
        "两族之外没有这一层：遗留 .ppt 的记录树里没有页底这一族（不交这个键，"
        "与形状树那一条同一规矩），docx / odt 也不交 —— 本机 32 份真件与 105 份 fixture 的 "
        "`word/document.xml` 里 `w:background` 与 `w:displayBackgroundShape` 各 0 处、"
        "python-docx 1.2.0 没有这个口，那是生产者做不出而不是读不出来",
        [dig(lbin("office-slide", fixture("deck.ppt")), "slides[0].page_background"),
         dig(lbin("office-doc", fixture("notes.docx")), "structure.page_background"),
         dig(lbin("office-doc", fixture("notes.odt")), "structure.page_background"),
         dig(lbin("office-sheet", fixture("book.xlsx")), "backgrounds")],
        [None, None, None, None],
    )

    # ── 3br) 包里那几份自定义 XML 存储：件、那一跳到 itemProps、正文那两条手指 ──
    print("=== 3br) custom_xml：`customXml/itemN.xml` 那一族部件一本账（三份件 + 逐件对）===")

    for name in sorted(one.name for one in list(FIXTURES.glob("*.docx"))
                       + list(FIXTURES.glob("*.pptx")) + list(FIXTURES.glob("*.xlsx"))):
        cmd = "office-slide" if name.endswith(".pptx") else ("office-sheet" if name.endswith(".xlsx") else "office-doc")
        got = lbin(cmd, fixture(name))
        # office-doc 的那本账挂在 `structure` 下面，另两家是平的一层
        pre = "structure." if cmd == "office-doc" else ""
        want = (files[name].get("ooxml") or {}).get("custom_xml")
        check("%s 那一族存储的整本账与读者一致（含逐件那一列）" % name, dig(got, pre + "custom_xml"), want)

    cx = lbin("office-doc", fixture("customxml.docx"))
    check(
        "`customxml.docx` 一份包两枚存储：每一件都靠**自己的 `.rels`** 跳到那一份 `itemPropsN.xml`，"
        "再从那一份里读出 `ds:itemID` 与 `ds:schemaRefs/ds:schemaRef/@ds:uri` —— 第二枚那一份 "
        "`schemaRefs` 在而里面空（真件 25 份里正有一份就是这个形状），所以它的 uri 是空表而**不是**「读不出」",
        [dig(cx, "structure.custom_xml.items"), dig(cx, "structure.custom_xml.items_empty"), dig(cx, "structure.custom_xml.overrides"),
         dig(cx, "structure.custom_xml.default_for_xml"),
         [[one["part"], one["root"], one["children"], one["props_rel"], one["props_found"],
           len(one["schema_uris"]), one["bound"], one["declared"]]
          for one in dig(cx, "structure.custom_xml.entries")]],
        [2, 0, 2, True,
         [["customXml/item1.xml", "Sources", 3, "itemProps1.xml", True, 1, 1, False],
          ["customXml/item2.xml", "customData", 1, "itemProps2.xml", True, 0, 0, False]]],
    )
    check(
        "`itemN.xml` 自己**不在** `[Content_Types].xml` 上点名（只有 props 那一份点），它靠 "
        "`Default Extension=\"xml\"` 兜着 —— 所以每行 `declared` 是 false 而整本那格 "
        "`default_for_xml` 是 true，这两格合起来才是「这一族部件怎么被包承认」的答案。"
        "正文那两条手指各一条：`w:customXml` 圈字、`w:sdt` 上 `w:dataBinding/@w:storeItemID` "
        "指着第一枚的号，所以 `bound` 那列是 1 / 0 而 `unresolved_bindings` 是 0",
        [dig(cx, "structure.custom_xml.anchors_custom_xml"), dig(cx, "structure.custom_xml.anchors_data_binding"),
         dig(cx, "structure.custom_xml.binding_ids"), dig(cx, "structure.custom_xml.unresolved_bindings")],
        [1, 1, ["{1B2C3D4E-5F60-4142-8384-858687888990}"], 0],
    )
    cxb = lbin("office-doc", fixture("customxml-lo.docx"))
    check(
        "同一份过一遍 LibreOffice：三枚 `itemN.xml` 全被**清空成 0 字节**（部件在、名字在、"
        "那一跳也在，可里面没字了 —— 所以 `root` / `children` 两格是 null，与「部件不在」是两件事），"
        "而 props 从两份变**三份**（它给正文那条绑定另写了一份存储）",
        [dig(cxb, "structure.custom_xml.items"), dig(cxb, "structure.custom_xml.items_empty"), dig(cxb, "structure.custom_xml.parts_total"),
         dig(cxb, "structure.custom_xml.overrides"),
         [[one["part"], one["size"], one["root"], one["props_rel"], one["item_id"]]
          for one in dig(cxb, "structure.custom_xml.entries")]],
        [3, 3, 9, 3,
         [["customXml/item1.xml", 0, None, "itemProps1.xml", "{1B2C3D4E-5F60-4142-8384-858687888990}"],
          ["customXml/item2.xml", 0, None, "itemProps2.xml", "{1B2C3D4E-5F60-4142-8384-858687888990}"],
          ["customXml/item3.xml", 0, None, "itemProps3.xml", "{0A0B0C0D-0E0F-4041-9293-949596979899}"]]],
    )
    check(
        "重写之后那一条手指**说不清指到哪一份**了：`itemProps1` 与 `itemProps2` 里 "
        "`ds:itemID` 是同一个号，于是两行的 `bound` 都是 1 —— 判不住就如实交两个 1，"
        "不替文件挑一份。另一条手指它干脆不认：`w:customXml` 整条丢了（1 → 0），"
        "`w:dataBinding` 照样留着（还自己补一枚 `<w:text/>`），同一族里两条手指两种待遇",
        [dig(cxb, "structure.custom_xml.anchors_custom_xml"), dig(cxb, "structure.custom_xml.anchors_data_binding"),
         [one["bound"] for one in dig(cxb, "structure.custom_xml.entries")],
         dig(cxb, "structure.custom_xml.unresolved_bindings")],
        [0, 1, [1, 1, 0], 0],
    )
    check(
        "`notes.docx` 那一份**不是加上去的**：python-docx 的打底模板本来就带一枚 `b:Sources`"
        "（0 条孩子、props 里一个 GUID 加一条 bibliography 的 uri），本机 25 份真件 docx 里那一份"
        "与它同形状 —— 这一族在真件里就是「存储躺在包里、正文一条手指都不写」（25/25 份两条手指各 0 处）",
        [dig(lbin("office-doc", fixture("notes.docx")), "structure.custom_xml.items"),
         [one["root"] for one in dig(lbin("office-doc", fixture("notes.docx")), "structure.custom_xml.entries")],
         [one["children"] for one in dig(lbin("office-doc", fixture("notes.docx")), "structure.custom_xml.entries")],
         dig(lbin("office-doc", fixture("notes.docx")), "structure.custom_xml.anchors_data_binding")],
        [1, ["Sources"], [0], 0],
    )
    check(
        "没有那一族的包交一本空账而不是缺键：表格件 `book.xlsx` 里 `customXml/` 0 件，"
        "而 `default_for_xml` 仍是 true（那条 Default 每个 OPC 包都写）—— 「没有存储」与"
        "「包没承认过 xml 这一族」是两问。ODF 那一族不交这个键（没有 OPC 包，这一层不存在）",
        [dig(lbin("office-sheet", fixture("book.xlsx")), "custom_xml.items"),
         dig(lbin("office-sheet", fixture("book.xlsx")), "custom_xml.parts_total"),
         dig(lbin("office-sheet", fixture("book.xlsx")), "custom_xml.default_for_xml"),
         dig(lbin("office-doc", fixture("notes.odt")), "structure.custom_xml"),
         dig(lbin("office-slide", fixture("deck.odp")), "custom_xml")],
        [0, 0, True, None, None],
    )

    # ── 3bs) mc:AlternateContent：同一件事写两遍，两遍各写了什么 ──
    print("=== 3bs) alternate_content：块 / Choice / Fallback / 孤儿块，四本数各数各的 ===")

    for name in sorted(one.name for one in list(FIXTURES.glob("*.docx"))
                       + list(FIXTURES.glob("*.pptx")) + list(FIXTURES.glob("*.xlsx"))):
        cmd = "office-slide" if name.endswith(".pptx") else ("office-sheet" if name.endswith(".xlsx") else "office-doc")
        got = lbin(cmd, fixture(name))
        pre = "structure." if cmd == "office-doc" else ""
        want = (files[name].get("ooxml") or {}).get("alternate_content")
        check("%s 那两遍写法的整本账与读者一致" % name, dig(got, pre + "alternate_content"), want)

    alt = lbin("office-doc", fixture("alternate.docx"))
    check(
        "`alternate.docx` 三种形状一份件里摆开：三块各写了一遍而**第三块写了两条 Choice**"
        "（`blocks` 3 / `choices` 4 —— ECMA 让读者按 `Requires` 挑第一条能认的），但只有两块"
        "配了退路（`fallbacks` 2）—— **「写了两遍」不是这条规矩的常态**，本机真件里 4 块"
        "只有 1 块配了两遍（另 3 块是孤儿）。所以 `orphans`（Choice 在、Fallback 没有的那几块）"
        "单独数一个数，别拿 `blocks - fallbacks` 去减：一块里可以有几条 Choice",
        [dig(alt, "structure.alternate_content.blocks"), dig(alt, "structure.alternate_content.choices"),
         dig(alt, "structure.alternate_content.fallbacks"), dig(alt, "structure.alternate_content.orphans"),
         dig(alt, "structure.alternate_content.parts_scanned"), dig(alt, "structure.alternate_content.requires_prefixes"),
         dig(alt, "structure.alternate_content.requires_counts")],
        [3, 4, 2, 1, 1, ["w14", "wps"], {"w14": 2, "wps": 2}],
    )
    check(
        "同一块里的两条分支各写了什么也交：`requires` 按文档序原样交文件写的前缀名"
        "（不解成 URI —— 前缀是在根元素那些 `xmlns:` 上定义的，这一本不再走那一跳），"
        "而 `choice_elements` 把**每一枚** Choice 的孩子都摊平在这一列里（那第二枚点的是 `w14`）",
        [dig(alt, "structure.alternate_content.entries[0].requires"),
         dig(alt, "structure.alternate_content.entries[0].choice_elements"),
         dig(alt, "structure.alternate_content.entries[0].fallback_elements")],
        [["wps", "w14", "wps", "w14"], ["r", "r", "r", "r"], ["r", "r"]],
    )
    check(
        "LibreOffice 重写同一份：三块**连字一起丢掉**（`blocks` 3 → 0，五句只写在分支里的字"
        "一句都不剩）—— 这一族的存在意义就是「同一个文件对不同读者说不同的话」，"
        "所以那些只写在某一条分支里的话必须被数出来，而不是被当成没写字",
        [dig(lbin("office-doc", fixture("alternate-lo.docx")), "structure.alternate_content.blocks"),
         dig(lbin("office-doc", fixture("alternate-lo.docx")), "structure.alternate_content.choices"),
         dig(lbin("office-doc", fixture("alternate-lo.docx")), "structure.alternate_content.parts_scanned"),
         dig(lbin("office-doc", fixture("alternate-lo.docx")), "structure.alternate_content.requires_prefixes")],
        [0, 0, 0, []],
    )
    check(
        "两个生产者的词汇几乎不重叠，而这一格就是答案的一半：docx 画布那一条点 `wps` 且"
        "**两遍写的是两种东西**（Choice 里 `drawing`、Fallback 里 `pict` —— DrawingML 的图"
        "对 VML 的图），pptx 整册每页点 `p14`，xlsx 的批注点 `v2`",
        [[dig(lbin("office-doc", fixture("tbox.docx")), "structure.alternate_content.requires_prefixes"),
          dig(lbin("office-doc", fixture("tbox.docx")), "structure.alternate_content.entries[0].choice_elements"),
          dig(lbin("office-doc", fixture("tbox.docx")), "structure.alternate_content.entries[0].fallback_elements")],
         dig(lbin("office-slide", fixture("deck-chart-lo.pptx")), "alternate_content.requires_prefixes"),
         dig(lbin("office-sheet", fixture("cell-notes-lo.xlsx")), "alternate_content.requires_prefixes")],
        [[["wps"], ["drawing"], ["pict"]], ["p14"], ["v2"]],
    )
    check(
        "空壳是一句说过的话（这里第三条）：那份 xlsx 的三块都**配了** Fallback 元素"
        "（`fallbacks` 3），而它肚子里一个元素都没写（`fallback_elements` 空表）—— "
        "「这枚分支在不在」与「它写了什么」是两个数。两遍写同一个元素名的也有："
        "pptx 那两份块里 Choice 与 Fallback 都写 `transition`",
        [dig(lbin("office-sheet", fixture("cell-notes-lo.xlsx")), "alternate_content.fallbacks"),
         [one["fallback_elements"] for one in dig(lbin("office-sheet", fixture("cell-notes-lo.xlsx")), "alternate_content.entries")],
         dig(lbin("office-sheet", fixture("cell-notes-lo.xlsx")), "alternate_content.entries[0].choice_elements"),
         [one["choice_elements"] for one in dig(lbin("office-slide", fixture("deck-chart-lo.pptx")), "alternate_content.entries")],
         [one["fallback_elements"] for one in dig(lbin("office-slide", fixture("deck-chart-lo.pptx")), "alternate_content.entries")]],
        [3, [[]], ["commentPr", "commentPr", "commentPr"], [["transition"], ["transition"]],
         [["transition"], ["transition"]]],
    )
    check(
        "没有那一族的包交一本零条的账（`blocks` 0 而 `available` 仍 true），"
        "而 ODF 那一族不交这个键（`mc:AlternateContent` 是 OPC 的东西，ODF 没有这一层）",
        [dig(lbin("office-doc", fixture("notes.docx")), "structure.alternate_content.blocks"),
         dig(lbin("office-doc", fixture("notes.odt")), "structure.alternate_content"),
         dig(lbin("office-slide", fixture("deck.odp")), "alternate_content"),
         dig(lbin("office-doc", fixture("notes.doc")), "structure.alternate_content")],
        [0, None, None, None],
    )

    print("=== 3bt) picture_layout：图是怎么摆的（锚、环绕、层序）——两族两份账 ===")

    for name in sorted(one.name for one in FIXTURES.glob("*.docx")):
        got = lbin("office-doc", fixture(name))
        check("%s 那张图的摆法（OOXML：inline / anchor 与环绕那一支）与读者一致" % name,
              dig(got, "structure.picture_layout"), files[name]["ooxml"]["picture_layout"])
    for name in sorted(one.name for one in FIXTURES.glob("*.odt")):
        got = lbin("office-doc", fixture(name))
        check("%s 那张图的摆法（ODF：框 + 那份 family=graphic 样式，一跳是默认形状）与读者一致" % name,
              dig(got, "structure.picture_layout"), files[name]["odt"]["picture_layout"])

    lay = lbin("office-doc", fixture("wrap.docx"))
    check(
        "`wrap.docx` 三种摆法一份件里摆开：`wp:inline` 那一枚**结构上就没有**环绕那一支"
        "（`wrap_element` 是 null，与「anchor 而文件一个环绕都没写」靠 `kind` 分得开），"
        "两枚 `wp:anchor` 各写一种环绕（`wrapSquare` / `wrapTopAndBottom`）—— 四本数各数各的",
        [dig(lay, "structure.picture_layout.parts_scanned"), dig(lay, "structure.picture_layout.drawings"),
         dig(lay, "structure.picture_layout.inline"), dig(lay, "structure.picture_layout.anchor"),
         dig(lay, "structure.picture_layout.other_kind"), dig(lay, "structure.picture_layout.anchor_without_wrap"),
         dig(lay, "structure.picture_layout.wrap_elements"), dig(lay, "structure.picture_layout.listed"),
         dig(lay, "structure.picture_layout.cut")],
        [1, 3, 1, 2, 0, 0, {"wrapSquare": 1, "wrapTopAndBottom": 1}, 3, False],
    )
    check(
        "浮着才写的那几格：层序号 `@relativeHeight`（合成那两枚写的是 251658240 这种大数）、"
        "压不压字 `@behindDoc`、锁不锁 `@locked`、能不能叠 `@allowOverlap` —— 全按文件自己那个串交，"
        "而「图周围留多少」那四格 EMU 两枚都有（`wp:inline` 那一枚没有，所以它的 `dist` 是 null）",
        [[one["kind"], one["para"], one["wrap_element"], one["behind_doc"], one["locked"],
          one["allow_overlap"], one["relative_height"], one["dist"]]
         for one in dig(lay, "structure.picture_layout.rows")],
        [["inline", 1, None, None, None, None, None, None],
         ["anchor", 2, "wrapSquare", "0", "0", "1", "251658240",
          {"distT": "0", "distB": "114300", "distL": "114300", "distR": "114300"}],
         ["anchor", 3, "wrapTopAndBottom", "1", "1", "0", "251658241",
          {"distT": "0", "distB": "114300", "distL": "114300", "distR": "114300"}]],
    )
    check(
        "位置分横竖两条，而「怎么定」写在孩子上：一枚 `positionH` 可以只点基准"
        "（`@relativeFrom=\"margin\"`）加一枚 `align`，另一枚可以写 `positionOffset` 那个数 —— "
        "`offset_written` 说的是「这一格在不在」，`offset` 才是它写的字。"
        "（LibreOffice 那一份里 `positionV` 的孩子叫 `posOffset` 而不是 `positionOffset`，"
        "于是 `offset_written` false 而 `children` 仍写着那个名字 —— 两家按同一格判，都不补）",
        [[one["kind"], one["position_h"], one["position_v"]]
         for one in dig(lay, "structure.picture_layout.rows")],
        [["inline", {"present": False, "relative_from": None, "align": None, "offset": None,
                     "offset_written": False, "children": []},
          {"present": False, "relative_from": None, "align": None, "offset": None,
           "offset_written": False, "children": []}],
         ["anchor", {"present": True, "relative_from": "margin", "align": None, "offset": None,
                     "offset_written": False, "children": ["align"]},
          {"present": True, "relative_from": "paragraph", "align": None, "offset": None,
           "offset_written": False, "children": ["posOffset"]}],
         ["anchor", {"present": True, "relative_from": "page", "align": None, "offset": "114300",
                     "offset_written": True, "children": ["positionOffset"]},
          {"present": True, "relative_from": "page", "align": None, "offset": "72000",
           "offset_written": True, "children": ["positionOffset"]}]],
    )
    back = lbin("office-doc", fixture("wrap-lo.docx"))
    check(
        "来回一趟丢掉一条、改了一个数（同一个意思的两种写法，谁也没错，两边都按原样交）："
        "三份框导成 docx 只剩两张图（按页锚的那一张整张丢掉，`drawings` 3 → 2），"
        "而留着的那枚 anchor 的 `relativeHeight` 从 `251658240` 变成 `3`，"
        "环绕方式也被它自己挑了一种（ODF 那面写 `parallel`，这面写 `wrapSquare`）",
        [dig(back, "structure.picture_layout.drawings"), dig(back, "structure.picture_layout.inline"),
         dig(back, "structure.picture_layout.anchor"), dig(back, "structure.picture_layout.wrap_elements"),
         dig(back, "structure.picture_layout.rows[1].relative_height"),
         dig(back, "structure.picture_layout.rows[1].wrap_element"),
         dig(back, "structure.picture_layout.rows[0].children")],
        [2, 1, 1, {"wrapSquare": 1}, "3", "wrapSquare",
         ["extent", "effectExtent", "docPr", "cNvGraphicFramePr", "graphic"]],
    )
    odt = lbin("office-doc", fixture("wrap.odt"))
    check(
        "同一问在 ODF 是**框 + 一跳**：框自己只写 `text:anchor-type`（三种锚各一枚），"
        "环绕、穿透、四个边距全在它点名的那份 `family=\"graphic\"` 样式里。"
        "「随字走」那一枚的样式里**根本没有 `style:wrap` 这一格**（`wrap_unwritten` 1）——"
        "「文件没说」与「说了不环绕」（`wrapNone`）是两句话，所以两张表都只数文件写着的那些值",
        [dig(odt, "structure.picture_layout.frames"), dig(odt, "structure.picture_layout.anchor_types"),
         dig(odt, "structure.picture_layout.wrap_values"), dig(odt, "structure.picture_layout.wrap_unwritten"),
         dig(odt, "structure.picture_layout.style_unfound"),
         [[one["anchor_type"], one["style_name"], one["style_found"], one["style_part"],
           one["wrap"], one["wrap_written"], one["props_written"]]
          for one in dig(odt, "structure.picture_layout.rows")]],
        [3, {"as-char": 1, "paragraph": 1, "page": 1}, {"parallel": 1, "through": 1}, 1, 0,
         [["as-char", "frChar", True, "content.xml", None, False, 5],
          ["paragraph", "frPara", True, "content.xml", "parallel", True, 11],
          ["page", "frPage", True, "content.xml", "through", True, 11]]],
    )
    check(
        "那一跳断了不替文件补一个「不环绕」：`notes.odt` 那一枚框的样式里 `wrap` 没写、"
        "四个边距却写着（`margin-left` 是 `0.319cm` —— 单位按文件的串交，不换算成毫米），"
        "而 .pptx / .rtf / 遗留 .doc 这三族**不交这个键**（图住在页自己的形状树里，不是这条规矩）",
        [dig(lbin("office-doc", fixture("notes.odt")), "structure.picture_layout.frames"),
         dig(lbin("office-doc", fixture("notes.odt")), "structure.picture_layout.wrap_unwritten"),
         dig(lbin("office-doc", fixture("notes.odt")), "structure.picture_layout.rows[0].margins"),
         dig(lbin("office-slide", fixture("deck-pictures.pptx")), "picture_layout"),
         dig(lbin("office-doc", fixture("notes.rtf")), "structure.picture_layout"),
         dig(lbin("office-doc", fixture("notes.doc")), "structure.picture_layout")],
        [1, 1, {"margin-top": "0cm", "margin-bottom": "0cm", "margin-left": "0.319cm",
                "margin-right": "0.319cm"}, None, None, None],
    )

    print("=== 3bu) row_heights：这一行多高 —— docx 在行上，ODF 一跳在 table-row 样式里 ===")

    for name in sorted(one.name for one in FIXTURES.glob("*.docx")):
        got = lbin("office-doc", fixture(name))
        check("%s 每行多高（OOXML：trHeight 的数与那条规则）与读者一致" % name,
              dig(got, "structure.row_heights"), files[name]["ooxml"]["row_heights"])
    for name in sorted(one.name for one in FIXTURES.glob("*.odt")):
        got = lbin("office-doc", fixture(name))
        check("%s 每行多高（ODF：那一跳到 table-row 样式，一跳是默认形状）与读者一致" % name,
              dig(got, "structure.row_heights"), files[name]["odt"]["row_heights"])

    rows = lbin("office-doc", fixture("row-height.docx"))
    mine = dig(rows, "structure.row_heights")
    check(
        "`row-height.docx` 一张表四行，一次只改一个变量：exact 一个数 / atLeast 一个数 / "
        "**一个都不写**（这行连 `w:trPr` 都没有）/ 写零并把 `@w:hRule` 摘掉。"
        "三种「没有」在账上是三格：`rows_without_tr_pr` 1、`rules` 里 `(没有 trHeight)` 1 与 "
        "`(没写)` 1 —— 缺 hRule 时本仓不替文件补一个 atLeast（那句是规范说的，不是文件写的）",
        [dig(mine, "parts_scanned"), dig(mine, "tables"), dig(mine, "rows_total"),
         dig(mine, "rows_with_height"), dig(mine, "rows_without_tr_pr"), dig(mine, "zero_height"),
         dig(mine, "rules"), dig(mine, "listed"), dig(mine, "cut")],
        [1, 1, 4, 3, 1, 1,
         {"exact": 1, "atLeast": 1, "(没有 trHeight)": 1, "(没写)": 1}, 4, False],
    )
    check(
        "一行一条交的是「那枚元素在不在」加「它写了哪几个属性」：`@w:val` 按 twips 的串交"
        "（`1361` 与 `680` 是 python-docx 由 2.4cm / 1.2cm 换算出来的，本仓不再倒推），"
        "而第三行那个 `0` 是 Word 里真用的「藏一行」手法 —— 它与「没写高度」不是一回事",
        [[one["row"], one["has_tr_pr"], one["tr_pr_children"], one["height_written"],
          one["val"], one["h_rule"], one["h_rule_written"]]
         for one in dig(mine, "rows")],
        [[0, True, ["trHeight"], True, "1361", "exact", True],
         [1, True, ["trHeight"], True, "680", "atLeast", True],
         [2, False, [], False, None, None, False],
         [3, True, ["trHeight"], True, "0", None, False]],
    )
    back = dig(lbin("office-doc", fixture("row-height-lo.docx")), "structure.row_heights")
    check(
        "来回一趟两处改动：那个零高的行被 LibreOffice 写成 `val=\"1\" hRule=\"atLeast\"`"
        "（它不承认零，`1` 是它自己挑的数），而那个「什么都不写」的行回来时带着**一枚空壳** "
        "`w:trPr` —— `has_tr_pr` true 而 `tr_pr_children` 空表。`rows_without_tr_pr` 因此 1 → 0，"
        "`zero_height` 因此 1 → 0",
        [dig(back, "rows_total"), dig(back, "rows_with_height"), dig(back, "rows_without_tr_pr"),
         dig(back, "zero_height"), dig(back, "rules"),
         [dig(back, "rows[3].val"), dig(back, "rows[3].h_rule"), dig(back, "rows[3].h_rule_written")],
         [dig(back, "rows[2].has_tr_pr"), dig(back, "rows[2].tr_pr_children")]],
        [4, 3, 0, 0, {"exact": 1, "atLeast": 2, "(没有 trHeight)": 1},
         ["1", "atLeast", True], [True, []]],
    )
    odt = dig(lbin("office-doc", fixture("row-height.odt")), "structure.row_heights")
    check(
        "同一问在 ODF 是**行点样式 + 一跳**：四行点四份 `family=\"table-row\"` 的样式，"
        "`style:row-height` 与 `style:min-row-height` 是**两个键**（exact 走前者、atLeast 走后者），"
        "第三行那份样式里两个键都没有而只写着 `keep-together`（`rows_unwritten` 1 而 "
        "`styles_unfound` 0 —— 样式在、这一格不在），第四行写的是 `0cm`（零又是一句说过的话）",
        [dig(odt, "tables"), dig(odt, "rows_total"), dig(odt, "rows_written"),
         dig(odt, "rows_unwritten"), dig(odt, "styles_unfound"), dig(odt, "zero_height"),
         [[one["row"], one["style_name"], one["style_found"], one["row_height"],
           one["min_row_height"], one["written"], one["props_written"]]
          for one in dig(odt, "rows")]],
        [1, 4, 3, 1, 0, 1,
         [[0, "表格1.1", True, "2.401cm", None, True, 2],
          [1, "表格1.2", True, None, "1.199cm", True, 2],
          [2, "表格1.3", True, None, None, False, 1],
          [3, "表格1.4", True, None, "0cm", True, 2]]],
    )
    check(
        "单位换算是有损的，所以两边都只交文件自己那个串：docx 那 `1361` 与 `680` twips 正好是 "
        "2.4cm 与 1.2cm，而 ODF 里写的是 `2.401cm` 与 `1.199cm`（本仓不换算也不比对）；"
        "一张表都没写的件交零条的账（`tables` 0 而 `available` 仍 true），"
        "而 .pptx / .rtf / 遗留 .doc 这三族**不交这个键**",
        [dig(lbin("office-doc", fixture("styled-text.odt")), "structure.row_heights.tables"),
         dig(lbin("office-doc", fixture("notes.docx")), "structure.row_heights.rules"),
         dig(lbin("office-slide", fixture("deck.pptx")), "row_heights"),
         dig(lbin("office-doc", fixture("notes.rtf")), "structure.row_heights"),
         dig(lbin("office-doc", fixture("notes.doc")), "structure.row_heights")],
        [0, {"(没有 trHeight)": 2}, None, None, None],
    )

    # ── 3bv) 这一段前面画什么：pptx 写在段自己的 `a:pPr` 上，odp 写在段点名的那份列表样式里 ──
    print("=== 3bv) slide_bullets：逐段那一本 + 版式/母版与列表样式那一层 ===")

    def bullet_pages_multiset(rows):
        return sorted(json.dumps(one.get("bullets"), sort_keys=True, ensure_ascii=False)
                      for one in rows)

    for name in sorted(one.name for one in FIXTURES.glob("*.pptx")):
        got = lbin("office-slide", fixture(name))
        want = files[name]["ooxml"]["slides"]
        check("%s 每页那本「段前画什么」合起来与读者一致（多重集，不比页序）" % name,
              bullet_pages_multiset(got.get("slides", [])), bullet_pages_multiset(want))
        # 版式与母版那一层的行数是百级的：镜像那本默认 400，CLI 默认 100，
        # 不把两端拉到同一个限额，比的就不是同一本账（`rows` / `listed` / `cut` 都会差）
        check("%s 版式与母版那一层的符号账与读者一致（件名排序，逐条比）" % name,
              dig(lbin("office-slide", fixture(name), "--limit", "400"), "bullet_layers"),
              files[name]["ooxml"]["bullet_layers"],
              diff_paths(dig(lbin("office-slide", fixture(name), "--limit", "400"), "bullet_layers"),
                         files[name]["ooxml"]["bullet_layers"], "bullet_layers"))
    for name in sorted(one.name for one in FIXTURES.glob("*.odp")):
        got = lbin("office-slide", fixture(name))
        want = files[name].get("odp", {})
        check("%s 每页那些列表（点名与解开的样式）与读者一致（多重集，不比页序）" % name,
              bullet_pages_multiset(got.get("slides", [])),
              bullet_pages_multiset(want.get("slides", [])))
        check("%s 整册那份列表样式账与读者一致" % name,
              dig(got, "bullet_layers"), want.get("bullet_layers"))

    bul = lbin("office-slide", fixture("bullets.pptx"))
    check(
        "`bullets.pptx` 第 1 页是这一族的第一问：整页三段**连 `a:pPr` 都没有** —— "
        "`ppr_written` 0、`ppr_missing` 3、`kinds` 只有 `(无 pPr)` 一项，而 `silent` 3 "
        "不等于「这些段没有符号」（符号在版式与母版那一层，见下面那本）",
        [dig(bul, "slides[0].bullets.paragraphs"), dig(bul, "slides[0].bullets.ppr_written"),
         dig(bul, "slides[0].bullets.ppr_missing"), dig(bul, "slides[0].bullets.declared"),
         dig(bul, "slides[0].bullets.silent"), dig(bul, "slides[0].bullets.kinds"),
         dig(bul, "slides[0].bullets.chars"), dig(bul, "slides[0].bullets.carriers_seen")],
        [3, 0, 3, 0, 3, {'(无 pPr)': 3}, {}, {'sp': 2}],
    )
    check(
        "第 2 页三枚 `buChar`：字按写的交（两枚 `•` 一枚 `‣`），`@marL` 与 `@indent` 是两个数、"
        "`buSzPct` 只有两条写了（第三条只写字），第三段那条 `spcAft` 里的点是 `1200`"
        "（百分之一磅，本仓不换算），而 `lvl` 三条全没写 —— 一级是文件的缺省，不是它说了一级",
        [dig(bul, "slides[1].bullets.kinds"), dig(bul, "slides[1].bullets.chars"),
         dig(bul, "slides[1].bullets.marl_written"), dig(bul, "slides[1].bullets.bu_sz_written"),
         dig(bul, "slides[1].bullets.spc_after_written"), dig(bul, "slides[1].bullets.lvl_written"),
         [[one["para"], one["kind"], one["char"], one["mar_l"], one["indent"],
           one["bu_sz_pct"], one["spc_after"], one["attrs_written"]]
          for one in dig(bul, "slides[1].bullets.rows")]],
        [{'(无 pPr)': 1, 'buChar': 3},
         {'•': 2, '‣': 1},
         3,
         2,
         1,
         0,
         [[0, '(无 pPr)', None, None, None, None, None, 0],
          [1, 'buChar', '•', '342900', '-342900', '100000', None, 2],
          [2, 'buChar', '‣', '342900', '-228600', '90000', None, 2],
          [3, 'buChar', '•', '342900', '-342900', None, '1200', 2]]],
    )
    check(
        "第 3 页那一支只能合成：`buAutoNum/@type` 是 `arabicPeriod` 两条，`@startAt` 只有第一条写"
        "（`3`），第二条没有 —— 「从三开始」与「它自己排」是两句话。"
        "真件普查里 `buAutoNum` **一条都没有**（104 份 pptx、11146 枚 `a:pPr`），"
        "而 `buNone` 7064、`buChar` 3672、什么符号都没写 410",
        [dig(bul, "slides[2].bullets.auto_types"), dig(bul, "slides[2].bullets.chars"),
         [[one["para"], one["kind"], one["auto_type"], one["start_at"]]
          for one in dig(bul, "slides[2].bullets.rows") if one["ppr_written"]]],
        [{'arabicPeriod': 2},
         {},
         [[1, 'buAutoNum', 'arabicPeriod', '3'], [2, 'buAutoNum', 'arabicPeriod', None]]],
    )
    check(
        "第 4 页是「明确不画」：两条 `buNone` —— 与第 1 页的「什么都没写」在 `kinds` 里是**两个不同的键**，"
        "而 `marl_written` 2 里有一条 `marL=914400`（缩进留大却不给符号）",
        [dig(bul, "slides[3].bullets.kinds"), dig(bul, "slides[3].bullets.declared"),
         dig(bul, "slides[3].bullets.silent"), dig(bul, "slides[3].bullets.marl_written"),
         [one["mar_l"] for one in dig(bul, "slides[3].bullets.rows") if one["ppr_written"]]],
        [{'(无 pPr)': 1, 'buNone': 2}, 2, 1, 2, ['0', '914400']],
    )
    back = lbin("office-slide", fixture("bullets-lo.pptx"))
    check(
        "LibreOffice 重写同一份：每段都被补上 `a:pPr`（`ppr_missing` 0、`ppr_written` 13），"
        "`declared` 13、`silent` 0 —— 连标题那一段也写上 `buNone`。"
        "同一个「左边距」的数它换了写法（合成的 `342900` EMU 被写成 `343080`），"
        "并把 `algn`、`defTabSz`、`lnSpc`、`buClr`、`buFont`、`tabLst` 一起写下来"
        "（`attrs_written` 从 2 涨到 4）—— 所以「几条属性」是笔迹，不是答案",
        [dig(back, "slides[0].bullets.paragraphs"), dig(back, "slides[0].bullets.ppr_written"),
         dig(back, "slides[0].bullets.silent"), dig(back, "slides[0].bullets.kinds"),
         dig(back, "slides[0].bullets.spc_before_written"), dig(back, "slides[0].bullets.chars"),
         [one["mar_l"] for one in dig(back, "slides[1].bullets.rows") if one["mar_l"]],
         [one["attrs_written"] for one in dig(back, "slides[1].bullets.rows")]],
        [3,
         3,
         0,
         {'buNone': 1, 'buChar': 2},
         2,
         {'•': 2},
         ['343080', '343080', '343080'],
         [3, 4, 4, 4]],
    )
    check(
        "同一问在两层之外：这一本的版式与母版里 `a:lvlNpPr` 才是符号的住处 —— python-pptx 那份"
        "134 条里 61 条**什么都不写**（`(没写)` 61），而 LibreOffice 重写那份 88 条一条不落"
        "（`silent` 0），且它把母版的 `p:txBody` 整个丢掉（`parts` 一样是 12 件，"
        "`parts_with_lst_style` 12 对 11）—— 两层各交各的密度，不做归属那一跳",
        [dig(bul, "bullet_layers.parts"), dig(bul, "bullet_layers.parts_with_lst_style"),
         dig(bul, "bullet_layers.rows_total"), dig(bul, "bullet_layers.kinds"),
         dig(bul, "bullet_layers.silent"), dig(bul, "bullet_layers.chars"),
         dig(bul, "bullet_layers.levels.lvl1pPr"),
         dig(back, "bullet_layers.rows_total"), dig(back, "bullet_layers.silent"),
         dig(back, "bullet_layers.kinds"), dig(back, "bullet_layers.chars")],
        [12,
         12,
         134,
         {'(没写)': 61, 'buNone': 64, 'buChar': 9},
         61,
         {'•': 6, '–': 2, '»': 1},
         22,
         88,
         0,
         {'buNone': 48, 'buChar': 40},
         {'•': 16, '–': 16, '»': 8}],
    )
    odp = lbin("office-slide", fixture("bullets.odp"))
    check(
        "同一份稿子到 ODF 是**另一套记法**：段自己什么都不写，写的是它外面那层 `text:list` 点的名 —— "
        "第 1 页两份列表（`L1`、`L2`）各解开一份样式、各定义十级、第 1 级都是 `bullet` 且字是 `•`，"
        "而 LibreOffice 从 pptx 转来时**一行拆一份列表**（`items` 1、`paras_direct` 1）",
        [dig(odp, "slides[0].bullets.lists"), dig(odp, "slides[0].bullets.styles_found"),
         dig(odp, "slides[0].bullets.kinds"), dig(odp, "slides[0].bullets.chars"),
         dig(odp, "slides[0].bullets.paras_in_lists"),
         [[one["element"], one["style_name"], one["style_part"], one["levels_defined"],
           one["level1_kind"], one["level1_char"], one["items"]]
          for one in dig(odp, "slides[0].bullets.rows")]],
        [2,
         2,
         {'bullet': 2},
         {'•': 2},
         2,
         [['list', 'L1', 'content.xml', 10, 'bullet', '•', 1],
          ['list', 'L2', 'content.xml', 10, 'bullet', '•', 1]]],
    )
    check(
        "自动编号在 ODF 换了一格：第 3 页那两份列表的样式第 1 级是 `text:list-level-style-number`，"
        "`num-format` 是 `1`、`num-suffix` 是 `.`、`start-value` 一条写 `3` 一条不写 —— "
        "与 pptx 那两条 `buAutoNum`（`@startAt` 只写一条）是同一件事的两种写法，"
        "而 `level1_char` 两格都是 null（编号没有「那个字」）",
        [dig(odp, "slides[2].bullets.kinds"), dig(odp, "slides[2].bullets.chars"),
         [[one["style_name"], one["level1_kind"], one["level1_char"],
           one["level1_num_format"], one["level1_start_value"], one["level1_num_suffix"]]
          for one in dig(odp, "slides[2].bullets.rows")]],
        [{'number': 2},
         {},
         [['L4', 'number', None, '1', '3', '.'], ['L5', 'number', None, '1', None, '.']]],
    )
    check(
        "反面凭据在 ODF 这一族最锋利：第 4 页（pptx 里那两条 `buNone`）转过来之后**一份列表都没有** —— "
        "`lists` 0、`paras` 2、`paras_outside` 2，「明确不画」与「这一族不写」在这一格上分不开，"
        "只能按交的回答：这一页没有任何列表结构",
        [dig(odp, "slides[3].bullets.lists"), dig(odp, "slides[3].bullets.paras"),
         dig(odp, "slides[3].bullets.paras_outside"), dig(odp, "slides[3].bullets.kinds"),
         dig(odp, "slides[3].bullets.styles_defined")],
        [0, 2, 2, {}, 54],
    )
    check(
        "整册那本样式账看得见没人点的名：这一包 54 份 `text:list-style`（页用的在 `content.xml`、"
        "母版页用的 `ML1`…`ML10` 在 `styles.xml`），每份都定义十级，"
        "而页只点了 5 份 —— `styles_unused` 49 就是「写了没人用」那一格",
        [dig(odp, "bullet_layers.styles"), dig(odp, "bullet_layers.styles_used"),
         dig(odp, "bullet_layers.styles_unused"), dig(odp, "bullet_layers.kinds"),
         dig(odp, "bullet_layers.levels_per_style"), dig(odp, "bullet_layers.chars"),
         dig(odp, "bullet_layers.cut"), len(dig(odp, "bullet_layers.rows"))],
        [54,
         5,
         49,
         {'bullet': 30, 'number': 24},
         {'10': 54},
         {'•': 15, '●': 14, '‣': 1},
         False,
         54],
    )
    py_pages = [page for name2 in files if name2.endswith(".pptx")
                for page in files[name2]["ooxml"]["slides"]]
    po_pages = [page for name2 in files if name2.endswith(".odp")
                for page in (files[name2].get("odp") or {}).get("slides", [])]

    def bl_tally(pages, key, sub=None):
        out = {}
        for one in pages:
            had = (one.get(key) or {})
            vals = had.get(sub) if sub else had
            if isinstance(vals, dict):
                for kk, vv in vals.items():
                    out[kk] = out.get(kk, 0) + vv
            elif vals is not None:
                out[str(vals)] = out.get(str(vals), 0) + 1
        return out
    check(
        "整库摊开：33 份 pptx 的 80 页与 18 份 odp 的 43 页各交一本 —— "
        "pptx 那几页里写符号的段、没写壳的段、`buNone` 与 `buChar` 各是多少，"
        "odp 那几页里列表数与「解开/解不开」各是多少，一次看全（不是逐件对账，是这一族的存量）",
        [len(py_pages), bl_tally(py_pages, "bullets", "kinds"),
         len(po_pages), bl_tally(po_pages, "bullets", "kinds"),
         bl_tally(po_pages, "bullets", "chars")],
        [80, {'buNone': 28, '(没写)': 64, '(无 pPr)': 99, 'buChar': 12, 'buAutoNum': 4}, 43, {'bullet': 9, 'number': 2}, {'•': 8, '‣': 1}],
    )
    check(
        "层也各有一本存量：33 份 pptx 的版式与母版件里那些 `a:lvlNpPr` 合起来是几枚、"
        "三种答案各多少、`•` 出现几次；18 份 odp 的列表样式合起来 792 份、其中 783 份没人点",
        [sum(had["ooxml"]["bullet_layers"]["rows_total"] for name2, had in files.items()
             if name2.endswith(".pptx")),
         sum(had["ooxml"]["bullet_layers"]["declared"] for name2, had in files.items()
             if name2.endswith(".pptx")),
         sum(had["ooxml"]["bullet_layers"]["silent"] for name2, had in files.items()
             if name2.endswith(".pptx")),
         sum(had["odp"]["bullet_layers"]["styles"] for name2, had in files.items()
             if name2.endswith(".odp")),
         sum(had["odp"]["bullet_layers"]["styles_unused"]
             for name2, had in files.items() if name2.endswith(".odp"))],
        [3705, 2650, 1055, 792, 783],
    )
    check(
        "反面凭据：这一格只在 office-slide 交。遗留 `.ppt` 的符号在 `TextHeaderAtom` 的样式里、"
        "本仓不猜，`.odt` 与 `.ods` 走的是 office-doc / office-sheet 那两个出口，"
        "三家的整份输出里都找不到 `bullets` 这个键（缺键 = 这一族没这一层，不交零条的账冒充读过）",
        [no_theme_key("office-slide", "deck.ppt", "bullets"),
         no_theme_key("office-doc", "notes.odt", "bullets"),
         no_theme_key("office-sheet", "book.ods", "bullets"),
         no_theme_key("office-slide", "bullets.odp", "bullets")],
        [False, False, False, True],
    )

    # ── 3bw) 格子的字离边多远：docx 两个住处、pptx 四枚属性、ODF 一跳在 table-cell 样式上 ──
    print("=== 3bw) cell_margins：表级 / 格级 / 一跳在样式 ===")
    for name in sorted(one.name for one in FIXTURES.glob("*.docx")):
        check("%s 格子内间距那份账与读者一致（表级一块 + 每格一块）" % name,
              dig(lbin("office-doc", fixture(name)), "structure.cell_margins"),
              files[name]["ooxml"]["cell_margins"])
    for name in sorted(one.name for one in FIXTURES.glob("*.odt")):
        check("%s 格子内间距那一跳与读者一致（四长款或一枚短款）" % name,
              dig(lbin("office-doc", fixture(name)), "structure.cell_margins"),
              files[name]["odt"]["cell_margins"])
    for name in sorted(one.name for one in FIXTURES.glob("*.pptx")):
        check("%s 表格里那四枚属性与读者一致（按 marL/R/T/B 的固定序）" % name,
              dig(lbin("office-slide", fixture(name)), "cell_margins"),
              files[name]["ooxml"]["cell_margins"])
    for name in sorted(one.name for one in FIXTURES.glob("*.odp")):
        check("%s 那一页表的 padding 一跳与读者一致（住在 graphic-properties 上）" % name,
              dig(lbin("office-slide", fixture(name)), "cell_margins"),
              files[name]["odp"]["cell_margins"])
    mar = lbin("office-doc", fixture("margins.docx"))
    check(
        "`margins.docx` 四张表四种答案（表级块 2 写 1 空壳 1 不写）：`w:tblCellMar` 里"
        "`left` 出现 6 次比 `top` 多（有一张只写左右、有一格只写一条 `auto`），"
        "而**正文里一个方向都没写的表照样有边距** —— `word/styles.xml` 那 100 枚 `tblCellMar`"
        "就是这件事的凭据，所以这一格分开交，不替文件挑一份样式",
        [dig(mar, "structure.cell_margins.tables"),
         dig(mar, "structure.cell_margins.tables_with_mar"),
         dig(mar, "structure.cell_margins.tables_without_mar"),
         dig(mar, "structure.cell_margins.tables_shell"),
         dig(mar, "structure.cell_margins.cells_with_mar"),
         dig(mar, "structure.cell_margins.dirs"),
         dig(mar, "structure.cell_margins.types"),
         dig(mar, "structure.cell_margins.styles_part_mar"),
         dig(mar, "structure.cell_margins.rows[1]")],
        [4,
         2,
         1,
         1,
         4,
         {'top': 3, 'left': 6, 'bottom': 3, 'right': 5},
         {'dxa': 16, 'auto': 1},
         100,
         {'part': 'word/document.xml',
          'table': 1,
          'mar_present': False,
          'dirs': [],
          'values': {},
          'shell': False}],
    )
    check(
        "格级那一本按「哪一格改过」记：第一格写四条（序与表级不同）、第二格只写左右两条且都是零、"
        "第三格四条约零、第四格只一条 `w:type=\"auto\"`；表级 8 条加格级 9 条合起来 zero 6 / "
        "nonzero 11，而 12 格里另 8 格一个方向都不改（不交行，交进 `cells_total`）",
        [[one["table"], one["cell"], one["dirs"], one["shell"]]
         for one in dig(mar, "structure.cell_margins.cell_rows")],
        [[0, 0, ['top', 'bottom', 'left', 'right'], False],
         [0, 1, ['left', 'right'], False],
         [0, 3, ['top', 'left', 'bottom', 'right'], False],
         [0, 4, ['left'], False]],
    )
    back = lbin("office-doc", fixture("margins-lo.docx"))
    check(
        "LibreOffice 重写同一份：四张表被并成一张（`tables` 4 → 1）、方向改名"
        "（`left/right` → `start/end`，四张票各 12）、11 格各补四条，而 `w:type=\"auto\"` "
        "那一条被换成 `dxa`（`types` 只剩一个键）—— 表级那四条也换成了第一个格写过的值，"
        "这一族的对调按文件记，不判谁对",
        [dig(back, "structure.cell_margins.tables"),
         dig(back, "structure.cell_margins.cells_with_mar"),
         dig(back, "structure.cell_margins.dirs"),
         dig(back, "structure.cell_margins.types"),
         dig(back, "structure.cell_margins.zero"),
         dig(back, "structure.cell_margins.nonzero"),
         dig(back, "structure.cell_margins.rows[0].dirs")],
        [1,
         11,
         {'top': 12, 'start': 12, 'bottom': 12, 'end': 12},
         {'dxa': 48},
         6,
         42,
         ['top', 'start', 'bottom', 'end']],
    )
    mdeck = lbin("office-slide", fixture("margins.pptx"))
    check(
        "pptx 这一族只有一处：`a:tcPr` 上四枚属性。python-pptx 只写被设过的那几枚 —— "
        "六格里两格写满四条、一格只有 `marL`、三格一枚都没有（`cells_silent` 3），"
        "`attrs_seen` 因此是 3/2/2/2 而不是 6/6/6/6",
        [dig(mdeck, "cell_margins.cells"), dig(mdeck, "cell_margins.cells_all_four"),
         dig(mdeck, "cell_margins.cells_partial"), dig(mdeck, "cell_margins.cells_silent"),
         dig(mdeck, "cell_margins.attrs_seen"), dig(mdeck, "cell_margins.zero"),
         dig(mdeck, "cell_margins.nonzero"),
         [[one["cell"], one["written"]] for one in dig(mdeck, "cell_margins.rows")]],
        [6,
         2,
         1,
         3,
         {'marL': 3, 'marR': 2, 'marT': 2, 'marB': 2},
         5,
         4,
         [[0, ['marL', 'marR', 'marT', 'marB']],
          [1, ['marL', 'marR', 'marT', 'marB']],
          [2, ['marL']],
          [3, []],
          [0, []],
          [1, []]]],
    )
    mback = lbin("office-slide", fixture("margins-lo.pptx"))
    check(
        "LibreOffice 重写那份 pptx：六格全被补齐（`cells_all_four` 6、`cells_silent` 0），"
        "而 `marT` 的 `36576` 绕一圈变成 `36360`（EMU 过一遍磅再回来）—— 本仓不换算，"
        "两个串各按写的交",
        [dig(mback, "cell_margins.cells_all_four"), dig(mback, "cell_margins.cells_silent"),
         dig(mback, "cell_margins.attrs_seen"), dig(mback, "cell_margins.zero"),
         dig(mback, "cell_margins.nonzero"),
         dig(mback, "cell_margins.rows[0].mar_t"), dig(mdeck, "cell_margins.rows[0].mar_t")],
        [6, 0, {'marL': 6, 'marR': 6, 'marT': 6, 'marB': 6}, 5, 19, '36360', '36576'],
    )
    odt = lbin("office-doc", fixture("margins.odt"))
    check(
        "同一问在 ODF 是一跳加两种写法：12 格各点一份 `family=table-cell` 的样式，"
        "11 份写四枚长款、**全零那一份被写成短款** `fo:padding`（`shorthand` 1 / `longhand` 11），"
        "数住在 `style:table-cell-properties` 上；cm 是有损换算（113 twips → `0.199cm`）",
        [dig(odt, "structure.cell_margins.cells"), dig(odt, "structure.cell_margins.cells_named"),
         dig(odt, "structure.cell_margins.styles_found"), dig(odt, "structure.cell_margins.shorthand"),
         dig(odt, "structure.cell_margins.longhand"), dig(odt, "structure.cell_margins.holders"),
         dig(odt, "structure.cell_margins.keys"), dig(odt, "structure.cell_margins.styles_defined"),
         [dig(odt, "structure.cell_margins.rows[0].top"),
          dig(odt, "structure.cell_margins.rows[0].left")]],
        [12,
         12,
         12,
         1,
         11,
         {'table-cell-properties': 12},
         {'padding-left': 11,
          'padding-right': 11,
          'padding-top': 11,
          'padding-bottom': 11,
          'padding': 1},
         5,
         ['0.101cm', '0.3cm']],
    )
    odp = lbin("office-slide", fixture("margins.odp"))
    check(
        "odp 那一族的格是图形对象：同样四枚 padding 住在 `style:graphic-properties` 上，"
        "六格里两格**连样式名都不点**（`cells_unnamed` 2）—— 那一格只能交「这一格没说」，"
        "不按母版页那份缺省补一个数",
        [dig(odp, "cell_margins.cells"), dig(odp, "cell_margins.cells_named"),
         dig(odp, "cell_margins.cells_unnamed"), dig(odp, "cell_margins.cells_with_pads"),
         dig(odp, "cell_margins.holders"), dig(odp, "cell_margins.shorthand"),
         dig(odp, "cell_margins.zero"), dig(odp, "cell_margins.nonzero"),
         dig(odp, "cell_margins.rows[0].holder"),
         dig(odp, "cell_margins.rows[0].top")],
        [6, 4, 2, 4, {'graphic-properties': 4}, 0, 5, 11, 'graphic-properties', '0.101cm'],
    )
    def _mar(name, fam):
        had = files.get(name) or {}
        one = had.get(fam)
        return one["cell_margins"] if isinstance(one, dict) and "cell_margins" in one else None

    word = dict((one, had) for one, _raw in files.items() if one.endswith((".docx", ".docm"))
                for had in [_mar(one, "ooxml")] if had)
    od = dict((one, had) for one, _raw in files.items() if one.endswith(".odt")
              for had in [_mar(one, "odt")] if had)
    decks = dict((one, had) for one, _raw in files.items() if one.endswith(".pptx")
                 for had in [_mar(one, "ooxml")] if had)
    shows = dict((one, had) for one, _raw in files.items() if one.endswith(".odp")
                 for had in [_mar(one, "odp")] if had)
    check(
        "整库摊开：92 份 word 件 59 张表里 27 张写了表级块、313 格里 15 格自己改过；33 份 pptx 的 70 格里 31 格一枚属性都不写、37 格写满四条 —"
        "— 「没写」这一形只在自产件里出现，本机 104 份真 pptx 的 893 枚 `a:tcPr` 每一枚都写满四条",
        [len(word), sum(one["tables"] for one in word.values()),
         sum(one["tables_with_mar"] for one in word.values()),
         sum(one["cells_total"] for one in word.values()),
         sum(one["cells_with_mar"] for one in word.values()),
         sum(one["zero"] for one in word.values()),
         sum(one["nonzero"] for one in word.values()),
         sum(one["styles_part_mar"] for one in word.values()),
         len(decks), sum(one["cells"] for one in decks.values()),
         sum(one["cells_silent"] for one in decks.values()),
         sum(one["cells_all_four"] for one in decks.values())],
        [95, 59, 27, 313, 15, 60, 101, 8100, 33, 70, 31, 37],
    )
    check(
        "ODF 那一头整库一本：49 份 .odt 的 162 格全部点得到样式（解开 162、解不开 0），161 份写四枚长款而**只有 1 份写短款**；18 份 .odp 一共才 33 格，"
        "其中 18 格连样式名都不点",
        [len(od), sum(one["cells"] for one in od.values()),
         sum(one["styles_found"] for one in od.values()),
         sum(one["shorthand"] for one in od.values()),
         sum(one["longhand"] for one in od.values()),
         len(shows), sum(one["cells"] for one in shows.values()),
         sum(one["cells_unnamed"] for one in shows.values()),
         sum(one["styles_defined"] for one in shows.values())],
        [51, 162, 162, 1, 161, 18, 33, 18, 23],
    )
    check(
        "反面凭据：这一格只在读了表的三个出口交。遗留 `.ppt` 与 `.xls` 的记录里"
        "没有「格子内间距」这个东西（BIFF 的 `MULBLANK` / ppt 的 `TextFooterAtom` 都不带），"
        "RTF 的表只有 `\\intbl` 与格分隔，所以三家的整份输出里都找不到 `cell_margins` 这个键",
        [no_theme_key("office-doc", "notes.rtf", "cell_margins"),
         no_theme_key("office-doc", "notes.doc", "cell_margins"),
         no_theme_key("office-sheet", "book.xls", "cell_margins"),
         no_theme_key("office-slide", "deck.ppt", "cell_margins"),
         no_theme_key("office-doc", "margins.odt", "cell_margins"),
         no_theme_key("office-slide", "margins.odp", "cell_margins")],
        [False, False, False, False, True, True],
    )

    # ── 3bx) 这一圈有没有线：docx 两个住处、pptx 六枚孩子、ODF 一跳在样式 ──
    print("=== 3bx) table_borders：表级 / 格级 / 一跳在样式 ===")
    for name in sorted(one.name for one in FIXTURES.glob("*.docx")):
        check("%s 表边框那份账与读者一致（表级一块 + 每格一块）" % name,
              dig(lbin("office-doc", fixture(name)), "structure.table_borders"),
              files[name]["ooxml"]["table_borders"])
    for name in sorted(one.name for one in FIXTURES.glob("*.odt")):
        check("%s 表边框那一跳与读者一致（短款、长款、还有只写线宽的第三种）" % name,
              dig(lbin("office-doc", fixture(name)), "structure.table_borders"),
              files[name]["odt"]["table_borders"])
    for name in sorted(one.name for one in FIXTURES.glob("*.pptx")):
        check("%s 那六枚线孩子与读者一致（按文档序，不是固定序）" % name,
              dig(lbin("office-slide", fixture(name)), "table_borders"),
              files[name]["ooxml"]["table_borders"])
    for name in sorted(one.name for one in FIXTURES.glob("*.odp")):
        check("%s 那一格的边框住在哪枚孩子上与读者一致" % name,
              dig(lbin("office-slide", fixture(name)), "table_borders"),
              files[name]["odp"]["table_borders"])
    bor = lbin("office-doc", fixture("borders.docx"))
    check(
        "`borders.docx` 三张表三种写法（表级写满六方向、表级只写 nil、格级自己改）：`@w:val` 的四态在这一份里出现三种 —— single 9、nil 2、double 1、none 1，"
        "而 **nil 那两枚不写 sz**（no_sz 2）；auto 这种颜色说了 3 次，另有 2 枚指向主题（themeColor 在场，解到实色不在这一格）；而**没人写表级块的那张表照样有线** —"
        "— `word/styles.xml` 那 85 枚 tblBorders 与 406 枚 tcBorders 就是这件事的凭据，所以分开交，不替文件挑一份样式",
        [dig(bor, "structure.table_borders.tables"),
         dig(bor, "structure.table_borders.tables_with_block"),
         dig(bor, "structure.table_borders.tables_without_block"),
         dig(bor, "structure.table_borders.cells_total"),
         dig(bor, "structure.table_borders.cells_with_block"),
         dig(bor, "structure.table_borders.cells_shell"),
         dig(bor, "structure.table_borders.dirs"),
         dig(bor, "structure.table_borders.vals"),
         dig(bor, "structure.table_borders.no_sz"),
         dig(bor, "structure.table_borders.auto_color"),
         dig(bor, "structure.table_borders.theme_pointed"),
         dig(bor, "structure.table_borders.styles_part_tbl"),
         dig(bor, "structure.table_borders.styles_part_tc"),
         [dig(bor, "structure.table_borders.rows[0].dirs"),
          dig(bor, "structure.table_borders.rows[2].dirs")]],
        [3,
         2,
         1,
         10,
         3,
         1,
         {'top': 3, 'left': 3, 'bottom': 2, 'right': 2, 'insideH': 1, 'insideV': 1, 'tl2br': 1},
         {'single': 9, 'nil': 2, 'double': 1, 'none': 1},
         2,
         3,
         2,
         85,
         406,
         [['top', 'left', 'bottom', 'right', 'insideH', 'insideV'],
          ['top', 'left', 'bottom', 'right']]],
    )
    backb = lbin("office-doc", fixture("borders-lo.docx"))
    check(
        "LibreOffice 重写同一份：三张表并成一张、表级那份**一份都不剩**（tables_with_block 0）而摊到了每个格上（cells_with_block 3 → 10）"
        "，方向改名（left/right → start/end），nil 与 none 全部换成写实（vals 只剩 single 与 double、no_sz 0），auto 与主题指针被换成实色（auto_color 0、theme_pointed 0）"
        "—— 块从哪一层挪到哪一层是它的算法，本仓按文件记下来的交",
        [dig(backb, "structure.table_borders.tables"),
         dig(backb, "structure.table_borders.tables_with_block"),
         dig(backb, "structure.table_borders.cells_with_block"),
         dig(backb, "structure.table_borders.dirs"),
         dig(backb, "structure.table_borders.vals"),
         dig(backb, "structure.table_borders.auto_color"),
         dig(backb, "structure.table_borders.theme_pointed"),
         dig(backb, "structure.table_borders.no_sz")],
        [1,
         0,
         10,
         {'bottom': 10, 'end': 10, 'start': 4, 'top': 8},
         {'single': 31, 'double': 1},
         0,
         0,
         0],
    )
    odtb = lbin("office-doc", fixture("borders.odt"))
    check(
        "同一问在 ODF 是一跳加三种写法：10 格各点一份 family=table-cell 的样式、各解开，四枚长款 fo:border-* 一条不落（longhand 10 而 shorthand 0）"
        "，数住在 style:table-cell-properties 上；single 八分之一磅回来是 1pt solid #000000、double 是 2.25pt double，"
        "而 nil 与 none 在这一格**合成同一个 none**（kinds 里 none 8），另有第三种只写线宽的 style:border-line-width-top 1 枚，"
        "画法是 collapsing 还是 separate 记在 border_models",
        [dig(odtb, "structure.table_borders.cells"),
         dig(odtb, "structure.table_borders.cells_named"),
         dig(odtb, "structure.table_borders.styles_found"),
         dig(odtb, "structure.table_borders.cells_with_borders"),
         dig(odtb, "structure.table_borders.shorthand"),
         dig(odtb, "structure.table_borders.longhand"),
         dig(odtb, "structure.table_borders.lines"),
         dig(odtb, "structure.table_borders.holders"),
         dig(odtb, "structure.table_borders.kinds"),
         dig(odtb, "structure.table_borders.border_models"),
         dig(odtb, "structure.table_borders.keys.border-line-width-top"),
         [dig(odtb, "structure.table_borders.rows[0].left"),
          dig(odtb, "structure.table_borders.rows[0].top"),
          dig(odtb, "structure.table_borders.rows[3].top")]],
        [10,
         10,
         10,
         10,
         0,
         10,
         40,
         {'table-cell-properties': 10},
         {'none': 8, 'solid': 31, 'double': 1},
         {'table-properties/collapsing': 1},
         1,
         ['none', 'none', '1pt solid #000000']],
    )
    bdeck = lbin("office-slide", fixture("borders.pptx"))
    check(
        "pptx 这一族线不是属性而是 `a:tcPr` 的**孩子**：六格里三格有（一格写满上下左右、两格只写一两枚、三格一枚都没有），一枚线带的是一串（w 与 fill / dash / round / head）"
        "——本仓按文档序交，edges 因此是 lnL 2 / lnR 1 / lnT 2 / lnB 2 / lnTlToBr 1，而零宽那枚（w=0）与一枚都不写是两种「没有」",
        [dig(bdeck, "table_borders.cells"), dig(bdeck, "table_borders.cells_with_edges"),
         dig(bdeck, "table_borders.cells_all_four"), dig(bdeck, "table_borders.cells_partial"),
         dig(bdeck, "table_borders.cells_silent"), dig(bdeck, "table_borders.edges"),
         dig(bdeck, "table_borders.widths"), dig(bdeck, "table_borders.fills"),
         dig(bdeck, "table_borders.dashes"), dig(bdeck, "table_borders.zero_width"),
         [[one["cell"], one["edges"]] for one in dig(bdeck, "table_borders.rows")]],
        [6,
         3,
         1,
         2,
         3,
         {'lnL': 2, 'lnR': 1, 'lnT': 2, 'lnB': 2, 'lnTlToBr': 1},
         {'6350': 5, '0': 1, '12700': 1, '25400': 1},
         {'solidFill': 7, 'noFill': 1},
         {'solid': 7, 'dash': 1},
         1,
         [[0, ['lnL', 'lnR', 'lnT', 'lnB']],
          [1, ['lnL', 'lnT']],
          [2, ['lnB', 'lnTlToBr']],
          [3, []],
          [0, []],
          [1, []]]],
    )
    bback = lbin("office-slide", fixture("borders-lo.pptx"))
    check(
        "LibreOffice 重写那份 pptx：六格全被补齐四条（cells_all_four 6、cells_silent 0），6350 换成 6480、12700 与 25400 那两枚换成 12240 而**对角线整个丢掉**（edges 里没有 lnTlToBr）"
        "，还有一枚线**不写 w**（no_width 1）—— 与页边距那条 36576→36360 同一类来回换算，本仓不判谁对",
        [dig(bback, "table_borders.cells_all_four"), dig(bback, "table_borders.cells_silent"),
         dig(bback, "table_borders.edges"), dig(bback, "table_borders.widths"),
         dig(bback, "table_borders.fills"), dig(bback, "table_borders.no_width"),
         dig(bback, "table_borders.zero_width"),
         [dig(bback, "table_borders.rows[0].lines.lnT.w"),
          dig(bdeck, "table_borders.rows[0].lines.lnT.w")]],
        [6,
         0,
         {'lnL': 6, 'lnR': 6, 'lnT': 6, 'lnB': 6},
         {'6480': 5, '12240': 18},
         {'solidFill': 22, 'noFill': 2},
         1,
         0,
         ['6480', '6350']],
    )
    odtb2 = lbin("office-slide", fixture("borders.odp"))
    check(
        "odp 那一族的格是图形对象，而它的**边框不住在 graphic-properties 上**（页边距住那里）—— 实测住在 style:paragraph-properties；"
        "六格里两格连样式名都不点（只能交「这一格没说」），短款与长款各 2，虚线那一条转过来还在（kinds 里 dashed 1）",
        [dig(odtb2, "table_borders.cells"), dig(odtb2, "table_borders.cells_named"),
         dig(odtb2, "table_borders.cells_unnamed"), dig(odtb2, "table_borders.holders"),
         dig(odtb2, "table_borders.shorthand"), dig(odtb2, "table_borders.longhand"),
         dig(odtb2, "table_borders.kinds"), dig(odtb2, "table_borders.styles_defined"),
         dig(odtb2, "table_borders.rows[0].shorthand"),
         dig(odtb2, "table_borders.rows[1].top")],
        [6,
         4,
         2,
         {'paragraph-properties': 4},
         2,
         2,
         {'solid': 8, 'none': 1, 'dashed': 1},
         5,
         '0.26pt solid #000000',
         '0.48pt dashed #ff0000'],
    )
    def _bor(name, fam):
        had = files.get(name) or {}
        one = had.get(fam)
        return one["table_borders"] if isinstance(one, dict) and "table_borders" in one else None

    wordb = dict((one, had) for one, _raw in files.items() if one.endswith((".docx", ".docm"))
                 for had in [_bor(one, "ooxml")] if had)
    odb = dict((one, had) for one, _raw in files.items() if one.endswith(".odt")
               for had in [_bor(one, "odt")] if had)
    deckb = dict((one, had) for one, _raw in files.items() if one.endswith(".pptx")
                 for had in [_bor(one, "ooxml")] if had)
    showb = dict((one, had) for one, _raw in files.items() if one.endswith(".odp")
                 for had in [_bor(one, "odp")] if had)
    check(
        "整库摊开（自产件）：92 份 word 件的 59 张表里只有 2 张写了表级块，而 156 个格自己写了边框块 —— 「表上没写」在这一族是常态而不是缺读：样式表里 6715 枚 `tblBorders` 就是给它们的；"
        "`nil` 那一形让 2 条方向条目**不带 `sz`**（`no_sz` 2），`auto` 说过 3 次。演示那一头 33 份 pptx 的 70 个格里 32 个一枚线都不写、36 个写满四条",
        [len(wordb), sum(one["tables"] for one in wordb.values()),
         sum(one["tables_with_block"] for one in wordb.values()),
         sum(one["cells_with_block"] for one in wordb.values()),
         sum(one["no_sz"] for one in wordb.values()),
         sum(one["auto_color"] for one in wordb.values()),
         sum(one["styles_part_tbl"] for one in wordb.values()),
         len(deckb), sum(one["cells"] for one in deckb.values()),
         sum(one["cells_silent"] for one in deckb.values()),
         sum(one["cells_all_four"] for one in deckb.values())],
        [95, 59, 2, 156, 2, 3, 6885, 33, 70, 32, 36],
    )
    check(
        "ODF 那一头整库一本：49 份 .odt 的 172 个格里 163 个跳得到样式且带边框，写法几乎全是短款 `fo:border`（150 对长款 13），一共 202 条边框串；"
        "18 份 .odp 共 35 个格、21 条串，其中 18 个格连样式名都不点",
        [len(odb), sum(one["cells"] for one in odb.values()),
         sum(one["cells_with_borders"] for one in odb.values()),
         sum(one["shorthand"] for one in odb.values()),
         sum(one["longhand"] for one in odb.values()),
         sum(one["lines"] for one in odb.values()),
         len(showb), sum(one["cells"] for one in showb.values()),
         sum(one["cells_unnamed"] for one in showb.values()),
         sum(one["lines"] for one in showb.values())],
        [51, 172, 163, 150, 13, 202, 18, 35, 18, 21],
    )
    check(
        "反面凭据：这一格只在读了表的三个出口交。遗留 `.ppt` 与 `.xls` 的记录里没有「这一圈的线」"
        "这个东西（BIFF 的边框在 XF 那一跳、ppt 的记录树里没有表边框这一层），RTF 的表只有 "
        "`\\intbl` 与格分隔，所以那四家的整份输出里都找不到 `table_borders` 这个键；"
        "而读了表的四族都交",
        [no_theme_key("office-doc", "notes.rtf", "table_borders"),
         no_theme_key("office-doc", "notes.doc", "table_borders"),
         no_theme_key("office-sheet", "book.xls", "table_borders"),
         no_theme_key("office-slide", "deck.ppt", "table_borders"),
         no_theme_key("office-doc", "borders.odt", "table_borders"),
         no_theme_key("office-slide", "borders.odp", "table_borders"),
         no_theme_key("office-doc", "borders.docx", "table_borders"),
         no_theme_key("office-slide", "borders.pptx", "table_borders")],
        [False, False, False, False, True, True, True, True],
    )

    # ── 3by) 这一格的字贴哪一边：docx 一枚元素两个住处、pptx 两枚属性、ODF 一跳且空串照交 ──
    print("=== 3by) vertical_align：格级 / 节上 / 一跳在样式 ===")
    for name in sorted(one.name for one in FIXTURES.glob("*.docx")):
        check("%s 那一格贴哪边与读者一致（格级与节上两本）" % name,
              dig(lbin("office-doc", fixture(name)), "structure.vertical_align"),
              files[name]["ooxml"]["vertical_align"])
    for name in sorted(one.name for one in FIXTURES.glob("*.odt")):
        check("%s 那一格贴哪边与读者一致（一跳在 table-cell 样式上）" % name,
              dig(lbin("office-doc", fixture(name)), "structure.vertical_align"),
              files[name]["odt"]["vertical_align"])
    for name in sorted(one.name for one in FIXTURES.glob("*.pptx")):
        check("%s 那两枚属性与读者一致（不写才是这一族的常态）" % name,
              dig(lbin("office-slide", fixture(name)), "vertical_align"),
              files[name]["ooxml"]["vertical_align"])
    for name in sorted(one.name for one in FIXTURES.glob("*.odp")):
        check("%s 那一格贴哪边与读者一致（转过来这一条整层没写）" % name,
              dig(lbin("office-slide", fixture(name)), "vertical_align"),
              files[name]["odp"]["vertical_align"])
    val = lbin("office-doc", fixture("valign.docx"))
    check(
        "`valign.docx` 六格里四格各写一枚 `w:vAlign`（`center` / `top` / `bottom` / `just` 四态各一），另两格连元素都没有（`said` false 而不是某个词）"
        "；**同名的 `w:sectPr/w:vAlign` 回答的是另一个问** —— 这一节的字在纸上居中吗，所以节上那一本单独交（`sections_total` 1、`sections_with_valign` 1、`section_vals` 只有 `center`）"
        "，而 `word/styles.xml` 这一份里 0 枚",
        [dig(val, "structure.vertical_align.cells_total"),
         dig(val, "structure.vertical_align.cells_with_valign"),
         dig(val, "structure.vertical_align.cells_without_valign"),
         dig(val, "structure.vertical_align.vals"),
         dig(val, "structure.vertical_align.sections_total"),
         dig(val, "structure.vertical_align.sections_with_valign"),
         dig(val, "structure.vertical_align.section_vals"),
         dig(val, "structure.vertical_align.styles_part_valign"),
         [dig(val, "structure.vertical_align.rows[%d].val" % i) for i in range(6)],
         dig(val, "structure.vertical_align.rows[4].said")],
        [6,
         4,
         2,
         {'center': 1, 'top': 1, 'bottom': 1, 'just': 1},
         1,
         1,
         {'center': 1},
         0,
         ['center', 'top', 'bottom', 'just', None, None],
         False],
    )
    valback = lbin("office-doc", fixture("valign-lo.docx"))
    check(
        "LibreOffice 重写同一份：六格里**只剩两格还写着** `vAlign`（`center` 与 `bottom`），`top` 与 `just` 整枚被丢掉（默认值不写是它的算法）"
        "，而节上那一枚 `center` 留着 —— 「没写」在这一趟里既可能是文件没说，也可能是它认为是默认，本仓只按在场的交",
        [dig(valback, "structure.vertical_align.cells_total"),
         dig(valback, "structure.vertical_align.cells_with_valign"),
         dig(valback, "structure.vertical_align.vals"),
         dig(valback, "structure.vertical_align.sections_with_valign"),
         dig(valback, "structure.vertical_align.section_vals"),
         dig(valback, "structure.vertical_align.styles_part_valign")],
        [6, 2, {'center': 1, 'bottom': 1}, 1, {'center': 1}, 0],
    )
    valodt = lbin("office-doc", fixture("valign.odt"))
    check(
        "同一问在 ODF 是一跳且**词表是第三套**：六格各点一份 `family=table-cell` 的样式、各解开，四格有值 —— `center` 在那儿叫 `middle`、`bottom` 还是 `bottom`，"
        "而**两格被写成空串** `style:vertical-align=\"\"`（账里记成 `(空串)`，不折成 null 也不折成词），另两格没这一条；值住在 `style:table-cell-properties` 上",
        [dig(valodt, "structure.vertical_align.cells"),
         dig(valodt, "structure.vertical_align.cells_named"),
         dig(valodt, "structure.vertical_align.styles_found"),
         dig(valodt, "structure.vertical_align.cells_with_valign"),
         dig(valodt, "structure.vertical_align.vals"),
         dig(valodt, "structure.vertical_align.holders"),
         dig(valodt, "structure.vertical_align.styles_defined"),
         [dig(valodt, "structure.vertical_align.rows[%d].val" % i) for i in range(6)]],
        [6,
         6,
         6,
         4,
         {'middle': 1, '(空串)': 2, 'bottom': 1},
         {'table-cell-properties': 4},
         5,
         ['middle', '', 'bottom', '', None, None]],
    )
    vdeck = lbin("office-slide", fixture("valign.pptx"))
    check(
        "pptx 这一族是**两枚属性**：六格里四格写了 `@anchor`（`t` / `ctr` / `b` / `just` 各一）、一格另带 `@anchorCtr=\"1\"`、两格两枚都不写（`cells_silent` 2）"
        "—— 「不写」在这一族是真的一件事，不推成 `t`",
        [dig(vdeck, "vertical_align.cells"), dig(vdeck, "vertical_align.cells_with_anchor"),
         dig(vdeck, "vertical_align.cells_silent"), dig(vdeck, "vertical_align.cells_with_anchor_ctr"),
         dig(vdeck, "vertical_align.anchors"), dig(vdeck, "vertical_align.anchor_ctr_vals"),
         [dig(vdeck, "vertical_align.rows[%d].anchor" % i) for i in range(6)],
         [dig(vdeck, "vertical_align.rows[%d].anchor_ctr" % i) for i in range(6)]],
        [6,
         4,
         2,
         1,
         {'t': 1, 'ctr': 1, 'b': 1, 'just': 1},
         {'1': 1},
         ['t', 'ctr', 'b', 'just', None, None],
         [None, '1', None, None, None, None]],
    )
    vback = lbin("office-slide", fixture("valign-lo.pptx"))
    check(
        "LibreOffice 重写那份 pptx：六格全被写上 `@anchor`（`t` 四、`ctr` 一、`b` 一），`just` 被换成 `t`、本来不写的两格补成 `t`，"
        "而 `@anchorCtr` **一枚都不剩** —— 四态里丢掉一个、第二枚整层蒸发，本仓按文件写下来的交，不替它找回",
        [dig(vback, "vertical_align.cells_with_anchor"), dig(vback, "vertical_align.cells_silent"),
         dig(vback, "vertical_align.cells_with_anchor_ctr"), dig(vback, "vertical_align.anchors"),
         [dig(vback, "vertical_align.rows[%d].anchor" % i) for i in range(6)]]
        ,
        [6, 0, 0, {'t': 4, 'ctr': 1, 'b': 1}, ['t', 'ctr', 'b', 't', 't', 't']],
    )
    vodp = lbin("office-slide", fixture("valign.odp"))
    check(
        "odp 这一头：表还在、样式还在、六格里四格点得到名，而**没有任何一格带 `style:vertical-align`**（转出去时这一条整层没写下来）—— 那一本只交 0，"
        "不按 pptx 那份来补",
        [dig(vodp, "vertical_align.cells"), dig(vodp, "vertical_align.cells_named"),
         dig(vodp, "vertical_align.cells_unnamed"), dig(vodp, "vertical_align.styles_found"),
         dig(vodp, "vertical_align.cells_with_valign"), dig(vodp, "vertical_align.vals"),
         dig(vodp, "vertical_align.holders"),
         [dig(vodp, "vertical_align.rows[%d].val" % i) for i in range(6)]],
        [6, 4, 2, 4, 0, {}, {}, [None, None, None, None, None, None]],
    )
    def _val(name, fam):
        had = files.get(name) or {}
        one = had.get(fam)
        return one["vertical_align"] if isinstance(one, dict) and "vertical_align" in one else None

    wordv = dict((one, had) for one, _raw in files.items() if one.endswith((".docx", ".docm"))
                 for had in [_val(one, "ooxml")] if had)
    odpv = dict((one, had) for one, _raw in files.items() if one.endswith(".odt")
                for had in [_val(one, "odt")] if had)
    deckv = dict((one, had) for one, _raw in files.items() if one.endswith(".pptx")
                 for had in [_val(one, "ooxml")] if had)
    showv = dict((one, had) for one, _raw in files.items() if one.endswith(".odp")
                 for had in [_val(one, "odp")] if had)
    check(
        "整库摊开（自产件）：92 份 word 件的 313 个格里**只有 8 格**写了 `vAlign`，100 个节里 2 个写了节上那一枚，样式表部件里 0 枚 —— 「没写」才是这一族的常态；"
        "33 份 pptx 的 70 个格里 40 个写了 `@anchor`、30 个两枚都不写、`@anchorCtr` 全库只有 1 枚",
        [len(wordv), sum(one["cells_total"] for one in wordv.values()),
         sum(one["cells_with_valign"] for one in wordv.values()),
         sum(one["sections_total"] for one in wordv.values()),
         sum(one["sections_with_valign"] for one in wordv.values()),
         sum(one["styles_part_valign"] for one in wordv.values()),
         len(deckv), sum(one["cells"] for one in deckv.values()),
         sum(one["cells_with_anchor"] for one in deckv.values()),
         sum(one["cells_silent"] for one in deckv.values()),
         sum(one["cells_with_anchor_ctr"] for one in deckv.values())],
        [95, 313, 8, 103, 2, 0, 33, 70, 40, 30, 1],
    )
    check(
        "ODF 那一头整库一本：49 份 .odt 的 172 个格里 5 格跳得到一枚值，18 份 .odp 的 35 个格**一个都没有** —— 与逐件那一格同一结论",
        [len(odpv), sum(one["cells"] for one in odpv.values()),
         sum(one["cells_with_valign"] for one in odpv.values()),
         len(showv), sum(one["cells"] for one in showv.values()),
         sum(one["cells_with_valign"] for one in showv.values())],
        [51, 172, 5, 18, 35, 0],
    )
    check(
        "反面凭据：这一格只在读了表的三个出口交。遗留 `.doc` 与 `.rtf` 与 `.ppt` 的记录里没有"
        "「这一格贴哪边」这个东西（.doc 的格属性在 `TAP` 之后的 DCX 里、RTF 的表只有格分隔），"
        "所以那三家的整份输出里都找不到 `vertical_align` 这个键；表格那几个出口也不交（.xlsx 的"
        "垂直对齐住在 `alignment/@vertical`，那是 office-sheet 的那一本）",
        [no_theme_key("office-doc", "notes.rtf", "vertical_align"),
         no_theme_key("office-doc", "notes.doc", "vertical_align"),
         no_theme_key("office-sheet", "book.xls", "vertical_align"),
         no_theme_key("office-sheet", "formats.xlsx", "vertical_align"),
         no_theme_key("office-slide", "deck.ppt", "vertical_align"),
         no_theme_key("office-doc", "valign.odt", "vertical_align"),
         no_theme_key("office-slide", "valign.odp", "vertical_align"),
         no_theme_key("office-doc", "valign.docx", "vertical_align"),
         no_theme_key("office-slide", "valign.pptx", "vertical_align")],
        [False, False, False, False, False, True, True, True, True],
    )

    # ── 3bz) 这份稿子有多少字：`docProps/app.xml` 自报的七个名与正文实算的三个口径 ──
    print("=== 3bz) statistics：自报 vs 实算（三个口径都交） ===")
    for name in sorted(one.name for one in FIXTURES.glob("*.pptx")):
        check("%s 这一族多少字与读者一致（页 / 页加备注 / 整包三份实算）" % name,
              dig(lbin("office-slide", fixture(name)), "statistics"),
              files[name]["ooxml"]["statistics"])
    for name in sorted(one.name for one in FIXTURES.glob("*.odp")):
        check("%s 这一族多少字与读者一致（自报那一份只有一条 `object-count`）" % name,
              dig(lbin("office-slide", fixture(name)), "statistics"),
              files[name]["odp"]["statistics"])
    check(
        "`stats.pptx` 的三份账并排（自报那份是手写的，`Slides` 写成正件那个对得上的数）——七个名全写 {'words': 999999, 'paragraphs': 7, 'slides': 2, 'notes': 0, 'hidden_slides': 0, 'mm_clips': 0, 'total_time': 1}、正文只数页部件 {'characters': 61, 'characters_no_spaces': 52, 'words_by_space': 16, 'paragraphs': 9, 'text_atoms': 7}、备注部件那份 {'characters': 10, 'characters_no_spaces': 10, 'words_by_space': 1, 'paragraphs': 1, 'text_atoms': 1}、版式与母版那份 {'characters': 1369, 'characters_no_spaces': 1203, 'words_by_space': 252, 'paragraphs': 99, 'text_atoms': 86}、整包那份 {'characters': 1440, 'characters_no_spaces': 1265, 'words_by_space': 269, 'paragraphs': 109, 'text_atoms': 94}、等号三本 {'words': {'slides': False, 'with_notes': False, 'all': False}, 'paragraphs': {'slides': False, 'all': False}, 'slides': True, 'notes': False}、页部件数 2（备注与其余部件数在同一条的另一半）、第一页那一行 {'characters': 39, 'characters_no_spaces': 33, 'words_by_space': 9, 'paragraphs': 4, 'text_atoms': 3, 'part': 'ppt/slides/slide1.xml', 'has_text': True}",
        [dig(lbin("office-slide", fixture('stats.pptx')), 'statistics.declared'),
         dig(lbin("office-slide", fixture('stats.pptx')), 'statistics.ours'),
         dig(lbin("office-slide", fixture('stats.pptx')), 'statistics.notes'),
         dig(lbin("office-slide", fixture('stats.pptx')), 'statistics.others'),
         dig(lbin("office-slide", fixture('stats.pptx')), 'statistics.all_parts'),
         dig(lbin("office-slide", fixture('stats.pptx')), 'statistics.agree'),
         dig(lbin("office-slide", fixture('stats.pptx')), 'statistics.slide_parts'),
         dig(lbin("office-slide", fixture('stats.pptx')), 'statistics.rows[0]')],
        [{'words': 999999, 'paragraphs': 7, 'slides': 2, 'notes': 0, 'hidden_slides': 0, 'mm_clips': 0, 'total_time': 1},
         {'characters': 61, 'characters_no_spaces': 52, 'words_by_space': 16, 'paragraphs': 9, 'text_atoms': 7},
         {'characters': 10, 'characters_no_spaces': 10, 'words_by_space': 1, 'paragraphs': 1, 'text_atoms': 1},
         {'characters': 1369, 'characters_no_spaces': 1203, 'words_by_space': 252, 'paragraphs': 99, 'text_atoms': 86},
         {'characters': 1440, 'characters_no_spaces': 1265, 'words_by_space': 269, 'paragraphs': 109, 'text_atoms': 94},
         {'words': {'slides': False, 'with_notes': False, 'all': False}, 'paragraphs': {'slides': False, 'all': False}, 'slides': True, 'notes': False},
         2,
         {'characters': 39, 'characters_no_spaces': 33, 'words_by_space': 9, 'paragraphs': 4, 'text_atoms': 3, 'part': 'ppt/slides/slide1.xml', 'has_text': True}],
    )
    check(
        "LibreOffice 重写同一份 pptx：手写的 `Words`/`Paragraphs` 原样搬过去，另四个名整个丢掉——自报只剩三个名 {'words': 999999, 'paragraphs': 7, 'total_time': 1}、字符数一字没动而文字原子被拆细 {'characters': 61, 'characters_no_spaces': 52, 'words_by_space': 16, 'paragraphs': 9, 'text_atoms': 9}、那两个数于是变成 null 而不是 false {'words': {'slides': False, 'with_notes': False, 'all': False}, 'paragraphs': {'slides': False, 'all': False}, 'slides': None, 'notes': None}、备注那份 {'characters': 14, 'characters_no_spaces': 14, 'words_by_space': 2, 'paragraphs': 2, 'text_atoms': 2}",
        [dig(lbin("office-slide", fixture('stats-lo.pptx')), 'statistics.declared'),
         dig(lbin("office-slide", fixture('stats-lo.pptx')), 'statistics.ours'),
         dig(lbin("office-slide", fixture('stats-lo.pptx')), 'statistics.agree'),
         dig(lbin("office-slide", fixture('stats-lo.pptx')), 'statistics.notes')],
        [{'words': 999999, 'paragraphs': 7, 'total_time': 1},
         {'characters': 61, 'characters_no_spaces': 52, 'words_by_space': 16, 'paragraphs': 9, 'text_atoms': 9},
         {'words': {'slides': False, 'with_notes': False, 'all': False}, 'paragraphs': {'slides': False, 'all': False}, 'slides': None, 'notes': None},
         {'characters': 14, 'characters_no_spaces': 14, 'words_by_space': 2, 'paragraphs': 2, 'text_atoms': 2}],
    )
    check(
        "python-pptx 打底那份（没手写）：七个名全写 0，而正文有字——自报 {'words': 0, 'paragraphs': 0, 'slides': 0, 'notes': 0, 'hidden_slides': 0, 'mm_clips': 0, 'total_time': 1}、实算 {'characters': 42, 'characters_no_spaces': 40, 'words_by_space': 10, 'paragraphs': 8, 'text_atoms': 8}、整包 {'characters': 1422, 'characters_no_spaces': 1254, 'words_by_space': 263, 'paragraphs': 108, 'text_atoms': 95}、等号 {'words': {'slides': False, 'with_notes': False, 'all': False}, 'paragraphs': {'slides': False, 'all': False}, 'slides': False, 'notes': False}",
        [dig(lbin("office-slide", fixture('deck.pptx')), 'statistics.declared'),
         dig(lbin("office-slide", fixture('deck.pptx')), 'statistics.ours'),
         dig(lbin("office-slide", fixture('deck.pptx')), 'statistics.all_parts'),
         dig(lbin("office-slide", fixture('deck.pptx')), 'statistics.agree')],
        [{'words': 0, 'paragraphs': 0, 'slides': 0, 'notes': 0, 'hidden_slides': 0, 'mm_clips': 0, 'total_time': 1},
         {'characters': 42, 'characters_no_spaces': 40, 'words_by_space': 10, 'paragraphs': 8, 'text_atoms': 8},
         {'characters': 1422, 'characters_no_spaces': 1254, 'words_by_space': 263, 'paragraphs': 108, 'text_atoms': 95},
         {'words': {'slides': False, 'with_notes': False, 'all': False}, 'paragraphs': {'slides': False, 'all': False}, 'slides': False, 'notes': False}],
    )
    check(
        "odp 这一头：`meta:document-statistic` 只写了一条 `object-count`——那一枚元素的属性 {'object-count': '144'}、整份件实算 {'characters': 75, 'characters_no_spaces': 66, 'words_by_space': 18, 'paragraphs': 9}、按页之和同一个数（备注在页里面）{'characters': 75, 'characters_no_spaces': 66, 'words_by_space': 18, 'paragraphs': 9}、页数 2、第一页 {'characters': 39, 'characters_no_spaces': 33, 'words_by_space': 9, 'paragraphs': 3, 'page': 0, 'name': 'page1', 'klass': None, 'has_text': True}、第二页 {'characters': 36, 'characters_no_spaces': 33, 'words_by_space': 9, 'paragraphs': 6, 'page': 1, 'name': 'page2', 'klass': None, 'has_text': True}",
        [dig(lbin("office-slide", fixture('stats.odp')), 'statistics.declared'),
         dig(lbin("office-slide", fixture('stats.odp')), 'statistics.ours'),
         dig(lbin("office-slide", fixture('stats.odp')), 'statistics.ours_in_pages'),
         dig(lbin("office-slide", fixture('stats.odp')), 'statistics.pages'),
         dig(lbin("office-slide", fixture('stats.odp')), 'statistics.rows[0]'),
         dig(lbin("office-slide", fixture('stats.odp')), 'statistics.rows[1]')],
        [{'object-count': '144'},
         {'characters': 75, 'characters_no_spaces': 66, 'words_by_space': 18, 'paragraphs': 9},
         {'characters': 75, 'characters_no_spaces': 66, 'words_by_space': 18, 'paragraphs': 9},
         2,
         {'characters': 39, 'characters_no_spaces': 33, 'words_by_space': 9, 'paragraphs': 3, 'page': 0, 'name': 'page1', 'klass': None, 'has_text': True},
         {'characters': 36, 'characters_no_spaces': 33, 'words_by_space': 9, 'paragraphs': 6, 'page': 1, 'name': 'page2', 'klass': None, 'has_text': True}],
    )
    check(
        "另一份 odp：页名是标题给的，自报仍然只有那一条——自报 {'object-count': '144'}、实算 {'characters': 57, 'characters_no_spaces': 55, 'words_by_space': 12, 'paragraphs': 11}、第一页的名字 '预算评审'",
        [dig(lbin("office-slide", fixture('deck.odp')), 'statistics.declared'),
         dig(lbin("office-slide", fixture('deck.odp')), 'statistics.ours'),
         dig(lbin("office-slide", fixture('deck.odp')), 'statistics.rows[0].name')],
        [{'object-count': '144'},
         {'characters': 57, 'characters_no_spaces': 55, 'words_by_space': 12, 'paragraphs': 11},
         '预算评审'],
    )
    check(
        "反面凭据（生产者）：`eqs.odp` 整份 `meta.xml` 都没有，于是自报那格是 null 而不是 0——meta 在不在 False、那一枚在不在 False、自报 None、实算照交 {'characters': 10, 'characters_no_spaces': 10, 'words_by_space': 1, 'paragraphs': 1}",
        [dig(lbin("office-slide", fixture('eqs.odp')), 'statistics.statistic_part'),
         dig(lbin("office-slide", fixture('eqs.odp')), 'statistics.statistic_present'),
         dig(lbin("office-slide", fixture('eqs.odp')), 'statistics.declared'),
         dig(lbin("office-slide", fixture('eqs.odp')), 'statistics.ours')],
        [False,
         False,
         None,
         {'characters': 10, 'characters_no_spaces': 10, 'words_by_space': 1, 'paragraphs': 1}],
    )
    check(
        "截行不截账（pptx）：`--limit 1` 只砍 `rows`，三份实算仍按全部页算——列了几行 1、砍没砍 True、实算不变 {'characters': 61, 'characters_no_spaces': 52, 'words_by_space': 16, 'paragraphs': 9, 'text_atoms': 7}、交出来的那一行 {'characters': 39, 'characters_no_spaces': 33, 'words_by_space': 9, 'paragraphs': 4, 'text_atoms': 3, 'part': 'ppt/slides/slide1.xml', 'has_text': True}",
        [dig(lbin("office-slide", fixture('stats.pptx'), '--limit', '1'), 'statistics.listed'),
         dig(lbin("office-slide", fixture('stats.pptx'), '--limit', '1'), 'statistics.cut'),
         dig(lbin("office-slide", fixture('stats.pptx'), '--limit', '1'), 'statistics.ours'),
         dig(lbin("office-slide", fixture('stats.pptx'), '--limit', '1'), 'statistics.rows[0]')],
        [1,
         True,
         {'characters': 61, 'characters_no_spaces': 52, 'words_by_space': 16, 'paragraphs': 9, 'text_atoms': 7},
         {'characters': 39, 'characters_no_spaces': 33, 'words_by_space': 9, 'paragraphs': 4, 'text_atoms': 3, 'part': 'ppt/slides/slide1.xml', 'has_text': True}],
    )
    check(
        "截行不截账（odp）：`--limit 1` 砍掉第二页的行，整份件与按页之和都不动——列了几页 1、砍没砍 True、整份件 {'characters': 75, 'characters_no_spaces': 66, 'words_by_space': 18, 'paragraphs': 9}、按页之和 {'characters': 75, 'characters_no_spaces': 66, 'words_by_space': 18, 'paragraphs': 9}",
        [dig(lbin("office-slide", fixture('stats.odp'), '--limit', '1'), 'statistics.listed'),
         dig(lbin("office-slide", fixture('stats.odp'), '--limit', '1'), 'statistics.cut'),
         dig(lbin("office-slide", fixture('stats.odp'), '--limit', '1'), 'statistics.ours'),
         dig(lbin("office-slide", fixture('stats.odp'), '--limit', '1'), 'statistics.ours_in_pages')],
        [1,
         True,
         {'characters': 75, 'characters_no_spaces': 66, 'words_by_space': 18, 'paragraphs': 9},
         {'characters': 75, 'characters_no_spaces': 66, 'words_by_space': 18, 'paragraphs': 9}],
    )
    def _stat(name, fam):
        had = files.get(name) or {}
        one = had.get(fam)
        return one["statistics"] if isinstance(one, dict) and "statistics" in one else None

    deckst = dict((one, had) for one, _raw in files.items() if one.endswith(".pptx")
                  for had in [_stat(one, "ooxml")] if had)
    odpst = dict((one, had) for one, _raw in files.items() if one.endswith(".odp")
                 for had in [_stat(one, "odp")] if had)
    check(
        "整库摊开（自产件）：" + "份数 pptx 33、带 app.xml 33、页部件之和 80、备注部件之和 4、其余带字部件之和 418、正文实算字符 1107、整包实算字符 44645、正文实算切词 203、Words 写 0 而正文有字 24、Words 与三口径都对 0、Paragraphs 与段数对 0、Slides 对 1、Slides 不对 16、Slides 没写 16、Notes 对 15、Notes 不对 2、Notes 没写 16、份数 odp 18、odp 带那一枚 17、odp 页数之和 43、odp 整份件字符 610、odp 按页与整份同一数 18",
        [len(deckst),
         sum(1 for one in deckst.values() if one["available"]),
         sum(one["slide_parts"] for one in deckst.values()),
         sum(one["notes_parts"] for one in deckst.values()),
         sum(one["other_text_parts"] for one in deckst.values()),
         sum(one["ours"]["characters"] for one in deckst.values()),
         sum(one["all_parts"]["characters"] for one in deckst.values()),
         sum(one["ours"]["words_by_space"] for one in deckst.values()),
         sum(1 for one in deckst.values() if one["declared"] and one["declared"].get("words") == 0 and one["ours"]["characters"] > 0),
         sum(1 for one in deckst.values() if one["agree"]["words"]["all"] is True),
         sum(1 for one in deckst.values() if one["agree"]["paragraphs"]["slides"] is True),
         sum(1 for one in deckst.values() if one["agree"]["slides"] is True),
         sum(1 for one in deckst.values() if one["agree"]["slides"] is False),
         sum(1 for one in deckst.values() if one["agree"]["slides"] is None),
         sum(1 for one in deckst.values() if one["agree"]["notes"] is True),
         sum(1 for one in deckst.values() if one["agree"]["notes"] is False),
         sum(1 for one in deckst.values() if one["agree"]["notes"] is None),
         len(odpst),
         sum(1 for one in odpst.values() if one["statistic_present"]),
         sum(one["pages"] for one in odpst.values()),
         sum(one["ours"]["characters"] for one in odpst.values()),
         sum(1 for one in odpst.values() if one["ours"] == one["ours_in_pages"])],
        [33,
         33,
         80,
         4,
         418,
         1107,
         44645,
         203,
         24,
         0,
         0,
         1,
         16,
         16,
         15,
         2,
         16,
         18,
         17,
         43,
         610,
         18],
    )
    check(
        "反面凭据：这一格只在读了放映那两族的出口交。遗留 `.ppt` 的自报数在 `SummaryInformation` "
        "的属性流里（那是 office-meta 读的那一本），而表格与 PDF 与 office-text 那三个命令根本没有 "
        "`statistics` 这个键（office-doc 那一族的住在 `structure` 下面，所以它说得出口）",
        [no_theme_key("office-slide", "deck.ppt", "statistics"),
         no_theme_key("office-sheet", "book.xlsx", "statistics"),
         no_theme_key("office-pdf", "risk.pdf", "statistics"),
         no_theme_key("office-text", "notes.docx", "statistics"),
         no_theme_key("office-slide", "stats.pptx", "statistics"),
         no_theme_key("office-slide", "stats.odp", "statistics"),
         no_theme_key("office-doc", "valign.docx", "statistics"),
         no_theme_key("office-slide", "deck.odp", "statistics")],
        [False, False, False, False, True, True, True, True],
    )

    # ── 3ca) 内容控件：一枚 `w:sdt` 说「这里可以填」，是哪一种看 `sdtPr` 写了什么 ──────
    print("=== 3ca) content_controls：类型元素、三个名、锁与绑定、正文两个口径 ===")
    for name in sorted(one.name for one in list(FIXTURES.glob("*.docx")) + list(FIXTURES.glob("*.docm"))):
        check("%s 的内容控件账与读者一致（逐枚一行，序按文档序）" % name,
              dig(lbin("office-doc", fixture(name), "--limit", "400"),
                  "structure.content_controls"),
              files[name]["ooxml"]["content_controls"])
    check(
        "`sdt.docx` 十枚手写（含一枚套娃）：类型元素五样、三个名各写各的、锁与绑定与占位都在——共 11 枚、住了 1 个部件、类型词表撞上几枚的分布 {'text': 2, 'richText': 3, 'date': 1, 'dropDownList': 1, '(没有类型元素)': 3, 'docPartObj': 1}、没有类型元素的 3 枚、带 sdtEndPr 的 2 枚、带 dataBinding 的 1 枚、套娃那枚 depth 1 1、第一枚的孩子序 ['alias', 'tag', 'id', 'text', 'placeholder']、下拉那枚的两个候选 ['甲', '乙']、绑定那枚的第四属性 '66666666-7777-8888-9999-000000000000'、目录那一块的 gallery {'Table of Contents': 1}、正文两个口径 11 对 14、表格里那几个格也算 2 格",
        [dig(lbin("office-doc", fixture('sdt.docx')), 'structure.content_controls.controls'),
         dig(lbin("office-doc", fixture('sdt.docx')), 'structure.content_controls.types'),
         dig(lbin("office-doc", fixture('sdt.docx')), 'structure.content_controls.type_none'),
         dig(lbin("office-doc", fixture('sdt.docx')), 'structure.content_controls.rows[7].depth'),
         dig(lbin("office-doc", fixture('sdt.docx')), 'structure.content_controls.rows[0].pr_children'),
         dig(lbin("office-doc", fixture('sdt.docx')), 'structure.content_controls.rows[3].list_values'),
         dig(lbin("office-doc", fixture('sdt.docx')), 'structure.content_controls.rows[4].data_binding.storeSchemaID'),
         dig(lbin("office-doc", fixture('sdt.docx')), 'structure.content_controls.rows[5].gallery'),
         dig(lbin("office-doc", fixture('sdt.docx')), 'structure.content_controls.paras_direct'),
         dig(lbin("office-doc", fixture('sdt.docx')), 'structure.content_controls.cells')],
        [11,
         {'text': 2, 'richText': 3, 'date': 1, 'dropDownList': 1, '(没有类型元素)': 3, 'docPartObj': 1},
         3,
         1,
         ['alias', 'tag', 'id', 'text', 'placeholder'],
         ['甲', '乙'],
         '66666666-7777-8888-9999-000000000000',
         'Table of Contents',
         11,
         2],
    )
    check(
        "LibreOffice 重写同一份：空正文那枚整枚不见、`sdtEndPr` 全丢、`richText` 降级成 `text`——共 10 枚、带 sdtEndPr 的 0 枚、类型分布 {'text': 6, 'date': 1, 'dropDownList': 1, 'docPartObj': 1, '(没有类型元素)': 1}、下拉那枚的 alias 被改写成空串（不是没写，所以 `alias_present` 仍 true）''、同一枚的 id 于是没了 None、日期那枚把文件写的 `dateFormat`/`calendarType` 换成它自己算的一格 {'fullDate': '2026-09-27T00:00:00Z'}、绑定那枚的孩子换了序 ['alias', 'tag', 'id', 'lock', 'showingPlcHdr', 'dataBinding', 'text'] 而它自己的四格属性只剩三格（`storeSchemaID` 没了）{'prefixMappings': 'w: http://x', 'xpath': '/w:document[1]/w:body[2]', 'storeItemID': '{11111111-2222-3333-4444-555555555555}'}、那张表**还在整件里**（`structure.tables` 1）却**已经不在这枚控件里**（控件那一层 `tables_total` 0）、段被摊成 `w:r`：两个口径都是 3 而 runs 23、字 69、套娃那枚外层失去了类型元素 None",
        [dig(lbin("office-doc", fixture('sdt-lo.docx')), 'structure.content_controls.controls'),
         dig(lbin("office-doc", fixture('sdt-lo.docx')), 'structure.content_controls.endpr_present'),
         dig(lbin("office-doc", fixture('sdt-lo.docx')), 'structure.content_controls.types'),
         dig(lbin("office-doc", fixture('sdt-lo.docx')), 'structure.content_controls.rows[3].alias'),
         dig(lbin("office-doc", fixture('sdt-lo.docx')), 'structure.content_controls.rows[3].id'),
         dig(lbin("office-doc", fixture('sdt-lo.docx')), 'structure.content_controls.rows[2].date'),
         dig(lbin("office-doc", fixture('sdt-lo.docx')), 'structure.content_controls.rows[4].pr_children'),
         dig(lbin("office-doc", fixture('sdt-lo.docx')), 'structure.content_controls.rows[4].data_binding'),
         dig(lbin("office-doc", fixture('sdt-lo.docx')), 'structure.content_controls.tables_total'),
         dig(lbin("office-doc", fixture('sdt-lo.docx')), 'structure.tables'),
         dig(lbin("office-doc", fixture('sdt-lo.docx')), 'structure.content_controls.paras_direct'),
         dig(lbin("office-doc", fixture('sdt-lo.docx')), 'structure.content_controls.rows[6].type_seen')],
        [10,
         0,
         {'text': 6, 'date': 1, 'dropDownList': 1, 'docPartObj': 1, '(没有类型元素)': 1},
         '',
         None,
         {'fullDate': '2026-09-27T00:00:00Z'},
         ['alias', 'tag', 'id', 'lock', 'showingPlcHdr', 'dataBinding', 'text'],
         {'prefixMappings': 'w: http://x', 'xpath': '/w:document[1]/w:body[2]', 'storeItemID': '{11111111-2222-3333-4444-555555555555}'},
         0,
         1,
         3,
         None],
    )
    check(
        "截行不截账：`--limit 3` 只砍 `rows`（`listed` 3、`cut` true），那十六本合计一个都不动——列了几枚 3、砍没砍 True、合计仍按全部 11 枚、段的两个口径 11 对 14、交出来的第三行 'date'",
        [dig(lbin("office-doc", fixture('sdt.docx'), '--limit', '3'), 'structure.content_controls.listed'),
         dig(lbin("office-doc", fixture('sdt.docx'), '--limit', '3'), 'structure.content_controls.cut'),
         dig(lbin("office-doc", fixture('sdt.docx'), '--limit', '3'), 'structure.content_controls.controls'),
         dig(lbin("office-doc", fixture('sdt.docx'), '--limit', '3'), 'structure.content_controls.paras_total'),
         dig(lbin("office-doc", fixture('sdt.docx'), '--limit', '3'), 'structure.content_controls.rows[2].type_seen')],
        [3,
         True,
         11,
         14,
         'date'],
    )
    check(
        "模板里那块目录本身就是一枚控件（`w:docPartObj` + gallery），而「有没有目录」那本账另有其一——共 1 枚、类型分布 {'docPartObj': 1}、gallery {'Table of Contents': 1}",
        [dig(lbin("office-doc", fixture('toc.docx')), 'structure.content_controls.controls'),
         dig(lbin("office-doc", fixture('toc.docx')), 'structure.content_controls.types'),
         dig(lbin("office-doc", fixture('toc.docx')), 'structure.content_controls.galleries')],
        [1,
         {'docPartObj': 1},
         {'Table of Contents': 1}],
    )
    check(
        "一份没有控件的件：这一格仍在（`available` true），只是十六本合计都是零 —— 「没有」与「没读」两件事——控件 0 枚、部件 0 个、可用 True",
        [dig(lbin("office-doc", fixture('notes.docx')), 'structure.content_controls.controls'),
         dig(lbin("office-doc", fixture('notes.docx')), 'structure.content_controls.parts_with_controls'),
         dig(lbin("office-doc", fixture('notes.docx')), 'structure.content_controls.available')],
        [0,
         0,
         True],
    )
    def _ctl(name):
        had = files.get(name) or {}
        one = had.get("ooxml")
        return one["content_controls"] if isinstance(one, dict) and "content_controls" in one else None

    ctl = dict((one, had) for one, _raw in files.items() if one.endswith((".docx", ".docm"))
               for had in [_ctl(one)] if had)
    check(
        "整库摊开（自产件）：份 docx/docm 92、带控件的份数 6、控件枚数之和 25、部件数之和 6、套娃枚数 2、没写 sdtPr 的枚数 1、没有类型元素的枚数 5、带 sdtEndPr 的枚数 2、带 dataBinding 的枚数 4、正文两个口径 20 对 23、字数之和 180",
        [len(ctl),
         sum(1 for one in ctl.values() if one["controls"] > 0),
         sum(one["controls"] for one in ctl.values()),
         sum(one["parts_with_controls"] for one in ctl.values()),
         sum(one["nested"] for one in ctl.values()),
         sum(one["pr_missing"] for one in ctl.values()),
         sum(one["type_none"] for one in ctl.values()),
         sum(one["endpr_present"] for one in ctl.values()),
         sum(one["data_binding"] for one in ctl.values()),
         "%d 对 %d" % (sum(one["paras_direct"] for one in ctl.values()), sum(one["paras_total"] for one in ctl.values())),
         sum(one["chars"] for one in ctl.values())],
        [95, 6, 25, 6, 2, 1, 5, 2, 4, '20 对 23', 180]
    )
    check(
        "反面凭据：这一格只住 OOXML 的文字那一族。ODF 的标准那一层没有这个名字（转出去那一份 "
        "`<form:` 零枚），RTF 与遗留 .doc 没有包结构，表与放映那三个出口更没有",
        [no_theme_key("office-doc", "notes.odt", "content_controls"),
         no_theme_key("office-doc", "notes.rtf", "content_controls"),
         no_theme_key("office-doc", "notes.doc", "content_controls"),
         no_theme_key("office-sheet", "book.xlsx", "content_controls"),
         no_theme_key("office-slide", "deck.pptx", "content_controls"),
         no_theme_key("office-doc", "sdt.docx", "content_controls"),
         no_theme_key("office-doc", "sdt.odt", "content_controls"),
         no_theme_key("office-doc", "sdt-lo.docx", "content_controls")],
        [False, False, False, False, False, True, False, True],
    )
    with zipfile.ZipFile(fixture("sdt.odt")) as _box:
        _odt = _box.read("content.xml").decode("utf-8", "replace")
    check(
        "但那一句 False 要说准：ODF 标准没有这一层，而 LibreOffice 把它写进了自己的 `loext:` "
        "扩展命名空间（这一转留着 6 枚 `loext:content-control`、一枚 `loext:lock` 照抄 "
        "`sdtContentLocked`、控件里的字也还在）—— 所以缺的是**我们的 odt 读者不读 loext**，"
        "不是「转出去这一层就没了」",
        [_odt.count("<form:"),
         _odt.count("<loext:content-control"),
         _odt.count('loext:lock="sdtContentLocked"'),
         "绑定来的字" in _odt],
        [0,
         6,
         1,
         True],
    )

    # ── 3db) 这一段是第几级：级别可能写在段上、样式名里、样式自己的 outlineLvl 上，而 9 是正文 ──
    print("=== 3db) outline_levels：三处都交、level 按一条优先序算、9 不加一 ===")
    for name in sorted(one.name for one in list(FIXTURES.glob("*.docx")) + list(FIXTURES.glob("*.docm"))):
        check("%s 的级别账与读者一致（逐段一行，序按文档序）" % name,
              dig(lbin("office-doc", fixture(name), "--limit", "400"),
                  "structure.outline_levels"),
              files[name]["ooxml"]["outline_levels"])
    check(
        "`levels.docx` 十一种形状（号写成数字、级在名字上、两处不一致、9 是正文、断链、空串）——"
        "共 11 段、点了样式的 9 段、名字像标题的 5 段、段自己写级的 3 段、样式自己写的 6 段、"
        "两处不一致的 2 段、写着 9 的 2 段、算得出级的 7 段、样式表里 171 枚样式、"
        "第一行那一枚号是 '1' 而名字是 'heading 1' 所以 level 1 来自 '样式名'、"
        "本地化那一枚名字 '标题 #1' 给 1 而样式给 '3' 所以 level 1 且 conflict true、"
        "名字不像标题那一枚 level 1 来自 '样式自己的 outlineLvl'、"
        "写着 9 的那两行 level 都是 None 而 level_from 一句是 '样式写 9（那是正文）' 一句是 '段上写 9（那是正文）'、"
        "断链那一行 style_found false 而 level_from '(没说)'、空串那一行 style_written 是 '' 而 "
        "level_from '样式写了但没值'",
        [dig(lbin("office-doc", fixture("levels.docx")), "structure.outline_levels.paragraphs"),
         dig(lbin("office-doc", fixture("levels.docx")), "structure.outline_levels.with_style"),
         dig(lbin("office-doc", fixture("levels.docx")), "structure.outline_levels.name_matched"),
         dig(lbin("office-doc", fixture("levels.docx")), "structure.outline_levels.own_written"),
         dig(lbin("office-doc", fixture("levels.docx")), "structure.outline_levels.style_written"),
         dig(lbin("office-doc", fixture("levels.docx")), "structure.outline_levels.conflicts"),
         dig(lbin("office-doc", fixture("levels.docx")), "structure.outline_levels.body_written"),
         dig(lbin("office-doc", fixture("levels.docx")), "structure.outline_levels.resolved"),
         dig(lbin("office-doc", fixture("levels.docx")), "structure.outline_levels.style_missing"),
         dig(lbin("office-doc", fixture("levels.docx")), "structure.outline_levels.styles_seen"),
         dig(lbin("office-doc", fixture("levels.docx")), "structure.outline_levels.levels"),
         dig(lbin("office-doc", fixture("levels.docx")), "structure.outline_levels.rows[0].style_id"),
         dig(lbin("office-doc", fixture("levels.docx")), "structure.outline_levels.rows[0].style_name"),
         dig(lbin("office-doc", fixture("levels.docx")), "structure.outline_levels.rows[0].level"),
         dig(lbin("office-doc", fixture("levels.docx")), "structure.outline_levels.rows[0].level_from"),
         dig(lbin("office-doc", fixture("levels.docx")), "structure.outline_levels.rows[2].style_name"),
         dig(lbin("office-doc", fixture("levels.docx")), "structure.outline_levels.rows[2].style_written"),
         dig(lbin("office-doc", fixture("levels.docx")), "structure.outline_levels.rows[2].level"),
         dig(lbin("office-doc", fixture("levels.docx")), "structure.outline_levels.rows[2].conflict"),
         dig(lbin("office-doc", fixture("levels.docx")), "structure.outline_levels.rows[3].level_from"),
         dig(lbin("office-doc", fixture("levels.docx")), "structure.outline_levels.rows[4].level"),
         dig(lbin("office-doc", fixture("levels.docx")), "structure.outline_levels.rows[4].level_from"),
         dig(lbin("office-doc", fixture("levels.docx")), "structure.outline_levels.rows[7].level_from"),
         dig(lbin("office-doc", fixture("levels.docx")), "structure.outline_levels.rows[9].style_found"),
         dig(lbin("office-doc", fixture("levels.docx")), "structure.outline_levels.rows[9].level_from"),
         dig(lbin("office-doc", fixture("levels.docx")), "structure.outline_levels.rows[10].style_written"),
         dig(lbin("office-doc", fixture("levels.docx")), "structure.outline_levels.rows[10].level_from")],
        [11,
         9,
         5,
         3,
         6,
         2,
         2,
         7,
         1,
         171,
         {"1": 3, "2": 1, "5": 2, "3": 1},
         "1",
         "heading 1",
         1,
         "样式名",
         "标题 #1",
         "3",
         1,
         True,
         "样式自己的 outlineLvl",
         None,
         "样式写 9（那是正文）",
         "段上写 9（那是正文）",
         False,
         "(没说)",
         "",
         "样式写了但没值"],
    )
    check(
        "LibreOffice 重写同一份动了六处：把号 `1`/`21`/`7`/`31` 换成可读的 id、给每一段都点上样式"
        "（于是 `with_style` 9 → 12）、把断链 `999` 修成 `Normal`（`style_missing` 1 → 0）、"
        "把写着 9 的那两枚 `outlineLvl` 整个丢掉（`body_written` 2 → 0）、把样式里 `@w:val=""` 那枚"
        "补成 `0`（那一段于是从「写了但没值」变成第 1 级）、而 `TOC Heading` 那份样式的级也没了——"
        "共 12 段、算得出级的 8 段、两处不一致仍 2 段、第一行点的是 'Normal' 而 level_from '(没说)'、"
        "第三行那个号被换成 '1' 而名字还是 '标题 #1'、第五行 'TOCHeading' 的 style_written 没了、"
        "第八行段自己写的那枚 9 不见了、第十二行 'emptyoutline' 的 style_written 是 '0' 而 level 1",
        [dig(lbin("office-doc", fixture("levels-lo.docx")), "structure.outline_levels.paragraphs"),
         dig(lbin("office-doc", fixture("levels-lo.docx")), "structure.outline_levels.with_style"),
         dig(lbin("office-doc", fixture("levels-lo.docx")), "structure.outline_levels.style_missing"),
         dig(lbin("office-doc", fixture("levels-lo.docx")), "structure.outline_levels.body_written"),
         dig(lbin("office-doc", fixture("levels-lo.docx")), "structure.outline_levels.resolved"),
         dig(lbin("office-doc", fixture("levels-lo.docx")), "structure.outline_levels.conflicts"),
         dig(lbin("office-doc", fixture("levels-lo.docx")), "structure.outline_levels.levels"),
         dig(lbin("office-doc", fixture("levels-lo.docx")), "structure.outline_levels.rows[0].style_id"),
         dig(lbin("office-doc", fixture("levels-lo.docx")), "structure.outline_levels.rows[0].level_from"),
         dig(lbin("office-doc", fixture("levels-lo.docx")), "structure.outline_levels.rows[3].style_id"),
         dig(lbin("office-doc", fixture("levels-lo.docx")), "structure.outline_levels.rows[3].style_name"),
         dig(lbin("office-doc", fixture("levels-lo.docx")), "structure.outline_levels.rows[5].style_id"),
         dig(lbin("office-doc", fixture("levels-lo.docx")), "structure.outline_levels.rows[5].style_written"),
         dig(lbin("office-doc", fixture("levels-lo.docx")), "structure.outline_levels.rows[8].own_written"),
         dig(lbin("office-doc", fixture("levels-lo.docx")), "structure.outline_levels.rows[11].style_written"),
         dig(lbin("office-doc", fixture("levels-lo.docx")), "structure.outline_levels.rows[11].level")],
        [12,
         12,
         0,
         0,
         8,
         2,
         {"1": 4, "2": 1, "5": 2, "3": 1},
         "Normal",
         "(没说)",
         "1",
         "标题 #1",
         "TOCHeading",
         None,
         None,
         "0",
         1],
    )
    check(
        "同一份件、两本账各说一个数：现成的 `headings` 那一本只拿段上那串**号**比 `heading`/`标题` 前缀，"
        "所以这份号写成数字的件它交 0 条，而这一本从样式名那一站把 7 段的级都取回来 —— "
        "LibreOffice 把号换成可读的 id 之后旧那一本突然认到 4 条，两本都不等于「这份没有标题」"
        "（真件 33 份 12525 段里 701 段有级，旧那一本只认到 290）",
        [len(dig(lbin("office-doc", fixture("levels.docx")), "headings") or []),
         dig(lbin("office-doc", fixture("levels.docx")), "structure.outline_levels.resolved"),
         len(dig(lbin("office-doc", fixture("levels-lo.docx")), "headings") or []),
         dig(lbin("office-doc", fixture("levels-lo.docx")), "structure.outline_levels.resolved"),
         len(dig(lbin("office-doc", fixture("notes.docx")), "headings") or []),
         dig(lbin("office-doc", fixture("notes.docx")), "structure.outline_levels.resolved")],
        [0,
         7,
         4,
         8,
         2,
         2],
    )
    check(
        "截行不截账：`--limit 3` 只砍 `rows`（`listed` 3、`cut` true），那十一本合计一个都不动——"
        "列了几段 3、砍没砍 True、段数仍按全部 11、算得出级的仍是 7、交出来的第三行的名字是 '标题 #1'",
        [dig(lbin("office-doc", fixture("levels.docx"), "--limit", "3"), "structure.outline_levels.listed"),
         dig(lbin("office-doc", fixture("levels.docx"), "--limit", "3"), "structure.outline_levels.cut"),
         dig(lbin("office-doc", fixture("levels.docx"), "--limit", "3"), "structure.outline_levels.paragraphs"),
         dig(lbin("office-doc", fixture("levels.docx"), "--limit", "3"), "structure.outline_levels.resolved"),
         dig(lbin("office-doc", fixture("levels.docx"), "--limit", "3"), "structure.outline_levels.rows[2].style_name")],
        [3,
         True,
         11,
         7,
         "标题 #1"],
    )
    def _lv(name):
        had = files.get(name) or {}
        one = had.get("ooxml")
        return one["outline_levels"] if isinstance(one, dict) and "outline_levels" in one else None

    lv = dict((one, had) for one, _raw in files.items() if one.endswith((".docx", ".docm"))
              for had in [_lv(one)] if had)
    check(
        "整库摊开（自产件）：份 docx/docm 92、每一份都交这一格 92、段数之和 527、算得出级的 75、"
        "名字像标题的 70、段自己写级的 5、两处不一致的 4、写着 9 的 4、断链的 10、"
        "级从样式名来的份数 33",
        [len(lv),
         sum(1 for one in lv.values() if one["available"]),
         sum(one["paragraphs"] for one in lv.values()),
         sum(one["resolved"] for one in lv.values()),
         sum(one["name_matched"] for one in lv.values()),
         sum(one["own_written"] for one in lv.values()),
         sum(one["conflicts"] for one in lv.values()),
         sum(one["body_written"] for one in lv.values()),
         sum(one["style_missing"] for one in lv.values()),
         sum(1 for one in lv.values() if one["froms"].get("样式名"))],
        [95, 95, 556, 75, 70, 5, 4, 4, 10, 33],
    )
    check(
        "反面凭据：这一格只住 OOXML 的文字那一族。ODF 把级写在**段落样式**的 "
        "`style:paragraph-properties/@fo:font-weight` 之外另有 `text:outline-level` 一条路，"
        "而 RTF 写在样式名 `\\sN` 指向的那条 `\\toc N` 上，遗留 .doc 与表与放映那几个出口更没有这一问",
        [no_theme_key("office-doc", "notes.odt", "outline_levels"),
         no_theme_key("office-doc", "notes.rtf", "outline_levels"),
         no_theme_key("office-doc", "notes.doc", "outline_levels"),
         no_theme_key("office-sheet", "book.xlsx", "outline_levels"),
         no_theme_key("office-slide", "deck.pptx", "outline_levels"),
         no_theme_key("office-doc", "levels.docx", "outline_levels"),
         no_theme_key("office-doc", "levels.odt", "outline_levels"),
         no_theme_key("office-doc", "levels-lo.docx", "outline_levels")],
        [False, False, False, False, False, True, False, True],
    )

    print("=== 3dc) workbook_settings：这一本工作簿自己的设置（谁存的、算不算、停在哪一张）===")
    for name in sorted(one.name for one in FIXTURES.glob("*.xlsx")):
        check("%s 工作簿那一层的账整本与读者一致（在场、写了哪几格、值原样）" % name,
              dig(lbin("office-sheet", fixture(name)), "workbook_settings"),
              files[name]["ooxml"]["workbook_settings"])
    mine = dig(lbin("office-sheet", fixture("workbook-settings.xlsx"), "--limit", "400"),
               "workbook_settings")
    check("手写那一份把这一层的四种说法一次摆开：两枚 fileVersion、manual 模式、"
          "迭代的 1/true 两拼、停在三张表里的第三张、一枚没人点的共享视图",
          [len(mine["file_versions"]), mine["elements"]["calcPr"]["calcMode"],
           mine["elements"]["calcPr"]["refMode"], mine["elements"]["calcPr"]["iterate"],
           mine["elements"]["calcPr"]["fullCalcOnLoad"],
           mine["views"][0]["attrs"]["activeTab"],
           mine["counts"]["customWorkbookView"], mine["counts"]["fileVersion"],
           mine["element_names"], mine["empty_elements"],
           [sorted(one) for one in sorted(mine["boolean_spellings"].items())
            if one[0] in ("backupFile", "autoFilterDateGrouping", "iterate", "minimized")]],
          [2, "manual", "row", "true", "1", "2", 1, 2,
           ["bookViews", "calcPr", "customWorkbookViews", "workbookPr"],
           ["customWorkbookViews"],
           [["autoFilterDateGrouping", ["false"]], ["backupFile", ["1"]],
            ["iterate", ["true"]], ["minimized", ["0"]]]])
    back = dig(lbin("office-sheet", fixture("workbook-settings-lo.xlsx"), "--limit", "400"),
               "workbook_settings")
    check("LibreOffice 重写同一份，这一层每一处都换了写法：两枚 fileVersion 合成一枚而 appName "
          "换成 Calc、calcMode 与 calcId 与 fullCalcOnLoad 三格不见、refMode 从 row 变成 A1"
          "（**同一格两个意思**，两边都按原样交而不判）、workbookPr 那四格换成它自己补的三格而 "
          "date1904 是其中唯一与别人同名的一枚、共享视图整层不见，只有「停在哪一张」原样穿过",
          [back["file_versions"], back["elements"]["calcPr"],
           sorted(back["attrs_written"]["workbookPr"]),
           back["counts"]["customWorkbookView"], back["counts"]["fileVersion"],
           back["element_names"], back["empty_elements"],
           back["views"][0]["attrs"]["activeTab"],
           back["views"][0]["attrs"]["windowWidth"],
           sorted(back["boolean_spellings"].items())],
          [[{"appName": "Calc", "lowestEdited": "5"}],
           {"iterateCount": "200", "refMode": "A1", "iterate": "true",
            "iterateDelta": "0.0005"},
           ["backupFile", "date1904", "showObjects"], 0, 1,
           ["bookViews", "calcPr", "workbookPr"], [], "2", "16384",
           [["backupFile", {"false": 1}], ["date1904", {"false": 1}],
            ["firstSheet", {"0": 1}], ["iterate", {"true": 1}],
            ["showHorizontalScroll", {"true": 1}], ["showSheetTabs", {"true": 1}],
            ["showVerticalScroll", {"true": 1}], ["xWindow", {"0": 1}],
            ["yWindow", {"0": 1}]]])
    plain = dig(lbin("office-sheet", fixture("book.xlsx"), "--limit", "400"), "workbook_settings")
    check("默认那一份就是「没写」的那一形：`workbookPr` **在场而一个属性都没有**（空壳是一句说过的话），"
          "没有 fileVersion、没有迭代，`definedName` 那本另有一枚",
          [plain["elements"]["workbookPr"], plain["empty_elements"],
           plain["counts"]["fileVersion"], plain["counts"]["definedName"],
           plain["counts"]["customWorkbookView"], plain["file_versions"],
           sorted(one for one in plain["boolean_spellings"]
                  if len(plain["boolean_spellings"][one]) > 1)],
          [{}, ["workbookPr"], 0, 1, 0, [], []])
    check("反面凭据：这一格只住 OOXML 的表格那一家。ODF 没有 `workbookPr` / `fileVersion` / `calcPr` "
          "这三枚元素，同类问题写在 `settings.xml` 的 `ooo:configuration-settings` 那一组里"
          "（`AutoCalculate` 与 `SyntaxStringRef` 15/15 份 .ods 全写，而迭代那三格一份都不写；其中 14 份的 `AutoCalculate` 写 true，从这一族转出去的那一份写 false（xlsx 写了 `calcMode` manual 是唯一穿过转换的一格），而那一份还多带一整格 `CodeName`，条数因此 39 变 40 —— "
          "转格式丢掉的事），那一本的名字与住处由排版兼容那条账交代；遗留 .xls 把计算模式记在 "
          "BIFF 的 DBSTAT / CALCCOUNT 里，本机没有第二个读者能核对那些字段偏移",
          [no_theme_key("office-sheet", "workbook-settings.ods", "workbook_settings"),
           no_theme_key("office-sheet", "book.ods", "workbook_settings"),
           no_theme_key("office-sheet", "book.xls", "workbook_settings"),
           no_theme_key("office-doc", "notes.docx", "workbook_settings"),
           no_theme_key("office-slide", "deck.pptx", "workbook_settings"),
           no_theme_key("office-sheet", "workbook-settings.xlsx", "workbook_settings"),
           no_theme_key("office-sheet", "workbook-settings-lo.xlsx", "workbook_settings")],
          [False, False, False, False, False, True, True])
    decks = dict((one, had["ooxml"]["workbook_settings"]) for one in files
                 if one.endswith(".xlsx") and "workbook_settings" in had.get("ooxml", {}))
    print("=== 3dc 汇总：语料 %d 份 xlsx 的工作簿层 ===" % len(decks))
    check("整库摊开（自产件）：" + "、".join([
        "份数 %d" % len(decks),
        "带 fileVersion %d" % sum(1 for one in decks.values() if one["counts"]["fileVersion"]),
        "fileVersion 枚数之和 %d" % sum(one["counts"]["fileVersion"] for one in decks.values()),
        "写了 calcPr %d" % sum(1 for one in decks.values() if "calcPr" in one["element_names"]),
        "写了 iterate %d" % sum(1 for one in decks.values()
                                if "iterate" in one["elements"].get("calcPr", {})),
        "其中 true %d" % sum(1 for one in decks.values()
                             if one["elements"].get("calcPr", {}).get("iterate") == "true"),
        "空壳 workbookPr 的份数 %d" % sum(1 for one in decks.values()
                                       if "workbookPr" in one["empty_elements"]),
        "带共享视图 %d" % sum(1 for one in decks.values() if one["counts"]["customWorkbookView"]),
        "写了 workbookView %d" % sum(1 for one in decks.values()
                                     if one["counts"]["workbookView"] >= 1),
        "workbookPr 写了 date1904 %d" % sum(1 for one in decks.values()
                                          if "date1904" in one["elements"].get("workbookPr", {})),
    ]),
        [len(decks),
         sum(1 for one in decks.values() if one["counts"]["fileVersion"]),
         sum(one["counts"]["fileVersion"] for one in decks.values()),
         sum(1 for one in decks.values() if "calcPr" in one["element_names"]),
         sum(1 for one in decks.values() if "iterate" in one["elements"].get("calcPr", {})),
         sum(1 for one in decks.values()
             if one["elements"].get("calcPr", {}).get("iterate") == "true"),
         sum(1 for one in decks.values() if "workbookPr" in one["empty_elements"]),
         sum(1 for one in decks.values() if one["counts"]["customWorkbookView"]),
         sum(1 for one in decks.values() if one["counts"]["workbookView"] >= 1),
         sum(1 for one in decks.values()
             if "date1904" in one["elements"].get("workbookPr", {}))],
        [41, 19, 20, 41, 19, 2, 21, 1, 41, 19],
    )
    check("同一层里「两种拼法」与「同一格两个意思」都在语料里数得出来："
          "fileVersion 的枚数只有 0 / 1 / 2 三种，`refMode` 只出现 `-`（没写）、`A1` 与 `row` 两种",
        sorted({str(one["counts"]["fileVersion"]) for one in decks.values()})
        + sorted({one["elements"].get("calcPr", {}).get("refMode", "-") for one in decks.values()}),
        ["0", "1", "2", "-", "A1", "row"])

    # ── 3de) 中文排版那九枚段开关：三处住处、裸写与空串是两句话、字侧那枚另交一本 ──
    print("=== 3de) cjk_typography：段上九枚（docx/docm）与 ODF 那四枚近亲 ===")
    CJK_W = ["kinsoku", "wordWrap", "overflowPunct", "autoSpaceDE", "autoSpaceDN",
             "adjustRightInd", "snapToGrid", "contextualSpacing", "textAlignment"]
    CJK_O = ["contextual-spacing", "line-break", "punctuation-wrap", "snap-to-layout-grid"]

    def cjk_shape(mine):
        """把一份段的账摊成一行可钉的数（整本那一条已经逐格比过，这里钉的是说法）"""
        return [mine["paragraphs_total"], mine["paragraphs_with_any"], mine["p_pr_elements"],
                mine["conflicts"], mine["styles_seen"],
                [[one, mine["words_written"][one]["written"], mine["words_written"][one]["bare"],
                  mine["words_written"][one]["empty_val"]] for one in CJK_W
                 if mine["words_written"][one]["written"]],
                [[one, n] for one, n in sorted(mine["style_written"].items()) if n],
                [one for one in CJK_W if mine["defaults_written"].get(one)],
                mine["paragraphs_see_defaults"],
                [mine["run_no_proof"]["runs"], mine["run_no_proof"]["paragraphs"],
                 [[k, v] for k, v in sorted(mine["run_no_proof"]["values"].items())]],
                mine["paragraphs_indexed"],
                [row["index"] for row in mine["paragraphs"] if row["conflict_with_style"]]]

    def odf_shape(mine):
        return [mine["paragraphs_total"], mine["paragraphs_with_any"], mine["styles_seen"],
                [[one, mine["words_written"][one]["written"],
                  [[k, v] for k, v in sorted(mine["words_written"][one]["values"].items())]]
                 for one in CJK_O if mine["words_written"][one]["written"]],
                mine["paragraphs_indexed"]]

    def cjk_cells(rows):
        """整库摊开：每枚 [段上次数, 裸写, 空串, 有此枚的份数, 样式那跳次数, 打架的段数]"""
        out = dict((word, [0, 0, 0, 0, 0, 0]) for word in CJK_W)
        out["run_no_proof"] = [0, 0, 0, 0, 0, 0]
        for mine in rows:
            for word in CJK_W:
                had = mine["words_written"][word]
                cell = out[word]
                cell[0] += had["written"]
                cell[1] += had["bare"]
                cell[2] += had["empty_val"]
                cell[3] += 1 if had["written"] else 0
                cell[4] += mine["style_written"][word]
                cell[5] += sum(1 for row in mine["paragraphs"] if row["conflict_with_style"])
            cell = out["run_no_proof"]
            cell[0] += mine["run_no_proof"]["runs"]
            cell[1] += mine["run_no_proof"]["paragraphs"]
            cell[3] += 1 if mine["run_no_proof"]["runs"] else 0
        return out

    cjk_rows = []
    for name in sorted(one.name for one in list(FIXTURES.glob("*.docx"))
                       + list(FIXTURES.glob("*.docm"))):
        got = dig(lbin("office-doc", fixture(name)), "structure.cjk_typography")
        check("%s 的段开关九枚整本与读者一致（三处都交、值按字面、字侧另交一本）" % name,
              got, files[name]["ooxml"]["cjk_typography"])
        cjk_rows.append(got)
    cjk_odf = []
    for name in sorted(one.name for one in FIXTURES.glob("*.odt")):
        got = dig(lbin("office-doc", fixture(name)), "structure.cjk_typography")
        check("%s 的 ODF 那四枚近亲整本与读者一致（段只看正文，一跳在样式）" % name,
              got, files[name]["odt"]["cjk_typography"])
        cjk_odf.append(got)

    check("手写那一份把九枚一次摆开：20 段里 16 段段上写着、18 枚 pPr，autoSpaceDE 六枚里有一枚**写了空串**，"
          "docDefaults 那处只有 adjustRightInd 一枚（真件 129 份**一处都不写**，所以这一枚是合成的），"
          "字侧三 run 交两枚 noProof（一枚裸、一枚 0），打架的两段正是段与样式都写而不一致的那两段（15、16）",
          cjk_shape(dig(lbin("office-doc", fixture("cjk-switches.docx")),
                        "structure.cjk_typography")),
          [20, 16, 18, 2, 166,
           [["kinsoku", 2, 1, 0], ["wordWrap", 3, 1, 0], ["overflowPunct", 2, 1, 0],
            ["autoSpaceDE", 6, 2, 1], ["autoSpaceDN", 2, 1, 0], ["adjustRightInd", 2, 2, 0],
            ["snapToGrid", 3, 1, 0], ["contextualSpacing", 3, 2, 0], ["textAlignment", 3, 0, 0]],
           [["autoSpaceDE", 3], ["contextualSpacing", 3], ["snapToGrid", 3],
            ["textAlignment", 3], ["wordWrap", 1]],
           ["adjustRightInd"], 20,
           [2, 1, [["0", 1], ["<没写 val>", 1]]],
           [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
           [15, 16]])
    check("LibreOffice 重写同一份在这一层动得最狠：kinsoku / wordWrap（含那枚 off）/ autoSpaceDE / "
          "autoSpaceDN / adjustRightInd / 字侧 noProof **六处整个不再写**，docDefaults 那枚也不留；"
          "overflowPunct 两枚都改写成 false，snapToGrid 的裸写与 0 与 1 换成 true true false，"
          "contextualSpacing 显式关掉的那一枚不见，只有 textAlignment 的三个枚举值一字不动穿过",
          cjk_shape(dig(lbin("office-doc", fixture("cjk-switches-lo.docx")),
                        "structure.cjk_typography")),
          [20, 7, 20, 1, 170,
           [["overflowPunct", 2, 0, 0], ["snapToGrid", 3, 0, 0],
            ["contextualSpacing", 2, 2, 0], ["textAlignment", 3, 0, 0]],
           [["contextualSpacing", 3], ["snapToGrid", 3], ["textAlignment", 3]],
           [], 0, [0, 0, []], [6, 9, 10, 11, 13, 14, 15], [15]])
    check("反方向那一份（手写的 .odt 转回 docx）只搬得动四件事：kinsoku 与 overflowPunct 与 snapToGrid "
          "与裸写的 contextualSpacing，auto-space 两枚、adjustRightInd、wordWrap、textAlignment、noProof **全是零** "
          "—— 两族之间没有一一对应可走；样式表只剩一份（模板那份），所以那一跳一次都没走到",
          cjk_shape(dig(lbin("office-doc", fixture("cjk-odf-lo.docx")),
                        "structure.cjk_typography")),
          [5, 3, 5, 0, 1,
           [["kinsoku", 1, 0, 0], ["overflowPunct", 2, 0, 0], ["snapToGrid", 1, 0, 0],
            ["contextualSpacing", 1, 1, 0]],
           [], [], 0, [0, 0, []], [0, 1, 2], []])
    check("ODF 手写那一份四枚都摆开：两枚从样式一跳拿到 contextual-spacing（true 与 false 各一段）、"
          "line-break 只有 strict 一枚、punctuation-wrap 两枚（hanging 与 simple）、"
          "snap-to-layout-grid 那一枚是**规范有而本机 64 份真件一条都不写**的形状；"
          "最后一段连样式名都不点，所以四枚全不在场",
          odf_shape(dig(lbin("office-doc", fixture("cjk-odf.odt")), "structure.cjk_typography")),
          [5, 3, 4,
           [["contextual-spacing", 2, [["false", 1], ["true", 1]]],
            ["line-break", 1, [["strict", 1]]],
            ["punctuation-wrap", 2, [["hanging", 1], ["simple", 1]]],
            ["snap-to-layout-grid", 1, [["false", 1]]]],
           [0, 1, 2]])
    check("docx 转 ODF 那一转把九枚换成另一套词：contextual-spacing 逐段补满（15 段，false 11 / true 4）、"
          "snap-to-layout-grid 五段（false 3 / true 2）、punctuation-wrap 两枚都是 simple，"
          "而 line-break **一段都没有**（真件那 123 枚 strict 写在没人点的样式上，按段解出来就是零），"
          "第 9 段一处不写 —— 段上没写不等于这一族不写，那一格在样式表那本里",
          odf_shape(dig(lbin("office-doc", fixture("cjk-switches.odt")),
                        "structure.cjk_typography")),
          [20, 18, 50,
           [["contextual-spacing", 15, [["false", 11], ["true", 4]]],
            ["punctuation-wrap", 2, [["simple", 2]]],
            ["snap-to-layout-grid", 5, [["false", 3], ["true", 2]]]],
           [0, 1, 2, 3, 4, 5, 6, 7, 8, 11, 12, 13, 14, 15, 16, 17, 18, 19]])
    cells = cjk_cells(cjk_rows)
    check("整库摊开（自产件 " + str(len(cjk_rows)) + " 份 docx/docm）：" + "、".join([
        "%s 段上 %d 次 / 样式那跳 %d 次" % (one, cells[one][0], cells[one][4])
        for one in ("kinsoku", "overflowPunct", "autoSpaceDE", "contextualSpacing")]) +
        "；段上有任何一枚的只有 3 份（全是本仓手写的），而样式那一跳有 13 份 —— 真件那边九枚**段上一次都没写**",
        cells,
        {"kinsoku": [3, 1, 0, 2, 44, 3], "wordWrap": [3, 1, 0, 1, 1, 3],
         "overflowPunct": [6, 1, 0, 3, 44, 3], "autoSpaceDE": [6, 2, 1, 1, 47, 3],
         "autoSpaceDN": [2, 1, 0, 1, 0, 3], "adjustRightInd": [2, 2, 0, 1, 0, 3],
         "snapToGrid": [7, 1, 0, 3, 6, 3], "contextualSpacing": [6, 5, 0, 3, 27, 3],
         "textAlignment": [6, 0, 0, 2, 6, 3], "run_no_proof": [2, 1, 0, 1, 0, 0]})
    odf_tally = dict((word, [0, 0, 0, 0]) for word in CJK_O)
    for mine in cjk_odf:
        for word in CJK_O:
            had = mine["words_written"][word]
            cell = odf_tally[word]
            cell[0] += had["written"]
            cell[1] += 1 if had["written"] else 0
            for k, v in had["values"].items():
                if k == "false":
                    cell[2] += v
                elif k == "true":
                    cell[3] += v
    check("整库摊开（自产件 " + str(len(cjk_odf)) + " 份 odt）：contextual-spacing 按段解出 %d 枚"
          "（false %d / true %d，带它的 %d 份）—— LibreOffice 写 odt 时把这一枚**逐段补成 false**，"
          "所以这一族里「段上没写」是少数；line-break 只有 1 枚 strict；punctuation-wrap %d 枚里 "
          "hanging 6 / simple 5；snap-to-layout-grid 6 枚全在本仓手写的两份里" % (
              odf_tally["contextual-spacing"][0], odf_tally["contextual-spacing"][2],
              odf_tally["contextual-spacing"][3], odf_tally["contextual-spacing"][1],
              odf_tally["punctuation-wrap"][0]),
        odf_tally,
        {"contextual-spacing": [377, 46, 372, 5], "line-break": [1, 1, 0, 0],
         "punctuation-wrap": [11, 6, 0, 0], "snap-to-layout-grid": [6, 2, 4, 2]})
    check("反面凭据：这一格只住 `w:pPr` 与 ODF 的段落样式那两路。RTF 没有对应的控制词、遗留 .doc 的在 "
          "SEPX 与样式流的位段里（本机没有第二个读者能核对那些位），而表格与演示那两家根本没有段开关这一层"
          "—— 那五份出口的账本里这个键**整个不在场**，而不是交一份零账",
          [no_theme_key("office-doc", "notes.rtf", "cjk_typography"),
           no_theme_key("office-doc", "notes.doc", "cjk_typography"),
           no_theme_key("office-sheet", "book.xlsx", "cjk_typography"),
           no_theme_key("office-slide", "deck.pptx", "cjk_typography"),
           no_theme_key("office-doc", "cjk-switches.odt", "cjk_typography")],
          [False, False, False, False, True])

    # ── 3bf) 单元格样式自己那两枚锁定位：三本容器、拼法按层量、写了名而值是空串也算写了 ──
    print("=== 3bf) cell_locks：xf / dxf 里的 protection，逐本与第二读者对（只在 xlsx 交）===")

    def locks_shape(d):
        """一份锁定位账摊成一行可钉的数（整本那一条已逐格比过，这里钉的是说法）"""
        return [d["protection_total"], d["empty_elements"],
                [[one["name"], one["children_total"], one["protection_elements"]]
                 for one in d["containers"]],
                [[k, d["attrs_written"][k]] for k in sorted(d["attrs_written"])],
                [[k, sorted(d["values"][k].items())] for k in sorted(d["values"])],
                d["on_formats"], d["unknown_attrs"]]

    lock_rows = []
    for name in sorted(one.name for one in FIXTURES.glob("*.xlsx")):
        got = dig(lbin("office-sheet", fixture(name)), "cell_locks")
        check("%s 的锁定位整本与读者一致（三本容器各记一笔、值按字面、认不出的属性进 unknown）" % name,
              got, files[name]["ooxml"]["cell_locks"])
        lock_rows.append(got)

    check("手写那一份把这一层能分开的几件事摆开：五枚里一枚**空的** `<protection/>`、"
          "`locked` 三种写法各一枚（`1`、空串、`true`），`hidden` 两样（`0` 与 `false` 与 `true`），"
          "多一枚本层没人写过的 `lockRule`（进 `unknown_attrs` 而不是丢），"
          "`dxfs` 那本在场而**一枚都不写**，四枚 `cellXfs` 的下标逐条交出来",
          locks_shape(dig(lbin("office-sheet", fixture("cell-locks.xlsx")), "cell_locks")),
          [5, 1, [["cellStyleXfs", 3, 1], ["cellXfs", 8, 4], ["dxfs", 1, 0]],
           [["hidden", 3], ["lockRule", 1], ["locked", 4]],
           [["hidden", [["0", 1], ["false", 1], ["true", 1]]],
            ["lockRule", [["all", 1]]],
            ["locked", [["", 1], ["1", 1], ["true", 2]]]],
           [["cellStyleXfs", 0], ["cellXfs", 0], ["cellXfs", 1], ["cellXfs", 2], ["cellXfs", 3]],
           ["lockRule"]])
    check("LibreOffice 重写同一份把这一层**补齐**：cellXfs 八枚全写（手写只有四枚）、样式那本从三枚变二十二枚而写三枚，"
          "拼法全换成 true/false（`1` 与 `0` 两枚穿过转写就不在），空的那枚整个不见，"
          "`lockRule` 也不留 —— 于是 unknown 那一格交空表，是它真的没有，不是没读",
          locks_shape(dig(lbin("office-sheet", fixture("cell-locks-lo.xlsx")), "cell_locks")),
          [11, 0, [["cellStyleXfs", 22, 3], ["cellXfs", 8, 8], ["dxfs", 1, 0]],
           [["hidden", 11], ["locked", 11]],
           [["hidden", [["false", 10], ["true", 1]]], ["locked", [["false", 1], ["true", 10]]]],
           [["cellStyleXfs", 0], ["cellStyleXfs", 20], ["cellStyleXfs", 21],
            ["cellXfs", 0], ["cellXfs", 1], ["cellXfs", 2], ["cellXfs", 3],
            ["cellXfs", 4], ["cellXfs", 5], ["cellXfs", 6], ["cellXfs", 7]],
           []])
    check("真件那一对把「表锁了」与「格式设了锁定位」分开：openpyxl 那份表锁着而这一层**一枚都不写**"
          "（cellXfs 两枚全空），LibreOffice 重写同一份时补成四枚、全是 locked=true / hidden=false 这一对默认值，"
          "而 `dxfs` 那本照样零枚 —— 所以「零枚」在这一族是常态，与「这一族没这一层」是两句话",
          [locks_shape(dig(lbin("office-sheet", fixture("locked-sheet.xlsx")), "cell_locks")),
           locks_shape(dig(lbin("office-sheet", fixture("locked-sheet-lo.xlsx")), "cell_locks"))],
          [[0, 0, [["cellStyleXfs", 1, 0], ["cellXfs", 2, 0]], [], [], [], []],
           [4, 0, [["cellStyleXfs", 20, 1], ["cellXfs", 3, 3], ["dxfs", 2, 0]],
            [["hidden", 4], ["locked", 4]],
            [["hidden", [["false", 4]]], ["locked", [["true", 4]]]],
            [["cellStyleXfs", 0], ["cellXfs", 0], ["cellXfs", 1], ["cellXfs", 2]], []]])
    lock_written = [one for one in lock_rows if one["protection_total"]]
    container_names = sorted({c["name"] for row in lock_rows for c in row["containers"]})
    per_container = dict((k, sum(c["protection_elements"] for row in lock_rows
                                 for c in row["containers"] if c["name"] == k))
                         for k in container_names)
    dxfs_books = sum(1 for row in lock_rows
                     if any(c["name"] == "dxfs" for c in row["containers"]))
    spelling_rows = {}
    for row in lock_rows:
        for key, book in row["values"].items():
            for val, n in book.items():
                if key in ("locked", "hidden"):
                    spelling_rows.setdefault(val, 0)
                    spelling_rows[val] += n
    check("整库摊开（自产件 %d 份 .xlsx）：%d 份写这一层共 %d 枚，按本分是 cellStyleXfs %d / cellXfs %d / dxfs %d；"
          "值只有两种拼法在场（true %d / false %d），而 1 与 0 与空串那三种只出现在本仓手写的那一份里 —— "
          "「有这一本」的件数 %d，其中写满每一枚格式的只有 LibreOffice 那一路"
          % (len(lock_rows), len(lock_written), sum(one["protection_total"] for one in lock_rows),
             per_container.get("cellStyleXfs", 0), per_container.get("cellXfs", 0),
             per_container.get("dxfs", 0), spelling_rows.get("true", 0), spelling_rows.get("false", 0),
             dxfs_books),
          [len(lock_rows), len(lock_written), sum(one["protection_total"] for one in lock_rows),
           sum(one["empty_elements"] for one in lock_rows),
           [[k, per_container[k]] for k in sorted(per_container)],
           sorted(spelling_rows.items()),
           sum(len(one["unknown_attrs"]) for one in lock_rows),
           sorted({one["part"] for one in lock_rows})],
          [43, 20, 79, 1, [["cellStyleXfs", 22], ["cellXfs", 57], ["dxfs", 0]],
           [("", 1), ("0", 1), ("1", 1), ("false", 75), ("true", 77)], 1, ["xl/styles.xml"]])
    check("反面凭据：这一层只住 OOXML 表格那一家。`.ods` 没有格式级的锁定属性（ODF 的锁只在 `table:table` 那一层，"
          "已由 `protection` 那一本交），`.xls` 的位在 BIFF 的 `XF` 记录里（本机没有第二个读者能核对那些位段），"
          "而 Word 与演示那两家根本没有 cellXfs 这本 —— 那四份出口的账本里这个键**整个不在场**，而不是交一份零账",
          [no_theme_key("office-sheet", "book.ods", "cell_locks"),
           no_theme_key("office-sheet", "book.xls", "cell_locks"),
           no_theme_key("office-doc", "notes.docx", "cell_locks"),
           no_theme_key("office-slide", "deck.pptx", "cell_locks"),
           no_theme_key("office-sheet", "cell-locks.xlsx", "cell_locks")],
          [False, False, False, False, True])


    failed = [one for one in RESULTS if not one[1]]
    print(f"=== 合计 {len(RESULTS)} 项，失败 {len(failed)} 项 ===")
    for name, _, detail in failed:
        print(f"  FAIL {name}: {detail}")
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
