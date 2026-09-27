r"""图自己那一份账：那串字节说的格式、像素与自带密度，对着文档说的那两个尺寸

与 `lilyco-binfmt/src/picture_bytes.rs` 一份账（第二个读者）。问的是同一句话的两种答案：
**文档说这张图多大**（页面上占的那块地方）与**图自己说它多大**（头里那 24 个字节）。
两个数可以差得很远（缩放进来的图），也可以一个根本没有（没有自带密度的 GIF），
所以两边都按原样交，差距用千分比交出去，不替谁圆场。

三条出口，一族一条，「文件说这是什么格式」那一句住在三个完全不同的地方：

* OOXML：声明在 `[Content_Types].xml`（按扩展名的 `Default`、按部件名的 `Override`），
  地址要顺着 `word/_rels/document.xml.rels` 跳一跳才落到 `word/media/image1.png`；
* ODF：声明是 `draw:image/@draw:mime-type`（可以整条不写），地址直接是包内路径；
* RTF：声明就是控制字本身（`\pngblip`、`\wmetafile`），尺寸写成「目标 twips × 缩放百分比」，
  而它**另外**还写了 `\picw` / `\pich` 两个像素数 —— 那两个数与图自己头里的像素数
  是两件事，所以 `px_agrees` 只在写过的族上有答案。

单位一律整数的 0.01mm（`lyco_pages.convert` 那条式子：round half up，无浮点），
自带的密度也按文件写的那个单位交（PNG/BMP 是每米像素、JPEG 是 DPI 或 DPCM、
TIFF 是分辨率标记 × 单位、EMF 干脆直接给 0.01mm 的框），换算成自然尺寸时才动一次式子。
"""
from __future__ import annotations

import zipfile
from pathlib import Path
from xml.etree import ElementTree as ET

from lyco_pages import convert
from lyco_rtf import PIC_CAP, group_stop, word_in_group

# 只看图的头：够走完 PNG 的块表（到 IDAT 为止）、JPEG 的段表（到 SOF 为止）、
# TIFF 的第一个 IFD，而不会把一张照片整个读进内存。RTF 那一族受群头 16KB 的限制，
# 十六进制解出来最多 8KB，所以两边的封顶不一样 —— 封顶本身是一条实测事实。
HEAD_CAP = 64 * 1024
RTF_HEAD_CAP = 8 * 1024
ROW_CAP = 200

# 长宽比与「原尺寸」的判据：千分比。生产者在最后一位上就不一致
# （python-docx 写 1219200 EMU = 3386.67 个 0.01mm，而 LibreOffice 直接写 0.339cm），
# 所以不用相等判，用 1‰ 的零头判 —— 那个数比生产者之间的抖动大、比任何真实拉伸小。
ASPECT_TOL = 10
NATURAL_TOL = 10

# 「文件说这是什么格式」的三种拼法换到图自己那一个名字。认不出来交 None，不猜一个
WORD_FORMAT = {
    "png": "png", "jpeg": "jpeg", "jpg": "jpeg", "jpe": "jpeg",
    "gif": "gif", "bmp": "bmp", "dib": "bmp", "rle": "bmp",
    "tif": "tiff", "tiff": "tiff",
    "emf": "emf", "x-emf": "emf",
    "wmf": "wmf", "x-wmf": "wmf",
    "svg": "svg", "svg+xml": "svg",
}

# RTF 那三个不带 `blip` 尾巴的控制字：`\wmetafile` 说的是 WMF，`\dibitmap` 说的是 BMP；
# `\pictbitmap` 只说「这是一张位图，格式你自己看」，所以它没有名字可给
RTF_WORDS = {"wmetafile": "wmf", "dibitmap": "bmp", "macpict": "pict",
             "pictbitmap": None}


def _u16(raw, at, be=False):
    if at + 2 > len(raw):
        return None
    return int.from_bytes(raw[at:at + 2], "big" if be else "little")


def _u32(raw, at, be=False):
    if at + 4 > len(raw):
        return None
    return int.from_bytes(raw[at:at + 4], "big" if be else "little")


def _rhu(num, den):
    """整数除法、half up 进位（生产者的最后一位抖动不靠浮点复现）"""
    if not den:
        return None
    return (2 * num + den) // (2 * den)


def word_format(raw):
    """那条声明（`image/png` / `pngblip` / `JPG`）说的格式名，认不出交 None"""
    if not raw:
        return None
    text = str(raw).strip().lower()
    for prefix in ("image/", "application/", "drawing/"):
        if text.startswith(prefix):
            text = text[len(prefix):]
    if text in RTF_WORDS:
        return RTF_WORDS[text]
    if text.endswith("blip") and len(text) > 4:
        text = text[: -len("blip")]
    return WORD_FORMAT.get(text)


def detect(raw: bytes):
    r"""那串字节自己是什么格式。认不出交 "unknown"，空的一交 None

    EMF 与 WMF 的头**同一个数**（都以 `01 00` 开头：一个是记录类型、一个是文件类型），
    所以前两字节分不开它们：EMF 的判据在偏移 40 那枚 ` EMF` 签名，
    WMF 的偏移 2 是头长（9 个字，`09 00`）。
    """
    if not raw:
        return None
    if raw[:8] == b"\x89PNG\r\n\x1a\n":
        return "png"
    if raw[:2] == b"\xff\xd8":
        return "jpeg"
    if raw[:6] in (b"GIF87a", b"GIF89a"):
        return "gif"
    if raw[:2] == b"BM" and _u32(raw, 14) in (12, 40, 56, 108, 124):
        return "bmp"
    if raw[:4] in (b"II*\x00", b"MM\x00*"):
        return "tiff"
    if _u32(raw, 0) == 1 and _u32(raw, 40) == 0x464D4520:
        return "emf"
    if _u32(raw, 0) == 0x9AC6CDD7:
        return "wmf"  # 可放置那一族：头里带单位数与英寸数
    if _u16(raw, 0) in (1, 2) and _u16(raw, 2) == 9:
        return "wmf"
    if raw[:8] == b"VCLMTF\x01\x00":
        return "svm"
    return "unknown"


def _dens(state, unit=None, x=None, y=None, raw=None):
    """密度那一条：`state` 说这三种不同的「没有」——
    read 是文件写了可用单位；unitless 是文件写了这个字段但它说的是「只有长宽比」或
    「单位不明」；absent 是这个格式有这一格而这份件没写；none 是这个格式压根没有这一格"""
    return {"state": state, "unit": unit, "x": x, "y": y, "written": raw}


def _natural(px_w, px_h, dens):
    """自然尺寸（0.01mm）：像素数按文件自己说的密度除一次，只除这一次"""
    if px_w is None or px_h is None or dens["state"] != "read":
        return {"w": None, "h": None}
    unit, x, y = dens["unit"], dens["x"], dens["y"]
    if unit == "ppm":
        return {"w": _rhu(px_w * 100000, x), "h": _rhu(px_h * 100000, y)}
    if unit == "dpi":
        return {"w": _rhu(px_w * 2540, x), "h": _rhu(px_h * 2540, y)}
    if unit == "dpcm":
        return {"w": _rhu(px_w * 1000, x), "h": _rhu(px_h * 1000, y)}
    if unit == "mm100":  # EMF：那个框本来就是 0.01mm，没有除法
        return {"w": x, "h": y}
    return {"w": None, "h": None}


def _png(raw: bytes) -> dict:
    """块表走到 IDAT 为止：IHDR 给像素，pHYs 给密度（单位字节只认 1=每米）"""
    out = {"px": {"w": None, "h": None}, "how": None}
    dens = _dens("absent")
    at = 8
    while at + 8 <= len(raw):
        size = _u32(raw, at, True)
        kind = raw[at + 4:at + 8]
        if size is None:
            break
        body = raw[at + 8:at + 8 + size]
        if kind == b"IHDR" and len(body) >= 9:
            out["px"] = {"w": _u32(body, 0, True), "h": _u32(body, 4, True)}
            out["how"] = "IHDR"
        elif kind == b"pHYs" and len(body) >= 9:
            unit = body[8]
            if unit == 1:
                dens = _dens("read", "ppm", _u32(body, 0, True), _u32(body, 4, True),
                             f"{_u32(body, 0, True)},{_u32(body, 4, True)},1")
            else:
                dens = _dens("unitless", "unknown", _u32(body, 0, True),
                             _u32(body, 4, True), f"{_u32(body, 0, True)},{_u32(body, 4, True)},{unit}")
        elif kind == b"IDAT":
            break
        if not size:
            break
        at += 12 + size
    out["density"] = dens
    return out


def _jpeg(raw: bytes) -> dict:
    """段表走到第一个 SOF 为止：APP0/JFIF 给密度（0 只说长宽比），SOFn 给像素"""
    out = {"px": {"w": None, "h": None}, "how": None}
    dens = _dens("absent")
    at = 2
    while at + 4 <= len(raw):
        if raw[at] != 0xFF:
            break
        marker = raw[at + 1]
        if marker in (0x01, 0xD8) or 0xD0 <= marker <= 0xD7:
            at += 2
            continue
        size = _u16(raw, at + 2, True)
        if not size or size < 2:
            break
        seg = raw[at + 4:at + 2 + size]
        if marker == 0xE0 and seg[:5] == b"JFIF\x00" and len(seg) >= 12:
            unit = seg[7]
            x, y = _u16(seg, 8, True), _u16(seg, 10, True)
            names = {0: "aspect", 1: "dpi", 2: "dpcm"}
            got = names.get(unit)
            # 单位 0 那两个数只是长宽比（Pillow 不带 dpi 参数时也写 JFIF，就是这种），
            # 它说不了「一英寸里几个像素」，所以与「压根没写」同等待遇：不算密度
            dens = _dens("read" if got in ("dpi", "dpcm") else "unitless",
                         got, x, y, f"JFIF,{unit},{x},{y}")
        elif marker == 0xEE and seg[:12] == b"Adobe\x00\xCC\xED":
            pass  # APP14 只说色彩空间，密度不在这里
        elif 0xC0 <= marker <= 0xCF and marker not in (0xC4, 0xC8, 0xCC):
            if len(seg) >= 5:
                # SOFn：精度 1 字节、高 2、宽 2 —— 高的在前
                out["px"] = {"w": _u16(seg, 3, True), "h": _u16(seg, 1, True)}
                out["how"] = f"SOF{marker - 0xC0:x}".upper()
            break
        at += 2 + size
    out["density"] = dens
    return out


def _gif(raw: bytes) -> dict:
    return {"px": {"w": _u16(raw, 6), "h": _u16(raw, 8)},
            "how": "LogicalScreenDescriptor", "density": _dens("none")}


def _bmp(raw: bytes) -> dict:
    """`BITMAPCOREHEADER`（12）没有密度那一格，`BITMAPINFOHEADER`（40）有；
    高度可以是负数（那是「从上往下存」，不是另一个尺寸），所以取绝对值算尺寸"""
    header = _u32(raw, 14)
    w = _u32(raw, 18)
    h = _u32(raw, 22)
    out = {"px": {"w": w, "h": h}, "how": f"BITMAPHEADER{header}",
           "height_signed": h if h is not None and h < 0x80000000 else None}
    if header == 12:
        out["density"] = _dens("none", raw="core")
        return out
    x, y = _u32(raw, 38), _u32(raw, 42)
    if x and y:
        out["density"] = _dens("read", "ppm", x, y, f"{x},{y}")
    else:
        out["density"] = _dens("absent", raw=f"{x},{y}")
    return out


def _tiff(raw: bytes) -> dict:
    """IFD 里那几条：256/257 是像素，282/283 是分辨率（RATIONAL，指到别处），
    296 是单位（1 没有、2 每英寸、3 每厘米）。SHORT 的值就塞在那 4 个字节的前两个里"""
    be = raw[:2] == b"MM"
    def u16(at):
        return _u16(raw, at, be)

    def u32(at):
        return _u32(raw, at, be)

    out = {"px": {"w": None, "h": None}, "how": "IFD0"}
    dens = _dens("absent")
    unit = None
    xres = yres = None
    ifd = u32(4)
    if ifd is None or ifd + 2 > len(raw):
        out["density"] = dens
        return out
    count = u16(ifd) or 0
    for i in range(count):
        one = ifd + 2 + i * 12
        if one + 12 > len(raw):
            break
        tag, typ, num = u16(one), u16(one + 2), u32(one + 4)
        if typ == 3:
            val = u16(one + 8)
        elif typ == 4:
            val = u32(one + 8)
        else:
            val = None
        if tag == 256:
            out["px"]["w"] = val
        elif tag == 257:
            out["px"]["h"] = val
        elif tag in (282, 283) and typ in (5, 6, 10):
            # 类型 5 才是 RATIONAL（Pillow 写的就是 5，实测 `282 5 1 176`）。
            # 那 8 个字节（分子+分母各一个 LONG）塞不进一条项，所以那格是**偏移**，
            # 与 SHORT/LONG 把值就地写下是两件事 —— 按就地那个数读会得到 176
            where = u32(one + 8)
            if where is not None and where + 8 <= len(raw):
                pair = (u32(where), u32(where + 4))
                if pair[0] is not None and pair[1] is not None:
                    if tag == 282:
                        xres = pair
                    else:
                        yres = pair
        elif tag == 296:
            unit = val
    if xres and yres and xres[1] and yres[1]:
        # 那条单位标签没写时 TIFF 自己的默认是 2（每英寸），不是「没说」
        names = {2: "dpi", 3: "dpcm"}
        got = names.get(2 if unit is None else unit)
        raw_s = f"{xres[0]}/{xres[1]},{yres[0]}/{yres[1]},{unit}"
        if got:
            dens = _dens("read", got, _rhu(xres[0], xres[1]), _rhu(yres[0], yres[1]), raw_s)
        else:
            # 分辨率两个数在场而单位那一格说的是「没有单位」或一个认不出的数：
            # 这不能算 absent（那两格明明写了），只能算 unitless
            dens = _dens("unitless", "unknown", xres[0], yres[0], raw_s)
    out["density"] = dens
    return out


def _emf(raw: bytes) -> dict:
    """记录头：`rclBounds` 是像素（含端点，所以宽是 右-左+1），`rclFrame` 已经是 0.01mm，
    所以 EMF 是唯一一个不用密度就能报自然尺寸的位图格式"""
    def s32(at):
        got = _u32(raw, at)
        if got is None:
            return None
        return got - (1 << 32) if got >= (1 << 31) else got

    left, top, right, bottom = (s32(8), s32(12), s32(16), s32(20))
    fl, ft, fr, fb = (s32(24), s32(28), s32(32), s32(36))
    px = {"w": None if left is None else right - left + 1,
          "h": None if top is None else bottom - top + 1}
    dens = _dens("read", "mm100",
                 None if fl is None else fr - fl + 1,
                 None if ft is None else fb - ft + 1,
                 f"{fl},{ft},{fr},{fb}")
    return {"px": px, "how": "ENHMETAHEADER", "density": dens}


def _wmf(raw: bytes) -> dict:
    """可放置 WMF（`0x9AC6CDD7` 开头）在头里给单位数与英寸数，普通 WMF 只给记录长度 ——
    那一种的头里没有任何尺寸可说，所以像素与自然都交 None，而不是交 0"""
    if _u32(raw, 0) == 0x9AC6CDD7:
        left, top, right, bottom = (_u16(raw, 8), _u16(raw, 10), _u16(raw, 12), _u16(raw, 14))
        inch = _u16(raw, 16)
        px = {"w": None if left is None else right - left + 1,
              "h": None if top is None else bottom - top + 1}
        dens = _dens("read" if inch else "absent", "dpi", inch, inch, str(inch))
        return {"px": px, "how": "PlaceableHeader", "density": dens}
    return {"px": {"w": None, "h": None}, "how": "Header", "density": _dens("none")}


DECODERS = {"png": _png, "jpeg": _jpeg, "gif": _gif, "bmp": _bmp, "tiff": _tiff,
            "emf": _emf, "wmf": _wmf}


def read(raw: bytes) -> dict:
    """一串字节 → 它自己说的那份账：格式、像素、密度、自然尺寸"""
    kind = detect(raw)
    if kind is None or kind not in DECODERS:
        return {"format": kind, "px": {"w": None, "h": None}, "how": None,
                "density": _dens("none" if kind else None), "nat_mm100": {"w": None, "h": None}}
    got = DECODERS[kind](raw)
    return {
        "format": kind,
        "px": got["px"],
        "how": got["how"],
        "density": got["density"],
        "nat_mm100": _natural(got["px"]["w"], got["px"]["h"], got["density"]),
    }


def _permille(a, b):
    """千分比：一次除法，half up（不先换成小数再比，那会让两个读者舍到不同的一位）"""
    if not a or not b:
        return None
    return _rhu(a * 1000, b)


def _abs(x):
    return None if x is None else abs(x)


def _row(item: dict) -> dict:
    """一条账：声明、检测、两边的尺寸、两两边的千分比。所有除法只在整数上做一次"""
    where = item["where"]
    word = item.get("word")
    said = word_format(word)
    ext = item.get("ext")
    ext_name = word_format(ext)
    raw = item.get("bytes")
    got = read(raw or b"")
    sig = got["format"]
    agrees = None if (said is None or sig in (None, "unknown")) else said == sig
    ext_agrees = None if (ext_name is None or sig in (None, "unknown")) else ext_name == sig
    px = got["px"]
    dens = got["density"]
    nat = got["nat_mm100"]
    placed_w = item.get("placed_w")
    placed_h = item.get("placed_h")
    # 长宽比差：交叉相乘，不除两次再比商（那会把两次舍入叠进判据），千分比
    aspect = None
    if placed_w and placed_h and px["w"] and px["h"]:
        left = placed_w * _abs(px["h"])
        right = placed_h * _abs(px["w"])
        peak = max(left, right)
        aspect = _rhu(abs(left - right) * 1000, peak) if peak else None
    scale_w = _permille(placed_w, nat["w"])
    scale_h = _permille(placed_h, nat["h"])
    declared_px_w = item.get("decl_px_w")
    declared_px_h = item.get("decl_px_h")
    if declared_px_w is None and declared_px_h is None:
        px_agrees = None
    elif not px["w"]:
        px_agrees = None
    else:
        px_agrees = (declared_px_w == px["w"]) and (declared_px_h == px["h"])
    return {
        "where": where,
        "addr": item.get("addr", "read"),
        "word": word,
        "word_name": said,
        "ext": ext,
        "ext_name": ext_name,
        "sig": sig,
        "agrees": agrees,
        "ext_agrees": ext_agrees,
        "pixels": {"w": px["w"], "h": px["h"]},
        "declared_pixels": None if (declared_px_w is None and declared_px_h is None)
        else {"w": declared_px_w, "h": declared_px_h},
        "px_agrees": px_agrees,
        "density": dens,
        "nat_mm100": nat,
        "placed_mm100": {"w": placed_w, "h": placed_h, "from": item.get("placed_from"),
                         "written": item.get("placed_written")},
        "scale_permille": {"w": scale_w, "h": scale_h},
        "aspect_permille": aspect,
        "stretched": None if aspect is None else aspect > ASPECT_TOL,
        "at_natural": None if (scale_w is None or scale_h is None) else (
            abs(scale_w - 1000) <= NATURAL_TOL and abs(scale_h - 1000) <= NATURAL_TOL),
        "how": got["how"],
        "head_hex": (raw or b"")[:8].hex(),
        "note": item.get("note"),
    }


def _tally(rows: list, family: str, total: int, limit: int, cap: int) -> dict:
    """那些数的数法：每一条只问一个「文件到底说了没有」的问题，两问不合并"""
    def count(pred):
        return sum(1 for one in rows if pred(one))

    detected: dict = {}
    for one in rows:
        key = one["sig"] if one["sig"] else "(没读到)"
        detected[key] = detected.get(key, 0) + 1
    placed_known = count(lambda one: one["placed_mm100"]["w"] is not None
                         or one["placed_mm100"]["h"] is not None)
    return {
        "family": family,
        "available": True,
        "total": total,
        "listed": min(total, limit),
        "cut": total > limit,
        "read_cap": cap,
        "distinct_parts": len({one["where"] for one in rows}),
        "addr": {
            "read": count(lambda one: one["addr"] == "read"),
            "unresolved": count(lambda one: one["addr"] == "unresolved"),
            "missing": count(lambda one: one["addr"] == "missing"),
            "none": count(lambda one: one["addr"] == "none"),
        },
        "detected": detected,
        "agrees": {
            "yes": count(lambda one: one["agrees"] is True),
            "no": count(lambda one: one["agrees"] is False),
            "undecided": count(lambda one: one["agrees"] is None),
        },
        "ext_agrees": {
            "yes": count(lambda one: one["ext_agrees"] is True),
            "no": count(lambda one: one["ext_agrees"] is False),
            "undecided": count(lambda one: one["ext_agrees"] is None),
        },
        "pixels": {
            "known": count(lambda one: one["pixels"]["w"] is not None),
            "unknown": count(lambda one: one["pixels"]["w"] is None),
        },
        "density": {
            "read": count(lambda one: one["density"]["state"] == "read"),
            "unitless": count(lambda one: one["density"]["state"] == "unitless"),
            "absent": count(lambda one: one["density"]["state"] == "absent"),
            "none": count(lambda one: one["density"]["state"] == "none"),
        },
        "natural": {
            "known": count(lambda one: one["nat_mm100"]["w"] is not None),
            "unknown": count(lambda one: one["nat_mm100"]["w"] is None),
        },
        "placed": {"known": placed_known, "unknown": len(rows) - placed_known},
        "stretched": {
            "yes": count(lambda one: one["stretched"] is True),
            "no": count(lambda one: one["stretched"] is False),
            "undecided": count(lambda one: one["stretched"] is None),
        },
        "at_natural": {
            "yes": count(lambda one: one["at_natural"] is True),
            "no": count(lambda one: one["at_natural"] is False),
            "undecided": count(lambda one: one["at_natural"] is None),
        },
        "declared_pixels": {
            "written": count(lambda one: one["declared_pixels"] is not None),
            "agrees": count(lambda one: one["px_agrees"] is True),
            "disagrees": count(lambda one: one["px_agrees"] is False),
            "undecided": count(lambda one: one["px_agrees"] is None),
        },
        "rows": rows[:limit],
    }


def _local(tag):
    return tag.rsplit("}", 1)[-1]


def _attr(node, want):
    for key, value in node.attrib.items():
        if key.rsplit("}", 1)[-1] == want or key.rsplit(":", 1)[-1] == want:
            return value
    return None


def _emu(raw):
    """EMU → 0.01mm：1 毫米 = 36000 EMU，所以一格 360，恰好整除时不产生舍入"""
    if raw is None:
        return None
    try:
        return convert(raw, "emu")[0]
    except (ValueError, KeyError, IndexError):
        return None


def _length(raw):
    if raw is None:
        return None
    try:
        return convert(raw, None)[0]
    except (ValueError, KeyError, IndexError):
        return None


def _twips(raw):
    if raw is None:
        return None
    try:
        return convert(raw, "twips")[0]
    except (ValueError, KeyError, IndexError):
        return None


def _content_types(names, box):
    """部件 → 那个声明串。`Override` 按部件名、`Default` 按扩展名，两条都是文件自己写的"""
    defaults, overrides = {}, {}
    if "[Content_Types].xml" not in names:
        return defaults, overrides
    for one in ET.fromstring(box.read("[Content_Types].xml")):
        if _local(one.tag) == "Default":
            defaults[(one.get("Extension") or "").lower()] = one.get("ContentType")
        elif _local(one.tag) == "Override":
            overrides[(one.get("PartName") or "").lower()] = one.get("ContentType")
    return defaults, overrides


def docx_ledger(path: Path, limit: int = ROW_CAP) -> dict:
    """docx（以及任何走 `word/document.xml` 的 OOXML 件）里每张图的那一份字节账

    尺寸取 `wp:extent`（EMU），地址取 `a:blip/@r:embed` 顺关系表那一跳；
    声明取 `[Content_Types].xml`。跳不到部件的交 `addr` 而不是把这一行丢掉。
    """
    with zipfile.ZipFile(path) as box:
        names = set(box.namelist())
        if "word/document.xml" not in names:
            return {"family": "docx", "available": False}
        defaults, overrides = _content_types(names, box)
        rels = {}
        if "word/_rels/document.xml.rels" in names:
            for one in ET.fromstring(box.read("word/_rels/document.xml.rels")):
                if _local(one.tag) != "Relationship" or one.get("Id") in rels:
                    continue
                rels[one.get("Id")] = (one.get("Target") or "", one.get("TargetMode") or "")
        body = ET.fromstring(box.read("word/document.xml"))
        items = []
        for drawing in [one for one in body.iter() if _local(one.tag) == "drawing"]:
            frames = [one for one in drawing.iter() if _local(one.tag) == "inline"]
            if not frames:
                frames = [one for one in drawing.iter() if _local(one.tag) == "anchor"]
            if not frames:
                continue
            frame = frames[0]
            extent = next((one for one in frame
                           if _local(one.tag) == "extent"), None)
            if extent is None:
                extent = next((one for one in frame.iter()
                               if _local(one.tag) == "extent"), None)
            placed_w = placed_h = None
            written = None
            if extent is not None:
                cx, cy = _attr(extent, "cx"), _attr(extent, "cy")
                placed_w, placed_h = _emu(cx), _emu(cy)
                written = [cx, cy]
            blip = next((one for one in frame.iter() if _local(one.tag) == "blip"), None)
            embed = _attr(blip, "embed") if blip is not None else None
            base = {"placed_w": placed_w, "placed_h": placed_h,
                    "placed_from": "wp:extent", "placed_written": written, "bytes": b""}
            if embed is None:
                items.append(dict(base, where="(no-blip)", addr="none", note=None))
                continue
            if embed not in rels:
                items.append(dict(base, where=embed, addr="unresolved", note=None))
                continue
            target, mode = rels[embed]
            if mode == "External":
                items.append(dict(base, where=target, addr="none", note="external"))
                continue
            part = "word/" + target.lstrip("/") if not target.startswith("word/") else target
            if part not in names:
                items.append(dict(base, where=part, addr="missing", note=None))
                continue
            raw = box.read(part)[:HEAD_CAP]
            items.append(dict(
                base, where=part, addr="read", bytes=raw,
                word=overrides.get("/" + part.lower())
                or defaults.get(part.rsplit(".", 1)[-1].lower()),
                ext=part.rsplit(".", 1)[-1].lower() if "." in part else None,
            ))
        rows = [_row(one) for one in items]
    return _tally(rows, "docx", len(rows), limit, HEAD_CAP)


def odf_ledger(path: Path, limit: int = ROW_CAP) -> dict:
    """odt 里每张图的那一份字节账：声明是 `draw:mime-type`，地址是包内路径，
    尺寸是 `draw:frame` 上那两条自带单位的串（`svg:width="0.339cm"`）"""
    with zipfile.ZipFile(path) as box:
        names = set(box.namelist())
        if "content.xml" not in names:
            return {"family": "odf", "available": False}
        root = ET.fromstring(box.read("content.xml"))
        items = []
        for frame in [one for one in root.iter()
                      if _local(one.tag) == "frame"
                      and next((k for k in one if _local(k.tag) == "image"), None) is not None]:
            image = next(k for k in frame if _local(k.tag) == "image")
            href = _attr(image, "href")
            mime = _attr(image, "mime-type")
            w_raw, h_raw = _attr(frame, "width"), _attr(frame, "height")
            base = {"placed_w": _length(w_raw), "placed_h": _length(h_raw),
                    "placed_from": "draw:frame/@svg:width", "placed_written": [w_raw, h_raw],
                    "word": mime, "bytes": b""}
            if not href:
                items.append(dict(base, where="(no-href)", addr="none"))
                continue
            part = href.split("#", 1)[0]
            if part.startswith("./"):
                part = part[2:]
            if part.startswith("/"):
                part = part[1:]
            ext = part.rsplit(".", 1)[-1].lower() if "." in part else None
            if part not in names:
                items.append(dict(base, where=part, addr="missing", ext=ext))
                continue
            items.append(dict(base, where=part, addr="read", ext=ext,
                              bytes=box.read(part)[:HEAD_CAP]))
        rows = [_row(one) for one in items]
    return _tally(rows, "odf", len(rows), limit, HEAD_CAP)


def _blip_word(group: str):
    r"""第一个「说这是哪种图」的控制字，交回名字与它写完之后的位置

    与 `rtf.rs` 的 `blip_word` 同一个问，但认的字更多：那一家只认 `*blip` 结尾的三个加
    `dibitmap` / `pictbitmap` / `macpict`，于是 LibreOffice 写 `\wmetafile` 的那一张在
    `picture_list` 里是 `blip: null` —— 文件明明说了是 WMF，读者说它没说。这一族认全。
    """
    at = 0
    while at < len(group):
        if group[at] != "\\":
            at += 1
            continue
        k = at + 1
        while k < len(group) and group[k].isalpha():
            k += 1
        name = group[at + 1:k]
        while k < len(group) and (group[k].isdigit() or group[k] == "-"):
            k += 1
        if name.endswith("blip") or name in RTF_WORDS:
            return name, k
        at = k if k > at + 1 else at + 1
    return None, None


def _pict_heads(text: str):
    """每一个 `\\pict` 群的群头起点：`{` 紧跟 `\\pict` 且词在边界上

    与 `rtf.rs` 的整群走查是两个不同的走法（那一家在 tokenizer 里认这个词），
    数出来的条数可以拿来对账：对不上就是有人把 `\\pict` 写在了群中间或正文里。
    """
    out = []
    at = text.find("{")
    while at >= 0:
        if text[at + 1:at + 2] == "\\" and text[at + 2:at + 6] == "pict":
            after = text[at + 6:at + 7]
            if not (after.isalnum() or after == "-"):
                out.append(at)
                at += 6
                continue
        at = text.find("{", at + 1)
    return out


def _hex_body(text: str, frm: int, cap: int):
    """从 `frm` 起把十六进制对解成字节：数据可以折行（LibreOffice 每 128 字符断一行），
    碰上既不是十六进制也不是空白的那个字节才停。交回（字节, 停在哪）"""
    got = bytearray()
    pending = None
    at = frm
    while at < len(text) and len(got) < cap:
        ch = text[at]
        if ch in " \t\r\n\f":
            at += 1
            continue
        try:
            digit = int(ch, 16)
        except ValueError:
            break
        if pending is None:
            pending = digit
        else:
            got.append(pending * 16 + digit)
            pending = None
        at += 1
    return bytes(got), at


def rtf_ledger(data: bytes, limit: int = ROW_CAP) -> dict:
    r"""rtf 里每张图的那一份字节账：这一族把尺寸拆成「目标 × 缩放百分比」两半

    `\picwgoal` 是 twips 的目标宽、`\picscalex` 是百分数（RTF 的缺省是 100，Word 在
    100 时常常整条不写），所以页面上那块地方要一次除法两半合成，不先舍入到中间单位；
    `\picw` / `\pich` 又另外写了**像素数** —— 那是文档自己声称的像素，与图自己头里那
    两个字节是两件事，所以 `px_agrees` 只在这一族有答案（别族没写过像素，交 None）。
    声明是控制字本身（`\pngblip`、`\wmetafile`），地址这一族压根没有（数据就在群里）。
    """
    text = data.decode("latin-1", "replace")
    items = []
    for ordinal, start in enumerate(_pict_heads(text)):
        # 从 `{` 的下一格起走：`group_stop` 交回「关掉当前这一群」的那个 `}`，
        # 从 `{` 起走会多关一层、一直走到文档末尾，于是一条没有 blip 的 `\pict`
        # 会把下一张图的控制字与字节读成自己的（两条账说同一句话）
        stop = group_stop(text, start + 1)
        head = text[start:min(stop, start + PIC_CAP)]
        kind, data_at = _blip_word(head)
        goal_w = word_in_group(head, "picwgoal")
        goal_h = word_in_group(head, "pichgoal")
        scale_x = word_in_group(head, "picscalex")
        scale_y = word_in_group(head, "picscaley")
        raw, _ = _hex_body(head, data_at, RTF_HEAD_CAP) if data_at else (b"", 0)
        items.append({
            "where": f"pict#{ordinal}", "addr": "read" if raw else "none",
            "word": kind, "ext": None, "bytes": raw,
            "placed_w": _goal_mm100(goal_w, scale_x),
            "placed_h": _goal_mm100(goal_h, scale_y),
            "placed_from": r"\picwgoal×\picscalex",
            "placed_written": [goal_w, goal_h, scale_x, scale_y],
            "decl_px_w": _int_or_none(word_in_group(head, "picw")),
            "decl_px_h": _int_or_none(word_in_group(head, "pich")),
        })
    rows = [_row(one) for one in items]
    return _tally(rows, "rtf", len(rows), limit, RTF_HEAD_CAP)


def _int_or_none(raw):
    try:
        return int(raw)
    except (TypeError, ValueError):
        return None


def _goal_mm100(goal, scale):
    """twips 的目标 × 百分比，一次除法换到 0.01mm（缺省的 100 是 RTF 的规格，不是猜的）"""
    base = _int_or_none(goal)
    if base is None:
        return None
    factor = _int_or_none(scale)
    if factor is None:
        factor = 100
    return _rhu(base * 2540 * factor, 1440 * 100)
