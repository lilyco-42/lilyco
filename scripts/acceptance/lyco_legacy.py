r"""遗留办公格式（.doc / .xls）的**独立读者**：只用标准库。

这个文件与 `lilyco-binfmt/src/word.rs`、`lilyco-binfmt/src/biff.rs` 是同一套规范的两份实现。
两边对同一批真实生产者文件（LibreOffice 写的 .doc / .xls）必须给出同样的文本，
这一点由 CI 上的 `office_probe.py` 核对 —— 单靠一份实现说「我读出来是这样」不算证据。

- `.doc`：MS-DOC 的正文位置不在 WordDocument 流里顺排，而是 FIB → `fcClx` → 表流里的
  `PlcPcd`（piece 表）；每个 piece 自己说字符宽（fc 的 bit30 = 压缩 → 8 位，偏移还要除二），
  所以中英混排能各自省一半空间 —— 读错这一位就整篇碎字。
- `.xls`：MS-XLS 的 Workbook 流是一串记录；字符串集中在 SST（可跨 CONTINUE 边界，
  每进一个新块都要重读一个 grbit 字节），单元格只存 SST 索引或 RK 压缩数。
"""

from __future__ import annotations

import struct
from pathlib import Path  # noqa: F401  # 与调用方保持同样的导入面

# ---------------------------------------------------------------- MS-DOC
WORD_FIELD_START = 0x13
WORD_FIELD_SEP = 0x14
WORD_FIELD_END = 0x15
# Word 拿来当结构用的字符：段落结束、单元格/行、软换行、分页、对象与批注锚……
WORD_STRUCTURAL = {0x01, 0x02, 0x05, 0x08, 0x0A, 0x0B, 0x0C, 0x0E, 0x0F, 0x16, 0x17, 0x18}


def _u8(buf: bytes, off: int):
    return buf[off] if 0 <= off < len(buf) else None


def _u16(buf: bytes, off: int):
    return struct.unpack_from("<H", buf, off)[0] if off + 2 <= len(buf) else None


def _u32(buf: bytes, off: int):
    return struct.unpack_from("<I", buf, off)[0] if off + 4 <= len(buf) else None


def clean_word_text(body: str) -> list[str]:
    """域代码只留结果部分，结构字符换成制表/换行或丢掉"""
    out: list[str] = []
    in_field = False
    keep_result = False
    for ch in body:
        code = ord(ch)
        if code == WORD_FIELD_START:
            in_field = True
            keep_result = False
            continue
        if code == WORD_FIELD_SEP:
            keep_result = True
            continue
        if code == WORD_FIELD_END:
            in_field = False
            keep_result = False
            continue
        if in_field and not keep_result:
            continue
        if code == 0x0D:
            out.append("\n")
        elif code == 0x07:
            out.append("\t")
        elif code in WORD_STRUCTURAL:
            continue
        else:
            out.append(ch)
    return [one.strip() for one in "".join(out).split("\n") if one.strip()]


def doc_pieces(cfb_bytes: dict) -> dict:
    """按 FIB 的 piece 表把 .doc 的正文取出来（入参是已经解析好的 {流名: 字节}）"""
    wd = cfb_bytes.get("WordDocument")
    if wd is None:
        return {"error": "没有 WordDocument 流"}
    flags = _u16(wd, 10) or 0
    table = "1Table" if flags & 0x0200 else "0Table"
    if table not in cfb_bytes:
        table = "1Table" if "1Table" in cfb_bytes else "0Table"
    if table not in cfb_bytes:
        return {"error": "1Table 与 0Table 都不在容器里"}
    tbl = cfb_bytes[table]
    fc_clx = _u32(wd, 0x01A2) or 0
    lcb_clx = _u32(wd, 0x01A6) or 0
    clx = tbl[fc_clx : fc_clx + lcb_clx]
    if len(clx) != lcb_clx:
        return {"error": f"表流装不下 Clx：要 {lcb_clx} 字节，只有 {len(clx)}"}
    at = 0
    plc = b""
    prcs = 0
    while at < len(clx):
        kind = clx[at]
        if kind == 1:  # Prc：图形参数，跳过
            cb = _u16(clx, at + 1) or 0
            at += 3 + cb
            prcs += 1
        elif kind == 2:  # Pcdt：正文的 piece 表
            lcb = _u32(clx, at + 1) or 0
            plc = clx[at + 5 : at + 5 + lcb]
            break
        else:
            return {"error": f"Clx 里出现不认识的标记 {kind}（位置 {at}）"}
    if len(plc) < 16:
        return {"error": "Pcdt 太短，读不出 piece 表", "table_stream": table}
    n = (len(plc) - 4) // 12
    cps = [_u32(plc, k * 4) or 0 for k in range(n + 1)]
    pieces = []
    text: list[str] = []
    for k in range(n):
        base = (n + 1) * 4 + k * 8
        fc = _u32(plc, base + 2) or 0
        compressed = bool(fc & 0x4000_0000)
        fc &= 0x3FFF_FFFF
        count = cps[k + 1] - cps[k]
        if compressed:
            raw = wd[fc // 2 : fc // 2 + count]
            text.append(raw.decode("cp1252", "replace"))
        else:
            raw = wd[fc : fc + count * 2]
            text.append(raw.decode("utf-16-le", "replace"))
        pieces.append(
            {"cp_start": cps[k], "chars": count, "compressed": compressed, "fc": fc}
        )
    body = "".join(text)
    lines = clean_word_text(body)
    return {
        "table_stream": table,
        "fc_clx": fc_clx,
        "lcb_clx": lcb_clx,
        "prc_skipped": prcs,
        "pieces": pieces,
        "piece_count": n,
        "cp_total": cps[-1] if cps else 0,
        "raw_chars": len(body),
        "text": body,
        "lines": lines,
        "line_count": len(lines),
    }


# ---------------------------------------------------------------- MS-XLS（BIFF8）
def decode_rk(packed: int) -> float:
    """RK 数（MS-XLS 2.5.10）：低两位是标志 —— bit0 = 除以 100，bit1 = 整数

    这两个位的顺序是最容易记反的地方：记反了 124000 会变成 1.05e-310，
    一个「看起来像浮点误差」的错值。判定标准很简单 —— 与 Excel 里看到的数一致。
    """
    div100 = bool(packed & 0x01)
    is_int = bool(packed & 0x02)
    signed = struct.unpack("<i", struct.pack("<I", packed & 0xFFFF_FFFF))[0]
    if is_int:
        value = float(signed >> 2)
    else:
        value = struct.unpack("<d", struct.pack("<Q", (packed & 0xFFFF_FFFF) << 32))[0]
    return value / 100.0 if div100 else value


class _ChunkReader:
    """在 SST 的多个块（SST 正文 + 若干 CONTINUE）上顺序取字节"""

    def __init__(self, chunks: list) -> None:
        self.chunks = chunks
        self.chunk = 0
        self.pos = 0

    def take(self, n: int) -> bytes:
        buf = bytearray()
        while len(buf) < n and self.chunk < len(self.chunks):
            data = self.chunks[self.chunk]
            room = len(data) - self.pos
            if room <= 0:
                self.chunk += 1
                self.pos = 0
                continue
            grab = min(room, n - len(buf))
            buf += data[self.pos : self.pos + grab]
            self.pos += grab
        return bytes(buf)

    def exhausted(self) -> bool:
        return self.chunk >= len(self.chunks) or self.pos >= len(self.chunks[self.chunk])

    def at_chunk_end(self) -> bool:
        return self.chunk + 1 < len(self.chunks) and self.pos >= len(self.chunks[self.chunk])


def read_sst(chunks: list, unique: int) -> list:
    """SST 的字符串可以跨 CONTINUE 边界，而且每进一个新块都要重读一个 grbit 字节"""
    cursor = _ChunkReader(chunks)
    out: list[str] = []
    for _ in range(unique):
        head = cursor.take(3)
        if len(head) < 3:
            break
        cch = struct.unpack("<H", head[:2])[0]
        grbit = head[2]
        rich = bool(grbit & 0x08)
        ext = bool(grbit & 0x04)
        wide = bool(grbit & 0x01)
        crun = struct.unpack("<H", cursor.take(2))[0] if rich else 0
        cext = struct.unpack("<I", cursor.take(4))[0] if ext else 0
        pieces: list[str] = []
        remaining = cch
        while remaining > 0:
            if cursor.at_chunk_end() or cursor.exhausted():
                if cursor.chunk + 1 >= len(chunks):
                    break
                cursor.chunk += 1
                cursor.pos = 0
                flag = cursor.take(1)
                if not flag:
                    break
                wide = bool(flag[0] & 0x01)
            data = cursor.chunks[cursor.chunk]
            per = 2 if wide else 1
            room = len(data) - cursor.pos
            can = min(remaining, room // per)
            if can == 0:
                continue
            raw = cursor.take(can * per)
            pieces.append(raw.decode("utf-16-le" if wide else "cp1252", "replace"))
            remaining -= can
        text = "".join(pieces)
        if rich:
            cursor.take(crun * 4)
        if ext:
            cursor.take(cext)
        out.append(text)
    return out


def _owner(sheets: list, offset: int):
    """这条记录落在哪张表的子流里：自报起点不超过它的最后一条 BOUNDSHEET。

    `.xls` 的所有表共用同一条 Workbook 流，BOUNDSHEET.lbPlyPos 给出的就是该子流
    在这条流内的字节偏移 —— 不按偏移归位，就只能按流顺序报一堆没有主人的格子。
    """
    for one in reversed(sheets):
        if one["record_start"] <= offset:
            return one["name"]
    return None


def a1(col: int, row: int) -> str:
    """`(列, 行)` → `A1` 那种写法：列是 26 进制但没有「0 列」这一位"""
    letters = ""
    while True:
        letters = chr(ord("A") + col % 26) + letters
        if col < 26:
            break
        col = col // 26 - 1
    return "%s%d" % (letters, row + 1)


# 批注在这条流里的三条记录。记录号只在这份件的意义上用 —— 手上没有第二个能写
# .xls 批注的生产者，所以不替它们编规范名（MS-XLS 把 0x001C 那个位置留给 EXTERNSHEET，
# 而这里量到的内容是「哪个格子 + 谁写的」，硬套名字就是编话）
NOTE_TEXT = 0x01B6
NOTE_CELL = 0x001C


def biff_workbook(cfb_bytes: dict) -> dict:
    """把 Workbook 流的记录表读成：工作表清单、共享字符串、带值的单元格（按表归位）"""
    raw = cfb_bytes.get("Workbook") or cfb_bytes.get("Book")
    if raw is None:
        return {"error": "既没有 Workbook 也没有 Book 流"}
    records: list = []
    at = 0
    while at + 4 <= len(raw):
        op = _u16(raw, at) or 0
        ln = _u16(raw, at + 2) or 0
        records.append((at, op, raw[at + 4 : at + 4 + ln]))
        at += 4 + ln
    sheets: list = []
    strings: list = []
    cells: list = []
    bofs: list = []
    formulas = 0
    dimensions = []
    locks: dict = {}
    # 数字格式那一跳：格子的 ixfe 是 XF 记录的**出现序号**，XF 正文偏移 2 是格式号，
    # 自定义号（>=164）的格式串在 FORMAT 记录里，内置号没有串
    xfs: list = []
    formats: dict = {}
    date1904 = None
    # 隐藏行与隐藏列：BIFF 不写开关，写在 ROW 与 COLINFO 的字段位上
    hidden_rows: dict = {}
    hidden_cols: dict = {}
    # 批注那三条记录分两处住：字在表子流里跟着格子走，格子与作者在子流末尾
    note_texts: dict = {}
    note_cells: dict = {}
    for index, (offset, op, body) in enumerate(records):
        belongs = _owner(sheets, offset)
        if op == 0x0809:  # BOF
            bofs.append((_u16(body, 0), _u16(body, 2)))
        elif op == 0x00FC:  # SST
            unique = _u32(body, 4) or 0
            chunks = [bytearray(body[8:])]
            probe = index + 1
            while probe < len(records) and records[probe][1] == 0x003C:
                chunks.append(bytearray(records[probe][2]))
                probe += 1
            strings = read_sst(chunks, unique)
        elif op == 0x0085:  # BOUNDSHEET：lbPlyPos(4) + grbit(2) + ShortXLUnicodeString
            grbit = _u16(body, 4) or 0
            nl = _u8(body, 6) or 0
            flags = _u8(body, 7) or 0
            wide = bool(flags & 0x01)
            text = (
                body[8 : 8 + nl * 2].decode("utf-16-le", "replace")
                if wide
                else body[8 : 8 + nl].decode("cp1252", "replace")
            )
            sheets.append(
                {
                    "name": text,
                    "state": {0: "visible", 1: "hidden", 2: "very-hidden"}.get(
                        grbit & 3, "visible"
                    ),
                    "record_start": _u32(body, 0),
                }
            )
        elif op == 0x00E0:  # XF：ixfeParent(2) + ifmt(2) + 样式位，格式号在偏移 2
            xfs.append(_u16(body, 2))
        elif op == 0x041E:  # FORMAT：内置号（<164）不在这里，自定义号才写串
            ifmt = _u16(body, 0)
            cch = _u16(body, 2) or 0
            flags = _u8(body, 4) or 0
            wide = bool(flags & 0x01)
            raw = body[5 : 5 + cch * (2 if wide else 1)]
            formats[ifmt or 0] = (
                raw.decode("utf-16-le", "replace") if wide else raw.decode("cp1252", "replace")
            )
        elif op == 0x0022:  # DATEMODE：0 = 1900 基准，1 = 1904
            date1904 = bool(_u16(body, 0))
        elif op == 0x0208:  # ROW：这一行的属性（行高、标志位、默认 XF）
            # 「这一行被隐藏」在哪一位是**量**出来的：LibreOffice 把 0x20 这一位写在
            # 正文偏移 12 那一格，偏移 8 那一格（MS-XLS 说那里是 grbit）它留零。
            # 三份对照件把两个变量拆开（README 第 30 条）：五行长高从 4pt 到 250pt 全不隐藏，
            # 那一格恒为 0x0140；只把第三行藏起来，它就变成 0x0120 —— 动的只有 0x20 这一位，
            # 0x40 那一位跟着「有没有自定义行高」走。所以查 0x20 不会把看得见的行算成隐藏。
            # 两个位置都查是因为手上只有 LibreOffice 写的 .xls：按偏移 8 那一位判的那条路
            # 在这台机器上没有任何件走过，宁可两处都看。
            if belongs is not None and len(body) >= 14:
                if ((_u16(body, 8) or 0) | (_u16(body, 12) or 0)) & 0x20:
                    hidden_rows.setdefault(belongs, []).append(_u16(body, 0) or 0)
        elif op in (0x07D0, 0x007D):  # COLINFO：BIFF8 写 0x07D0，LibreOffice 写 0x007D
            # 正文：colFirst(2) colLast(2) 宽度(2) 默认 XF(2) grbit(2) 保留(2)
            # grbit 的 0x01 位 = 这段列隐藏；范围是首末都含的，少展开一格就少报一列
            if len(body) >= 10 and (_u16(body, 8) or 0) & 0x01:
                first = _u16(body, 0) or 0
                last = _u16(body, 2) or 0
                hidden_cols.setdefault(belongs, []).extend(range(first, last + 1))
        elif op == 0x00FD:  # LABELSST
            r, col, xf, sst_index = struct.unpack_from("<HHHI", body, 0)
            value = strings[sst_index] if sst_index < len(strings) else None
            cells.append(
                {
                    "row": r,
                    "col": col,
                    "type": "sst",
                    "value": value,
                    "ixfe": xf,
                    "sheet": belongs,
                }
            )
        elif op == 0x0203:  # NUMBER
            r, col, xf, value = struct.unpack_from("<HHHd", body, 0)
            cells.append(
                {
                    "row": r,
                    "col": col,
                    "type": "number",
                    "value": value,
                    "ixfe": xf,
                    "sheet": belongs,
                }
            )
        elif op == 0x027E:  # RK
            r, col, xf = struct.unpack_from("<HHH", body, 0)
            cells.append(
                {
                    "row": r,
                    "col": col,
                    "type": "rk",
                    "value": decode_rk(_u32(body, 6) or 0),
                    "ixfe": xf,
                    "sheet": belongs,
                }
            )
        elif op == 0x0204:  # LABEL（老式：字符串直接跟在记录里）
            r, col, xf = struct.unpack_from("<HHH", body, 0)
            cch = _u16(body, 6) or 0
            grbit = body[8] if len(body) > 8 else 0
            raw_text = body[9 : 9 + cch * (2 if grbit & 1 else 1)]
            cells.append(
                {
                    "row": r,
                    "col": col,
                    "type": "label",
                    "value": raw_text.decode(
                        "utf-16-le" if grbit & 1 else "cp1252", "replace"
                    ),
                    "ixfe": xf,
                    "sheet": belongs,
                }
            )
        elif op == 0x0006:  # FORMULA
            r, col, xf = struct.unpack_from("<HHH", body, 0)
            cells.append(
                {
                    "row": r,
                    "col": col,
                    "type": "formula",
                    "value": None,
                    "ixfe": xf,
                    "sheet": belongs,
                }
            )
            formulas += 1
        elif op == 0x00BD:  # MULRK：一行里连续若干列
            # rw(2) + colFirst(2) + 每 6 字节一个 {ixfe(2), rkmac(4)} + colLast(2)。
            # rkmac 是每条的**后**四个字节：读早两字节就把 ixfe 当成了数，解出来是
            # 一个看着像浮点误差的乱数（真件量出来的，见 mulrk.xls）
            r = _u16(body, 0) or 0
            col_from = _u16(body, 2) or 0
            for i in range((len(body) - 6) // 6):
                packed = _u32(body, 6 + i * 6) or 0
                cells.append(
                    {
                        "row": r,
                        "col": col_from + i,
                        "type": "mulrk",
                        "value": decode_rk(packed),
                        "ixfe": _u16(body, 4 + i * 6),
                        "sheet": belongs,
                    }
                )
        elif op == 0x0200:  # DIMENSIONS
            dimensions.append((_u32(body, 0), _u32(body, 4)))
        elif op == NOTE_TEXT:
            # 注的字：正文偏移 10 是这条记录自报的字数（偏移 0 是它自报的头长 18，
            # 偏移 12 那份件一律写 0x0010），紧跟的第一条 CONTINUE 首字节是编码旗标
            # （0 = 一格一字节，1 = 一格两字节）。这个位义与 BIFF8 那个
            # fCompressed 的惯例**相反**，是拿一份 ASCII 作者的件与三份中文作者的件
            # 对出来的，不是引来的。后面那条 CONTINUE 是注的扩展头（不是字的续块），
            # 所以只吃第一条，并按自报的字数切 —— 字比这个数长时如实报不完整
            cch = _u16(body, 10) or 0
            text, wide, whole = None, None, False
            probe = index + 1
            while probe < len(records) and records[probe][1] == 0x003C:
                blob = records[probe][2]
                probe += 1
                if text is not None or not blob:
                    continue
                wide = blob[0] == 1
                need = cch * (2 if wide else 1)
                cut = blob[1 : 1 + need]
                text = cut.decode("utf-16-le" if wide else "cp1252", "replace")
                whole = len(cut) == need
            note_texts.setdefault(belongs, []).append(
                {"text": text, "wide": wide, "whole": whole, "cch": cch}
            )
        elif op == NOTE_CELL:
            # 注住在哪个格子、谁写的：row(2) col(2) 那位留零(2) 自报的序号(2)
            # 作者字数(2) 编码旗标(1) 作者串（这一族的串后面还跟一个 0x00）
            row = _u16(body, 0) or 0
            col = _u16(body, 2) or 0
            acch = _u16(body, 8) or 0
            wide = bool((_u8(body, 10) or 0) & 1)
            need = acch * (2 if wide else 1)
            cut = body[11 : 11 + need]
            note_cells.setdefault(belongs, []).append(
                {
                    "row": row,
                    "col": col,
                    "slot": _u16(body, 6),
                    "author": cut.decode("utf-16-le" if wide else "cp1252", "replace"),
                    "whole": len(cut) == need,
                }
            )
        elif op in (0x0012, 0x0013, 0x00DD):
            # PROTECT / PASSWORD / SCENPROTECT。为什么按「落在谁的子流里」记：
            # 对照 locked-sheet.xls 与 locked-second.xls（唯一差别是锁在第一张还是
            # 第二张表），这三条记录跟着锁挪窝；而 LibreOffice 自己 import 回 .ods
            # 也只在被锁那张上写 table:protected —— 于是「.xls 的锁是按表的」是量出来的
            where = belongs if belongs is not None else "工作簿全局"
            got = locks.setdefault(where, {})
            got["%04x" % op] = {
                "grbit": _u16(body, 0),
                "hex": body[:8].hex(),
                "length": len(body),
            }
    per_sheet: dict = {}
    for one in cells:
        per_sheet[one["sheet"]] = per_sheet.get(one["sheet"], 0) + 1
    # 两份列表按出现顺序配：量的这三份件里字的记录与格子记录同序，而格子记录自己
    # 还写着一个 1 起的序号 —— 两个都对上才算读过。对不上时两份计数都交出来，
    # 让「配了几条」与「各有几条」在同一份账上看得见，而不是只报一个小的数
    comments: dict = {}
    for name in [one["name"] for one in sheets]:
        anchors = note_cells.get(name, [])
        texts = note_texts.get(name, [])
        got = []
        for position in range(min(len(anchors), len(texts))):
            anchor, had = anchors[position], texts[position]
            got.append(
                {
                    "ref": a1(anchor["col"], anchor["row"]),
                    "author": anchor["author"],
                    "date": None,
                    "text": had["text"],
                    "whole": bool(anchor["whole"] and had["whole"]),
                }
            )
        comments[name] = {
            "list": got,
            "text_records": len(texts),
            "cell_records": len(anchors),
        }
    return {
        "comments": comments,
        "records": len(records),
        "bofs": bofs,
        "sheets": sheets,
        "shared_strings": strings,
        "cells": cells,
        "formula_cells": formulas,
        "dimensions": dimensions,
        "cells_per_sheet": per_sheet,
        "locks": locks,
        "xfs": xfs,
        "formats": {str(k): v for k, v in formats.items()},
        "date1904": date1904,
        # 位置而不是条数：隐藏行是逐条 ROW 记录，隐藏列是首末都含的范围，
        # 交回展开后的位置才对得上 xlsx / ods 那两份账
        "hidden": {
            name: {
                "rows": sorted(set(hidden_rows.get(name) or [])),
                "cols": sorted(set(hidden_cols.get(name) or [])),
            }
            for name in {one["name"] for one in sheets}
        },
    }


# ── MS-PPT（PowerPoint 97）：PowerPoint Document 流的记录树 ─────────────────


PPT_TEXT_ATOMS = {0x0FA0: "text-chars", 0x0FA8: "text-bytes", 0x0FBA: "c-string"}
PPT_SLIDE_CONTAINER = 0x03F8
PPT_C_STRING = 0x0FBA
# 一页一个的容器（数值是实测出来的，与 pptx 侧逐张对过；名字我没有）
PPT_SLIDE_RECORD = 0x03EE
PPT_MAX_DEPTH = 8


def _ppt_head(buf: bytes, off: int):
    """记录头：+0 recVer（容器 0xF / 原子 0x0）、+2 recType、+4 正文长度。

    这个布局不是背来的：这份真件里三个已知原子（4000 / 3999 / 4010）都落在 +2，
    而且按它整条流严丝合缝地铺满。
    """
    if off + 8 > len(buf):
        return None
    size = _u32(buf, off + 4)
    if size is None:
        return None
    return _u16(buf, off) or 0, _u16(buf, off + 2) or 0, int(size)


def _ppt_tiles(buf: bytes) -> bool:
    """这一段正文能不能再走成一条完整的记录流（走完正好停在末尾才算）"""
    if len(buf) < 8:
        return False
    at = 0
    while at + 8 <= len(buf):
        head = _ppt_head(buf, at)
        if head is None or at + 8 + head[2] > len(buf):
            return False
        at += 8 + head[2]
    return at == len(buf)


def ppt_decode_atom(kind: int, body: bytes) -> str:
    """TextBytesAtom 按 Windows-1252；另两种规范写「16 位字符」，真实生产者非 ASCII
    时写的是 UTF-16 —— 判据用字节自己给：高字节全零时两种读法结果相同。"""
    data = bytes(body)
    if kind == 0x0FA8:
        return data.decode("cp1252", "replace")
    wide = any(data[i] != 0 for i in range(1, len(data), 2))
    if wide:
        return data.decode("utf-16-le", "replace")
    return data[0::2].decode("cp1252", "replace")


def ppt_text(cfb_bytes: dict) -> dict:
    """递归走出记录树，按流顺序取出文本原子（容器靠「正文能铺满」判，不看 recVer）"""
    raw = cfb_bytes.get("PowerPoint Document")
    if raw is None:
        return {"error": "容器里没有 PowerPoint Document 流"}
    atoms: list = []
    notes: list = []
    slides: list = []
    box = {"records": 0, "containers": 0, "slide_containers": 0}

    def walk(buf: bytes, depth: int, current: int | None) -> None:
        at = 0
        while at + 8 <= len(buf) and box["records"] < 200000:
            head = _ppt_head(buf, at)
            if head is None:
                break
            _inst, kind, size = head
            end = at + 8 + size
            if end > len(buf):
                notes.append(f"深度 {depth} 处一条记录伸出这一层末尾（在 {at}）")
                return
            box["records"] += 1
            body = buf[at + 8 : end]
            tiles_here = depth < PPT_MAX_DEPTH and bool(body) and _ppt_tiles(bytes(body))
            # 按页归位：recType 0x03EE 的容器一页一个（这条对应关系是拿同一份文档的
            # pptx 那一份逐张对出来的，不是照 recType 的名字猜的 —— 名字我没有）
            child = current
            if kind == PPT_SLIDE_RECORD and tiles_here:
                slides.append({"record_offset": at, "depth": depth, "name": "", "atoms": 0, "lines": []})
                # 只往**这一条记录的子树**里传，不复用 current：改了它，同一层后面的
                # 兄弟记录（母版、备注…）就会被算进上一页
                child = len(slides) - 1
            if kind in PPT_TEXT_ATOMS:
                text = ppt_decode_atom(kind, body)
                atoms.append(
                    {
                        "kind": PPT_TEXT_ATOMS[kind],
                        "depth": depth,
                        "offset": at,
                        "text": text,
                    }
                )
                if child is not None:
                    one = slides[child]
                    one["atoms"] += 1
                    if kind == PPT_C_STRING:
                        # LibreOffice 每页写一条版式名（`___PPT10`），不是页面上的字
                        if not one["name"]:
                            one["name"] = text
                    else:
                        for part in text.replace("\r", "\n").replace("\x0b", "\n").split("\n"):
                            if part.strip():
                                one["lines"].append(part)
            if kind == PPT_SLIDE_CONTAINER:
                box["slide_containers"] += 1
            if tiles_here:
                box["containers"] += 1
                walk(bytes(body), depth + 1, child)
            at = end

    walk(bytes(raw), 0, None)
    lines: list = []
    for one in atoms:
        for part in one["text"].replace("\r", "\n").replace("\x0b", "\n").split("\n"):
            if part.strip():
                lines.append(part)
    return {
        "records": box["records"],
        "text_atoms": atoms,
        "lines": lines,
        "containers": box["containers"],
        "slide_containers": box["slide_containers"],
        "slides": slides,
        "notes": notes,
    }
