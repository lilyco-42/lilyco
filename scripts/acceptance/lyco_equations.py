"""公式那一本账的第二读者：OOXML 的 OMML 与 ODF 的嵌入 MathML 各按自己文件写的读。

与 `lilyco-binfmt/src/equations.rs` 同口径，两条判据必须一致：

- **按局部名找，不按我以为的前缀名**：`svg:width` 在 LibreOffice 的 odt 里挂的命名空间是
  `urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0`（不是 SVG 官方那个 URI），
  所以这里按「去掉前缀的名字、文档顺序里第一个」取，与 Rust 的 `attr_local` 一模一样；
- 式子的「字」只拼 `m:t`（OMML）与 `mi/mn/mo/mtext`（MathML），两处都是文件自己写着的字符，
  不做任何线性化推断（ODF 那侧的 StarMath 线性式在 `<annotation>` 里，按写的原样交，不拿来代替字）。
"""
import re
import zipfile
from pathlib import Path
from xml.etree import ElementTree as ET

M = "{http://schemas.openxmlformats.org/officeDocument/2006/math}"
W = "{http://schemas.openxmlformats.org/wordprocessingml/2006/main}"
OFFICE = "{urn:oasis:names:tc:opendocument:xmlns:office:1.0}"
TEXT = "{urn:oasis:names:tc:opendocument:xmlns:text:1.0}"
DRAW = "{urn:oasis:names:tc:opendocument:xmlns:drawing:1.0}"
XLINK = "{http://www.w3.org/1999/xlink}"

# MathML 里「算字」的那几类（Rust 侧同一份名单）
MATH_TEXT = ("mi", "mn", "mo", "mtext")


def local(tag: str) -> str:
    return tag.rsplit("}", 1)[-1] if "}" in tag else tag


def parts_of(path: Path) -> dict:
    with zipfile.ZipFile(path) as box:
        return {one.filename: box.read(one.filename) for one in box.infolist()}


CALCEXT = "{urn:oasis:names:tc:opendocument:xmlns:calcext:1.0}"


def attr_local(node, want: str):
    """文档顺序里第一个局部名等于 want 的属性

    与 Rust 的 `odsheet::attr_of` 同一条判据：LibreOffice 抄的那份 `calcext:` 副本不算
    （它按写的命名空间展开成 `{…calcext:1.0}xxx`）。
    """
    for key, value in node.attrib.items():
        if key.startswith(CALCEXT):
            continue
        if local(key) == want:
            return value
    return None


def kids(node, want: str):
    return [one for one in node if local(one.tag) == want and not one.tag.startswith("<?")]


# ── OOXML：OMML ───────────────────────────────────────────────────────────────
def docx_equations(path: Path) -> dict:
    parts = parts_of(path)
    if "word/document.xml" not in parts:
        return {"family": "ooxml", "available": False}
    root = ET.fromstring(parts["word/document.xml"])
    body = kids(root, "body")
    items = []
    structures_seen = []
    stats = {"equations_total": 0, "inline_total": 0, "display_total": 0,
             "paragraphs_with": 0, "align_written_total": 0, "nor_runs": 0,
             "lit_runs": 0, "math_runs": 0, "text_chars": 0, "o_math_para_total": 0}
    paragraphs = body[0] if body else root
    for p_index, par in enumerate(one for one in paragraphs if local(one.tag) == "p"):
        hosts = list(par)
        if not hosts:
            continue
        found = 0
        for child in hosts:
            kind = local(child.tag)
            if kind == "oMath":
                found += 1
                items.append(omath_item(child, len(items), p_index, "inline", "w:p", None,
                                        structures_seen, stats))
            elif kind == "oMathPara":
                found += 1
                stats["o_math_para_total"] += 1
                jc = None
                for props in (one for one in child if local(one.tag) == "oMathParaPr"):
                    hit = kids(props, "jc")
                    if hit:
                        jc = attr_local(hit[0], "val")
                for inner in kids(child, "oMath"):
                    items.append(omath_item(inner, len(items), p_index, "display",
                                            "m:oMathPara", jc, structures_seen, stats))
        if found:
            stats["paragraphs_with"] += 1
    stats["equations_total"] = len(items)
    return {"family": "ooxml", "available": True, "items": items,
            "paragraphs_total": sum(1 for one in paragraphs if local(one.tag) == "p"),
            "structures_seen": structures_seen, **stats}


def omath_item(node, index, p_index, placement, host, jc, seen, stats) -> dict:
    names = []
    for one in node.iter():
        if one is node:
            continue
        if not one.tag.startswith(M):
            continue
        name = local(one.tag)
        if name in ("oMath", "r", "t"):
            continue
        names.append(name)
        if name not in seen:
            seen.append(name)
    text = "".join("".join(one.itertext()) for one in node.iter(M + "t"))
    nor = sum(1 for one in node.iter() if local(one.tag) == "nor")
    lit = sum(1 for one in node.iter() if local(one.tag) == "lit")
    runs = sum(1 for one in node.iter() if local(one.tag) == "r")
    stats["inline_total" if placement == "inline" else "display_total"] += 1
    if jc is not None:
        stats["align_written_total"] += 1
    stats["nor_runs"] += nor
    stats["lit_runs"] += lit
    stats["math_runs"] += runs
    stats["text_chars"] += len(text)
    return {"index": index, "paragraph": p_index, "placement": placement, "host": host,
            "align_written": jc, "structures": names, "runs": runs, "nor_runs": nor,
            "lit_runs": lit, "text": text}


# ── ODF：嵌入对象里的 MathML ──────────────────────────────────────────────────
def odf_equations(path: Path) -> dict:
    parts = parts_of(path)
    if "content.xml" not in parts:
        return {"family": "odf", "available": False}
    root = ET.fromstring(parts["content.xml"])
    body = [one for one in root.iter() if one.tag == OFFICE + "text"]
    body = body[0] if body else root
    items = []
    elements_seen = []
    stats = {"frames_seen": 0, "objects_total": 0, "parts_found": 0, "parts_missing": 0,
             "math_found": 0, "block_written": 0, "inline_written": 0, "display_missing": 0,
             "annotations_found": 0, "replacements_written": 0, "text_chars": 0,
             "objects_without_math": 0}
    par_index = -1
    for par in body:
        if local(par.tag) not in ("p", "h"):
            continue
        par_index += 1
        for frame in (one for one in par if local(one.tag) == "frame"):
            stats["frames_seen"] += 1
            obj = [one for one in frame.iter() if local(one.tag) == "object"]
            obj = obj[0] if obj else None
            if obj is None:
                continue
            stats["objects_total"] += 1
            href = obj.get(XLINK + "href") or ""
            member = href.lstrip("./") + "/content.xml" if href else ""
            found = member in parts
            if found:
                stats["parts_found"] += 1
            else:
                stats["parts_missing"] += 1
            repl = [one for one in frame.iter() if local(one.tag) == "image"]
            repl_target = (repl[0].get(XLINK + "href") or "") if repl else None
            if repl_target:
                stats["replacements_written"] += 1
            display, elems, text, enc, source = mathml_of(parts.get(member, b""))
            # 部件里有没有 `<math>` 根：这是「这条对象真是条公式吗」唯一的凭据，
            # 图表那类嵌入对象也有 draw:object，不能靠地址形状猜
            is_math = bool(elems)
            if not is_math:
                # 部件在而里面没有 `<math>`：这条不是公式（图表那类也是 draw:object），
                # 于是 display、批注与字那几个数一个都不涨，只留下这一行
                stats["objects_without_math"] += 1
            else:
                stats["math_found"] += 1
                if display == "block":
                    stats["block_written"] += 1
                elif display == "inline":
                    stats["inline_written"] += 1
                else:
                    stats["display_missing"] += 1
                if source is not None:
                    stats["annotations_found"] += 1
                for name in elems:
                    if name not in elements_seen:
                        elements_seen.append(name)
                stats["text_chars"] += len(text)
            items.append({
                "index": len(items),
                "paragraph": par_index,
                "frame_name": attr_local(frame, "name"),
                "style_written": attr_local(frame, "style-name"),
                "anchor_written": attr_local(frame, "anchor-type"),
                "width_written": attr_local(frame, "width"),
                "height_written": attr_local(frame, "height"),
                "z_written": attr_local(frame, "z-index"),
                "object_target": href,
                "object_part": member if found else None,
                "part_found": found,
                "math_found": is_math,
                "replacement_target": repl_target,
                "display_written": display,
                "elements": elems,
                "text": text,
                "annotation_encoding": enc,
                "annotation_source": source,
            })
    return {"family": "odf", "available": True, "items": items,
            "equations_total": stats["math_found"],
            "paragraphs_total": par_index + 1, "elements_seen": elements_seen, **stats}


def mathml_of(blob: bytes):
    """一个公式部件里的 MathML：display 按写的交，元素按文档顺序，字只拼那四类"""
    if not blob:
        return None, [], "", None, None
    try:
        node = ET.fromstring(blob)
    except ET.ParseError:
        return None, [], "", None, None
    elems = [local(one.tag) for one in node.iter()]
    text = "".join("".join(one.itertext())
                   for one in node.iter() if local(one.tag) in MATH_TEXT)
    ann = [one for one in node.iter() if local(one.tag) == "annotation"]
    if ann:
        return (node.get("display"), elems, text, ann[0].get("encoding"),
                "".join(ann[0].itertext()))
    return node.get("display"), elems, text, None, None
