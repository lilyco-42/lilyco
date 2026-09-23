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


def rtf_text(data: bytes) -> dict:
    """返回 `{text, lines, line_count, chars, ...}`：计数都是文件自己账上的数"""
    text = data.decode("latin-1", "replace")
    out: list[str] = []
    page: dict = {"headers": [], "footers": [], "destinations": 0}
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
        "headers": page["headers"],
        "footers": page["footers"],
        "page_destinations": page["destinations"],
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
