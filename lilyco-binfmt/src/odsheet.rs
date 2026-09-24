//! ODF 电子表格（`.ods`）：`content.xml` 里的 `table:table`。
//!
//! 这一家跟 OOXML 是两套脾气，三处坑各绊一次：
//!
//! 1. **格子里不写序列数**。日期直接是 `office:date-value="2013-12-23"`，
//!    百分比写 `office:value="0.125"` 而显示文本是 `12.5%`，布尔写
//!    `office:boolean-value="true"` 而显示是 `TRUE`。所以这里没有 1900/1904 那套基准
//!    要猜 —— 但也别把显示文本当成值。
//! 2. **格子没有名字**。位置得自己数：`table:number-columns-repeated` 说的是「这一格
//!    顶 N 格」，LibreOffice 给行尾那片空行填的是 16381、16382（一张表的列数上限），
//!    照字面数就成了一万六千格；它也会出现在**行首**（前两个格子空着），
//!    所以列号必须一路累加，不然 `C2` 会数成 `A2`。行数同理是
//!    `table:number-rows-repeated`。
//! 3. **表可不可见不在表上**。`table:table` 只写 `table:style-name="ta3"`，
//!    那个自动样式的 `style:table-properties` 里的 `table:display="false"` 才是隐藏。
//!    跟 xlsx 格子的 `s=` 要绕 `cellXfs` 是同一类账：值在别处。
//!
//! 合并区在这里是「跨 N 列的格子 + 后面跟着 `covered:table-cell`/`table:covered-table-cell`」，
//! 覆盖格不是内容，单列一个数。
//!
//! 与 `scripts/acceptance/office_reader.py` 的 `ods_facts()` 是同一套规范的两份实现；
//! 那一份还顺手跟 `meta.xml` 里 LibreOffice **自己写的** `cell-count` 对过账
//! （book.ods 与 formats.ods 都是 11），所以这不是「自己跟自己对账」。

use serde_json::{json, Value};

use crate::xmlscan::{self, Node};
use crate::zipread;

const CONTENT_PART: &str = "content.xml";
const MEMBER_CAP: u64 = 8 << 20;
/// 一行里最多走多少个格子元素：生产者把整行的空白压成一个重复元素，
/// 但坏文件可以一个接一个写，这里给一个足够大又不至于转不完的数
const MAX_CELL_ELEMENTS: usize = 20_000;
/// 列/行的逐条尺寸账同样要有上限：一条 `<table:table-row/>` 只要十几个字节，
/// 一份 8 MB 的件能写几十万条，逐条存下去就是把内存交给生产者。
/// 计数照数，存不下就不存，并在这条族里说清楚
const MAX_SIZE_ELEMENTS: usize = 4096;
/// 表元素自己可以写的那四个数。逐个查、逐个交，没写的交 null ——
/// 实测 LibreOffice 转出来的六份 .ods 一个都不写，但「没写」与「写了 0」不是一回事
const STATED_ON_TABLE: [&str; 4] = [
    "number-columns",
    "number-rows",
    "default-column-width",
    "default-row-height",
];

/// 一个有内容的格子
#[derive(Debug, Clone)]
pub struct Cell {
    pub reference: String,
    /// `office:value-type`：**文件写了的那个类型，没写就是 null**。
    /// 这个属性可以省略，而省略时该按什么算，两份读者原本的猜法并不一样：
    /// 一份按「有没有字」推 string / empty，另一份一律写 empty —— 它们在 .ods 上
    /// 恰好一直没撞见（六份件里进了账本的格子全部写了这个属性），到 odp 就撞开了。
    /// 实测 Impress 页上那张表 9 个格子元素一个都没写，其中 7 个还是有字的 ——
    /// 所以这里不推断：没写就交 null，两种猜法都不替文件圆话
    pub value_type: Option<String>,
    pub value: Option<String>,
    pub date_value: Option<String>,
    pub boolean_value: Option<String>,
    pub formula: Option<String>,
    pub text: String,
    /// 这一格里有几段带了另一份字符样式（`text:span`）：字照抄进 `text`，样式名不追
    pub spans: usize,
    /// 这一格里 `text:s` / `text:tab` / `text:line-break` 这几个记号的条数（展开见下）
    pub specials: usize,
    /// 这一格引用的单元格样式名（`table:style-name`）：格式在它的 `style:data-style-name` 那一跳后面
    pub style_name: Option<String>,
    pub columns_spanned: usize,
    pub rows_spanned: usize,
}

impl Cell {
    pub fn to_json(&self) -> Value {
        json!({
            "ref": self.reference,
            "kind": self.value_type,
            "value": numeric_or_text(self.value.as_deref()),
            "date_value": self.date_value,
            "boolean_value": self.boolean_value,
            "formula": self.formula,
            "text": self.text,
            "spans": self.spans,
            "specials": self.specials,
            "columns_spanned": self.columns_spanned,
            "rows_spanned": self.rows_spanned,
        })
    }
}

/// 一条列/行元素的尺寸账。这一族的宽度与高度**一律不在元素上**：元素只写
/// `number-columns-repeated` / `number-rows-repeated` 与（偶尔）`table:visibility`，
/// 尺寸在它点名的那份自动样式里，所以每条都说清「点了哪个名」「样式找着没找着」
#[derive(Debug, Clone, Default)]
pub struct SizeInfo {
    /// 元素点名的样式（`table:style-name`），没点名是 None
    pub style: Option<String>,
    /// 这一条顶几列 / 几行（文件写的 repeated，没写按 1 算）
    pub repeated: usize,
    /// 元素自己写的 `table:visibility`（没写是 None，不替它填 visible）
    pub element_visibility: Option<String>,
    /// 样式里那条 `style:column-width` / `style:row-height`，按写的串交
    pub size: Option<String>,
    /// 样式里的 `style:use-optimal-column-width` / `…-row-height` 串
    pub optimal: Option<String>,
    /// 样式里的 `style:visibility`（隐藏的另一条来路，与上面那条各记各的）
    pub style_visibility: Option<String>,
    /// 样式自己的 `style:parent-style-name`。**这里不顺父链再跳**：实测的文件里
    /// 一个都没写，尺寸挂在父样式上时这一条就正好说出「为什么不认识」
    pub style_parent: Option<String>,
    /// 点名的样式在这份件里找着了吗（没点名也算没找着）
    pub resolved: bool,
}

/// 一份行/列自动样式上抄下来的那几样 —— 就是上面那一跳的落点
#[derive(Debug, Clone, Default)]
struct SizeStyle {
    name: String,
    size: Option<String>,
    optimal: Option<String>,
    visibility: Option<String>,
    parent: Option<String>,
}

/// 一条轴（列或行）上的总账。逐条账本可能因为上限只存了前一段，
/// 而这几个数是**全部**元素加出来的：「几条元素」「盖住几列」「看得见的几列」是三本账
#[derive(Debug, Clone, Default)]
pub struct SizeTally {
    /// 走到的元素条数
    pub elements: usize,
    /// 这些元素一共盖住几列 / 几行（`number-*-repeated` 累加）
    pub spans: usize,
    /// 点了名、且那份样式在这件里找着的条数
    pub resolved: usize,
    /// 样式里报得出尺寸的条数
    pub with_size: usize,
    /// 样式里写了 `use-optimal-*` 的条数
    pub optimal: usize,
    /// 元素自己写了 `table:visibility` 的条数（没写的交 null，不替它填 visible）
    pub spoken_visibility: usize,
}

/// 一张表
#[derive(Debug, Clone)]
pub struct Sheet {
    pub name: String,
    pub visible: bool,
    pub rows: usize,
    pub columns: usize,
    pub cells: Vec<Cell>,
    pub covered: usize,
    pub merged: usize,
    /// 隐藏的行数与列数：`table:visibility="collapse"` 可以直接写在行/列上，
    /// 也可以只写在它引的那个自动样式里，两边都得看
    pub hidden_rows: usize,
    pub hidden_cols: usize,
    /// 逐条列元素的尺寸账
    pub col_sizes: Vec<SizeInfo>,
    /// 逐条行元素的尺寸账
    pub row_sizes: Vec<SizeInfo>,
    /// 列/行的总账（全部元素加出来的，账本截断了也照加）
    pub col_tally: SizeTally,
    pub row_tally: SizeTally,
    /// 表元素自己写的那四个数（见 `STATED_ON_TABLE`）：按名字列出来，没写的交 null
    pub stated: Vec<(String, Option<String>)>,
    /// 这张表里的批注：`{ref, author, date, text}`。ODF 的批注**坐在格子里面**
    /// （`office:annotation` 是 `table:table-cell` 的孩子），所以取格子的字时要跳过它 ——
    /// 与 .odt 那边「批注与修订表里的段不算正文」是同一条规矩
    pub comments: Vec<Value>,
}

/// 格子里那些「算这一格内容」的段：批注子树整个跳过。
///
/// ODF 把空格与制表符**写成记号而不是字面**：`  两头有空格  ` 是
/// `<text:s/><text:s text:c="2"/>两头有空格<text:s/><text:s/>`（`text:c` 说这一个记号顶
/// 几个空格，没写就是一个），制表符是 `<text:tab/>`，段内换行是 `<text:line-break/>`。
/// 不展开就一个空格也读不出来 —— 而 LibreOffice 把同一份 .ods 自己导成 CSV 时那些空格
/// 一个不少（`rich.ods` 与 `rich.xlsx` 那两份 CSV 一模一样），所以这里照文件的写法展开。
/// 段首尾也不 trim：那一格的字就是文件写的那一串。
fn plain_paragraphs(
    node: &xmlscan::Node,
    out: &mut Vec<String>,
    spans: &mut usize,
    specials: &mut usize,
) {
    for one in node.children.iter() {
        if one.is("annotation") {
            continue;
        }
        if one.is("p") {
            let mut body = String::new();
            paragraph_text(one, &mut body, spans, specials);
            out.push(body);
            continue;
        }
        plain_paragraphs(one, out, spans, specials);
    }
}

/// 一段里的字，按 ODF 的写法展开（`spans` 数 `text:span`，`specials` 数那三种记号）
fn paragraph_text(node: &xmlscan::Node, out: &mut String, spans: &mut usize, specials: &mut usize) {
    for one in node.children.iter() {
        if one.is("annotation") {
            continue;
        }
        if one.local() == "#text" {
            out.push_str(&one.direct);
            continue;
        }
        if one.is("s") {
            // 一个记号顶几个空格。上限是给坏文件留的：`text:c` 是文件自己写的数，
            // 而这里展开的是内存 —— 条数照数，不替它把那一格撑爆
            let times = attr_of(one, "c")
                .and_then(|raw| raw.trim().parse::<usize>().ok())
                .unwrap_or(1)
                .min(4096);
            for _ in 0..times {
                out.push(' ');
            }
            *specials += 1;
            continue;
        }
        if one.is("tab") {
            out.push('\t');
            *specials += 1;
            continue;
        }
        if one.is("line-break") {
            out.push('\n');
            *specials += 1;
            continue;
        }
        if one.is("span") {
            *spans += 1;
        }
        paragraph_text(one, out, spans, specials);
    }
}

/// 这一格上的批注元素（可以有不止一条）
fn annotation_nodes<'a>(node: &'a xmlscan::Node) -> Vec<&'a xmlscan::Node> {
    let mut out: Vec<&'a xmlscan::Node> = Vec::new();
    for one in node.children.iter() {
        if one.is("annotation") {
            out.push(one);
        } else {
            out.extend(annotation_nodes(one));
        }
    }
    out
}

/// 批注的作者与时间挂在它的孩子元素上：`dc:creator`、`meta:date-string`（或 `dc:date`）。
/// 空的 `<meta:date-string/>` 算「没写」而不是「写了空时间」
fn note_field(node: &xmlscan::Node, wants: &[&str]) -> Option<String> {
    wants.iter().find_map(|want| {
        node.descendants(want)
            .first()
            .map(|one| one.text().trim().to_string())
            .filter(|had| !had.is_empty())
    })
}

impl Sheet {
    pub fn count_of(&self, want: &str) -> usize {
        self.cells
            .iter()
            .filter(|one| one.value_type.as_deref() == Some(want))
            .count()
    }

    pub fn date_cells(&self) -> usize {
        self.count_of("date") + self.count_of("time")
    }

    pub fn formulas(&self) -> usize {
        self.cells
            .iter()
            .filter(|one| one.formula.is_some())
            .count()
    }
}

/// 一份 ODF 表格
#[derive(Debug, Default)]
pub struct Book {
    pub sheets: Vec<Sheet>,
    pub notes: Vec<String>,
}

/// 一个数字就报成数字，报不动就照原样给文本
fn numeric_or_text(raw: Option<&str>) -> Value {
    match raw {
        Some(one) => match one.parse::<f64>() {
            Ok(got) => json!(got),
            Err(_) => json!(one),
        },
        None => Value::Null,
    }
}

/// 按局部名取属性（`table:number-columns-repeated` 与 `number-columns-repeated` 都认）：
/// ODF 的前缀虽然照着规范写，但**声明是文件自己做的**，只认死前缀就是给自己埋雷。
/// 唯一要躲开的是 LibreOffice 那份实验命名空间：`calcext:value-type` 是照着
/// `office:value-type` 抄的一份，撞上它就等于信了副本。
pub(crate) fn attr_of<'a>(node: &'a Node, local: &str) -> Option<&'a str> {
    node.attrs
        .iter()
        .filter(|(key, _)| key.rsplit(':').next().unwrap_or_default() == local)
        .find(|(key, _)| !key.starts_with("calcext:"))
        .map(|(_, value)| value.as_str())
}

/// 「这一元素顶 N 个」：没写就是 1，写坏了也是 1（不猜）
fn repeated(node: &Node, local: &str) -> usize {
    attr_of(node, local)
        .and_then(|one| one.trim().parse::<usize>().ok())
        .filter(|one| *one > 0)
        .unwrap_or(1)
}

/// 一条轴的总账：逐条存不存得下都要照加（存不下只发生在坏文件身上，
/// 而那时候更要让「几条元素」「盖住几列」这两个数是真的）
fn tally(had_tally: &mut SizeTally, had: &SizeInfo) {
    had_tally.elements += 1;
    had_tally.spans += had.repeated;
    had_tally.resolved += usize::from(had.resolved);
    had_tally.with_size += usize::from(had.size.is_some());
    had_tally.optimal += usize::from(had.optimal.is_some());
    had_tally.spoken_visibility += usize::from(had.element_visibility.is_some());
}

fn col_letter(index: usize) -> String {
    let mut out = String::new();
    let mut at = index;
    loop {
        out.insert(0, (b'A' + (at % 26) as u8) as char);
        if at < 26 {
            return out;
        }
        at = at / 26 - 1;
    }
}

/// `office:value-type` 与 `calcext:value-type` 局部名相同（后者是 LibreOffice 的实验
/// 命名空间，抄了一份出来）。`attr_of` 已经躲开那个副本，这里只要那一句原话：
/// 没写就是没写，不替文件推一个类型出来
fn value_type(cell: &Node) -> Option<String> {
    attr_of(cell, "value-type").map(|one| one.to_string())
}

/// 读一份 ODF 表格的 `content.xml`。读不出来不报错，写进 notes。
pub fn read(bytes: &[u8]) -> Book {
    let mut book = Book::default();
    let member = match zipread::member(bytes, CONTENT_PART, MEMBER_CAP) {
        Ok(one) => one,
        Err(why) => {
            book.notes.push(format!("读不到 {CONTENT_PART}：{why}"));
            return book;
        }
    };
    let root = xmlscan::parse_str(&member.as_text());
    // 表 → 它引的自动样式 → 那个样式的 table:display
    let mut shown: Vec<(String, bool)> = Vec::new();
    // 行与列的隐藏也有两种写法：`table:visibility="collapse"` 直接写在行/列上，
    // 或者只写在它引的那个自动样式的 row/column-properties 里。LibreOffice 转出来
    // 的这份用前一种，而只查一种的读者会把藏起来的行整批当成正常的
    let mut folded: Vec<(String, bool)> = Vec::new();
    // 同一跳顺手把尺寸也抄下来：列的 `style:column-width` 与行的 `style:row-height`，
    // 连 `style:use-optimal-*` 一起（LibreOffice 常常两根都写：给了高度又说「按最优」）
    let mut col_sized: Vec<SizeStyle> = Vec::new();
    let mut row_sized: Vec<SizeStyle> = Vec::new();
    for style in root.descendants("style") {
        let family = attr_of(style, "family").unwrap_or_default();
        let props = match family {
            "table" => {
                let name = attr_of(style, "name").unwrap_or_default().to_string();
                let display = style
                    .child("table-properties")
                    .and_then(|one| attr_of(one, "display"))
                    .unwrap_or("true");
                shown.push((name, display != "false"));
                continue;
            }
            "table-row" => style.child("table-row-properties"),
            "table-column" => style.child("table-column-properties"),
            _ => continue,
        };
        let name = attr_of(style, "name").unwrap_or_default().to_string();
        let hidden = props
            .and_then(|one| attr_of(one, "visibility"))
            .unwrap_or("visible")
            == "collapse";
        folded.push((name.clone(), hidden));
        let size = match family {
            "table-column" => props.and_then(|one| attr_of(one, "column-width")),
            "table-row" => props.and_then(|one| attr_of(one, "row-height")),
            _ => None,
        }
        .map(String::from);
        let optimal = match family {
            "table-column" => props.and_then(|one| attr_of(one, "use-optimal-column-width")),
            "table-row" => props.and_then(|one| attr_of(one, "use-optimal-row-height")),
            _ => None,
        }
        .map(String::from);
        let found = SizeStyle {
            name: name.clone(),
            size,
            optimal,
            visibility: props
                .and_then(|one| attr_of(one, "visibility"))
                .map(String::from),
            parent: attr_of(style, "parent-style-name").map(String::from),
        };
        if family == "table-column" {
            col_sized.push(found);
        } else if family == "table-row" {
            row_sized.push(found);
        }
    }
    let folded_by_style = |name: Option<&str>| -> bool {
        match name {
            Some(want) => folded.iter().any(|(one, flag)| one == want && *flag),
            None => false,
        }
    };
    for table in root.descendants("table") {
        let mut sheet = Sheet {
            name: attr_of(table, "name").unwrap_or_default().to_string(),
            visible: true,
            rows: 0,
            columns: 0,
            cells: Vec::new(),
            covered: 0,
            merged: 0,
            hidden_rows: 0,
            hidden_cols: 0,
            col_sizes: Vec::new(),
            row_sizes: Vec::new(),
            col_tally: SizeTally::default(),
            row_tally: SizeTally::default(),
            stated: STATED_ON_TABLE
                .iter()
                .map(|want| ((*want).to_string(), attr_of(table, *want).map(String::from)))
                .collect(),
            comments: Vec::new(),
        };
        if let Some(style) = attr_of(table, "style-name") {
            if let Some((_, flag)) = shown.iter().find(|(one, _)| one == style) {
                sheet.visible = *flag;
            }
        }
        // 列：LibreOffice 把一片连续的同款列压成一个带 repeated 的元素，
        // 隐藏的三列就写成一条 visibility="collapse" + repeated="3"
        for column in table.all("table-column") {
            let span = repeated(column, "number-columns-repeated");
            let seen = attr_of(column, "visibility");
            let hidden = seen == Some("collapse") || folded_by_style(attr_of(column, "style-name"));
            if hidden {
                sheet.hidden_cols += span;
            }
            // 宽度不在这个元素上：一跳在它点名的那份自动样式里
            let style = attr_of(column, "style-name");
            let hit = style.and_then(|want| col_sized.iter().find(|one| one.name == want));
            let had = SizeInfo {
                style: style.map(String::from),
                repeated: span,
                element_visibility: seen.map(String::from),
                size: hit.and_then(|one| one.size.clone()),
                optimal: hit.and_then(|one| one.optimal.clone()),
                style_visibility: hit.and_then(|one| one.visibility.clone()),
                style_parent: hit.and_then(|one| one.parent.clone()),
                resolved: hit.is_some(),
            };
            tally(&mut sheet.col_tally, &had);
            if sheet.col_sizes.len() < MAX_SIZE_ELEMENTS {
                sheet.col_sizes.push(had);
            }
        }
        let mut row_at = 0usize;
        let mut used_rows = 0usize;
        let mut walked = 0usize;
        for row in table.all("table-row") {
            let row_repeat = repeated(row, "number-rows-repeated");
            let seen = attr_of(row, "visibility");
            if seen == Some("collapse") || folded_by_style(attr_of(row, "style-name")) {
                sheet.hidden_rows += row_repeat;
            }
            // 高度同理：一跳在行样式上，而且常与 use-optimal-row-height 同时写
            let row_style = attr_of(row, "style-name");
            let sized = row_style.and_then(|want| row_sized.iter().find(|one| one.name == want));
            let had = SizeInfo {
                style: row_style.map(String::from),
                repeated: row_repeat,
                element_visibility: seen.map(String::from),
                size: sized.and_then(|one| one.size.clone()),
                optimal: sized.and_then(|one| one.optimal.clone()),
                style_visibility: sized.and_then(|one| one.visibility.clone()),
                style_parent: sized.and_then(|one| one.parent.clone()),
                resolved: sized.is_some(),
            };
            tally(&mut sheet.row_tally, &had);
            if sheet.row_sizes.len() < MAX_SIZE_ELEMENTS {
                sheet.row_sizes.push(had);
            }
            let mut col_at = 0usize;
            let mut hit = false;
            // 格子的局部名是 table-cell / covered-table-cell，不是一个叫 cell 的元素
            for cell in &row.children {
                if !(cell.is("table-cell") || cell.is("covered-table-cell")) {
                    continue;
                }
                walked += 1;
                if walked > MAX_CELL_ELEMENTS {
                    book.notes.push(format!(
                        "表 {} 的格子元素多到 {} 个，后面不走了",
                        sheet.name, walked
                    ));
                    break;
                }
                let span = repeated(cell, "number-columns-repeated");
                if cell.is("covered-table-cell") {
                    sheet.covered += 1;
                    col_at += span;
                    continue;
                }
                let reference = format!("{}{}", col_letter(col_at), row_at + 1);
                let mut paragraphs: Vec<String> = Vec::new();
                let (mut spans, mut specials) = (0usize, 0usize);
                plain_paragraphs(cell, &mut paragraphs, &mut spans, &mut specials);
                let text = paragraphs.join("\n");
                // 批注不算这一格的字，但它自己要交账：作者、时间（没写就 null）、正文
                for had in annotation_nodes(cell) {
                    let mut body: Vec<String> = Vec::new();
                    plain_paragraphs(had, &mut body, &mut 0usize, &mut 0usize);
                    sheet.comments.push(json!({
                        "ref": reference.clone(),
                        "author": note_field(had, &["creator"]),
                        "date": note_field(had, &["date-string", "date"]),
                        "text": body.join("\n"),
                    }));
                }
                let value = attr_of(cell, "value").map(|one| one.to_string());
                let date_value = attr_of(cell, "date-value").map(|one| one.to_string());
                let boolean_value = attr_of(cell, "boolean-value").map(|one| one.to_string());
                let formula = attr_of(cell, "formula").map(|one| one.to_string());
                let filled = !text.is_empty()
                    || value.is_some()
                    || date_value.is_some()
                    || boolean_value.is_some()
                    || formula.is_some();
                if filled {
                    let columns_spanned = repeated(cell, "number-columns-spanned");
                    let rows_spanned = repeated(cell, "number-rows-spanned");
                    if columns_spanned > 1 || rows_spanned > 1 {
                        sheet.merged += 1;
                    }
                    sheet.columns = sheet.columns.max(col_at + 1);
                    sheet.cells.push(Cell {
                        reference,
                        value_type: value_type(cell),
                        value,
                        date_value,
                        boolean_value,
                        formula,
                        text,
                        spans,
                        specials,
                        style_name: attr_of(cell, "style-name").map(|one| one.to_string()),
                        columns_spanned,
                        rows_spanned,
                    });
                    hit = true;
                }
                col_at += span;
            }
            if hit {
                used_rows += row_repeat;
            }
            row_at += row_repeat;
        }
        sheet.rows = used_rows;
        if sheet.col_tally.elements > sheet.col_sizes.len()
            || sheet.row_tally.elements > sheet.row_sizes.len()
        {
            book.notes.push(format!(
                "表 {} 走到的列/行元素有 {} / {} 条，逐条尺寸只存前 {} 条（总账按全部元素加）",
                sheet.name, sheet.col_tally.elements, sheet.row_tally.elements, MAX_SIZE_ELEMENTS
            ));
        }
        book.sheets.push(sheet);
    }
    book
}

impl Book {
    pub fn cell_total(&self) -> usize {
        self.sheets.iter().map(|one| one.cells.len()).sum()
    }
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

    fn find<'a>(book: &'a Book, want: &str) -> &'a Sheet {
        let names: Vec<&str> = book.sheets.iter().map(|one| one.name.as_str()).collect();
        book.sheets
            .iter()
            .find(|one| one.name == want)
            .unwrap_or_else(|| panic!("没有表 {}：这里只有 {:?}", want, names))
    }

    fn cell<'a>(book: &'a Book, sheet: &str, want: &str) -> &'a Cell {
        find(book, sheet)
            .cells
            .iter()
            .find(|one| one.reference == want)
            .unwrap_or_else(|| panic!("{sheet} 里没有格子 {want}"))
    }

    /// 批注坐在格子里面：那一格的字还是那一格的字，注另交一份账
    /// （期望值来自 `office_reader.py` 的 ods_facts）
    #[test]
    fn a_comment_lives_inside_its_cell_without_becoming_its_text() {
        let book = read(&bytes_of("cell-notes.ods"));
        assert_eq!(
            cell(&book, "预算表", "B2").text,
            "124000",
            "注的字没混进格子"
        );
        let first = find(&book, "预算表");
        let refs: Vec<&str> = first
            .comments
            .iter()
            .map(|one| one["ref"].as_str().unwrap_or_default())
            .collect();
        assert_eq!(refs, ["B2", "A3", "B3"], "{refs:?}");
        assert_eq!(
            first.comments[0]["author"],
            json!("张三"),
            "{:?}",
            first.comments[0]
        );
        assert_eq!(
            first.comments[0]["text"],
            json!("这里要补上不含税口径"),
            "{:?}",
            first.comments[0]
        );
        assert!(
            first.comments.iter().all(|one| one["date"].is_null()),
            "两个生产者都没往批注里写时间，那就交回 null"
        );
        // 反面对照：另一份没有批注的件不能凭空报出批注
        let clean = read(&bytes_of("book.ods"));
        assert!(
            clean.sheets.iter().all(|one| one.comments.is_empty()),
            "{:?}",
            clean
                .sheets
                .iter()
                .map(|one| one.comments.len())
                .collect::<Vec<usize>>()
        );
    }

    /// LibreOffice 写的三张表：名字、可见性、合并与覆盖、公式带缓存值
    #[test]
    fn a_produced_spreadsheet_lands_where_the_producer_said_so() {
        let book = read(&bytes_of("book.ods"));
        assert!(book.notes.is_empty(), "{:?}", book.notes);
        let named: Vec<&str> = book.sheets.iter().map(|one| one.name.as_str()).collect();
        assert_eq!(named, ["预算表", "说明", "草稿"], "{named:?}");
        let seen: Vec<bool> = book.sheets.iter().map(|one| one.visible).collect();
        assert_eq!(seen, [true, true, false], "第三张是隐藏表");
        assert_eq!(book.cell_total(), 11, "与 meta.xml 的 cell-count 一致");
        let first = find(&book, "预算表");
        assert_eq!((first.rows, first.columns, first.cells.len()), (5, 2, 9));
        assert_eq!(first.merged, 1);
        assert_eq!(first.covered, 1, "覆盖格单列一个数，不当内容");
        assert_eq!(first.formulas(), 1);
        assert_eq!(
            cell(&book, "预算表", "B4").formula.as_deref(),
            Some("of:=SUM([.B2:.B3])")
        );
        assert_eq!(cell(&book, "预算表", "B4").value.as_deref(), Some("142000"));
        assert_eq!(cell(&book, "预算表", "A5").columns_spanned, 2);
        assert_eq!(cell(&book, "预算表", "A5").text, "口径：含税");
        // 值是数、文本是给人看的：两者都要，但不混
        assert_eq!(cell(&book, "预算表", "B2").value.as_deref(), Some("124000"));
        assert_eq!(
            cell(&book, "预算表", "A1").value_type.as_deref(),
            Some("string")
        );
    }

    /// 那一行 `number-columns-repeated="16381"` 是本模块存在的理由：照字面数就是
    /// 一万六千格，而它连一个内容都没有；行首也会有（前两格空着），所以列号要累加
    #[test]
    fn a_run_of_empty_cells_is_one_element_not_sixteen_thousand() {
        let book = read(&bytes_of("formats.ods"));
        let first = find(&book, "格式");
        assert_eq!(first.columns, 3, "列数按**有内容**的最右一格数：{first:?}");
        assert_eq!(first.rows, 8);
        assert_eq!(first.cells.len(), 10);
        // 行首那两个空格把 C 列推到了正确的第三个位置
        assert_eq!(
            cell(&book, "格式", "C1").date_value.as_deref(),
            Some("2013-12-23")
        );
        assert_eq!(
            cell(&book, "格式", "C1").value_type.as_deref(),
            Some("date")
        );
        assert_eq!(
            cell(&book, "格式", "C2").date_value.as_deref(),
            Some("2013-12-23T15:15:00")
        );
        assert_eq!(
            cell(&book, "格式", "C3").value_type.as_deref(),
            Some("percentage")
        );
        assert_eq!(cell(&book, "格式", "C3").value.as_deref(), Some("0.125"));
        assert_eq!(cell(&book, "格式", "C3").text, "12.5%");
        // 货币在这里只是显示：值类型还是 float，¥ 在文本里
        assert_eq!(
            cell(&book, "格式", "C4").value_type.as_deref(),
            Some("float")
        );
        assert_eq!(cell(&book, "格式", "C4").text, "¥124,000.00");
        assert_eq!(
            cell(&book, "格式", "C5").text,
            "2013年12月23日",
            "中文格式显示出来的样子"
        );
        assert_eq!(
            cell(&book, "格式", "C7").value_type.as_deref(),
            Some("string")
        );
        assert_eq!(cell(&book, "格式", "C7").text, "12/23/2013");
        assert_eq!(
            cell(&book, "格式", "C8").boolean_value.as_deref(),
            Some("true")
        );
        assert_eq!(
            cell(&book, "格式", "C8").formula.as_deref(),
            Some("of:=TRUE()")
        );
        assert_eq!(first.date_cells(), 3, "两个日期一个日期时间");
        assert_eq!(find(&book, "另一张").date_cells(), 1);
        assert_eq!(
            cell(&book, "另一张", "A1").date_value.as_deref(),
            Some("2026-09-23")
        );
        assert_eq!(book.cell_total(), 11, "与 meta.xml 的 cell-count 一致");
    }

    /// 认的是内容不是后缀：`.odt` 里那张表照样是一等公民地读出来
    /// （LibreOffice 自己在 meta.xml 写的 table-count 也是 1）
    #[test]
    fn a_word_processors_table_is_still_a_table() {
        let book = read(&bytes_of("notes.odt"));
        assert_eq!(book.sheets.len(), 1, "{:?}", book.sheets);
        let one = find(&book, "表格1");
        assert_eq!((one.rows, one.columns, one.cells.len()), (2, 2, 4));
        assert_eq!(cell(&book, "表格1", "B2").text, "124000");
    }

    /// 列宽与行高**不在列/行元素上**：元素只写「我顶几个」，尺寸在它点名的那份自动样式里。
    /// 「几条元素」「盖住几列」「有内容的最右一列」是三本账（期望值来自 office_reader.py 的 ods_facts）
    #[test]
    fn a_column_width_is_one_hop_away_in_the_style_it_names() {
        let book = read(&bytes_of("book.ods"));
        let one = find(&book, "预算表");
        assert_eq!(
            (one.col_tally.elements, one.col_tally.spans, one.columns),
            (2, 16384, 2),
            "2 条列元素盖住 16384 列，而有内容的最右一格在第 2 列"
        );
        assert_eq!(
            (
                one.col_tally.resolved,
                one.col_tally.with_size,
                one.col_tally.optimal,
                one.col_tally.spoken_visibility
            ),
            (2, 2, 0, 0)
        );
        assert_eq!(one.col_sizes[0].style.as_deref(), Some("co1"));
        assert_eq!(
            (one.col_sizes[0].repeated, one.col_sizes[1].repeated),
            (2, 16382),
            "那一条 16382 就是补到一万六千列的那片空白"
        );
        assert_eq!(one.col_sizes[0].size.as_deref(), Some("1.672cm"));
        assert_eq!(one.col_sizes[0].style_parent, None, "这份件里样式没有父链");
        assert!(
            one.col_sizes[0].optimal.is_none(),
            "列这一族没有 use-optimal-column-width，不替它填"
        );
        // 行：高度与「按最优」是同时写的两句，而且两条来路都没说隐藏
        assert_eq!(
            (
                one.row_tally.elements,
                one.row_tally.spans,
                one.row_tally.optimal,
                one.row_tally.spoken_visibility
            ),
            (5, 5, 5, 0)
        );
        assert_eq!(one.row_sizes[0].size.as_deref(), Some("0.529cm"));
        assert_eq!(one.row_sizes[0].optimal.as_deref(), Some("true"));
        // 换算是另算的：文件自己写的那一串原样留着
        assert_eq!(crate::paper::length("1.672cm"), Some(1672));
        assert_eq!(crate::paper::length("0.529cm"), Some(529));
        // 表元素自己那四个数 LibreOffice 一个都不写：交回四个 None，而不是四个 0
        assert_eq!(one.stated.len(), 4, "{:?}", one.stated);
        assert!(
            one.stated.iter().all(|(_, raw)| raw.is_none()),
            "{:?}",
            one.stated
        );
    }

    /// 隐藏有两条来路：这个文件是**元素自己**说了 `collapse`（一条顶三列），
    /// 而 5 条行元素盖 20 行的是另一份件 —— 数几条与数盖住几格永远分开
    #[test]
    fn a_collapsed_column_speaks_for_itself_and_a_run_covers_twenty() {
        let book = read(&bytes_of("hidden.ods"));
        assert!(book.notes.is_empty(), "{:?}", book.notes);
        let one = find(&book, "预算表");
        assert_eq!(
            (
                one.col_tally.elements,
                one.col_tally.spans,
                one.col_tally.spoken_visibility,
                one.hidden_cols
            ),
            (3, 16384, 1, 3)
        );
        assert_eq!(one.col_sizes[1].style.as_deref(), Some("co2"));
        assert_eq!(one.col_sizes[1].size.as_deref(), Some("2.545cm"));
        assert_eq!(crate::paper::length("2.545cm"), Some(2545));
        assert_eq!(
            one.col_sizes[1].element_visibility.as_deref(),
            Some("collapse")
        );
        assert_eq!(one.col_sizes[1].repeated, 3, "一条元素藏了三列");
        assert!(
            one.col_sizes[1].style_visibility.is_none(),
            "样式那条来路这个文件没写，两条账各记各的"
        );
        assert_eq!((one.row_tally.spoken_visibility, one.hidden_rows), (2, 2));
        assert_eq!(
            one.row_tally.elements, one.row_tally.spans,
            "行这边没有合并"
        );

        let charts = read(&bytes_of("chart.ods"));
        assert!(charts.notes.is_empty(), "{:?}", charts.notes);
        let data = find(&charts, "数据");
        assert_eq!(
            (data.row_tally.elements, data.row_tally.spans, data.rows),
            (5, 20, 3),
            "5 条行元素盖 20 行（其中一条 repeated=16），而有内容的只有 3 行"
        );
        assert_eq!(data.row_sizes[3].repeated, 16);
        assert_eq!((data.col_tally.elements, data.col_tally.spans), (2, 16384));
    }
}
