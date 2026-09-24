#!/usr/bin/env python3
"""只依赖标准库的第二个 PDF 读者 —— 用来核对 `lbin office-pdf`，不是给产品用的。

为什么敢用「不追 xref」的读法：PDF 的对象就在文件里以 `N G obj … endobj` 出现，
交叉引用表只是索引。真的坏文件（xref 指错而对象齐全）反而要靠这种容忍读法。

但这条读法必须补两层，否则量出来的数是假的：
1. **对象流**（/Type /ObjStm）：若干对象打包在一个压缩流里，`obj` 扫不到它们。
   头部是一串「对象号 体内偏移」成对出现，偏移相对 /First —— 这三件事是从一份
   真的 Word 2013 文件里量出来的（见 fixture README），不是照记忆写的。
2. **没有 `trailer` 这个词的文件**：PDF 1.5 起 trailer 的键可以搬进 /Type /XRef
   的流字典。qpdf 存的那份就是这样：`trailer` 出现 0 次，/Info 只在 XRef 流里。

字符串解码的三件事：字面量 `(...)` 里的 `\\(` `\\)` `\\\\` 与八进制 `\\ddd`、括号可以嵌套；
十六进制 `<...>` 是字节串，以 `FEFF` 开的是 UTF-16BE；两边都不猜「大概是 Latin-1」，
解不出来就照实说。
"""

from __future__ import annotations

import re
import zlib

OBJ = re.compile(rb"(\d+)\s+(\d+)\s+obj\b", re.S)
ESCAPES = {
    ord("n"): 0x0A,
    ord("r"): 0x0D,
    ord("t"): 0x09,
    ord("b"): 0x08,
    ord("f"): 0x0C,
    ord("("): 0x28,
    ord(")"): 0x29,
    ord("\\"): 0x5C,
}


def literal(body: bytes, at: int) -> tuple[bytes, int]:
    """从开头的 `(` 之后读到配对的 `)`：括号会嵌套，反斜杠转义吃掉下一个字符"""
    out = bytearray()
    depth = 1
    i = at
    while i < len(body):
        ch = body[i]
        if ch == ord("\\"):
            nxt = body[i + 1] if i + 1 < len(body) else 0
            if nxt in ESCAPES:
                out.append(ESCAPES[nxt])
                i += 2
                continue
            if 0x30 <= nxt <= 0x37:
                digits = bytearray()
                j = i + 1
                while j < len(body) and len(digits) < 3 and 0x30 <= body[j] <= 0x37:
                    digits.append(body[j])
                    j += 1
                out.append(int(digits.decode("ascii")) & 0xFF)
                i = j
                continue
            out.append(nxt)
            i += 2
            continue
        if ch == ord("("):
            depth += 1
        elif ch == ord(")"):
            depth -= 1
            if depth == 0:
                return bytes(out), i + 1
        out.append(ch)
        i += 1
    return bytes(out), i


def strings_of(body: bytes, key: bytes) -> list[bytes]:
    """`/Key (…) 或 /Key <…>` 的值，按出现顺序交回**解好的字节**

    分隔符用先行断言而不是吃掉一个字符：`(Literal)` 与 `<Hex>` 本身就是下一个
    要解析的东西，正则把它们吞了就只能读出空串 —— 第一版就是这么把 `/Producer`
    读成没有的。
    """
    out: list[bytes] = []
    for m in re.finditer(re.escape(key) + rb"(?![A-Za-z0-9_])", body):
        at = m.end()
        while at < len(body) and body[at] in b" \t\r\n":
            at += 1
        if at >= len(body):
            break
        if body[at] == 0x28:  # (
            raw, _ = literal(body, at + 1)
            out.append(raw)
        elif body[at] == 0x3C:  # <
            close = body.find(b">", at)
            if close < 0:
                break
            hexits = re.sub(rb"[^0-9A-Fa-f]", b"", body[at + 1 : close])
            if len(hexits) % 2:
                hexits += b"0"
            try:
                out.append(bytes.fromhex(hexits.decode("ascii")))
            except ValueError:
                continue
        else:
            continue
    return out


def decode_pdf_text(raw: bytes) -> str:
    """`FEFF` 开的是 UTF-16BE；其余按 Latin-1 交出去，并说明这是**近似**"""
    if raw.startswith(b"\xfe\xff"):
        return raw[2:].decode("utf-16-be", "replace")
    if raw.startswith(b"\xff\xfe"):
        return raw[2:].decode("utf-16-le", "replace")
    return raw.decode("latin-1", "replace")


def number(body: bytes, key: bytes) -> int | None:
    m = re.search(re.escape(key) + rb"[\s\[\](<]*(\d+)", body)
    return int(m.group(1)) if m else None


def name_of(body: bytes, key: bytes) -> str:
    m = re.search(re.escape(key) + rb"\s*/([A-Za-z0-9._+\-]+)", body)
    return m.group(1).decode("latin-1") if m else ""


def dict_head(body: bytes) -> bytes:
    """字典部分：切在真正的 `stream` 关键字上（前面是行尾、后面跟行尾）。
    按三个字切会被字典里的字（如标题里的 "stream"）撞倒 —— Rust 侧同一条件"""
    m = re.search(rb"(^|[\r\n])stream\r?\n", body)
    return body[: m.start()] if m else body


def stream_of(body: bytes) -> bytes | None:
    """取流正文：`stream` 关键字后面必须跟一个行尾，`endstream` 前那个行尾不算正文"""
    m = re.search(rb"[\r\n]stream\r?\n", body)
    if not m:
        return None
    rest = body[m.end() :]
    end = rest.rfind(b"endstream")
    if end < 0:
        return None
    raw = rest[:end]
    if raw.endswith(b"\r\n"):
        raw = raw[:-2]
    elif raw.endswith(b"\n") or raw.endswith(b"\r"):
        raw = raw[:-1]
    if b"/FlateDecode" in dict_head(body):
        try:
            return zlib.decompress(raw)
        except Exception:  # noqa: BLE001
            return None
    return raw


def scan_objects(data: bytes) -> tuple[dict[int, bytes], int]:
    """第一层：文件里明写的 `N G obj … endobj`，同号只取第一次出现的那个"""
    by_id: dict[int, bytes] = {}
    duplicates = 0
    marks = [(m.start(), int(m.group(1)), m.end()) for m in OBJ.finditer(data)]
    for pos, num, after in marks:
        end = data.find(b"endobj", after)
        if end < 0:
            continue
        if num in by_id:
            duplicates += 1
            continue
        by_id[num] = data[after:end]
    return by_id, duplicates


def unpack_object_streams(data: bytes, plain: dict[int, bytes]) -> tuple[dict[int, bytes], list[dict]]:
    """第二层：对象流里的对象。头部数组、/N、/First 三样少一个就整流作废（不猜）"""
    inner: dict[int, bytes] = {}
    info: list[dict] = []
    for num, body in sorted(plain.items()):
        head = dict_head(body)
        if not re.search(rb"/Type\s*/ObjStm\b", head):
            continue
        n = number(head, b"/N")
        first = number(head, b"/First")
        raw = stream_of(body)
        one: dict = {"id": num, "declared_n": n, "first": first}
        if raw is None or n is None or first is None or first > len(raw):
            one["error"] = "读不出流正文或缺 /N /First"
            info.append(one)
            continue
        pairs = re.findall(rb"\d+", raw[:first])
        one["header_pairs"] = len(pairs) // 2
        if len(pairs) % 2 or len(pairs) // 2 != n:
            one["error"] = "头部数组的个数与 /N 不符"
            info.append(one)
            continue
        offsets = [(int(pairs[i]), int(pairs[i + 1])) for i in range(0, len(pairs), 2)]
        bounds = [first + off for _o, off in offsets] + [len(raw)]
        for index, (obj_num, _off) in enumerate(offsets):
            inner[obj_num] = raw[bounds[index] : bounds[index + 1]]
        one["ok"] = len(offsets)
        info.append(one)
    return inner, info


def pdf_facts(data: bytes) -> dict:
    m = re.match(rb"%PDF-(\d+\.\d+)", data[:1024])
    version = m.group(1).decode("ascii") if m else ""
    plain, duplicates = scan_objects(data)
    inner, streams = unpack_object_streams(data, plain)
    by_id = dict(plain)
    for num, body in inner.items():
        by_id.setdefault(num, body)

    page_objs = [
        (key, body)
        for key, body in sorted(by_id.items())
        if re.search(rb"/Type\s*/Page\b(?!s)", body)
    ]
    tree_objs = [
        (key, body)
        for key, body in sorted(by_id.items())
        if re.search(rb"/Type\s*/Pages\b", body)
    ]
    counts = [number(body, b"/Count") for _key, body in tree_objs]

    def up(page_body: bytes, want):
        """页自己不写就沿 /Parent 往上找：MediaBox / Rotate 这几项是**可继承**的。
        交回 (值, 是不是继承来的)；链上重复的父节点直接停，免得文件自己转圈。"""
        cur = page_body
        seen = set()
        for _hop in range(8):
            got = want(cur)
            if got is not None:
                return got, bool(seen)
            ref = re.search(rb"/Parent\s+(\d+)\s+\d+\s+R", cur)
            if not ref:
                break
            nxt = int(ref.group(1))
            if nxt in seen or nxt not in by_id:
                break
            seen.add(nxt)
            cur = by_id[nxt]
        return None, False

    def box(body: bytes):
        m = re.search(rb"/MediaBox\s*\[([^\]]+)\]", body)
        return re.sub(rb"\s+", b" ", m.group(1)).decode("ascii").strip() if m else None

    def rot(body: bytes):
        one = number(body, b"/Rotate")
        return str(one) if one is not None else None

    sizes, rotations, inherited = [], [], []
    for _key, body in page_objs:
        got, was = up(body, box)
        sizes.append(got or "")
        inherited.append(was)
        got, _was = up(body, rot)
        rotations.append(got or "")

    # trailer 有两种存在方式：`trailer` 关键字，或者 /Type /XRef 的流字典
    trailers = [
        data[m.end() : data.find(b">>", m.end()) + 2]
        for m in re.finditer(rb"\btrailer\b", data)
    ]
    xref_dicts = [
        (key, dict_head(body))
        for key, body in sorted(by_id.items())
        if re.search(rb"/Type\s*/XRef\b", body)
    ]
    xref_bodies = [body for _key, body in xref_dicts]
    catalogs = [
        (key, body) for key, body in sorted(by_id.items()) if re.search(rb"/Type\s*/Catalog\b", body)
    ]

    def from_sources(key: bytes) -> int | None:
        for one in xref_bodies + trailers:
            hit = re.search(re.escape(key) + rb"\s+(\d+)\s+\d+\s+R", one)
            if hit:
                return int(hit.group(1))
        return None

    info_id = from_sources(b"/Info")
    info_objs: list[tuple[int, bytes]] = []
    if info_id is not None and info_id in by_id:
        info_objs = [(info_id, by_id[info_id])]
    else:
        info_objs = [
            (key, body)
            for key, body in sorted(by_id.items())
            if b"/Producer" in body or b"/CreationDate" in body
        ]

    lang, marked, struct = "", False, False
    for _key, body in catalogs:
        found = strings_of(body, b"/Lang")
        lang = decode_pdf_text(found[0]) if found else ""
        marked = bool(re.search(rb"/Marked\s+true", body))
        struct = bool(re.search(rb"/StructTreeRoot", body))

    enc_ref = re.search(rb"/Encrypt\s+(\d+)\s+\d+\s+R", data)
    enc_dict = by_id.get(int(enc_ref.group(1))) if enc_ref else None
    encryption = None
    if enc_dict is not None:
        head = dict_head(enc_dict)
        encryption = {
            "id": int(enc_ref.group(1)),
            "filter": name_of(head, b"/Filter"),
            "v": number(head, b"/V"),
            "revision": number(head, b"/R"),
            "length_bits": number(head, b"/Length"),
            "has_O": b"/O" in head,
            "has_U": b"/U" in head,
            "has_owner_entries": b"/OE" in head or b"/UE" in head,
            "perms": b"/P" in head,
        }

    fonts, images = [], []
    for key, body in sorted(by_id.items()):
        if re.search(rb"/Type\s*/Font\b", body):
            base = re.search(rb"/BaseFont\s*/([^\s/>\]]+)", body)
            sub = re.search(rb"/Subtype\s*/([^\s/>\]]+)", body)
            fonts.append(
                {
                    "id": key,
                    "base_font": base.group(1).decode("latin-1") if base else "",
                    "subtype": sub.group(1).decode("latin-1") if sub else "",
                    "encoding": name_of(body, b"/Encoding"),
                    "to_unicode": bool(re.search(rb"/ToUnicode\s+\d+\s+\d+\s+R", body)),
                    "in_object_stream": key not in plain,
                }
            )
        if re.search(rb"/Subtype\s*/Image\b", body):
            images.append(
                {
                    "id": key,
                    "width": number(body, b"/Width") or 0,
                    "height": number(body, b"/Height") or 0,
                    "filter": name_of(body, b"/Filter"),
                    "color_space": name_of(body, b"/ColorSpace"),
                    "bits": number(body, b"/BitsPerComponent") or 0,
                }
            )

    if encryption is not None:
        # 加密的文件里字符串本体是密文：/Lang 解出来是一串乱码，Info 各项也是。
        # 把乱码当元数据交出去是假答案，所以两边都不给，只说为什么。
        lang = ""
    info: dict = {}
    if encryption is None:
        for _key, body in info_objs[:1]:
            for name, key in (
                ("producer", b"/Producer"),
                ("title", b"/Title"),
                ("author", b"/Author"),
                ("creator", b"/Creator"),
                ("creation_date", b"/CreationDate"),
                ("mod_date", b"/ModDate"),
                ("subject", b"/Subject"),
                ("keywords", b"/Keywords"),
            ):
                found = strings_of(body, key)
                if found:
                    info[name] = decode_pdf_text(found[0])

    # 风险面：PDF 的「宏」就是脚本与动作
    all_text = b"".join(by_id.values())
    actions = len(re.findall(rb"/S\s*/(?:JavaScript|Launch|SubmitForm|ImportData|GoToR|EmbeddedGoto)", all_text))
    annot_types = re.findall(rb"/Subtype\s*/(Link|Widget|Popup|Text|Stamp|FreeText)", all_text)
    annot_counts: dict[str, int] = {}
    for one in annot_types:
        key = one.decode("latin-1")
        annot_counts[key] = annot_counts.get(key, 0) + 1
    return {
        "version": version,
        "header_bytes": data[:8].hex(),
        "binary_comment": re.search(rb"%\xe2\xe3\xcf\xdc", data[:32]) is not None,
        "objects": {
            "plain": len(plain),
            "duplicated_ids": duplicates,
            "in_object_streams": len(inner),
            "object_streams": [one["id"] for one in streams],
            "object_stream_detail": streams,
            "total_seen": len(by_id),
        },
        "xref": {
            "trailer_keyword": len(trailers),
            "xref_streams": [key for key, _body in xref_dicts],
            "sizes": [number(one, b"/Size") for one in xref_bodies + trailers],
            "startxref": len(re.findall(rb"startxref", data)),
            "info_ref": info_id,
        },
        "encryption": encryption,
        "pages": {
            "page_objects": len(page_objs),
            "pages_tree_nodes": len(tree_objs),
            "counts": [one for one in counts if one is not None],
            "sizes": sizes,
            "distinct_sizes": sorted({one for one in sizes if one}),
            "rotations": rotations,
            "missing_mediabox": sum(1 for one in sizes if not one),
            "inherited_mediabox": sum(1 for one in inherited if one),
        },
        "catalogs": [key for key, _body in catalogs],
        "tags": {"lang": lang, "marked": marked, "struct_tree_root": struct},
        "info": info,
        "fonts": fonts,
        "images": images,
        "features": {
            "annotations": annot_counts,
            "javascript": len(re.findall(rb"/JavaScript", all_text)),
            "actions_dangerous": actions,
            "acroform": len(re.findall(rb"/AcroForm", all_text)),
            "fields": len(re.findall(rb"/FT\s*/", all_text)),
            "attachments": len(re.findall(rb"/Type\s*/Filespec\b", all_text)),
            "embedded_files_tree": len(re.findall(rb"/EmbeddedFiles", all_text)),
            "launch": len(re.findall(rb"/S\s*/Launch", all_text)),
            "uri_actions": len(re.findall(rb"/S\s*/URI", all_text)),
            "open_action": len(re.findall(rb"/OpenAction", all_text)),
            "submit_form": len(re.findall(rb"/SubmitForm", all_text)),
        },
        "streams": {
            "total": sum(1 for body in by_id.values() if b"stream" in body),
            "flate": sum(bool(re.search(rb"/FlateDecode", body)) for body in by_id.values()),
        },
    }


if __name__ == "__main__":
    import json
    import sys
    from pathlib import Path

    for one in sys.argv[1:]:
        print(json.dumps(pdf_facts(Path(one).read_bytes()), ensure_ascii=False, indent=1))
