#!/usr/bin/env python3
"""MS-XLSX 的数字格式：一个格子到底是「日期」还是「数」，账全在 `xl/styles.xml` 里。

这是办公文件的第二读者之一（另外两位：`office_reader.py` 与 `lyco_legacy.py` /
`lyco_rtf.py`），与 `lilyco-binfmt/src/office_sheet.rs` 是同一套规范的两份实现：
两边对同一份 openpyxl / LibreOffice 写的 xlsx 必须给出同样的格式号、格式串、
类型判定与换算出来的日期。CI 的 `office_probe.py` 会把 `lbin` 与它逐字段对账。

三处只有踩过才会写进注释的地方：
1. **格子上的 `s=` 是 `cellXfs` 的下标，不是格式号。** 少绕这一层，日期就永远是
   「一个数」。
2. **`m` 跟在时间标记后面是分钟，不是月份。** `h:mm` 是时间；`yyyy-mm` 才是日期。
   格式串里引号中的字面量（`yyyy"年"`）也不参与判定 —— 不剥掉就会把汉字当格式。
3. **1900 系统里第 60 号是那个不存在的 1900-02-29**（Excel 的闰年 bug），
   而 `date1904="1"` 的工作簿整套基准都不一样。序列数与日期之间没有唯一的换算，
   除非先看 `xl/workbook.xml` 怎么说。
"""

from __future__ import annotations

import datetime as dt
import zipfile
from pathlib import Path
from xml.etree import ElementTree as ET

# ECMA-376 / MS-XLSX 2.1.310 的内置格式号：文件里只写号、不写串，所以这张表得带着
BUILTIN_NUMFMT: dict = {
    0: "General",
    1: "0",
    2: "0.00",
    3: "#,##0",
    4: "#,##0.00",
    5: "$#,##0_);($#,##0)",
    6: "$#,##0_);[Red]($#,##0)",
    7: "$#,##0.00_);($#,##0.00)",
    8: "$#,##0.00_);[Red]($#,##0.00)",
    9: "0%",
    10: "0.00%",
    11: "0.00E+00",
    12: "# ?/?",
    13: "# ??/??",
    14: "mm-dd-yy",
    15: "d-mmm-yy",
    16: "d-mmm",
    17: "mmm-yy",
    18: "h:mm AM/PM",
    19: "h:mm:ss AM/PM",
    20: "h:mm",
    21: "h:mm:ss",
    22: "m/d/yy h:mm",
    37: "#,##0 ;(#,##0)",
    38: "#,##0 ;[Red](#,##0)",
    39: "#,##0.00;(#,##0.00)",
    40: "#,##0.00;[Red](#,##0.00)",
    45: "mm:ss",
    46: "[h]:mm:ss",
    47: "mmss.##",
    48: "h:mm:ss",
    49: "hhmm",
    50: "hh:mm",
    51: "hh-mm",
    52: 'yyyy-MM\\",月\\"hh-mm',
    53: 'yyyy-MM\\",月\\"',
    54: "m/d/yy h:mm",
    55: "yyyy年m月",
    56: "yyyy年m月d日",
    57: "yyyy年m月d日",
    58: '上午/下午hh"时"mm"分"',
}


def xml_local(tag: str) -> str:
    return tag.rsplit("}", 1)[-1]


def strip_format_tokens(code: str) -> str:
    """只留下真正参与格式的字符：引号里的字面量、反斜杠转义与 [方括号] 段都剥掉"""
    out: list = []
    i = 0
    while i < len(code):
        ch = code[i]
        if ch in "\"'":
            closing = code.find(ch, i + 1)
            i = len(code) if closing < 0 else closing + 1
            continue
        if ch == "\\" and i + 1 < len(code):
            i += 2
            continue
        if ch == "[":
            closing = code.find("]", i)
            i = len(code) if closing < 0 else closing + 1
            continue
        out.append(ch)
        i += 1
    return "".join(out).lower()


def format_kind(code: str) -> str:
    """general / percent / date / datetime / time / currency / number —— 按格式串自己说"""
    tokens = strip_format_tokens(code)
    has_time = ("h" in tokens) or ("s" in tokens) or ("am/pm" in tokens)
    # m 跟在时间标记之后是分钟：h:mm 是时间，yyyy-mm 才是日期
    has_date = ("y" in tokens) or ("d" in tokens) or ("m" in tokens and not has_time)
    if "%" in tokens:
        return "percent"
    if has_date and has_time:
        return "datetime"
    if has_date:
        return "date"
    if has_time:
        return "time"
    if any(one in tokens for one in ("$", "¥", "€", "£")):
        return "currency"
    if tokens in ("general", ""):
        return "general"
    return "number"


def serial_to_iso(value: float, year1904: bool) -> str:
    """Excel 的序列数换回 ISO 串；60 号那个不存在的日子照文件原样报"""
    days = int(value)
    rest = value - days
    if year1904:
        base = dt.date(1904, 1, 1)
    elif days == 60:
        return "1900-02-29"
    elif days < 60:
        base = dt.date(1899, 12, 31)
    else:
        base = dt.date(1899, 12, 30)
    stamp = base + dt.timedelta(days=days)
    seconds = int(round(rest * 86400)) % 86400
    if seconds:
        return stamp.isoformat() + "T%02d:%02d:%02d" % (
            seconds // 3600,
            seconds % 3600 // 60,
            seconds % 60,
        )
    return stamp.isoformat()


def _sheet_parts(book: ET.Element) -> list:
    """工作簿里 (表名, 第几张) 的**放映顺序** —— 部件号由调用方按 sheetN.xml 的老规矩配"""
    for holder in book.iter():
        if xml_local(holder.tag) == "sheets":
            out = []
            for index, child in enumerate(holder):
                if xml_local(child.tag) != "sheet":
                    continue
                out.append((child.get("name"), index + 1))
            return out
    return []


def xlsx_formats(path: Path) -> dict:
    """每张表每个格子的格式号 / 格式串 / 类型判定 / 换算出来的日期"""
    with zipfile.ZipFile(path) as box:
        parts = {one.filename: box.read(one.filename) for one in box.infolist()}
    if "xl/styles.xml" not in parts:
        return {"error": "包里读不到 xl/styles.xml"}
    styles = ET.fromstring(parts["xl/styles.xml"])
    custom: dict = {}
    for one in styles.iter():
        if xml_local(one.tag) == "numFmt":
            try:
                custom[int(one.get("numFmtId"))] = one.get("formatCode") or ""
            except (TypeError, ValueError):
                continue
    xfs: list = []
    holder = None
    for one in styles.iter():
        if xml_local(one.tag) == "cellXfs":
            holder = one
            break
    if holder is not None:
        for xf in holder:
            if xml_local(xf.tag) != "xf":
                continue
            try:
                xfs.append(int(xf.get("numFmtId") or 0))
            except ValueError:
                xfs.append(0)
    book = ET.fromstring(parts["xl/workbook.xml"])
    year1904 = False
    for one in book.iter():
        if xml_local(one.tag) == "workbookPr":
            year1904 = (one.get("date1904") or "0").lower() in ("1", "true", "on")
    out: list = []
    for name, number in _sheet_parts(book):
        part = f"xl/worksheets/sheet{number}.xml"
        if part not in parts:
            continue
        root = ET.fromstring(parts[part])
        for cell in (one for one in root.iter() if xml_local(one.tag) == "c"):
            style = cell.get("s") or "0"
            which = int(style) if style.isdigit() else 0
            fmt_id = xfs[which] if which < len(xfs) else 0
            code = custom.get(fmt_id) or BUILTIN_NUMFMT.get(fmt_id, "")
            kind_raw = cell.get("t") or "n"
            value = ""
            # `t` 有两种落点：`<is><t>…</t></is>`（内联串）里是孙子，普通 `<v>` 才是直接儿子
            for child in cell.iter():
                if xml_local(child.tag) == "v":
                    value = (child.text or "").strip()
                elif xml_local(child.tag) == "t" and child.text:
                    value = child.text
            entry = {
                "sheet": name,
                "ref": cell.get("r"),
                "type": kind_raw,
                "style": which,
                "num_fmt_id": fmt_id,
                "format_code": code,
                "kind": (
                    "text"
                    if kind_raw in ("s", "inlineStr", "str")
                    else ("bool" if kind_raw == "b" else format_kind(code))
                ),
                "raw": value,
            }
            if entry["kind"] in ("date", "datetime", "time") and value:
                try:
                    entry["as_date"] = serial_to_iso(float(value), year1904)
                except ValueError:
                    entry["as_date"] = None
            out.append(entry)
    return {"date1904": year1904, "cells": out, "xfs": xfs, "custom": custom}


if __name__ == "__main__":
    import json
    import sys

    for one in sys.argv[1:]:
        print(json.dumps(xlsx_formats(Path(one)), ensure_ascii=False, indent=1, sort_keys=True))
