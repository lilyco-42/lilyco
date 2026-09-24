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


def key_positions(body: bytes, key: bytes) -> list[int]:
    """整个名字都要匹配上才算：`/Page` 不能算 `/Pages`（与 Rust 的 key_positions 同一条）"""
    out = []
    pat = re.compile(re.escape(key) + rb"(?![A-Za-z0-9._+\-])")
    pos = 0
    while True:
        m = pat.search(body, pos)
        if not m:
            return out
        out.append(m.start())
        pos = m.end()


def skip_spaces(body: bytes, at: int) -> int:
    while at < len(body) and body[at : at + 1] in (b" ", b"\t", b"\r", b"\n", b"\x00", b"\x0c"):
        at += 1
    return at


def refs_in(raw: bytes) -> list[int]:
    return [int(m.group(1)) for m in re.finditer(rb"(\d+)\s+\d+\s+R", raw)]


def ref_of(body: bytes, key: bytes) -> int | None:
    """`/Key 12 0 R` 里的对象号"""
    for at in key_positions(body, key):
        m = re.match(rb"\s*(\d+)\s+\d+\s+R", body[at + len(key) :])
        if m:
            return int(m.group(1))
    return None


def refs_of(body: bytes, key: bytes) -> list[int]:
    """`/Key[a 0 R b 0 R]` 与 `/Key N G R` 两种写法都收（与 Rust 同一条）"""
    out: list[int] = []
    for at in key_positions(body, key):
        from_ = skip_spaces(body, at + len(key))
        if body[from_ : from_ + 1] == b"[":
            stop = body.find(b"]", from_)
            out.extend(refs_in(body[from_ + 1 : stop if stop > 0 else len(body)]))
            continue
        m = re.match(rb"\s*(\d+)\s+\d+\s+R", body[at + len(key) :])
        if m:
            out.append(int(m.group(1)))
    return out


def keyword_after(body: bytes, key: bytes) -> bool | None:
    """`/NeedAppearances true` 那种关键字：写了才交布尔，写了别的东西交 None"""
    for at in key_positions(body, key):
        rest = body[skip_spaces(body, at + len(key)) :]
        if rest.startswith(b"true"):
            return True
        if rest.startswith(b"false"):
            return False
        return None
    return None


def one_str(body: bytes, key: bytes) -> str | None:
    """这个键的第一个字符串值，按 PDF 字符串那三件事解（与 Rust 的 one_string 同一条）"""
    got = strings_of(body, key)
    return decode_pdf_text(got[0]) if got else None


def form_of(by_id: dict[int, bytes]) -> tuple[dict, list]:
    """`/AcroForm` → `/Fields` → `/Kids` 那一份账（只从 /Fields 走，避免与 /Annots 数两遍）"""
    empty = {
        "present": False,
        "object": None,
        "roots": 0,
        "total": 0,
        "listed": 0,
        "with_v": 0,
        "default_appearance": None,
        "need_appearances": None,
        "sig_flags": None,
        "xfa": False,
        "by_type": {"text": 0, "button": 0, "choice": 0, "signature": 0, "unknown": 0},
        "inherited_type": 0,
        "inherited_flags": 0,
        "widgets": 0,
        "deepest": 0,
        "items": [],
    }
    root = None
    for num, body in sorted(by_id.items()):
        if b"/XRef" in body:
            root = ref_of(body, b"/Root")
            if root is not None:
                break
    catalog_body = None
    if root is not None:
        catalog_body = by_id.get(root)
    else:
        for _num, body in sorted(by_id.items()):
            if b"/Catalog" in dict_head(body):
                catalog_body = body
                break
    if catalog_body is None or not key_positions(catalog_body, b"/AcroForm"):
        return empty, []
    acro = ref_of(catalog_body, b"/AcroForm")
    if acro is None:
        return empty, []
    head = by_id.get(acro)
    if head is None:
        one = dict(empty)
        one["present"] = True
        one["object"] = acro
        return one, []
    fields: list = []
    seen: set[int] = set()

    def inherited(kind: str, parent: int | None, key: bytes, hops: int):
        cursor, used = parent, 0
        while cursor is not None and used <= hops:
            used += 1
            body = by_id.get(cursor)
            if body is None:
                return None
            if kind == "name":
                m = re.search(re.escape(key) + rb"\s*/([A-Za-z0-9._+\-]+)", body)
                if m:
                    return m.group(1).decode("latin-1")
            else:
                got = number(body, key)
                if got is not None:
                    return got
            cursor = ref_of(body, b"/Parent")
        return None

    def walk(num: int, depth: int) -> None:
        if num in seen:
            return
        seen.add(num)
        body = by_id.get(num)
        if body is None:
            return
        names: list[str] = []
        own = one_str(body, b"/T")
        if own:
            names.append(own)
        parent = ref_of(body, b"/Parent")
        hops, cursor = 0, parent
        while cursor is not None and hops <= 16:
            hops += 1
            had = by_id.get(cursor)
            if had is None:
                break
            text = one_str(had, b"/T") or ""
            if text:
                names.append(text)
            cursor = ref_of(had, b"/Parent")
        names.reverse()
        ft_here = name_of(body, b"/FT") or None
        ft, ft_inherited = ft_here, False
        if ft is None:
            ft = inherited("name", parent, b"/FT", hops)
            ft_inherited = ft is not None
        flags_here = number(body, b"/Ff")
        flags, flags_inherited = flags_here, False
        if flags is None:
            flags = inherited("int", parent, b"/Ff", hops)
            flags_inherited = flags is not None
        subtype = name_of(body, b"/Subtype") or None
        fields.append(
            {
                "object": num,
                "depth": depth,
                "order": len(fields),
                "partial": own,
                "qualified": ".".join(names) if names else None,
                "type": ft,
                "type_inherited": ft_inherited,
                "value": one_str(body, b"/V"),
                "value_present": bool(key_positions(body, b"/V")),
                "default": one_str(body, b"/DV"),
                "flags": flags,
                "flags_inherited": flags_inherited,
                "max_len": number(body, b"/MaxLen"),
                "options": [decode_pdf_text(one) for one in strings_of(body, b"/Opt")],
                "kids": len(refs_of(body, b"/Kids")),
                "parent": parent,
                "widget": subtype == "Widget",
                "subtype": subtype,
            }
        )
        for kid in refs_of(body, b"/Kids"):
            walk(kid, depth + 1)

    for top in refs_of(head, b"/Fields"):
        walk(top, 0)
    kinds = {"Tx": 0, "Btn": 0, "Ch": 0, "Sig": 0, "none": 0}
    for one in fields:
        if one["type"] in kinds:
            kinds[one["type"]] += 1
        else:
            kinds["none"] += 1
    info = dict(empty)
    info.update(
        {
            "present": True,
            "object": acro,
            "roots": len(refs_of(head, b"/Fields")),
            "total": len(fields),
            "listed": len(fields),
            "with_v": sum(1 for one in fields if one["value_present"]),
            "default_appearance": one_str(head, b"/DA"),
            "need_appearances": keyword_after(head, b"/NeedAppearances"),
            "sig_flags": number(head, b"/SigFlags"),
            "xfa": bool(key_positions(head, b"/XFA")),
            "by_type": {
                "text": kinds["Tx"],
                "button": kinds["Btn"],
                "choice": kinds["Ch"],
                "signature": kinds["Sig"],
                "unknown": kinds["none"],
            },
            "inherited_type": sum(1 for one in fields if one["type_inherited"]),
            "inherited_flags": sum(1 for one in fields if one["flags_inherited"]),
            "widgets": sum(1 for one in fields if one["widget"]),
            "deepest": max([one["depth"] for one in fields], default=0),
            "items": fields,
        }
    )
    return info, fields


def form_facts(data: bytes) -> dict:
    """`/AcroForm` 那一份账的入口：对象两层都扫（对象流里的字段也算），再走 `/Fields`"""
    plain, duplicates = scan_objects(data)
    inner, streams = unpack_object_streams(data, plain)
    by_id = dict(plain)
    for num, body in inner.items():
        by_id.setdefault(num, body)
    return form_of(by_id)[0]


def _tokens(raw: bytes):
    """内容流的记号流：数字、`/名字`、`(串)`、`<十六进制>`、`[` `]`、`<<` `>>`、操作符

    只分到这粒度就够：文本这一族要的是 `Tf` 换了哪张字体、`Tj` / `TJ` 里给了哪些字节、
    `Td` / `T*` / `'` 这些位置算符在哪儿断行。数组里的东西照样往外交，
    调用方自己看 `[` `]` 的配对。
    """
    i = 0
    n = len(raw)
    while i < n:
        ch = raw[i]
        if ch in b" \t\r\n\x00\x0c":
            i += 1
            continue
        if ch == 0x28:  # (
            value, nxt = literal(raw, i + 1)
            yield ("str", value)
            i = nxt
            continue
        if ch == 0x3C:  # <
            if raw[i + 1 : i + 2] == b"<":
                yield ("dict", b"<<")
                i += 2
                continue
            close = raw.find(b">", i)
            if close < 0:
                break
            hexits = re.sub(rb"[^0-9A-Fa-f]", b"", raw[i + 1 : close])
            if len(hexits) % 2:
                hexits += b"0"
            yield ("str", bytes.fromhex(hexits.decode("ascii")))
            i = close + 1
            continue
        if ch == 0x3E and raw[i + 1 : i + 2] == b">":
            yield ("dict", b">>")
            i += 2
            continue
        if ch in b"[]":
            yield ("bracket", bytes([ch]))
            i += 1
            continue
        if ch == 0x2F:  # /
            j = i + 1
            while j < n and not (raw[j] in b" \t\r\n/<>[](){}%"):
                j += 1
            yield ("name", raw[i + 1 : j].decode("latin-1"))
            i = j
            continue
        j = i
        while j < n and not (raw[j] in b" \t\r\n/<>[](){}%"):
            j += 1
        if j == i:
            i += 1
            continue
        word = raw[i:j]
        try:
            yield ("num", float(word.decode("ascii")))
        except ValueError:
            yield ("op", word.decode("latin-1"))
        i = j


def tounicode_of(objects: dict[int, bytes], font_body: bytes) -> tuple[int, dict[bytes, str]]:
    """`/ToUnicode` 那张 CMap：codespace 定字节宽，bfchar / bfrange 定映射

    字节宽从 `begincodespacerange` 下界的长度读，不是猜 1 —— 2 字节 CID 字体
    混着 1 字节映射时，宽度读错就整片乱码。
    bfrange 的目标写成一个数时，区间里每个码对应**连续递增**的 Unicode 码元；
    写成数组时逐个对应。两种都要认，只认一种就会整段错位。
    """
    first, widths = _widths_of(font_body)
    blank = {"width": 1, "table": {}, "first": first, "w": widths}
    ref = re.search(rb"/ToUnicode\s+(\d+)\s+\d+\s+R", font_body)
    if not ref:
        return blank
    target = objects.get(int(ref.group(1)))
    if target is None:
        return blank
    raw = stream_of(target)
    if raw is None:
        return blank
    text = raw.decode("latin-1", "replace")
    width = 1
    space = re.search(rb"begincodespacerange(.*?)endcodespacerange", raw, re.S)
    if space:
        probe = re.search(rb"<([0-9A-Fa-f]+)>", space.group(1))
        if probe:
            width = max(1, len(probe.group(1)) // 2)
    table: dict[bytes, str] = {}

    def units_to_text(one: str) -> str:
        digits = re.sub(r"[^0-9A-Fa-f]", "", one)
        if len(digits) % 2:
            digits += "0"
        units = bytes.fromhex(digits)
        if len(units) % 2:
            return units.decode("latin-1", "replace")
        return units.decode("utf-16-be", "replace")

    def src_width(one: str) -> int:
        return max(1, len(re.sub(r"[^0-9A-Fa-f]", "", one)) // 2)

    for block in re.findall(r"beginbfchar(.*?)endbfchar", text, re.S):
        for src, dst in re.findall(r"<([0-9A-Fa-f]+)>\s*<([0-9A-Fa-f]+)>", block):
            table[bytes.fromhex(re.sub(r"[^0-9A-Fa-f]", "", src))] = units_to_text(dst)
    for block in re.findall(r"beginbfrange(.*?)endbfrange", text, re.S):
        # 目标是一个数：区间内逐个加一；目标是数组：按数组给的那些逐个对应
        for line in re.finditer(
            r"<([0-9A-Fa-f]+)>\s*<([0-9A-Fa-f]+)>\s*(\[[^\]]*\]|<([0-9A-Fa-f]+)>)", block
        ):
            lo_hex, hi_hex, form = line.group(1), line.group(2), line.group(3)
            low, high = int(lo_hex, 16), int(hi_hex, 16)
            size = src_width(lo_hex)
            if form.startswith("["):
                pieces = re.findall(r"<([0-9A-Fa-f]+)>", form)
                for offset, piece in enumerate(pieces):
                    if high - low + 1 <= offset:
                        break
                    key = (low + offset).to_bytes(size, "big")
                    table[key] = units_to_text(piece)
                continue
            base = int(line.group(4), 16)
            for offset in range(high - low + 1):
                key = (low + offset).to_bytes(size, "big")
                table[key] = chr(base + offset)
    return {"width": width, "table": table, "first": first, "w": widths}


def decode_run(raw: bytes, width: int, table: dict[bytes, str]) -> str:
    """一段字节码 → 文本：按 codespace 的宽度自长向短试查表，查不到按 Latin-1 交回"""
    if not table:
        return raw.decode("latin-1", "replace")
    out: list[str] = []
    i = 0
    while i < len(raw):
        hit = None
        for size in range(min(width, len(raw) - i), 0, -1):
            key = raw[i : i + size]
            if key in table:
                hit = (table[key], size)
                break
        if hit is None:
            out.append(raw[i : i + 1].decode("latin-1", "replace"))
            i += 1
        else:
            out.append(hit[0])
            i += hit[1]
    return "".join(out)


def _widths_of(font_body: bytes) -> tuple[int, list]:
    """`/FirstChar` 与 `/Widths`：算字形前进量要用（TJ 里那串负数是字距，不是空格）"""
    first = re.search(rb"/FirstChar\s+(-?\d+)", font_body)
    array = re.search(rb"/Widths\s*\[([^\]]*)\]", font_body)
    if not first or not array:
        return 0, []
    numbers = [int(one) for one in re.findall(rb"-?\d+", array.group(1))]
    return int(first.group(1)), numbers


def moved(matrix: list, tx: float, ty: float) -> list:
    """`Td` / 换行都是「先把矩阵乘上平移矩阵」：e' = a·tx + c·ty + e，f' = b·tx + d·ty + f"""
    a, b, c, d, e, f = matrix
    return [a, b, c, d, a * tx + c * ty + e, b * tx + d * ty + f]


def times(left: list, right: list) -> list:
    """两个仿射矩阵相乘（PDF 的 6 数写法 [a b c d e f]）"""
    a1, b1, c1, d1, e1, f1 = left
    a2, b2, c2, d2, e2, f2 = right
    return [
        a1 * a2 + c1 * b2,
        b1 * a2 + d1 * b2,
        a1 * c2 + c1 * d2,
        b1 * c2 + d1 * d2,
        a1 * e2 + c1 * f2 + e1,
        b1 * e2 + d1 * f2 + f1,
    ]


def apply(matrix: list, x: float, y: float) -> tuple[float, float]:
    """把一点从文本空间送到页面空间"""
    a, b, c, d, e, f = matrix
    return (a * x + c * y + e, b * x + d * y + f)


def _text_runs(raw: bytes, fonts: dict[str, dict]) -> list[tuple[float, float, float, float, str]]:
    """走一遍文本算符，交回 (x, y, 文本) 一列

    为什么要走到这一步：LibreOffice 写一个中文标题会拆成好几段 `TJ`，段与段之间还
    带着几千分之一个字的大负数（那是字距补偿，不是空格）。**照书写顺序拼字符串会把
    一行拼成「一：算口径级标题预」这种看着像乱码的东西** —— 只有把每段的起点 x 真的
    算出来、再按 (行, x) 排，才谈得上「这份文件写了什么」。
    """
    runs: list[tuple[float, float, float, float, str]] = []
    blank = {"width": 1, "table": {}, "first": 0, "w": []}
    font: dict = blank
    size = 10.0
    leading = 0.0
    # 文本矩阵与文本行矩阵：`BT` 把两者都复位成单位阵（PDF 就是这么规定的），
    # `Td` 在前进一步走一行，`T*` 回到那一行的开头。不复位就会把上一行的坐标
    # 一路累加下去 —— 这边第一版就是这么把同一行的三段分到三个「行」里去的。
    tm = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]
    tl = list(tm)
    # 图形矩阵：`q`/`Q` 是压栈与弹栈，`cm` 右乘一个矩阵。LibreOffice 给每个带标签的
    # 文本段套一层 `q … cm … Q`，只算文本矩阵就会把整段送到错的位置上去（'：' 会被
    # 排到行首左边）—— 页面坐标 = CTM × 文本矩阵。
    ctm = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]
    stack: list[list] = []
    operands: list = []
    array: list | None = None

    def at() -> tuple[float, float]:
        return apply(times(ctm, tm), 0.0, 0.0)

    def advance(codes: bytes, kern: float = 0.0) -> None:
        step = 0.0
        i = 0
        while i < len(codes):
            take = 1
            for want in range(min(font["width"], len(codes) - i), 0, -1):
                if codes[i : i + want] in font["table"]:
                    take = want
                    break
            code = int.from_bytes(codes[i : i + take], "big")
            index = code - font["first"]
            width = font["w"][index] if 0 <= index < len(font["w"]) else 0
            step += width / 1000.0 * size
            i += take
        # `TJ` 数组里那个数是**反着**的：正数把笔往左推、负数往右推（PDF §9.4.4）。
        # 当成正常符号就会把每一段都往前挪，`notes.pdf` 的标题会被挪到行首左边 84 点。
        step -= kern / 1000.0 * size
        tm[4] += step * tm[0]
        tm[5] += step * tm[1]

    def emit(codes: bytes) -> None:
        if not codes:
            return
        x, y = at()
        text = decode_run(codes, font["width"], font["table"])
        advance(codes)
        runs.append((x, y, at()[0], size, text))

    for kind, value in _tokens(raw):
        if kind == "bracket":
            if value == b"[":
                array = []
            elif array is not None:
                operands.append(list(array))
                array = None
            continue
        if kind == "dict":
            operands = []
            array = None
            continue
        if array is not None:
            if kind in ("str", "num"):
                array.append(value)
            continue
        if kind in ("name", "num", "str"):
            operands.append(value)
            continue
        numbers = [one for one in operands if isinstance(one, float)]
        if value == "q":
            stack.append(list(ctm))
        elif value == "Q":
            if stack:
                ctm = stack.pop()
        elif value == "cm" and len(numbers) >= 6:
            ctm = times(ctm, [float(one) for one in numbers[:6]])
        elif value == "Tf":
            name = next((one for one in reversed(operands) if isinstance(one, str)), "")
            font = fonts.get(name, blank)
            if numbers:
                size = numbers[-1]
        elif value == "Tm" and len(numbers) >= 6:
            tm = [float(one) for one in numbers[:6]]
            tl = list(tm)
        elif value == "Td" and len(numbers) >= 2:
            tm = moved(tm, numbers[-2], numbers[-1])
            tl = list(tm)
        elif value == "TD" and len(numbers) >= 2:
            leading = -numbers[-1]
            tm = moved(tm, numbers[-2], numbers[-1])
            tl = list(tm)
        elif value == "TL" and numbers:
            leading = numbers[-1]
        elif value == "T*":
            tm = moved(tl, 0.0, -leading)
            tl = list(tm)
        elif value == "BT":
            tm = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]
            tl = list(tm)
        elif value == "Tj":
            hit = next((one for one in reversed(operands) if isinstance(one, bytes)), b"")
            emit(hit)
        elif value in ("'", '"'):
            tm = moved(tl, 0.0, -leading)
            tl = list(tm)
            hit = next((one for one in reversed(operands) if isinstance(one, bytes)), b"")
            emit(hit)
        elif value == "TJ":
            for one in operands:
                if not isinstance(one, list):
                    continue
                for piece in one:
                    if isinstance(piece, bytes):
                        emit(piece)
                    elif isinstance(piece, float):
                        advance(b"", piece)
        operands = []
    return runs


def _layout(runs: list[tuple[float, float, float, float, str]], size: float = 12.0) -> str:
    """按 (行, x) 重排：y 相近的算同一行，行内按起点 x 排，段间按距离补一个空格

    行容差跟着字号走（半个字高）：表格里「服务器」与右对齐的「124000」用了两张字体，
    基线差 4.3 点 —— 固定 2 点会把它们拆成两行，按字号算才合成一行，也就是眼睛看到的那一行。
    """
    if not runs:
        return ""
    lines: list[list] = []
    tolerance = max(2.0, size * 0.5)
    for x, y, end, _size, text in runs:
        for base, items in lines:
            if abs(base - y) <= tolerance:
                items.append((x, end, text))
                break
        else:
            lines.append([y, [(x, end, text)]])
    lines.sort(key=lambda one: -one[0])  # PDF 的 y 向上：从页顶往下读
    out: list[str] = []
    for _base, items in lines:
        # 同一行内按起点 x 排：位置算对了之后，这才是读的顺序
        items.sort(key=lambda one: one[0])
        # 两段之间只有「真有空隙、且前一段结尾不是空白」时才补一个空格
        line: list[str] = []
        edge = None
        for x, end, text in items:
            if edge is not None and x - edge > 1.0 and line and not line[-1].endswith(" "):
                line.append(" ")
            line.append(text)
            edge = end
        out.append("".join(line))
    return "\n".join(out)


def page_order(by_id: dict[int, bytes]) -> list[int]:
    """按页树 `/Kids` 的顺序走一遍（放映顺序就是这个，不是对象号顺序）"""
    root_ref = None
    for key, body in by_id.items():
        if re.search(rb"/Type\s*/Catalog\b", body):
            hit = re.search(rb"/Pages\s+(\d+)\s+\d+\s+R", body)
            if hit:
                root_ref = int(hit.group(1))
            break
    if root_ref is None:
        return []
    order: list[int] = []
    stack = [root_ref]
    guard = set()
    while stack:
        node = stack.pop(0)
        if node in guard or node not in by_id:
            continue
        guard.add(node)
        body = by_id[node]
        kids = re.search(rb"/Kids\s*\[([^\]]*)\]", body)
        if kids:
            refs = [int(one) for one in re.findall(rb"(\d+)\s+\d+\s+R", kids.group(1))]
            stack = refs + stack
        elif re.search(rb"/Type\s*/Page\b(?!s)", body):
            order.append(node)
    return order


def page_text(data: bytes) -> list[str]:
    """每页一份正文：内容流里按顺序走文本算符，字节码用当时那张字体的 ToUnicode 解"""
    plain, _dups = scan_objects(data)
    inner, _streams = unpack_object_streams(data, plain)
    by_id = dict(plain)
    for key, body in inner.items():
        by_id.setdefault(key, body)
    fonts: dict[str, tuple[int, dict[bytes, str]]] = {}
    pages = []
    for page_id in page_order(by_id):
        body = by_id[page_id]
        # 字体名 → CMap：Resource 可能是内联字典也可能是间接引用，`/Font` 那一层同样两种写法
        resource = body
        ref = re.search(rb"/Resources\s+(\d+)\s+\d+\s+R", body)
        if ref and int(ref.group(1)) in by_id:
            resource = by_id[int(ref.group(1))]
        elif ref:
            resource = body  # 指不到就退回页自己那份（继承 Resource 不在这次的界里）
        own: dict[str, dict] = {}
        for name, target in re.findall(rb"/(\w+)\s+(\d+)\s+\d+\s+R", _font_table(resource, by_id)):
            own[name.decode("latin-1")] = tounicode_of(by_id, by_id.get(int(target), b""))
        # 页与页之间会复用同名条目（/F1 在第二页可能是另一张字体）：以本页为准，
        # 本页没写时才沿用上一页的（Resource 继承的那一种）
        merged = dict(fonts)
        merged.update(own)
        runs: list[tuple[float, float, float, float, str]] = []
        for one in _content_of(by_id, body):
            runs.extend(_text_runs(one, merged))
        # 行容差跟这一页最大的字号走（与 Rust 侧 `page_texts` 同一条规则）
        pages.append(_layout(runs, max((one[3] for one in runs), default=12.0)))
        fonts = merged
    return pages


def _font_table(resource: bytes, by_id: dict[int, bytes]) -> bytes:
    """Resource 里的 `/Font` 既可能是内联字典，也可能**整个是一个间接对象**

    LibreOffice 写的就是 `/Font 66 0 R` —— 只认内联那一种的读者会把 `Font` 这个
    名字当成第一张字体，于是一整页的码都解不出来（这边第一版就是这么错的）。
    """
    hit = re.search(rb"/Font\s*<<(.*?)>>", resource, re.S)
    if hit:
        return hit.group(1)
    ref = re.search(rb"/Font\s+(\d+)\s+\d+\s+R", resource)
    if ref and int(ref.group(1)) in by_id:
        target = by_id[int(ref.group(1))]
        inner = re.search(rb"<<(.*)>>", target, re.S)
        return inner.group(1) if inner else target
    return b""


def _content_of(by_id: dict[int, bytes], page_body: bytes) -> list[bytes]:
    refs: list[int] = []
    hit = re.search(rb"/Contents\s+(\d+)\s+\d+\s+R", page_body)
    if hit:
        refs.append(int(hit.group(1)))
    else:
        array = re.search(rb"/Contents\s*\[([^\]]*)\]", page_body)
        if array:
            refs = [int(one) for one in re.findall(rb"(\d+)\s+\d+\s+R", array.group(1))]
    out = []
    for one in refs:
        body = by_id.get(one)
        if body is None:
            continue
        raw = stream_of(body)
        if raw is not None:
            out.append(raw)
    return out


if __name__ == "__main__":
    import json
    import sys
    from pathlib import Path

    if "--text" in sys.argv[1:]:
        names = [one for one in sys.argv[1:] if not one.startswith("--")]
        for one in names:
            print(json.dumps(page_text(Path(one).read_bytes()), ensure_ascii=False, indent=1))
    else:
        for one in sys.argv[1:]:
            print(json.dumps(pdf_facts(Path(one).read_bytes()), ensure_ascii=False, indent=1))

