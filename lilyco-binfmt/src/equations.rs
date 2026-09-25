//! 文档里的公式：OOXML 写成 OMML 挂在段上，ODF 把每条式子装进一个**嵌入对象**的 MathML 部件
//!
//! 形状：`{family, available, items[], <聚合数>}`，两支各一本，另一族的键整个不在。
//! OOXML 一支每条 `{index, paragraph, placement, host, align_written, structures, runs,
//! nor_runs, lit_runs, text}`，聚合 `equations_total / inline_total / display_total /
//! paragraphs_with / paragraphs_total / align_written_total / nor_runs / lit_runs /
//! math_runs / text_chars / o_math_para_total / structures_seen`；
//! ODF 一支每条 `{index, paragraph, frame_name, style_written, anchor_written,
//! width_written, height_written, z_written, object_target, object_part, part_found,
//! math_found, replacement_target, display_written, elements, text, annotation_encoding,
//! annotation_source}`，聚合 `frames_seen / objects_total / math_found /
//! objects_without_math / parts_found / parts_missing / block_written / inline_written /
//! display_missing / annotations_found / replacements_written / text_chars /
//! paragraphs_total / elements_seen`。
//!
//! 实测四份件（`eq.docx` 由 python-docx 挂 OMML 原文写、每段只管一种写法；`eq-lo.docx` 是
//! LibreOffice 的 docx → docx 重写；`eq.odt` 是同一份的 docx → odt；`eq-od.docx` 再从这个 odt
//! 转回 docx）：
//! 1. **「式子占不占一行」是生产者会改的**：种子 3 条行内 + 3 条独立，LibreOffice 重写之后变成
//!    2 条行内 + 4 条独立 —— 它把两条行内式升级成 `m:oMathPara`；所以 `placement` 与 `host`
//!    都按文件写着的交，不照「作者原本想怎样」推断；
//! 2. **对齐值会被换掉**：种子里只有一条自己写了 `m:jc m:val="centerGroup"`，重写那份里它变成
//!    `center`，而另外三条（原本一个字没写）都补上了值 —— `align_written_total` 从 1 变 4。
//!    「没说」与「说了默认」是两件事，这里连「说了 A 改成 B」也要看得见；
//! 3. **`m:nor` 那一条被补了一枚 `m:lit`**：点名成普通字的那一路，LibreOffice 在 `<m:rPr>` 里
//!    同时写 `<m:lit/><m:nor/>`，所以两个数各交一份，不并成一个「普通字」；
//! 4. **一条式子在 odt 里住在另一份部件**：`draw:frame`（`text:anchor-type="as-char"`，尺寸写成
//!    `0.314cm` 这种自带单位的串）里 `draw:object xlink:href="./Object 1"`，式子的字在
//!    `Object 1/content.xml` 的 MathML 里，清单把 `Object 1/` 声明成
//!    `application/vnd.oasis.opendocument.formula`；另有一枚 `draw:image` 指向
//!    `ObjectReplacements/Object 1` 的替位图，两处地址都按写的交（少交一处就有一处没人认）；
//! 5. **行内与独立在 ODF 这一族分不出来**：六条的 `<math>` 一律写 `display="block"`
//!    （`inline_written` 0），所以那一路只交「按写的有几个 block」，不猜原本是哪种；
//! 6. **同一句话在两族的「字」不一样长**：`[n]` 那一条在 OMML 里括号是 `m:d` 的属性
//!    （`m:begChr` / `m:endChr`），`<m:t>` 只有 `n`；到 MathML 里括号成了 `mo` **元素**，于是
//!    式子的字是 `[n]`。两份件的 `text_chars` 因此是 13 与 17 —— 各按自己文件写着的交；
//! 7. **线性式另有存放**：ODF 那个部件里 `<semantics>` 还带一枚
//!    `<annotation encoding="StarMath 5.0">`，写的是 `{a} over {b}` 这种线性源
//!    （`annotation_source` / `annotation_encoding` 按原样交）—— 它不是式子里的字，没算进 `text`。
//!
//! 界（这一本**不**做的那几件事）：
//! - 表格里、脚注尾注部件里的式子不数（两支同一条口径：只看正文段 `w:p` / `text:p|h` 的**直接
//!   孩子**）—— 那些地方住的段不在这本账走的顺序里，硬并会把两本的段号弄乱；
//! - 不做线性化（不生成 LaTeX / UnicodeMath），只交文件自己写着的元素名与字符；
//! - `.doc` / RTF / `.ppt` 不交这个键：那几族把式子内嵌成字段/对象是另一套记号，本机没有能写出
//!   这些件的生产者，量不到就不写那一支。

use crate::xmlscan::Node;
use crate::zipread::{self, DEFAULT_MEMBER_CAP};
use serde_json::{json, Value};

const OMML_URI: &str = "http://schemas.openxmlformats.org/officeDocument/2006/math";

/// MathML 里「算式子里的字」的那四类（与 `lyco_equations.py:MATH_TEXT` 同一份名单）
const MATH_CHARS: [&str; 4] = ["mi", "mn", "mo", "mtext"];

/// 子树里所有元素（`#text` 那种记号的孩子是字，不是元素），按文档顺序
fn subtree<'a>(node: &'a Node, out: &mut Vec<&'a Node>) {
    for one in node.children.iter().filter(|kid| kid.name != "#text") {
        out.push(one);
        subtree(one, out);
    }
}

/// OMML 的前缀是文件自己声明的（本语料的两个生产者都写 `m:`；没声明时按 `m` 读）
fn math_prefix(root: &Node) -> String {
    for one in root.descendants("document").iter() {
        for (key, value) in one.attrs.iter() {
            if value == OMML_URI {
                if let Some(tail) = key.strip_prefix("xmlns:") {
                    return tail.to_string();
                }
            }
        }
    }
    "m".to_string()
}

/// OOXML 那一支的运行态：条目清单、见过的结构名与那些各交各的数
struct DocxRun {
    head: String,
    items: Vec<Value>,
    seen: Vec<String>,
    paragraphs_total: usize,
    with_equation: usize,
    para_hosts: usize,
    inline_total: usize,
    display_total: usize,
    align_written: usize,
    nor_total: usize,
    lit_total: usize,
    run_total: usize,
    char_total: usize,
}

impl DocxRun {
    fn new(head: String) -> DocxRun {
        DocxRun {
            head,
            items: Vec::new(),
            seen: Vec::new(),
            paragraphs_total: 0,
            with_equation: 0,
            para_hosts: 0,
            inline_total: 0,
            display_total: 0,
            align_written: 0,
            nor_total: 0,
            lit_total: 0,
            run_total: 0,
            char_total: 0,
        }
    }

    /// 一条式子：结构名按文档顺序（`oMath` / `r` / `t` 是壳与字，不当结构记），
    /// 字只拼 `m:t`，`m:nor` 与 `m:lit` 各数各的
    fn add(
        &mut self,
        node: &Node,
        paragraph: usize,
        placement: &str,
        host: &str,
        align: Option<String>,
    ) {
        let mut kids: Vec<&Node> = Vec::new();
        subtree(node, &mut kids);
        let mut structures: Vec<String> = Vec::new();
        let mut runs = 0usize;
        let mut nor = 0usize;
        let mut lit = 0usize;
        let mut text = String::new();
        for one in kids.iter() {
            if !one.name.starts_with(&self.head) {
                continue;
            }
            let name = one.local();
            if name == "r" {
                // 数学 run 是「一串字」的壳，不算结构：与 python 那侧同一条排除表
                runs += 1;
                continue;
            }
            if name == "t" {
                text.push_str(&one.text());
                continue;
            }
            if name == "nor" {
                nor += 1;
            }
            if name == "lit" {
                lit += 1;
            }
            if name == "oMath" {
                continue;
            }
            let name = name.to_string();
            structures.push(name.clone());
            if !self.seen.iter().any(|one| *one == name) {
                self.seen.push(name);
            }
        }
        let chars = text.chars().count();
        if placement == "inline" {
            self.inline_total += 1;
        } else {
            self.display_total += 1;
        }
        if align.is_some() {
            self.align_written += 1;
        }
        self.nor_total += nor;
        self.lit_total += lit;
        self.run_total += runs;
        self.char_total += chars;
        self.items.push(json!({
            "index": self.items.len(),
            "paragraph": paragraph,
            "placement": placement,
            "host": host,
            "align_written": match &align {
                Some(raw) => json!(raw),
                None => Value::Null,
            },
            "structures": structures,
            "runs": runs,
            "nor_runs": nor,
            "lit_runs": lit,
            "text": text,
        }));
    }

    fn finish(self, limit: usize) -> Value {
        let total = self.items.len();
        json!({
            "family": "ooxml",
            "available": true,
            "items": self.items.into_iter().take(limit).collect::<Vec<Value>>(),
            "equations_total": total,
            "paragraphs_total": self.paragraphs_total,
            "paragraphs_with": self.with_equation,
            "inline_total": self.inline_total,
            "display_total": self.display_total,
            "align_written_total": self.align_written,
            "nor_runs": self.nor_total,
            "lit_runs": self.lit_total,
            "math_runs": self.run_total,
            "text_chars": self.char_total,
            "o_math_para_total": self.para_hosts,
            "structures_seen": self.seen,
        })
    }
}

/// 独立成行那条的对齐：`m:oMathParaPr/m:jc/@m:val`，没写就是没写
fn jc_of(holder: &Node) -> Option<String> {
    for props in holder
        .children
        .iter()
        .filter(|kid| kid.local() == "oMathParaPr")
    {
        if let Some(hit) = props.all("jc").into_iter().next() {
            return hit.attr_local("val").map(String::from);
        }
    }
    None
}

/// OOXML 那一份：正文段的**直接孩子**里找 `m:oMath`（行内）与 `m:oMathPara`（独立成行）
pub(crate) fn docx(document: &Node, limit: usize) -> Value {
    let body = match document.descendants("body").into_iter().next() {
        Some(had) => had,
        None => {
            return json!({"family": "ooxml", "available": false});
        }
    };
    let mut run = DocxRun::new(math_prefix(document) + ":");
    for (p_index, par) in body
        .children
        .iter()
        .filter(|kid| kid.name != "#text" && kid.local() == "p")
        .enumerate()
    {
        run.paragraphs_total += 1;
        let mut found = 0usize;
        for child in par.children.iter().filter(|kid| kid.name != "#text") {
            match child.local() {
                "oMath" => {
                    found += 1;
                    run.add(child, p_index, "inline", "w:p", None);
                }
                "oMathPara" => {
                    run.para_hosts += 1;
                    found += 1;
                    let align = jc_of(child);
                    for inner in child.all("oMath") {
                        run.add(inner, p_index, "display", "m:oMathPara", align.clone());
                    }
                }
                _ => {}
            }
        }
        if found > 0 {
            run.with_equation += 1;
        }
    }
    run.finish(limit)
}

/// `./Object 3` → `Object 3/content.xml`（地址按写的原样交，部件路径自己拼）
fn member_of(href: &str) -> String {
    let tail = href.strip_prefix("./").unwrap_or(href);
    let clean = tail.strip_prefix('/').unwrap_or(tail).trim_end_matches('/');
    if clean.is_empty() {
        return String::new();
    }
    format!("{}/content.xml", clean)
}

/// 一个公式部件里的 MathML：元素按文档顺序（含根），字只拼那四类，`<annotation>` 单独交
fn mathml_of(
    bytes: &[u8],
    member: &str,
) -> (
    Option<String>,
    Vec<String>,
    String,
    Option<String>,
    Option<String>,
) {
    let had = match zipread::member(bytes, member, DEFAULT_MEMBER_CAP) {
        Ok(one) => one,
        Err(_) => return (None, Vec::new(), String::new(), None, None),
    };
    let root = crate::xmlscan::parse_str(&had.as_text());
    let math = match root.descendants("math").into_iter().next() {
        Some(hit) => hit,
        None => return (None, Vec::new(), String::new(), None, None),
    };
    let mut nodes: Vec<&Node> = vec![math];
    subtree(math, &mut nodes);
    let elements: Vec<String> = nodes.iter().map(|one| one.local().to_string()).collect();
    let mut text = String::new();
    for one in nodes.iter() {
        if MATH_CHARS.contains(&one.local()) {
            text.push_str(&one.text());
        }
    }
    let annotation = math.descendants("annotation").into_iter().next();
    let encoding = annotation
        .and_then(|one| one.attr("encoding"))
        .map(String::from);
    let source = annotation.map(|one| one.text());
    let display = math.attr("display").map(String::from);
    (display, elements, text, encoding, source)
}

/// ODF 那一份：正文段里的 `draw:frame` → `draw:object` → `Object N/content.xml`
pub(crate) fn odf(bytes: &[u8], root: &Node, limit: usize) -> Value {
    let text_body = match root
        .descendants("body")
        .into_iter()
        .find_map(|one| one.child("text"))
    {
        Some(had) => had,
        None => {
            return json!({"family": "odf", "available": false});
        }
    };
    let mut items: Vec<Value> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    let mut paragraphs_total = 0usize;
    let mut frames_seen = 0usize;
    let mut objects_total = 0usize;
    let mut math_found = 0usize;
    let mut objects_without_math = 0usize;
    let mut parts_found = 0usize;
    let mut parts_missing = 0usize;
    let mut block_written = 0usize;
    let mut inline_written = 0usize;
    let mut display_missing = 0usize;
    let mut annotations_found = 0usize;
    let mut replacements_written = 0usize;
    let mut char_total = 0usize;
    for par in text_body
        .children
        .iter()
        .filter(|kid| kid.name != "#text" && matches!(kid.local(), "p" | "h"))
    {
        for frame in par.children.iter().filter(|kid| kid.local() == "frame") {
            frames_seen += 1;
            let object = match frame.descendants("object").into_iter().next() {
                Some(had) => had,
                None => continue,
            };
            objects_total += 1;
            let href = crate::odsheet::attr_of(object, "href")
                .unwrap_or_default()
                .to_string();
            let member = member_of(&href);
            let found =
                !member.is_empty() && zipread::member(bytes, &member, DEFAULT_MEMBER_CAP).is_ok();
            if found {
                parts_found += 1;
            } else {
                parts_missing += 1;
            }
            let replacement = frame
                .descendants("image")
                .into_iter()
                .next()
                .and_then(|had| crate::odsheet::attr_of(had, "href"))
                .map(String::from);
            if replacement.is_some() {
                replacements_written += 1;
            }
            let (display, elements, text, encoding, source) = match found {
                true => mathml_of(bytes, &member),
                false => (None, Vec::new(), String::new(), None, None),
            };
            // 部件里有没有 `<math>` 根是「这条对象真是条公式吗」唯一的凭据：
            // 图表那类嵌入对象也有 draw:object，不能靠地址形状猜
            let is_math = !elements.is_empty();
            if is_math {
                math_found += 1;
                match display.as_deref() {
                    Some("block") => block_written += 1,
                    Some("inline") => inline_written += 1,
                    _ => display_missing += 1,
                }
                if source.is_some() {
                    annotations_found += 1;
                }
                for name in elements.iter() {
                    if !seen.iter().any(|one| one == name) {
                        seen.push(name.clone());
                    }
                }
                char_total += text.chars().count();
            } else {
                objects_without_math += 1;
            }
            items.push(json!({
                "index": items.len(),
                "paragraph": paragraphs_total,
                "frame_name": crate::odsheet::attr_of(frame, "name").map(String::from),
                "style_written": crate::odsheet::attr_of(frame, "style-name").map(String::from),
                "anchor_written": crate::odsheet::attr_of(frame, "anchor-type").map(String::from),
                "width_written": crate::odsheet::attr_of(frame, "width").map(String::from),
                "height_written": crate::odsheet::attr_of(frame, "height").map(String::from),
                "z_written": crate::odsheet::attr_of(frame, "z-index").map(String::from),
                "object_target": href,
                "object_part": match found {
                    true => json!(member),
                    false => Value::Null,
                },
                "part_found": found,
                "math_found": is_math,
                "replacement_target": match &replacement {
                    Some(raw) => json!(raw),
                    None => Value::Null,
                },
                "display_written": match &display {
                    Some(raw) => json!(raw),
                    None => Value::Null,
                },
                "elements": elements,
                "text": text,
                "annotation_encoding": match &encoding {
                    Some(raw) => json!(raw),
                    None => Value::Null,
                },
                "annotation_source": match &source {
                    Some(raw) => json!(raw),
                    None => Value::Null,
                },
            }));
        }
        paragraphs_total += 1;
    }
    json!({
        "family": "odf",
        "available": true,
        "items": items.into_iter().take(limit).collect::<Vec<Value>>(),
        "equations_total": math_found,
        "paragraphs_total": paragraphs_total,
        "frames_seen": frames_seen,
        "objects_total": objects_total,
        "math_found": math_found,
        "objects_without_math": objects_without_math,
        "parts_found": parts_found,
        "parts_missing": parts_missing,
        "block_written": block_written,
        "inline_written": inline_written,
        "display_missing": display_missing,
        "annotations_found": annotations_found,
        "replacements_written": replacements_written,
        "text_chars": char_total,
        "elements_seen": seen,
    })
}
