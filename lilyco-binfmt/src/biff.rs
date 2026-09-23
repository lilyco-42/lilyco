//! MS-XLS（BIFF8）：`.xls` 的 Workbook 流是一串记录，本模块读出「有哪些表 /
//! 有哪些字符串 / 哪些格子里有值」。
//!
//! 三处只有踩过才会写进注释的地方：
//! 1. **SST 的字符串可以跨 CONTINUE 边界**，而且每进一个新块都要**重读一个 grbit 字节**
//!    —— 也就是说同一个字符串的前半可能是 8 位、后半是 16 位。连着读成一整块字节
//!    再解，就会在跨界处出现「一个字符吃掉下一个字符的头」。
//! 2. **RK 数的两个标志位**：bit0 是「除以 100」，bit1 是「整数」。记反的话
//!    124000 会变成 `1.05e-310` —— 一个看着像浮点误差、其实是位序错的值。
//! 3. **BOUNDSHEET 的可见性在 grbit 的最低两位**（0 可见 / 1 隐藏 / 2 深度隐藏），
//!    不是从 bit2 开始；错位就把隐藏表报成可见表。
//!
//! 还有一处容易想当然的：**`.xls` 的「表」不是容器里的多条流**。整本工作簿只有
//! 一条 `Workbook` 流，每张表是这条流里的一个子流，`BOUNDSHEET.lbPlyPos` 给出的
//! 正是该子流在 `Workbook` 内的**字节偏移**。所以要问「这个格子属于哪张表」，
//! 答案是「起点不超过它的那最后一条 BOUNDSHEET」—— 靠数 BOF 的出现次序也能蒙对，
//! 但那是猜规范，而偏移是文件自己写着的。
//!
//! 与 `scripts/acceptance/lyco_legacy.py` 是同一套规范的两份实现，两边对同一批
//! 真实生产者文件（openpyxl 写的 xlsx 经 LibreOffice 转成 xls）必须给出同样的表名、
//! 可见性与单元格值。

use serde_json::{json, Value};

use crate::cfb::Cfb;
use crate::read::{le16, le32, le64};
use crate::word::decode_cp1252;

/// BIFF8 记录号（MS-XLS 2.4）
const BOF: u64 = 0x0809;
const BOUNDSHEET: u64 = 0x0085;
const SST: u64 = 0x00FC;
const CONTINUE: u64 = 0x003C;
const LABELSST: u64 = 0x00FD;
const NUMBER: u64 = 0x0203;
const RK: u64 = 0x027E;
const MUL_RK: u64 = 0x00BD;
const LABEL: u64 = 0x0204;
const FORMULA: u64 = 0x0006;

#[derive(Debug, Clone)]
pub struct Sheet {
    pub name: String,
    pub state: &'static str,
    pub record_start: u64,
}

#[derive(Debug, Clone)]
pub struct Cell {
    pub row: u32,
    pub col: u32,
    pub kind: &'static str,
    pub text: Option<String>,
    pub number: Option<f64>,
    /// 这条记录落在哪张表的子流里（由 BOUNDSHEET 的字节偏移判出；全局区里的为 None）
    pub sheet: Option<String>,
}

impl Cell {
    /// 单元格引用：`(0,0)` → `A1`。列是 26 进制但没有「0 列」这一位
    pub fn reference(&self) -> String {
        let mut col = self.col;
        let mut letters: Vec<char> = Vec::new();
        loop {
            letters.push(char::from(b'A' + (col % 26) as u8));
            if col < 26 {
                break;
            }
            col = col / 26 - 1;
        }
        letters.reverse();
        format!("{}{}", letters.iter().collect::<String>(), self.row + 1)
    }

    pub fn to_json(&self) -> Value {
        json!({
            "ref": self.reference(),
            "row": self.row,
            "col": self.col,
            "kind": self.kind,
            "text": self.text,
            "number": self.number,
            "sheet": self.sheet,
        })
    }
}

#[derive(Debug)]
pub struct Book {
    pub records: usize,
    pub bofs: Vec<(u64, u64)>,
    pub sheets: Vec<Sheet>,
    pub strings: Vec<String>,
    pub cells: Vec<Cell>,
    pub formula_cells: usize,
    pub notes: Vec<String>,
}

pub fn read(cfb: &Cfb, bytes: &[u8]) -> Result<Book, String> {
    let raw = cfb
        .read(bytes, "Workbook")
        .or_else(|| cfb.read(bytes, "Book"))
        .ok_or("容器里既没有 Workbook 也没有 Book 流")?;
    let mut records: Vec<(usize, u64, Vec<u8>)> = Vec::new();
    let mut at = 0usize;
    while at + 4 <= raw.len() {
        let op = le16(at)(&raw).unwrap_or(0);
        let len = usize::try_from(le16(at + 2)(&raw).unwrap_or(0)).unwrap_or(0);
        let start = at + 4;
        let body = raw.get(start..start + len).unwrap_or(&[]).to_vec();
        if len > raw.len().saturating_sub(start) {
            records.push((at, op, body));
            break;
        }
        records.push((at, op, body));
        at = start + len;
    }
    let mut sheets: Vec<Sheet> = Vec::new();
    let mut strings: Vec<String> = Vec::new();
    let mut cells: Vec<Cell> = Vec::new();
    let mut bofs: Vec<(u64, u64)> = Vec::new();
    let mut formula_cells = 0usize;
    let mut notes: Vec<String> = Vec::new();
    for index in 0..records.len() {
        let (offset, op, body) = &records[index];
        // 这条记录落在哪张表的子流里。BOUNDSHEET 全部待在全局区，所以走到任何一条
        // 单元格记录时清单都已经收齐了。
        let belongs = owner(&sheets, *offset);
        match *op {
            BOF => bofs.push((le16(0)(body).unwrap_or(0), le16(2)(body).unwrap_or(0))),
            BOUNDSHEET => {
                // lbPlyPos(4) + grbit(2) + ShortXLUnicodeString{cch(1), flags(1), 正文}
                let grbit = le16(4)(body).unwrap_or(0);
                let count = usize::from(body.get(6).copied().unwrap_or(0));
                let wide = body.get(7).copied().unwrap_or(0) & 0x01 != 0;
                let take = if wide { count * 2 } else { count };
                let raw_name = body.get(8..8 + take).unwrap_or(&[]);
                let name = if wide {
                    let mut units: Vec<u16> = Vec::new();
                    for pair in raw_name.chunks(2) {
                        if pair.len() == 2 {
                            units.push(u16::from_le_bytes([pair[0], pair[1]]));
                        }
                    }
                    String::from_utf16_lossy(&units)
                } else {
                    decode_cp1252(raw_name)
                };
                sheets.push(Sheet {
                    name,
                    state: match grbit & 3 {
                        0 => "visible",
                        1 => "hidden",
                        2 => "very-hidden",
                        _ => "unknown",
                    },
                    record_start: le32(0)(body).unwrap_or(0),
                });
            }
            SST => {
                let unique = usize::try_from(le32(4)(body).unwrap_or(0)).unwrap_or(usize::MAX);
                let mut chunks: Vec<Vec<u8>> = vec![body.get(8..).unwrap_or(&[]).to_vec()];
                let mut probe = index + 1;
                while probe < records.len() && records[probe].1 == CONTINUE {
                    chunks.push(records[probe].2.clone());
                    probe += 1;
                }
                strings = shared_strings(&chunks, unique, &mut notes);
            }
            LABELSST => {
                let row = le16(0)(body).unwrap_or(0) as u32;
                let col = le16(2)(body).unwrap_or(0) as u32;
                let which = usize::try_from(le32(6)(body).unwrap_or(0)).unwrap_or(usize::MAX);
                let text = strings.get(which).cloned();
                if text.is_none() {
                    notes.push(format!(
                        "LABELSST 指向 SST 第 {which} 条，但那份表只有 {} 条",
                        strings.len()
                    ));
                }
                cells.push(Cell {
                    row,
                    col,
                    kind: "sst",
                    text,
                    number: None,
                    sheet: belongs.clone(),
                });
            }
            NUMBER => {
                let row = le16(0)(body).unwrap_or(0) as u32;
                let col = le16(2)(body).unwrap_or(0) as u32;
                cells.push(Cell {
                    row,
                    col,
                    kind: "number",
                    text: None,
                    number: Some(f64::from_bits(le64(6)(body).unwrap_or(0))),
                    sheet: belongs.clone(),
                });
            }
            RK => {
                let row = le16(0)(body).unwrap_or(0) as u32;
                let col = le16(2)(body).unwrap_or(0) as u32;
                cells.push(Cell {
                    row,
                    col,
                    kind: "rk",
                    text: None,
                    number: Some(decode_rk(le32(6)(body).unwrap_or(0) as u32)),
                    sheet: belongs.clone(),
                });
            }
            MUL_RK => {
                // 一行里连续若干列的 RK：colFrom(2) 之后每 6 字节一个 {xf(2), rk(4)}
                let row = le16(0)(body).unwrap_or(0) as u32;
                let first = le16(2)(body).unwrap_or(0) as u32;
                let room = body.len().saturating_sub(6);
                for i in 0..room / 6 {
                    let packed = le32(4 + i * 6)(body).unwrap_or(0) as u32;
                    cells.push(Cell {
                        row,
                        col: first + i as u32,
                        kind: "mulrk",
                        text: None,
                        number: Some(decode_rk(packed)),
                        sheet: belongs.clone(),
                    });
                }
            }
            LABEL => {
                let row = le16(0)(body).unwrap_or(0) as u32;
                let col = le16(2)(body).unwrap_or(0) as u32;
                let count = usize::try_from(le16(6)(body).unwrap_or(0)).unwrap_or(0);
                let flags = body.get(8).copied().unwrap_or(0);
                let wide = flags & 0x01 != 0;
                let raw_text = body
                    .get(9..9 + count * if wide { 2 } else { 1 })
                    .unwrap_or(&[]);
                cells.push(Cell {
                    row,
                    col,
                    kind: "label",
                    text: Some(if wide {
                        let mut units: Vec<u16> = Vec::new();
                        for pair in raw_text.chunks(2) {
                            if pair.len() == 2 {
                                units.push(u16::from_le_bytes([pair[0], pair[1]]));
                            }
                        }
                        String::from_utf16_lossy(&units)
                    } else {
                        decode_cp1252(raw_text)
                    }),
                    number: None,
                    sheet: belongs.clone(),
                });
            }
            FORMULA => {
                let row = le16(0)(body).unwrap_or(0) as u32;
                let col = le16(2)(body).unwrap_or(0) as u32;
                cells.push(Cell {
                    row,
                    col,
                    kind: "formula",
                    text: None,
                    number: None,
                    sheet: belongs.clone(),
                });
                formula_cells += 1;
            }
            _ => {}
        }
    }
    Ok(Book {
        records: records.len(),
        bofs,
        sheets,
        strings,
        cells,
        formula_cells,
        notes,
    })
}

/// 这条记录属于哪张表：BOUNDSHEET 自报的子流起点里，不超过该记录偏移的最后一条。
/// 表清单是按流顺序收集的，所以从后往前找第一个放得下的就是它。
fn owner(sheets: &[Sheet], offset: usize) -> Option<String> {
    sheets
        .iter()
        .rev()
        .find(|one| usize::try_from(one.record_start).unwrap_or(usize::MAX) <= offset)
        .map(|one| one.name.clone())
}

/// RK 数：bit0 = 除以 100，bit1 = 整数（顺序记反会得到看着像浮点误差的错值）
pub fn decode_rk(packed: u32) -> f64 {
    let div100 = packed & 0x01 != 0;
    let is_int = packed & 0x02 != 0;
    let value = if is_int {
        f64::from((packed as i32) >> 2)
    } else {
        f64::from_bits(u64::from(packed) << 32)
    };
    if div100 {
        value / 100.0
    } else {
        value
    }
}

/// 共享字符串表：字符串可以跨 CONTINUE 边界，且每进一个新块要重读一个 grbit
pub fn shared_strings(chunks: &[Vec<u8>], unique: usize, notes: &mut Vec<String>) -> Vec<String> {
    let mut cursor = ChunkReader::new(chunks);
    let mut out: Vec<String> = Vec::new();
    for _ in 0..unique {
        let head = match cursor.take(3) {
            Some(one) => one,
            None => {
                notes.push("SST 自报的条数比实际可读出的多，后面的读不出来".to_string());
                break;
            }
        };
        let count = u16::from_le_bytes([head[0], head[1]]) as usize;
        let mut grbit = head[2];
        let rich = grbit & 0x08 != 0;
        let ext = grbit & 0x04 != 0;
        let mut wide = grbit & 0x01 != 0;
        let runs = if rich {
            match cursor.take(2) {
                Some(one) => usize::from(u16::from_le_bytes([one[0], one[1]])),
                None => 0,
            }
        } else {
            0
        };
        let extra = if ext {
            match cursor.take(4) {
                Some(one) => usize::try_from(u32::from_le_bytes([one[0], one[1], one[2], one[3]]))
                    .unwrap_or(0),
                None => 0,
            }
        } else {
            0
        };
        let mut pieces: Vec<String> = Vec::new();
        let mut remaining = count;
        while remaining > 0 {
            if cursor.at_chunk_end() {
                if !cursor.next_chunk() {
                    break;
                }
                grbit = match cursor.take(1) {
                    Some(one) => one[0],
                    None => break,
                };
                wide = grbit & 0x01 != 0;
            }
            let per = if wide { 2 } else { 1 };
            let room = cursor.room() / per;
            let can = if room == 0 { 0 } else { remaining.min(room) };
            if can == 0 {
                // 这一块连一个字符都放不下：换块继续（下一块开头有它自己的 grbit）
                if !cursor.next_chunk() {
                    break;
                }
                grbit = match cursor.take(1) {
                    Some(one) => one[0],
                    None => break,
                };
                wide = grbit & 0x01 != 0;
                continue;
            }
            let raw = match cursor.take(can * per) {
                Some(one) => one,
                None => break,
            };
            pieces.push(if wide {
                let mut units: Vec<u16> = Vec::new();
                for pair in raw.chunks(2) {
                    if pair.len() == 2 {
                        units.push(u16::from_le_bytes([pair[0], pair[1]]));
                    }
                }
                String::from_utf16_lossy(&units)
            } else {
                decode_cp1252(&raw)
            });
            remaining -= can;
        }
        if rich {
            cursor.take(runs * 4);
        }
        if extra > 0 {
            cursor.take(extra);
        }
        out.push(pieces.concat());
    }
    out
}

/// 在「SST 正文 + 若干 CONTINUE」上顺序取字节
struct ChunkReader<'a> {
    chunks: &'a [Vec<u8>],
    chunk: usize,
    pos: usize,
}

impl<'a> ChunkReader<'a> {
    fn new(chunks: &'a [Vec<u8>]) -> Self {
        ChunkReader {
            chunks,
            chunk: 0,
            pos: 0,
        }
    }

    fn room(&self) -> usize {
        match self.chunks.get(self.chunk) {
            Some(one) => one.len().saturating_sub(self.pos),
            None => 0,
        }
    }

    /// 当前块已经吃完、后面还有块：这正是 SST 要重读 grbit 的位置
    fn at_chunk_end(&self) -> bool {
        self.room() == 0 && self.chunk + 1 < self.chunks.len()
    }

    fn next_chunk(&mut self) -> bool {
        if self.chunk + 1 >= self.chunks.len() {
            return false;
        }
        self.chunk += 1;
        self.pos = 0;
        true
    }

    fn take(&mut self, n: usize) -> Option<Vec<u8>> {
        if n == 0 {
            return Some(Vec::new());
        }
        let mut out: Vec<u8> = Vec::with_capacity(n);
        while out.len() < n {
            let data = self.chunks.get(self.chunk)?;
            let room = data.len().saturating_sub(self.pos);
            if room == 0 {
                if !self.next_chunk() {
                    return None;
                }
                continue;
            }
            let grab = room.min(n - out.len());
            out.extend_from_slice(&data[self.pos..self.pos + grab]);
            self.pos += grab;
        }
        Some(out)
    }
}

impl Book {
    pub fn to_json(&self) -> Value {
        json!({
            "records": self.records,
            "bofs": self.bofs.iter().map(|(a, b)| json!({"version": a, "type": b})).collect::<Vec<Value>>(),
            "sheets": self.sheets.iter().map(|one| json!({
                "name": one.name, "state": one.state, "record_start": one.record_start,
            })).collect::<Vec<Value>>(),
            "shared_strings": self.strings,
            "cells": self.cells.iter().map(|one| one.to_json()).collect::<Vec<Value>>(),
            "formula_cells": self.formula_cells,
            "notes": self.notes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open(name: &str) -> (Vec<u8>, Cfb) {
        let bytes = std::fs::read(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/office")
                .join(name),
        )
        .expect("读 fixture");
        let cfb = crate::cfb::open(&bytes).expect("打开复合文档");
        (bytes, cfb)
    }

    /// openpyxl 写 xlsx、LibreOffice 转成 xls：三张表的名字与可见性、八个字符串、
    /// 十一个有值的格子，全部要与 `lyco_legacy.py` 独立读出来的结果一致
    #[test]
    fn reads_a_real_producer_workbook() {
        let (bytes, cfb) = open("book.xls");
        let book = read(&cfb, &bytes).expect("读得出 BIFF8");
        assert_eq!(book.records, 175, "{:?}", book.records);
        assert_eq!(book.bofs.len(), 4, "一个全局 BOF + 三张表");
        assert_eq!(book.bofs[0], (1536, 5), "全局 BOF 的 dt 是 5");
        let sheets: Vec<(String, &str)> = book
            .sheets
            .iter()
            .map(|one| (one.name.clone(), one.state))
            .collect();
        assert_eq!(
            sheets,
            vec![
                ("预算表".to_string(), "visible"),
                ("说明".to_string(), "visible"),
                ("草稿".to_string(), "hidden")
            ],
            "{sheets:?}"
        );
        assert_eq!(
            book.strings,
            vec![
                "科目",
                "金额",
                "服务器",
                "网络",
                "合计",
                "口径：含税",
                "第二张表：口径说明",
                "隐藏的草稿表"
            ]
            .iter()
            .map(|one| (*one).to_string())
            .collect::<Vec<String>>()
        );
        assert_eq!(book.cells.len(), 11, "{:?}", book.cells);
        assert_eq!(book.formula_cells, 1);
        assert!(book.notes.is_empty(), "{:?}", book.notes);
        // 按 BOUNDSHEET 自报的子流起点归位：隐藏的「草稿」也有一个格子，
        // 而全局区里没有任何单元格记录（所以不该出现 None）。
        let mut per_sheet: Vec<(String, usize)> = Vec::new();
        for one in &book.cells {
            let name = one.sheet.clone().unwrap_or_else(|| "?全局?".to_string());
            match per_sheet.iter_mut().find(|(had, _)| *had == name) {
                Some((_, count)) => *count += 1,
                None => per_sheet.push((name, 1)),
            }
        }
        assert_eq!(
            per_sheet,
            vec![
                ("预算表".to_string(), 9),
                ("说明".to_string(), 1),
                ("草稿".to_string(), 1)
            ],
            "{per_sheet:?}"
        );
    }

    /// 单元格引用的进制换算：第 26 列是 AA，不是 Z+1
    #[test]
    fn cell_references_use_the_spreadsheet_notation() {
        let at = |row: u32, col: u32| Cell {
            row,
            col,
            kind: "sst",
            text: None,
            number: None,
            sheet: None,
        };
        assert_eq!(at(0, 0).reference(), "A1");
        assert_eq!(at(3, 1).reference(), "B4");
        assert_eq!(at(9, 25).reference(), "Z10");
        assert_eq!(at(0, 26).reference(), "AA1");
        assert_eq!(at(0, 27).reference(), "AB1");
        assert_eq!(at(0, 51).reference(), "AZ1");
    }

    /// RK 的两个标志位：整数 / 除以 100 / 两者组合，都要与 Excel 里看到的数一致
    #[test]
    fn rk_flags_are_bit_zero_for_hundredth_and_bit_one_for_integer() {
        // 124000 写成整数 RK：(124000 << 2) | 0b10
        assert_eq!(decode_rk((124000u32 << 2) | 0x02), 124000.0);
        // 1234.56 写成「整数除以 100」
        assert_eq!(decode_rk((123456u32 << 2) | 0x03), 1234.56);
        assert_eq!(decode_rk((1_234_567u32 << 2) | 0x02), 1234567.0);
        // 负数走符号扩展，不是把补码当正数
        let negative = (((-1234i32) << 2) as u32) | 0x02;
        assert_eq!(decode_rk(negative), -1234.0, "{negative:#x}");
        // IEEE 分支：1.5 的双精度高位 32 位就是 RK 的正文
        assert_eq!(decode_rk(0x3FF8_0000), 1.5);
    }

    /// SST 跨 CONTINUE 边界：字符串的后半要重读 grbit，8 位与 16 位可以在同一个串里换
    #[test]
    fn strings_can_straddle_a_continue_boundary_and_switch_width() {
        // grbit 的 bit0=0 才是 8 位字符；写成 0x01 会连注释里说的「4 个」变成 2 个字
        let first = [b'\x05', b'\x00', b'\x00', b'a', b'b', b'c', b'd']; // cch=5，8 位，只放得下 4 个
        let second = [0x00u8, b'e', b'f']; // 新块开头重读 grbit（还是 8 位）
        let mut notes: Vec<String> = Vec::new();
        let got = shared_strings(&[first.to_vec(), second.to_vec()], 1, &mut notes);
        assert_eq!(got, vec!["abcde".to_string()], "{got:?} {notes:?}");
        // 16 位 → 8 位的切换
        let mut wide_head: Vec<u8> = vec![0x02, 0x00, 0x01]; // cch=2, wide
        wide_head.extend_from_slice(&0x4E2Du32.to_le_bytes()); // 一个中文字
        let mut tail: Vec<u8> = vec![0x00]; // 新块：grbit=0 → 8 位
        tail.push(b'!');
        let got = shared_strings(&[wide_head, tail], 1, &mut notes);
        assert_eq!(got, vec!["中!".to_string()], "{got:?}");
    }

    /// 不是电子表格的复合文档要报错，而不是返回一份空表
    #[test]
    fn a_non_workbook_says_so() {
        let (bytes, cfb) = open("notes.doc");
        let why = read(&cfb, &bytes).unwrap_err();
        assert!(why.contains("Workbook"), "{why}");
    }

    /// 自报的条数比实际可读出的多：读到哪算哪，并把「少了」说出来
    #[test]
    fn a_sst_that_promises_more_than_it_has_says_so() {
        let mut notes: Vec<String> = Vec::new();
        let got = shared_strings(&[Vec::new()], 3, &mut notes);
        assert!(got.is_empty(), "{got:?}");
        assert!(!notes.is_empty(), "读不出条数时要留话");
    }
}
