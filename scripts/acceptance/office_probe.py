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
    elsewhere = lbin("office-text", fixture("notes.rtf"), "--markdown")
    check(
        "没开 `--markdown` 就整个键都不给（不给一份空串装作渲染过）；开了而这一族还没搬的那一份，"
        "键也不在，只在 notes 里说一句",
        [quiet.get("markdown"), elsewhere.get("markdown"),
         any("markdown" in str(one) for one in (dig(elsewhere, "notes") or []))],
        [None, None, True],
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
        "那一族的形状、段号与页级三格都在 3aw 那一本上对。`.ppt` 仍旧不交这个键，但这是量过的：",
        "odp → ppt 那一转把式子变成 `Pictures` 里的一笔位图，容器里连 `ObjectPool` 都没有",
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
        [lbin("office-doc", fixture("eq.docx")).get("equations", {}).get("equations_total"),
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

    failed = [one for one in RESULTS if not one[1]]
    print(f"=== 合计 {len(RESULTS)} 项，失败 {len(failed)} 项 ===")
    for name, _, detail in failed:
        print(f"  FAIL {name}: {detail}")
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
