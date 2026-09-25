//! 把一份文档的结构搬进 markdown —— 标题、列表、表、链接、图，一条不漏地搬，搬不过去的说清
//!
//! 走这一本的两族：`docx()`（OOXML 的 word）与 `odf()`（ODF 的 odt）。
//!
//! 形状：`--markdown` 开着才交这一本（没开就整个键都不给，不交一份空串装作有）：
//! `{family, available, text, chars, cut, blocks, paragraphs, headings, list_items,
//! bullet_items, ordered_items, list_from_style, unresolved_fmt, tables, table_rows,
//! empty_dropped}`。`text` 是渲染结果（按 `--max-chars` 截，`chars` 仍是全长，`cut` 说截没截），
//! 其余那些数说的是「这一本凭什么这么长」。
//!
//! 实测两份件（`md.docx` 由 python-docx 写、每段只管一件事；`md-lo.docx` 是 LibreOffice 的
//! docx → docx 重写）：
//! 1. **两家的 markdown 一字不差**（359 个码位），而账本说得出为什么：五个列表项在 python-docx
//!    那份里号写在**样式**上（`List Bullet` → `numId`，段上自己一个字没写），
//!    LibreOffice 重写时把号抄到**段上**（`list_from_style` 5 → 0）—— 同一个选择在两家文件里
//!    落在两个地方，渲染结果却不需要知道这件事；
//! 2. 第二层列表在 python-docx 那份里是**换了一个 numId** 表达的（样式里连 `ilvl` 都不写），
//!    所以缩进只按文件写着的 `ilvl` 走：那一条搬不过去的层级由 `list_items` 与
//!    `bullet_items` 这两个数说清，不拿样式名字尾数的数字当层级。反面凭据是 `lists.docx`：
//!    那里「直接挂在段上的第二级」是真的写了 `ilvl="1"`，缩进两格；而「点了一个不存在的号」
//!    与 LibreOffice 重排出的 `numId="0"` 两条都解不到格式（`unresolved_fmt` 2）——
//!    渲染必须挑一个标记，这里挑最保守的 `- `，并把这件事说在账上而不是藏进字符串；
//! 3. 「有 `w:numPr`」不等于「是列表项」：本机 28 份真件的模板样式 `Subtitle` 里就带一枚
//!    **没有 `numId` 的** `w:numPr`，按「看见 numPr 就算列表」去读，那 28 份的副标题全变成列表项；
//! 4. 表外的竖线**不转义**、表里的转义（`esc` 只在格子里带 `pipe`）：页面上看得见的字
//!    不能被渲染器改掉；
//! 5. 一段普通正文的行首如果长得像结构记号（`#` / `>` / `- ` / `1.`），补一个反斜杠 ——
//!    `md.docx` 里就是两句文件自己写着的字，不守住它们会被读成标题与编号；
//! 6. 空段与「整段只有一个分页符」的那种段都不进 markdown（这一层在 markdown 里没有对应物），
//!    条数交在 `empty_dropped`（这两份件各 2 条）—— 0 与「没这一层」不是一回事。
//!
//! ODF 那一面（`md.odt` = LibreOffice 把 `md.docx` 转成 odt，同一份稿子的第三副样子）另量到五条：
//! 7. **同一份稿子跨族同形**：35 行里只有一行不同 —— 那一行是图片地址，各按自己文件写的交
//!    （docx 的关系表写 `media/image1.png`，LibreOffice 在 ODF 里按内容哈希命名成
//!    `Pictures/1000000100000008000000088E4DF5D4.png`），`chars` 因此从 359 变 388；
//! 8. **字要一跳字符样式**：粗斜不在 run 上而在 `text:span/@text:style-name` 引的那份
//!    `style:family="text"` 样式里，而样式可能坐在 content.xml 也可能坐在 styles.xml
//!    （先到先得，与编号那一条同口径）—— 解不到就交 `spans_unresolved`，不猜形状；
//! 9. **空格与换行是记号不是字**：`<text:s/>`（`text:c` 说几个）、`<text:tab/>`、
//!    `<text:line-break/>` 三种都要还原，`space_markers` 数还原了几枚；这一族没有
//!    `xml:space="preserve"` 这个东西，两个空格的句子被拆成「一个字面空格 + 一枚记号」，
//!    后半句挂在**这个元素的尾**上 —— 漏了尾巴，那句只剩「两处空格 」；
//! 10. **列表是嵌套元素**：`text:list`（名字 → `text:list-style` 的各级 `bullet`/`number`）套
//!     `text:list-item` 套段，层级由嵌套深度给（docx 那边是 `ilvl` 那个数），
//!     所以这边用 `lists_named` / `lists_unnamed` 说「这几个列表有没有点名样式」；
//! 11. **批注与注嵌在正文段里面**（`office:annotation` / `text:note`），它们的字整块跳过并各记
//!     一条数（`annotations_dropped` / `notes_dropped`）—— docx 那边这些东西住在别的部件里，
//!     本来也不在正文，两族同一口径；表格里 `number-columns-repeated` 顶几列要展开（上限 64），
//!     `covered-table-cell` 记在 `covered_cells`。
//!
//! 界（这一本**不**做的那几件事）：
//! - **odp / ods / rtf / .doc / .pdf 不交这个键**（缺键 = 这一族还没搬，不是空文档）：
//!   演示稿的正文按页分、表格的「段」是格子，都要另量一遍；
//! - OOXML 表格里被合并的格子（`gridSpan` / `vMerge`）markdown 表达不了，所以**不展开也不补空格**
//!   （每行按文件写着的格子数排，行数不够就补空串）；ODF 那侧的 `number-columns-repeated`
//!   是「这一条列元素顶几列」，文件自己说了数，所以照数展开（一条最多展 64 列）；
//! - 链接与图片的 target **按关系表写的原样交**（`media/image1.png` 是相对 `word/` 的，
//!   本渲染器不把图搬出来）：实测 14 条链接的地址里没有一个含空格、括号或竖线，
//!   所以不转义 target 不会截断链接；
//! - 修订：`w:ins` 壳里的字要（那是这份稿子现在的样子），`w:del` 壳里的字**不要**；
//!   删掉的字在 `office-doc` 的 `revisions` 那本账里逐条交，这里不重复。

use crate::opack::{self, Rel};
use crate::xmlscan::{self, Node};
use crate::zipread::{self, DEFAULT_MEMBER_CAP};
use serde_json::{json, Value};

/// 段内的硬换行先占一个不会出现在正文里的字符，拼完再换成「反斜杠 + 换行」
const HARD: char = '\u{0}';
/// OOXML 说「不」的三种拼法（`w:val` 上）
const OFF: [&str; 3] = ["0", "false", "none"];

/// 一颗带形状的片段：一段字（带着粗、斜、链接）或一张图
struct Seg {
    text: String,
    bold: bool,
    italic: bool,
    link: Option<String>,
    image: Option<(String, String)>,
}

impl Seg {
    fn said(text: &str, bold: bool, italic: bool, link: Option<String>) -> Seg {
        Seg {
            text: text.to_string(),
            bold,
            italic,
            link,
            image: None,
        }
    }

    fn words(text: &str, bold: bool, italic: bool) -> Seg {
        Seg {
            text: text.to_string(),
            bold,
            italic,
            link: None,
            image: None,
        }
    }

    fn picture(alt: &str, target: &str) -> Seg {
        Seg {
            text: String::new(),
            bold: false,
            italic: false,
            link: None,
            image: Some((alt.to_string(), target.to_string())),
        }
    }
}

/// 渲染要查的四张表：关系、样式名、样式的列表号、编号定义（后者要再跳一跳）
struct Book {
    rels: Vec<Rel>,
    names: Vec<(String, Option<String>)>,
    lists: Vec<(String, Option<String>, Option<String>)>,
    nums: Vec<(String, Option<String>)>,
    abstracts: Vec<(String, Vec<(String, Option<String>)>)>,
}

impl Book {
    fn target(&self, source: &str, id: &str) -> Option<&str> {
        self.rels
            .iter()
            .find(|one| one.source == source && one.id == id)
            .map(|one| one.target.as_str())
    }

    fn style_name(&self, id: &str) -> Option<&str> {
        self.names
            .iter()
            .find(|one| one.0 == id)
            .and_then(|one| one.1.as_deref())
    }

    fn style_list(&self, id: &str) -> Option<(&Option<String>, &Option<String>)> {
        self.lists
            .iter()
            .find(|one| one.0 == id)
            .map(|one| (&one.1, &one.2))
    }

    fn level_format(&self, num_id: &str, depth: usize) -> Option<&str> {
        let abstract_id = self
            .nums
            .iter()
            .find(|one| one.0 == num_id)
            .and_then(|one| one.1.as_deref())?;
        self.abstracts
            .iter()
            .find(|one| one.0 == abstract_id)
            .and_then(|had| {
                had.1
                    .iter()
                    .find(|lvl| lvl.0 == depth.to_string())
                    .and_then(|lvl| lvl.1.as_deref())
            })
    }
}

/// 直接孩子里的元素（`#text` 那种记号的 children 才是字，不是元素）
fn kids<'a>(node: &'a Node, want: &str) -> Vec<&'a Node> {
    node.children
        .iter()
        .filter(|one| one.name != "#text" && one.local() == want)
        .collect()
}

fn elements(node: &Node) -> Vec<&Node> {
    node.children
        .iter()
        .filter(|one| one.name != "#text")
        .collect()
}

/// 整棵子树按文档顺序的元素清单（找 `w:docPr` 与 `a:blip` 要跨层，但顺序必须是文件的顺序）
fn walk<'a>(node: &'a Node, out: &mut Vec<&'a Node>) {
    for one in elements(node) {
        out.push(one);
        walk(one, out);
    }
}

fn val(node: &Node) -> Option<&str> {
    node.attr_local("val")
}

/// 一个部件自己的关系表在 `opack` 里一次读全包，渲染只按 `r:id` 现查
fn read_part(bytes: &[u8], name: &str) -> Option<Node> {
    let member = zipread::member(bytes, name, DEFAULT_MEMBER_CAP).ok()?;
    Some(xmlscan::parse_str(&member.as_text()))
}

fn build(bytes: &[u8]) -> Book {
    let doc = opack::open(bytes);
    let (rels, _) = opack::relationships(bytes, &doc.entries);
    let mut names: Vec<(String, Option<String>)> = Vec::new();
    let mut lists: Vec<(String, Option<String>, Option<String>)> = Vec::new();
    if let Some(styles) = read_part(bytes, "word/styles.xml") {
        for one in styles.descendants("style") {
            let Some(id) = one.attr_local("styleId") else {
                continue;
            };
            let named = kids(one, "name").into_iter().next();
            names.push((
                id.to_string(),
                named.and_then(|hit| val(hit).map(String::from)),
            ));
            if let Some(props) = kids(one, "pPr").into_iter().next() {
                if let Some(hit) = kids(props, "numPr").into_iter().next() {
                    let numid = kids(hit, "numId")
                        .into_iter()
                        .next()
                        .and_then(|one| val(one).map(String::from));
                    let ilvl = kids(hit, "ilvl")
                        .into_iter()
                        .next()
                        .and_then(|one| val(one).map(String::from));
                    if numid.is_some() {
                        lists.push((id.to_string(), numid, ilvl));
                    }
                }
            }
        }
    }
    let mut nums: Vec<(String, Option<String>)> = Vec::new();
    let mut abstracts: Vec<(String, Vec<(String, Option<String>)>)> = Vec::new();
    if let Some(book) = read_part(bytes, "word/numbering.xml") {
        // `parse_str` 交回的是伪根（唯一孩子是 `w:numbering`），所以这里必须走全树而不是直接孩子
        for one in book.descendants("abstractNum") {
            let id = one
                .attr_local("abstractNumId")
                .unwrap_or_default()
                .to_string();
            let mut levels: Vec<(String, Option<String>)> = Vec::new();
            for lvl in kids(one, "lvl") {
                let depth = lvl.attr_local("ilvl").unwrap_or_default().to_string();
                let fmt = kids(lvl, "numFmt")
                    .into_iter()
                    .next()
                    .and_then(|hit| val(hit).map(String::from));
                levels.push((depth, fmt));
            }
            abstracts.push((id, levels));
        }
        for one in book.descendants("num") {
            let id = one.attr_local("numId").unwrap_or_default().to_string();
            let hit = kids(one, "abstractNumId")
                .into_iter()
                .next()
                .and_then(|one| val(one).map(String::from));
            nums.push((id, hit));
        }
    }
    Book {
        rels,
        names,
        lists,
        nums,
        abstracts,
    }
}

/// 标题层级：样式名与样式 id 两种来源（都按「heading 打头 + 数字」认），再看 `w:outlineLvl`
///
/// 两种来源都没说「这是标题」时**还要问一句 `w:outlineLvl`**（顺序与第二读者一致）：
/// 少了这一步，一个点了不带数字样式的段就再也没机会被认成标题。
fn heading_level(para: &Node, book: &Book) -> Option<u32> {
    if let Some(found) = para.descendants("pStyle").into_iter().next() {
        if let Some(id) = val(found) {
            for source in [book.style_name(id), Some(id)] {
                let Some(text) = source else { continue };
                let low = text.to_lowercase().replace("_20_", " ");
                if !(low.starts_with("heading") || low.starts_with("标题")) {
                    continue;
                }
                let digits: String = text.chars().filter(|one| one.is_ascii_digit()).collect();
                return Some(digits.parse::<u32>().unwrap_or(1).max(1));
            }
        }
    }
    let hit = para.descendants("outlineLvl").into_iter().next()?;
    val(hit)
        .and_then(|raw| raw.parse::<u32>().ok())
        .map(|raw| raw + 1)
}

/// 这一段是不是列表项：号写在段上、还是写在段点的那份样式上，两种都认
fn list_of(para: &Node, book: &Book) -> Option<(usize, Option<String>, bool)> {
    let props = kids(para, "pPr").into_iter().next();
    let on_para = props.and_then(|one| kids(one, "numPr").into_iter().next());
    if let Some(hit) = on_para {
        let numid = kids(hit, "numId")
            .into_iter()
            .next()
            .and_then(|one| val(one).map(String::from));
        let depth = kids(hit, "ilvl")
            .into_iter()
            .next()
            .and_then(|one| val(one))
            .and_then(|raw| raw.parse::<usize>().ok())
            .unwrap_or(0);
        return Some((
            depth,
            numid.and_then(|id| book.level_format(&id, depth).map(String::from)),
            false,
        ));
    }
    let found = kids(para, "pPr")
        .into_iter()
        .next()
        .and_then(|one| kids(one, "pStyle").into_iter().next())
        .or_else(|| para.descendants("pStyle").into_iter().next())?;
    let id = val(found)?;
    let (numid, ilvl) = book.style_list(id)?;
    let depth = ilvl
        .as_deref()
        .and_then(|raw| raw.parse::<usize>().ok())
        .unwrap_or(0);
    let fmt = numid
        .as_deref()
        .and_then(|id| book.level_format(id, depth))
        .map(String::from);
    Some((depth, fmt, true))
}

/// 一串字里能进正文的部分：`w:rPr` 整块跳过，tab 与 br 还原，图与域不写字
fn seg_text(node: &Node) -> String {
    let mut out = String::new();
    for one in elements(node) {
        match one.local() {
            "rPr" => {}
            "tab" => out.push(' '),
            "br" => {
                let kind = one.attr_local("type").unwrap_or("textWrapping");
                if kind == "textWrapping" {
                    out.push(HARD);
                }
            }
            "t" | "delText" => out.push_str(&one.text()),
            "drawing" | "pict" | "object" => {}
            _ => out.push_str(&seg_text(one)),
        }
    }
    out
}

/// 一张图：替代文字（`wp:docPr` 或 `pic:cNvPr` 上第一个写了 descr 的）与部件地址
fn picture(node: &Node, book: &Book) -> (String, String) {
    let mut hits: Vec<&Node> = Vec::new();
    walk(node, &mut hits);
    let mut alt = String::new();
    let mut embed: Option<String> = None;
    for one in hits {
        let name = one.local();
        if alt.is_empty() && (name == "docPr" || name == "cNvPr") {
            if let Some(hit) = one.attr("descr") {
                if !hit.is_empty() {
                    alt = hit.to_string();
                }
            }
        }
        if embed.is_none() && name == "blip" {
            embed = one
                .attrs
                .iter()
                .find(|(key, _)| key.rsplit(':').next() == Some("embed"))
                .map(|(_, value)| value.clone());
        }
    }
    let target = match &embed {
        Some(id) => book
            .target("word/document.xml", id)
            .unwrap_or_default()
            .to_string(),
        None => String::new(),
    };
    (alt, target)
}

fn run_seg(node: &Node, book: &Book, out: &mut Vec<Seg>, link: Option<String>) {
    let mut bold = false;
    let mut italic = false;
    if let Some(props) = kids(node, "rPr").into_iter().next() {
        for one in elements(props) {
            let on = match val(one) {
                Some(text) => !OFF.contains(&text),
                None => true,
            };
            match one.local() {
                "b" | "bCs" => bold = bold || on,
                "i" | "iCs" => italic = italic || on,
                _ => {}
            }
        }
    }
    let text = seg_text(node);
    if !text.is_empty() {
        let mut one = Seg::words(&text, bold, italic);
        one.link = link;
        out.push(one);
    }
    for hit in kids(node, "drawing").into_iter().chain(kids(node, "pict")) {
        let (alt, target) = picture(hit, book);
        out.push(Seg::picture(&alt, &target));
    }
}

/// 一段拆成一串带形状的片段：链接与图是壳，壳里的字带着壳出来
fn segments(para: &Node, book: &Book) -> Vec<Seg> {
    let mut out: Vec<Seg> = Vec::new();
    for one in elements(para) {
        match one.local() {
            "pPr" => {}
            "r" => run_seg(one, book, &mut out, None),
            "hyperlink" => {
                let mut inside: Vec<Seg> = Vec::new();
                for kid in elements(one) {
                    if kid.local() == "r" {
                        run_seg(kid, book, &mut inside, Some(String::new()));
                    }
                }
                let id = one.attr_local("id");
                let anchor = one.attr_local("anchor");
                let target = match (id, anchor) {
                    (Some(id), _) => book
                        .target("word/document.xml", id)
                        .unwrap_or_default()
                        .to_string(),
                    (None, Some(anchor)) => format!("#{anchor}"),
                    _ => String::new(),
                };
                for slot in &mut inside {
                    slot.link = Some(target.clone());
                }
                out.extend(inside);
            }
            "drawing" | "pict" | "object" => {
                let (alt, target) = picture(one, book);
                out.push(Seg::picture(&alt, &target));
            }
            "ins" | "smartTag" | "sdt" | "sdtContent" => out.extend(segments(one, book)),
            "del" => {}
            _ => out.extend(segments(one, book)),
        }
    }
    out
}

/// 正文里的 markdown 记号按字交；竖线只在格子里才补（表外的 `|` 不是记号）
fn esc(text: &str, pipe: bool) -> String {
    let mut out = String::with_capacity(text.len());
    for one in text.chars() {
        if one == '\\'
            || one == '*'
            || one == '_'
            || one == '`'
            || one == '['
            || one == ']'
            || one == '<'
            || one == '>'
            || (pipe && one == '|')
        {
            out.push('\\');
        }
        out.push(one);
    }
    out
}

/// 一段普通正文的行首长得像结构记号吗（标题与列表项的前缀是渲染器自己加的，不算）
fn starts_like_marker(text: &str) -> bool {
    let mut chars = text.chars();
    match chars.next() {
        Some('#') | Some('>') => true,
        Some('-') | Some('+') => chars.next() == Some(' '),
        Some(digit) if digit.is_ascii_digit() => {
            let mut rest = String::new();
            for one in chars.by_ref() {
                if one.is_ascii_digit() {
                    continue;
                }
                rest.push(one);
                break;
            }
            rest.starts_with('.') || rest.starts_with(')')
        }
        _ => false,
    }
}

/// 一串片段拼成 markdown：相邻同形状的并成一段（两家生产者拆 run 的习惯不同，
/// 同一种形状必须拼回同一个字面量，不然 `**a****b**` 这种就露出来了）
fn render(segs: &[Seg], pipe: bool) -> String {
    struct Piece {
        text: String,
        bold: bool,
        italic: bool,
        link: Option<String>,
        image: Option<(String, String)>,
    }
    let mut merged: Vec<Piece> = Vec::new();
    for one in segs {
        if let Some(hit) = &one.image {
            merged.push(Piece {
                text: String::new(),
                bold: false,
                italic: false,
                link: None,
                image: Some(hit.clone()),
            });
            continue;
        }
        match merged.last_mut() {
            Some(last)
                if last.image.is_none()
                    && last.bold == one.bold
                    && last.italic == one.italic
                    && last.link == one.link =>
            {
                last.text.push_str(&one.text);
            }
            _ => merged.push(Piece {
                text: one.text.clone(),
                bold: one.bold,
                italic: one.italic,
                link: one.link.clone(),
                image: None,
            }),
        }
    }
    let mut out = String::new();
    for one in merged {
        if let Some((alt, target)) = &one.image {
            out.push_str(&format!("![{alt}]({target})"));
            continue;
        }
        if one.text.trim().is_empty() && !one.text.contains(HARD) {
            continue;
        }
        let mut body = esc(&one.text, pipe);
        if one.bold && one.italic {
            body = format!("***{body}***");
        } else if one.bold {
            body = format!("**{body}**");
        } else if one.italic {
            body = format!("*{body}*");
        }
        if let Some(link) = &one.link {
            body = format!("[{body}]({link})");
        }
        out.push_str(&body.replace(HARD, "\\\n"));
    }
    out
}

/// 一个格子的字：多段用 `<br>` 连，竖线在这里才转义
fn cell_text(tc: &Node, book: &Book) -> String {
    let mut bits: Vec<String> = Vec::new();
    for one in kids(tc, "p") {
        let made = render(&segments(one, book), true);
        let flat = made.trim().replace('\n', "<br>");
        if !flat.is_empty() {
            bits.push(flat);
        }
    }
    bits.join("<br>")
}

fn table_md(tbl: &Node, book: &Book) -> String {
    let mut rows: Vec<Vec<String>> = Vec::new();
    for tr in kids(tbl, "tr") {
        rows.push(
            kids(tr, "tc")
                .into_iter()
                .map(|one| cell_text(one, book))
                .collect(),
        );
    }
    if rows.is_empty() {
        return String::new();
    }
    let width = rows.iter().map(|one| one.len()).max().unwrap_or(0);
    for one in &mut rows {
        while one.len() < width {
            one.push(String::new());
        }
    }
    let line = |cells: &[String]| -> String { format!("| {} |", cells.join(" | ")) };
    let mut out = line(&rows[0]);
    out.push('\n');
    let filler: Vec<String> = (0..width).map(|_| "---".to_string()).collect();
    out.push_str(&line(&filler));
    for one in rows.iter().skip(1) {
        out.push('\n');
        out.push_str(&line(one));
    }
    out
}

/// ODF 那一族渲染要查的两张样式表（都在 content.xml 与 styles.xml 里**先到先得**）
struct OdfStyles {
    /// 字符样式名 → (粗, 斜)
    chars: Vec<(String, (bool, bool))>,
    /// 列表样式名 → (层级号 `text:level`, 这一级是 bullet 还是 number)
    lists: Vec<(String, Vec<(String, String)>)>,
}

/// ODF 那一族的运行态：统计、块清单与两张样式表
struct OdfRun {
    styles: OdfStyles,
    blocks: Vec<(bool, String)>,
    counts: Vec<(&'static str, i64)>,
}

const ODF_KEYS: [&str; 19] = [
    "paragraphs",
    "headings",
    "list_items",
    "bullet_items",
    "ordered_items",
    "unresolved_fmt",
    "tables",
    "table_rows",
    "empty_dropped",
    "lists_named",
    "lists_unnamed",
    "spans_unresolved",
    "annotations_dropped",
    "notes_dropped",
    "space_markers",
    "links",
    "images",
    "covered_cells",
    "repeated_spans",
];

impl OdfRun {
    fn new() -> OdfRun {
        OdfRun {
            styles: OdfStyles {
                chars: Vec::new(),
                lists: Vec::new(),
            },
            blocks: Vec::new(),
            counts: ODF_KEYS.iter().map(|one| (*one, 0i64)).collect(),
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
}

fn local_attr(node: &Node, want: &str) -> Option<&str> {
    node.attr_local(want)
}

/// 两份件都走，**先到先得**（与编号那一条同一口径）
///
/// 实测：`text:span` 点的字符样式在 content.xml（自动样式）与 styles.xml（命名样式）里都有，
/// 列表样式 `text:list-style` 全在 styles.xml（这份语料 300 条、content 里 0 条）。
fn collect_odf_styles(bytes: &[u8], run: &mut OdfRun) {
    for part in ["content.xml", "styles.xml"].iter() {
        let Some(root) = read_part(bytes, part) else {
            continue;
        };
        for one in root.descendants("style") {
            if local_attr(one, "family") != Some("text") {
                continue;
            }
            let Some(name) = local_attr(one, "name") else {
                continue;
            };
            if run.styles.chars.iter().any(|had| had.0 == name) {
                continue;
            }
            let props = match one.child("text-properties") {
                Some(had) => had,
                None => one,
            };
            // ElementTree 那边按命名空间 URI 取，这里按局部名取：实测同一元素上
            // 只有 `fo:font-weight` 与 `fo:font-style` 落成这两个局部名
            //（`style:font-weight-asian` 那种是另一个局部名），所以两边同一条判据。
            let weight = local_attr(props, "font-weight");
            let posture = local_attr(props, "font-style");
            let flags = (
                weight.map_or(false, |raw| raw != "normal" && raw != "0"),
                posture.map_or(false, |raw| raw != "normal" && raw != "none" && raw != "0"),
            );
            run.styles.chars.push((name.to_string(), flags));
        }
        for one in root.descendants("list-style") {
            let Some(name) = local_attr(one, "name") else {
                continue;
            };
            if run.styles.lists.iter().any(|had| had.0 == name) {
                continue;
            }
            let mut levels: Vec<(String, String)> = Vec::new();
            for lvl in one.children.iter().filter(|kid| kid.name != "#text") {
                let kind = match lvl.local() {
                    "list-level-style-bullet" => "bullet",
                    "list-level-style-number" => "number",
                    _ => continue,
                };
                if let Some(depth) = local_attr(lvl, "level") {
                    levels.push((depth.to_string(), kind.to_string()));
                }
            }
            run.styles.lists.push((name.to_string(), levels));
        }
    }
}

impl OdfRun {
    fn list_format(&self, name: &str, depth: usize) -> Option<&str> {
        self.styles
            .lists
            .iter()
            .find(|one| one.0 == name)
            .and_then(|had| {
                had.1
                    .iter()
                    .find(|lvl| lvl.0 == depth.to_string())
                    .map(|lvl| lvl.1.as_str())
            })
    }

    fn char_flags(&self, name: &str) -> Option<(bool, bool)> {
        self.styles
            .chars
            .iter()
            .find(|one| one.0 == name)
            .map(|one| one.1)
    }
}

/// 一棵子树按文档顺序摊成片段
///
/// ODF 的字挂在元素的 `.text` 与孩子们的 `.tail` 上，而 xmlscan 把这两处都存成 `#text`
/// **孩子**（保住顺序），所以这里天然分得清「谁在说什么话」：孩子的字带孩子的形状，
/// 孩子后面那段是**父亲**的话，用父亲的形状交 —— 漏了它，「两处空格 之间是一个记号」
/// 会只剩前半句（真件量到的：`<text:p>两处空格 <text:s/>之间是一个记号</text:p>`）。
///
/// 批注（`text:annotation`）、注（`text:note`）、修订表（`text:tracked-changes`）整块不算
/// 正文的字 —— 与 docx 那一本同一口径（那边的这些字住在**别的部件**里）；
/// `text:soft-page-break` 是渲染时落下的位置，也不写字。
fn odf_inline(
    node: &Node,
    run: &mut OdfRun,
    out: &mut Vec<Seg>,
    link: Option<String>,
    bold: bool,
    italic: bool,
) {
    for one in node.children.iter() {
        if one.name == "#text" {
            if !one.direct.is_empty() {
                out.push(Seg::said(&one.direct, bold, italic, link.clone()));
            }
            continue;
        }
        match one.local() {
            "annotation" => run.bump("annotations_dropped"),
            "note" | "tracked-changes" => run.bump("notes_dropped"),
            "soft-page-break" => {}
            "s" => {
                let raw = local_attr(one, "c").unwrap_or("1");
                let wide: usize = raw.parse().unwrap_or(1).min(64);
                run.bump("space_markers");
                out.push(Seg::said(&" ".repeat(wide), bold, italic, link.clone()));
            }
            "tab" => out.push(Seg::said("\t", bold, italic, link.clone())),
            "line-break" => out.push(Seg::said("\u{0}", bold, italic, link.clone())),
            "span" => {
                let name = local_attr(one, "style-name").unwrap_or_default();
                let flags = match run.char_flags(name) {
                    Some(had) => had,
                    None => {
                        run.bump("spans_unresolved");
                        (false, false)
                    }
                };
                odf_inline(
                    one,
                    run,
                    out,
                    link.clone(),
                    bold || flags.0,
                    italic || flags.1,
                );
            }
            "a" => {
                let href = local_attr(one, "href").unwrap_or_default().to_string();
                run.bump("links");
                odf_inline(one, run, out, Some(href), bold, italic);
            }
            "frame" | "object" => {
                let image = one
                    .descendants("image")
                    .into_iter()
                    .next()
                    .and_then(|hit| local_attr(hit, "href"))
                    .unwrap_or_default()
                    .to_string();
                run.bump("images");
                out.push(Seg::picture("", &image));
            }
            _ => odf_inline(one, run, out, link.clone(), bold, italic),
        }
    }
}

fn odf_line(par: &Node, run: &mut OdfRun, pipe: bool) -> String {
    let mut segs: Vec<Seg> = Vec::new();
    odf_inline(par, run, &mut segs, None, false, false);
    render(&segs, pipe)
}

fn odf_cell_text(tc: &Node, run: &mut OdfRun) -> String {
    let mut bits: Vec<String> = Vec::new();
    for one in tc.children.iter().filter(|kid| kid.name != "#text") {
        if !matches!(one.local(), "p" | "h") {
            continue;
        }
        let flat = odf_line(one, run, true).trim().replace('\n', "<br>");
        if !flat.is_empty() {
            bits.push(flat);
        }
    }
    bits.join("<br>")
}

fn odf_table_md(tbl: &Node, run: &mut OdfRun) -> String {
    let mut rows: Vec<Vec<String>> = Vec::new();
    for tr in tbl.children.iter().filter(|kid| kid.local() == "table-row") {
        let mut cells: Vec<String> = Vec::new();
        for tc in tr
            .children
            .iter()
            .filter(|kid| matches!(kid.local(), "table-cell" | "covered-table-cell"))
        {
            if tc.local() == "covered-table-cell" {
                run.bump("covered_cells");
            }
            let made = odf_cell_text(tc, run);
            let raw = local_attr(tc, "number-columns-repeated").unwrap_or("1");
            let times: usize = raw.parse().unwrap_or(1);
            if times > 1 {
                run.bump_by("repeated_spans", times as i64 - 1);
            }
            for _ in 0..times.min(64) {
                cells.push(made.clone());
            }
        }
        rows.push(cells);
    }
    if rows.is_empty() {
        return String::new();
    }
    let width = rows.iter().map(|one| one.len()).max().unwrap_or(0);
    for one in &mut rows {
        while one.len() < width {
            one.push(String::new());
        }
    }
    let line = |cells: &[String]| -> String { format!("| {} |", cells.join(" | ")) };
    let mut out = line(&rows[0]);
    out.push('\n');
    let filler: Vec<String> = (0..width).map(|_| "---".to_string()).collect();
    out.push_str(&line(&filler));
    for one in rows.iter().skip(1) {
        out.push('\n');
        out.push_str(&line(one));
    }
    out
}

/// 一段（或一个标题）落成 markdown 的一个块；层级与列表记号各按 ODF 自己的写法认
fn odf_emit(par: &Node, run: &mut OdfRun, depth: usize, list_name: &Option<String>, heading: bool) {
    let text = odf_line(par, run, false).trim().to_string();
    if text.is_empty() {
        run.bump("empty_dropped");
        return;
    }
    if heading {
        if let Some(level) = local_attr(par, "outline-level")
            .and_then(|raw| raw.parse::<u32>().ok())
            .map(|raw| raw.max(1))
        {
            run.bump("headings");
            run.blocks
                .push((false, format!("{} {}", "#".repeat(level as usize), text)));
            return;
        }
    }
    if depth > 0 {
        run.bump("list_items");
        let fmt = match list_name {
            Some(name) => run.list_format(name, depth).map(String::from),
            None => None,
        };
        let indent = "  ".repeat(depth - 1);
        if fmt.as_deref() == Some("number") {
            run.bump("ordered_items");
            run.blocks.push((true, format!("{indent}1. {text}")));
        } else {
            if fmt.is_none() {
                run.bump("unresolved_fmt");
            }
            run.bump("bullet_items");
            run.blocks.push((true, format!("{indent}- {text}")));
        }
        return;
    }
    run.bump("paragraphs");
    let head = starts_like_marker(&text);
    run.blocks
        .push((false, if head { format!("\\{text}") } else { text }));
}

impl OdfRun {
    fn walk(&mut self, node: &Node, depth: usize, list_name: &Option<String>) {
        for one in node.children.iter().filter(|kid| kid.name != "#text") {
            match one.local() {
                "h" | "p" => {
                    let name = list_name.clone();
                    odf_emit(one, self, depth, &name, one.local() == "h");
                }
                "list" => {
                    let raw = local_attr(one, "style-name");
                    match raw {
                        Some(_) => self.bump("lists_named"),
                        None => self.bump("lists_unnamed"),
                    }
                    let name = raw.map(String::from).or_else(|| list_name.clone());
                    self.walk(one, depth + 1, &name);
                }
                "list-item" => {
                    let name = list_name.clone();
                    self.walk(one, depth, &name);
                }
                "table" => {
                    let made = odf_table_md(one, self);
                    if !made.is_empty() {
                        self.bump("tables");
                        let rows = one
                            .children
                            .iter()
                            .filter(|kid| kid.local() == "table-row")
                            .count();
                        self.bump_by("table_rows", rows as i64);
                        self.blocks.push((false, made));
                    }
                }
                _ => {
                    let name = list_name.clone();
                    self.walk(one, depth, &name);
                }
            }
        }
    }
}

/// 一份 odt 的结构渲染（`--markdown` 开着且这一族是 ODF 时走这里）
pub(crate) fn odf(bytes: &[u8], budget: usize) -> Value {
    let Some(content) = read_part(bytes, "content.xml") else {
        return json!({"family": "odf", "available": false});
    };
    let mut run = OdfRun::new();
    collect_odf_styles(bytes, &mut run);
    let body = content
        .descendants("body")
        .into_iter()
        .next()
        .and_then(|had| had.child("text"));
    if let Some(text) = body {
        let empty: Option<String> = None;
        run.walk(&text, 0, &empty);
    }
    let mut joined = String::new();
    for (index, (is_list, block)) in run.blocks.iter().enumerate() {
        if index > 0 {
            joined.push_str(if *is_list && run.blocks[index - 1].0 {
                "\n"
            } else {
                "\n\n"
            });
        }
        joined.push_str(block);
    }
    if !run.blocks.is_empty() {
        joined.push('\n');
    }
    let chars = joined.chars().count();
    let cut = chars > budget;
    let shown: String = joined.chars().take(budget).collect();
    let mut mine = serde_json::Map::new();
    mine.insert("family".to_string(), json!("odf"));
    mine.insert("available".to_string(), json!(true));
    mine.insert("text".to_string(), json!(shown));
    mine.insert("chars".to_string(), json!(chars));
    mine.insert("cut".to_string(), json!(cut));
    mine.insert("blocks".to_string(), json!(run.blocks.len()));
    for (key, value) in run.counts.iter() {
        mine.insert((*key).to_string(), json!(value));
    }
    Value::Object(mine)
}

/// 一份 docx 的正文渲染结果（`--markdown` 开着才走到这里）
pub(crate) fn docx(bytes: &[u8], budget: usize) -> Value {
    let Some(root) = read_part(bytes, "word/document.xml") else {
        return json!({"family": "ooxml", "available": false});
    };
    let book = build(bytes);
    let body = root.descendants("body").into_iter().next().unwrap_or(&root);
    let mut blocks: Vec<(bool, String)> = Vec::new();
    let mut stats = json!({
        "paragraphs": 0, "headings": 0, "list_items": 0, "bullet_items": 0,
        "ordered_items": 0, "list_from_style": 0, "unresolved_fmt": 0,
        "tables": 0, "table_rows": 0, "empty_dropped": 0,
    });
    let mut bump = |name: &str, by: i64| {
        let slot = stats[name].as_i64().unwrap_or(0) + by;
        stats[name] = json!(slot);
    };
    for one in elements(body) {
        match one.local() {
            "p" => {
                let text = render(&segments(one, &book), false).trim().to_string();
                if text.is_empty() {
                    bump("empty_dropped", 1);
                    continue;
                }
                if let Some(level) = heading_level(one, &book) {
                    bump("headings", 1);
                    blocks.push((false, format!("{} {}", "#".repeat(level as usize), text)));
                    continue;
                }
                if let Some((depth, fmt, from_style)) = list_of(one, &book) {
                    bump("list_items", 1);
                    if from_style {
                        bump("list_from_style", 1);
                    }
                    let indent = "  ".repeat(depth);
                    match fmt.as_deref() {
                        None => {
                            bump("unresolved_fmt", 1);
                            bump("bullet_items", 1);
                            blocks.push((true, format!("{indent}- {text}")));
                        }
                        Some("bullet") => {
                            bump("bullet_items", 1);
                            blocks.push((true, format!("{indent}- {text}")));
                        }
                        Some(_) => {
                            bump("ordered_items", 1);
                            blocks.push((true, format!("{indent}1. {text}")));
                        }
                    }
                    continue;
                }
                bump("paragraphs", 1);
                let head = starts_like_marker(&text);
                blocks.push((false, if head { format!("\\{text}") } else { text }));
            }
            "tbl" => {
                let made = table_md(one, &book);
                if !made.is_empty() {
                    bump("tables", 1);
                    bump("table_rows", kids(one, "tr").len() as i64);
                    blocks.push((false, made));
                }
            }
            _ => {}
        }
    }
    let mut text = String::new();
    for (index, (is_list, block)) in blocks.iter().enumerate() {
        if index > 0 {
            text.push_str(if *is_list && blocks[index - 1].0 {
                "\n"
            } else {
                "\n\n"
            });
        }
        text.push_str(block);
    }
    if !blocks.is_empty() {
        text.push('\n');
    }
    let chars = text.chars().count();
    let cut = chars > budget;
    let shown: String = text.chars().take(budget).collect();
    json!({
        "family": "ooxml",
        "available": true,
        "text": shown,
        "chars": chars,
        "cut": cut,
        "blocks": blocks.len(),
        "paragraphs": stats["paragraphs"],
        "headings": stats["headings"],
        "list_items": stats["list_items"],
        "bullet_items": stats["bullet_items"],
        "ordered_items": stats["ordered_items"],
        "list_from_style": stats["list_from_style"],
        "unresolved_fmt": stats["unresolved_fmt"],
        "tables": stats["tables"],
        "table_rows": stats["table_rows"],
        "empty_dropped": stats["empty_dropped"],
    })
}
