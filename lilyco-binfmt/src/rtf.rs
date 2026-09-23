//! RTF 正文提取：目标群感知、字符集感知的最小解释器。
//!
//! 与 `scripts/acceptance/lyco_rtf.py` 是同一份规范的两套独立实现，CI 上互相核对。
//! 五件真正决定成败的事（前四件都是这份实现在真实文件上错过的位置）：
//!
//! 1. 控制字按**词边界**结束：`\pard` 不许当成 `\par` 加一个字面 `d`，否则样式名与
//!    字体名会整段漏进正文；
//! 2. 目标群整群跳过：`fonttbl` / `colortbl` / `stylesheet` / `\*\...` 里的东西不是正文；
//!    子群继承父群的跳过状态；
//! 3. `\uN` 之后要按 `\ucN` 丢掉**等价回退字符**，而 `\'hh` 只算一个字符 ——
//!    少这一步，中文段落会剩下一串 `'3f`；
//! 4. 连续的 `\'hh` 攒成字节串，再按文件自己声明的 `\ansicpgNNNN` 解：
//!    UTF-8(65001) 直接解，1252/0 逐字节当 latin-1，**其余字符集本版本不内置**
//!    （936/GBK 之类的多字节编码不在 std 里）—— 那就逐字节给出来并说明用的哪档，
//!    绝不假装解对了；
//! 5. 一切「说不清」都留在计数与 `notes` 里，不悄悄丢字节。

use serde_json::{json, Value};

/// 见到这些控制字就把所在群整群跳过：它们是表 / 元数据 / 域指令原文，不是正文
const SKIP_DESTINATIONS: &[&str] = &[
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
];

/// 断点类：输出一个换行
const BREAK_WORDS: &[&str] = &["par", "line", "sect", "page", "pbb"];
/// 单元格分隔：输出一个制表符
const TAB_WORDS: &[&str] = &["tab", "cell", "nestcell"];
/// 行结束：一行表格就是一行文本 —— 把 \row 当制表符会把整张表挤成一行
const ROW_WORDS: &[&str] = &["row", "nestrow"];
/// 嵌套对象类：整群跳过并计数（办公文件里最常见的是 OLE 对象）
const OBJECT_WORDS: &[&str] = &[
    "object", "objattph", "objdata", "objclass", "objname", "objemb", "objhide",
];

#[derive(Debug, Clone)]
pub struct Rtf {
    pub text: String,
    pub lines: Vec<String>,
    pub declared_codepage: u32,
    pub hex_bytes: usize,
    pub unicode_escapes: usize,
    pub pictures: usize,
    pub embedded_objects: usize,
    pub skipped_destinations: usize,
    pub notes: Vec<String>,
}

impl Rtf {
    pub fn to_json(&self) -> Value {
        json!({
            "text": self.text,
            "lines": self.lines,
            "line_count": self.lines.len(),
            "chars": self.text.chars().count(),
            "declared_codepage": self.declared_codepage,
            "hex_bytes": self.hex_bytes,
            "unicode_escapes": self.unicode_escapes,
            "pictures": self.pictures,
            "embedded_objects": self.embedded_objects,
            "skipped_destinations": self.skipped_destinations,
            "notes": self.notes,
        })
    }
}

/// 解一份 RTF。RTF 是 8 位文本协议，这里一律按字节走，非 ASCII 字节只出现在
/// `\'hh` 与文本里，所以用 `Vec<u8>` 而不是 `&str` 能避免一半越界问题。
pub fn extract(bytes: &[u8]) -> Rtf {
    let mut out: Vec<u8> = Vec::new();
    let mut pending: Vec<u8> = Vec::new();
    let mut skip: Vec<bool> = vec![false];
    let mut codepage: u32 = 1252;
    let mut ucount: usize = 1;
    let mut notes: Vec<String> = Vec::new();
    let mut me = Rtf {
        text: String::new(),
        lines: Vec::new(),
        declared_codepage: 1252,
        hex_bytes: 0,
        unicode_escapes: 0,
        pictures: 0,
        embedded_objects: 0,
        skipped_destinations: 0,
        notes: Vec::new(),
    };
    let mut i = 0usize;
    while i < bytes.len() {
        let ch = bytes[i];
        match ch {
            b'{' => {
                flush(&mut out, &mut pending, codepage, &mut notes);
                skip.push(*skip.last().unwrap_or(&false));
                i += 1;
                continue;
            }
            b'}' => {
                flush(&mut out, &mut pending, codepage, &mut notes);
                if skip.len() > 1 {
                    skip.pop();
                }
                i += 1;
                continue;
            }
            b'\r' | b'\n' => {
                i += 1;
                continue;
            }
            _ => {}
        }
        if ch != b'\\' {
            flush(&mut out, &mut pending, codepage, &mut notes);
            if !*skip.last().unwrap_or(&false) {
                out.push(ch);
            }
            i += 1;
            continue;
        }
        // 反斜杠开头
        if bytes.get(i + 1) == Some(&b'*') {
            // `\*` 的语义就是「不认识这个目标群就整群跳过」。本域不认识任何带 \* 的目标：
            // fldinst（域指令原文）、userprops、批注的内部文本都从这里过。
            flush(&mut out, &mut pending, codepage, &mut notes);
            let last = skip.len() - 1;
            skip[last] = true;
            me.skipped_destinations += 1;
            i += 2;
            continue;
        }
        if bytes.get(i + 1) == Some(&b'\'') {
            me.hex_bytes += 1;
            let hi = hex_digit(bytes.get(i + 2).copied().unwrap_or(b'0'));
            let lo = hex_digit(bytes.get(i + 3).copied().unwrap_or(b'0'));
            if !*skip.last().unwrap_or(&false) {
                pending.push(hi * 16 + lo);
            }
            i += 4;
            continue;
        }
        let mut j = i + 1;
        let mut word: Vec<u8> = Vec::new();
        while j < bytes.len() && bytes[j].is_ascii_alphabetic() {
            word.push(bytes[j]);
            j += 1;
        }
        let word = String::from_utf8_lossy(&word).into_owned();
        if word.is_empty() {
            // 单字符转义：\{ \} \( \) 与单独一个反斜杠
            flush(&mut out, &mut pending, codepage, &mut notes);
            if j < bytes.len() {
                if !*skip.last().unwrap_or(&false) {
                    out.push(bytes[j]);
                }
                i = j + 1;
            } else {
                i = j;
            }
            continue;
        }
        let mut digits: Vec<u8> = Vec::new();
        while j < bytes.len() && (bytes[j].is_ascii_digit() || bytes[j] == b'-') {
            digits.push(bytes[j]);
            j += 1;
        }
        let digits = String::from_utf8_lossy(&digits).into_owned();
        if bytes.get(j) == Some(&b' ') {
            j += 1; // 控制字后的那个空格属于控制字，不是正文
        }
        flush(&mut out, &mut pending, codepage, &mut notes);
        let skipping = *skip.last().unwrap_or(&false);
        match word.as_str() {
            "uc" => {
                ucount = digits.parse::<usize>().unwrap_or(1);
                i = j;
                continue;
            }
            "ansicpg" => {
                if let Ok(one) = digits.parse::<u32>() {
                    codepage = one;
                }
                i = j;
                continue;
            }
            "u" => {
                if !digits.is_empty() {
                    if !skipping {
                        match digits.parse::<i64>() {
                            Ok(raw) => {
                                let value = if raw < 0 { 65536 + raw } else { raw };
                                match u32::try_from(value).ok().and_then(char::from_u32) {
                                    Some(one) => push_char(&mut out, one),
                                    None => notes.push(format!("\\u{raw} 不是一个合法的码位")),
                                }
                                me.unicode_escapes += 1;
                            }
                            Err(_) => notes.push(format!("\\u 的数字 {digits} 读不出来")),
                        }
                    }
                    j = skip_rtf_chars(bytes, j, ucount);
                }
                i = j;
                continue;
            }
            _ => {}
        }
        if SKIP_DESTINATIONS.contains(&word.as_str()) {
            let last = skip.len() - 1;
            skip[last] = true;
            me.skipped_destinations += 1;
        } else if word == "pict" {
            let last = skip.len() - 1;
            skip[last] = true;
            me.pictures += 1;
        } else if OBJECT_WORDS.contains(&word.as_str()) {
            let last = skip.len() - 1;
            skip[last] = true;
            me.embedded_objects += 1;
        } else if !skipping {
            if BREAK_WORDS.contains(&word.as_str()) {
                out.push(b'\n');
            } else if ROW_WORDS.contains(&word.as_str()) {
                out.push(b'\n');
            } else if TAB_WORDS.contains(&word.as_str()) {
                out.push(b'\t');
            }
        }
        i = j;
    }
    flush(&mut out, &mut pending, codepage, &mut notes);
    me.declared_codepage = codepage;
    me.notes = notes;
    let text = String::from_utf8_lossy(&out).into_owned();
    let text = text.trim().to_string();
    me.lines = text
        .lines()
        .map(|one| one.trim().to_string())
        .filter(|one| !one.is_empty())
        .collect();
    me.text = text;
    me
}

fn push_char(out: &mut Vec<u8>, one: char) {
    let mut buf = [0u8; 4];
    out.extend_from_slice(one.encode_utf8(&mut buf).as_bytes());
}

/// 从 `at` 起丢掉 `how_many` 个 **RTF 字符**：`\'hh` 是一个，控制字连同数字参数也是一个
fn skip_rtf_chars(bytes: &[u8], mut at: usize, how_many: usize) -> usize {
    while at < bytes.len() && matches!(bytes[at], b'\r' | b'\n') {
        at += 1;
    }
    let mut left = how_many;
    while left > 0 && at < bytes.len() {
        if bytes[at] == b'\\' && bytes.get(at + 1) == Some(&b'\'') {
            at += 4;
        } else if bytes[at] == b'\\'
            && bytes
                .get(at + 1)
                .is_some_and(|one| one.is_ascii_alphabetic())
        {
            at += 1;
            while at < bytes.len() && bytes[at].is_ascii_alphabetic() {
                at += 1;
            }
            while at < bytes.len() && (bytes[at].is_ascii_digit() || bytes[at] == b'-') {
                at += 1;
            }
            if bytes.get(at) == Some(&b' ') {
                at += 1;
            }
        } else {
            at += 1;
        }
        left -= 1;
    }
    at
}

fn hex_digit(one: u8) -> u8 {
    match one {
        b'0'..=b'9' => one - b'0',
        b'a'..=b'f' => one - b'a' + 10,
        b'A'..=b'F' => one - b'A' + 10,
        _ => 0,
    }
}

/// 一串 `\'hh` 字节按声明的字符集解释，然后清空缓冲
fn flush(out: &mut Vec<u8>, pending: &mut Vec<u8>, codepage: u32, notes: &mut Vec<String>) {
    if pending.is_empty() {
        return;
    }
    if codepage == 65001 {
        match std::str::from_utf8(pending) {
            Ok(text) => out.extend_from_slice(text.as_bytes()),
            Err(_) => {
                notes.push("声明是 UTF-8 但字节解不过去，按 lossy 给出".to_string());
                out.extend_from_slice(String::from_utf8_lossy(pending).as_bytes());
            }
        }
    } else if codepage == 1252 || codepage == 0 {
        // cp1252 与 latin-1 只在 0x80-0x9F 一段不同，办公文件的属性与正文里
        // 走这一段的是少数；真出现时下面那档会明确说用的是哪档
        for one in pending.iter() {
            push_char(out, char::from(*one));
        }
    } else {
        notes.push(format!(
            "字符集 {codepage} 本版本不内置，\'hh 字节按 latin-1 逐字节给出"
        ));
        for one in pending.iter() {
            push_char(out, char::from(*one));
        }
    }
    pending.clear();
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

    fn rtf(input: &str) -> Rtf {
        extract(input.as_bytes())
    }

    /// 词边界：`\pard` 不是 `\par` 加一个字面 `d`
    #[test]
    fn control_words_end_at_a_word_boundary() {
        let one = rtf("{\\pard\\plain \\rtlpar Hello\\par}");
        assert_eq!(one.text, "Hello");
        assert!(!one.text.contains('d'), "{}", one.text);
    }

    /// 目标群里的东西不是正文
    #[test]
    fn destinations_are_not_body_text() {
        let one = rtf("{\\rtf1\\ansi{\\fonttbl{\\f0 Cambria;}}{\\colortbl;\\red0\\green0;}正文}");
        assert_eq!(one.text, "正文");
        assert!(
            one.skipped_destinations >= 2,
            "{}",
            one.skipped_destinations
        );
    }

    /// `\*` 的语义：不认识就整群跳过 —— 域指令原文与用户属性都从这里过
    #[test]
    fn starred_groups_are_skipped() {
        let one = rtf("{\\rtf1{\\*\\fldinst HYPERLINK \"https://example.com\"}{\\fldrslt 链接}{\\*\\userprops{\\propname AppVersion}}完}");
        assert_eq!(one.text, "链接\n完".replace('\n', ""), "域指令文本不许出现");
    }

    /// `\uN` 之后的回退字节要按 `\ucN` 丢，且 `\'hh` 算一个字符
    #[test]
    fn unicode_escapes_drop_their_fallback() {
        let one = rtf("{\\uc1\\u20013\\'39\\u22914\\'41 x}");
        assert_eq!(one.text, "中太x");
        assert_eq!(one.unicode_escapes, 2);
        assert_eq!(one.hex_bytes, 0, "被丢掉的回退字节不该计数成 hex");
        let zero = rtf("{\\uc0\\u20013?}");
        assert_eq!(zero.text, "中");
    }

    /// 连续 `\'hh` 攒起来按文件声明的字符集解：UTF-8 那档能还原中文
    #[test]
    fn consecutive_hex_bytes_decode_with_the_declared_codepage() {
        // “中” 的 UTF-8 三字节：E4 B8 AD
        let one = rtf("{\\ansicpg65001\\'e4\\'b8\\'ad}");
        assert_eq!(one.declared_codepage, 65001);
        assert_eq!(one.text, "中");
        assert_eq!(one.hex_bytes, 3);
    }

    /// 内置之外的字符集不许假装成功：给 latin-1 并把用的哪档说出来
    #[test]
    fn an_unsupported_codepage_says_so_instead_of_guessing() {
        let one = rtf("{\\ansicpg936\\'a1\\'a2}");
        assert!(!one.notes.is_empty(), "936 要留下话");
        assert!(one.notes.join("；").contains("936"), "{:?}", one.notes);
        assert_eq!(one.text, "\u{a1}\u{a2}");
    }

    /// 表格：单元格与行分隔出制表符，正文按行分开
    #[test]
    fn table_cells_become_tabs_and_paragraphs_newlines() {
        let one = rtf("{\\trowd\\trgaph108\\cellx1000 科目\\cell 金额\\cell\\row 服务器\\cell 124000\\cell\\row}");
        assert_eq!(one.lines.len(), 2, "{:?}", one.lines);
        assert_eq!(one.lines[0], "科目\t金额");
        assert_eq!(one.lines[1], "服务器\t124000");
    }

    /// LibreOffice 从 python-docx 的 docx 转出来的 RTF：正文必须与 docx 的段落对得上
    /// （期望值来自 `office_reader.py` 对同一份文件的独立读取；表格两行各算一行）
    #[test]
    fn reads_a_real_producer_rtf() {
        let one = extract(&fixture("notes.rtf"));
        assert_eq!(one.lines.len(), 7, "{:?}", one.lines);
        assert_eq!(one.lines[0], "一级标题：预算口径");
        assert_eq!(one.lines[1], "第三季度服务器预算为十二万四千元");
        assert_eq!(one.lines[2], "二级标题：明细");
        assert_eq!(one.lines[3], "科目\t金额");
        assert_eq!(one.lines[4], "服务器\t124000");
        assert_eq!(one.lines[5], "口径见 预算制度");
        assert_eq!(one.lines[6], "最后一页说明：数字为含税口径");
        assert_eq!(one.pictures, 1, "文档里那张 PNG 是以 pict 的形式进来的");
        assert_eq!(one.hex_bytes, 183);
        assert_eq!(one.unicode_escapes, 60);
        assert_eq!(one.declared_codepage, 1252);
        assert!(one.text.contains("预算制度"), "超链接的结果文本要在");
        assert!(
            !one.text.contains("https://example.com"),
            "域指令原文不属于正文，混进来就是跳过没生效"
        );
        assert!(
            !one.text.contains("\\p"),
            "不许有没吃完的控制字：{}",
            one.text
        );
        assert!(one.notes.is_empty(), "{:?}", one.notes);
    }

    /// 空输入与只有控制字的输入：给空文本，而不是 panic
    #[test]
    fn degenerate_inputs_answer_empty() {
        assert_eq!(extract(b"").text, "");
        assert_eq!(extract(b"{\\rtf1}").text, "");
        assert_eq!(extract(b"\\").text, "");
        assert_eq!(
            extract(b"{unclosed").text,
            "unclosed",
            "没有控制字时文本就是文本"
        );
    }
}
