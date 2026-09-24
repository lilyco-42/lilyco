//! 「那张纸」写着什么：docx 的 `w:sectPr`、odt 的 `style:page-layout-properties`、
//! RTF 文档级的那一串 `\paperw`。三家各写各的单位 —— docx 与 RTF 写 twips（1/1440 英寸），
//! ODF 写 `21.59cm` 这种自带单位的十进制串 —— 这里统一换成 **0.1mm 的整数**。
//!
//! 为什么不用浮点毫米：浮点在最后一位上会因式子的写法而不同，而第二读者
//! （`scripts/acceptance/lyco_pages.py`）要用同一条式子逐位对上。整数 + 分数单位
//! 就没有这件事。舍入是「逢半进一」，不用银行家舍入 —— 那在 .5 上两边会分家。
//!
//! 每一条都交两份：换算后的整数与**文件自己写的那一串**（`written`）。
//! 生产者之间会不一致（同一批字的 notes-hf：docx 上下边距写 1440 twips，
//! LibreOffice 的 odt 与 rtf 两个导出都写 720 / `1.27cm`），所以这里只把三家各自写的
//! 摆出来，不挑一个当「真值」。没写的项交 null，不是 0。

use serde_json::{json, Value};

use crate::xmlscan::Node;

/// 每一条长度都换成这个单位交出来（整数，免得两边读者在浮点最后一位上分家）
pub const UNIT: &str = "0.1mm";

/// 那份账的外壳：单位说一次，纸按条列出来（docx 一份一节，odt 一个真写了尺寸的页布局一条，
/// RTF 只有一条文档默认的）
pub fn ledger(papers: Vec<Value>) -> Value {
    json!({"unit": UNIT, "papers": papers})
}

/// 边距那份账的七个口袋；某一家不写的就交 null
const MARGIN_KEYS: [&str; 7] = [
    "top", "right", "bottom", "left", "header", "footer", "gutter",
];

/// 一 twip 是多少个 0.1mm：254/144（1 英寸 = 254 个 0.1mm = 1440 twips）
const TWIPS: (i64, i64) = (254, 144);

/// ODF 的长度串自带单位，各自的分数（多少个 0.1mm）
const ODF_UNITS: [(&str, (i64, i64)); 4] = [
    ("cm", (1000, 1)),
    ("mm", (100, 1)),
    ("in", (254, 1)),
    ("pt", (127, 36)),
];

/// `12240` / `21.59` 这种十进制串展开成 (整数, 缩放)，全程不用浮点。
/// 负数、空串、`12.` 这种点半后面没数字的、位数长到会溢出的，一律 None ——
/// 那不是一个我们敢换算的长度
fn decimal(raw: &str) -> Option<(i64, i64)> {
    let raw = raw.trim();
    if raw.is_empty() || raw.starts_with('-') || raw.len() > 16 {
        return None;
    }
    let (head, tail) = match raw.split_once('.') {
        Some((front, back)) => (front, back),
        None => (raw, ""),
    };
    if head.is_empty() || head.len() > 9 || !head.bytes().all(|one| one.is_ascii_digit()) {
        return None;
    }
    // 「12.」在两个读者里都不算长度：点后面得有数字
    if raw.contains('.') && tail.is_empty() {
        return None;
    }
    if tail.len() > 6 || !tail.bytes().all(|one| one.is_ascii_digit()) {
        return None;
    }
    let digits = format!("{head}{tail}").parse::<i64>().ok()?;
    Some((digits, 10i64.pow(tail.len() as u32)))
}

/// (值, 缩放) 乘一个分数单位，逢半进一：与 python 那边的 `(2*a'+b)//(2*b)` 同一条式子
fn convert(digits: i64, scale: i64, unit: (i64, i64)) -> Option<i64> {
    let (num, den) = unit;
    let a = digits.checked_mul(num)?.checked_mul(2)?;
    let b = scale.checked_mul(den)?;
    (a + b).checked_div(2 * b)
}

/// twips（docx 与 RTF 的单位）换成 0.1mm
pub fn twips(raw: &str) -> Option<i64> {
    let (digits, scale) = decimal(raw)?;
    convert(digits, scale, TWIPS)
}

/// ODF 那种自带单位的长度串换成 0.1mm。单位不认识就 None（不猜它是厘米）
pub fn length(raw: &str) -> Option<i64> {
    let raw = raw.trim();
    for (name, unit) in ODF_UNITS {
        if let Some(head) = raw.strip_suffix(name) {
            let (digits, scale) = decimal(head.trim_end())?;
            return convert(digits, scale, unit);
        }
    }
    None
}

/// 从这个元素上按**局部名**取一个属性（前缀是各家自己绑的，不认前缀）
fn attr_from(node: Option<&Node>, local: &str) -> Option<&str> {
    node.and_then(|one| one.attr_local(local))
}

/// 文件自己写的那一串（换算之前）。`unit` 是「用什么单位写的」这个标签：
/// docx 与 RTF 是 `twips`，ODF 的串里已经带着单位，所以那个标签交 null
struct Written<'a> {
    unit: Option<&'a str>,
    width: Option<&'a str>,
    height: Option<&'a str>,
    orient: Option<&'a str>,
    margins: [Option<&'a str>; 7],
}

fn shown(raw: Option<&str>) -> Value {
    match raw {
        Some(one) => Value::String(one.to_string()),
        None => Value::Null,
    }
}

/// 一条纸的账：换算值与文件写的那一串并排
fn entry(from: &str, section: usize, conv: fn(&str) -> Option<i64>, one: Written<'_>) -> Value {
    let mut margins = serde_json::Map::new();
    let mut written = serde_json::Map::new();
    for (index, key) in MARGIN_KEYS.iter().enumerate() {
        let raw = one.margins[index];
        margins.insert((*key).to_string(), json!(raw.and_then(conv)));
        written.insert((*key).to_string(), shown(raw));
    }
    json!({
        "from": from,
        "section": section,
        "width": json!(one.width.and_then(conv)),
        "height": json!(one.height.and_then(conv)),
        // 方向只交文件写的那个值：docx 与 RTF 默认竖排时**不写**这一项，
        // 于是它们是 null，而 odt 会明写 portrait —— 三种存法各说各的
        "orient": shown(one.orient),
        "margins": Value::Object(margins),
        "written": {
            "unit": shown(one.unit),
            "width": shown(one.width),
            "height": shown(one.height),
            "orient": shown(one.orient),
            "margins": Value::Object(written),
        },
    })
}

/// OOXML：一个 `w:sectPr` 一张纸。前面每一节的住在某个段的 `w:pPr` 里，
/// 最后那一节是 `w:body` 的直属孩子 —— `descendants` 两种都收得到（实测 notes-hf.docx 两个）
pub fn ooxml(body: &Node, limit: usize) -> Vec<Value> {
    body.descendants("sectPr")
        .into_iter()
        .take(limit)
        .enumerate()
        .map(|(index, sect)| {
            // 一个 sectPr 里真出现两个 pgSz 是坏件；与第二读者同一个取法（最后一个）
            let size = sect.descendants("pgSz").into_iter().last();
            let paper_box = sect.descendants("pgMar").into_iter().last();
            entry(
                "word/document.xml",
                index,
                twips,
                Written {
                    unit: Some("twips"),
                    width: attr_from(size, "w"),
                    height: attr_from(size, "h"),
                    orient: attr_from(size, "orient"),
                    margins: [
                        attr_from(paper_box, "top"),
                        attr_from(paper_box, "right"),
                        attr_from(paper_box, "bottom"),
                        attr_from(paper_box, "left"),
                        attr_from(paper_box, "header"),
                        attr_from(paper_box, "footer"),
                        attr_from(paper_box, "gutter"),
                    ],
                },
            )
        })
        .collect()
}

/// ODF：纸在 styles.xml 的 `style:page-layout-properties` 上。
/// 只数**真写了 page-width** 的那些 —— LibreOffice 还会写一条只有网格设置的占位属性，
/// 那条不是一张纸（序号也只给真纸，与第二读者同一条规则）
pub fn odf(styles: &Node, limit: usize) -> Vec<Value> {
    styles
        .descendants("page-layout-properties")
        .into_iter()
        .filter(|one| one.attr_local("page-width").is_some())
        .take(limit)
        .enumerate()
        .map(|(index, one)| {
            entry(
                "styles.xml",
                index,
                length,
                Written {
                    unit: None,
                    width: one.attr_local("page-width"),
                    height: one.attr_local("page-height"),
                    orient: one.attr_local("print-orientation"),
                    margins: [
                        one.attr_local("margin-top"),
                        one.attr_local("margin-right"),
                        one.attr_local("margin-bottom"),
                        one.attr_local("margin-left"),
                        None,
                        None,
                        None,
                    ],
                },
            )
        })
        .collect()
}

/// 一串控制字里按词找**第一次**写的那个值
fn written_of(writes: &[(String, String)], word: &str) -> Option<&str> {
    writes
        .iter()
        .find(|(one, _)| one == word)
        .map(|(_, value)| value.as_str())
}

/// RTF：文档级那一串属性（`\paperw12240\paperh15840\margl1800…`），所以只有一条。
/// `writes` 来自 `Rtf::paper_writes`：每个词只留**没被跳过的那一层**里第一次写的那一个 ——
/// 后面 `{\*\sectx …}` 里的那些是某一节的覆写，而这一族不判分节归属（`sections` 仍是 null）。
/// `\landscape` 是个旗标（没有数字参数），写了就交 orient
pub fn rtf(writes: &[(String, String)]) -> Value {
    entry(
        "rtf-stream",
        0,
        twips,
        Written {
            unit: Some("twips"),
            width: written_of(writes, "paperw"),
            height: written_of(writes, "paperh"),
            orient: if written_of(writes, "landscape").is_some() {
                Some("landscape")
            } else {
                None
            },
            margins: [
                written_of(writes, "margt"),
                written_of(writes, "margr"),
                written_of(writes, "margb"),
                written_of(writes, "margl"),
                None,
                None,
                None,
            ],
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Vec<u8> {
        std::fs::read(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/office")
                .join(name),
        )
        .expect("读 fixture")
    }

    /// 同一张 Letter 的三种写法必须换成同一个整数（这是这条链唯一的地基）
    #[test]
    fn the_same_paper_in_three_spellings_is_one_number() {
        assert_eq!(twips("12240"), Some(21590));
        assert_eq!(length("21.59cm"), Some(21590));
        assert_eq!(twips("15840"), Some(27940));
        assert_eq!(length("27.94cm"), Some(27940));
        assert_eq!(twips("1800"), Some(3175));
        assert_eq!(length("3.175cm"), Some(3175));
        assert_eq!(twips("720"), Some(1270));
        assert_eq!(length("1.27cm"), Some(1270));
        assert_eq!(twips("1440"), Some(2540));
        assert_eq!(length("0mm"), Some(0));
    }

    /// 其它单位与别的进位：一英寸就是 254 个 0.1mm；磅按分数走，不查表
    #[test]
    fn other_units_take_their_own_fraction() {
        assert_eq!(length("1in"), Some(254));
        assert_eq!(length("72pt"), Some(254));
        assert_eq!(length("10mm"), Some(100));
        assert_eq!(length("0.5in"), Some(127));
    }

    /// 不像长度的串一律 None：不猜单位，也不把坏值换算成一个看着像数的数
    #[test]
    fn a_string_that_is_not_a_length_stays_unconverted() {
        for raw in [
            "", " ", "abc", "-100", "12.", "1 2", "21.59fy", "21.59", "cm",
        ] {
            assert_eq!(length(raw), None, "{raw}");
        }
        for raw in ["", "abc", "-100", "12.", "1 2", "0x10"] {
            assert_eq!(twips(raw), None, "{raw}");
        }
    }

    /// docx：一个 sectPr 一条纸，notes-hf.docx 有两节（第二个读者同样数到两个）
    #[test]
    fn ooxml_reports_one_paper_per_section() {
        let bytes = fixture("notes-hf.docx");
        let member = crate::zipread::member(
            &bytes,
            "word/document.xml",
            crate::zipread::DEFAULT_MEMBER_CAP,
        )
        .expect("部件读得出");
        let root = crate::xmlscan::parse_str(&member.as_text());
        let body = root
            .child("document")
            .and_then(|one| one.child("body"))
            .expect("有 body");
        let got = ooxml(body, 100);
        assert_eq!(got.len(), 2, "{got:?}");
        for (index, one) in got.iter().enumerate() {
            assert_eq!(one["from"], "word/document.xml", "{one}");
            assert_eq!(one["section"], json!(index), "{one}");
            assert_eq!(one["width"], 21590, "{one}");
            assert_eq!(one["height"], 27940, "{one}");
            assert!(one["orient"].is_null(), "竖排时 docx 不写这一项：{one}");
            assert_eq!(one["margins"]["top"], 2540, "{one}");
            assert_eq!(one["margins"]["right"], 3175, "{one}");
            assert_eq!(one["margins"]["header"], 1270, "{one}");
            assert_eq!(one["margins"]["gutter"], 0, "{one}");
            assert_eq!(one["written"]["unit"], "twips", "{one}");
            assert_eq!(one["written"]["width"], "12240", "{one}");
            assert_eq!(one["written"]["margins"]["top"], "1440", "{one}");
        }
    }

    /// odt：那条只写网格设置的占位属性不是一张纸，所以序号也不给它
    #[test]
    fn odf_skips_the_placeholder_layout() {
        let bytes = fixture("notes.odt");
        let member =
            crate::zipread::member(&bytes, "styles.xml", crate::zipread::DEFAULT_MEMBER_CAP)
                .expect("部件读得出");
        let root = crate::xmlscan::parse_str(&member.as_text());
        let got = odf(&root, 100);
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!(got[0]["width"], 21590, "{:?}", got[0]);
        assert_eq!(got[0]["height"], 27940, "{:?}", got[0]);
        assert_eq!(got[0]["orient"], "portrait", "odf 明写方向：{:?}", got[0]);
        assert_eq!(got[0]["margins"]["top"], 2540, "{:?}", got[0]);
        assert_eq!(got[0]["margins"]["right"], 3175, "{:?}", got[0]);
        // odt 的边距只有四边：页眉页脚的距离不在这一项上
        assert!(got[0]["margins"]["header"].is_null(), "{:?}", got[0]);
        // 串里已带单位，所以那个标签交 null；写的那一串照原样交
        assert!(got[0]["written"]["unit"].is_null(), "{:?}", got[0]);
        assert_eq!(got[0]["written"]["width"], "21.59cm", "{:?}", got[0]);
    }

    /// RTF 的那一串文档级属性：与同一批字的 docx 换成同一个数
    #[test]
    fn rtf_reads_the_document_level_paper() {
        let one = crate::rtf::extract(&fixture("notes.rtf"));
        let got = rtf(&one.paper_writes);
        assert_eq!(got["width"], 21590, "{got}");
        assert_eq!(got["height"], 27940, "{got}");
        assert_eq!(got["margins"]["top"], 2540, "{got}");
        assert_eq!(got["margins"]["left"], 3175, "{got}");
        assert!(got["margins"]["header"].is_null(), "{got}");
        assert!(got["orient"].is_null(), "没写 landscape 就是 null：{got}");
        assert_eq!(got["written"]["width"], "12240", "{got}");
        // 换了边距的那一份：只有上下边距不同（这是文件写的，不是我们算出来的）
        let hf = crate::rtf::extract(&fixture("notes-hf.rtf"));
        let hfp = rtf(&hf.paper_writes);
        assert_eq!(hfp["margins"]["top"], 1270, "{hfp}");
        assert_eq!(hfp["margins"]["right"], 3175, "{hfp}");
    }

    /// 什么都没写的那一串：每条都是 null，而不是 0（「没写」与「写的是 0」两回事）
    #[test]
    fn a_stream_that_wrote_nothing_says_so() {
        let one = crate::rtf::extract(b"{\\rtf1\\ansi one line\\par}");
        assert!(one.paper_writes.is_empty(), "{:?}", one.paper_writes);
        let got = rtf(&one.paper_writes);
        assert!(got["width"].is_null() && got["height"].is_null(), "{got}");
        assert!(got["margins"]["top"].is_null(), "{got}");
        assert_eq!(got["written"]["unit"], "twips", "{got}");
    }

    /// `\landscape` 是旗标：没有数字参数也算写了
    #[test]
    fn the_landscape_flag_has_no_number_but_still_says_landscape() {
        let one = crate::rtf::extract(
            b"{\\rtf1\\ansi\\paperw15840\\paperh12240\\landscape wide page\\par}",
        );
        let got = rtf(&one.paper_writes);
        assert_eq!(got["orient"], "landscape", "{got}");
        assert_eq!(got["width"], 27940, "{got}");
        assert_eq!(got["height"], 21590, "{got}");
    }

    /// 跳过区与已知目标群里的那些都不是文档默认值：`\header` 群里写的 paperw 不算
    #[test]
    fn paper_words_inside_another_destination_are_not_the_default() {
        let one = crate::rtf::extract(
            b"{\\rtf1\\ansi{\\header\\paperw1\\paperh2 header text}{\\paperw12240\\paperh15840 body\\par}",
        );
        let got = rtf(&one.paper_writes);
        assert_eq!(got["width"], 21590, "{got}");
        assert_eq!(got["height"], 27940, "{got}");
        assert_eq!(one.paper_writes.len(), 2, "{:?}", one.paper_writes);
    }
}
