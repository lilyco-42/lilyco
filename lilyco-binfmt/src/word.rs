//! MS-DOC：`.doc` 的正文位置。
//!
//! Word 不把正文顺着放在 `WordDocument` 流里，而是放一张 **piece 表**：
//!
//! ```text
//! WordDocument 流  FIB 头 → 第 10 个字节里的 bit9 说表流叫 1Table 还是 0Table
//!                          → fcClx / lcbClx（0x01A2 / 0x01A6）指出 Clx 在表流里的位置
//! 表流里的 Clx      一串 {0x01 Prc} 之后跟着 {0x02 Pcdt}
//! Pcdt 里的 PlcPcd  (n+1) 个 CP + n 个 8 字节的 Pcd，n = (len - 4) / 12
//! 每个 Pcd          自己的 fc：bit30 = 压缩标志
//!                   压缩 → 8 位字符，真实偏移是 fc/2，长度 = cp 差值
//!                   未压缩 → 16 位 UTF-16LE，偏移就是 fc
//! ```
//!
//! 那个「压缩」位是这一份格式最容易读错的地方：它不是「文件压过没有」，而是
//! **这一个 piece 的字符宽度**，所以同一篇文档里中英两段可以各走各的路 ——
//! 忘了把 fc 除以二，就会从文件中间开始取字节，得到一串看着像字的垃圾。
//!
//! 本模块刻意拆成两个纯函数（`piece_table` 与 `render`）：真实文件走
//! [`crate::cfb`] 拿到两段流之后调它们，而 8 位那条分支用合成字节直接测算术 ——
//! 手上没有 Word 可用，就不要拿一个手搓的容器冒充 Word 写出来的文件。

use serde_json::{json, Value};

use crate::cfb::Cfb;
use crate::read::{le16, le32};

/// 一个 piece：它管哪一段 CP、字节在哪、是不是 8 位字符
#[derive(Debug, Clone)]
pub struct Piece {
    pub cp_start: u32,
    pub chars: u32,
    pub compressed: bool,
    pub fc: u32,
}

/// 正文读取的结果
#[derive(Debug)]
pub struct WordBody {
    pub table_stream: String,
    pub fc_clx: u32,
    pub lcb_clx: u32,
    pub prcs_skipped: usize,
    pub pieces: Vec<Piece>,
    pub cp_total: u32,
    pub text: String,
    pub lines: Vec<String>,
    pub notes: Vec<String>,
}

impl WordBody {
    pub fn to_json(&self) -> Value {
        json!({
            "table_stream": self.table_stream,
            "fc_clx": self.fc_clx,
            "lcb_clx": self.lcb_clx,
            "prc_skipped": self.prcs_skipped,
            "piece_count": self.pieces.len(),
            "pieces": self.pieces.iter().map(|one| json!({
                "cp_start": one.cp_start,
                "chars": one.chars,
                "compressed": one.compressed,
                "fc": one.fc,
            })).collect::<Vec<Value>>(),
            "cp_total": self.cp_total,
            "lines": self.lines,
            "line_count": self.lines.len(),
            "chars": self.text.chars().count(),
            "notes": self.notes,
        })
    }
}

/// 从复合文档里读出 .doc 的正文
pub fn read(cfb: &Cfb, bytes: &[u8]) -> Result<WordBody, String> {
    let word = cfb
        .read(bytes, "WordDocument")
        .ok_or("容器里没有 WordDocument 流")?;
    let flags = le16(10)(&word).unwrap_or(0);
    let mut table = if flags & 0x0200 == 0x0200 {
        "1Table"
    } else {
        "0Table"
    };
    if cfb.find(table).is_none() {
        table = if cfb.find("1Table").is_some() {
            "1Table"
        } else {
            "0Table"
        };
    }
    let table_bytes = cfb
        .read(bytes, table)
        .ok_or("两条表流都不在容器里，piece 表无处可取")?;
    let fc_clx = usize::try_from(le32(0x01A2)(&word).ok_or("FIB 读不到 fcClx")?).unwrap_or(0);
    let lcb_clx = usize::try_from(le32(0x01A6)(&word).ok_or("FIB 读不到 lcbClx")?).unwrap_or(0);
    let clx = table_bytes
        .get(fc_clx..fc_clx.checked_add(lcb_clx).ok_or("lcbClx 溢出")?)
        .ok_or_else(|| {
            format!(
                "表流装不下 Clx：要 {lcb_clx} 字节从 {fc_clx} 开始，表流只有 {}",
                table_bytes.len()
            )
        })?;
    let (pieces, prcs_skipped) = piece_table(clx)?;
    let mut notes: Vec<String> = Vec::new();
    if prcs_skipped > 0 {
        notes.push(format!(
            "Clx 里跳过了 {prcs_skipped} 个 Prc（图形参数），与正文无关"
        ));
    }
    let text = render(&word, &pieces, &mut notes);
    let lines = clean(&text);
    let cp_total = pieces
        .last()
        .map(|one| one.cp_start + one.chars)
        .unwrap_or(0);
    Ok(WordBody {
        table_stream: table.to_string(),
        fc_clx: fc_clx as u32,
        lcb_clx: lcb_clx as u32,
        prcs_skipped,
        pieces,
        cp_total,
        text,
        lines,
        notes,
    })
}

/// 解析 Clx：跳过若干 Prc，取第一个 Pcdt 里的 PlcPcd
pub fn piece_table(clx: &[u8]) -> Result<(Vec<Piece>, usize), String> {
    let mut at = 0usize;
    let mut prcs = 0usize;
    while at < clx.len() {
        match clx.get(at) {
            Some(1) => {
                let cb = usize::try_from(le16(at + 1)(clx).unwrap_or(0)).unwrap_or(usize::MAX);
                let next = at.checked_add(3).and_then(|one| one.checked_add(cb));
                match next {
                    Some(one) => at = one,
                    None => return Err("Prc 的长度字段跑出 Clx 之外".to_string()),
                }
                prcs += 1;
            }
            Some(2) => {
                let lcb = usize::try_from(le32(at + 1)(clx).unwrap_or(0)).unwrap_or(usize::MAX);
                let plc = clx
                    .get(at + 5..at + 5usize.checked_add(lcb).ok_or("Pcdt 长度溢出")?)
                    .ok_or("Pcdt 说它比 Clx 剩下的部分还长")?;
                if plc.len() < 16 {
                    return Err(format!("Pcdt 只有 {} 字节，读不出 piece 表", plc.len()));
                }
                let n = (plc.len() - 4) / 12;
                let mut cps: Vec<u32> = Vec::with_capacity(n + 1);
                for k in 0..=n {
                    match le32(k * 4)(plc) {
                        Some(one) => cps.push(one as u32),
                        None => break,
                    }
                }
                if cps.len() != n + 1 {
                    return Err("CP 数组读不齐".to_string());
                }
                let mut out: Vec<Piece> = Vec::new();
                for k in 0..n {
                    let base = (n + 1) * 4 + k * 8;
                    let raw = plc.get(base..base + 8).ok_or("Pcd 数组读不齐")?;
                    let fc_raw = le32(2)(raw).unwrap_or(0) as u32;
                    out.push(Piece {
                        cp_start: cps[k],
                        chars: cps[k + 1].saturating_sub(cps[k]),
                        compressed: fc_raw & 0x4000_0000 != 0,
                        fc: fc_raw & 0x3FFF_FFFF,
                    });
                }
                return Ok((out, prcs));
            }
            Some(other) => return Err(format!("Clx 里出现不认识的标记 {other}（位置 {at}）")),
            None => break,
        }
    }
    Err("Clx 里没有 Pcdt：这份 .doc 没有正文 piece 表".to_string())
}

/// 按 piece 表把字节取成文本（两个分支的取法完全不同，见模块注释）
pub fn render(word: &[u8], pieces: &[Piece], notes: &mut Vec<String>) -> String {
    let mut out = String::new();
    for one in pieces {
        let count = usize::try_from(one.chars).unwrap_or(usize::MAX);
        if one.compressed {
            let start = match usize::try_from(one.fc / 2) {
                Ok(one) => one,
                Err(_) => continue,
            };
            match word.get(start..start.saturating_add(count)) {
                Some(raw) => out.push_str(&decode_cp1252(raw)),
                None => notes.push(format!(
                    "压缩 piece（fc={} 的一半）跑到 WordDocument 流之外，只读到 {} 字节",
                    one.fc,
                    word.len().saturating_sub(start)
                )),
            }
        } else {
            let start = match usize::try_from(one.fc) {
                Ok(one) => one,
                Err(_) => continue,
            };
            let need = match count.checked_mul(2) {
                Some(one) => one,
                None => continue,
            };
            match word.get(start..start + need) {
                Some(raw) => {
                    let mut units: Vec<u16> = Vec::with_capacity(raw.len() / 2);
                    for pair in raw.chunks(2) {
                        if pair.len() == 2 {
                            units.push(u16::from_le_bytes([pair[0], pair[1]]));
                        }
                    }
                    out.push_str(&String::from_utf16_lossy(&units));
                }
                None => notes.push(format!(
                    "piece（fc={}，{count} 个字符）需要的字节超出了 WordDocument 流",
                    one.fc
                )),
            }
        }
    }
    out
}

/// Word 的结构字符不当正文：段落结束(0D)→换行，单元格/行(07)→制表符，
/// 域代码只留结果部分，锚点/对象/软换行/分页这些直接丢掉
fn clean(body: &str) -> Vec<String> {
    let mut out = String::new();
    let mut in_field = false;
    let mut keep = false;
    for ch in body.chars() {
        match ch as u32 {
            0x13 => {
                in_field = true;
                keep = false;
                continue;
            }
            0x14 => {
                keep = true;
                continue;
            }
            0x15 => {
                in_field = false;
                keep = false;
                continue;
            }
            _ => {}
        }
        if in_field && !keep {
            continue;
        }
        match ch as u32 {
            0x0D => out.push('\n'),
            0x07 => out.push('\t'),
            0x01 | 0x02 | 0x05 | 0x08 | 0x0A | 0x0B | 0x0C | 0x0E | 0x0F | 0x16 | 0x17 | 0x18 => {}
            _ => out.push(ch),
        }
    }
    out.lines()
        .map(|one| one.trim().to_string())
        .filter(|one| !one.is_empty())
        .collect()
}

/// cp1252：与 latin-1 只在 0x80-0x9F 这 21 个位置不同，而那几位正好是
/// 「弯引号、省略号、商标符号」——按 latin-1 解会掉进 C1 控制区，看起来像坏数据
pub(crate) fn decode_cp1252(raw: &[u8]) -> String {
    const HIGH: [char; 32] = [
        '\u{20AC}', '\u{FFFD}', '\u{201A}', '\u{0192}', '\u{201E}', '\u{2026}', '\u{2020}',
        '\u{2021}', '\u{02C6}', '\u{2030}', '\u{0160}', '\u{2039}', '\u{0152}', '\u{FFFD}',
        '\u{017D}', '\u{FFFD}', '\u{FFFD}', '\u{2019}', '\u{2018}', '\u{201C}', '\u{201D}',
        '\u{2022}', '\u{2013}', '\u{2014}', '\u{02DC}', '\u{2122}', '\u{0161}', '\u{203A}',
        '\u{0153}', '\u{FFFD}', '\u{017E}', '\u{0178}',
    ];
    let mut out = String::with_capacity(raw.len());
    for one in raw {
        match one {
            0x80..=0x9F => out.push(HIGH[usize::from(*one) - 0x80]),
            _ => out.push(char::from(*one)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(name: &str) -> Vec<u8> {
        std::fs::read(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/office")
                .join(name),
        )
        .expect("读 fixture")
    }

    fn body_of(name: &str) -> WordBody {
        let bytes = fixture(name);
        let cfb = crate::cfb::open(&bytes).expect("打开复合文档");
        read(&cfb, &bytes).expect("读出正文")
    }

    /// LibreOffice 写的中文 .doc：一段 piece 表、UTF-16 分支、六行正文
    /// （期望值来自 `lyco_legacy.py` 对同一份文件的独立读取）
    #[test]
    fn reads_a_real_producer_document() {
        let one = body_of("notes.doc");
        assert_eq!(one.table_stream, "1Table");
        assert_eq!(one.pieces.len(), 1);
        assert_eq!(one.cp_total, 139);
        assert_eq!(one.prcs_skipped, 0);
        assert!(!one.pieces[0].compressed, "LO 这份走的是 16 位分支");
        assert_eq!(one.lines.len(), 6, "{:?}", one.lines);
        assert_eq!(one.lines[0], "一级标题：预算口径");
        assert_eq!(one.lines[1], "第三季度服务器预算为十二万四千元");
        assert_eq!(one.lines[2], "二级标题：明细");
        assert_eq!(
            one.lines[3],
            "科目\t金额\t\t服务器\t124000\t\t口径见 预算制度"
        );
        assert_eq!(one.lines[4], "最后一页说明：数字为含税口径");
        // 批注的正文在 Word 里也是文档正文的一部分（它排在主文之后）
        assert_eq!(one.lines[5], "这里要补上不含税口径");
        assert!(one.notes.is_empty(), "{:?}", one.notes);
    }

    /// 纯 ASCII 那份同样要读对（同一个分支，不同内容）
    #[test]
    fn reads_the_english_document_too() {
        let one = body_of("notes-en.doc");
        assert_eq!(
            one.lines,
            vec![
                "Quarterly budget note",
                "The server budget for Q3 is 124,000 yuan.",
                "Second line: numbers are tax-inclusive."
            ]
        );
    }

    /// 8 位分支没有真实的 Word 样本可拿（LibreOffice 中英都写 16 位），
    /// 所以这里用**合成**字节证明这条路的算术：fc 要除二、按 cp1252 解、
    /// 以及 0x93 这种位置不能当控制字符。合成样本不冒充 Word 的文件。
    #[test]
    fn the_compressed_branch_halves_fc_and_decodes_cp1252() {
        // 造一个只含 Pcdt 的 Clx：两个 CP + 一个 Pcd
        let mut clx: Vec<u8> = vec![0x02];
        let mut plc: Vec<u8> = Vec::new();
        plc.extend_from_slice(&0u32.to_le_bytes());
        plc.extend_from_slice(&4u32.to_le_bytes());
        // Pcd：flags(2) + fc(4) + prm(2)；bit30 置起来 = 压缩
        let mut pcd: Vec<u8> = vec![0x00, 0x00];
        pcd.extend_from_slice(&(0x4000_0020u32).to_le_bytes()); // fc = 0x40000020 → 真实偏移 0x20/2 = 16
        pcd.extend_from_slice(&[0x00, 0x00]);
        let body_len = 4 + 4 + 8;
        clx.extend_from_slice(&(body_len as u32).to_le_bytes());
        clx.extend_from_slice(&plc);
        clx.extend_from_slice(&pcd);
        let (pieces, prcs) = piece_table(&clx).expect("读得出 piece 表");
        assert_eq!(prcs, 0);
        assert_eq!(pieces.len(), 1);
        assert!(pieces[0].compressed);
        assert_eq!(pieces[0].fc, 0x0020);
        assert_eq!(pieces[0].chars, 4);
        let mut word = vec![0u8; 32];
        word[16..20].copy_from_slice(b"Q3's");
        let mut notes: Vec<String> = Vec::new();
        let text = render(&word, &pieces, &mut notes);
        assert_eq!(text, "Q3's");
        assert!(notes.is_empty(), "{notes:?}");
        // cp1252 的 0x93/0x94 是弯引号，不是 C1 控制字符
        let mut word = vec![0u8; 32];
        word[16..20].copy_from_slice(&[b'a', 0x93, 0x94, b'z']);
        let text = render(&word, &pieces, &mut notes);
        assert_eq!(text, "a\u{201C}\u{201D}z", "{text}");
    }

    /// Clx 里先有 Prc 再有 Pcdt：Prc 要按它自己声明的长度跳干净
    #[test]
    fn prcs_are_skipped_by_their_own_length() {
        let mut clx: Vec<u8> = vec![0x01];
        clx.extend_from_slice(&3u16.to_le_bytes());
        clx.extend_from_slice(&[9u8, 9, 9]); // Prc 正文 3 字节
        let mut plc: Vec<u8> = Vec::new();
        plc.extend_from_slice(&0u32.to_le_bytes());
        plc.extend_from_slice(&1u32.to_le_bytes());
        plc.extend_from_slice(&[0x00, 0x00]);
        plc.extend_from_slice(&0x4000_0000u32.to_le_bytes());
        plc.extend_from_slice(&[0x00, 0x00]);
        clx.push(0x02);
        clx.extend_from_slice(&(plc.len() as u32).to_le_bytes());
        clx.extend_from_slice(&plc);
        let (pieces, prcs) = piece_table(&clx).expect("跳完 Prc 还能找到 Pcdt");
        assert_eq!(prcs, 1);
        assert_eq!(pieces.len(), 1);
        assert_eq!(pieces[0].fc, 0);
    }

    /// 表流名字由 FIB 的 bit9 决定；只有一条在时要退回另一条，两条都不在要报错
    #[test]
    fn missing_streams_are_reported_not_guessed() {
        let bytes = fixture("book.xls");
        let cfb = crate::cfb::open(&bytes).expect("打开复合文档");
        let why = read(&cfb, &bytes).unwrap_err();
        assert!(why.contains("WordDocument"), "{why}");
    }

    /// 截断的表流：宁可报错，也不要从不存在的位置取字节
    #[test]
    fn a_clx_that_does_not_fit_says_so() {
        let why = piece_table(&[0x02, 0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x00]).unwrap_err();
        assert!(why.contains("Pcdt") || why.contains("Clx"), "{why}");
        let why = piece_table(&[0x07, 0x00]).unwrap_err();
        assert!(why.contains("不认识的标记"), "{why}");
    }

    /// 域代码只留结果：`HYPERLINK "url"` 是指令，链接文字才是正文
    #[test]
    fn field_instructions_are_dropped_and_results_kept() {
        let raw = "口径见 \u{13}HYPERLINK \"https://example.com\"\u{14}预算制度\u{15}\u{7}";
        let lines = clean(raw);
        assert_eq!(lines, vec!["口径见 预算制度"], "{lines:?}");
        assert!(!lines[0].contains("example.com"));
    }
}
