//! 这份稿子有多少字 —— 生产者自报的那一份与正文实算的那一份并排交。
//!
//! OOXML 把声明写在 `docProps/app.xml` 的七个名上（`Words` / `Paragraphs` / `Slides` /
//! `Notes` / `HiddenSlides` / `MMClips` / `TotalTime`），而「实算」有三个口径：只数
//! `ppt/slides/slideN.xml`、页 + 备注、包里所有带 `a:t` 的部件（版式与母版里也有字）。
//! 生产者到底数了哪些部件，文件里没写 —— 所以三个都交、三个各自打等号，谁也不选。
//!
//! ODF 那一族的声明住在 `meta.xml` 的 `meta:document-statistic`：整枚元素的属性按写的交
//! （属性名去掉前缀、值全是串，与 `office-doc` 那一族同一口径，本仓不换算也不补齐）。
//!
//! 实测（`stats.pptx` = python-pptx 打底 + 按 ECMA 手写 `docProps/app.xml`；`stats-lo.pptx` =
//! LibreOffice 重写同一份；`stats.odp` = LibreOffice 转出去的那一份）：
//! 1. **真件里没有一份自报的字数对得上**（本机 104 份 pptx，102 份带 app.xml）：`Words`
//!    与三个口径的实算切词都比，**96 份可比、0 份对**；`Paragraphs` 与 `a:p` 条数比，
//!    **98 份可比、0 份对**；而 `Slides` 与页部件数对 **101/102**、`Notes` 与备注部件数对
//!    **102/102** —— 「数得清件数的都对，数不清字的都不对」；另有 6 份带了 app.xml
//!    却没写 `Words`，2 份整件没有 app.xml（那种件的这一格是 `null`，不是 0）；
//! 2. 78/102 份的 `Words` 写着 `0` 而正文有字（103/104 份的页部件有字）——
//!    「自报 0」是「没数过」，不是「没有字」，所以两份账谁也不许盖掉谁；
//! 3. 版式与母版里有没有字是**生产者**差异，不是文档差异：真件只有 3/104 份有，而
//!    python-pptx 那份模板给每份自产件写了 12 个部件、1369 个字符（`others`），
//!    于是「按所有部件实算」在自产件上是正文的二十倍（1440 对 61）；
//! 4. LibreOffice 重写同一份 pptx 时把**手写的** `Words`（999999）与 `Paragraphs`（7）原样搬过去，
//!    却把 `Slides` / `Notes` / `HiddenSlides` / `MMClips` 四个名整个丢掉 —— 那两个本来
//!    对得上的数于是从 `true` / `false` 变成 `null`（「没写」与「写了而不对」不是一回事）；
//!    同一份件它把 7 枚 `a:t` 拆成 9 枚，而字符数 61 一个字没变；
//! 5. odp 那一族的「生产者声明」只有一条 `object-count`（17 份自产 odp 全是这一个名，值
//!    28~150，另有一份连 `meta.xml` 都没有），字数、字符数、段落数、页数**一条都没写**；
//!    同一件转成 odt 时那枚元素写着八条 —— 放映这一族唯一自报的那个数与「多少字」无关；
//! 6. odp 的备注住在 `draw:page` **里面**（`presentation:notes` 是页的孩子），所以「按页实算」
//!    与「按整份件实算」是同一个数（`stats.odp` 两处都是 75 / 66 / 18 / 9），而 pptx 的备注在
//!    另一个部件里、`ours` 不算它 —— 两族对「备注算不算正文」的答案不同，所以两处都交。
//!
//! 不做的事：**不换算也不判谁对**（自报的照抄，实算的按口径交）、**不猜生产者数了哪些部件**
//! （三个口径并排，等号各自打）、**遗留 .ppt 不交这一格**（它的自报数在 `SummaryInformation`
//! 的属性流里，那是 `office-meta` 那条分支读的东西，而本机没有第二个读者能核对这一族的
//! 字数口径 —— 没看过的就没有这一格，而不是交 0）。

use crate::xmlscan::{self, Node};
use crate::zipread;
use serde_json::{json, Value};

/// `docProps/app.xml` 里这一族会写的七个名（交出去的键 → 文件里那个名）
const STAT_NAMES: [(&str, &str); 7] = [
    ("words", "Words"),
    ("paragraphs", "Paragraphs"),
    ("slides", "Slides"),
    ("notes", "Notes"),
    ("hidden_slides", "HiddenSlides"),
    ("mm_clips", "MMClips"),
    ("total_time", "TotalTime"),
];

/// 这一族的部件名（前缀就按写的，`_rels` 一律绕开）
const SLIDE_HEAD: &str = "ppt/slides/slide";
const NOTES_HEAD: &str = "ppt/notesSlides/notesSlide";
const OTHER_HEAD: &str = "ppt/slide";

/// 一枚 properties 之外的两处文字元素名（`a:p` 与 `a:t`，只认局部名）
const STAT_PARA: &str = "p";
const STAT_ATOM: &str = "t";
/// ODF 的段：`text:p` 与 `text:h`（标题也是段，与 odt 那几本同一口径）
const ODF_PARAS: [&str; 2] = ["p", "h"];

/// 实算的那五个数（口径写在键名上：`words_by_space` 只按空白切，一整段中文可能算一个）
#[derive(Clone, Default)]
struct Stat {
    characters: usize,
    no_space: usize,
    tokens: usize,
    paragraphs: usize,
    atoms: usize,
}

impl Stat {
    fn add_text(&mut self, text: &str) {
        self.characters += text.chars().count();
        self.no_space += text.chars().filter(|one| !one.is_whitespace()).count();
        self.tokens += text.split_whitespace().count();
    }

    fn to_json(&self) -> Value {
        json!({
            "characters": self.characters,
            "characters_no_spaces": self.no_space,
            "words_by_space": self.tokens,
            "paragraphs": self.paragraphs,
            "text_atoms": self.atoms,
        })
    }

    /// ODF 没有「文字原子」这一层：那个键干脆不交，而不是交 0
    fn odf_json(&self) -> Value {
        json!({
            "characters": self.characters,
            "characters_no_spaces": self.no_space,
            "words_by_space": self.tokens,
            "paragraphs": self.paragraphs,
        })
    }
}

fn stat_member(bytes: &[u8], part: &str) -> Option<Node> {
    let member = zipread::member(bytes, part, zipread::DEFAULT_MEMBER_CAP).ok()?;
    let text = member.as_text();
    if text.trim().is_empty() {
        return None;
    }
    Some(xmlscan::parse_str(&text))
}

/// 按前缀收部件名，再按**名**排（zip 里的先后不算）—— 与第二读者同一口径
fn stat_names(bytes: &[u8], head: &str) -> Vec<String> {
    let mut out: Vec<String> = zipread::member_names(bytes)
        .into_iter()
        .filter(|one| one.starts_with(head) && one.ends_with(".xml") && !one.contains("_rels"))
        .collect();
    out.sort();
    out
}

/// DrawingML 的一个部件里有几个段、多少字
fn tally_of(root: &Node) -> Stat {
    let mut out = Stat::default();
    out.paragraphs = root.descendants(STAT_PARA).len();
    for one in root.descendants(STAT_ATOM) {
        out.atoms += 1;
        out.add_text(&one.text());
    }
    out
}

/// ODF 的一页（或整份件）里有多少字：段与标题都算一段，整段的字按子树拼
fn odf_tally(host: &Node) -> Stat {
    let mut out = Stat::default();
    for want in ODF_PARAS {
        for one in host.descendants(want) {
            out.paragraphs += 1;
            out.add_text(&one.text());
        }
    }
    out
}

fn merge(into: &mut Stat, got: &Stat) {
    into.characters += got.characters;
    into.no_space += got.no_space;
    into.tokens += got.tokens;
    into.paragraphs += got.paragraphs;
    into.atoms += got.atoms;
}

/// `docProps/app.xml` 自报的那七个名：没写的那个名不进表，写了就按串的数交
fn declared_counts(bytes: &[u8]) -> Option<Value> {
    let root = stat_member(bytes, "docProps/app.xml")?;
    let mut out = serde_json::Map::new();
    for (key, want) in STAT_NAMES.iter() {
        let found = root.descendants(*want);
        let Some(one) = found.first() else {
            continue;
        };
        let raw = one.text().trim().to_string();
        out.insert(
            key.to_string(),
            match raw.parse::<i64>() {
                Ok(number) => json!(number),
                Err(_) => json!(raw),
            },
        );
    }
    Some(Value::Object(out))
}

/// pptx / pptm：三个实算口径 + 自报的那份 + 逐页的行
pub(crate) fn pptx(bytes: &[u8], limit: usize) -> Value {
    let slides = stat_names(bytes, SLIDE_HEAD);
    let notes = stat_names(bytes, NOTES_HEAD);
    let others: Vec<String> = stat_names(bytes, OTHER_HEAD)
        .into_iter()
        .filter(|one| !slides.contains(one) && !notes.contains(one))
        .collect();
    let mut rows: Vec<Value> = Vec::new();
    let mut ours = Stat::default();
    let mut notes_tally = Stat::default();
    let mut other_tally = Stat::default();
    for name in slides.iter() {
        let got = match stat_member(bytes, name) {
            Some(root) => tally_of(&root),
            None => Stat::default(),
        };
        merge(&mut ours, &got);
        if rows.len() < limit {
            let mut row = match got.to_json() {
                Value::Object(one) => one,
                _ => serde_json::Map::new(),
            };
            row.insert("part".to_string(), json!(name.as_str()));
            row.insert("has_text".to_string(), json!(got.characters > 0));
            rows.push(Value::Object(row));
        }
    }
    for name in notes.iter() {
        if let Some(root) = stat_member(bytes, name) {
            merge(&mut notes_tally, &tally_of(&root));
        }
    }
    for name in others.iter() {
        if let Some(root) = stat_member(bytes, name) {
            merge(&mut other_tally, &tally_of(&root));
        }
    }
    let mut with_notes = notes_tally.clone();
    merge(&mut with_notes, &ours);
    let mut all_parts = other_tally.clone();
    merge(&mut all_parts, &with_notes);
    let declared = declared_counts(bytes);
    // 「没写」交 null，不交 false：那是两件不同的事
    let same = |key: &str, mine: i64| -> Option<bool> {
        let table = match &declared {
            Some(Value::Object(one)) => one,
            _ => return None,
        };
        table.get(key).map(|got| match got.as_i64() {
            Some(number) => number == mine,
            None => false,
        })
    };
    let agree = json!({
        "words": {
            "slides": same("words", ours.tokens as i64),
            "with_notes": same("words", with_notes.tokens as i64),
            "all": same("words", all_parts.tokens as i64),
        },
        "paragraphs": {
            "slides": same("paragraphs", ours.paragraphs as i64),
            "all": same("paragraphs", all_parts.paragraphs as i64),
        },
        "slides": same("slides", slides.len() as i64),
        "notes": same("notes", notes.len() as i64),
    });
    json!({
        "family": "ooxml",
        "available": declared.is_some(),
        "slide_parts": slides.len(),
        "notes_parts": notes.len(),
        "other_text_parts": others.len(),
        "declared": declared,
        "ours": ours.to_json(),
        "notes": notes_tally.to_json(),
        "others": other_tally.to_json(),
        "with_notes": with_notes.to_json(),
        "all_parts": all_parts.to_json(),
        "agree": agree,
        "rows": rows,
        "listed": rows.len(),
        "cut": slides.len() > rows.len(),
    })
}

/// odp：`meta:document-statistic` 照抄 + 正文按页实算（备注在页里面，所以跟着页算）
pub(crate) fn odf(bytes: &[u8], limit: usize) -> Value {
    let root = match stat_member(bytes, "content.xml") {
        Some(one) => one,
        None => {
            return json!({
                "family": "odf", "available": false, "statistic_part": false,
                "statistic_present": false, "declared": Value::Null, "pages": 0,
                "ours": Value::Null, "ours_in_pages": Value::Null,
                "rows": [], "listed": 0, "cut": false,
            })
        }
    };
    let meta = stat_member(bytes, "meta.xml");
    let mut declared: Option<Value> = None;
    let mut present = false;
    if let Some(one) = &meta {
        let found = one.descendants("document-statistic");
        present = !found.is_empty();
        if let Some(holder) = found.first() {
            let mut table = serde_json::Map::new();
            for (key, value) in holder.attrs.iter() {
                let local = key.rsplit(':').next().unwrap_or(key.as_str()).to_string();
                table.insert(local, json!(value));
            }
            declared = Some(Value::Object(table));
        }
    }
    let pages = root.descendants("page");
    let mut rows: Vec<Value> = Vec::new();
    let mut in_pages = Stat::default();
    for (index, page) in pages.iter().copied().enumerate() {
        let got = odf_tally(page);
        merge(&mut in_pages, &got);
        if rows.len() < limit {
            let mut row = match got.odf_json() {
                Value::Object(one) => one,
                _ => serde_json::Map::new(),
            };
            row.insert("page".to_string(), json!(index));
            row.insert("name".to_string(), attr_json(page, "name"));
            row.insert("klass".to_string(), attr_json(page, "class"));
            row.insert("has_text".to_string(), json!(got.characters > 0));
            rows.push(Value::Object(row));
        }
    }
    let ours = odf_tally(&root);
    json!({
        "family": "odf",
        "available": true,
        "statistic_part": meta.is_some(),
        "statistic_present": present,
        "declared": declared,
        "pages": pages.len(),
        "ours": ours.odf_json(),
        "ours_in_pages": in_pages.odf_json(),
        "rows": rows,
        "listed": rows.len(),
        "cut": pages.len() > rows.len(),
    })
}

/// 一枚属性按写的交（没写就是 null，空串是空串）
fn attr_json(node: &Node, want: &str) -> Value {
    match node.attr_local(want) {
        Some(had) => json!(had),
        None => Value::Null,
    }
}
