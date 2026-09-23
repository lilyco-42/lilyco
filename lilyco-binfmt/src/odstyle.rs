//! ODF 表格「这一格按什么格式显示」那一跳（T0 只读）。
//!
//! ODF 没有 Excel 那种格式串（`yyyy-mm-dd`）：格式是一棵元素树。链路是
//! `table:table-cell/@table:style-name` → 那个单元格样式的 `@style:data-style-name`
//! → 一个 `number:date-style` / `number:currency-style` / … 元素。三件事要注意：
//!
//! 1. 数据样式可能在 `content.xml` 的 automatic-styles 里，也可能在 `styles.xml` 的
//!    命名样式里 —— LibreOffice 两份都用（`formats.ods` 的 `ce5` 指向 content 里的
//!    `N41`，而 `ce2` 指向 styles.xml 里的 `N150`），只读一份就会漏。
//! 2. 单元格样式可以只带 `@style:parent-style-name`，格式挂在父样式上，所以要顺着
//!    链往上找。
//! 3. 这里**不重构格式串**：把元素树逐条抄成 token 清单（`year`、`text:-`、`month`…），
//!    加上样式自带的 `number:decimal-places` 与 `number:currency-symbol`。
//!    把 `number:style="long"` 翻译成「四位年」是解释，不是文件里写着的字。

use serde_json::{json, Value};

use crate::xmlscan;
use crate::zipread;

const CONTENT_PART: &str = "content.xml";
const STYLES_PART: &str = "styles.xml";
const MEMBER_CAP: u64 = 8 << 20;
/// 父样式链最多走几层：真文件的链只有一两层，坏文件可以把自己绕成环
const MAX_PARENT_HOPS: usize = 8;

/// 一个 `number:*-style` 元素
struct DataStyle {
    name: String,
    kind: &'static str,
    decimals: Option<usize>,
    currency_symbol: Option<String>,
    tokens: Vec<String>,
}

/// 一份表格的两类样式账：单元格样式（含它的父链与指的数据样式）与数据样式本身
pub struct Styles {
    cells: Vec<(String, CellStyle)>,
    data: Vec<DataStyle>,
    pub notes: Vec<String>,
}

struct CellStyle {
    data_style: Option<String>,
    parent: Option<String>,
}

/// `number:date-style` 之类的局部名直接给类别，不猜
fn kind_of(local: &str) -> Option<&'static str> {
    Some(match local {
        "date-style" => "date",
        "time-style" => "time",
        "number-style" => "number",
        "currency-style" => "currency",
        "percentage-style" => "percent",
        "text-style" => "text",
        "boolean-style" => "bool",
        _ => return None,
    })
}

fn attr_local(node: &xmlscan::Node, want: &str) -> Option<String> {
    node.attrs
        .iter()
        .filter(|(key, _)| key.rsplit(':').next().unwrap_or_default() == want)
        .find(|(key, _)| !key.starts_with("calcext:"))
        .map(|(_, value)| value.clone())
}

/// 元素树逐条抄下来：`<number:text>-</number:text>` 记成 `text:-`，其余记局部名
fn tokens_of(style: &xmlscan::Node) -> Vec<String> {
    style
        .children
        .iter()
        .map(|one| {
            if one.local() == "text" {
                format!("text:{}", one.direct)
            } else {
                one.local().to_string()
            }
        })
        .collect()
}

fn decimals_of(style: &xmlscan::Node) -> Option<usize> {
    style.children.iter().find_map(|one| {
        attr_local(one, "decimal-places").and_then(|raw| raw.trim().parse::<usize>().ok())
    })
}

/// 读一个部件里的两类样式。content.xml 后写进来，所以同名时它赢。
fn gather(
    from: &str,
    bytes: &[u8],
    cells: &mut Vec<(String, CellStyle)>,
    data: &mut Vec<DataStyle>,
) {
    let Ok(member) = zipread::member(bytes, from, MEMBER_CAP) else {
        return;
    };
    let root = xmlscan::parse_str(&member.as_text());
    for node in root.descendants("style") {
        let Some(name) = attr_local(&node, "name") else {
            continue;
        };
        if attr_local(&node, "family").as_deref() == Some("table-cell") {
            cells.push((
                name,
                CellStyle {
                    data_style: attr_local(&node, "data-style-name"),
                    parent: attr_local(&node, "parent-style-name"),
                },
            ));
        }
    }
    for local in [
        "date-style",
        "time-style",
        "number-style",
        "currency-style",
        "percentage-style",
        "text-style",
        "boolean-style",
    ] {
        for node in root.descendants(local) {
            let Some(name) = attr_local(&node, "name") else {
                continue;
            };
            let Some(kind) = kind_of(local) else {
                continue;
            };
            data.push(DataStyle {
                name,
                kind,
                decimals: decimals_of(&node),
                currency_symbol: attr_local(&node, "currency-symbol"),
                tokens: tokens_of(&node),
            });
        }
    }
}

impl Styles {
    pub fn read(bytes: &[u8]) -> Styles {
        let mut cells: Vec<(String, CellStyle)> = Vec::new();
        let mut data: Vec<DataStyle> = Vec::new();
        // 顺序有讲究：先命名样式后自动样式，同名时后写的（content.xml 里那份）赢
        gather(STYLES_PART, bytes, &mut cells, &mut data);
        gather(CONTENT_PART, bytes, &mut cells, &mut data);
        let mut notes = Vec::new();
        if data.is_empty() {
            notes.push(
                "两份样式部件里都没找到 number:*-style：这一版对 ODF 的格式只能说「不知道」"
                    .to_string(),
            );
        }
        Styles { cells, data, notes }
    }

    fn cell_style(&self, name: &str) -> Option<&CellStyle> {
        self.cells
            .iter()
            .find(|(one, _)| one == name)
            .map(|(_, two)| two)
    }

    /// 两份样式账各有多少条（命令里报出来，读的人才知道「没找到」是文件没有还是我没读）
    pub fn counts(&self) -> (usize, usize) {
        (self.cells.len(), self.data.len())
    }

    /// 顺着父样式链找那个数据样式（父链上没写就直接用本层的）
    fn data_of(&self, cell_style: &str) -> Option<(&DataStyle, String)> {
        let mut here = cell_style.to_string();
        for _ in 0..MAX_PARENT_HOPS {
            let found = self.cell_style(&here)?;
            if let Some(want) = &found.data_style {
                return self
                    .data
                    .iter()
                    .find(|one| &one.name == want)
                    .map(|one| (one, want.clone()));
            }
            match &found.parent {
                Some(one) => here = one.clone(),
                None => return None,
            }
        }
        None
    }

    /// 一格的那份格式账；格子没写样式就整个交回 null（不给一份空对象装作有）。
    /// 写了样式但那一跳落空的，五个键也在，只是都空着 —— 「没有」也要说清楚
    pub fn for_cell(&self, cell_style: Option<&str>) -> Value {
        let Some(name) = cell_style else {
            return Value::Null;
        };
        match self.data_of(name) {
            Some((one, want)) => json!({
                "cell_style": name,
                "data_style": want,
                "format_kind": one.kind,
                "decimals": one.decimals,
                "currency_symbol": one.currency_symbol,
                "format_tokens": one.tokens,
            }),
            None => json!({
                "cell_style": name,
                "data_style": Value::Null,
                "format_kind": Value::Null,
                "decimals": Value::Null,
                "currency_symbol": Value::Null,
                "format_tokens": Vec::<String>::new(),
            }),
        }
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

    /// 数据样式分布在两个部件里：`ce5` 在 content，`ce2` 指向 styles.xml 的 `N150`
    #[test]
    fn data_styles_come_from_both_parts() {
        let styles = Styles::read(&bytes_of("formats.ods"));
        let by_date = styles.for_cell(Some("ce5"));
        assert_eq!(by_date["data_style"], "N41", "{by_date}");
        assert_eq!(by_date["format_kind"], "date");
        assert_eq!(
            by_date["format_tokens"],
            json!(["year", "text:年", "month", "text:月", "day", "text:日"]),
            "汉字字面量要照抄：{by_date}"
        );
        let far = styles.for_cell(Some("ce2"));
        assert_eq!(far["data_style"], "N150", "这一条在 styles.xml 里：{far}");
        assert_eq!(far["format_kind"], "date");
    }

    /// 百分数与「带货币符号的数」：类别只看元素自己的名字。
    /// `ce4` 那份是个反例 —— 屏上写着 `¥124000.00`，可 LibreOffice 把它写成
    /// `number:number-style` 加一个字面量 `¥`，**不是** `number:currency-style`：
    /// 照「有货币符号就是 currency」报就等于替文件编一个类别
    #[test]
    fn percent_and_a_literal_currency_sign_are_not_the_same_kind() {
        let styles = Styles::read(&bytes_of("formats.ods"));
        let percent = styles.for_cell(Some("ce3"));
        assert_eq!(percent["format_kind"], "percent", "{percent}");
        assert_eq!(percent["decimals"], 1, "小数位在 number:decimal-places 上");
        assert_eq!(percent["format_tokens"], json!(["number", "text:%"]));
        let money = styles.for_cell(Some("ce4"));
        assert_eq!(money["format_kind"], "number", "{money}");
        assert_eq!(money["data_style"], "N152");
        assert_eq!(money["decimals"], 2);
        assert_eq!(
            money["currency_symbol"],
            Value::Null,
            "货币符号是字面量，不是 number:currency-symbol 属性"
        );
        assert_eq!(money["format_tokens"], json!(["text:¥", "number"]));
    }

    /// 日期样式里也能装时间部件：ODF 的「日期时间」就是一个含着 hours/minutes/seconds
    /// 的 `number:date-style`，没有单独的元素名可看
    #[test]
    fn a_date_style_can_carry_time_parts() {
        let styles = Styles::read(&bytes_of("formats.ods"));
        let both = styles.for_cell(Some("ce2"));
        assert_eq!(both["format_kind"], "date", "{both}");
        let tokens = both["format_tokens"].as_array().expect("是数组");
        let names: Vec<&str> = tokens
            .iter()
            .map(|one| one.as_str().unwrap_or(""))
            .collect();
        assert!(
            names.contains(&"hours") && names.contains(&"seconds"),
            "{names:?}"
        );
    }

    /// 没写样式的格子不交一份空对象；写了样式但没有数据样式的，说清楚「这一跳没有」
    #[test]
    fn a_cell_without_a_style_gives_nothing() {
        let styles = Styles::read(&bytes_of("book.ods"));
        assert!(styles.for_cell(None).is_null());
        let plain = styles.for_cell(Some("ce1"));
        assert_eq!(
            plain["data_style"],
            Value::Null,
            "book.ods 的 ce1 不指数据样式"
        );
        assert_eq!(plain["cell_style"], "ce1");
    }
}
