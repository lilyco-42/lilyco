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


def _iter_all(node):
    """整棵子树（含自己），文档顺序 —— 与 Rust 的 `descendants` + 自己同一条"""
    out = [node]
    for one in node:
        out.extend(_iter_all(one))
    return out


def _descendables(node):
    """子树里**除自己以外**的元素，文档顺序 —— 与 Rust 的 `subtree` 同一条"""
    out = []
    for one in node:
        out.append(one)
        out.extend(_descendables(one))
    return out


def _page_walk(node, in_notes):
    """页里的每一枚 `draw:frame`，带上「它是不是在 `presentation:notes` 里面」

    不能「自底往上找祖先再比身份」：ElementTree 每次迭代子节点都换一个包装对象出来，
    `kid is target` 永远不成立（这一条本机测出来过：注块的排除静默失效，frame 全算成页上的）。
    所以自顶往下带一个标志 —— Rust 那一侧没有父指针，走的也是同样带标志的递归。
    """
    for one in node:
        kind = local(one.tag)
        deeper = in_notes or kind == "notes"
        if kind == "frame":
            yield one, deeper
        yield from _page_walk(one, deeper)


def odp_equations(path: Path) -> dict:
    """一份 odp 的公式账：式子在**每页**的 frame 里，而页缩略图也是 frame，两格必须分开数

    与 `odf_equations`（odt）同一套判据（局部名、部件里要有 `<math>` 根才算式子），
    多出来的是这两条：
    - `page_thumbnails` 单记 `draw:page-thumbnail` —— LibreOffice 的 odp 重写给**每页**插一枚
      （包在一枚没有名字的 `draw:frame` 里，宽 16.799cm），混进 `frames_seen` 就会把公式数顶高；
    - `replacement_found` 问「那枚替位图的地址在包里吗」—— 实测重写给第二枚对象写了
      `./ObjectReplacements/Object 2`，而**包里没有这个部件、清单里也没有这一条**。
    备注块（`presentation:notes`）里的 frame 一个都不算：注里的东西不是页上的东西。
    """
    parts = parts_of(path)
    if "content.xml" not in parts:
        return {"family": "odp", "available": False}
    root = ET.fromstring(parts["content.xml"])
    holder = [one for one in root.iter() if one.tag == OFFICE + "presentation"]
    if not holder:
        return {"family": "odp", "available": False}
    body = holder[0]
    items, pages, elements_seen = [], [], []
    stats = {"pages_total": 0, "frames_seen": 0, "page_thumbnails": 0, "objects_total": 0,
             "math_found": 0, "objects_without_math": 0, "parts_found": 0, "parts_missing": 0,
             "replacements_written": 0, "replacements_missing": 0, "block_written": 0,
             "inline_written": 0, "display_missing": 0, "annotations_found": 0, "text_chars": 0,
             "anchors_written": 0, "frames_in_notes": 0}
    for page in (one for one in body if local(one.tag) == "page"):
        stats["pages_total"] += 1
        index = stats["pages_total"] - 1
        frames = 0
        notes_frames = 0
        thumbs = 0
        formulas = 0
        for one, inside_notes in _page_walk(page, False):
            # 注块里的 frame 不算页上的 frame：notes 是另一本账。实测重写给每页在
            # `presentation:notes` 里加一枚 frame，而 `draw:page-thumbnail` 直接挂在 notes 下面
            if inside_notes:
                notes_frames += 1
                continue
            frames += 1
            obj = [x for x in _descendables(one) if local(x.tag) == "object"]
            if not obj:
                continue
            stats["objects_total"] += 1
            formulas += 1
            href = obj[0].get(XLINK + "href") or ""
            member = (href[2:] if href.startswith("./") else href).lstrip("/")
            member = member.rstrip("/") + "/content.xml" if member else ""
            found = bool(member) and member in parts
            stats["parts_found" if found else "parts_missing"] += 1
            image = [x for x in _iter_all(one) if local(x.tag) == "image"]
            repl = (image[0].get(XLINK + "href") or "") if image else None
            repl_found = None
            if repl is not None:
                stats["replacements_written"] += 1
                stem = (repl[2:] if repl.startswith("./") else repl).lstrip("/")
                repl_found = stem in parts
                if not repl_found:
                    stats["replacements_missing"] += 1
            display, elems, text, enc, source = mathml_of(parts.get(member, b"")) if found \
                else (None, [], "", None, None)
            is_math = bool(elems)
            if is_math:
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
            else:
                stats["objects_without_math"] += 1
            anchor = attr_local(one, "anchor-type")
            if anchor is not None:
                stats["anchors_written"] += 1
            items.append({
                "index": len(items),
                "page": index,
                "frame_name": attr_local(one, "name"),
                "style_written": attr_local(one, "style-name"),
                "anchor_written": anchor,
                "width_written": attr_local(one, "width"),
                "height_written": attr_local(one, "height"),
                "z_written": attr_local(one, "z-index"),
                "object_target": href,
                "object_part": member if found else None,
                "part_found": found,
                "math_found": is_math,
                "replacement_target": repl,
                "replacement_found": repl_found,
                "display_written": display,
                "elements": elems,
                "text": text,
                "annotation_encoding": enc,
                "annotation_source": source,
            })
        for one in (x for x in _iter_all(page) if local(x.tag) == "page-thumbnail"):
            thumbs += 1
        stats["frames_seen"] += frames
        stats["frames_in_notes"] += notes_frames
        stats["page_thumbnails"] += thumbs
        pages.append({"page": index, "frames": frames, "frames_in_notes": notes_frames,
                      "thumbnails": thumbs, "formulas": formulas})
    stats["equations_total"] = stats["math_found"]
    return {"family": "odp", "available": True, "items": items, "pages": pages,
            "elements_seen": elements_seen, **stats}


def mathml_of(blob: bytes):
    """一个公式部件里的 MathML：display 按写的交，元素按文档顺序，字只拼那四类

    **先要有一枚 `<math>` 根**：`Object N/` 这一族既装公式也装图表，图表的
    `content.xml` 一样解析得开 —— 以前这里只判「解析得出」，于是 `deck-chart.odp` 里
    两枚图表对象被报成两条公式（Rust 那一侧一直是按 `<math>` 根判的，两家在 CI 上
    对不上才暴露）。没有 `<math>` 根就交回那一组空值，与 Rust 同一条哨兵。
    """
    if not blob:
        return None, [], "", None, None
    try:
        node = ET.fromstring(blob)
    except ET.ParseError:
        return None, [], "", None, None
    roots = [one for one in node.iter() if local(one.tag) == "math"]
    if not roots:
        return None, [], "", None, None
    math = roots[0]
    elems = [local(one.tag) for one in math.iter()]
    text = "".join("".join(one.itertext())
                   for one in math.iter() if local(one.tag) in MATH_TEXT)
    ann = [one for one in math.iter() if local(one.tag) == "annotation"]
    if ann:
        return (math.get("display"), elems, text, ann[0].get("encoding"),
                "".join(ann[0].itertext()))
    return math.get("display"), elems, text, None, None


# ── pptx：OMML 挂在文本体里，替身图挂在 Fallback 里 ──────────────────────────────
def pptx_prefixes(text: str) -> dict:
    """命名空间 URI → **文档里第一个**声明它的前缀

    Rust 那一侧 `xmlscan` 把元素名连前缀一起存，所以它交出的 `holder` 就是文件写下的那一串
    （`a14:m`）。ElementTree 只给 URI，于是这里按声明顺序还原本文件的叫法 —— 前缀是生产者
    自己的事，两家都只按写的交，不改成「规范推荐的前缀」。
    """
    out: dict = {}
    for pre, uri in re.findall(r'xmlns:([\w.\-]+)="([^"]*)"', text):
        out.setdefault(uri, pre)
    return out


def pptx_name(node, prefixes: dict) -> str:
    tag = node.tag
    if not tag.startswith("{"):
        return tag
    uri, _, tail = tag[1:].partition("}")
    pre = prefixes.get(uri)
    return "%s:%s" % (pre, tail) if pre else tail


def _pptx_resolve(base: str, target: str) -> str:
    seg = base.split("/")
    for one in target.split("/"):
        if one == "..":
            seg = seg[:-1]
        elif one != ".":
            seg.append(one)
    return "/".join(seg)


def omml_of(node):
    """一条 OMML：结构名按文档顺序（`r` / `t` / `oMath` 是壳与字，不算结构），
    字只拼 `m:t`，`m:nor` 与 `m:lit` 各数各的 —— 与 `equations.rs` 的 `add` 同一张排除表
    """
    structures = []
    runs = 0
    nor = 0
    lit = 0
    text = ""
    for one in node.iter():
        if one is node:
            continue
        name = local(one.tag)
        if name == "r":
            runs += 1
            continue
        if name == "t":
            text += "".join(one.itertext())
            continue
        if name == "nor":
            nor += 1
        if name == "lit":
            lit += 1
        if name == "oMath":
            continue
        structures.append(name)
    return structures, runs, nor, lit, text


def _pptx_page_rels(parts: dict, part: str) -> dict:
    """这一页自己的关系表：号只在这里查（`ppt/slides/_rels/slideN.xml.rels`）"""
    member = part.rsplit("/", 1)
    member = "%s/_rels/%s.rels" % (member[0], member[1])
    out: dict = {}
    if member not in parts:
        return out
    for one in ET.fromstring(parts[member]).iter():
        if local(one.tag) == "Relationship":
            out[one.get("Id")] = _pptx_resolve("ppt/slides", one.get("Target") or "")
    return out


def _pptx_walk(node, holder, ctx, prefixes, hits, found):
    """自顶往下带着上下文走：式子住在哪条链上、外面那层 `AlternateContent` 是哪一枚

    不能自底往上找祖先再比身份 —— ElementTree 每次迭代子节点都换一枚包装对象出来，
    `is` 永远不成立（这一条本机量过）。所以标志一路往下带，Rust 那一侧没有父指针，走的同一条。
    """
    for one in node:
        kind = local(one.tag)
        sub = dict(ctx)
        name = pptx_name(one, prefixes)
        if kind == "sp":
            found["shapes"] += 1
            sub["in_sp"] = True
            cn = [x for x in one.iter() if local(x.tag) == "cNvPr"]
            sub["shape"] = (cn[0].get("id"), cn[0].get("name")) if cn else (None, None)
        elif kind == "AlternateContent":
            found["alternates"] += 1
            sub["alternate"] = one
        elif kind == "Choice":
            sub["requires"] = one.get("Requires")
        elif kind == "p" and ctx["in_sp"]:
            sub["paragraph"] = found["paragraphs"]
            found["paragraphs"] += 1
        elif kind == "oMath":
            found["formulas"] += 1
            hits.append({
                "node": one,
                "holder": holder,
                "shape": ctx["shape"],
                "requires": ctx["requires"],
                "alternate": ctx["alternate"],
                "paragraph": ctx["paragraph"],
            })
        _pptx_walk(one, name, sub, prefixes, hits, found)


def pptx_equations(path: Path) -> dict:
    """pptx 那一份：一条式子是文本体里的 OMML，而同一个形状在 Fallback 里还画了一次

    与 `equations.rs::pptx` 同口径的四条判据：
    - **有没有式子看 `m:oMath` 在不在**：LibreOffice 把式子写成 `mc:AlternateContent` →
      `mc:Choice Requires="a14"` → `p:sp` → `p:txBody` → `a:p` → `a14:m` → `m:oMath`，
      并在同一个 `AlternateContent` 的 `mc:Fallback` 里**把那个形状又写一遍**（`cNvPr` 的
      id 与名字一字不差），那一遍里没有字、改挂一枚 `a:blipFill` 指向 `ppt/media/imageN.emf`；
      python-pptx 手挂的那一份则是 `a:p` 里直接一枚 `a14:m`，没有 `AlternateContent`、
      没有替身图 —— 所以「页上有几枚形状」与「有几条式子」在前者那里是两倍关系，两格各数各的；
    - **替身图按引用的那串地址去包里查**（`fallback_found`），查不到就交 false；
    - 段号与 `office-slide` 的 `paragraph_total` 同一 population：只给**在某个 `p:sp` 里**的
      `a:p` 编号（表格里那些段不在这一本走的顺序里）；
    - 前缀按写的交（`a14:m` 这一串是文件自己声明的），两家都不改成「规范推荐的前缀」。
    """
    parts = parts_of(path)
    if "ppt/presentation.xml" not in parts:
        return {"family": "pptx", "available": False}
    rels: dict = {}
    if "ppt/_rels/presentation.xml.rels" in parts:
        for one in ET.fromstring(parts["ppt/_rels/presentation.xml.rels"]).iter():
            if local(one.tag) == "Relationship":
                rels[one.get("Id")] = _pptx_resolve("ppt", one.get("Target") or "")
    order = []
    for one in ET.fromstring(parts["ppt/presentation.xml"]).iter():
        if local(one.tag) != "sldId":
            continue
        rid = None
        for key, value in one.attrib.items():
            if local(key) == "id" and key != "id":
                rid = value
        if rid in rels:
            order.append((one.get("id"), rels[rid]))
    items: list = []
    slides: list = []
    structures_seen: list = []
    stats = {"equations_total": 0, "slides_with": 0, "paragraphs_total": 0,
             "text_chars": 0, "math_runs": 0, "nor_runs": 0, "lit_runs": 0,
             "alternates_total": 0, "fallbacks_written": 0, "duplicated_shapes": 0,
             "rasters_written": 0, "rasters_found": 0, "rasters_missing": 0}
    empty_ctx = {"in_sp": False, "shape": None, "requires": None,
                 "alternate": None, "paragraph": None}
    for show_index, part in order:
        blob = parts.get(part)
        if blob is None:
            continue
        prefixes = pptx_prefixes(blob.decode("utf-8", "replace"))
        root = ET.fromstring(blob)
        page_rels = _pptx_page_rels(parts, part)
        found = {"paragraphs": 0, "shapes": 0, "formulas": 0, "alternates": 0}
        hits: list = []
        _pptx_walk(root, pptx_name(root, prefixes), empty_ctx, prefixes, hits, found)
        for hit in hits:
            structures, runs, nor, lit, words = omml_of(hit["node"])
            align = None
            for one in hit["node"].iter():
                if local(one.tag) == "jc":
                    align = attr_local(one, "val")
                    break
            alternate = hit["alternate"]
            fb_written = fb_blip = fb_target = fb_shape_id = None
            fb_found = None
            choice_sp_id = None
            if alternate is not None:
                kids = [one for one in alternate if local(one.tag) == "Fallback"]
                fb_written = bool(kids)
                if fb_written:
                    stats["fallbacks_written"] += 1
                for one in alternate:
                    if local(one.tag) == "Choice":
                        sps = [x for x in one.iter() if local(x.tag) == "sp"]
                        if sps:
                            cn = [x for x in sps[0].iter() if local(x.tag) == "cNvPr"]
                            choice_sp_id = cn[0].get("id") if cn else None
                        break
                if kids:
                    blip = [x for x in kids[0].iter() if local(x.tag) == "blip"]
                    if blip:
                        fb_blip = attr_local(blip[0], "embed")
                        stats["rasters_written"] += 1
                        fb_target = page_rels.get(fb_blip)
                        if fb_target in parts:
                            stats["rasters_found"] += 1
                            fb_found = True
                        else:
                            stats["rasters_missing"] += 1
                            fb_found = False
                    sps = [x for x in kids[0].iter() if local(x.tag) == "sp"]
                    if sps:
                        cn = [x for x in sps[0].iter() if local(x.tag) == "cNvPr"]
                        fb_shape_id = cn[0].get("id") if cn else None
                        if fb_shape_id is not None and fb_shape_id == choice_sp_id:
                            stats["duplicated_shapes"] += 1
            stats["equations_total"] += 1
            stats["text_chars"] += len(words)
            stats["math_runs"] += runs
            stats["nor_runs"] += nor
            stats["lit_runs"] += lit
            for name in structures:
                if name not in structures_seen:
                    structures_seen.append(name)
            shape = hit["shape"] or (None, None)
            items.append({
                "index": len(items),
                "part": part,
                "show_index": show_index,
                "paragraph": hit["paragraph"],
                "holder": hit["holder"],
                "shape_id": shape[0],
                "shape_name": shape[1],
                "structures": structures,
                "runs": runs,
                "nor_runs": nor,
                "lit_runs": lit,
                "align_written": align,
                "text": words,
                "choice_requires": hit["requires"],
                "in_alternate": alternate is not None,
                "fallback_written": fb_written,
                "fallback_blip": fb_blip,
                "fallback_target": fb_target,
                "fallback_found": fb_found,
                "fallback_shape_id": fb_shape_id,
            })
        stats["slides_with"] += 1 if found["formulas"] else 0
        slides.append({"part": part, "show_index": show_index,
                       "paragraphs_total": found["paragraphs"],
                       "shapes_total": found["shapes"],
                       "formulas": found["formulas"],
                       "alternates": found["alternates"]})
    stats["alternates_total"] = sum(one["alternates"] for one in slides)
    stats["paragraphs_total"] = sum(one["paragraphs_total"] for one in slides)
    return {"family": "pptx", "available": True, "items": items, "slides": slides,
            "slides_total": len(slides), "structures_seen": structures_seen, **stats}


# ── 遗留 .doc：式子是 OLE 对象，住在目录树的 ObjectPool 里 ─────────────────────────
CONTROLLED = ("\x01CompObj", "\x01Ole")


def compobj_labels(body: bytes):
    """`\\x01CompObj` 后半是三枚「u32 长度 + ANSI 串」：显示名、用户类型、ProgID

    头部实测 28 字节（u32 版本两枚 + u32 保留 + 16 字节 CLSID；`02 ce 00 …` 那 16 个字节
    整个就是 CLSID 本身，不是一个「类型字节 + 15 字节」—— 第一版按后者读，三枚串全读成 null，
    在这份件上才量出来）。
    两份实测凭据：公式对象写 `Microsoft Equation 3.0` / `DS Equation` / `Equation.3`，
    Word 自己的根写 `Microsoft Word-Dokument` / `MSWordDoc` / `Word.Document.8`
    —— 同一形状，所以这三格的含义不是猜的。**读不了就交 null，不猜。**
    """
    out: list = []
    at = 28
    while len(out) < 3 and at + 4 <= len(body):
        size = int.from_bytes(body[at:at + 4], "little")
        if size <= 0 or size > 256 or at + 4 + size > len(body):
            break
        raw = body[at + 4:at + 4 + size]
        out.append(raw.split(b"\x00", 1)[0].decode("latin-1"))
        at += 4 + size
    while len(out) < 3:
        out.append(None)
    return out


def _ole_children(entries: list, index: int) -> list:
    """一枚 storage 的孩子，按目录树的中序（left → 自己 → right）

    两族读者都只认这一条走法：CFB 的孩子挂成一棵红黑树，`left` / `right` 是同层兄弟，
    `child` 是自己的孩子；python 这一侧把「没有」读成 0，Rust 那一侧是 0xFFFFFFFF，
    都当结束。界与 Rust 的 `TREE_DEPTH` 同一条：畸形文件能把 `left` 指回自己。
    """
    out: list = []

    def go(where, depth):
        if not where or where >= len(entries) or depth >= 64:
            return
        one = entries[where]
        go(one["left"], depth + 1)
        out.append(where)
        go(one["right"], depth + 1)
    go(entries[index]["child"], 0)
    return out


def ole_equations(parsed: dict) -> dict:
    """遗留 .doc 那一份：式子是内嵌 OLE 对象，字在 MTEF 二进制里，**这一本不读字**

    与 `equations.rs::ole` 同口径的三条判据：
    - 对象在 `ObjectPool` 那枚 storage 的孩子里，一物一 storage（LibreOffice 写的名字是
      `_2147483647` 起递减）；对象名之外那两条控制流（`\\x01CompObj` / `\\x01Ole`）不算正文，
      **正文那条流的文件名按写的交**（`payload_stream`）—— 公式那份写的是 `Equation Native`，
      这一族既装公式也装别的，所以 `objects_total` 与 `equations_total` 两格各数各的；
    - `\\x01CompObj` 里那三枚串按上面那条布局读，读不出交 null；
    - MTEF 载荷**不解**：本机没有第二个读者认得它，流的大小与头几字节按原样交，
      式子里的字因此整个不在（缺键，不是空串）。
    """
    entries = parsed.get("entries") or []
    bytes_at = parsed.get("bytes_at") or {}
    pool = [i for i, one in enumerate(entries)
            if one["type"] == "storage" and one["name"] == "ObjectPool"]
    if not pool:
        return {"family": "ole", "available": True, "items": [], "objects_total": 0,
                "equations_total": 0, "payload_stream_seen": [], "native_bytes_total": 0,
                "pool_found": False}
    items = []
    seen = []
    equations_total = 0
    native_bytes = 0
    for index in _ole_children(entries, pool[0]):
        streams = [i for i in _ole_children(entries, index)
                   if entries[i]["type"] == "stream"]
        names = [entries[i]["name"] for i in streams]
        payload = next((one for one in names if one not in CONTROLLED), None)
        payload_at = next((i for i in streams if entries[i]["name"] == payload), None)
        body = bytes_at.get(payload_at, b"") if payload_at is not None else b""
        comp_at = next((i for i in streams if entries[i]["name"] == "\x01CompObj"), None)
        label, user_type, prog_id = compobj_labels(bytes_at.get(comp_at, b"")
                                                   if comp_at is not None else b"")
        if payload == "Equation Native":
            equations_total += 1
            native_bytes += len(body)
        if payload and payload not in seen:
            seen.append(payload)
        items.append({
            "index": len(items),
            "pool_name": entries[index]["name"],
            "streams": names,
            "compobj_written": "\x01CompObj" in names,
            "ole_written": "\x01Ole" in names,
            "payload_stream": payload,
            "payload_size": len(body) if payload else None,
            "declared_size": entries[payload_at]["size"] if payload_at is not None else None,
            "label": label,
            "user_type": user_type,
            "prog_id": prog_id,
        })
    return {"family": "ole", "available": True, "items": items,
            "objects_total": len(items), "equations_total": equations_total,
            "payload_stream_seen": seen, "native_bytes_total": native_bytes,
            "pool_found": True}
