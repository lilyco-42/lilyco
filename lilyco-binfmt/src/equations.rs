//! 文档里的公式：OOXML 写成 OMML 挂在段上，ODF 把每条式子装进一个**嵌入对象**的 MathML 部件
//!
//! 形状：`{family, available, items[], <聚合数>}`，四支各一本，另一族的键整个不在。
//! .doc 一支（`office-doc`）每条 `{index, pool_name, streams, compobj_written, ole_written,
//! payload_stream, payload_size, declared_size, label, user_type, prog_id}`，聚合
//! `objects_total / equations_total / payload_stream_seen / native_bytes_total / pool_found`。
//! pptx 一支（`office-slide`）每条 `{index, part, show_index, paragraph, holder, shape_id,
//! shape_name, structures, runs, nor_runs, lit_runs, align_written, text, choice_requires,
//! in_alternate, fallback_written, fallback_blip, fallback_target, fallback_found,
//! fallback_shape_id}`，页级 `slides[]` 交 `paragraphs_total / shapes_total / formulas /
//! alternates`，聚合 `equations_total / slides_with / paragraphs_total / text_chars /
//! math_runs / nor_runs / lit_runs / structures_seen / alternates_total /
//! fallbacks_written / duplicated_shapes / rasters_written / rasters_found /
//! rasters_missing`。
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
//! - RTF 与 `.ppt` 不交这个键。`.ppt` 这一条是**量过**的而不是想当然：`eqs.odp` 转
//!   MS PowerPoint 97 之后，容器里只有 8 条目录项（没有 `ObjectPool`，`.doc` 那一条路整个
//!   不存在），整份件里 `Equation` 0 次、MTEF 的头 0 次、`Equation.3` 0 次、EMF 的签名 0 次
//!   —— 式子既不是 OMML、也不是 OLE 对象，只剩 `Pictures` 流里那 916 字节的一笔位图。
//!   所以那一族缺这个键；见事实 114。
//! - `.doc` 交，但只交到对象那一层：式子是 `ObjectPool` 里一枚枚内嵌 OLE 对象，正文流的名字
//!   按写的交（`Equation Native`），字在 MTEF 二进制里 —— 本机没有第二个读者认得它，所以那一族
//!   **整个不交 `text`**（缺键，不是空串）。

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

/// 一条 OMML 自己的形状（docx 与 pptx 两支共用这一份判据，别抄成两份）
struct OmmlFacts {
    structures: Vec<String>,
    runs: usize,
    nor: usize,
    lit: usize,
    text: String,
}

/// `head` 是 OMML 前缀加冒号（docx 那一支按写着的命名空间前缀筛）；pptx 那一支不筛，
/// 因为式子外面那层是 `a14:m`，里面的名字与 docx 同一家族
fn omml_of(node: &Node, head: Option<&str>) -> OmmlFacts {
    let mut kids: Vec<&Node> = Vec::new();
    subtree(node, &mut kids);
    let mut out = OmmlFacts {
        structures: Vec::new(),
        runs: 0,
        nor: 0,
        lit: 0,
        text: String::new(),
    };
    for one in kids {
        let keep = match head {
            Some(want) => one.name.starts_with(want),
            None => true,
        };
        if !keep {
            continue;
        }
        let name = one.local();
        if name == "r" {
            // 数学 run 是「一串字」的壳，不算结构：与 python 那侧同一条排除表
            out.runs += 1;
            continue;
        }
        if name == "t" {
            out.text.push_str(&one.text());
            continue;
        }
        if name == "nor" {
            out.nor += 1;
        }
        if name == "lit" {
            out.lit += 1;
        }
        if name == "oMath" {
            continue;
        }
        out.structures.push(name.to_string());
    }
    out
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
        let facts = omml_of(node, Some(&self.head));
        let structures = facts.structures;
        let runs = facts.runs;
        let nor = facts.nor;
        let lit = facts.lit;
        let text = facts.text;
        for name in structures.iter() {
            if !self.seen.iter().any(|one| *one == *name) {
                self.seen.push(name.clone());
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

/// odp 那一本要数的格（键名与 `lyco_equations.odp_equations` 一字不差）
const ODP_KEYS: [&str; 17] = [
    "pages_total",
    "frames_seen",
    "frames_in_notes",
    "page_thumbnails",
    "objects_total",
    "math_found",
    "objects_without_math",
    "parts_found",
    "parts_missing",
    "replacements_written",
    "replacements_missing",
    "block_written",
    "inline_written",
    "display_missing",
    "annotations_found",
    "anchors_written",
    "text_chars",
];

/// odp 那一本的运行态
struct OdpRun {
    items: Vec<Value>,
    pages: Vec<Value>,
    seen: Vec<String>,
    counts: Vec<(&'static str, i64)>,
}

impl OdpRun {
    fn new() -> OdpRun {
        OdpRun {
            items: Vec::new(),
            pages: Vec::new(),
            seen: Vec::new(),
            counts: ODP_KEYS.iter().map(|one| (*one, 0i64)).collect(),
        }
    }

    fn bump(&mut self, key: &str) {
        self.bump_by(key, 1);
    }

    fn bump_by(&mut self, key: &str, by: i64) {
        if let Some(slot) = self.counts.iter_mut().find(|one| one.0 == key) {
            slot.1 += by;
        }
    }

    fn count(&self, key: &str) -> i64 {
        self.counts
            .iter()
            .find(|one| one.0 == key)
            .map(|one| one.1)
            .unwrap_or(0)
    }
}

/// 页里的每一枚 `draw:frame`，带上「它在不在 `presentation:notes` 里面」
///
/// 只能自顶往下带标志：这一族要找的「祖先里有没有 notes」用身份比对是走不通的
/// （python 那侧 `kid is target` 永远不成立，注块的排除会静默失效），
/// 而 Rust 这边根本没有父指针，所以两边同一形状。
fn collect_frames<'a>(node: &'a Node, in_notes: bool, out: &mut Vec<(&'a Node, bool)>) {
    for one in node.children.iter().filter(|kid| kid.name != "#text") {
        let deeper = in_notes || one.local() == "notes";
        if one.local() == "frame" {
            out.push((one, deeper));
        }
        collect_frames(one, deeper, out);
    }
}

/// 页缩略图：LibreOffice 的 odp 重写给每页插一枚 `draw:page-thumbnail`（挂在 notes 里），
/// 它既不是公式也不算页上的 frame
fn count_thumbs(node: &Node) -> i64 {
    let mut kids: Vec<&Node> = Vec::new();
    subtree(node, &mut kids);
    kids.iter()
        .filter(|one| one.local() == "page-thumbnail")
        .count() as i64
}

fn member_exists(bytes: &[u8], member: &str) -> bool {
    !member.is_empty() && zipread::member(bytes, member, DEFAULT_MEMBER_CAP).is_ok()
}

/// odp 那一份：每页的 frame → `draw:object` → `Object N/content.xml` 的 MathML
pub(crate) fn odp(bytes: &[u8], root: &Node, limit: usize) -> Value {
    let body = match root.descendants("presentation").into_iter().next() {
        Some(had) => had,
        None => {
            return json!({"family": "odp", "available": false});
        }
    };
    let mut run = OdpRun::new();
    for page in body
        .children
        .iter()
        .filter(|kid| kid.name != "#text" && kid.local() == "page")
    {
        let index = run.count("pages_total");
        run.bump("pages_total");
        let thumbs = count_thumbs(page);
        run.bump_by("page_thumbnails", thumbs);
        let mut found: Vec<(&Node, bool)> = Vec::new();
        collect_frames(page, false, &mut found);
        let mut page_frames = 0i64;
        let mut notes_frames = 0i64;
        let mut formulas = 0i64;
        for (frame, in_notes) in found.iter() {
            if *in_notes {
                notes_frames += 1;
                continue;
            }
            page_frames += 1;
            run.bump("frames_seen");
            let object = match frame.descendants("object").into_iter().next() {
                Some(had) => had,
                None => continue,
            };
            run.bump("objects_total");
            formulas += 1;
            let href = crate::odsheet::attr_of(object, "href")
                .unwrap_or_default()
                .to_string();
            let member = member_of(&href);
            let found_part = member_exists(bytes, &member);
            run.bump(if found_part {
                "parts_found"
            } else {
                "parts_missing"
            });
            let image = frame.descendants("image").into_iter().next();
            let replacement = image
                .and_then(|had| crate::odsheet::attr_of(had, "href"))
                .map(String::from);
            let mut repl_found: Option<bool> = None;
            if let Some(raw) = &replacement {
                run.bump("replacements_written");
                let stem = raw.strip_prefix("./").unwrap_or(raw);
                let stem = stem.strip_prefix('/').unwrap_or(stem);
                repl_found = Some(member_exists(bytes, stem));
                if repl_found == Some(false) {
                    run.bump("replacements_missing");
                }
            }
            let (display, elements, text, encoding, source) = match found_part {
                true => mathml_of(bytes, &member),
                false => (None, Vec::new(), String::new(), None, None),
            };
            // 部件里有没有 `<math>` 根才算式子：图表那类嵌入对象走的是同一扇门
            let is_math = !elements.is_empty();
            if is_math {
                run.bump("math_found");
                match display.as_deref() {
                    Some("block") => run.bump("block_written"),
                    Some("inline") => run.bump("inline_written"),
                    _ => run.bump("display_missing"),
                }
                if source.is_some() {
                    run.bump("annotations_found");
                }
                for name in elements.iter() {
                    if !run.seen.iter().any(|one| one == name) {
                        run.seen.push(name.clone());
                    }
                }
                run.bump_by("text_chars", text.chars().count() as i64);
            } else {
                run.bump("objects_without_math");
            }
            let anchor = crate::odsheet::attr_of(frame, "anchor-type").map(String::from);
            if anchor.is_some() {
                run.bump("anchors_written");
            }
            let ordinal = run.items.len();
            run.items.push(json!({
                "index": ordinal,
                "page": index,
                "frame_name": crate::odsheet::attr_of(frame, "name").map(String::from),
                "style_written": crate::odsheet::attr_of(frame, "style-name").map(String::from),
                "anchor_written": anchor,
                "width_written": crate::odsheet::attr_of(frame, "width").map(String::from),
                "height_written": crate::odsheet::attr_of(frame, "height").map(String::from),
                "z_written": crate::odsheet::attr_of(frame, "z-index").map(String::from),
                "object_target": href,
                "object_part": match found_part {
                    true => json!(member),
                    false => Value::Null,
                },
                "part_found": found_part,
                "math_found": is_math,
                "replacement_target": match &replacement {
                    Some(raw) => json!(raw),
                    None => Value::Null,
                },
                "replacement_found": match repl_found {
                    Some(had) => json!(had),
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
        run.bump_by("frames_in_notes", notes_frames);
        run.pages.push(json!({
            "page": index,
            "frames": page_frames,
            "frames_in_notes": notes_frames,
            "thumbnails": thumbs,
            "formulas": formulas,
        }));
    }
    let math_total = run.count("math_found");
    let mut mine = serde_json::Map::new();
    mine.insert("family".to_string(), json!("odp"));
    mine.insert("available".to_string(), json!(true));
    mine.insert(
        "items".to_string(),
        json!(run.items.into_iter().take(limit).collect::<Vec<Value>>()),
    );
    mine.insert("pages".to_string(), json!(run.pages));
    mine.insert("elements_seen".to_string(), json!(run.seen));
    for (key, value) in run.counts.iter() {
        mine.insert((*key).to_string(), json!(value));
    }
    mine.insert("equations_total".to_string(), json!(math_total));
    Value::Object(mine)
}

/// pptx 那一支：一条式子是**文本体里的 OMML**，而同一个形状在外层那个
/// `mc:AlternateContent` 的 `mc:Fallback` 里**还写了一遍**（那一遍没有字，改挂一张替身图）
///
/// 页级那三格（`paragraphs_total` / `shapes_total` / `formulas`）必须分开交：同一页在
/// LibreOffice 那份件里写 2 枚 `p:sp`、1 条式子，在 python-pptx 手挂的那份里写 1 枚 `p:sp`、
/// 1 条式子 —— 拿形状数当式子数就会在第一家那里翻一倍。
#[derive(Default)]
pub(crate) struct PptxRun {
    items: Vec<Value>,
    slides: Vec<Value>,
    seen: Vec<String>,
    equations_total: usize,
    slides_with: usize,
    paragraphs_total: usize,
    text_chars: usize,
    math_runs: usize,
    nor_runs: usize,
    lit_runs: usize,
    alternates_written: usize,
    fallbacks_written: usize,
    duplicated_shapes: usize,
    rasters_written: usize,
    rasters_found: usize,
    rasters_missing: usize,
}

/// 往下走时带着的那串上下文：这一族没有父指针，所以「在谁里面」一路带
#[derive(Clone)]
struct PptxCtx<'a> {
    in_sp: bool,
    shape_id: Option<String>,
    shape_name: Option<String>,
    requires: Option<String>,
    alternate: Option<&'a Node>,
    paragraph: Option<usize>,
}

/// 撞见的一条式子：住在哪条链上、外面那层 `AlternateContent` 是哪一枚
struct PptxHit<'a> {
    node: &'a Node,
    holder: String,
    shape_id: Option<String>,
    shape_name: Option<String>,
    requires: Option<String>,
    alternate: Option<&'a Node>,
    paragraph: Option<usize>,
}

#[derive(Default)]
struct PptxFound {
    paragraphs: usize,
    shapes: usize,
    formulas: usize,
    alternates: usize,
}

/// 自顶往下的一趟：`holder` 是**当前这个元素**写着的名字，所以它的孩子若是 `m:oMath`，
/// 这一串就是那条式子的挂法（`a14:m`）
fn walk_pptx<'a>(
    node: &'a Node,
    holder: &str,
    ctx: &PptxCtx<'a>,
    hits: &mut Vec<PptxHit<'a>>,
    found: &mut PptxFound,
) {
    for one in node.children.iter().filter(|kid| kid.name != "#text") {
        let kind = one.local();
        let mut sub = ctx.clone();
        if kind == "sp" {
            found.shapes += 1;
            sub.in_sp = true;
            let named = one.descendants("cNvPr").into_iter().next();
            sub.shape_id = named.and_then(|hit| hit.attr_local("id")).map(String::from);
            sub.shape_name = named
                .and_then(|hit| hit.attr_local("name"))
                .map(String::from);
        } else if kind == "AlternateContent" {
            found.alternates += 1;
            sub.alternate = Some(one);
        } else if kind == "Choice" {
            sub.requires = one.attr("Requires").map(String::from);
        } else if kind == "p" && ctx.in_sp {
            sub.paragraph = Some(found.paragraphs);
            found.paragraphs += 1;
        } else if kind == "oMath" {
            found.formulas += 1;
            hits.push(PptxHit {
                node: one,
                holder: holder.to_string(),
                shape_id: ctx.shape_id.clone(),
                shape_name: ctx.shape_name.clone(),
                requires: ctx.requires.clone(),
                alternate: ctx.alternate,
                paragraph: ctx.paragraph,
            });
        }
        walk_pptx(one, &one.name, &sub, hits, found);
    }
}

impl PptxRun {
    /// 一页：按放映顺序逐页喂进来（页部件名与 `show_index` 都由调用方给，这里不自己排序）
    pub(crate) fn add_slide(
        &mut self,
        bytes: &[u8],
        slide_root: &Node,
        part: &str,
        show_index: &Value,
        rels: &[crate::opack::Rel],
    ) {
        let ctx = PptxCtx {
            in_sp: false,
            shape_id: None,
            shape_name: None,
            requires: None,
            alternate: None,
            paragraph: None,
        };
        let mut hits: Vec<PptxHit> = Vec::new();
        let mut found = PptxFound::default();
        walk_pptx(slide_root, &slide_root.name, &ctx, &mut hits, &mut found);
        for hit in hits {
            let facts = omml_of(hit.node, None);
            let align = hit
                .node
                .descendants("jc")
                .into_iter()
                .next()
                .and_then(|one| one.attr_local("val"))
                .map(String::from);
            let mut fallback_written = Value::Null;
            let mut fallback_blip: Option<String> = None;
            let mut fallback_target: Option<String> = None;
            let mut fallback_found = Value::Null;
            let mut fallback_shape_id: Option<String> = None;
            if let Some(alternate) = hit.alternate {
                let fallback = alternate
                    .children
                    .iter()
                    .find(|kid| kid.local() == "Fallback");
                fallback_written = json!(fallback.is_some());
                let choice_sp = alternate
                    .children
                    .iter()
                    .find(|kid| kid.local() == "Choice")
                    .and_then(|one| one.descendants("sp").into_iter().next())
                    .and_then(|one| {
                        one.descendants("cNvPr")
                            .into_iter()
                            .next()
                            .and_then(|hit| hit.attr_local("id"))
                    });
                if let Some(one) = fallback {
                    let embed = one
                        .descendants("blip")
                        .into_iter()
                        .next()
                        .and_then(|kid| kid.attr_local("embed"))
                        .map(String::from);
                    if embed.is_some() {
                        self.rasters_written += 1;
                        fallback_blip = embed.clone();
                        fallback_target = embed.and_then(|want| {
                            rels.iter()
                                .find(|one| one.id == want && one.source == part)
                                .and_then(|one| one.resolved.clone())
                        });
                        let had = match &fallback_target {
                            Some(raw) => member_exists(bytes, raw),
                            None => false,
                        };
                        if had {
                            self.rasters_found += 1;
                        } else {
                            self.rasters_missing += 1;
                        }
                        fallback_found = json!(had);
                    }
                    fallback_shape_id = one
                        .descendants("sp")
                        .into_iter()
                        .next()
                        .and_then(|kid| {
                            kid.descendants("cNvPr")
                                .into_iter()
                                .next()
                                .and_then(|hit| hit.attr_local("id"))
                        })
                        .map(String::from);
                    if fallback_shape_id.is_some()
                        && fallback_shape_id == choice_sp.map(String::from)
                    {
                        self.duplicated_shapes += 1;
                    }
                }
                if fallback.is_some() {
                    self.fallbacks_written += 1;
                }
            }
            for name in facts.structures.iter() {
                if !self.seen.iter().any(|one| *one == *name) {
                    self.seen.push(name.clone());
                }
            }
            self.equations_total += 1;
            self.text_chars += facts.text.chars().count();
            self.math_runs += facts.runs;
            self.nor_runs += facts.nor;
            self.lit_runs += facts.lit;
            self.items.push(json!({
                "index": self.items.len(),
                "part": part,
                "show_index": show_index,
                "paragraph": match hit.paragraph {
                    Some(raw) => json!(raw),
                    None => Value::Null,
                },
                "holder": hit.holder,
                "shape_id": hit.shape_id,
                "shape_name": hit.shape_name,
                "structures": facts.structures,
                "runs": facts.runs,
                "nor_runs": facts.nor,
                "lit_runs": facts.lit,
                "align_written": match align {
                    Some(raw) => json!(raw),
                    None => Value::Null,
                },
                "text": facts.text,
                "choice_requires": hit.requires,
                "in_alternate": json!(hit.alternate.is_some()),
                "fallback_written": fallback_written,
                "fallback_blip": fallback_blip,
                "fallback_target": fallback_target,
                "fallback_found": fallback_found,
                "fallback_shape_id": fallback_shape_id,
            }));
        }
        self.slides_with += usize::from(found.formulas > 0);
        self.paragraphs_total += found.paragraphs;
        self.alternates_written += found.alternates;
        self.slides.push(json!({
            "part": part,
            "show_index": show_index,
            "paragraphs_total": found.paragraphs,
            "shapes_total": found.shapes,
            "formulas": found.formulas,
            "alternates": found.alternates,
        }));
    }

    pub(crate) fn finish(self, limit: usize) -> Value {
        json!({
            "family": "pptx",
            "available": true,
            "items": self.items.into_iter().take(limit).collect::<Vec<Value>>(),
            "slides": self.slides,
            "slides_total": self.slides.len(),
            "structures_seen": self.seen,
            "equations_total": self.equations_total,
            "slides_with": self.slides_with,
            "paragraphs_total": self.paragraphs_total,
            "text_chars": self.text_chars,
            "math_runs": self.math_runs,
            "nor_runs": self.nor_runs,
            "lit_runs": self.lit_runs,
            // 页里所有 `mc:AlternateContent`（连 `p:transition` 那枚也算），与下面那几格
            // 「式子所在那一枚有没有 Fallback」不是同一个 population
            "alternates_total": self.alternates_written,
            "fallbacks_written": self.fallbacks_written,
            "duplicated_shapes": self.duplicated_shapes,
            "rasters_written": self.rasters_written,
            "rasters_found": self.rasters_found,
            "rasters_missing": self.rasters_missing,
        })
    }
}

/// 遗留 .doc 那一支：式子是内嵌 OLE 对象，住在目录树的 `ObjectPool` 下面
const POOL: &str = "ObjectPool";
/// 一物一 storage 里那两条**控制流**：它们不写这条对象是什么，正文那条流的名字才写
const CONTROLLED: [&str; 2] = ["\u{1}CompObj", "\u{1}Ole"];
/// 目录树走多深就停：畸形文件能把 `left` 指回自己，没有界就是一趟不回来的递归
const TREE_DEPTH: usize = 64;

/// 一枚 storage 的孩子，按中序（left → 自己 → right）
///
/// 「没有孩子」这一格各家写得不一样（规范是 `0xFFFFFFFF`，也有文件直接写 0）：
/// 这里与第二读者同一条判据 —— 0 或越界都当结束。下标 0 永远是 Root Entry，
/// 而 Root 不会是谁的孩子，所以两种哨兵在这里不互相冒充。
fn ole_walk(cfb: &crate::cfb::Cfb, where_: u32, depth: usize, out: &mut Vec<usize>) {
    if depth >= TREE_DEPTH {
        return;
    }
    let index = match usize::try_from(where_) {
        Ok(raw) => raw,
        Err(_) => return,
    };
    if index == 0 {
        return;
    }
    let one = match cfb.entries.get(index) {
        Some(raw) => raw,
        None => return,
    };
    ole_walk(cfb, one.left, depth + 1, out);
    out.push(index);
    ole_walk(cfb, one.right, depth + 1, out);
}

/// `\x01CompObj` 后半的三枚「u32 长度 + latin-1 串」：显示名、用户类型、ProgID。
/// 头部 28 字节（u32 版本两枚 + u32 保留 + 16 字节 CLSID）—— 第一版把 CLSID 读成
/// 「一个类型字节 + 15 字节」，三枚串全成 null，在 `eq.doc` 上才量出来。
/// 读不出的一律 null，不拿邻居的数凑。
fn compobj_labels(body: &[u8]) -> (Option<String>, Option<String>, Option<String>) {
    let mut out: Vec<Option<String>> = Vec::new();
    let mut at = 28usize;
    while out.len() < 3 {
        if at + 4 > body.len() {
            break;
        }
        let head = [body[at], body[at + 1], body[at + 2], body[at + 3]];
        let size = u32::from_le_bytes(head) as usize;
        if size == 0 || size > 256 || at + 4 + size > body.len() {
            break;
        }
        let text: String = body[at + 4..at + 4 + size]
            .iter()
            .take_while(|raw| **raw != 0)
            .map(|raw| *raw as char)
            .collect();
        out.push(Some(text));
        at += 4 + size;
    }
    while out.len() < 3 {
        out.push(None);
    }
    let mut it = out.into_iter();
    (
        it.next().unwrap_or(None),
        it.next().unwrap_or(None),
        it.next().unwrap_or(None),
    )
}

/// .doc 那一支：`ObjectPool` 下每枚对象一条账，正文流的名字按写的交
///
/// 界：MTEF 载荷**不解**（本机没有第二个读者认得它），所以式子里的字整个不在这一本里 ——
/// 缺键，不是空串。`objects_total` 与 `equations_total` 两格各数各的，因为这一族
/// 既装公式也装别的，判据是那条正文流自己叫什么。
pub(crate) fn ole(cfb: &crate::cfb::Cfb, bytes: &[u8]) -> Value {
    let pool = cfb
        .entries
        .iter()
        .position(|one| one.kind == "storage" && one.name == POOL);
    let pool = match pool {
        Some(raw) => raw,
        None => {
            return json!({
                "family": "ole",
                "available": true,
                "items": [],
                "objects_total": 0,
                "equations_total": 0,
                "payload_stream_seen": [],
                "native_bytes_total": 0,
                "pool_found": false,
            })
        }
    };
    let mut objects: Vec<usize> = Vec::new();
    ole_walk(cfb, cfb.entries[pool].child, 0, &mut objects);
    let mut items: Vec<Value> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    let mut objects_total = 0usize;
    let mut equations_total = 0usize;
    let mut native_bytes = 0usize;
    for index in objects {
        let mut kids: Vec<usize> = Vec::new();
        ole_walk(cfb, cfb.entries[index].child, 0, &mut kids);
        let streams: Vec<usize> = kids
            .into_iter()
            .filter(|raw| cfb.entries[*raw].is_stream())
            .collect();
        let names: Vec<String> = streams
            .iter()
            .map(|raw| cfb.entries[*raw].name.clone())
            .collect();
        let free = |name: &str| CONTROLLED.iter().any(|one| *one == name);
        let payload_at = streams
            .iter()
            .find(|raw| !free(&cfb.entries[**raw].name))
            .copied();
        let payload = payload_at.map(|raw| cfb.entries[raw].name.clone());
        let body = payload_at
            .and_then(|raw| cfb.entries.get(raw))
            .and_then(|one| cfb.read_entry(bytes, one))
            .unwrap_or_default();
        let declared = payload_at
            .and_then(|raw| cfb.entries.get(raw))
            .map(|one| json!(one.size));
        let comp_at = streams
            .iter()
            .find(|raw| cfb.entries[**raw].name == CONTROLLED[0])
            .copied();
        let comp_body = comp_at
            .and_then(|raw| cfb.entries.get(raw))
            .and_then(|one| cfb.read_entry(bytes, one))
            .unwrap_or_default();
        let (label, user_type, prog_id) = compobj_labels(&comp_body);
        if payload.as_deref() == Some("Equation Native") {
            equations_total += 1;
            native_bytes += body.len();
        }
        if let Some(raw) = &payload {
            if !seen.iter().any(|one| *one == *raw) {
                seen.push(raw.clone());
            }
        }
        let payload_size = match &payload {
            Some(_) => json!(body.len()),
            None => Value::Null,
        };
        objects_total += 1;
        items.push(json!({
            "index": items.len(),
            "pool_name": cfb.entries[index].name.clone(),
            "streams": names,
            "compobj_written": json!(names.iter().any(|one| one.as_str() == CONTROLLED[0])),
            "ole_written": json!(names.iter().any(|one| one.as_str() == CONTROLLED[1])),
            "payload_stream": payload,
            "payload_size": payload_size,
            "declared_size": declared.unwrap_or(Value::Null),
            "label": label,
            "user_type": user_type,
            "prog_id": prog_id,
        }));
    }
    json!({
        "family": "ole",
        "available": true,
        "items": items,
        "objects_total": objects_total,
        "equations_total": equations_total,
        "payload_stream_seen": seen,
        "native_bytes_total": native_bytes,
        "pool_found": true,
    })
}
