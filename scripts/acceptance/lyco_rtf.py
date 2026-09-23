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


def rtf_text(data: bytes) -> dict:
    """返回 `{text, lines, line_count, chars, ...}`：计数都是文件自己账上的数"""
    text = data.decode("latin-1", "replace")
    out: list[str] = []
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
            # \* 的语义是「不认识这个目标群就整群跳过」。本域不认识任何带 \* 的目标：
            # fldinst（域指令原文，如 HYPERLINK 的 URL）、userprops、atncluster（批注的
            # 内部控制文本）都从这里过 —— 不跳就会把指令与二进制混进正文。
            flush()
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
        if word in SKIP_DESTINATIONS:
            skip[-1] = True
            stats["destinations"] += 1
        elif word == "pict":
            skip[-1] = True
            stats["pictures"] += 1
        elif word in OBJECT_WORDS:
            skip[-1] = True
            stats["objects"] += 1
        elif not skip[-1]:
            if word in BREAK_WORDS or word in ROW_WORDS:
                out.append("\n")
            elif word in TAB_WORDS:
                out.append("\t")
        i = j
    flush()
    body = "".join(out).strip()
    lines = [one.strip() for one in body.split("\n") if one.strip()]
    return {
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
        "replacement_chars": stats["replacements"],
    }
