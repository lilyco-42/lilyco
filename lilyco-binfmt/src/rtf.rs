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

/// 这两群仍然整群跳过（里面的一个字都不是页面上的字），但本域**认得**它们，
/// 所以跳过之前先前瞻读一遍里面的定义 —— 与 `\*` 那条同一个道理：
/// 「不认识才跳」不等于「认识了就不许跳」，只是认识了就别连里面的名字一起丢
const DEF_DESTINATIONS: &[&str] = &["fonttbl", "stylesheet"];

/// 断点类：输出一个换行
const BREAK_WORDS: &[&str] = &["par", "line", "sect", "page", "pbb"];
/// 页眉与页脚的目标群：字是真的，但它们不是正文。
/// 注意 `\headery` / `\footery` 是「页眉高度」这种**格式**控制字，
/// 控制字读到字母为止，所以整名匹配不会把它们误当成目标
const PAGE_DESTINATIONS: &[&str] = &[
    "header", "headerl", "headerr", "headert", "headerf", "footer", "footerl", "footerr",
    "footert", "footerf",
];

/// 目标属于页眉还是页脚
fn page_kind(word: &str) -> &'static str {
    if word.starts_with("header") {
        "header"
    } else {
        "footer"
    }
}

/// 从 `at` 起找到关闭**当前这一群**的那个 `}`：交回它的位置与群里的字节。
/// `\{` 与 `\}` 是字面花括号，不参与配对；找不到就走到尾（坏文件不咬人）
fn group_end(bytes: &[u8], at: usize) -> (usize, Vec<u8>) {
    let mut depth = 0i32;
    let mut i = at;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => {
                if depth == 0 {
                    return (i, bytes[at..i].to_vec());
                }
                depth -= 1;
            }
            b'\\' => {
                if matches!(bytes.get(i + 1), Some(&b'{') | Some(&b'}')) {
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    (bytes.len(), bytes[at.min(bytes.len())..].to_vec())
}

/// 链接：`{\field{\*\fldinst HYPERLINK "地址" }{\fldrslt {显示文字}}}`。
/// 这两个前缀是文件里字面写的样子（一个反斜杠 + 一个星号 + 群名）
const FLDINST_HEAD: &[u8] = b"{\\*\\fldinst";
const FLDRSLT_HEAD: &[u8] = b"{\\fldrslt";

/// 从一群 `\field …` 里读出一条链接：域指令里的地址 + `\fldrslt` 的显示文字。
/// 读不出 HYPERLINK 就交回 None（页码、日期那些域不是链接）
fn field_link(group: &[u8]) -> Option<Value> {
    let at = windows_position(group, 0, FLDINST_HEAD)?;
    let (_stop, instruction) = group_end(group, at + FLDINST_HEAD.len());
    let target = hyperlink_target(&instruction)?;
    let mut text = String::new();
    if let Some(nxt) = windows_position(group, at, FLDRSLT_HEAD) {
        let (_stop2, result) = group_end(group, nxt + FLDRSLT_HEAD.len());
        text = extract(&result).text;
    }
    Some(json!({ "target": target, "text": text }))
}

/// `HYPERLINK "地址"` 里那段引号包住的地址。指令原文的大小写各家不同，这里按
/// ASCII 大小写无关找字面量；地址本身照文件写的字节交回
fn hyperlink_target(inst: &[u8]) -> Option<String> {
    const WORD: &[u8] = b"HYPERLINK";
    let low: Vec<u8> = inst.iter().map(|one| one.to_ascii_lowercase()).collect();
    let at = windows_position(&low, 0, &WORD.to_ascii_lowercase())?;
    let mut k = at + WORD.len();
    while matches!(
        inst.get(k),
        Some(b' ') | Some(b'\t') | Some(b'\r') | Some(b'\n')
    ) {
        k += 1;
    }
    if inst.get(k) != Some(&b'"') {
        return None;
    }
    k += 1;
    let start = k;
    while k < inst.len() && inst[k] != b'"' {
        k += 1;
    }
    Some(String::from_utf8_lossy(&inst[start..k]).into_owned())
}

/// 一群之内的**顶层**子群：交回每个子群的内容（不含外面那对花括号）
fn child_groups(text: &[u8]) -> Vec<Vec<u8>> {
    let mut out: Vec<Vec<u8>> = Vec::new();
    let mut i = 0usize;
    while i < text.len() {
        if text[i] != b'{' {
            i += 1;
            continue;
        }
        let (stop, inner) = group_end(text, i + 1);
        out.push(inner);
        i = stop + 1;
    }
    out
}

/// 一条 `{\f0 … Times New Roman;}` / `{\s1 … heading 1;}` / `{\*\cs15 … Name;}`：
/// 种类、编号、字符集与名字。名字用本模块自己解（控制字不产字，
/// 嵌套的 `{\*\falt …}` 那一群按「不认识就跳」跳掉），末尾那个分号去掉。
fn definition_of(child: &[u8]) -> Option<Value> {
    const CHARSET_HEAD: &[u8] = b"\\fcharset";
    let rest = if let Some(tail) = child.strip_prefix(b"\\*\\") {
        tail
    } else if let Some(tail) = child.strip_prefix(b"\\") {
        tail
    } else {
        return None;
    };
    let (prefix, kind) = if rest.starts_with(b"cs") {
        (2usize, "character")
    } else if rest.starts_with(b"s") {
        (1usize, "paragraph")
    } else if rest.starts_with(b"f") {
        (1usize, "font")
    } else {
        return None;
    };
    let index = digits_after(rest, prefix)?;
    let mut charset = 0u64;
    if let Some(at) = windows_position(child, 0, CHARSET_HEAD) {
        charset = digits_after(child, at + CHARSET_HEAD.len()).unwrap_or(0);
    }
    let name = extract(child)
        .text
        .trim()
        .trim_end_matches(';')
        .trim_end()
        .to_string();
    // 全 ASCII 的名字与字符集无关（RTF 用的每一种字符集都含 ASCII）；
    // 否则只有 ANSI(0) 那份能按 cp1252 读。非 ANSI 又含非 ASCII 的名字交回 null ——
    // 按 cp1252 硬解会得出「‚l‚r ƒSƒVƒbƒN」这种串（文件写的是 Shift-JIS），
    // 那不是名字，是我们解错了
    let ascii_name = name.chars().all(|one| (one as u32) < 0x80);
    let shown = if name.is_empty() || (kind == "font" && charset != 0 && !ascii_name) {
        Value::Null
    } else {
        json!(name)
    };
    let kind_of = if kind == "font" {
        json!(charset)
    } else {
        Value::Null
    };
    Some(json!({"kind": kind, "index": index, "charset": kind_of, "name": shown}))
}

/// 从 `from` 起那一串数字（没有数字就交回 None）
fn digits_after(bytes: &[u8], from: usize) -> Option<u64> {
    let mut num: Vec<u8> = Vec::new();
    let mut k = from;
    while k < bytes.len() && bytes[k].is_ascii_digit() {
        num.push(bytes[k]);
        k += 1;
    }
    if num.is_empty() {
        return None;
    }
    Some(String::from_utf8_lossy(&num).parse().unwrap_or(0))
}

/// 样式表里那个号的名字写成 `heading N` 时，N 就是段落的层级。
/// 形状之外什么都不认：`heading` 与数字之间至少一个空格、数字后面不能再有别的东西
/// （`Heading1`、`heading 1a` 都不算 —— 那是文件里另一个名字，不是我们的推断）。
/// 大小写各家不同（Word 写 `Heading 1`，LibreOffice 的 RTF 导出写 `heading 1`），
/// 所以只有这个词不分大小写。只查段落样式：`\sN` 与 `\csN` 是两个各自的编号空间
fn heading_level(styles: &[Value], index: u64) -> Option<usize> {
    let named = styles
        .iter()
        .find(|one| one["kind"] == json!("paragraph") && one["index"] == json!(index))?;
    let name = named["name"].as_str()?.trim();
    let bytes = name.as_bytes();
    if bytes.len() < 7 || !bytes[..7].eq_ignore_ascii_case(b"heading") {
        return None;
    }
    let after = &name[7..];
    if !after
        .as_bytes()
        .first()
        .is_some_and(|one| one.is_ascii_whitespace())
    {
        return None;
    }
    let rest = after.trim_start();
    let end = rest
        .find(|one: char| !one.is_ascii_digit())
        .unwrap_or(rest.len());
    if end == 0 || !rest[end..].trim().is_empty() {
        return None;
    }
    rest[..end].parse().ok()
}

/// 单元格分隔：输出一个制表符
const TAB_WORDS: &[&str] = &["tab", "cell", "nestcell"];
/// 行结束：一行表格就是一行文本 —— 把 \row 当制表符会把整张表挤成一行
const ROW_WORDS: &[&str] = &["row", "nestrow"];
/// 嵌套对象类：整群跳过并计数（办公文件里最常见的是 OLE 对象）
const OBJECT_WORDS: &[&str] = &[
    "object", "objattph", "objdata", "objclass", "objname", "objemb", "objhide",
];

/// 文档级「那张纸」写在哪几个控制字上（单位是 twips，1/1440 英寸）。
/// `landscape` 是个旗标（写了就是横的），其余都带一个数字参数
const PAPER_WORDS: &[&str] = &[
    "paperw",
    "paperh",
    "margl",
    "margr",
    "margt",
    "margb",
    "landscape",
];
/// 注的两个口袋。**LibreOffice 的 RTF 导出只用 `footnote` 这一个**，
/// 尾注靠群里的 `\ftnalt` 反标志区分（Word 那族还会另写 `endnote` 口袋），
/// 所以两个词都认、再看那个标志
const NOTE_DESTINATIONS: &[&str] = &["footnote", "endnote"];

/// 注的**排版定义**（分隔符、续分符、编号占位）不是一条注：
/// 这份件里就明写着 `{\\*\\ftnsep\\chftnsep}` —— 当成注就会凭空多出几条空注
const NOTE_DEFINITION_WORDS: &[&str] = &[
    "ftnsep", "ftnsepc", "ftncn", "aftnsep", "aftnsepc", "aftncn",
];

/// `\*` 之后紧跟的那个控制字叫什么（可能隔着空白与另一个反斜杠）
fn peek_word(bytes: &[u8], at: usize) -> String {
    let mut k = at;
    while k < bytes.len()
        && (bytes[k] == b' ' || bytes[k] == b'\r' || bytes[k] == b'\n' || bytes[k] == b'\\')
    {
        k += 1;
    }
    let mut word: Vec<u8> = Vec::new();
    while k < bytes.len() && bytes[k].is_ascii_alphabetic() {
        word.push(bytes[k]);
        k += 1;
    }
    String::from_utf8_lossy(&word).into_owned()
}

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
    /// 页眉与页脚的字：它们与正文混在同一个流里，靠目标群分开。
    /// 一条一节会同时写进 `\header`、`\headerl`、`\headert` 好几个口袋，
    /// 所以每条都带着自己是哪个口袋（slot），不替文件合并
    pub headers: Vec<Value>,
    pub footers: Vec<Value>,
    pub page_destinations: usize,
    /// 脚注与尾注：每条带 `kind`（footnote / endnote）、`slot`（文件用的哪个口袋名）与字。
    /// 注的字**不混进正文** —— 它住在正文流里的一个目标群里，位置就在引用点后面
    pub note_list: Vec<Value>,
    pub note_destinations: usize,
    /// 链接：`{\field{\*\fldinst HYPERLINK "地址" }{\fldrslt {显示文字}}}` 那一群读出来的。
    /// 域指令那一群仍然是跳过的（它不是页面上的字），这里只是**前瞻**读它一眼，
    /// 不推进游标 —— 显示文字照旧留在正文里
    pub links: Vec<Value>,
    /// `\field` 出现了几次（一份文档里域比链接多：页码、日期都是域）
    pub fields: usize,
    /// 字体表与样式表里的定义（前瞻读出来的，那一群照旧不进正文）。
    /// 每条是 `{kind, index, charset, name}`；字体条目自己声明了非 ANSI 字符集
    /// （`\fcharset128` 是 Shift-JIS）而名字里又有非 ASCII 字节时交回 `name: null` ——
    /// 按 cp1252 硬解出来的那串不是名字，是我们解错了
    pub fonts: Vec<Value>,
    pub styles: Vec<Value>,
    /// 正文里用了哪些样式号、各几次（样式表那一群自己不算）
    pub style_uses: Vec<Value>,
    /// 标题：样式名写成 `heading N` 的那些段，层级就写在名字里。
    /// 段用的是哪个样式号在段属性里（`\pard\s1`），名字在样式表里，两边一接才有层级
    pub headings: Vec<Value>,
    /// 文档级那张纸的**原样**：`\paperw12240` 这类控制字，每个词只留没被跳过的那一层里
    /// 第一次写的那一个（后面 `{\*\sectx …}` 里的那些是某一节的覆写，而这一族不判分节归属）。
    /// 换算在 `crate::paper`（三家同一条式子），这里不预先换成毫米
    pub paper_writes: Vec<(String, String)>,
    /// 表那份账。这六个数都是**控制字本身的条数**（`\trowd` / `\row` / `\cell` / `\intbl`
    /// 与嵌套表那两个），不是「有几张表」的推断 —— 那条规则拿两份件试过：
    /// 一张 2×2 的对，两张（3×2 与 2×2）的把两张数成一张，所以这里只交数得清的
    pub table_row_defines: usize,
    pub table_rows: usize,
    pub table_cells: usize,
    pub table_cell_paras: usize,
    pub nested_table_rows: usize,
    pub nested_table_cells: usize,
    /// 这一群里出现过 `\ftnalt`：LibreOffice 用它把 `footnote` 口袋标成尾注。
    /// 只在提取子群时用来判 kind，不单独交出去
    pub ftnalt: bool,
    pub notes: Vec<String>,
}

impl Rtf {
    pub fn to_json(&self) -> Value {
        json!({
            "text": self.text,
            "lines": self.lines,
            "headers": self.headers,
            "footers": self.footers,
            "page_destinations": self.page_destinations,
            "note_list": self.note_list,
            "note_destinations": self.note_destinations,
            "links": self.links,
            "fields": self.fields,
            "fonts": self.fonts,
            "styles": self.styles,
            "style_uses": self.style_uses,
            "headings": self.headings,
            "paper_writes": self.paper_writes,
            "line_count": self.lines.len(),
            "chars": self.text.chars().count(),
            "declared_codepage": self.declared_codepage,
            "hex_bytes": self.hex_bytes,
            "unicode_escapes": self.unicode_escapes,
            "pictures": self.pictures,
            "embedded_objects": self.embedded_objects,
            "skipped_destinations": self.skipped_destinations,
            "table_row_defines": self.table_row_defines,
            "table_rows": self.table_rows,
            "table_cells": self.table_cells,
            "table_cell_paras": self.table_cell_paras,
            "nested_table_rows": self.nested_table_rows,
            "nested_table_cells": self.nested_table_cells,
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
    // 正文里用到的样式号（样式表那一群自己不算，跳过的区域里也不算）
    let mut uses: Vec<u64> = Vec::new();
    // 段那一份账：每段收尾时记下「这一段的字从哪儿到哪儿」与「这一段用的是哪个样式号」。
    // 收尾点就是段控制字（`\par` 那几个）与行控制字（`\row`）；两端都记下，是因为切片
    // 只写起点会把后面整篇字都当成这一段的内容（那个 bug 由 CI 抓出来：标题变成整份文档）。
    // 这一段与 `lines` 是两本账：这里空段也留
    let mut marks: Vec<(usize, usize, Option<u64>)> = Vec::new();
    let mut para_start = 0usize;
    let mut para_style: Option<u64> = None;
    // 文档级那张纸的原样（`paper_writes`）：只收第一次写的那一个，见下面那条判断
    let mut paper: Vec<(String, String)> = Vec::new();
    let mut me = Rtf {
        text: String::new(),
        lines: Vec::new(),
        declared_codepage: 1252,
        hex_bytes: 0,
        unicode_escapes: 0,
        pictures: 0,
        embedded_objects: 0,
        skipped_destinations: 0,
        headers: Vec::new(),
        footers: Vec::new(),
        page_destinations: 0,
        note_list: Vec::new(),
        note_destinations: 0,
        links: Vec::new(),
        fields: 0,
        fonts: Vec::new(),
        styles: Vec::new(),
        style_uses: Vec::new(),
        headings: Vec::new(),
        paper_writes: Vec::new(),
        table_row_defines: 0,
        table_rows: 0,
        table_cells: 0,
        table_cell_paras: 0,
        nested_table_rows: 0,
        nested_table_cells: 0,
        ftnalt: false,
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
            // `\*` 说的是「**紧跟它的那个目标群**你不认识就整群跳过」。
            // 所以先看清那是个什么群：认识的就不跳 —— LibreOffice 的脚注与尾注恰恰写成
            // `{\\*\\footnote …}`，一见 `\*` 就跳会把整条注丢掉（这份件就是这么发现的）。
            // 不认识才跳：fldinst（域指令原文）、userprops、批注的内部文本都从这一条走。
            flush(&mut out, &mut pending, codepage, &mut notes);
            let named = peek_word(bytes, i + 2);
            if NOTE_DESTINATIONS.contains(&named.as_str())
                || PAGE_DESTINATIONS.contains(&named.as_str())
            {
                i += 2;
                continue;
            }
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
        if word == "ftnalt" {
            // 只在「这是注的群」时被读到；判 kind 用
            me.ftnalt = true;
        }
        if !skipping && NOTE_DESTINATIONS.contains(&word.as_str()) {
            // 注住在自己的群里（`\footnote` 或 `{\*\footnote …}`，外层已经由 `\*` 那一支放行）：
            // 整群提出来单独交账，正文里一份不留
            let (stop, inner) = group_end(bytes, j);
            let sub = extract(&inner);
            let kind = if word == "endnote" || sub.ftnalt {
                "endnote"
            } else {
                "footnote"
            };
            for line in sub.lines {
                me.note_list
                    .push(json!({"kind": kind, "slot": word.clone(), "text": line}));
            }
            me.note_destinations += 1;
            // 这一跳吃掉了收尾的那个 `}`，群里层的跳过标记要自己弹掉
            if skip.len() > 1 {
                skip.pop();
            }
            i = stop + 1;
            continue;
        }
        if !skipping && PAGE_DESTINATIONS.contains(&word.as_str()) {
            // 目标群从这一位起，到关掉「当前这一群」的那个 `}` 止。
            // 里面递归走一遍：页眉也会有 \par、\u 与字段。
            let (stop, inner) = group_end(bytes, j);
            let sub = extract(&inner);
            let slot = word.clone();
            let into: &mut Vec<Value> = if page_kind(word.as_str()) == "header" {
                &mut me.headers
            } else {
                &mut me.footers
            };
            for line in sub.lines {
                into.push(json!({"slot": slot.clone(), "text": line}));
            }
            me.page_destinations += 1;
            // 那个 `}` 被这一跳吃掉了，群里层的跳过标记要自己弹掉
            if skip.len() > 1 {
                skip.pop();
            }
            i = stop + 1;
            continue;
        }
        if SKIP_DESTINATIONS.contains(&word.as_str())
            || NOTE_DEFINITION_WORDS.contains(&word.as_str())
        {
            if !skipping && DEF_DESTINATIONS.contains(&word.as_str()) {
                // 前瞻：不推进游标、不改这里的 skip —— 那一群照旧整群跳过，
                // 只是里面的 `{\fN …名字;}` / `{\sN …名字;}` 别再丢了
                let (_stop, inner) = group_end(bytes, j);
                for child in child_groups(&inner) {
                    if let Some(one) = definition_of(&child) {
                        if word == "fonttbl" {
                            me.fonts.push(one);
                        } else {
                            me.styles.push(one);
                        }
                    }
                }
            }
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
        } else if word == "field" {
            // 前瞻一步：找到紧跟这一群的那个 `{`，从群里读出链接。
            // 不推进游标，也不改 skip —— `\fldrslt` 的显示文字是页面上的字，
            // 而 `{\*\fldinst …}` 那一群照旧由 `\*` 那条规则处理（本域不认识就不进正文）
            me.fields += 1;
            if !skipping {
                let brace = (j..bytes.len()).find(|&k| bytes[k] == b'{');
                if let Some(at) = brace {
                    // 先把这一群量到收尾，再在群内找 —— 不看后面域的字
                    let (_stop, inner) = group_end(bytes, at);
                    if let Some(link) = field_link(&inner) {
                        me.links.push(link);
                    }
                }
            }
        } else if !skipping {
            if BREAK_WORDS.contains(&word.as_str()) || ROW_WORDS.contains(&word.as_str()) {
                out.push(b'\n');
                marks.push((para_start, out.len(), para_style));
                para_start = out.len();
                para_style = None;
            } else if TAB_WORDS.contains(&word.as_str()) {
                out.push(b'\t');
            }
            // 表那份账：数的就是这几个控制字本身（在不在跳过区由上面那条顺序决定）。
            // 「几张表」不在这儿 —— 那条推断规则拿两张件的对照试过，判不住
            match word.as_str() {
                "trowd" => me.table_row_defines += 1,
                "row" => me.table_rows += 1,
                "cell" => me.table_cells += 1,
                "intbl" => me.table_cell_paras += 1,
                "nestrow" => me.nested_table_rows += 1,
                "nestcell" => me.nested_table_cells += 1,
                _ => {}
            }
            // 样式被用了几次：正文里的 `\sN`（数字参数就在 digits 里）。
            // 同一处也记下「这一段现在用的是哪个样式」—— 段属性就在收尾之前
            if word == "s" {
                if let Some(index) = digits_after(digits.as_bytes(), 0) {
                    uses.push(index);
                    para_style = Some(index);
                }
            }
            // 那张纸写在文档级的属性里。每个词只记第一次写的，而且只看没被跳过的那一层 ——
            // 后面 `{\*\sectx …}` 里的那些是某一节的覆写，`\header` 那种已知目标群整个另读，
            // 都不算文档默认值。`landscape` 没有数字参数，记 "1" 表示「写了」
            if PAPER_WORDS.contains(&word.as_str())
                && !paper.iter().any(|(one, _)| one == &word)
                && (word == "landscape" || !digits.is_empty())
            {
                let value = if word == "landscape" {
                    "1".to_string()
                } else {
                    digits.clone()
                };
                paper.push((word, value));
            }
        }
        i = j;
    }
    flush(&mut out, &mut pending, codepage, &mut notes);
    me.declared_codepage = codepage;
    me.notes = notes;
    me.paper_writes = paper;
    // 用了几次的样式按名字合：文件写的是 `\s1`，名字在样式表那一群里
    uses.sort_unstable();
    let mut seen: Vec<(u64, usize)> = Vec::new();
    for one in &uses {
        match seen.last_mut() {
            Some(last) if last.0 == *one => last.1 += 1,
            _ => seen.push((*one, 1)),
        }
    }
    let resolved: Vec<Value> = seen
        .iter()
        .map(|(index, count)| {
            let named = me
                .styles
                .iter()
                .find(|one| one["kind"] == json!("paragraph") && one["index"] == json!(*index));
            // 用了却没定义的样式号也照交：名字给 `sN` 这种占位，不替文件编一个
            let name = match named.and_then(|one| one["name"].as_str()) {
                Some(text) => text.to_string(),
                None => format!("s{}", index),
            };
            json!({"index": index, "name": name, "count": count})
        })
        .collect();
    me.style_uses = resolved;
    // 标题：样式名写成 `heading N` 的那些段。层级不是猜出来的 —— 某一段用的是哪个样式号
    // 写在段属性里，那个号叫什么名字写在样式表里，两头都在文件上。
    // 用了却没定义的号（样式表里查不到）不算标题，也不给它编一个名字
    let headings: Vec<Value> = marks
        .iter()
        .filter_map(|(start, end, style)| {
            let level = heading_level(&me.styles, (*style)?)?;
            let text = String::from_utf8_lossy(out.get(*start..*end)?)
                .trim()
                .to_string();
            if text.is_empty() {
                return None;
            }
            Some(json!({"level": level, "text": text}))
        })
        .collect();
    me.headings = headings;
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

/// `\info` 一带的元数据：键由控制字自己说，值在它所引导的那一群里。
///
/// 三条判据都是从真件上踩出来的（`notes.rtf`，LibreOffice 由 python-docx 的 docx 转出）：
/// * `\upr{A}{B}` 的第一群是 7 位回退文本，这份文件在那儿只留下 `?`，真值在第二群
///   `\*\ud{...}` 里 —— 正文抽取把 `\*` 群整群跳过是对的，读元数据时照搬就会丢掉标题。
///   所以这里只跳 `\upr` 的第一群，`\*` 反而不跳。
/// * `\info{}` 在这份文件里只包住了标题那一对 `\upr`，其余键（subject / keywords /
///   doccomm / author / creatim / userprops）紧跟在同一层。只认「第一个群」会漏掉九成
///   元数据，所以扫到下一个**非元数据目标**（`\stylesheet` 那类）为止，并有硬上界。
/// * 文字要**按群整块交账**：逐字交账会把 `AppVersion` 变成十条自定义属性。
#[derive(Debug, Clone)]
pub struct Prop {
    pub name: String,
    pub kind: Option<i64>,
    pub value: Option<String>,
}

#[derive(Debug, Default)]
pub struct Info {
    pub found: bool,
    pub fields: std::collections::BTreeMap<String, String>,
    pub props: Vec<Prop>,
    pub notes: Vec<String>,
    pub codepage: u32,
}

impl Info {
    pub fn to_json(&self) -> Value {
        json!({
            "found": self.found,
            "codepage": self.codepage,
            "fields": self.fields,
            "user_props": self.props.iter().map(|one| json!({
                "name": one.name, "type": one.kind, "value": one.value,
            })).collect::<Vec<Value>>(),
            "notes": self.notes,
        })
    }
}

const INFO_TEXT_KEYS: &[(&str, &str)] = &[
    ("title", "title"),
    ("subject", "subject"),
    ("author", "author"),
    ("operator", "operator"),
    ("keywords", "keywords"),
    ("doccomm", "comment"),
    ("comments", "comment"),
    ("lastsavedby", "last_saved_by"),
    ("nchars", "chars"),
    ("nwords", "words"),
    ("npages", "pages"),
    ("nparas", "paragraphs"),
    ("nlines", "lines"),
    ("version", "version"),
    ("category", "category"),
    ("manager", "manager"),
    ("company", "company"),
    ("propname", "propname"),
    ("staticval", "staticval"),
];

const INFO_TIME_KEYS: &[(&str, &str)] = &[
    ("creatim", "created"),
    ("revtim", "modified"),
    ("printim", "printed"),
];

const INFO_TIME_PARTS: &[&str] = &["yr", "mo", "dy", "hr", "min", "sec"];

const INFO_STOP: &[&str] = &[
    "stylesheet",
    "fonttbl",
    "colortbl",
    "generator",
    "listtable",
    "listoverridetable",
    "themedata",
    "colorschememapping",
    "datastore",
    "rsidtbl",
    "xmlnstbl",
    "filetbl",
    "header",
    "footer",
    "pict",
    "object",
    "sect",
    "latentstyles",
    "bkmkstart",
    "atncluster",
    "mmathPr",
];

const INFO_SCAN_CAP: usize = 65536;

/// 找一个作为**完整控制字**出现的目标（`\info` 不算 `\information` 的一部分）
fn find_control(bytes: &[u8], name: &[u8]) -> Option<usize> {
    let mut at = 0usize;
    while let Some(found) = windows_position(bytes, at, name) {
        if found >= 1
            && bytes[found - 1] == b'\\'
            && !bytes
                .get(found + name.len())
                .is_some_and(|one| one.is_ascii_alphabetic())
        {
            return Some(found - 1);
        }
        at = found + 1;
    }
    None
}

fn windows_position(bytes: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    bytes
        .get(from..)?
        .windows(needle.len())
        .position(|one| one == needle)
        .map(|one| one + from)
}

fn key_for(word: &str) -> Option<&'static str> {
    INFO_TEXT_KEYS
        .iter()
        .find(|(one, _)| *one == word)
        .map(|(_, key)| *key)
}

fn time_key(word: &str) -> Option<&'static str> {
    INFO_TIME_KEYS
        .iter()
        .find(|(one, _)| *one == word)
        .map(|(_, key)| *key)
}

/// `\ansicpg` 声明的字符集：取最后一次声明，跟 `extract` 的走法一致
fn declared_codepage(head: &[u8]) -> u32 {
    let mut found: Option<usize> = None;
    let mut at = 0usize;
    while let Some(hit) = windows_position(head, at, b"ansicpg") {
        if hit >= 1 && head[hit - 1] == b'\\' {
            found = Some(hit + b"ansicpg".len());
        }
        at = hit + 1;
    }
    let Some(mut i) = found else { return 1252 };
    let mut digits: String = String::new();
    while i < head.len() && head[i].is_ascii_digit() {
        digits.push(head[i] as char);
        i += 1;
    }
    digits.parse::<u32>().unwrap_or(1252)
}

struct Frame {
    key: Option<&'static str>,
    skip: bool,
    buf: Vec<u8>,
}

/// 读出 `\info` 一带的元数据；没有 `\info` 时返回 `found: false`，不编字段
pub fn parse_info(bytes: &[u8]) -> Info {
    let Some(start) = find_control(bytes, b"info") else {
        let mut info = Info::default();
        info.notes.push("这份 RTF 没有 \\info 群".to_string());
        return info;
    };
    let head = bytes.get(..start).unwrap_or_default();
    let codepage = declared_codepage(head);
    let mut info = Info {
        found: true,
        fields: std::collections::BTreeMap::new(),
        props: Vec::new(),
        notes: Vec::new(),
        codepage,
    };
    let mut stack: Vec<Frame> = vec![Frame {
        key: None,
        skip: false,
        buf: Vec::new(),
    }];
    let mut next_key: Option<&'static str> = None;
    let mut skip_next = false;
    let mut current_time: Option<&'static str> = None;
    let mut times: std::collections::BTreeMap<String, [u32; 6]> = std::collections::BTreeMap::new();
    let mut pending: Vec<u8> = Vec::new();
    let mut ucount = 1usize;
    let end = (start + INFO_SCAN_CAP).min(bytes.len());
    let mut i = start;

    while i < end {
        let ch = bytes[i];
        if ch == b'{' {
            flush_into(&mut stack, &mut pending, codepage, &mut info.notes);
            let parent = stack.last().and_then(|one| one.key);
            let key = next_key.or(parent);
            stack.push(Frame {
                key,
                skip: skip_next,
                buf: Vec::new(),
            });
            next_key = None;
            skip_next = false;
            i += 1;
            continue;
        }
        if ch == b'}' {
            flush_into(&mut stack, &mut pending, codepage, &mut info.notes);
            if stack.len() > 1 {
                let frame = stack.pop();
                if let Some(frame) = frame {
                    commit(frame, &mut info);
                }
            }
            i += 1;
            continue;
        }
        if ch == b'\r' || ch == b'\n' {
            i += 1;
            continue;
        }
        if ch != b'\\' {
            flush_into(&mut stack, &mut pending, codepage, &mut info.notes);
            if let Some(frame) = stack.last_mut() {
                if frame.key.is_some() && !frame.skip {
                    push_char(&mut frame.buf, ch as char);
                }
            }
            i += 1;
            continue;
        }
        if bytes.get(i + 1) == Some(&b'*') {
            i += 2;
            continue;
        }
        if bytes.get(i + 1) == Some(&b'\'') {
            if let Some(frame) = stack.last() {
                if frame.key.is_some() && !frame.skip {
                    let high = bytes.get(i + 2).copied().unwrap_or(b'0');
                    let low = bytes.get(i + 3).copied().unwrap_or(b'0');
                    pending.push(hex_digit(high) * 16 + hex_digit(low));
                }
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
            flush_into(&mut stack, &mut pending, codepage, &mut info.notes);
            if let Some(frame) = stack.last_mut() {
                if frame.key.is_some() && !frame.skip {
                    if let Some(one) = bytes.get(j) {
                        frame.buf.push(*one);
                    }
                }
            }
            i = if j < bytes.len() { j + 1 } else { j };
            continue;
        }
        let mut digits: Vec<u8> = Vec::new();
        while j < bytes.len() && (bytes[j].is_ascii_digit() || bytes[j] == b'-') {
            digits.push(bytes[j]);
            j += 1;
        }
        if bytes.get(j) == Some(&b' ') {
            j += 1;
        }
        flush_into(&mut stack, &mut pending, codepage, &mut info.notes);
        let digits = String::from_utf8_lossy(&digits).into_owned();
        if word == "uc" {
            ucount = digits.parse::<usize>().unwrap_or(1);
        } else if word == "u" && !digits.is_empty() {
            let raw = digits.parse::<i64>().unwrap_or(0);
            let value = if raw < 0 { raw + 65536 } else { raw };
            if let Some(frame) = stack.last_mut() {
                if frame.key.is_some() && !frame.skip {
                    if let Some(one) = u32::try_from(value).ok().and_then(char::from_u32) {
                        push_char(&mut frame.buf, one);
                    } else {
                        info.notes.push(format!("\\u{digits} 这个数不是有效码位"));
                    }
                }
            }
            i = skip_rtf_chars(bytes, j, ucount);
            continue;
        } else if let Some(key) = key_for(&word) {
            // 这类控制字出现在它自己那一群的开头（{\title 文字}），定的是当前这一帧的键
            let frame_is_open = stack.last().map(|one| one.key.is_none()).unwrap_or(false);
            if frame_is_open {
                if let Some(frame) = stack.last_mut() {
                    frame.key = Some(key);
                }
            } else {
                next_key = Some(key);
            }
            if key == "version" && !digits.is_empty() {
                info.fields.insert("version".to_string(), digits.clone());
            }
        } else if let Some(key) = time_key(&word) {
            current_time = Some(key);
            times.insert(key.to_string(), [0u32; 6]);
        } else if INFO_TIME_PARTS.contains(&word.as_str()) && current_time.is_some() {
            let slot = match word.as_str() {
                "yr" => 0,
                "mo" => 1,
                "dy" => 2,
                "hr" => 3,
                "min" => 4,
                _ => 5,
            };
            let name = current_time.unwrap_or_default().to_string();
            if let Some(one) = times.get_mut(&name) {
                one[slot] = digits.parse::<u32>().unwrap_or(0);
            }
        } else if word == "proptype" {
            if !digits.is_empty() {
                if let Some(last) = info.props.last_mut() {
                    last.kind = digits.parse::<i64>().ok();
                }
            }
        } else if word == "upr" {
            skip_next = true;
        } else if INFO_STOP.contains(&word.as_str()) {
            break;
        }
        i = j;
    }
    flush_into(&mut stack, &mut pending, codepage, &mut info.notes);
    if let Some(frame) = stack.pop() {
        commit(frame, &mut info);
    }
    for (name, stamp) in &times {
        if stamp[0] == 0 {
            info.notes
                .push(format!("{name} 在文件里是全零（等于没写这个时间）"));
            continue;
        }
        info.fields.insert(
            name.clone(),
            format!(
                "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
                stamp[0], stamp[1], stamp[2], stamp[3], stamp[4], stamp[5]
            ),
        );
    }
    info
}

fn flush_into(
    stack: &mut Vec<Frame>,
    pending: &mut Vec<u8>,
    codepage: u32,
    notes: &mut Vec<String>,
) {
    if pending.is_empty() {
        return;
    }
    let mut out: Vec<u8> = Vec::new();
    flush(&mut out, pending, codepage, notes);
    if let Some(frame) = stack.last_mut() {
        frame.buf.extend_from_slice(&out);
    }
}

fn commit(frame: Frame, info: &mut Info) {
    let Some(key) = frame.key else { return };
    if frame.buf.is_empty() {
        return;
    }
    let text = String::from_utf8_lossy(&frame.buf).into_owned();
    match key {
        "propname" => info.props.push(Prop {
            name: text,
            kind: None,
            value: None,
        }),
        "staticval" => {
            if let Some(last) = info.props.last_mut() {
                let joined = match last.value.take() {
                    Some(had) => had + &text,
                    None => text,
                };
                last.value = Some(joined);
            }
        }
        "created" | "modified" | "printed" => {}
        other => {
            let joined = match info.fields.get(other) {
                Some(had) => had.clone() + &text,
                None => text,
            };
            info.fields.insert(other.to_string(), joined);
        }
    }
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

    /// `\*` 修饰的是**紧跟它的那个群**：认识的（脚注、尾注、页眉）就不跳。
    /// LibreOffice 把脚注与尾注都写进 `\footnote` 口袋，尾注只多一个 `\ftnalt`；
    /// 注的排版定义（`\ftnsep` 那一族）不是一条注
    #[test]
    fn star_destinations_are_read_when_the_domain_knows_them() {
        let one = rtf(
            "{\\rtf1\\ansi 正文{\\super \\chftn{\\*\\footnote \\chftn 脚注的字。}}{\\super \\chftn{\\*\\footnote\\ftnalt \\chftn 尾注的字。}}}",
        );
        assert_eq!(one.lines, vec!["正文".to_string()], "{:?}", one.lines);
        let kinds: Vec<&str> = one
            .note_list
            .iter()
            .map(|had| had["kind"].as_str().unwrap_or_default())
            .collect();
        assert_eq!(kinds, ["footnote", "endnote"], "{:?}", one.note_list);
        assert_eq!(one.note_destinations, 2);
        // 分隔符定义里没有「一条注」
        let sep = rtf("{\\rtf1\\ansi 正文{\\*\\ftnsep\\chftnsep}{\\*\\ftncn\\chftncn}}");
        assert!(sep.note_list.is_empty(), "{:?}", sep.note_list);
        assert_eq!(sep.lines, vec!["正文".to_string()], "{:?}", sep.lines);
        // 真件：一份 LibreOffice 写的 RTF，两条脚注一条尾注
        let real = extract(&fixture("notes-end.rtf"));
        assert_eq!(real.note_destinations, 3, "{:?}", real.note_list);
        assert_eq!(
            real.note_list
                .iter()
                .filter(|had| had["kind"] == json!("endnote"))
                .count(),
            1,
            "{:?}",
            real.note_list
        );
        assert!(
            !real.text.contains("gross"),
            "注的字混进正文了：{}",
            real.text
        );
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
        let one = rtf("{\\uc1\\u20013\\'39\\u22826\\'41 x}");
        assert_eq!(
            one.text, "中太 x",
            "控制字后的那个空格属于控制字，正文里的空格要留下"
        );
        assert_eq!(one.unicode_escapes, 2);
        assert_eq!(one.hex_bytes, 0, "被丢掉的回退字节不该计数成 hex");
        // `\uc0` 说的是「一个回退字符都不跳」：那个 ? 就是正文，不是回退垃圾
        let zero = rtf("{\\uc0\\u20013?}");
        assert_eq!(
            zero.text, "中?",
            "\\uc0 被当成默认的 1 就会吞掉正文里的问号"
        );
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

    /// 标题的层级只从样式名来：`heading 1` 算、`Heading 2` 也算（这个词不分大小写），
    /// `heading3`（中间没空格）、`heading 1a`（数字后面还有字）、`Summary`（自定义名）都不算；
    /// 同号的字符样式 `{\*\cs1 heading 3;}` 不能顶掉段落样式的名字 —— `\sN` 与 `\csN`
    /// 是两个各自的编号空间；末尾没有 `\par` 的那一段不收（段以回车收，与 `lines` 同一口径）。
    /// 期望值来自 `lyco_rtf.py` 对同一串字的独立读取
    #[test]
    fn headings_come_only_from_the_style_name() {
        let one = rtf(
            "{\\rtf1\\ansi{\\stylesheet {\\s0 Normal;}{\\s1 heading 1;}{\\s2 Heading 2;}{\\s3 heading3;}{\\s4 heading 1a;}{\\s5 Summary;}{\\*\\cs1 heading 3;}}{\\pard\\s1 one\\par}{\\pard\\s2 two\\par}{\\pard\\s3 nospace\\par}{\\pard\\s4 trailing\\par}{\\pard\\s5 custom\\par}{\\pard\\s0 body\\par}{\\pard\\s1 noendpar}}",
        );
        assert_eq!(
            one.headings,
            vec![
                json!({"level": 1, "text": "one"}),
                json!({"level": 2, "text": "two"})
            ],
            "{:?}",
            one.headings
        );
        // 那一段仍然在正文里 —— 不认它当标题，不等于把它丢了
        assert_eq!(one.lines.len(), 7, "{:?}", one.lines);
        assert_eq!(one.lines[6], "noendpar", "{:?}", one.lines);
        // 真件：LibreOffice 的 RTF 导出把样式名写成小写 `heading 1`，两本账都要对
        let real = extract(&fixture("notes.rtf"));
        assert_eq!(
            real.headings,
            vec![
                json!({"level": 1, "text": "一级标题：预算口径"}),
                json!({"level": 2, "text": "二级标题：明细"})
            ],
            "{:?}",
            real.headings
        );
        // 一份全用 Normal 的：空数组，不是 null
        assert!(
            extract(&fixture("notes-end.rtf")).headings.is_empty(),
            "{:?}",
            extract(&fixture("notes-end.rtf")).headings
        );
        // 一条标题就是**一段**：换行与制表都不该混进它的字里。
        // 这一条要单独守 —— 上一版把切片写成「从起点到文末」，层级数全对、文本却吃掉整篇文档，
        // 只比条数与层级的断言抓不住
        for one in real.headings.iter().chain(one.headings.iter()) {
            let text = one["text"].as_str().unwrap_or_default();
            assert!(!text.contains('\n') && !text.contains('\t'), "{text}");
        }
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

    /// `\info` 群：标题在 `\upr` 的 `\*\ud` 那一边，其余键与 `\info{}` 平级排着。
    /// 期望值来自 `lyco_rtf.py` 的 `rtf_info`，CI 里逐字段对账。
    #[test]
    fn info_group_metadata_comes_out_whole() {
        let bytes = fixture("notes.rtf");
        let info = parse_info(&bytes);
        assert!(info.found);
        assert_eq!(info.codepage, 1252);
        assert_eq!(
            info.fields.get("title").map(|one| one.as_str()),
            Some("季度预算说明")
        );
        assert_eq!(
            info.fields.get("subject").map(|one| one.as_str()),
            Some("季度预算")
        );
        assert_eq!(
            info.fields.get("keywords").map(|one| one.as_str()),
            Some("budget, quarterly")
        );
        assert_eq!(
            info.fields.get("comment").map(|one| one.as_str()),
            Some("fixture produced by python-docx")
        );
        assert_eq!(
            info.fields.get("author").map(|one| one.as_str()),
            Some("liuqi")
        );
        assert_eq!(
            info.fields.get("created").map(|one| one.as_str()),
            Some("2013-12-23T23:15:00")
        );
        assert_eq!(info.fields.len(), 7, "{:?}", info.fields);
        // 四条自定义属性：名字要整块交账，逐字交账会变成几十条
        let names: Vec<String> = info.props.iter().map(|one| one.name.clone()).collect();
        assert_eq!(
            names,
            vec![
                "AppVersion".to_string(),
                "OOXMLCorePropertyCategory".to_string(),
                "口径".to_string(),
                "预算额度".to_string()
            ],
            "{names:?}"
        );
        assert_eq!(
            info.props[0].value.as_deref(),
            Some("14.0000"),
            "{:?}",
            info.props[0]
        );
        assert_eq!(info.props[3].kind, Some(3));
        assert_eq!(info.props[3].value.as_deref(), Some("124000"));
        // printim 全零：不能把 0000-00-00 当成一个真时间报出去
        assert!(!info.fields.contains_key("printed"), "{:?}", info.fields);
        assert!(
            info.notes.iter().any(|one| one.contains("printed")),
            "{:?}",
            info.notes
        );
    }

    /// 没有 `\info` 的文件要照实说，不能给一个空 map 装作读过
    #[test]
    fn a_document_without_info_says_so() {
        let info = parse_info(br"{\ansi hello}");
        assert!(!info.found);
        assert!(info.fields.is_empty());
        assert!(!info.notes.is_empty());
    }

    /// `\upr` 的第一群是 7 位回退文本：只认它就会拿到一串问号
    #[test]
    fn the_upr_fallback_is_not_the_value() {
        let src = br"\{\ansi\ansicpg1252\info{\upr{\title \'3f\'3f}{\*\ud{\title \u26381\'3f\u21153\'3f}}}";
        let info = parse_info(src);
        assert_eq!(
            info.fields.get("title").map(|one| one.as_str()),
            Some("服务"),
            "{:?}",
            info.fields
        );
    }
}
