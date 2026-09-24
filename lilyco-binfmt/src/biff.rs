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

use std::collections::BTreeMap;

use serde_json::{json, Value};

use crate::cfb::Cfb;
use crate::numfmt;
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
const PROTECT: u64 = 0x0012;
const PASSWORD: u64 = 0x0013;
const SCENPROTECT: u64 = 0x00DD;
const XF: u64 = 0x00E0;
const FORMAT: u64 = 0x041E;
const DATEMODE: u64 = 0x0022;
const ROW: u64 = 0x0208;
/// 列的属性：MS-XLS 说这个记录在 BIFF8 里是 0x07D0，而 LibreOffice 写 .xls 时
/// 用的是老 id 0x007D（正文布局一样）—— 两个都认，见 `Sheet::hidden_cols`
const COLINFO: u64 = 0x07D0;
const COLINFO_OLD: u64 = 0x007D;
/// 批注的字。这两个记录号只在「LibreOffice 写的 .xls」这个意义上用：手上没有第二个
/// 能写 .xls 批注的生产者，MS-XLS 又把 0x001C 那个位置留给 EXTERNSHEET，
/// 而这里量到的内容是「哪个格子 + 谁写的」—— 硬套规范名就是编话
const NOTE_TEXT: u64 = 0x01B6;
const NOTE_CELL: u64 = 0x001C;

#[derive(Debug, Clone)]
pub struct Sheet {
    pub name: String,
    pub state: &'static str,
    pub record_start: u64,
    /// 这一张表自己子流里那几条保护记录（记录号 → 16 位原值）。
    /// 不按子流归位就说不清「锁的是哪一张」：对照过两份件，锁挪到第二张表时
    /// 这几条记录跟着挪窝（见 `protect::xls_sheet`）
    pub protection: BTreeMap<u64, u64>,
    /// 这一张表里被整行藏起来的行号（0 基，按 ROW 记录的 0x20 位判出来并排好序）。
    /// 「看不见」不等于「没有」：那些格子里的字仍然算在 cells 里
    pub hidden_rows: Vec<u64>,
    /// 同上，列。COLINFO 写的是首末都含的一段，这里已经展开
    pub hidden_cols: Vec<u64>,
    /// 这一张表的批注：字与「哪个格子、谁写的」在这条流里是两类记录，按出现顺序配
    pub comments: Vec<Comment>,
    /// 那两类记录各几条。配的条数只能到两者中小的那个，所以这两个数要一起交出去
    pub note_text_records: usize,
    pub note_cell_records: usize,
}

/// 一条批注。这里没有日期字段：这一族的三条记录里都不写作者时间，
/// 所以调用方把那一项交回 None，不替文件编一个
#[derive(Debug, Clone)]
pub struct Comment {
    pub reference: String,
    pub author: String,
    pub text: String,
    /// 两边自报的字数是不是都正好切出来。长的注会跨多条 CONTINUE，
    /// 而这一版只吃第一条 —— 那条 CONTINUE 之后还有一条是注的扩展头，不能拼进来
    pub whole: bool,
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
    /// 数字格式那一跳的入口：XF 记录（0x00E0）的出现序号。文字格也带着它，
    /// 但那一格是字还是数由记录类型说，不由格式说，所以查格式只在要用的时候查
    pub ixfe: Option<u64>,
}

/// 16 位单元拼回文字（little-endian，落单的尾字节丢掉）
fn wide_text(bytes: &[u8]) -> String {
    let mut units: Vec<u16> = Vec::new();
    for pair in bytes.chunks(2) {
        if pair.len() == 2 {
            units.push(u16::from_le_bytes([pair[0], pair[1]]));
        }
    }
    String::from_utf16_lossy(&units)
}

/// 单元格引用：`(0,0)` → `A1`。列是 26 进制但没有「0 列」这一位
fn a1(row: u32, col: u32) -> String {
    let mut col = col;
    let mut letters: Vec<char> = Vec::new();
    loop {
        letters.push(char::from(b'A' + (col % 26) as u8));
        if col < 26 {
            break;
        }
        col = col / 26 - 1;
    }
    letters.reverse();
    format!("{}{}", letters.iter().collect::<String>(), row + 1)
}

impl Cell {
    pub fn reference(&self) -> String {
        a1(self.row, self.col)
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
    /// XF 记录（0x00E0）自报的格式号，按出现顺序 —— 格子的 ixfe 就是这里的下标
    pub xfs: Vec<u64>,
    /// FORMAT 记录（0x041E）：只有自定义号（>=164）才写串，内置号在这张表里没有
    pub formats: BTreeMap<u64, String>,
    /// DATEMODE（0x0022）：文件自己没说就交回 None，不默认成 1900
    pub date1904: Option<bool>,
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
    let mut xfs: Vec<u64> = Vec::new();
    let mut formats: BTreeMap<u64, String> = BTreeMap::new();
    let mut date1904: Option<bool> = None;
    let mut notes: Vec<String> = Vec::new();
    // 批注那两类记录分两处住：字跟着格子走，格子与作者在表子流的末尾。
    // 先各自收下，走完再按出现顺序配
    let mut texts_of: BTreeMap<String, Vec<(String, bool)>> = BTreeMap::new();
    let mut cells_of: BTreeMap<String, Vec<Comment>> = BTreeMap::new();
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
                    protection: BTreeMap::new(),
                    hidden_rows: Vec::new(),
                    hidden_cols: Vec::new(),
                    comments: Vec::new(),
                    note_text_records: 0,
                    note_cell_records: 0,
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
                    ixfe: le16(4)(body).map(u64::from),
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
                    ixfe: le16(4)(body).map(u64::from),
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
                    ixfe: le16(4)(body).map(u64::from),
                    sheet: belongs.clone(),
                });
            }
            MUL_RK => {
                // 一行里连续若干列的 RK：rw(2) + colFirst(2) + 每 6 字节一个
                // {ixfe(2), rkmac(4)} + colLast(2)。值是每条的**后**四个字节，
                // 不是紧跟 colFirst 那四个 —— 早两字节就把 ixfe 当成了数，解出的是
                // 一个看着像浮点误差的乱数。这条是在真件上量出来的：手边的 .xls 样本
                // 里没有 MULRK，两份实现一起读早了也没人发现
                let row = le16(0)(body).unwrap_or(0) as u32;
                let first = le16(2)(body).unwrap_or(0) as u32;
                let room = body.len().saturating_sub(6);
                for i in 0..room / 6 {
                    let packed = le32(6 + i * 6)(body).unwrap_or(0) as u32;
                    cells.push(Cell {
                        row,
                        col: first + i as u32,
                        kind: "mulrk",
                        text: None,
                        number: Some(decode_rk(packed)),
                        ixfe: le16(4 + i * 6)(body).map(u64::from),
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
                    ixfe: le16(4)(body).map(u64::from),
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
                    ixfe: le16(4)(body).map(u64::from),
                    sheet: belongs.clone(),
                });
                formula_cells += 1;
            }
            // 表级保护那三条，记在**它所在子流那张表**名下：对照 locked-sheet.xls 与
            // locked-second.xls（唯一差别是锁在第一张还是第二张表），这几条跟着锁挪窝
            code @ (PROTECT | PASSWORD | SCENPROTECT) => {
                let Some(name) = belongs else { continue };
                let Some(value) = le16(0)(body) else { continue };
                if let Some(one) = sheets.iter_mut().rev().find(|had| had.name == name) {
                    one.protection.insert(code, u64::from(value));
                }
            }
            XF => {
                // 格子记的 ixfe 就是 XF 记录在这条流里的**出现序号**，所以这里只能按顺序收。
                // 格式号在正文偏移 2（前两个字节是父样式索引）
                if let Some(done) = le16(2)(body) {
                    xfs.push(u64::from(done));
                }
            }
            FORMAT => {
                // 量出来的布局：ifmt(2) + cch(2) + 拼法(1) + 串（cch 个字节或码元）。
                // 两条记录的总长正好都对得上（12 = 5+7、29 = 5+12×2），所以不是猜的
                let Some(ifmt) = le16(0)(body) else { continue };
                let count = usize::try_from(le16(2)(body).unwrap_or(0)).unwrap_or(0);
                let wide = body.get(4).copied().unwrap_or(0) & 0x01 != 0;
                let raw = body
                    .get(5..5 + count * if wide { 2 } else { 1 })
                    .unwrap_or(&[]);
                let code = if wide {
                    let mut units: Vec<u16> = Vec::new();
                    for pair in raw.chunks(2) {
                        if pair.len() == 2 {
                            units.push(u16::from_le_bytes([pair[0], pair[1]]));
                        }
                    }
                    String::from_utf16_lossy(&units)
                } else {
                    decode_cp1252(raw)
                };
                formats.insert(u64::from(ifmt), code);
            }
            DATEMODE => {
                // 0 = 1900 基准，1 = 1904 —— 序列数换日期要用它
                date1904 = Some(le16(0)(body).unwrap_or(0) == 1);
            }
            // 隐藏整行。位的位置是**量**出来的，不是背出来的（fixture README 第 30 条）：
            // LibreOffice 把 0x20 写在正文偏移 12 那一格，偏移 8 那一格（MS-XLS 说那里是
            // grbit）它留零。拆开两个变量的三份对照件显示：行高从 4pt 到 250pt 只动同一格
            // 的另几位，把某一行藏起来才动 0x20 —— 所以按这一位判不会把看得见的行算成隐藏。
            // 两个位置都查，是因为手上只有 LibreOffice 写的 .xls，按偏移 8 判那条路没件走过
            ROW => {
                let Some(name) = belongs else { continue };
                if (le16(8)(body).unwrap_or(0) | le16(12)(body).unwrap_or(0)) & 0x20 == 0 {
                    continue;
                }
                let Some(rw) = le16(0)(body) else { continue };
                if let Some(one) = sheets.iter_mut().rev().find(|had| had.name == name) {
                    one.hidden_rows.push(u64::from(rw));
                }
            }
            // 隐藏整列：grbit 的第 0 位，首末两端都含，所以按段展开
            COLINFO | COLINFO_OLD => {
                let Some(name) = belongs else { continue };
                if le16(8)(body).unwrap_or(0) & 0x01 == 0 {
                    continue;
                }
                let (Some(first), Some(last)) = (le16(0)(body), le16(2)(body)) else {
                    continue;
                };
                let Some(one) = sheets.iter_mut().rev().find(|had| had.name == name) else {
                    continue;
                };
                for column in u64::from(first)..=u64::from(last) {
                    one.hidden_cols.push(column);
                }
            }
            NOTE_TEXT => {
                let Some(name) = belongs else { continue };
                // 正文偏移 10 是这条记录自报的字数（偏移 0 是它自报的头长 18）
                let count = usize::try_from(le16(10)(body).unwrap_or(0)).unwrap_or(0);
                let mut one: Option<(String, bool)> = None;
                let mut probe = index + 1;
                while probe < records.len() && records[probe].1 == CONTINUE {
                    let blob = &records[probe].2;
                    probe += 1;
                    // 只吃第一条：它后面那条 CONTINUE 是注的扩展头，不是字的续块，
                    // 拼进来就会多出些不像字的字节
                    if one.is_some() || blob.is_empty() {
                        continue;
                    }
                    // 首字节是编码旗标：0 = 一格一字节，1 = 一格两字节。这一位与 BIFF8
                    // 那个 fCompressed 的惯例相反，是拿一份 ASCII 作者的件与几份中文作者
                    // 的件对出来的，不是引来的
                    let wide = blob[0] == 1;
                    let need = count * if wide { 2 } else { 1 };
                    let cut = blob.get(1..1 + need).unwrap_or(&[]);
                    let text = if wide {
                        wide_text(cut)
                    } else {
                        decode_cp1252(cut)
                    };
                    one = Some((text, cut.len() == need));
                }
                texts_of
                    .entry(name)
                    .or_default()
                    .push(one.unwrap_or_else(|| (String::new(), false)));
            }
            NOTE_CELL => {
                let Some(name) = belongs else { continue };
                // row(2) col(2) 那位留零(2) 文件自报的序号(2) 作者字数(2) 编码旗标(1)
                // 作者串（这一族的串后面还跟着一个 0x00）
                let (Some(row), Some(col)) = (le16(0)(body), le16(2)(body)) else {
                    continue;
                };
                let count = usize::try_from(le16(8)(body).unwrap_or(0)).unwrap_or(0);
                let wide = body.get(10).copied().unwrap_or(0) & 1 != 0;
                let need = count * if wide { 2 } else { 1 };
                let cut = body.get(11..11 + need).unwrap_or(&[]);
                let author = if wide {
                    wide_text(cut)
                } else {
                    decode_cp1252(cut)
                };
                cells_of.entry(name).or_default().push(Comment {
                    reference: a1(u32::from(row), u32::from(col)),
                    author,
                    text: String::new(),
                    whole: cut.len() == need,
                });
            }
            _ => {}
        }
    }
    // 行与列按记录出现顺序收的：排一遍再去重，段与段叠在一起也只算一次
    for one in sheets.iter_mut() {
        one.hidden_rows.sort_unstable();
        one.hidden_rows.dedup();
        one.hidden_cols.sort_unstable();
        one.hidden_cols.dedup();
        // 批注按出现顺序配：量的这几份件里字的记录与格子记录同序，而格子记录自己
        // 还写着一个 1 起的序号。配不上的那几条不硬凑 —— 两类记录各自的条数一起交出去
        let texts = texts_of.remove(&one.name).unwrap_or_default();
        let mut cells = cells_of.remove(&one.name).unwrap_or_default();
        one.note_text_records = texts.len();
        one.note_cell_records = cells.len();
        let paired = cells.len().min(texts.len());
        for (cell, (text, whole)) in cells.iter_mut().take(paired).zip(texts.iter()) {
            cell.text = text.clone();
            cell.whole = cell.whole && *whole;
        }
        cells.truncate(paired);
        one.comments = cells;
    }
    Ok(Book {
        records: records.len(),
        bofs,
        sheets,
        strings,
        cells,
        formula_cells,
        xfs,
        formats,
        date1904,
        notes,
    })
}

impl Book {
    /// 这一格的格式号与格式串。串只有自定义号（>=164）才写；内置号交回 None，
    /// 由调用方去查那张内置表（`numfmt::builtin_code`，xlsx 那侧已经在用同一张）
    pub fn format_of(&self, one: &Cell) -> Option<(u64, Option<&String>)> {
        let index = usize::try_from(one.ixfe?).ok()?;
        let ifmt = *self.xfs.get(index)?;
        Some((ifmt, self.formats.get(&ifmt)))
    }

    /// 与 xlsx 那侧同一组字段名，让「同一个问题的三种存法」能并排看：
    /// `num_fmt` / `format` / `format_kind` / `style`，日期格式再加 `as_date`。
    /// 那一格是字还是数由记录类型说（LABELSST 就是字），所以文字格的 `format_kind`
    /// 交回 text，不拿格式串去猜
    pub fn cell_format(&self, one: &Cell) -> Option<Value> {
        let (ifmt, code) = self.format_of(one)?;
        let shown: Option<String> = match code {
            Some(done) => Some(done.clone()),
            None => numfmt::builtin_code(ifmt).map(|done| done.to_string()),
        };
        let judged = match &shown {
            Some(done) => numfmt::kind_of(done),
            None => numfmt::Kind::Unknown,
        };
        let kind = if one.text.is_some() {
            "text"
        } else {
            judged.as_str()
        };
        let mut out = serde_json::Map::new();
        out.insert("num_fmt".to_string(), json!(ifmt));
        out.insert("format".to_string(), json!(shown));
        out.insert("format_kind".to_string(), json!(kind));
        out.insert("style".to_string(), json!(one.ixfe));
        if matches!(
            judged,
            numfmt::Kind::Date | numfmt::Kind::Datetime | numfmt::Kind::Time
        ) {
            let stamp = match (one.number, self.date1904) {
                (Some(done), Some(year1904)) => json!(numfmt::serial_to_iso(done, year1904)),
                _ => Value::Null,
            };
            out.insert("as_date".to_string(), stamp);
        }
        Some(Value::Object(out))
    }

    /// 数字格换算成日期了就给 ISO，否则照旧给序列数 —— CSV 与 `as_date` 走同一个判断，
    /// 不许一条路换了一条路不换
    pub fn shown(&self, one: &Cell) -> String {
        if let Some(done) = &one.text {
            return done.clone();
        }
        let Some(number) = one.number else {
            return String::new();
        };
        if let Some((ifmt, code)) = self.format_of(one) {
            let judged = match code {
                Some(done) => numfmt::kind_of(done),
                None => match numfmt::builtin_code(ifmt) {
                    Some(done) => numfmt::kind_of(done),
                    None => numfmt::Kind::Unknown,
                },
            };
            if matches!(
                judged,
                numfmt::Kind::Date | numfmt::Kind::Datetime | numfmt::Kind::Time
            ) {
                if let Some(year1904) = self.date1904 {
                    return numfmt::serial_to_iso(number, year1904);
                }
            }
        }
        format!("{number}")
    }
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
            ixfe: None,
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
        // 16 位 → 8 位的切换：一个宽字符用掉 2 字节，第二个字符要等到下一个块
        let mut wide_head: Vec<u8> = vec![0x02, 0x00, 0x01]; // cch=2, wide
        wide_head.extend_from_slice(&0x4E2Du16.to_le_bytes()); // 一个中文字（2 字节）
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

    /// MULRK(0x00BD) 的正文，从 `mulrk.xls` 里那条 24 字节的记录原样抄下来
    /// （LibreOffice 把第 5 行三个连续整数并成了一条）
    fn a_real_run_record() -> Vec<u8> {
        vec![
            0x04, 0x00, 0x00, 0x00, 0x0f, 0x00, 0x1e, 0x00, 0x00, 0x00, 0x0f, 0x00, 0x3a, 0x00,
            0x00, 0x00, 0x0f, 0x00, 0x56, 0x00, 0x00, 0x00, 0x02, 0x00,
        ]
    }

    /// 这条记录自己说的是什么：rw + colFirst + 每格 {ixfe(2), rkmac(4)} + colLast。
    /// `rkmac` 是每条的**后**四个字节 —— 早两字节读就把 ixfe(15) 当成了数
    #[test]
    fn a_run_record_names_each_cell_format_before_its_value() {
        let body = a_real_run_record();
        assert_eq!(le16(0)(&body), Some(4), "行号");
        assert_eq!(le16(2)(&body), Some(0), "起始列");
        assert_eq!(le16(4)(&body), Some(15), "第一格的格式索引");
        assert_eq!(le32(6)(&body), Some(0x1e), "第一格的值在这");
        assert_eq!(le32(12)(&body), Some(0x3a));
        assert_eq!(le32(18)(&body), Some(0x56));
        assert_eq!(le16(22)(&body), Some(2), "colLast：三格占到第 2 列");
        assert_eq!(decode_rk(0x1e), 7.0);
        assert_eq!(decode_rk(0x3a), 14.0);
        assert_eq!(decode_rk(0x56), 21.0);
    }

    /// 一行连续数字在 .xls 里是两条 MULRK：值、格子位置与表名都要逐格对得上。
    /// 期望值来自 `lyco_legacy.py` 读同一份件，而这份件与 LibreOffice 自己读回
    /// .ods 交出来的 A2:H2 / A5:C5 逐格一致
    #[test]
    fn a_row_of_numbers_written_as_runs_reads_back_cell_by_cell() {
        let (bytes, cfb) = open("mulrk.xls");
        let book = read(&cfb, &bytes).expect("读得出 BIFF8");
        assert_eq!(book.bofs.len(), 2, "一个全局 BOF + 一张表");
        assert_eq!(book.sheets.len(), 1);
        assert_eq!(book.sheets[0].name, "连续");
        assert_eq!(book.cells.len(), 13, "{:?}", book.cells);
        let run: Vec<(String, f64)> = book
            .cells
            .iter()
            .filter(|one| one.kind == "mulrk")
            .map(|one| (one.reference(), one.number.unwrap_or_default()))
            .collect();
        assert_eq!(
            run,
            vec![
                ("A2".to_string(), 1000.5),
                ("B2".to_string(), 2000.5),
                ("C2".to_string(), 3000.5),
                ("D2".to_string(), 4000.5),
                ("E2".to_string(), 5000.5),
                ("F2".to_string(), 6000.5),
                ("G2".to_string(), 7000.5),
                ("H2".to_string(), 8000.5),
                ("A5".to_string(), 7.0),
                ("B5".to_string(), 14.0),
                ("C5".to_string(), 21.0),
            ],
            "{run:?}"
        );
        assert_eq!(book.strings.len(), 2, "两条文字格");
    }

    /// 隐藏行与隐藏列那两位是在真件上量的：openpyxl 把第 3、4 行与 C/D/E 三列藏起来，
    /// LibreOffice 转成 .xls 之后把它们写进 ROW 的 0x20 位与 COLINFO 的第 0 位。
    /// 这里是 0 基行号；列那段首末都含，展开成三格（期望值来自 `lyco_legacy.py`）
    #[test]
    fn hidden_rows_and_columns_come_from_the_row_and_colinfo_bits() {
        let (bytes, cfb) = open("hidden.xls");
        let book = read(&cfb, &bytes).expect("读得出 BIFF8");
        assert_eq!(book.sheets.len(), 1, "{:?}", book.sheets);
        let one = &book.sheets[0];
        assert_eq!(one.name, "预算表");
        assert_eq!(one.state, "visible", "藏的是行与列，表本身看得见");
        assert_eq!(one.hidden_rows, vec![2, 3], "第 3、4 行整行隐藏");
        assert_eq!(
            one.hidden_cols,
            vec![2, 3, 4],
            "C/D/E 是一段范围，展开成三列"
        );
        assert_eq!(book.cells.len(), 13, "藏起来的格子还是格子");
        // 反面对照：同一批生产者写的另一份件没藏行列，不能凭空报出来
        let (clean_bytes, clean_cfb) = open("book.xls");
        let clean = read(&clean_cfb, &clean_bytes).expect("读得出");
        let reported: Vec<(String, Vec<u64>, Vec<u64>)> = clean
            .sheets
            .iter()
            .map(|had| {
                (
                    had.name.clone(),
                    had.hidden_rows.clone(),
                    had.hidden_cols.clone(),
                )
            })
            .filter(|(_, rows, cols)| !rows.is_empty() || !cols.is_empty())
            .collect();
        assert!(reported.is_empty(), "{reported:?}");
    }
}
