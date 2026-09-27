r"""题注与交叉引用那一份账（三家）：一句「引用谁」写在三种地方，查的书也各有三本

与 `lyco_rtf` / `lyco_pdf` 同规格：这一份是 lbin `structure.cross_refs` 的**独立镜像**，
两边各自读同一批真件，数对不上才算哪一边读错。三条 lane 的规矩一样：
行的来源是**已经交过的那本域账**（`field_ledger`），这里只把「有目标的那几条」挑出来，
不重读一遍文件、不替文件补一条指令。

三本查的书（`book` 那一列说这一条查哪一本）：
* `bookmark` —— REF / PAGEREF / NOTEREF（ODF 是 `text:bookmark-ref` 一枚顶两家）查书签名
* `style` —— STYLEREF 查**样式名**（OOXML 有两个名字可查：`w:styleId` 与 `w:name` 的值）
* `sequence` —— SEQ 查序列声明（只有 ODF 有声明这一层；那边一条 `text:sequence` 用
  `text:sequence-name` 指回 `text:sequence-decl`）

「这一族有没有声明这一层」是一等公民，所以 `resolves` 有三态：true / false / **null**。
SEQ 在 OOXML 与 RTF 交 null —— 不是「查不到」，是**这一族压根没有可查的那本书**；
把它当成 false 就等于替文件编一条「引用坏了」。同理 NOTEREF 的缓存值在 docx 里
是域自己算的那一段，跟书签没有对应关系。
"""

import zipfile
import xml.etree.ElementTree as ET

# 种类词 → 查哪本书。名字按各家自己写的那个词/元素名收，不折成同一个词
BOOK_OF_INSTR = {"REF": "bookmark", "PAGEREF": "bookmark", "NOTEREF": "bookmark",
                 "STYLEREF": "style", "SEQ": "sequence"}
# `text:sequence-ref`（引用一个序列号）**不在这张表里**：`field_ledger` 的认字表没有它，
# 所以它一行也开不出来。这不是漏了 —— 41 份 odt 里它出现 0 次（`declarations.
# sequence_ref_elements` 就是那一本零计数），真件来了先在那格里露头，再进这张表。
BOOK_OF_ODF = {"sequence": "sequence", "bookmark-ref": "bookmark"}
# ODF 那一族的 targets 是属性，不是指令里的一个词。两只名字**不通用**（实测）：
# `text:sequence` 用自己的 `text:name` 说「我给哪一条序列编号」（表 / 图），
# 而 `text:bookmark-ref` 用 `text:ref-name` 说「我引用谁」——
# 被引用的那枚书签自己的名字却写在 `text:name` 上。同一个值住在两个属性名下。
ODF_TARGET_ATTR = {"sequence": "name", "bookmark-ref": "ref-name"}
# 题注样式的认法：名字**含**这一个词根（小写比），两家语言各一条
CAPTION_MARK = ("caption", "题注")


def xml_local(tag):
    return tag.rpartition("}")[2]


def local_attr(node, name):
    """按局部名取一枚属性（这一族的属性全带 namespace，名字却只有一份）"""
    for key, value in node.attrib.items():
        if key.rsplit("}", 1)[-1] == name:
            return value
    return None


def all_named(node, name):
    return [one for one in node.iter() if xml_local(one.tag) == name]


def attr_local(held, local):
    """从**带前缀**的属性表里按局部名取一枚：`text:sequence-name` 与 `style:sequence-name`
    是两个不同的名字，但这一族的 `field_ledger` 交的是文件写的那个带前缀的键，
    所以查的时候要按局部名查 —— 直接 `held.get("sequence-name")` 会一律拿到 None，
    而 None 在这本账里是「没写目标」，于是每条都变成「引用不成立」。这就是为什么
    这个助手单独有一只，而不是顺手 `.get` 一下。
    """
    for key, value in held.items():
        if key.rsplit(":", 1)[-1] == local:
            return value
    return None


def is_caption(name):
    if not name:
        return False
    low = name.lower()
    return any(one in low for one in CAPTION_MARK)


def ref_target(instr, kind):
    r"""种类词后面那一个「词」就是目标名 —— 它可以是带引号的一整串

    `STYLEREF "标题 1"` 与 `SEQ 图 \* ARABIC` 是两种形状：前者名字里带空格，只能靠引号
    切开；后者没名字，种类词后面直接就是一个开关。所以这里分三种答案：
    `None` 是没写目标（可能是「下一个 token 是开关」，也可能是整串到此为止），
    `""` 是**写了两个引号中间什么也没有** —— 这两件事在真件里都存在过，不能并成一个。
    返回 `(target, written_quoted, next_is_switch, unterminated)`。
    """
    text = instr or ""
    head = text.lstrip()
    if kind and head.upper().startswith(kind.upper()):
        head = head[len(kind):]
    head = head.lstrip()
    if not head:
        return None, False, False, False
    if head.startswith('"'):
        close = head.find('"', 1)
        if close < 0:
            return head[1:], True, False, True
        return head[1:close], True, False, False
    token = head.split(" ", 1)[0]
    if token.startswith("\\"):
        return None, False, True, False
    return token, False, False, False


def _rows_of(book):
    """域账里挑出「有目标这一问」的那几条：种类认得才算，认不得的一个也不丢进账"""
    out = []
    for one in book.get("rows", []):
        kind = one.get("kind")
        if kind is None:
            continue
        book_of = BOOK_OF_INSTR.get(kind.upper()) or BOOK_OF_ODF.get(kind)
        if book_of is None:
            continue
        out.append(one)
    return out


def _census(rows, key):
    out = {}
    for one in rows:
        had = one.get(key)
        out[str(had)] = out.get(str(had), 0) + 1
    return out


def _tri(rows, key):
    """三态那一列只数得清三个数：Rust 那边 JSON 的键不能是 null，所以摊成显式三格"""
    return {"true": sum(1 for one in rows if one.get(key) is True),
            "false": sum(1 for one in rows if one.get(key) is False),
            "null": sum(1 for one in rows if one.get(key) is None)}


def _ledger(family, rows, books, caption, limit, notes, declarations=None):
    """三家共用的收尾：三条对账计数 + 缓存值那一本原名 census

    `cache_values` 按**文件写的那串字**原样 census，不认它的语义 —— 实测里同一格里
    并存过 `''`（引用成立、缓存却是空，LibreOffice 重算前就是这样）、正常值、
    以及生产者留下的错误串 `错误: 引用源未找到`。判「这是不是一条错误」要给一个语言
    相关的表，而这里一个都不编：只把字交出来，让人自己看见。

    `books` 里三族的键一模一样（书签交名字，样式那两本**只交条数**）：一份带模板的
    docx 在 `word/styles.xml` 里写 164 条样式，整本名字摊进每条出口会把账本撑大两个
    数量级，而「这一条查得到吗」的答案在 `resolves` 那一列，不在名单里。
    """
    def count(pred):
        return sum(1 for one in rows if pred(one))

    shown = rows[:limit]
    return {
        "family": family,
        "available": True,
        "target_rows": len(rows),
        "listed": min(len(rows), limit),
        "cut": len(rows) > limit,
        "books": {one: (sorted(vals) if isinstance(vals, set) else vals)
                  for one, vals in books.items()},
        "kinds": _census(rows, "kind"),
        "target_books": _census(rows, "book"),
        "resolves": _tri(rows, "resolves"),
        "quoted": _tri(rows, "target_written_quoted"),
        "resolving_but_no_cache": count(
            lambda one: one["resolves"] is True and one["cached"] in (None, "")),
        "cached_but_unresolved": count(
            lambda one: one["resolves"] is False and one["cached"] not in (None, "")),
        "cache_missing": count(lambda one: one["cached"] is None),
        "cache_values": _census([one for one in rows if one["cached"] is not None],
                                "cached"),
        "caption_styles": caption,
        "declarations": declarations,
        "notes": notes,
        "rows": shown,
    }


# ---------------------------------------------------------------- OOXML


def _docx_text_parts(names):
    out = []
    for one in names:
        if not one.startswith("word/") or not one.endswith(".xml"):
            continue
        base = one.rsplit("/", 1)[-1]
        if (base == "document.xml" or base.startswith("header")
                or base.startswith("footer")
                or base in ("footnotes.xml", "endnotes.xml")):
            out.append(one)
    return out


def docx_cross_refs(path, limit=100):
    r"""三家里目标**只住在指令串里**的一家：SEQ 没有声明层，REF 查 `w:bookmarkStart` 的名字

    实测 155 份里的三条要紧处：
    * 全库没有任何元素的**局部名**里带 seq / caption —— 这一族的序列号是 `SEQ` 指令里的
      一个词，没有 `text:sequence-decl` 那样的声明本子，所以 SEQ 那四条 `resolves` 全交
      **null**（「这一族没有这本书」），而不是 false。
    * 题注**样式**却是有的：`word/styles.xml` 里 71 份 .docx 有 69 份声明一条
      （`w:styleId="Caption"`，它的 `w:name` 值是 `caption` —— 两个名字大小写不同，
      所以两个都查、各自报是在哪一本查到的，实测 `matched_on` 两本都中），
      而正文里 `w:pStyle` 指到它的是 **0 段**：生产者写了样式却一段没用过。
      剩下那 2 份（`pnum.docx` / `tbox.docx`）是**有样式表、只写 3 条 / 2 条样式、
      一条题注都不写** —— 「部件在但没声明」与「没有这个部件」两件事分开交。
    * STYLEREF 的目标查的是**声明名**那本书。`fields.docx` 写 `STYLEREF "标题 1"`，
      库里的声明名是英文的 `heading 1`，所以这一条按写的比就是 false —— 而它的缓存值
      里躺着 `标题一：给 STYLEREF 用`。这一对（引用不成立、缓存却在）就是要交的形状。
    """
    from office_reader import docx_field_ledger

    book = docx_field_ledger(path, limit=100000)
    if not book.get("available"):
        return {"family": "ooxml", "available": False}
    with zipfile.ZipFile(path) as box:
        names = sorted(one.filename for one in box.infolist())
        parts = _docx_text_parts(names)
        roots = {one: ET.fromstring(box.read(one)) for one in parts}
        # 样式表**不在**正文那一组部件里（`word/styles.xml` 的基名不是 document/header/...）：
        # 只扫上面那一组就拿不到任何一条样式声明 —— 题注普查会恒零、STYLEREF 查的书恒空，
        # 而账本会把这些读成「这份文档没写样式」。所以单独开一份，并交一个部件在场位。
        styles_root = (ET.fromstring(box.read("word/styles.xml"))
                       if "word/styles.xml" in names else None)
    marks = []
    for root in roots.values():
        # 书签那一本扫**所有文本部件**，与域那一本同一个范围：`REF` 指到页眉里的书签
        # 是完全写得出来的，只扫正文就会把一条成立的引用报成坏的
        marks.extend(all_named(root, "bookmarkStart"))
    bookmarks = [local_attr(one, "name") for one in marks
                 if local_attr(one, "name") is not None]
    declared_ids, declared_names = [], []
    style_defs = []
    if styles_root is not None:
        for one in all_named(styles_root, "style"):
            style_id = local_attr(one, "styleId")
            named = [kid for kid in one if xml_local(kid.tag) == "name"]
            value = local_attr(named[0], "val") if named else None
            # `w:type` 是 `w:style` **自己的一枚属性**，不是孩子：拿孩子的局部名去找
            # 会一律拿到 None，于是这一列整本都是「没写类型」，看着像文件偷懒
            style_defs.append({
                "style_id": style_id,
                "declared_name": value,
                "type": local_attr(one, "type"),
            })
    for one in style_defs:
        if one["style_id"] is not None:
            declared_ids.append(one["style_id"])
        if one["declared_name"] is not None:
            declared_names.append(one["declared_name"])
    uses = {}
    for part, root in roots.items():
        for one in all_named(root, "pStyle"):
            had = local_attr(one, "val")
            if had is not None:
                uses[had] = uses.get(had, 0) + 1

    caption_rows = []
    for one in style_defs:
        on = [nm for nm, txt in (("styleId", one["style_id"]), ("name", one["declared_name"]))
              if is_caption(txt)]
        if not on:
            continue
        caption_rows.append({
            "part": "word/styles.xml",
            "style_id": one["style_id"],
            "declared_name": one["declared_name"],
            "matched_on": on,
            "type": one["type"],
            "paragraphs_using_it": uses.get(one["style_id"], 0),
        })

    rows = []
    for one in _rows_of(book):
        kind = one["kind"].upper()
        which = BOOK_OF_INSTR[kind]
        target, quoted, after, unterminated = ref_target(one["instruction"], one["kind"])
        if which == "bookmark":
            hit = target in bookmarks
        elif which == "style":
            hit = target in declared_names or target in declared_ids
        else:
            hit = None
        rows.append({
            "part": one["part"], "index": one["index"], "kind": one["kind"],
            "instruction": one["instruction"], "target": target,
            "target_written_quoted": quoted, "next_token_is_switch": after,
            "target_unterminated": unterminated,
            "book": which, "resolves": hit,
            "cached": one["cached"], "cache_written": one["cached"] is not None,
        })
    return _ledger("ooxml", rows, {
        "bookmarks": sorted(set(bookmarks)),
        "bookmark_marks": len(marks),
        "style_defs": len(style_defs),
        "style_ids": len(set(declared_ids)),
        "style_names": len(set(declared_names)),
        "sequences": None,
    }, {
        "declared": len(caption_rows),
        "paragraphs_using_them": sum(one["paragraphs_using_it"] for one in caption_rows),
        # 「没有样式表这一份部件」与「有部件但一条题注样式都不写」是两件事，分开交
        "styles_part": styles_root is not None,
        "rows": caption_rows,
    }, limit, ["这一族没有序列声明这一层：SEQ 的 resolves 一律 null"])


# ---------------------------------------------------------------- ODF


def odf_cross_refs(path, limit=100):
    r"""目标写在**属性**上的一家：序列声明这一层只有这里有，题注样式也一样「声明了没用」

    与 OOXML 反过来：`text:sequence` 的号是属性 `text:name` 指的（说「我给哪一条序列编号」，
    而**引用**别人的那一枚才用 `text:ref-name`），指向 `text:sequence-decl` 那一份声明；
    所以这一族 SEQ 的 `resolves` 是**真能判**的 true/false。
    实测（41 份 .odt）：会编号的那 2 份（`fields.odt` / `fields-mix.odt`）六条声明全写，
    名字是 `Drawing / Figure / Illustration / Table / Text` 再加一条中文的（「表」「图」），
    而**用到的名字**只有那一条中文的；另 37 份只写五条英文的、一条都没用到，
    还有 2 份（`pnum.odt` / `tbox.odt`）一份声明都不写 ——
    「声明了没人用」是本族的常态（`declared_unused` 在 39 份里恒 5 条），
    所以 `declared_unused` 与 `used_without_decl`（本库恒 0）两本分交。
    同一份件里 `text:sequence-ref`（引用一个序列号）出现 **0 次**：这一族会编号、会写题注，
    却从不引用那个号 —— 所以那一条只交一个零计数，等真件来推翻。
    REF 与 PAGEREF 在这里塌成同一枚 `text:bookmark-ref`，只有 `text:reference-format`
    分得开（`number` / `page`，实测 `fields-mix.odt` 两枚同名的书签引用一条写 number
    一条写 page，而 docx 那边是两种指令），目标名在 `text:ref-name` 上；`_RefMix1` 那一条
    在 content.xml 里有同名 `text:bookmark`，所以 resolves 是 true。
    书签那一本数的是**元素枚数**：一对 `bookmark-start` + `bookmark-end` 就是两枚
    （`fields.odt` 一枚书签 → `bookmark_marks` 是 2），名字那一本却按去重交 ——
    两个数本来就不该相等，别拿一个顶另一个。
    题注样式本族也是「声明多、使用零」：`style:name="Caption"` 41 份里 38 份有，
    段落自己写的 `style:name` 却是 `Standard`（题注那一段没用自己那一族）。
    """
    from office_reader import odf_field_ledger

    book = odf_field_ledger(path, limit=100000)
    if not book.get("available"):
        return {"family": "odf", "available": False}
    with zipfile.ZipFile(path) as box:
        have = set(one.filename for one in box.infolist())
        roots = {}
        for part in ("content.xml", "styles.xml"):
            if part in have:
                roots[part] = ET.fromstring(box.read(part))
    declared, decl_wrappers = [], []
    bookmarks, seq_ref_uses, style_nodes = [], [], []
    for part, root in roots.items():
        for one in root.iter():
            local = xml_local(one.tag)
            if local == "sequence-decl":
                declared.append(local_attr(one, "name") or "")
            elif local == "sequence-decls":
                decl_wrappers.append({"part": part,
                                      "count": len([kid for kid in one
                                                    if xml_local(kid.tag) == "sequence-decl"])})
            elif local in ("bookmark", "bookmark-start", "bookmark-end"):
                bookmarks.append({"part": part, "name": local_attr(one, "name"),
                                  "element": local})
            elif local == "sequence-ref":
                seq_ref_uses.append({"part": part,
                                     "name": local_attr(one, "sequence-name")})
            elif local == "style":
                style_nodes.append((part, one))
    # 一枚 `text:bookmark` 可以把名字省了（这一族的 start/end 那一对容许没名字），
    # 那是「写了一枚没名字的书签」，不是「没写书签」，所以两本分交
    marks_written = len(bookmarks)
    names = sorted(set(one["name"] for one in bookmarks if one["name"] is not None))
    declared_sorted = sorted(set(declared))

    caption_rows = []
    style_uses = {}
    for part, root in roots.items():
        for one in all_named(root, "p"):
            had = local_attr(one, "style-name")
            if had is not None:
                style_uses[had] = style_uses.get(had, 0) + 1
    for part, one in style_nodes:
        named = local_attr(one, "name")
        if not is_caption(named):
            continue
        caption_rows.append({
            "part": part, "style_id": None, "declared_name": named,
            "matched_on": ["name"], "type": local_attr(one, "family"),
            "paragraphs_using_it": style_uses.get(named, 0),
        })

    rows = []
    for one in _rows_of(book):
        kind = one["kind"]
        which = BOOK_OF_ODF[kind]
        attr = ODF_TARGET_ATTR[kind]
        held = one["attrs"]
        target = attr_local(held, attr)
        written = target is not None
        if which == "bookmark":
            hit = target in names
        else:
            hit = target in declared_sorted
        rows.append({
            "part": one["part"], "index": one["index"], "kind": one["kind"],
            "element": one["element"], "target": target, "target_written": written,
            "target_written_quoted": None,
            "reference_format": attr_local(held, "reference-format"),
            "formula": attr_local(held, "formula"),
            "ref_name": attr_local(held, "ref-name"),
            "own_name": attr_local(held, "name"),
            "num_format": attr_local(held, "num-format"),
            "seq_sub_formula": attr_local(held, "seq-sub-formula"),
            "book": which, "resolves": hit,
            "cached": one["cached"], "cache_written": True,
        })
    used = sorted(set(one["target"] for one in rows
                      if one["book"] == "sequence" and one["target"] is not None))
    return _ledger("odf", rows, {
        "bookmarks": names,
        "bookmark_marks": marks_written,
        "style_defs": len(style_nodes),
        # 这一族一枚样式只有一个名字：`style:name` 既是声明名也是 id，没有两本可分
        "style_ids": None,
        "style_names": len(set(local_attr(one, "name")
                               for _part, one in style_nodes
                               if local_attr(one, "name") is not None)),
        "sequences": declared_sorted,
    }, {
        "declared": len(caption_rows),
        "paragraphs_using_them": sum(one["paragraphs_using_it"] for one in caption_rows),
        "rows": caption_rows,
    }, limit, [], {
        "elements": len(declared),
        "wrappers": decl_wrappers,
        "declared_unused": [one for one in declared_sorted if one not in used],
        "used_without_decl": [one for one in used if one not in declared_sorted],
        "sequence_ref_elements": len(seq_ref_uses),
        "sequence_ref_uses": seq_ref_uses,
    })


# ---------------------------------------------------------------- RTF


def rtf_cross_refs(page, limit=100):
    r"""第三家：目标与 docx 同一个形状（都在指令串里），书签名却已经解过 `\u`

    这一族的指令是从 `{\*\fldinst ...}` 里解掉反斜杠转义交出来的，所以
    `REF 表锚点 \h` 与 docx 的 `w:instrText` 逐字同一个形状 —— 目标这把尺子两家共用。
    书签这一边 `bookmarks` 已经把 `\uN` 解好了，于是中文书签名**配得上**：`fields.rtf`
    的 `\bkmkstart 表锚点` 与指令里那个名字是同一串字。样式那本书本族有名字
    （`\s55` 那一条叫 `caption`），但 STYLEREF 在真件里一个都没写。
    题注样式本族声明了、`style_uses` 里没有它那一条 —— 也就是 0 段在用，
    与前两家同一个形状；不一样的是**样式号**：`fields*.rtf` 是 55，`toc.rtf` 是 109，
    所以号只按文件自己写的交，别家生产者写的号不算。
    """
    from office_reader import _field_kind

    rows = []
    bookmarks = list(page.get("bookmarks", []))
    styles = [one for one in page.get("styles", []) if one.get("kind") == "paragraph"]
    style_names = sorted(set(one["name"] for one in styles if one.get("name")))
    uses = {one["index"]: one["count"] for one in page.get("style_uses", [])}
    caption_rows = [{
        "part": "stylesheet",
        "style_id": one["index"],
        "declared_name": one["name"],
        "matched_on": ["name"],
        "type": "paragraph",
        "paragraphs_using_it": uses.get(one["index"], 0),
    } for one in styles if is_caption(one.get("name"))]
    held_rows = [{"index": i, "kind": _field_kind(one.get("instruction")),
                  "instruction": one.get("instruction"),
                  "cached": one.get("cached"), "has_result": one.get("has_result")}
                 for i, one in enumerate(page.get("field_rows", []))]
    for one in _rows_of({"rows": held_rows}):
        kind = one["kind"].upper()
        which = BOOK_OF_INSTR[kind]
        target, quoted, after, unterminated = ref_target(one["instruction"], one["kind"])
        hit = (target in bookmarks) if which == "bookmark" else \
            (target in style_names) if which == "style" else None
        rows.append({
            "part": "stream", "index": one["index"], "kind": one["kind"],
            "instruction": one["instruction"], "target": target,
            "target_written_quoted": quoted, "next_token_is_switch": after,
            "target_unterminated": unterminated,
            "book": which, "resolves": hit,
            "cached": one["cached"], "cache_written": bool(one["has_result"]),
        })
    return _ledger("rtf", rows, {
        "bookmarks": sorted(set(bookmarks)),
        "bookmark_marks": page["bookmark_starts"],
        "style_defs": len(styles),
        # `\sN` 那个号不是声明名：STYLEREF 指令里写的从来是名字
        "style_ids": None,
        "style_names": len(style_names),
        "sequences": None,
    }, {
        "declared": len(caption_rows),
        "paragraphs_using_them": sum(one["paragraphs_using_it"] for one in caption_rows),
        "rows": caption_rows,
    }, limit, ["这一族也没有序列声明这一层：SEQ 的 resolves 一律 null"])
