//! MS-OPS（变长属性 / OLE 属性集）：`.doc` / `.xls` / `.ppt` 的「文件属性」对话框背后那套字节。
//!
//! 布局（全部小端，偏移都是相对属性集流的起点）：
//!
//! ```text
//! 0   byteOrder(2)=0xFFFE  format(2)=0  osVersion(4)  CLSID(16)   = 24 字节
//! 24  numPropertySets(4)
//! 28  { fmtid(16), offset(4) } × numPropertySets
//!        ↓ offset 指向一个 section
//!     cbSection(4)  numProperties(4)  { pid(4), valueOffset(4) } × n
//!     …值区在 section 起点 + valueOffset：vt(4) + 该类型的正文
//! ```
//!
//! 两个真踩过的坑，写成注释钉在这里：
//! 1. **24 与 28 别记错**：把 FMTID 的前四个字节当成属性集个数，会得到一个四十亿的循环；
//! 2. **`VT_LPSTR` 的字符集是文件自己说的**：PID 1（CodePage）是 `VT_I2`，LibreOffice 写
//!    65001（UTF-8）、旧版 Word 写 1252。按 latin-1 硬解，中文标题会变成
//!    「å­£åº¦é¢ç®è¯´æ」这种碎字 —— 而这恰好是这段代码最容易装作成功的输出。
//!
//! 值类型认不全时不猜：把 `{vt, note}` 交出去，让人看得见「这里有个我不认的类型」。

use serde_json::{json, Value};

use crate::read::{le16, le32, le64};

/// 已知的 FMTID（按流里小端序列化后的十六进制写，不按 GUID 的括号写法：
/// {D5CDD502-…} 在字节里就是 d5cdd502 2e9c 101b 9397 08002bcf9ae）
pub const FMTID_SUMMARY: &str = "e0859ff2f94f6810ab9108002b27b3d9";
pub const FMTID_DOC_SUMMARY: &str = "d5cdd5022e9c101b939708002bcf9ae";
pub const FMTID_CUSTOM_DOCPROPS: &str = "d5cdd5052e9c101b939708002bcf9ae";

/// SummaryInformation（PID 1 是 CodePage，所以标题从 2 开始 —— 别按 1=标题写）
pub fn summary_pid_name(pid: u32) -> Option<&'static str> {
    Some(match pid {
        1 => "codepage",
        2 => "title",
        3 => "subject",
        4 => "author",
        5 => "keywords",
        6 => "comments",
        7 => "template",
        8 => "last-author",
        9 => "revision",
        10 => "edit-time",
        11 => "last-printed",
        12 => "created",
        13 => "last-saved",
        14 => "page-count",
        15 => "word-count",
        16 => "char-count",
        17 => "thumbnail",
        18 => "application",
        19 => "doc-security",
        _ => return None,
    })
}

/// 按 FMTID 起组名；认不得的照实说 unknown，不硬编一个像样的名字
pub fn fmtid_label(fmtid: &str) -> &'static str {
    match fmtid {
        FMTID_SUMMARY => "SummaryInformation",
        FMTID_DOC_SUMMARY => "DocSummaryInformation",
        FMTID_CUSTOM_DOCPROPS => "CustomDocProps",
        _ => "unknown",
    }
}

/// 一个属性：pid + 变体类型 + 已经按文件声明的字符集解好的值
pub struct Prop {
    pub pid: u32,
    pub name: Option<&'static str>,
    pub vt: u32,
    pub value: Value,
}

pub struct Set {
    pub fmtid: String,
    pub label: &'static str,
    pub props: Vec<Prop>,
}

/// 解一个序列化属性集流（通常就是 `\005SummaryInformation` 那条流的内容）
pub fn decode(bytes: &[u8]) -> Result<Vec<Set>, String> {
    if bytes.len() < 28 {
        return Err(format!(
            "属性集头部只有 {} 字节，放不下 28 字节的固定头部",
            bytes.len()
        ));
    }
    if le16(0)(bytes).unwrap_or(0) != 0xFFFE {
        return Err(format!(
            "字节序标记是 {:#x}，属性集只允许小端（0xFFFE）",
            le16(0)(bytes).unwrap_or(0)
        ));
    }
    let count = le32(24)(bytes).unwrap_or(0) as usize;
    let mut out: Vec<Set> = Vec::new();
    // 头部 24 字节之后是「属性集个数」，再之后才是 20 字节一条的 {FMTID, 偏移} 表
    for index in 0..count {
        let table_at = 28 + index * 20;
        if bytes.len() < table_at + 20 {
            out.push(Set {
                fmtid: String::new(),
                label: "truncated",
                props: vec![Prop {
                    pid: 0,
                    name: None,
                    vt: 0,
                    value: json!({"note": format!("第 {index} 个 FMTID 表项越界")}),
                }],
            });
            break;
        }
        let fmtid = fmtid_hex(&bytes[table_at..table_at + 16]);
        let section = le32(table_at + 16)(bytes).unwrap_or(0) as usize;
        if bytes.len() < section + 8 {
            out.push(Set {
                fmtid,
                label: "truncated",
                props: vec![Prop {
                    pid: 0,
                    name: None,
                    vt: 0,
                    value: json!({"note": format!("属性集起点 {section} 越界")}),
                }],
            });
            continue;
        }
        let declared = le32(section)(bytes).unwrap_or(0);
        let _ = declared;
        let n = le32(section + 4)(bytes).unwrap_or(0) as usize;
        let mut pids: Vec<(u32, usize)> = Vec::new();
        for i in 0..n {
            let at = section + 8 + i * 8;
            if bytes.len() < at + 8 {
                break;
            }
            let pid = le32(at)(bytes).unwrap_or(0) as u32;
            let off = le32(at + 4)(bytes).unwrap_or(0) as usize;
            pids.push((pid, off));
        }
        // 字符集由 PID 1（CodePage, VT_I2）说；没写就按 cp1252 兜底并说明是兜底
        let mut codepage: u64 = 1252;
        for (pid, off) in &pids {
            let at = section + off;
            if *pid == 1 && le32(at)(bytes) == Some(2) {
                if let Some(one) = le16(at + 4)(bytes) {
                    codepage = one;
                }
                break;
            }
        }
        let label = fmtid_label(&fmtid);
        let props = pids
            .iter()
            .map(|(pid, off)| one(bytes, section, *pid, *off, codepage, label))
            .collect();
        out.push(Set {
            fmtid,
            label,
            props,
        });
    }
    Ok(out)
}

fn one(
    bytes: &[u8],
    section: usize,
    pid: u32,
    offset: usize,
    codepage: u64,
    label: &'static str,
) -> Prop {
    let at = section + offset;
    let vt = le32(at)(bytes).unwrap_or(0) as u32;
    let value = match vt {
        // VT_I2：计数与字符集都按无符号读（65001 按有符号读会变成 -535，看着像坏数据）
        2 => le16(at + 4)(bytes)
            .map(|one| json!(one))
            .unwrap_or(Value::Null),
        3 => le32(at + 4)(bytes)
            .map(|one| json!((one as i32) as i64))
            .unwrap_or(Value::Null),
        11 => le32(at + 4)(bytes)
            .map(|one| json!(one != 0))
            .unwrap_or(Value::Null),
        19 => le64(at + 4)(bytes)
            .map(|one| json!({ "ole-date": one }))
            .unwrap_or(Value::Null),
        20 => le64(at + 4)(bytes)
            .map(|one| json!((one as i64)))
            .unwrap_or(Value::Null),
        30 => {
            // VT_LPSTR：先按长度切出来，再按文件声明的字符集解
            let n = le32(at + 4)(bytes).unwrap_or(0) as usize;
            let raw = bytes.get(at + 8..at + 8 + n).unwrap_or(&[]);
            let raw = trim_nul(raw);
            let (text, said) = decode_bytes(raw, codepage);
            match said {
                Some(note) => json!({"text": text, "note": note}),
                None => json!(text),
            }
        }
        31 => {
            let n = le32(at + 4)(bytes).unwrap_or(0) as usize;
            let raw = bytes.get(at + 8..at + 8 + n * 2).unwrap_or(&[]);
            let mut units: Vec<u16> = Vec::new();
            let mut i = 0usize;
            while i + 1 < raw.len() {
                units.push(u16::from_le_bytes([raw[i], raw[i + 1]]));
                i += 2;
            }
            let mut text = String::from_utf16_lossy(&units);
            while text.ends_with('\0') {
                text.pop();
            }
            json!(text)
        }
        64 => {
            let ticks = le64(at + 4)(bytes).unwrap_or(0);
            match filetime_iso(ticks) {
                Some(iso) => json!({"filetime": ticks, "utc": iso}),
                None => json!({"filetime": ticks, "utc": Value::Null}),
            }
        }
        65 => {
            let n = le32(at + 4)(bytes).unwrap_or(0);
            json!({"blob-bytes": n})
        }
        // 计数后的字符串向量：Keywords 在旧 Word 里就是这个（VT_VECTOR|VT_LPSTR）
        4126 => vec_of(bytes, at, codepage, false),
        4108 => vec_of(bytes, at, codepage, true),
        other => json!({"vt": other, "note": "这个变体类型本版本不解"}),
    };
    Prop {
        pid,
        name: name_for(label, pid),
        vt,
        value,
    }
}

/// 只有 SummaryInformation 与 CustomDocProps 的 PID 语义在各生产者之间对得上。
/// DocSummaryInformation 对不上：同一个 1 号位，Word 写段落数、LibreOffice 写 CodePage ——
/// 那种情况下硬给一个名字就是替文件编话，所以只报 PID 与变体类型。
fn name_for(label: &'static str, pid: u32) -> Option<&'static str> {
    match label {
        "SummaryInformation" => summary_pid_name(pid),
        "CustomDocProps" if pid > 1 => Some("custom-property"),
        _ => None,
    }
}

fn vec_of(bytes: &[u8], at: usize, codepage: u64, wide: bool) -> Value {
    let n = le32(at + 4)(bytes).unwrap_or(0) as usize;
    let mut cursor = at + 8;
    let mut items: Vec<Value> = Vec::new();
    for _ in 0..n {
        let len = match le32(cursor)(bytes) {
            Some(one) => one as usize,
            None => break,
        };
        cursor += 4;
        let take = if wide { len * 2 } else { len };
        let raw = bytes.get(cursor..cursor + take).unwrap_or(&[]);
        let raw = trim_nul(raw);
        let text = if wide {
            let mut units: Vec<u16> = Vec::new();
            let mut i = 0usize;
            while i + 1 < raw.len() {
                units.push(u16::from_le_bytes([raw[i], raw[i + 1]]));
                i += 2;
            }
            String::from_utf16_lossy(&units)
        } else {
            decode_bytes(raw, codepage).0
        };
        items.push(json!(text));
        cursor += take;
    }
    json!(items)
}

fn trim_nul(raw: &[u8]) -> &[u8] {
    let mut end = raw.len();
    while end > 0 && raw[end - 1] == 0 {
        end -= 1;
    }
    &raw[..end]
}

/// 字符集解释：UTF-8 / cp1252 / latin-1 三档，解不动就用替换字符并把用的是哪档说出来
fn decode_bytes(raw: &[u8], codepage: u64) -> (String, Option<String>) {
    if codepage == 65001 {
        return match std::str::from_utf8(raw) {
            Ok(text) => (text.to_string(), None),
            Err(_) => (
                String::from_utf8_lossy(raw).into_owned(),
                Some("声明是 UTF-8 但字节解不过去，按 lossy 处理".to_string()),
            ),
        };
    }
    if codepage == 1252 || codepage == 0 {
        // cp1252 的前 0x20 段与 latin-1 不同，但这批属性里没有控制字符，
        // 所以按 latin-1 解对可打印区完全等价 —— 记下来免得后人以为漏了
        let mut out = String::new();
        for one in raw {
            out.push(char::from(*one));
        }
        return (out, None);
    }
    let mut out = String::new();
    for one in raw {
        out.push(char::from(*one));
    }
    (
        out,
        Some(format!(
            "字符集 {codepage} 本版本不内置，按 latin-1 逐字节给出"
        )),
    )
}

/// FILETIME（1601-01-01 起的 100ns 数）→ `YYYY-MM-DDTHH:MM:SS`（UTC 时刻，不解释任何人的时区）
pub fn filetime_iso(ticks: u64) -> Option<String> {
    const TO_EPOCH: u64 = 116_444_736_000_000_000;
    if ticks < TO_EPOCH {
        return None;
    }
    let secs = (ticks - TO_EPOCH) / 10_000_000;
    let days = (secs / 86_400) as i64;
    let rest = (secs % 86_400) as i64;
    let (y, m, d) = civil_from_days(days);
    Some(format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}",
        rest / 3600,
        (rest % 3600) / 60,
        rest % 60
    ))
}

/// Howard Hinnant 的 civil_from_days：从 1970-01-01 的天数算回年月日
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn fmtid_hex(raw: &[u8]) -> String {
    raw.iter()
        .map(|one| format!("{one:02x}"))
        .collect::<String>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cfb;

    fn stream(name: &str, which: &str) -> Vec<u8> {
        let b = std::fs::read(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/office")
                .join(name),
        )
        .expect("读 fixture");
        let doc = cfb::open(&b).expect("打开复合文档");
        doc.read(&b, which)
            .unwrap_or_else(|| panic!("{name} 里读不出 {which}"))
    }

    use std::path::PathBuf;

    /// LibreOffice 写的 .doc：标题/主题/作者/关键字/模板/修订号/两个时间戳
    #[test]
    fn reads_summary_information_from_a_real_doc() {
        let sets = decode(&stream("notes.doc", "\u{5}SummaryInformation")).expect("解得开");
        let one = sets
            .iter()
            .find(|one| one.label == "SummaryInformation")
            .expect("有 SummaryInformation 这一组");
        let get = |pid: u32| -> Value {
            one.props
                .iter()
                .find(|one| one.pid == pid)
                .map(|one| one.value.clone())
                .unwrap_or(Value::Null)
        };
        assert_eq!(get(1), json!(65001), "CodePage 按无符号读");
        assert_eq!(get(2), json!("季度预算说明"));
        assert_eq!(get(3), json!("季度预算"));
        assert_eq!(get(4), json!("liuqi"));
        assert_eq!(get(5), json!("budget, quarterly"));
        assert_eq!(get(7), json!("Normal.dotm"));
        assert_eq!(get(9), json!("1"));
        assert_eq!(
            get(12),
            json!({"filetime": 130322853000000000u64, "utc": "2013-12-23T15:15:00"})
        );
        assert_eq!(one.props[0].name, Some("codepage"), "PID 要有名字");
    }

    /// 同一批属性在 .xls / .ppt 里也在，但值不一样：不能只对一份文件成立
    #[test]
    fn the_same_layout_works_for_excel_and_powerpoint() {
        let sets = decode(&stream("book.xls", "\u{5}SummaryInformation")).expect("解得开");
        let one = sets
            .iter()
            .find(|one| one.label == "SummaryInformation")
            .expect("有一组");
        let title = one.props.iter().find(|one| one.pid == 2).expect("有标题");
        assert_eq!(title.value, json!("季度预算说明"));

        let sets = decode(&stream("deck.ppt", "\u{5}SummaryInformation")).expect("解得开");
        let one = sets
            .iter()
            .find(|one| one.label == "SummaryInformation")
            .expect("有一组");
        let keys: Vec<String> = one
            .props
            .iter()
            .filter(|one| one.pid == 5)
            .map(|one| one.value.as_str().unwrap_or_default().to_string())
            .collect();
        assert_eq!(keys, vec!["budget, quarterly".to_string()]);
    }

    /// 大文件里的属性集可以很大（.ppt 那份 59 万字节，因为预览图塞在里面）：
    /// 解它不能靠「先整个读进内存再切」以外的假设，也不能因为体积大就报错
    #[test]
    fn a_huge_property_set_still_decodes() {
        let raw = stream("deck.ppt", "\u{5}SummaryInformation");
        assert!(raw.len() > 500_000, "fixture 变了？{}", raw.len());
        let sets = decode(&raw).expect("大也得解");
        assert!(!sets.is_empty());
    }

    /// 头不足 28 字节时报错，而不是把读不到的个数当成 0 编出一个「什么都没写」的假象
    #[test]
    fn an_incomplete_header_is_an_error_not_a_zero_set_count() {
        let why = decode(b"\xfe\xff").unwrap_err();
        assert!(why.contains("固定头部"), "{why}");
    }

    #[test]
    fn a_big_endian_header_is_rejected_and_a_zero_count_is_empty() {
        let mut raw = vec![0u8; 64];
        raw[..2].copy_from_slice(&[0xFF, 0xFE]);
        let why = decode(&raw).unwrap_err();
        assert!(why.contains("字节序"), "{why}");
        let mut raw = vec![0u8; 64];
        raw[..2].copy_from_slice(&[0xFE, 0xFF]);
        assert!(
            decode(&raw).expect("个数读成 0 就是空集").is_empty(),
            "头部说零个就不该编出属性集"
        );
    }

    /// 时间戳换算要有绝对锚点：1601-01-01 之前的值与已知的 epoch 都当场验
    #[test]
    fn filetimes_convert_against_known_points() {
        assert_eq!(filetime_iso(0), None);
        assert_eq!(
            filetime_iso(116_444_736_000_000_000).as_deref(),
            Some("1970-01-01T00:00:00")
        );
        assert_eq!(
            filetime_iso(130_322_853_000_000_000).as_deref(),
            Some("2013-12-23T15:15:00")
        );
        // 2026-09-23 也是闰年后的一段，顺便验年月日换算不吃天
        assert_eq!(
            filetime_iso(134_346_299_470_000_000).as_deref(),
            Some("2026-09-23T09:39:07")
        );
    }
}
