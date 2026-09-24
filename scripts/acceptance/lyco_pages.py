"""第二读者：这份文档的「那张纸」写着什么。

只用标准库。docx 走 ElementTree 找 `w:sectPr` 里的 `w:pgSz` / `w:pgMar`；
odt 走 styles.xml 里带 `fo:page-width` 的那些 `style:page-layout-properties`。
RTF 不在这里 —— 它是一条流，读法在 `lyco_rtf.py`（与正文同一次走查）。

单位统一换成 **0.1mm 的整数**：三家各写各的（twips 与 `21.59cm` 这种带单位的十进制串），
换成浮点毫米会让两个读者在最后一位上各说各话，换成整数就不存在这件事。
换算用「逢半进一」的整数算法，python 这边不用 `round()` —— 它的舍入是「逢半取偶」，
两边就会在 .5 上分家。
"""

import pathlib
import re
import xml.etree.ElementTree as ET
import zipfile

UNIT = "0.1mm"
MARGIN_KEYS = ("top", "right", "bottom", "left", "header", "footer", "gutter")

# 每一「书写单位」等于多少个 0.1mm，用分数表示免得浮点
SCALE = {
    "twips": (254, 144),  # 1/1440 英寸
    "cm": (1000, 1),
    "mm": (100, 1),
    "in": (254, 1),
    "pt": (127, 36),  # 1/72 英寸
}
LENGTH = re.compile(r"^(-?[\d.]+)\s*(cm|mm|in|pt)$")


def split_length(raw: str) -> tuple:
    """`21.59cm` → 数字串与单位。不是这个形状就抛错（不猜单位）。"""
    hit = LENGTH.match(raw.strip())
    if not hit:
        raise ValueError("不像长度的串：%r" % raw)
    return hit.group(1), hit.group(2)


def to_0p1mm(digits: str, unit: str) -> int:
    """文件里写的长度换成 0.1mm 的整数：十进制精确展开，再逢半进一，全程整数。"""
    num, den = SCALE[unit]
    hit = re.fullmatch(r"(\d+)(?:\.(\d+))?", digits)
    if not hit:
        raise ValueError("不像长度的串：%r" % digits)
    frac = hit.group(2) or ""
    value = int(hit.group(1) + frac)
    scale = 10 ** len(frac)
    a = value * num * 2
    b = scale * den
    return (a + b) // (2 * b)


def convert(raw, unit_hint):
    """交回（换算值, 文件写的那一串）。没写就是 (None, None)。"""
    if raw is None:
        return None, None
    if unit_hint == "twips":
        return to_0p1mm(raw, "twips"), raw
    digits, unit = split_length(raw)
    return to_0p1mm(digits, unit), raw


def entry_from(frm: str, section: int, unit_hint, size, box, orient) -> dict:
    """一张纸的账：换算值与文件自己写的那一串都交。"""
    width, width_raw = convert(size[0], unit_hint)
    height, height_raw = convert(size[1], unit_hint)
    margins, written = {}, {}
    for key in MARGIN_KEYS:
        value, raw = convert(box.get(key), unit_hint)
        margins[key] = value
        written[key] = raw
    return {
        "from": frm,
        "section": section,
        "width": width,
        "height": height,
        "orient": orient,
        "margins": margins,
        "written": {
            "unit": unit_hint,
            "width": width_raw,
            "height": height_raw,
            "orient": orient,
            "margins": written,
        },
    }


def local(tag: str) -> str:
    return tag.split("}")[-1]


def attrs_of(node) -> dict:
    return {local(k): v for k, v in node.attrib.items()}


def docx(path: pathlib.Path) -> list:
    """OOXML：每个 `w:sectPr` 一张纸（正文最后那一节写作 body 的直属孩子）。"""
    with zipfile.ZipFile(path) as z:
        root = ET.fromstring(z.read("word/document.xml"))
    out = []
    section = 0
    for one in root.iter():
        if local(one.tag) != "sectPr":
            continue
        size = box = None
        for kid in one.iter():
            if local(kid.tag) == "pgSz":
                size = attrs_of(kid)
            elif local(kid.tag) == "pgMar":
                box = attrs_of(kid)
        size = size or {}
        box = box or {}
        out.append(
            entry_from(
                "word/document.xml",
                section,
                "twips",
                (size.get("w"), size.get("h")),
                {
                    "top": box.get("top"),
                    "right": box.get("right"),
                    "bottom": box.get("bottom"),
                    "left": box.get("left"),
                    "header": box.get("header"),
                    "footer": box.get("footer"),
                    "gutter": box.get("gutter"),
                },
                size.get("orient"),
            )
        )
        section += 1
    return out


def odt(path: pathlib.Path) -> list:
    """ODF：纸张尺寸在 styles.xml 的 `style:page-layout-properties` 上（各带前缀）。"""
    with zipfile.ZipFile(path) as z:
        root = ET.fromstring(z.read("styles.xml"))
    out = []
    section = 0
    for one in root.iter():
        if local(one.tag) != "page-layout-properties":
            continue
        at = attrs_of(one)
        if "page-width" not in at:
            # 那条只写了 layout-grid 的占位属性，不是一张纸
            continue
        out.append(
            entry_from(
                "styles.xml",
                section,
                None,
                (at.get("page-width"), at.get("page-height")),
                {
                    "top": at.get("margin-top"),
                    "right": at.get("margin-right"),
                    "bottom": at.get("margin-bottom"),
                    "left": at.get("margin-left"),
                },
                at.get("print-orientation"),
            )
        )
        section += 1
    return out


def rtf_entry(writes: dict) -> dict:
    """RTF 的那张纸：控制字的原样交给进来，换算走同一条规则（`landscape` 是旗标）。"""
    return entry_from(
        "rtf-stream",
        0,
        "twips",
        (writes.get("paperw"), writes.get("paperh")),
        {
            "top": writes.get("margt"),
            "right": writes.get("margr"),
            "bottom": writes.get("margb"),
            "left": writes.get("margl"),
        },
        "landscape" if "landscape" in writes else None,
    )


def pages_of(path: pathlib.Path) -> dict:
    name = path.name.lower()
    if name.endswith((".docx", ".docm")):
        papers = docx(path)
    elif name.endswith(".odt"):
        papers = odt(path)
    else:
        papers = []
    return {"unit": UNIT, "papers": papers}


if __name__ == "__main__":
    import json
    import sys

    for arg in sys.argv[1:]:
        print(arg, json.dumps(pages_of(pathlib.Path(arg)), ensure_ascii=False))
