"""PDF 的「去哪儿」那一层：书签（/Outlines）、页内链接（/Link 注记）、加密件的权限位（/P）。

第二读者，只用标准库；Rust 那边在 `lilyco-binfmt/src/pdf.rs` 与 `src/office_pdf.rs`。
两条与主读者相同的判断（都是 CI 上踩出来的）：

* `/Key` 找的是**整个名字**（`/Page` 不算 `/Pages`），但键带着 `/` 时**前面**是什么都不管 ——
  `/` 本身就是分隔符，而生产者写紧凑字典是 `/Size 69/Root 9 0 R` 连着写的。
* `/A 12 0 R` 里版本号与 `R` 之间可以有空白，不留这一步就什么都找不到。

三条这一版自己踩到的坑（都写进取法里了）：
1. `/Outlines` 那个字典**不是**一条书签，第一条在它的 `/First` 上；`/Count` 的**符号**说的是
   展开还是折叠，绝对值才是子孙数。
2. 字符串两种写法：LibreOffice 把中文标题写成 `/Title <FEFF…>`（十六进制的 UTF-16BE），
   不是 `/Title (…)`。所以取字要走主读者的 `strings_of` + `decode_pdf_text` —— 从 `/Title`
   末尾往后找括号会一路吞到字典末尾，把 `/Dest` 也吞进「标题」里。
3. `/P -4196` 是**负数**：不带符号的整数读法直接读不出来。
"""

from __future__ import annotations

import re

from lyco_pdf import (
    decode_pdf_text,
    number,
    page_order,
    scan_objects,
    strings_of,
    unpack_object_streams,
)

NOT_NAME = rb"(?![A-Za-z0-9._+\-%])"


def key(body: bytes, name: bytes):
    """`/Name` 的所有出现位置（整段名字比对，前面不挑）"""
    return re.finditer(rb"/" + re.escape(name) + NOT_NAME, body)


def ref(body: bytes, name: bytes) -> int | None:
    """`/Name 12 0 R` 里的对象号（版本号与 R 之间可有空白）"""
    for hit in key(body, name):
        got = re.match(rb"\s*(\d+)\s+(\d+)\s+R" + NOT_NAME, body[hit.end():])
        if got:
            return int(got.group(1))
    return None


def one_text(body: bytes, name: bytes) -> str | None:
    """`/Name (…)` 与 `/Name <FEFF…>` 两种写法都认，再按 PDF 字符串规则解"""
    got = strings_of(body, rb"/" + name)
    return decode_pdf_text(got[0]) if got else None


def signed(body: bytes, name: bytes) -> int | None:
    """`/P -4196`：权限那个数是带符号的"""
    for hit in key(body, name):
        got = re.match(rb"\s*(-?\d+)(?![0-9])", body[hit.end():])
        if got:
            return int(got.group(1))
    return None


def all_objects(data: bytes) -> dict:
    """明写的对象 + 对象流里打包的那些"""
    plain, _dup = scan_objects(data)
    inner, _streams = unpack_object_streams(data, plain)
    by_id = dict(plain)
    for num, body in inner.items():
        by_id.setdefault(num, body)
    return by_id


def catalog(by_id: dict) -> bytes:
    for body in by_id.values():
        if re.search(rb"/Type\s*/Catalog" + NOT_NAME, body):
            return body
    return rb""


def target(body: bytes) -> tuple[int | None, str, str | None]:
    """这一处想去哪：交回 (页对象号, 形态, 目标名 / 动作类型)"""
    for name in (rb"Dest", rb"D"):
        for hit in key(body, name):
            tail = body[hit.end(): hit.end() + 240]
            arr = re.match(rb"\s*\[\s*(\d+)\s+\d+\s+R\s*/(\w+)", tail)
            if arr:
                return int(arr.group(1)), "explicit", arr.group(2).decode("ascii")
            arr = re.match(rb"\s*\[\s*/(\w+)", tail)
            if arr:
                return None, "current-page", arr.group(1).decode("ascii")
            named = re.match(rb"\s*\(([^)]*)\)", tail)
            if named:
                return None, "named", named.group(1).decode("latin-1", "replace")
            ref_hit = re.match(rb"\s*(\d+)\s+\d+\s+R", tail)
            if ref_hit:
                return int(ref_hit.group(1)), "page-object", None
    for hit in key(body, rb"A"):
        act = body[hit.end(): hit.end() + 400]
        kind = re.search(rb"/S\s*/(\w+)", act)
        label = kind.group(1).decode("ascii") if kind else ""
        if label == "URI":
            return None, "uri", one_text(act, rb"URI")
        page, form, extra = target(act)
        if page is not None or form != "none":
            return page, form, label or extra
    return None, "none", None


def outlines(data: bytes, encrypted: bool = False) -> dict:
    """书签：从 /Outlines 的 /First 起，按 /Next 走，子层再走 /First"""
    by_id = all_objects(data)
    root = ref(catalog(by_id), rb"Outlines")
    if root is None or root not in by_id:
        return {"present": False, "items": [], "declared_count": None}
    order = page_order(by_id)
    items: list = []
    seen: set = set()

    def walk(node, depth: int) -> None:
        while (
            node is not None
            and node in by_id
            and node not in seen
            and len(items) < 400
        ):
            seen.add(node)
            body = by_id[node]
            page, form, via = target(body)
            count = number(body, rb"/Count") or 0
            items.append(
                {
                    "depth": depth,
                    "title": None if encrypted else one_text(body, rb"Title"),
                    "page_object": page,
                    "page": (order.index(page) + 1) if page in order else None,
                    "target": form,
                    "via": via,
                    "children": abs(count),
                    "closed": count < 0,
                }
            )
            kid = ref(body, rb"First")
            if kid is not None:
                walk(kid, depth + 1)
            node = ref(body, rb"Next")

    walk(ref(by_id[root], rb"First"), 0)
    return {
        "present": True,
        "items": items,
        "declared_count": number(by_id[root], rb"/Count"),
    }


def links(data: bytes, encrypted: bool = False) -> dict:
    """页上 `/Subtype /Link` 的注记：往站外 / 往内 / 别的动作"""
    by_id = all_objects(data)
    order = page_order(by_id)
    internal: list = []
    external: list = []
    other: list = []
    for index, page in enumerate(order):
        body = by_id.get(page, rb"")
        refs: list = []
        for hit in key(body, rb"Annots"):
            tail = body[hit.end(): hit.end() + 400]
            inline = re.match(rb"\s*\[([^\]]*)\]", tail)
            if inline:
                refs += [
                    int(one) for one in re.findall(rb"(\d+)\s+\d+\s+R", inline.group(1))
                ]
                continue
            got = re.match(rb"\s*(\d+)\s+\d+\s+R", tail)
            if got:
                refs.append(int(got.group(1)))
        for num in refs:
            annot = by_id.get(num)
            if annot is None or not re.search(rb"/Subtype\s*/Link" + NOT_NAME, annot):
                continue
            to_page, form, via = target(annot)
            if form == "uri":
                external.append({"page": index + 1, "uri": None if encrypted else via})
            elif to_page is not None:
                internal.append(
                    {
                        "page": index + 1,
                        "target_object": to_page,
                        "target_page": (order.index(to_page) + 1) if to_page in order else None,
                        "via": form,
                    }
                )
            else:
                other.append({"page": index + 1, "via": form, "kind": via})
    return {
        "internal": internal,
        "external": external,
        "other": other,
        "link_objects": sum(
            1 for one in by_id.values() if re.search(rb"/Subtype\s*/Link" + NOT_NAME, one)
        ),
    }


def annotations(data: bytes, encrypted: bool = False) -> dict:
    """页上 `/Annots` 的整本账：链接、批注与弹出框都算一条注记

    一条 `/Text` 通常另配一条 `/Popup`，而那个框自己也在同一个数组里 —— 所以
    「几条注记」与「几条批注」是两个数，谁也不替谁圆场。`modified` 连 `D:` 前缀
    原样交（LibreOffice 在那里写的是一串全零），不替它解成日期也不替它改成 null。
    """
    by_id = all_objects(data)
    order = page_order(by_id)
    items: list = []
    for index, page in enumerate(order):
        body = by_id.get(page, rb"")
        refs: list = []
        for hit in key(body, rb"Annots"):
            tail = body[hit.end(): hit.end() + 400]
            inline = re.match(rb"\s*\[([^\]]*)\]", tail)
            if inline:
                refs += [int(one) for one in re.findall(rb"(\d+)\s+\d+\s+R", inline.group(1))]
                continue
            got = re.match(rb"\s*(\d+)\s+\d+\s+R", tail)
            if got:
                refs.append(int(got.group(1)))
        for num in refs:
            annot = by_id.get(num)
            if annot is None:
                continue
            had = re.search(rb"/Subtype\s*/([A-Za-z0-9._+-]+)" + NOT_NAME, annot)
            items.append(
                {
                    "page": index + 1,
                    "object": num,
                    "subtype": had.group(1).decode("latin-1") if had else None,
                    "author": None if encrypted else one_text(annot, rb"T"),
                    "contents": None if encrypted else one_text(annot, rb"Contents"),
                    "modified": None if encrypted else one_text(annot, rb"M"),
                    "popup": ref(annot, rb"Popup"),
                    "parent": ref(annot, rb"Parent"),
                }
            )
    kinds: dict = {}
    for one in items:
        key_name = one["subtype"] if one["subtype"] is not None else ""
        kinds[key_name] = kinds.get(key_name, 0) + 1
    return {
        "total": len(items),
        "notes": sum(1 for one in items if one["subtype"] == "Text"),
        "popups": sum(1 for one in items if one["subtype"] == "Popup"),
        "links": sum(1 for one in items if one["subtype"] == "Link"),
        "no_subtype": sum(1 for one in items if one["subtype"] is None),
        "by_subtype": [{"subtype": one, "count": kinds[one]} for one in sorted(kinds)],
        "items": items,
    }


def permissions(data: bytes) -> dict:
    """`/Encrypt` 的 `/P`：位号从 1 起算（规范就是这么编号的），R2 只有 3~6 位有意义"""
    by_id = all_objects(data)
    holder = None
    for body in by_id.values():
        hit = next(key(body, rb"Encrypt"), None)
        if hit is None:
            continue
        got = re.match(rb"\s*(\d+)\s+\d+\s+R", body[hit.end(): hit.end() + 40])
        if got:
            holder = int(got.group(1))
            break
    if holder is None:
        # 键只住在 trailer 里时（有的文件连 trailer 都没有），认那个带 /Filter/Standard 的对象
        plain, _dup = scan_objects(data)
        for num, body in plain.items():
            if re.search(rb"/Filter\s*/(Standard|AdobePS|AESV2|AESV3)", body):
                holder = num
                break
    if holder is None or holder not in by_id:
        return {"encrypted": False, "revision": None, "raw": None, "permissions": None}
    enc = by_id[holder]
    raw = signed(enc, rb"P")
    rev = number(enc, rb"/R")
    if raw is None:
        return {
            "encrypted": True,
            "revision": rev,
            "object": holder,
            "raw": None,
            "permissions": None,
        }

    def bit(n: int) -> bool:
        return bool((raw >> (n - 1)) & 1)

    wide = (rev or 0) >= 3
    return {
        "encrypted": True,
        "revision": rev,
        "object": holder,
        "raw": raw,
        "permissions": {
            "print": bit(3),
            "modify": bit(4),
            "copy": bit(5),
            "annotate": bit(6),
            "forms": bit(9) if wide else None,
            "extract_for_screen": bit(10) if wide else None,
            "assemble": bit(11) if wide else None,
            "print_high_quality": bit(12) if wide else None,
        },
    }
