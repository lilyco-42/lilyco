r"""RTF 正文提取：目标群感知、字符集感知的最小解释器。

`office_reader.py` 用它算出「这份 RTF 的正文应该是什么」，`lilyco-binfmt/src/rtf.rs` 是同一份
规范的另一套实现 —— 两边必须一致，CI 上的 `office_probe.py` 就是来对这一件事的。

五件真正决定成败的事：

1. 控制字按**词边界**结束：`\pard` 不许当成 `\par` 加一个字面 `d` —— 那样样式名与字体名
   会整段漏进正文（这份实现的旧版本就是这么错的）；
2. 目标群整群跳过：`fonttbl` / `colortbl` / `stylesheet` / `info` / `generator` / `\*\...`
   里的内容不是正文；子群继承父群的跳过状态；
3. `\uN` 之后要按 `\ucN` 声明的个数丢掉**等价回退字符**，而且 `\'hh` 算一个字符 ——
   少这一步，中文段落会剩下一串 `'3f`；
4. `\'hh` 是**当前字符集**的一个字节，字符集由文件自己的 `\ansicpgNNNN` 说：
   中文 Word 写 936（GBK），英文写 1252。按 cp1252 硬解一份 cp936 的 RTF 会得到碎字，
   所以连续的 `\'hh` 先攒成字节串，再整段按声明的字符集解；
5. 解不出来的字节计数并说出来，不悄悄丢掉。
"""

from __future__ import annotations

import re

from lyco_pages import convert

BS = "\\"  # 一个反斜杠

# 见到这些控制字就把所在群整群跳过：它们是表 / 元数据，不是正文
SKIP_DESTINATIONS = {
    "fonttbl",
    "colortbl",
    "stylesheet",
    "info",
    "generator",
    "listtable",
    "listoverridetable",
    "themedata",
    "colorschememapping",
    "datastore",
    "panorama",
    "latentstyles",
    "rsidtbl",
    "xmlnstbl",
    "filetbl",
    "upr",
    "bkmkstart",
    "bkmkend",
    "nonshppict",
    "pntxtb",
    "pntext",
    "mmathPr",
    "docPr",
    "atrsnc",
    "atrspr",
    "operator",
}

# 断点类控制字 → 输出一个换行
BREAK_WORDS = {"par", "line", "sect", "page", "pbb"}
# 这一族按**词边界**数这几个断点词（子串数会骗人：`\pard` 里有 `\par`、
# `\sectd` / `\sectx` 里有 `\sect`）。固定这六个键：数过了没有就交 0，
# 「没有」与「没数」不是一件事
BREAK_COUNT = ("par", "line", "page", "pagebb", "pbb", "sect")
# 制表类（表格单元格与行分隔）→ 输出一个制表符
TAB_WORDS = {"tab", "cell", "nestcell"}
# 行结束：一行表格就是一行文本（把 \row 当制表符会把整张表挤成一行）
ROW_WORDS = {"row", "nestrow"}
# 嵌套对象类：整个跳过并计数（办公文件里常见的是 OLE 对象与图片）
OBJECT_WORDS = {"object", "objattph", "objdata", "objclass", "objname", "objemb", "objhide"}
# 文档级「那张纸」写在哪几个控制字上（单位是 twips，1/1440 英寸）。
# `landscape` 是个旗标（写了就是横的），其余都带一个数字参数
PAPER_WORDS = {"paperw", "paperh", "margl", "margr", "margt", "margb", "landscape"}


def peek_word(text: str, at: int) -> str:
    """`at` 之后第一个控制字的名字（可能隔着一个反斜杠与空白），没有就交回空串

    `\*` 修饰的是紧跟它的那个目标群，所以要跳之前得先看清那是个什么群。
    """
    k = at
    while k < len(text) and (text[k] in " \r\n" or text[k] == "\\"):
        k += 1
    word = ""
    while k < len(text) and text[k].isalpha():
        word += text[k]
        k += 1
    return word


def skip_rtf_chars(text: str, at: int, how_many: int) -> int:
    """从 `at` 起丢掉 `how_many` 个 **RTF 字符**

    `\'hh` 是一个字符（不是一个反斜杠加三个字面量），控制字连同它的数字参数也是一个字符。
    """
    j = at
    while j < len(text) and text[j] in "\r\n":
        j += 1
    left = how_many
    while left > 0 and j < len(text):
        if text.startswith(BS + "'", j):
            j += 4
        elif text.startswith(BS, j) and j + 1 < len(text) and text[j + 1].isalpha():
            j += 1
            while j < len(text) and text[j].isalpha():
                j += 1
            while j < len(text) and (text[j].isdigit() or text[j] == "-"):
                j += 1
            if j < len(text) and text[j] == " ":
                j += 1
        else:
            j += 1
        left -= 1
    return j


def decode_pending(raw: bytes, codepage: int) -> tuple[str, int]:
    """按文件声明的字符集解一串 `\'hh` 字节；解不动的按 latin-1 兜底并数出替换了几个"""
    if not raw:
        return "", 0
    try:
        return raw.decode(f"cp{codepage}"), 0
    except (LookupError, UnicodeDecodeError):
        pass
    try:
        return raw.decode("cp1252"), 0
    except UnicodeDecodeError:
        pass
    text = raw.decode("latin-1", "replace")
    return text, text.count(chr(0xFFFD))


def group_end(text: str, at: int) -> tuple[int, str]:
    """从 `at` 起找到关掉**当前这一群**的 `}`：交回它的位置与群内文本。

    `\\{` 与 `\\}` 是字面花括号，不参与配对；找不到就走到尾。
    """
    depth = 0
    i = at
    while i < len(text):
        ch = text[i]
        if ch == "{":
            depth += 1
        elif ch == "}":
            if depth == 0:
                return i, text[at:i]
            depth -= 1
        elif ch == "\\" and i + 1 < len(text) and text[i + 1] in "{}":
            i += 1
        i += 1
    return len(text), text[at:]


PAGE_DESTINATIONS = {
    "header", "headerl", "headerr", "headert", "headerf",
    "footer", "footerl", "footerr", "footert", "footerf",
}

# 字体表与样式表也住在群里面，本域认得它们 —— 但认得不等于要把那一群收下：
# 那两群里坐着几十个「不认识就跳」的子群（`{\*\falt …}`、`{\*\csN …}`），
# 整群吃掉会让 skipped_destinations 从 65 掉到 23，那个诊断数就不可比了。
# 所以走 `field` 那一条路：照旧跳过（一个字不进正文），只**前瞻**读一遍里面的定义
# `listtable` 与 `listoverridetable` 也走这一条：列表定义不是页面上的字，但段上那个
# `\ls` 点的就是这里的某一份，不读等于把号的来源丢掉（整群照样跳，destinations 不变）
DEF_DESTINATIONS = {"fonttbl", "stylesheet", "listtable", "listoverridetable"}
# 一群里的第一条控制字决定它是什么定义：`\fN` 字体、`\sN` 段落样式、`\csN` 字符样式
FIRST_DEF = re.compile(r"^\\(?:\*\\)?(cs|s|f)(\d+)")
KIND_OF_PREFIX = {"f": "font", "s": "paragraph", "cs": "character"}
# 标题的层级就写在样式名里：`heading 1` … `heading 9`（大小写与空格各家不同）
HEADING_NAME = re.compile(r"^\s*heading\s+(\d+)\s*$", re.IGNORECASE)
# 一个字体条目自己声明的字符集：`\fcharset0` 是 ANSI，非 0 的那一串（128 是 Shift-JIS、
# 134 是 GB2312 …）意味着名字里的字节不是 cp1252 —— 按 cp1252 解出来就是乱码，
# 所以那种条目只交字符集号，名字交 null
FCHARSET = re.compile(r"\\fcharset(\d+)")


def child_groups(text: str) -> list:
    """一群之内的**顶层**子群：交回每个子群的内容（不含外面那对花括号）"""
    out: list[str] = []
    i = 0
    while i < len(text):
        if text[i] != "{":
            i += 1
            continue
        stop, inner = group_end(text, i + 1)
        out.append(inner)
        i = stop + 1
    return out


def definition_of(child: str) -> dict:
    """一条 `{\f0 … Times New Roman;}` / `{\s1 … heading 1;}` 读出种类、编号与名字。

    名字用 `rtf_text` 自己解：控制字不产字，嵌套的 `{\\*\\falt …}` 那一群按「不认识就跳」
    跳掉，所以剩下的那段文字就是这一个定义的名字（末尾跟着一个分号）。
    """
    hit = FIRST_DEF.match(child)
    if not hit:
        return {}
    kind = KIND_OF_PREFIX[hit.group(1)]
    charset = 0
    mark = FCHARSET.search(child)
    if mark:
        charset = int(mark.group(1))
    text = rtf_text(child.encode("latin-1", "replace"))["text"].strip()
    if text.endswith(";"):
        text = text[:-1].rstrip()
    # 全 ASCII 的名字与字符集无关（RTF 用的每一种字符集都含 ASCII）；
    # 否则只有 ANSI(0) 那份能按 cp1252 读。非 ANSI 又含非 ASCII 的：交 null 加字符集号，
    # 不交那串按 cp1252 解出来的乱码字（"‚l‚r ƒSƒVƒbƒN" 就是它 —— 文件写的是 Shift-JIS）
    ascii_name = all(ord(one) < 0x80 for one in text)
    one = {
        "kind": kind,
        "index": int(hit.group(2)),
        "charset": charset if kind == "font" else None,
        "name": (text or None) if (kind != "font" or charset == 0 or ascii_name) else None,
    }
    return one

# 注的两个口袋。LibreOffice 的 RTF 导出**只用 `footnote` 这一个口袋**，
# 尾注靠群里的 `\ftnalt` 反标志区分（Word 那族还会另写 `endnote` 口袋），
# 所以两个词都认、再看 `\ftnalt`
NOTE_DESTINATIONS = {"footnote", "endnote"}

# 这些是注的**排版定义**（分隔符、续分符、编号占位），不是一条注：
# 这份件里就写着 `{\*\ftnsep\chftnsep}` —— 当成注就会凭空多出几条空注
NOTE_DEFINITION_DESTINATIONS = {
    "ftnsep", "ftnsepc", "ftncn", "aftnsep", "aftnsepc", "aftncn",
}

# 链接在 RTF 里是一个域：`{\field{\*\fldinst HYPERLINK "地址" }{\fldrslt {显示文字}}}`。
# 指令原文那一群照旧是「不认识就跳」的对象（它不是页面上的字），这一族本域认识的是
# 里面的 `HYPERLINK "…"`，所以只**前瞻**读它，不推进游标 —— `\fldrslt` 的显示文字
# 仍然要按正文留下来，那是 Word 与 LibreOffice 都遵守的一条口径
FLDINST_HEAD = "{" + BS + "*" + BS + "fldinst"
FLDRSLT_HEAD = "{" + BS + "fldrslt"
LINK_INSTRUCTION = re.compile('HYPERLINK\\s+"([^"]*)"', re.IGNORECASE)
# 目录收几级写在域指令的开关上：`TOC \o "1-2" \h`。文件里那两个反斜杠是成对写的
# （单个反斜杠会开出一个控制字），解掉之后这一串与 docx 的 w:instrText 逐字相同 ——
# 所以 `switch_value` 这一家与 `office_doc.rs` 里 OOXML 用的那一把规则一样
LEVEL_SWITCH = BS + "o "


def starts_word(group: str, want: str) -> bool:
    """一群的开头是不是控制字 `want`（`\list{` 与 `\listlevel{` 不是一回事）"""
    if not group.startswith(BS):
        return False
    rest = group[1:]
    k = 0
    while k < len(rest) and rest[k].isalpha():
        k += 1
    if rest[:k] != want:
        return False
    return k >= len(rest) or not rest[k].isalnum()


def word_len(group: str) -> int:
    """开头那个控制字占了几个字：名字、紧跟的数字（`-` 也算），以及后面那**一个**空格。
    `\'hh` 不是控制字，只占四个字节"""
    if group.startswith(BS + "'"):
        return min(4, len(group))
    if not group.startswith(BS):
        return 0
    k = 1
    while k < len(group) and group[k].isalpha():
        k += 1
    while k < len(group) and (group[k].isdigit() or group[k] == "-"):
        k += 1
    if k < len(group) and group[k] == " ":
        k += 1
    return k


def payload_of(group: str) -> str:
    """去掉开头控制字之后的部分：`{\\leveltext \\'02\\'01.;}` 要说的是 `\\'02\\'01.;`"""
    return group[min(word_len(group), len(group)):]


def written_of(group: str) -> str:
    """一群去掉开头控制字之后的原样串（`\'hh` 与 `\\uN` 按字节交，不猜字面）"""
    return payload_of(group)


def word_in_group(group: str, want: str):
    """一群里第一个叫 `want` 的控制字后面那串数字（词在而没数字时交空串），没有交 None"""
    i = 0
    while i < len(group):
        if group[i] != BS:
            i += 1
            continue
        k = i + 1
        name = ""
        while k < len(group) and group[k].isalpha():
            name += group[k]
            k += 1
        from_ = k
        while k < len(group) and (group[k].isdigit() or group[k] == "-"):
            k += 1
        if name == want:
            return group[from_:k]
        i = k if k > i + 1 else i + 1
    return None

PIC_CAP = 16 * 1024
PIC_ROW_CAP = 512
PIC_MAGIC = [
    (b"\x89PNG", "png"),
    (b"\xff\xd8", "jpeg"),
    (b"BM", "bmp"),
    (b"GIF8", "gif"),
    (b"\xd7\xcd\xc6\x9a", "emf"),
    (b"II*\x00", "tiff"),
    (b"MM\x00*", "tiff"),
]


# 字符格式那一族：与 Rust 的 RUN_WORDS / RUN_DIRECT / RUN_UNDERLINE 同三张表
RUN_WORDS = (
    "b", "i", "ul", "uld", "aul", "iul", "outl", "strike", "sub", "super", "cf", "cb",
    "highlight", "fs", "afs", "f", "af", "kerning", "expnd", "caps", "scaps", "ulc",
)
RUN_UNDERLINE = ("ul", "uld", "aul", "iul")
RUN_DIRECT = ("loch", "hich", "dbch", "rtlch", "ltrch")
RUN_ROW_CAP = 512


def color_table(inner: str) -> list:
    """`{\\colortbl;\\red0\\green0\\blue0;…}`：一格一个号，开头那个空位就是 0 号

    三条齐了才算一个颜色，缺一条的那一格交 None 而不是补 0。
    """
    out: list = []
    parts: list = [None, None, None]
    at = 0
    while at < len(inner):
        ch = inner[at]
        if ch == ";":
            out.append(
                "%02X%02X%02X" % (parts[0], parts[1], parts[2])
                if all(one is not None for one in parts)
                else None
            )
            parts = [None, None, None]
            at += 1
            continue
        if ch != BS:
            at += 1
            continue
        word = peek_word(inner, at + 1)
        which = {"red": 0, "green": 1, "blue": 2}.get(word)
        if which is None:
            at += 1
            continue
        stop = at + 1 + len(word)
        hit = re.match(r"\d+", inner[stop:])
        parts[which] = int(hit.group(0)) if hit else None
        at = stop
    return out


def word_switch(words: list, name: str):
    """这一族说「开 / 关」只看那个数字参数：`\\b` 与 `\\b1` 都是开，`\\b0` 是明确写着不"""
    for one, two in words:
        if one == name:
            return two == "" or two != "0"
    return None


def underline_switch(words: list):
    """下划线那四个口袋：文件点哪个算哪个，一个都没点才是「没说」"""
    hits = [one for one in words if one[0] in RUN_UNDERLINE]
    if not hits:
        return None, None
    return any(two == "" or two != "0" for _, two in hits), hits[0][0]


def color_hop(words: list, name: str, colors: list):
    for one, two in words:
        if one != name:
            continue
        try:
            at = int(two)
        except ValueError:
            return {"index": two, "resolved": False, "rgb": None}
        got = colors[at] if 0 <= at < len(colors) else None
        return {"index": two, "resolved": got is not None, "rgb": got}
    return None


def font_hop(words: list, name: str, fonts: list):
    for one, two in words:
        if one != name:
            continue
        try:
            want = int(two)
        except ValueError:
            return {"index": two, "resolved": False, "name": None}
        got = [had for had in fonts if had.get("index") == want]
        return {
            "index": two,
            "resolved": bool(got),
            "name": got[0].get("name") if got else None,
        }
    return None


def resolve_run_rows(rows: list, colors: list, fonts: list) -> None:
    """群头上那一串控制字之后解出来的两本账（整条流读完才跳，表的先后不参与判断）"""
    for row in rows:
        words = [(one, two) for one, two in row["words"]]
        underline, which_word = underline_switch(words)
        position = next((one for one in ("super", "sub") if any(a == one for a, _ in words)), None)
        found = dict(words)
        row["switches"] = {
            "bold": word_switch(words, "b"),
            "italic": word_switch(words, "i"),
            "strike": word_switch(words, "strike"),
            "underline": underline,
        }
        row["values"] = {
            "color": color_hop(words, "cf", colors),
            "fill": color_hop(words, "cb", colors),
            "highlight": color_hop(words, "highlight", colors),
            "underline_word": which_word,
            "size": found.get("fs"),
            "asian_size": found.get("afs"),
            "position": position,
            "font": font_hop(words, "f", fonts),
            "asian_font": font_hop(words, "af", fonts),
        }
        row["format"] = [
            {"element": one, "digits": two if two != "" else None} for one, two in words
        ]


def unstar(one: str) -> str:
    """跳过 `\*` 那个「不认识就整群跳过」的标记：`{\*\picprop …}` 里面开头是 `\*\picprop`"""
    return one[2:] if one.startswith(BS + "*") else one


def group_stop(text: str, at: int) -> int:
    """从 `at` 起走到「关掉当前这一群」的那个 `}`，只交那个下标（不抄整群）"""
    depth = 0
    i = at
    while i < len(text):
        ch = text[i]
        if ch == "{":
            depth += 1
        elif ch == "}":
            if depth == 0:
                return i
            depth -= 1
        elif ch == BS and text[i + 1:i + 2] in ("{", "}"):
            i += 1
        i += 1
    return len(text)


def blip_word(group: str):
    """第一个「说这是哪种图」的控制字：交回名字与它写完之后的位置"""
    i = 0
    while i < len(group):
        if group[i] != BS:
            i += 1
            continue
        k = i + 1
        name = ""
        while k < len(group) and group[k].isalpha():
            name += group[k]
            k += 1
        while k < len(group) and (group[k].isdigit() or group[k] == "-"):
            k += 1
        if name.endswith("blip") or name in ("dibitmap", "pictbitmap", "macpict"):
            return name, k
        i = k if k > i + 1 else i + 1
    return None, None


def hex_head(group: str, frm: int) -> bytes:
    """那串十六进制的前 8 个字节：数据行里可以夹着换行与空格（LibreOffice 就是折行写的），
    碰上既不是十六进制也不是空白的字节才停"""
    got: list[int] = []
    pending = None
    i = frm
    while i < len(group) and len(got) < 8:
        ch = group[i]
        if ch in " \t\r\n":
            i += 1
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
        i += 1
    return bytes(got)


def picture_kind(head: bytes):
    """那几个字节是什么图。认不出名字交 "unknown"，一个字节都没读到才交 None"""
    for want, name in PIC_MAGIC:
        if head.startswith(want):
            return name
    return "unknown" if head else None


def blip_agrees(kind, sig):
    """「文件说这是什么格式」与「字节自己说这是什么格式」对不对得上。
    只在两边都认得时才比（`\dibitmap` 那种没有词干可读 → null，不猜一个「不一致」）"""
    if not kind or not kind.endswith("blip"):
        return None
    said = kind[: -len("blip")]
    if not said or sig == "unknown" or sig is None:
        return None
    return said == sig


def shape_props(group: str):
    """`{\*\picprop …}` 那一格里的 `{\sp{\sn 名字}{\sv 值}}`：一对一条按文件顺序交。
    值可以是空串（`notes.rtf` 两条都是），那与整对没写 `{\sv}` 是两件事。
    第一个布尔说有没有见过 picprop 那一格"""
    seen = False
    out: list[dict] = []
    for holder in child_groups(group):
        if not starts_word(unstar(holder), "picprop"):
            continue
        seen = True
        for one in child_groups(holder):
            pair: dict = {}
            for had in child_groups(one):
                body = unstar(had)
                if starts_word(body, "sn"):
                    pair["name"] = rtf_text(payload_of(body).encode(
                        "latin-1", "replace"))["text"]
                elif starts_word(body, "sv"):
                    pair["value"] = rtf_text(payload_of(body).encode(
                        "latin-1", "replace"))["text"]
            out.append({
                "name": pair.get("name"),
                "value": pair.get("value"),
                "value_written": "value" in pair,
            })
    return seen, out


def picture_ledger(text: str, at: int) -> dict:
    """一张 `{\pict …}` 群的账（与 Rust 的 `picture_ledger` 同一条）：只看群头 16KB，
    两个生产者都把形状写在数据之前。尺寸写在三种单位上（像素、twips 目标、缩放百分比），
    文件里没有一个地方写 DPI，所以像素那两个不换算法"""
    stop = group_stop(text, at)
    head = text[at:min(stop, at + PIC_CAP)]
    kind, data_at = blip_word(head)
    read = hex_head(head, data_at) if data_at is not None else b""
    sig = picture_kind(read)
    seen, props = shape_props(head)
    alt = None
    for one in props:
        if one["name"] == "wzDescription":
            alt = one["value"]
            break
    return {
        "blip": kind,
        "sig": sig,
        "sig_agrees": blip_agrees(kind, sig),
        "head_hex": read.hex(),
        "pixels": {"w": word_in_group(head, "picw"), "h": word_in_group(head, "pich")},
        "goal": {
            "w": word_in_group(head, "picwgoal"),
            "h": word_in_group(head, "pichgoal"),
            "unit": "twips",
            "mm_w": _twips_mm(word_in_group(head, "picwgoal")),
            "mm_h": _twips_mm(word_in_group(head, "pichgoal")),
        },
        "scale": {"x": word_in_group(head, "picscalex"), "y": word_in_group(head, "picscaley")},
        "crop": {
            "left": word_in_group(head, "piccropl"),
            "right": word_in_group(head, "piccropr"),
            "top": word_in_group(head, "piccropt"),
            "bottom": word_in_group(head, "piccropb"),
        },
        "props_written": seen,
        "props": props,
        "alt": alt,
        "alt_written": any(one["name"] == "wzDescription" for one in props),
        "truncated": stop > at + PIC_CAP,
    }


def _twips_mm(raw):
    """twips 换成 0.01mm：与 Rust 的 `paper::twips` 同一条整数式子（lyco_pages.convert）"""
    if raw is None:
        return None
    try:
        return convert(raw, "twips")[0]
    except (ValueError, KeyError, IndexError):
        return None


def list_level_of(group: str, at: int) -> dict:
    """`{\\listlevel\\levelnfc0…{\\leveltext …;}…}` 一群：这一级的账。
    级别号是**这一份 list 里第几个 `\\listlevel`**（实测 LibreOffice 一份 `\ilvl` 也不写在
    级上），所以号是读者按顺序给的，不是文件写的"""
    kids = child_groups(group)

    def pick(want: str):
        for one in kids:
            if starts_word(one, want):
                return written_of(one)
        return None

    return {
        "at": at,
        "nfc": word_in_group(group, "levelnfc"),
        "jc": word_in_group(group, "leveljc"),
        "startat": word_in_group(group, "levelstartat"),
        "follow": word_in_group(group, "levelfollow"),
        "font": (word_in_group(group, "f") or None),
        "first_indent": word_in_group(group, "fi"),
        "indent": word_in_group(group, "li"),
        "level_text": pick("leveltext"),
        "level_numbers": pick("levelnumbers"),
        "children": [written_of(one) for one in kids],
    }


def list_definition_of(group: str) -> dict:
    """`{\\list\\listtemplateid1 {…九级…}\\listid1}` 一群：一份列表定义。
    **`\\listid` 写在群的最后**（按 `{\list\listid` 去抓一条也抓不到，而全文 `\listid`
    有 14 次 —— 这里 7 次、`listoverridetable` 里 7 次），所以整群读完才拿得到号"""
    levels = []
    for one in child_groups(group):
        if starts_word(one, "listlevel"):
            levels.append(list_level_of(one, len(levels)))
    return {
        "template_id": word_in_group(group, "listtemplateid"),
        "list_id": word_in_group(group, "listid"),
        "levels": len(levels),
        "nfc": [one["nfc"] for one in levels],
        "list_level": levels,
    }


def list_override_of(group: str):
    """`{\\listoverride\\listid4\\listoverridecount0\\ls4}` 一群：段上那个 `\ls` 号
    指的是哪一份定义。群里的 `listoverridecount` 说的是「这一条覆写了几级」
    （实测 LibreOffice 写 0 —— 只换个号，一级也没改），与群名不是一回事"""
    ls = word_in_group(group, "ls")
    if ls is None:
        return None
    return {
        "ls": ls,
        "list_id": word_in_group(group, "listid"),
        "override_count": word_in_group(group, "listoverridecount"),
    }


def switch_value(instruction: str, switch: str) -> str:
    r"""指令里 `\o "1-2"` 那一对引号之间的字；没有这个开关就交回 None"""
    at = instruction.find(switch)
    if at < 0:
        return None
    rest = instruction[at + len(switch):]
    start = rest.find('"')
    if start < 0:
        return None
    stop = rest.find('"', start + 1)
    if stop < 0:
        return None
    return rest[start + 1:stop]


def field_instruction(group: str) -> str:
    r"""`\field` 那一群里的域指令原文，**解掉成对的反斜杠**之后。

    与 Rust 那边同一个做法：`{\*\fldinst` 是「不认识就跳」的目标群，一个字不许进正文，
    这里只前瞻读它一眼。群里那一段整个交给 `rtf_text` 解一遍 —— `\\o` 是「一个反斜杠
    转义出的字面反斜杠」后面跟字母 o，解完才是指令本身 `\o`。没有指令的域交回 None
    """
    at = group.find(FLDINST_HEAD)
    if at < 0:
        return None
    _stop, instruction = group_end(group, at + len(FLDINST_HEAD))
    had = rtf_text(instruction.encode("latin-1", "replace"))["text"].strip()
    return had or None


# 批注那两个词（星号群）是本域认得的：注的那一群带着正文与自己的号，紧挨在它前面
# 那条 `{\*\atnauthor …}` 是「谁写的」。锚区两头的 `{\*\atrfstart N}` / `{\*\atrfend N}`
# 不单独进账 —— 那个号已经在注自己的 `ref` 里，两边按号对
ATN_WORDS = {"annotation", "atnauthor"}
ATN_CHILDREN = {"atnref", "atndate"}


def atn_child(child: str):
    r"""`{\*\atnref 0}` 那种「控制字紧跟一个值」的子群：交回 (词, 值)，不是认识的就 None

    与 Rust 的 `atn_child` 同一步法：剥掉开头的 `\*\`，取字母段当词，剩下的整段解一遍
    再 trim —— 不用正则限定值的形状，两家各自限定就会在第三种生产者上分家。
    """
    if not child.startswith(BS + "*" + BS):
        return None
    rest = child[3:]
    name = ""
    k = 0
    while k < len(rest) and rest[k].isalpha():
        name += rest[k]
        k += 1
    if name not in ATN_CHILDREN:
        return None
    had = rtf_text(rest[k:].encode("latin-1", "replace"))["text"].strip()
    return (name, had) if had else None


def starred_body(text: str, head: int, named: str) -> tuple:
    r"""从控制字的第一个字母起那一个星号群：交回（解过的字, 群里认识的那几个值）

    只**前瞻**读 —— 调用方照旧把这一群整群跳过，所以注的字不进正文。
    """
    _stop, inner = group_end(text, head)
    body = inner[len(named):]
    had = rtf_text(body.encode("latin-1", "replace"))["text"].strip()
    kids: dict = {}
    for child in child_groups(body):
        got = atn_child(child)
        if got:
            kids[got[0]] = got[1]
    return had, kids


def contents_of(instructions: list) -> dict:
    r"""目录那份账：RTF 没有 OOXML 那个 w:sdt 壳，也没有 ODF 的 outline-level 属性，
    只有流里一条自报家门的 `TOC …` 域。所以这份账只有这四个键，那两家的键不造假"""
    toc = [one for one in instructions if one.upper().startswith("TOC")]
    levels = None
    for one in toc:
        levels = switch_value(one, LEVEL_SWITCH)
        if levels:
            break
    return {
        "present": bool(toc),
        "via": None if not toc else "field",
        "fields": toc,
        "levels": levels,
    }


def field_link(group: str) -> dict:
    """从 `\field` 那一群的内部文本里取链接：指令里的地址 + `\fldrslt` 的显示文字"""
    at = group.find(FLDINST_HEAD)
    if at < 0:
        return {}
    _stop, instruction = group_end(group, at)
    hit = LINK_INSTRUCTION.search(instruction)
    if not hit:
        return {}
    shown = ""
    nxt = group.find(FLDRSLT_HEAD)
    if nxt >= 0:
        _stop2, result = group_end(group, nxt)
        shown = rtf_text(result.encode("latin-1", "replace"))["text"]
    return {"target": hit.group(1), "text": shown}


def rtf_text(data: bytes) -> dict:
    """返回 `{text, lines, line_count, chars, ...}`：计数都是文件自己账上的数"""
    text = data.decode("latin-1", "replace")
    out: list[str] = []
    page: dict = {
        "headers": [], "footers": [], "notes": [], "links": [],
        "instructions": [], "annotations": [], "destinations": 0,
    }
    # 定义类（字体与样式）不是页面上的字，也不进 page 那几个口袋
    found: dict = {"fonts": [], "styles": [], "list_defs": [], "list_over": []}
    # 段那一份账：每段收尾时记下「这一段的字」与「这一段用的样式号」。
    # 样式号在段属性里（`\pard\s1`），所以它一定出现在这一段的 `\par` 之前
    paras: list = []
    mark = {"start": 0, "style": None}
    # 与 `paras` 一条一条对着收：`\ilvl` / `\ls` / `\li` / `\fi` 与那段正文前那个
    # `{\listtext…}` 群。收在同一处、跟着同一次走，段号才不会分家
    flows: list = []
    para: dict = {"ilvl": None, "ls": None, "li": None, "fi": None, "label": None}
    # 文档级的那张纸：每个词只认第一次写的（`\landscape` 是个旗标，没有数字参数）
    paper_writes: dict = {}
    # 刚读到、还没配上注的那条作者：文件把 `{\*\atnauthor …}` 写在注的前面一格
    pending_author = None
    pending = bytearray()  # 连续的 \'hh 字节，攒着按字符集一起解
    skip: list[bool] = [False]
    # 与 `skip` 同步进出的一对栈：每一群群头上说过的格式控制字，与这一群开群时
    # `out` 走到第几块（收尾时那几块就是这一群的字）
    words_stack: list[list] = [[]]
    run_open: list[int] = [0]
    colors: list = []
    codepage = 1252
    ucount = 1
    i = 0
    stats = {
        "hex_bytes": 0,
        "unicode_escapes": 0,
        "pictures": 0,
        "picture_rows": [],
        "destinations": 0,
        "objects": 0,
        "replacements": 0,
        # 这一群里出现过 `\ftnalt`：LibreOffice 用它把脚注口袋标成尾注
        "alt": 0,
        "note_destinations": 0,
        # 表的四个计数：这几个控制字本身就在那里，数一条是一条（见返回字典里那六个键）
        "row_defines": 0,
        "row_ends": 0,
        "cell_ends": 0,
        "cell_paras": 0,
        "nest_rows": 0,
        "nest_cells": 0,
        "fields": 0,
        # 断点词各几条（只在没被跳过的那一层数：页眉里的 \par 不是正文的段）
        "break_words": {one: 0 for one in BREAK_COUNT},
        # `{\*\atnauthor …}` 出现了几条：与 annotations 的条数不等就是文件自己没配上
        "atnauthors": 0,
        # 样式被用了几次：样式号 → 条数（正文里出现的 \sN，不含样式表自己的那些）
        "style_uses": {},
        # 逐串的字符格式（群头上说过格式控制字的那些群）与落在所有群之外的那些控制字条数
        "run_rows": [],
        "run_words_stray": 0,
        # `{\listtext…}` 那种群出现了几次（与「几个段带标签」是两个数）
        "label_words": 0,
    }

    def flush() -> None:
        if not pending:
            return
        chunk, bad = decode_pending(bytes(pending), codepage)
        stats["replacements"] += bad
        out.append(chunk)
        pending.clear()

    while i < len(text):
        ch = text[i]
        if ch == "{":
            flush()
            skip.append(skip[-1])
            words_stack.append([])
            run_open.append(len(out))
            i += 1
            continue
        if ch == "}":
            flush()
            # 一群收尾：群头上说过格式控制字、群里又有字，才是一条「这几串字长什么样」。
            # 弹栈与 `skip` 同一条规则（第 0 层那个占位不弹），所以此刻 `skip[-1]`
            # 说的正是**这一群**自己是不是被跳过的目标群
            words = words_stack.pop() if len(words_stack) > 1 else []
            began = run_open.pop() if len(run_open) > 1 else len(out)
            # 第 2 层是文档群，它自己的「字」是整篇 —— 那不是串，所以只数第 3 层往下
            if len(skip) >= 3 and not skip[-1]:
                direct = any(one in RUN_DIRECT for one, _ in words)
                said = [(one, two) for one, two in words if one in RUN_WORDS]
                body = "".join(out[began:]).strip()
                if said and body and len(stats["run_rows"]) < RUN_ROW_CAP:
                    stats["run_rows"].append(
                        {
                            "para": len(paras),
                            "text": body,
                            "direct": direct,
                            "words": [[one, two] for one, two in said],
                        }
                    )
            if len(skip) > 1:
                skip.pop()
            i += 1
            continue
        if ch in "\r\n":
            i += 1
            continue
        if not text.startswith(BS, i):
            flush()
            if not skip[-1]:
                out.append(ch)
            i += 1
            continue
        if text.startswith(BS + "*", i):
            # `\*` 说的是「**后面那个目标群**你不认识就整群跳过」。
            # 所以先看一眼那个词：认识的就不跳 —— LibreOffice 的脚注与尾注恰恰写成
            # `{\*\footnote …}`，照「见 \* 就跳」处理会把整条注弄丢（这份件就是这么发现的）。
            # 不认识才跳：fldinst（域指令原文）、userprops、atncluster 都从这一条走。
            flush()
            named = peek_word(text, i + 2)
            if named in NOTE_DESTINATIONS or named in PAGE_DESTINATIONS:
                i += 2
                continue
            if named in DEF_DESTINATIONS and not skip[-1]:
                # `{{\}*\}listtable`：列表定义写在星号群里（实测同一份件里
                # `listtable` 带星号、`listoverridetable` 不带 —— 只认一条路径就会一份读到
                # 一份读不到）。整群照旧跳，destinations 那一笔也不动
                head = i + 2 + len(named)
                if text.startswith(" ", head):
                    head += 1
                _stop, inner = group_end(text, head)
                if named == "listtable":
                    for child in child_groups(inner):
                        if starts_word(child, "list"):
                            found["list_defs"].append(list_definition_of(child))
                elif named == "listoverridetable":
                    for child in child_groups(inner):
                        got = list_override_of(child)
                        if got:
                            found["list_over"].append(got)
            if named in ATN_WORDS and not skip[-1]:
                # 批注那一群**照样跳**（注的字不是页面上的正文），只是前瞻读一遍值 ——
                # 所以 destinations 那一笔账不因为这个改动而变
                head = i + 2
                while head < len(text) and text[head] in " \r\n\\":
                    head += 1
                had, kids = starred_body(text, head, named)
                if named == "annotation":
                    page["annotations"].append(
                        {
                            "author": pending_author,
                            "text": had,
                            "ref": kids.get("atnref"),
                            "date_written": kids.get("atndate"),
                        }
                    )
                    pending_author = None
                elif had:
                    stats["atnauthors"] += 1
                    pending_author = had
            skip[-1] = True
            stats["destinations"] += 1
            i += 2
            continue
        if text.startswith(BS + "'", i):
            stats["hex_bytes"] += 1
            if not skip[-1]:
                pending += bytes.fromhex(text[i + 2 : i + 4])
            i += 4
            continue
        j = i + 1
        word = ""
        while j < len(text) and text[j].isalpha():
            word += text[j]
            j += 1
        if not word:
            # 单字符转义：\{ \} \( \) 与单独一个反斜杠
            flush()
            if j < len(text):
                if not skip[-1]:
                    out.append(text[j])
                i = j + 1
            else:
                i = j
            continue
        digits = ""
        while j < len(text) and (text[j].isdigit() or text[j] == "-"):
            digits += text[j]
            j += 1
        if j < len(text) and text[j] == " ":
            j += 1  # 控制字后的那个空格属于控制字，不是正文
        flush()
        if word == "uc":
            try:
                ucount = int(digits)
            except ValueError:
                ucount = 1
            i = j
            continue
        if word == "ansicpg" and digits:
            try:
                codepage = int(digits)
            except ValueError:
                pass
            i = j
            continue
        if word == "u" and digits:
            if not skip[-1]:
                try:
                    value = int(digits)
                    out.append(chr(value if value >= 0 else 65536 + value))
                    stats["unicode_escapes"] += 1
                except (ValueError, OverflowError):
                    stats["replacements"] += 1
            j = skip_rtf_chars(text, j, max(ucount, 0))
            i = j
            continue
        if word in RUN_WORDS or word in RUN_DIRECT:
            # 群外面（段前缀那一层）说的那些归属于**段**，不属于任何一串字：只数不挂
            if not skip[-1]:
                if len(skip) >= 3:
                    words_stack[-1].append((word, digits))
                elif word in RUN_WORDS:
                    stats["run_words_stray"] += 1
        if word in NOTE_DESTINATIONS and not skip[-1]:
            stop, inner = group_end(text, j)
            sub = rtf_text(inner.encode("latin-1", "replace"))
            # 尾注的判法：自己叫 endnote，或者群里带了 \ftnalt（LibreOffice 的写法）
            kind = "endnote" if word == "endnote" or sub["ftnalt"] else "footnote"
            for line in sub["lines"]:
                page["notes"].append({"kind": kind, "slot": word, "text": line})
            stats["note_destinations"] += 1
            if len(skip) > 1:
                skip.pop()
                words_stack.pop()
                run_open.pop()
            i = stop + 1
            continue
        if word == "ftnalt":
            stats["alt"] += 1
        if word in PAGE_DESTINATIONS and not skip[-1]:
            # 目标群到「关掉当前这一群」的那个 } 止；里面递归走一遍（页眉也有 \par、\u）
            stop, inner = group_end(text, j)
            sub = rtf_text(inner.encode("latin-1", "replace"))
            # 一条一节会同时写进 \header / \headerl / \headert 好几个口袋：
            # 每条都带着自己是哪个口袋，不替文件合并
            key = "headers" if word.startswith("header") else "footers"
            for line in sub["lines"]:
                page[key].append({"slot": word, "text": line})
            page["destinations"] += 1
            if len(skip) > 1:
                skip.pop()
                words_stack.pop()
                run_open.pop()
            i = stop + 1
            continue
        if word in SKIP_DESTINATIONS or word in NOTE_DEFINITION_DESTINATIONS:
            if word in DEF_DESTINATIONS and not skip[-1]:
                # 前瞻：这一群照旧整群跳过（里面的一个字都不进正文），
                # 但本域认得这些定义，所以读一遍再走 —— 不推进游标、不改 skip
                _stop, inner = group_end(text, j)
                if word == "listtable":
                    for child in child_groups(inner):
                        if starts_word(child, "list"):
                            found["list_defs"].append(list_definition_of(child))
                elif word == "listoverridetable":
                    for child in child_groups(inner):
                        got = list_override_of(child)
                        if got:
                            found["list_over"].append(got)
                else:
                    for child in child_groups(inner):
                        got = definition_of(child)
                        if got:
                            found["fonts" if word == "fonttbl" else "styles"].append(got)
            if word == "colortbl" and not skip[-1]:
                # 前瞻那一张颜色表（游标不动、skip 不改）：`\cfN` 点的就是这里的第 N 格
                _stop, inner = group_end(text, j)
                colors.extend(color_table(inner))
            skip[-1] = True
            stats["destinations"] += 1
        elif word == "listtext":
            # 前瞻那一句标签：`{\listtext\pard\plain  1.\tab}`。那是生产者**算好之后写进
            # 文件**的号，不是我们数出来的，所以解出来的字、它点的字体、那条 `\tab` 在不在
            # 与文件那一串原样一起交。字照旧留在正文里（这一群不跳）
            if not skip[-1]:
                stats["label_words"] += 1
                if para["label"] is None:
                    # 读到 `\listtext` 时已经站在这个群里了：从 `j` 到关掉这一群的那个
                    # `}` 就是那一句标签（找下一个 `{` 会一路读到后面好几个段）
                    _stop, inner = group_end(text, j)
                    got = word_in_group(inner, "f")
                    para["label"] = {
                        "text": rtf_text(inner.encode("latin-1", "replace"))["text"],
                        "font": got or None,
                        "tab": word_in_group(inner, "tab") is not None,
                        "written": inner,
                    }
        elif word == "pict":
            skip[-1] = True
            stats["pictures"] += 1
            # 前瞻这一群的群头（不推进游标、也不改 skip —— 那串数据照旧一个字节都不进正文）
            if len(stats["picture_rows"]) < PIC_ROW_CAP:
                stats["picture_rows"].append(picture_ledger(text, j))
        elif word in OBJECT_WORDS:
            skip[-1] = True
            stats["objects"] += 1
        elif word == "field":
            # 域比链接多（页码、日期都是域），所以两个数分开交：条数是条数
            stats["fields"] += 1
            if not skip[-1]:
                # 前瞻一步：读出这一群里的链接，但**不推进游标** ——
                # `\fldrslt` 的显示文字是页面上的字，得留给正文
                brace = text.find("{", j)
                if brace >= 0:
                    _stop, inner = group_end(text, brace)
                    link = field_link(inner)
                    if link:
                        page["links"].append(link)
                    had = field_instruction(inner)
                    if had:
                        page["instructions"].append(had)
        elif not skip[-1]:
            if word in BREAK_WORDS or word in ROW_WORDS:
                out.append("\n")
                # 段收尾：这一段的字与它用的样式号一起记。空段也记 —— 与 lines 的
                # 「只留有字的行」是两本账，别互相冒充
                paras.append(
                    {"text": "".join(out[mark["start"] :]).strip(), "style": mark["style"]}
                )
                flows.append(dict(para))
                for key in ("ilvl", "ls", "li", "fi", "label"):
                    para[key] = None
                mark["start"] = len(out)
                mark["style"] = None
            elif word in TAB_WORDS:
                out.append("\t")
            # 表的账就是数这几个控制字本身（它们在不在跳过区，由上面那条顺序决定）。
            # 「几张表」不敢这么算：那条规则在两份件上试过，单表对、两表数成一张
            if word == "trowd":
                stats["row_defines"] += 1
            elif word == "row":
                stats["row_ends"] += 1
            elif word == "cell":
                stats["cell_ends"] += 1
            elif word == "intbl":
                stats["cell_paras"] += 1
            elif word == "nestrow":
                stats["nest_rows"] += 1
            elif word == "nestcell":
                stats["nest_cells"] += 1
            # 断点词：按词边界数（`\pard` 不算 `\par`、`\sectd` 不算 `\sect`）。
            # 这一族把「换页」写成三种词：`\page` 在这里换页、`\pagebb` / `\pbb`
            # 是「这一段之前换页」，而 Word 那条 w:br type=page 在 LibreOffice 的
            # RTF 导出里就是 `\pagebb`（三份件都这么量到）
            if word in stats["break_words"]:
                stats["break_words"][word] += 1
            # 样式被用了几次：正文里的 \sN（样式表那一群已被吃掉，不会自己数自己）。
            # 同一处也记下「这一段现在用的是哪个样式」—— 段属性就在 \par 之前
            if word == "s" and digits.isdigit():
                which = int(digits)
                stats["style_uses"][which] = stats["style_uses"].get(which, 0) + 1
                mark["style"] = which
            # 段自己说过的号：`\ilvl` 与 `\ls`（号本在 listoverridetable 那一头，这里只记
            # 文件写在段上的那两串，不拿号当号用）。`\li` / `\fi` 记最后写下的那一个 ——
            # 这一族段上写了一份，列表定义里另有一份
            if word in ("ilvl", "ls", "li", "fi") and digits:
                para[word] = digits
            # 那张纸写在文档级的属性里（`\paperw12240\paperh15840\margl1800…`，单位 twips）。
            # 只认**没被跳过的那一层**里第一次写的那一个：后面 `{\*\sectx …}` 里的那些是
            # 某一节的覆写，不是文档默认值（这一族不看分节归属，所以也不去数它）
            if word in PAPER_WORDS and word not in paper_writes:
                if word == "landscape" or digits.isdigit():
                    paper_writes[word] = "1" if word == "landscape" else digits
        i = j
    flush()
    body = "".join(out).strip()
    lines = [one.strip() for one in body.split("\n") if one.strip()]
    # 用了几次的样式按名字合：文件写的是 `\s1`，名字在样式表那一群里；
    # 用了却没定义的样式号也照交（名字给 `sN` 这种占位，不编一个好看的）。
    # 只按**段落样式**查：`\sN` 与 `\csN` 是两个各自的编号空间，别互相冒充名字
    by_index = {
        one["index"]: one["name"]
        for one in found["styles"]
        if one["kind"] == "paragraph" and one["name"]
    }
    uses = [
        {"index": which, "name": by_index.get(which, "s%d" % which), "count": count}
        for which, count in sorted(stats["style_uses"].items())
    ]
    # 标题：样式名写成 `heading N` 的那些段。这不是「猜」出来的层级 —— 样式名与级别
    # 都在文件里（LibreOffice 与 Word 都用这个形状，大小写各家不同，所以不分大小写匹配）
    headings = []
    for one in paras:
        if one["style"] is None or not one["text"]:
            continue
        named = by_index.get(one["style"])
        if not named:
            continue
        hit = HEADING_NAME.match(named)
        if hit:
            headings.append({"level": int(hit.group(1)), "text": one["text"]})
    # 列表那份账：段上的 `\ls` → `listoverridetable` 里那一条 → 它点名的 `\listid` →
    # `listtable` 里那一份定义 → 定义里第 `\ilvl` 个 `{\listlevel`。
    # 四步各自一个布尔，指不到就交到那一步为止，不拿邻居的数顶上
    entries = []
    checked = len(paras)
    with_ilvl = with_ls = with_label = 0
    override_found = definition_found = level_found = 0
    for at, had in enumerate(flows):
        if not (had["ilvl"] or had["ls"] or had["label"]):
            continue
        with_ilvl += 1 if had["ilvl"] else 0
        with_ls += 1 if had["ls"] else 0
        with_label += 1 if had["label"] else 0
        over = None
        if had["ls"]:
            over = next((one for one in found["list_over"] if one["ls"] == had["ls"]), None)
        list_id = over["list_id"] if over else None
        held = None
        if list_id:
            held = next((one for one in found["list_defs"] if one["list_id"] == list_id), None)
        level = None
        if held is not None and (had["ilvl"] or "").lstrip("-").isdigit():
            want = int(had["ilvl"])
            if 0 <= want < len(held["list_level"]):
                level = held["list_level"][want]
        override_found += 1 if over else 0
        definition_found += 1 if held else 0
        level_found += 1 if level else 0
        label = had["label"] or {}
        style = paras[at]["style"]
        entries.append(
            {
                "at": at,
                "text": paras[at]["text"],
                "style_index": style,
                "style_name": by_index.get(style) if style is not None else None,
                "ilvl": had["ilvl"],
                "ls": had["ls"],
                "indent": {"li": had["li"], "fi": had["fi"]},
                "override_found": over is not None,
                "list_id": list_id,
                "template_id": held["template_id"] if held else None,
                "definition_found": held is not None,
                "level_found": level is not None,
                "level": level,
                "label": label.get("text"),
                "label_font": label.get("font"),
                "label_tab": bool(label.get("tab")),
                "label_written": label.get("written"),
            }
        )
    defs = [
        {
            "at": at,
            "list_id": one["list_id"],
            "template_id": one["template_id"],
            "levels": one["levels"],
            "nfc": one["nfc"],
            "used_by": sum(1 for had in entries if had["list_id"] == one["list_id"]),
        }
        for at, one in enumerate(found["list_defs"])
    ]
    numbering = {
        "checked": checked,
        "listed": len(entries),
        "with_ilvl": with_ilvl,
        "with_ls": with_ls,
        "with_label": with_label,
        "label_words": stats["label_words"],
        "list_definitions": len(found["list_defs"]),
        "overrides": len(found["list_over"]),
        "override_list": [dict(one) for one in found["list_over"]],
        "levels": sum(one["levels"] for one in found["list_defs"]),
        "override_found": override_found,
        "definition_found": definition_found,
        "level_found": level_found,
        "resolved": definition_found,
        "definitions": defs,
        "list": entries,
    }
    resolve_run_rows(stats["run_rows"], colors, found["fonts"])
    return {
        "headings": headings,
        "numbering": numbering,
        # 文档级那张纸的原样（`{"paperw":"12240","margt":"1440","landscape":"1"}`）——
        # 换成 0.1mm 的换算放在 `lyco_pages.py`，三家共用同一条换算规则才好对账
        "paper_writes": paper_writes,
        "fonts": found["fonts"],
        "styles": found["styles"],
        "style_uses": uses,
        "text": body,
        "lines": lines,
        "line_count": len(lines),
        "chars": len(body),
        "declared_codepage": codepage,
        "hex_bytes": stats["hex_bytes"],
        "unicode_escapes": stats["unicode_escapes"],
        "pictures": stats["pictures"],
        "picture_rows": stats["picture_rows"],
        # 逐串的字符格式（号已经按文件自己那两张表跳过一跳）与落在所有群之外的那些控制字
        "run_rows": stats["run_rows"],
        "run_words_stray": stats["run_words_stray"],
        "embedded_objects": stats["objects"],
        "skipped_destinations": stats["destinations"],
        "headers": page["headers"],
        "footers": page["footers"],
        "page_destinations": page["destinations"],
        "notes": page["notes"],
        # 链接：`{\field{\*\fldinst HYPERLINK "地址"}{\fldrslt 显示文字}}` 那一群读出来的
        "links": page["links"],
        "fields": stats["fields"],
        # 每个域自己写的指令原文（解掉成对反斜杠之后），按文件里的顺序
        "field_instructions": page["instructions"],
        # 目录那份账：TOC 域在这份表里挑出来，级数在它自己的开关上
        "contents": contents_of(page["instructions"]),
        # 批注：`{\*\annotation …}` 那一群前瞻读出来的（字不混进正文）。`date` 在
        # 这边压根没有 —— 文件写的 `atndate` 两个样本都对不上 docx 的 w:date，
        # 解不动就只交原样那串（date_written），不替它挑历法
        "annotations": page["annotations"],
        "annotation_authors": stats["atnauthors"],
        # 断点词的条数（六个键固定）：`\pagebb` 那一条就是 Word 的分页符换了一种写法
        "break_words": dict(stats["break_words"]),
        "note_destinations": stats["note_destinations"],
        # 表那份账：六个数都是控制字的条数，不是「表」的推断
        "table_row_defines": stats["row_defines"],
        "table_rows": stats["row_ends"],
        "table_cells": stats["cell_ends"],
        "table_cell_paras": stats["cell_paras"],
        "nested_table_rows": stats["nest_rows"],
        "nested_table_cells": stats["nest_cells"],
        "ftnalt": bool(stats["alt"]),
        "replacement_chars": stats["replacements"],
    }


def rtf_info(data: bytes) -> dict:
    """读 `\\info` 群与其中的 `\\*\\userprops`：RTF 的元数据全在这一带，键由控制字自己说。

    三处不能想当然（都是拿真件踩出来的，见 fixture README）：
    * `\\upr{A}{B}` 的第一群是 7 位回退文本 —— 写这份样本的工具在那儿只留下问号，
      真值在第二群 `\\*\\ud{...}` 里。正文抽取器把 `\\*` 群整群跳过是对的，读元数据时
      照搬就会丢掉标题；所以这里只跳 `\\upr` 的第一群，`\\*` 反而不跳。
    * `\\info{}` 在真件里只包住了标题那一对 `\\upr`，其余键（subject / keywords /
      doccomm / author / creatim / userprops）紧跟在同一层排着 —— 只认「第一个群」
      会漏掉九成元数据。所以扫到下一个**非元数据目标**为止，并给一个硬上界。
    * `\\uN` 可以是负数（UTF-16 的符号扩展），`\\ucN` 声明要丢的回退字符里 `\\'hh`
      只算一个字符。
    """
    text = data.decode("latin-1", "replace")
    at = text.find(BS + "info")
    if at < 0:
        return {"found": False, "fields": {}, "user_props": [], "notes": [r"没有 info 群"]}
    codepage = 1252
    where = text[:at].rfind(BS + "ansicpg")
    if where >= 0:
        probe = where + len(BS + "ansicpg")
        lead = ""
        while probe < len(text) and text[probe].isdigit():
            lead += text[probe]
            probe += 1
        if lead:
            codepage = int(lead)

    TEXT_KEYS = {
        "title": "title",
        "subject": "subject",
        "author": "author",
        "operator": "operator",
        "keywords": "keywords",
        "doccomm": "comment",
        "comments": "comment",
        "lastsavedby": "last_saved_by",
        "nchars": "chars",
        "nwords": "words",
        "npages": "pages",
        "nparas": "paragraphs",
        "nlines": "lines",
        "version": "version",
        "category": "category",
        "manager": "manager",
        "company": "company",
        "propname": "propname",
        "staticval": "staticval",
    }
    TIME_KEYS = {"creatim": "created", "revtim": "modified", "printim": "printed"}
    TIME_PARTS = {"yr", "mo", "dy", "hr", "min", "sec"}
    STOP = {
        "stylesheet", "fonttbl", "colortbl", "generator", "listtable",
        "listoverridetable", "themedata", "colorschememapping", "datastore",
        "rsidtbl", "xmlnstbl", "filetbl", "header", "footer", "pict", "object",
        "sect", "latentstyles", "bkmkstart", "atncluster", "mmathPr",
    }

    fields: dict = {}
    props: list = []
    notes: list = []
    state = {
        "ucount": 1,
        "stack": [{"key": None, "skip": False, "buf": ""}],
        "next_key": None,
        "skip_next": False,
        "current_time": None,
    }
    pending = bytearray()

    def live() -> bool:
        one = state["stack"][-1]
        return one["key"] is not None and not one["skip"]

    def put(chunk: str) -> None:
        """文字先进**当前那一帧的缓冲区**，群闭合时整块交账。

        逐字交账会把 `AppVersion` 变成 10 条自定义属性 —— 真件上就这么错过。
        """
        if not chunk or not live():
            return
        state["stack"][-1]["buf"] += chunk

    def commit(frame: dict) -> None:
        key, chunk = frame.get("key"), frame.get("buf", "")
        if key is None or not chunk:
            return
        if key == "propname":
            props.append({"name": chunk, "type": None, "value": None})
        elif key == "staticval":
            if props:
                props[-1]["value"] = (props[-1]["value"] or "") + chunk
        elif key not in ("created", "modified", "printed"):
            fields[key] = (fields.get(key) or "") + chunk

    def flush() -> None:
        if not pending:
            return
        raw = bytes(pending)
        pending.clear()
        try:
            put(raw.decode(f"cp{codepage}"))
        except (LookupError, UnicodeDecodeError):
            put(raw.decode("cp1252", "replace"))

    i = at
    end = min(len(text), at + 65536)
    while i < end:
        ch = text[i]
        if ch == "{":
            flush()
            parent = state["stack"][-1]["key"]
            state["stack"].append(
                {
                    "key": state["next_key"] or parent,
                    "skip": state["skip_next"],
                    "buf": "",
                }
            )
            state["next_key"] = None
            state["skip_next"] = False
            i += 1
            continue
        if ch == "}":
            flush()
            if len(state["stack"]) > 1:
                commit(state["stack"].pop())
            i += 1
            continue
        if ch in "\r\n":
            i += 1
            continue
        if not text.startswith(BS, i):
            flush()
            put(ch)
            i += 1
            continue
        if text.startswith(BS + "*", i):
            # \* 在正文抽取里是「整群跳过」，在这里不是：真值就在 \*\ud 群里
            i += 2
            continue
        if text.startswith(BS + "'", i):
            if live():
                pending += bytes.fromhex(text[i + 2 : i + 4])
            i += 4
            continue
        j = i + 1
        word = ""
        while j < end and text[j].isalpha():
            word += text[j]
            j += 1
        if not word:
            flush()
            put(text[j] if j < end else "")
            i = j + 1 if j < end else j
            continue
        digits = ""
        while j < end and (text[j].isdigit() or text[j] == "-"):
            digits += text[j]
            j += 1
        if j < end and text[j] == " ":
            j += 1
        flush()
        if word == "uc":
            state["ucount"] = int(digits) if digits else 1
        elif word == "u" and digits:
            try:
                value = int(digits)
                if value < 0:
                    value += 65536
                put(chr(value))
            except (ValueError, OverflowError):
                notes.append("u 的数值解不出字符: " + digits)
            i = skip_rtf_chars(text, j, state["ucount"])
            continue
        elif word in TEXT_KEYS:
            # 这类控制字出现在它自己那一群的开头（{ 标题 文字}），定的是当前这一帧的键
            frame = state["stack"][-1]
            if frame["key"] is None:
                frame["key"] = TEXT_KEYS[word]
            else:
                state["next_key"] = TEXT_KEYS[word]
            if digits and TEXT_KEYS[word] == "version":
                fields["version"] = digits
        elif word in TIME_KEYS:
            state["current_time"] = TIME_KEYS[word]
            fields[state["current_time"]] = {
                "yr": 0, "mo": 0, "dy": 0, "hr": 0, "min": 0, "sec": 0,
            }
        elif word in TIME_PARTS and state["current_time"]:
            try:
                fields[state["current_time"]][word] = int(digits or 0)
            except ValueError:
                pass
        elif word == "proptype":
            if props and digits:
                try:
                    props[-1]["type"] = int(digits)
                except ValueError:
                    pass
        elif word == "upr":
            state["skip_next"] = True
        elif word in STOP:
            break
        i = j
    flush()
    for name in ("created", "modified", "printed"):
        stamp = fields.get(name)
        if not isinstance(stamp, dict):
            continue
        if not stamp.get("yr"):
            fields.pop(name)
            notes.append(name + " 在文件里是全零（等于没写这个时间）")
        else:
            fields[name] = (
                "%04d-%02d-%02dT%02d:%02d:%02d"
                % (stamp["yr"], stamp["mo"], stamp["dy"], stamp["hr"], stamp["min"], stamp["sec"])
            )
    return {
        "found": True,
        "fields": fields,
        "user_props": props,
        "notes": notes,
        "codepage": codepage,
    }
