//! MS-XLSX / MS-XLS 的数字格式：一个格子是「日期」还是「数」，账不在格子上。
//!
//! 格子写的是 `s="3"` —— 那是 `xl/styles.xml` 里 `cellXfs` 的**下标**，不是格式号。
//! 少绕这一层，日期就永远只是一个数（比如 41631）。绕过去之后还有三处会绊人：
//!
//! 1. **内置格式号只写号、不写串**（14 就是 `mm-dd-yy`），所以得带着那张表；
//!    而真实生产者不一定用内置号 —— 写这份 fixture 的 openpyxl 把日期、百分比、
//!    货币全写成自定义号 164 往上，只查内置表就会一个都认不出来。
//! 2. **格式串里的字面量不是格式标记**。`yyyy"年"m"月"d"日"` 的「月」在引号里，
//!    是照着抄的汉字；`[Red]`、`\-` 之类的方括号段同理。不先剥掉这些，
//!    一段纯文本能被认成日期。另外 `m` 跟在时间标记之后是**分钟**不是月份：
//!    `h:mm` 是时间，`yyyy-mm` 才是日期。
//! 3. **序列数与日期之间没有唯一换算**：1900 系统里第 60 号是那个不存在的
//!    1900-02-29（Excel 的闰年 bug），而 `date1904="1"` 的工作簿整套基准都不一样。
//!    所以先看 `xl/workbook.xml` 怎么说，再换算；60 号照文件原样报，不编成某一天。
//!
//! 与 `scripts/acceptance/lyco_formats.py` 是同一套规范的两份实现，CI 里对
//! `formats.xlsx`（openpyxl 写的真件）逐格对账。

use std::collections::BTreeMap;

use serde_json::{json, Value};

use crate::xmlscan::{self, Node};
use crate::zipread;

const STYLES_PART: &str = "xl/styles.xml";
const WORKBOOK_PART: &str = "xl/workbook.xml";
const MEMBER_CAP: u64 = 8 << 20;

/// 「日期还是数」的判定结果。`general` 是文件真的没写格式（`numFmtId=0`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    General,
    Number,
    Percent,
    Currency,
    Date,
    Datetime,
    Time,
    /// 格式号在内置表里，但这一版没抄它的串（见 `builtin_code` 那段注释）——
    /// 报成 general 就是替文件编话，所以说「不知道」。
    Unknown,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::General => "general",
            Kind::Number => "number",
            Kind::Percent => "percent",
            Kind::Currency => "currency",
            Kind::Date => "date",
            Kind::Datetime => "datetime",
            Kind::Time => "time",
            Kind::Unknown => "unknown",
        }
    }
}

/// ECMA-376 / MS-XLSX 2.1.310 的内置格式号（文件里只写号，不写串）
pub fn builtin_code(id: u64) -> Option<&'static str> {
    Some(match id {
        0 => "General",
        1 => "0",
        2 => "0.00",
        3 => "#,##0",
        4 => "#,##0.00",
        5 => "$#,##0_);($#,##0)",
        6 => "$#,##0_);[Red]($#,##0)",
        7 => "$#,##0.00_);($#,##0.00)",
        8 => "$#,##0.00_);[Red]($#,##0.00)",
        9 => "0%",
        10 => "0.00%",
        11 => "0.00E+00",
        12 => "# ?/?",
        13 => "# ??/??",
        14 => "mm-dd-yy",
        15 => "d-mmm-yy",
        16 => "d-mmm",
        17 => "mmm-yy",
        18 => "h:mm AM/PM",
        19 => "h:mm:ss AM/PM",
        20 => "h:mm",
        21 => "h:mm:ss",
        22 => "m/d/yy h:mm",
        45 => "mm:ss",
        46 => "[h]:mm:ss",
        47 => "mmss.##",
        // 23-36 与 48-58 这一批是**跟着地区变**的日期/时间内置号（中文 Excel 写 31 是
        // `yyyy"年"m"月"d"日"`，日文 Excel 写同一个号是另一套串）。手上没有真件证明
        // 该抄哪一份，所以宁可让 `format_of` 说「这个号我没抄」，也不照记忆写一个。
        _ => return None,
    })
}

/// 只留真正参与格式的字符：引号里的字面量、反斜杠转义与 `[方括号]` 段都剥掉
pub fn strip_tokens(code: &str) -> String {
    let raw: Vec<char> = code.chars().collect();
    let mut out: Vec<char> = Vec::new();
    let mut i = 0usize;
    while i < raw.len() {
        let ch = raw[i];
        if ch == '"' || ch == '\'' {
            match raw[i + 1..].iter().position(|one| *one == ch) {
                Some(stop) => i += stop + 2,
                None => i = raw.len(),
            }
            continue;
        }
        if ch == '\\' {
            i += 2;
            continue;
        }
        if ch == '[' {
            match raw[i + 1..].iter().position(|one| *one == ']') {
                Some(stop) => i += stop + 2,
                None => i = raw.len(),
            }
            continue;
        }
        out.push(ch.to_ascii_lowercase());
        i += 1;
    }
    out.into_iter().collect()
}

/// 格式串自己说它是什么：先剥字面量，再看标记字母
pub fn kind_of(code: &str) -> Kind {
    let tokens = strip_tokens(code);
    let has_time = tokens.contains('h')
        || tokens.contains('s')
        || tokens.contains("am/pm")
        || tokens.contains("a/p");
    // m 跟在时间标记之后是分钟：h:mm 是时间，yyyy-mm 才是日期
    let has_date =
        tokens.contains('y') || tokens.contains('d') || (tokens.contains('m') && !has_time);
    if tokens.contains('%') {
        return Kind::Percent;
    }
    if has_date && has_time {
        return Kind::Datetime;
    }
    if has_date {
        return Kind::Date;
    }
    if has_time {
        return Kind::Time;
    }
    if tokens.contains('$') || tokens.contains('¥') || tokens.contains('￥') || tokens.contains('€')
    {
        return Kind::Currency;
    }
    if tokens.is_empty() || tokens == "general" {
        return Kind::General;
    }
    Kind::Number
}

/// 序列数 → ISO 串。`year1904` 来自 `xl/workbook.xml` 的 `date1904`。
pub fn serial_to_iso(value: f64, year1904: bool) -> String {
    let days = value.trunc() as i64;
    let rest = value - days as f64;
    if !year1904 && days == 60 {
        // 文件自己就写着 60 号；这一天不存在，但替它改个日期是替文件编话
        return "1900-02-29".to_string();
    }
    let unix_days: i64 = if year1904 {
        // 1904-01-01 距 1970-01-01 是 24107 天（1900 系统那边是 25569，两套基准差 1462 天）
        days - 24_107
    } else if days < 60 {
        days - 25_568
    } else {
        days - 25_569
    };
    let (year, month, day) = crate::props::civil_from_days(unix_days);
    let seconds = (rest * 86_400.0).round() as i64 % 86_400;
    if seconds < 0 {
        return format!("{year:04}-{month:02}-{day:02}");
    }
    if seconds == 0 {
        format!("{year:04}-{month:02}-{day:02}")
    } else {
        format!(
            "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}",
            seconds / 3600,
            seconds % 3600 / 60,
            seconds % 60
        )
    }
}

/// 一个工作簿的格式账：`cellXfs` 的下标 → 格式号，自定义号 → 格式串，以及 1904 基准。
///
/// 后半截是「这一格长什么样」的那一跳：`cellXfs` 的每条 `xf` 只写三个号
/// （`fontId` / `fillId` / `borderId`），字面住在 `fonts` / `fills` / `borders` 那三张表里。
/// 两张表的条数与它们自报的 `count` 一起交，行内容按文件写的属性与孩子元素原样交 ——
/// 同一个底色，openpyxl 的占位是**空的** `<patternFill/>` 而 LibreOffice 写
/// `patternType="none"`；同一个粗体开关一家写 `val="1"` 另一家写 `val="true"`；
/// LibreOffice 还会把 `lightGrid` 那种花纹**换算成 solid** 并改颜色 —— 所以只交不比。
#[derive(Debug, Default)]
pub struct Styles {
    pub xfs: Vec<u64>,
    pub custom: BTreeMap<u64, String>,
    pub year1904: bool,
    pub notes: Vec<String>,
    /// `cellXfs` 的每一条：写着的属性 + 孩子元素（`alignment` / `protection`）
    pub xf_rows: Vec<Value>,
    pub font_rows: Vec<Value>,
    pub fill_rows: Vec<Value>,
    pub border_rows: Vec<Value>,
    /// 那几张表各自「自报几条 / 实际几条」
    pub ledger: Value,
}

/// 一个元素自己写着的属性（按文件写的名字与顺序，键原样带前缀）
fn attrs_json(node: &Node) -> Value {
    let mut out = serde_json::Map::new();
    for (key, value) in node.attrs.iter() {
        out.insert(key.clone(), json!(value));
    }
    Value::Object(out)
}

/// 一行表内容：自己的属性 + 孩子元素（名字与各自的属性）
fn row_json(node: &Node) -> Value {
    let parts: Vec<Value> = node
        .children
        .iter()
        .filter(|one| one.local() != "#text")
        .map(|one| json!({"element": one.name, "attrs": attrs_json(one)}))
        .collect();
    json!({"attrs": attrs_json(node), "parts": parts})
}

/// 一张表的条数账：`count` 是文件自己说的，`found` 是数出来的
fn table_ledger(root: &Node, want: &str, found: usize) -> Value {
    let written = root
        .descendants(want)
        .into_iter()
        .next()
        .and_then(|one| one.attr_local("count"))
        .map(|one| one.to_string());
    let agreed = match written.as_deref() {
        None => true,
        Some(raw) => raw.trim().parse::<usize>().ok() == Some(found),
    };
    json!({"written": written, "found": found, "whole": agreed})
}

fn child_attrs<'a>(row: &'a Value, name: &str) -> Option<&'a Value> {
    row.get("parts")?
        .as_array()?
        .iter()
        .find(|one| one["element"].as_str() == Some(name))?
        .get("attrs")
}

/// OOXML 那两种布尔拼法：元素在而没写 `val` 按 true 算，`0` / `false` / `none` 按 false
fn said_on(had: Option<&Value>) -> Option<bool> {
    match had.and_then(|one| one.as_str()) {
        None => Some(true),
        Some(raw) => Some(!matches!(
            raw.trim().to_ascii_lowercase().as_str(),
            "0" | "false" | "none"
        )),
    }
}

fn flag_in(row: &Value, name: &str) -> Option<bool> {
    child_attrs(row, name).and_then(|had| said_on(had.get("val")))
}

/// 属性形式的开关：没写这个属性就是「文件没说」（null），不像元素形式那样按 true 算
fn attr_flag(had: Option<&Value>) -> Option<bool> {
    had.and_then(|one| one.as_str()).map(|raw| {
        !matches!(
            raw.trim().to_ascii_lowercase().as_str(),
            "" | "0" | "false" | "none"
        )
    })
}

impl Styles {
    /// 某个样式下标对应的 (格式号, 格式串)。串是 `None` 表示这个号这一版没抄串。
    pub fn format_of(&self, style: usize) -> (u64, Option<String>) {
        let id = self.xfs.get(style).copied().unwrap_or(0);
        if let Some(one) = self.custom.get(&id) {
            return (id, Some(one.clone()));
        }
        (id, builtin_code(id).map(|one| one.to_string()))
    }

    pub fn kind_of(&self, style: usize) -> Kind {
        match self.format_of(style).1 {
            Some(code) => kind_of(&code),
            None => Kind::Unknown,
        }
    }

    /// 一个格子「长什么样」那一跳：`cellXfs` 那条自己说了什么，加上顺着它三个号查到的
    /// 三行表内容（`fontId` → `fonts`、`fillId` → `fills`、`borderId` → `border`）。
    ///
    /// 号写了而那张表里没有 → 那一行交 null（`style_*_id` 仍交那个号 —— 「写了个指不到
    /// 的号」是文件自己说的话）。三个三态开关（`style_bold` / `style_filled` /
    /// `style_wrapped`）是按文件写的两种布尔拼法数的，null 是「这一条文件没说」：
    /// 粗体看 `font` 行里那个 `b` 孩子在不在，换行看 `alignment` 的 `wrapText`，
    /// 底色看 `patternFill/@patternType`（空的 `<patternFill/>` 与 `patternType="none"`
    /// 都算「没有底色」，而那两件事在 `style_fill` 那份原样账里看得见）。
    pub fn appearance(&self, style: usize) -> Value {
        let Some(row) = self.xf_rows.get(style) else {
            return json!({
                "style_found": false,
                "style_attrs": Value::Null,
                "style_font_id": Value::Null,
                "style_font": Value::Null,
                "style_fill_id": Value::Null,
                "style_fill": Value::Null,
                "style_border_id": Value::Null,
                "style_border": Value::Null,
                "style_alignment": Value::Null,
                "style_bold": Value::Null,
                "style_filled": Value::Null,
                "style_wrapped": Value::Null,
            });
        };
        let written = || -> Value { row["attrs"].clone() };
        let id_of = |key: &str| -> Value { row["attrs"].get(key).cloned().unwrap_or(Value::Null) };
        let picked = |key: &str, table: &[Value]| -> Value {
            row["attrs"]
                .get(key)
                .and_then(|one| one.as_str())
                .and_then(|raw| raw.trim().parse::<usize>().ok())
                .and_then(|which| table.get(which))
                .cloned()
                .unwrap_or(Value::Null)
        };
        let tri = |one: Option<bool>| -> Value { one.map(|yes| json!(yes)).unwrap_or(Value::Null) };
        let font = picked("fontId", &self.font_rows);
        let fill = picked("fillId", &self.fill_rows);
        let bold = flag_in(&font, "b");
        let filled = child_attrs(&fill, "patternFill").map(|had| {
            match had.get("patternType").and_then(|one| one.as_str()) {
                None => false,
                Some(raw) => raw != "none",
            }
        });
        let wrapped = child_attrs(row, "alignment").and_then(|had| attr_flag(had.get("wrapText")));
        json!({
            "style_found": true,
            "style_attrs": written(),
            "style_font_id": id_of("fontId"),
            "style_font": font,
            "style_fill_id": id_of("fillId"),
            "style_fill": fill,
            "style_border_id": id_of("borderId"),
            "style_border": picked("borderId", &self.border_rows),
            "style_alignment": child_attrs(row, "alignment").cloned().unwrap_or(Value::Null),
            "style_bold": tri(bold),
            "style_filled": tri(filled),
            "style_wrapped": tri(wrapped),
        })
    }

    /// 一个格子完整的格式账：号、串、判定，必要时再加上换算出来的日期。
    /// `cell_type` 是 `<c t="…">`：文本与布尔格子的类型由文件直接说，不用去猜格式串。
    pub fn cell_format(&self, style: usize, raw: Option<&str>, cell_type: &str) -> Value {
        let (id, code) = self.format_of(style);
        let judged = match &code {
            Some(one) => kind_of(one),
            None => Kind::Unknown,
        };
        let kind = match cell_type {
            "s" | "str" | "inlineStr" => "text",
            "b" => "bool",
            _ => judged.as_str(),
        };
        let mut out = serde_json::Map::new();
        out.insert("num_fmt".to_string(), json!(id));
        out.insert("format".to_string(), json!(code));
        // `format_kind` 而不是 `kind`：格子上已经有一个 `kind` 写着它的类型（n / s / b）
        out.insert("format_kind".to_string(), json!(kind));
        out.insert("style".to_string(), json!(style));
        if matches!(judged, Kind::Date | Kind::Datetime | Kind::Time) {
            let stamp = raw
                .and_then(|one| one.trim().parse::<f64>().ok())
                .map(|one| json!(serial_to_iso(one, self.year1904)))
                .unwrap_or(Value::Null);
            out.insert("as_date".to_string(), stamp);
        }
        Value::Object(out)
    }
}

fn attr_u64(node: &Node, want: &str) -> Option<u64> {
    node.attr_local(want)?.trim().parse::<u64>().ok()
}

/// 从包里读出格式账。没有 styles.xml 就给一份全 General 的表并说明。
pub fn read_styles(bytes: &[u8]) -> Styles {
    let mut me = Styles::default();
    let Ok(member) = zipread::member(bytes, WORKBOOK_PART, MEMBER_CAP) else {
        me.notes.push(format!("包里读不到 {WORKBOOK_PART}"));
        return me;
    };
    let book = xmlscan::parse_str(&member.as_text());
    me.year1904 = book
        .descendants("workbookPr")
        .into_iter()
        .next()
        .and_then(|one| one.attr_local("date1904"))
        .is_some_and(|raw| {
            matches!(
                raw.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "on"
            )
        });
    let Ok(member) = zipread::member(bytes, STYLES_PART, MEMBER_CAP) else {
        me.notes
            .push(format!("包里读不到 {STYLES_PART}（所有格子按 General 算）"));
        return me;
    };
    let root = xmlscan::parse_str(&member.as_text());
    for one in root.descendants("numFmt") {
        let Some(id) = attr_u64(one, "numFmtId") else {
            me.notes.push("有一个 numFmt 没写 numFmtId".to_string());
            continue;
        };
        let code = one.attr_local("formatCode").unwrap_or_default().to_string();
        me.custom.insert(id, code);
    }
    // 三张表先收：`cellXfs` 那三个号就是指着它们的
    me.font_rows = table_rows(&root, "fonts", "font");
    me.fill_rows = table_rows(&root, "fills", "fill");
    me.border_rows = table_rows(&root, "borders", "border");
    me.xf_rows = table_rows(&root, "cellXfs", "xf");
    let cell_style_xfs = table_rows(&root, "cellStyleXfs", "xf");
    me.ledger = json!({
        "part": true,
        "fonts": table_ledger(&root, "fonts", me.font_rows.len()),
        "fills": table_ledger(&root, "fills", me.fill_rows.len()),
        "borders": table_ledger(&root, "borders", me.border_rows.len()),
        "cell_xfs": table_ledger(&root, "cellXfs", me.xf_rows.len()),
        "cell_style_xfs": table_ledger(&root, "cellStyleXfs", cell_style_xfs.len()),
    });
    let Some(list) = root.descendants("cellXfs").into_iter().next() else {
        me.notes.push("styles.xml 里没有 cellXfs".to_string());
        return me;
    };
    for one in list.all("xf") {
        me.xfs.push(attr_u64(one, "numFmtId").unwrap_or(0));
    }
    me
}

/// 一张表的每一行（`fonts` 里的 `font`、`fills` 里的 `fill`…）：只收名字对得上的直接孩子
fn table_rows(root: &Node, want: &str, keep: &str) -> Vec<Value> {
    let Some(holder) = root.descendants(want).into_iter().next() else {
        return Vec::new();
    };
    holder
        .children
        .iter()
        .filter(|one| one.local() == keep)
        .map(row_json)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes_of(name: &str) -> Vec<u8> {
        std::fs::read(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/office")
                .join(name),
        )
        .expect("读 fixture")
    }

    /// 字面量与方括号段不许参与判定
    #[test]
    fn literals_and_conditions_are_stripped_before_judging() {
        assert_eq!(strip_tokens("yyyy\"年\"m\"月\"d\"日\""), "yyyymd");
        assert_eq!(strip_tokens("[Red]0.00"), "0.00");
        assert_eq!(kind_of("\"yyyy\""), Kind::General);
        assert_eq!(kind_of("\"G/通用格式\""), Kind::General);
    }

    /// `h:mm` 是时间不是日期；`m/d/yy h:mm` 两者都是
    #[test]
    fn the_letter_m_only_means_month_when_there_is_no_time_mark() {
        assert_eq!(kind_of("h:mm"), Kind::Time);
        assert_eq!(kind_of("mm:ss"), Kind::Time);
        assert_eq!(kind_of("yyyy-mm"), Kind::Date);
        assert_eq!(kind_of("m/d/yy h:mm"), Kind::Datetime);
        assert_eq!(kind_of("0.00%"), Kind::Percent);
        assert_eq!(kind_of("$#,##0.00"), Kind::Currency);
        assert_eq!(kind_of("#,##0.00"), Kind::Number);
        assert_eq!(kind_of("General"), Kind::General);
        assert_eq!(kind_of("0"), Kind::Number);
    }

    /// 序列数换日期：1900 系统的闰年 bug 与 1904 系统各算各的
    #[test]
    fn serial_numbers_become_dates_on_the_basis_the_file_states() {
        assert_eq!(serial_to_iso(41_631.0, false), "2013-12-23");
        assert_eq!(
            serial_to_iso(41_631.635_416_666_66, false),
            "2013-12-23T15:15:00"
        );
        assert_eq!(serial_to_iso(1.0, false), "1900-01-01");
        assert_eq!(serial_to_iso(59.0, false), "1900-02-28");
        assert_eq!(
            serial_to_iso(60.0, false),
            "1900-02-29",
            "不存在的日子照文件报"
        );
        assert_eq!(serial_to_iso(61.0, false), "1900-03-01");
        assert_eq!(serial_to_iso(0.0, true), "1904-01-01");
        assert_eq!(serial_to_iso(46_288.0, true), "2030-09-24");
    }

    /// openpyxl 写的真件：绕开 cellXfs 就一个日期都认不出来
    #[test]
    fn a_real_workbook_hides_its_dates_behind_a_style_index() {
        let bytes = bytes_of("formats.xlsx");
        let styles = read_styles(&bytes);
        assert!(!styles.year1904, "{:?}", styles.notes);
        assert!(styles.notes.is_empty(), "{:?}", styles.notes);
        assert_eq!(styles.xfs.len(), 6, "{:?}", styles.xfs);
        // 1 号样式不是内置 14，而是自定义 164 —— 只查内置表就会漏掉
        assert_eq!(styles.format_of(1), (164, Some("yyyy-mm-dd".to_string())));
        assert_eq!(styles.kind_of(1), Kind::Date);
        assert_eq!(styles.kind_of(2), Kind::Datetime);
        assert_eq!(styles.kind_of(3), Kind::Percent);
        assert_eq!(styles.kind_of(4), Kind::Currency);
        assert_eq!(
            styles.kind_of(5),
            Kind::Date,
            "带汉字字面量的自定义日期格式"
        );
        assert_eq!(styles.kind_of(0), Kind::General);
        let one = styles.cell_format(1, Some("41631"), "n");
        assert_eq!(one["as_date"], "2013-12-23", "{one}");
        assert_eq!(one["format_kind"], "date");
        assert_eq!(one["num_fmt"], 164);
        assert_eq!(one["format"], "yyyy-mm-dd");
        assert_eq!(one["style"], 1);
        // 没写样式的格子（C7 那个文本）走 0 号：General，不会被猜成日期
        let text = styles.cell_format(0, Some("12/23/2013"), "inlineStr");
        assert_eq!(text["format_kind"], "text", "{text}");
        assert!(text.get("as_date").is_none(), "{text}");
        // 数字格子按格式判，日期格子才带 as_date
        assert_eq!(
            styles.cell_format(3, Some("0.125"), "n")["format_kind"],
            "percent"
        );
        assert_eq!(
            styles.cell_format(2, Some("41631.63541666666"), "n")["as_date"],
            "2013-12-23T15:15:00"
        );
    }

    /// 内置号里地区相关的那批没抄串，就说 unknown —— 报成 general 是替文件编话
    #[test]
    fn an_untranscribed_builtin_says_unknown_not_general() {
        let styles = Styles {
            xfs: vec![0, 14, 47, 31],
            custom: BTreeMap::new(),
            year1904: false,
            notes: Vec::new(),
        };
        assert_eq!(styles.kind_of(0), Kind::General);
        assert_eq!(styles.kind_of(1), Kind::Date, "内置 14 = mm-dd-yy");
        assert_eq!(styles.kind_of(2), Kind::Time, "内置 47 = mmss.##");
        let unknown = styles.cell_format(3, Some("41631"), "n");
        assert_eq!(unknown["format_kind"], "unknown", "{unknown}");
        assert_eq!(
            unknown["format"],
            Value::Null,
            "串没抄就别给一条串：{unknown}"
        );
        assert_eq!(unknown["num_fmt"], 31, "号还是照文件给：{unknown}");
        assert!(
            unknown.get("as_date").is_none(),
            "判定不知道就不换算：{unknown}"
        );
    }

    /// 没有 styles.xml 的包要说出来，而不是安静地全给 General
    #[test]
    fn a_package_without_styles_says_so() {
        let bytes = bytes_of("notes.docx");
        let styles = read_styles(&bytes);
        assert!(!styles.notes.is_empty(), "{:?}", styles.notes);
        assert_eq!(styles.format_of(3).0, 0);
    }
}
