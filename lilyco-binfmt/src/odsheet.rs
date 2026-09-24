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

/// 一个有内容的格子
#[derive(Debug, Clone)]
pub struct Cell {
    pub reference: String,
    pub value_type: String,
    pub value: Option<String>,
    pub date_value: Option<String>,
    pub boolean_value: Option<String>,
    pub formula: Option<String>,
    pub text: String,
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
            "columns_spanned": self.columns_spanned,
            "rows_spanned": self.rows_spanned,
        })
    }
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
    /// 这张表里的批注：`{ref, author, date, text}`。ODF 的批注**坐在格子里面**
    /// （`office:annotation` 是 `table:table-cell` 的孩子），所以取格子的字时要跳过它 ——
    /// 与 .odt 那边「批注与修订表里的段不算正文」是同一条规矩
    pub comments: Vec<Value>,
}

/// 格子里那些「算这一格内容」的段：批注子树整个跳过
fn plain_paragraphs(node: &xmlscan::Node, out: &mut Vec<String>) {
    for one in node.children.iter() {
        if one.is("annotation") {
            continue;
        }
        if one.is("p") {
            out.push(one.text().trim().to_string());
            continue;
        }
        plain_paragraphs(one, out);
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
            .filter(|one| one.value_type == want)
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
/// 命名空间，抄了一份出来）。`attr_of` 已经躲开那个副本，这里再兜一层：
/// 没写类型的格子按其有没有字判 string / empty。
fn value_type(cell: &Node) -> String {
    match attr_of(cell, "value-type") {
        Some(one) => one.to_string(),
        None => {
            if cell.text().trim().is_empty() {
                "empty".to_string()
            } else {
                "string".to_string()
            }
        }
    }
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
        folded.push((name, hidden));
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
            let hidden = attr_of(column, "visibility") == Some("collapse")
                || folded_by_style(attr_of(column, "style-name"));
            if hidden {
                sheet.hidden_cols += repeated(column, "number-columns-repeated");
            }
        }
        let mut row_at = 0usize;
        let mut used_rows = 0usize;
        let mut walked = 0usize;
        for row in table.all("table-row") {
            let row_repeat = repeated(row, "number-rows-repeated");
            if attr_of(row, "visibility") == Some("collapse")
                || folded_by_style(attr_of(row, "style-name"))
            {
                sheet.hidden_rows += row_repeat;
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
                plain_paragraphs(cell, &mut paragraphs);
                let text = paragraphs.join("\n");
                // 批注不算这一格的字，但它自己要交账：作者、时间（没写就 null）、正文
                for had in annotation_nodes(cell) {
                    let mut body: Vec<String> = Vec::new();
                    plain_paragraphs(had, &mut body);
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
        assert_eq!(cell(&book, "预算表", "A1").value_type, "string");
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
        assert_eq!(cell(&book, "格式", "C1").value_type, "date");
        assert_eq!(
            cell(&book, "格式", "C2").date_value.as_deref(),
            Some("2013-12-23T15:15:00")
        );
        assert_eq!(cell(&book, "格式", "C3").value_type, "percentage");
        assert_eq!(cell(&book, "格式", "C3").value.as_deref(), Some("0.125"));
        assert_eq!(cell(&book, "格式", "C3").text, "12.5%");
        // 货币在这里只是显示：值类型还是 float，¥ 在文本里
        assert_eq!(cell(&book, "格式", "C4").value_type, "float");
        assert_eq!(cell(&book, "格式", "C4").text, "¥124,000.00");
        assert_eq!(
            cell(&book, "格式", "C5").text,
            "2013年12月23日",
            "中文格式显示出来的样子"
        );
        assert_eq!(cell(&book, "格式", "C7").value_type, "string");
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
}
