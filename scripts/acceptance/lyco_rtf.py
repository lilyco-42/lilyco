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
# 制表类（表格单元格与行分隔）→ 输出一个制表符
TAB_WORDS = {"tab", "cell", "nestcell"}
# 行结束：一行表格就是一行文本（把 \row 当制表符会把整张表挤成一行）
ROW_WORDS = {"row", "nestrow"}
# 嵌套对象类：整个跳过并计数（办公文件里常见的是 OLE 对象与图片）
OBJECT_WORDS = {"object", "objattph", "objdata", "objclass", "objname", "objemb", "objhide"}


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
DEF_DESTINATIONS = {"fonttbl", "stylesheet"}
# 一群里的第一条控制字决定它是什么定义：`\fN` 字体、`\sN` 段落样式、`\csN` 字符样式
FIRST_DEF = re.compile(r"^\\(?:\*\\)?(cs|s|f)(\d+)")
KIND_OF_PREFIX = {"f": "font", "s": "paragraph", "cs": "character"}
# 一个字体条目自己声明的字符集：`\fcharset0` 是 ANSI，非 0 的那一串（128 是 Shift-JIS、
# 134 是 GB2312 …）意味着名字里的字节不是 cp1252 —— 按 cp1252 解出来就是乱码，
# 所以那种条目只交字符集号与原始字节，不交一个我们解错的名字
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
    page: dict = {"headers": [], "footers": [], "notes": [], "links": [], "destinations": 0}
    # 定义类（字体与样式）不是页面上的字，也不进 page 那几个口袋
    found: dict = {"fonts": [], "styles": []}
    pending = bytearray()  # 连续的 \'hh 字节，攒着按字符集一起解
    skip: list[bool] = [False]
    codepage = 1252
    ucount = 1
    i = 0
    stats = {
        "hex_bytes": 0,
        "unicode_escapes": 0,
        "pictures": 0,
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
        # 样式被用了几次：样式号 → 条数（正文里出现的 \sN，不含样式表自己的那些）
        "style_uses": {},
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
            i += 1
            continue
        if ch == "}":
            flush()
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
            i = stop + 1
            continue
        if word in SKIP_DESTINATIONS or word in NOTE_DEFINITION_DESTINATIONS:
            if word in DEF_DESTINATIONS and not skip[-1]:
                # 前瞻：这一群照旧整群跳过（里面的一个字都不进正文），
                # 但本域认得这些定义，所以读一遍再走 —— 不推进游标、不改 skip
                _stop, inner = group_end(text, j)
                for child in child_groups(inner):
                    got = definition_of(child)
                    if got:
                        found["fonts" if word == "fonttbl" else "styles"].append(got)
            skip[-1] = True
            stats["destinations"] += 1
        elif word == "pict":
            skip[-1] = True
            stats["pictures"] += 1
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
        elif not skip[-1]:
            if word in BREAK_WORDS or word in ROW_WORDS:
                out.append("\n")
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
            # 样式被用了几次：正文里的 \sN（样式表那一群已被吃掉，不会自己数自己）
            if word == "s" and digits.isdigit():
                which = int(digits)
                stats["style_uses"][which] = stats["style_uses"].get(which, 0) + 1
        i = j
    flush()
    body = "".join(out).strip()
    lines = [one.strip() for one in body.split("\n") if one.strip()]
    # 用了几次的样式按名字合：文件写的是 `\s1`，名字在样式表那一群里；
    # 用了却没定义的样式号也照交（名字给 `sN` 这种占位，不编一个好看的）
    by_index = {one["index"]: one["name"] for one in found["styles"]}
    uses = [
        {"index": which, "name": by_index.get(which, "s%d" % which), "count": count}
        for which, count in sorted(stats["style_uses"].items())
    ]
    return {
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
        "embedded_objects": stats["objects"],
        "skipped_destinations": stats["destinations"],
        "headers": page["headers"],
        "footers": page["footers"],
        "page_destinations": page["destinations"],
        "notes": page["notes"],
        # 链接：`{\field{\*\fldinst HYPERLINK "地址"}{\fldrslt 显示文字}}` 那一群读出来的
        "links": page["links"],
        "fields": stats["fields"],
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
