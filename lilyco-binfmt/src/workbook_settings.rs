//! 这一本工作簿自己的设置：谁存的、算不算、打开停在哪一张。
//!
//! 问的是 `xl/workbook.xml` 顶上那一排元素 —— 它们不在任何一张表上，所以也不在任何一份「按表」
//! 的账里：`workbookPr`（`codeName`、`backupFile`、`showObjects`、`defaultThemeVersion`，以及那枚
//! 与日期那本账同名的 `date1904`）、`fileVersion`（`appName` / `lastEdited` / `lowestEdited` /
//! `rupBuild` —— 「最后一版是谁存的」就在这一格）、`bookViews` 里的 `workbookView`（`activeTab`
//! 就是「打开停在哪一张」，再加窗口尺寸与那几枚滚动条开关）、`calcPr`（`calcMode` 是自动还是手动、
//! `iterate` 那一套是迭代计算），以及只数不读的 `customWorkbookViews` / `pivotCaches` /
//! `externalReferences` / `smartTagTypes` 四个容器。
//!
//! 每一枚同时交「元素在不在场」与「它写了哪些属性、值原样是什么」，因为这两件事在真件里常常不是一
//! 回事，而且**两个生产者对同一份件的答案几乎不重叠**（`workbook-settings.xlsx` 与 LibreOffice
//! 重写它自己得到的那一份，实测）：
//! 1. openpyxl 写一枚**空的** `<workbookPr/>`（在场、一个属性都没有 —— 与「没写这枚元素」是两件
//!    事，所以 `empty_elements` 单记一本），LibreOffice 把它填成三格，而填的三格与手写那四格
//!    **只有一格同名**（`showObjects`）：`codeName` 与 `defaultThemeVersion` 没了，换成它自己补的
//!    `backupFile="false"` 与 `date1904="false"`；
//! 2. 布尔拼法在同一层里就不统一：openpyxl 这一层一律 `1` / `0`，LibreOffice 一律 `true` / `false`。
//!    手写那份故意两样各写一枚（`backupFile="1"` 与 `autoFilterDateGrouping="false"`），于是
//!    `boolean_spellings` 交的是一份件里同一格到底出现了几种拼法 —— 数出来，不合并；
//! 3. `fileVersion` 手写两枚（`xl15` 那一枚，加真 Office 常补的 `GenuineMicrosoftOffice` 那一枚），
//!    LibreOffice 重写时**合成一枚**：`appName` 换成它自己的 `"Calc"`，`lastEdited` 与 `rupBuild`
//!    都不留，只从第二枚留下 `lowestEdited="5"` —— 「谁存的」这一问在两份件里是两个答案，
//!    所以这一格按**列表**交（条数另记在 `counts`），不替文件挑一枚；
//! 4. `calcPr` 两家的属性集合**一枚都不重合**：openpyxl 写 `calcId` 与 `fullCalcOnLoad`，
//!    LibreOffice 反过来留 `iterate` / `iterateCount` / `iterateDelta` / `refMode` 四格而把前两格
//!    丢掉；更糟的是 `refMode` 这一格两边说的不是同一件事 —— MS-XLSX 里它是迭代引用的行 / 交叉
//!    模式（值域 `row` / `crossSheet`），LibreOffice 在同一格里写它自己的公式语言记号 `"A1"`。
//!    两格都按原样交，**不换算也不判**；
//! 5. `workbookView` 那九格被换成另一套九格：`xWindow` / `yWindow` / `windowWidth` / `windowHeight`
//!    名字留着而值改成它自己的（120 / 90 / 18000 / 9000 → 0 / 0 / 16384 / 8192），
//!    `visibility` / `minimized` / `autoFilterDateGrouping` 整枚不见，只有 `firstSheet` /
//!    `tabRatio` / `activeTab` 原样穿过 —— 「打开停在哪一张」是这一层里少数两家人都认的问题；
//! 6. `customWorkbookViews` 里那枚共享视图（`name` + `guid` 七格属性）LibreOffice **整层不留**：
//!    手写件里有这个容器（空着，`empty_elements` 记下它）与那一枚视图，重写件里连容器都没了，
//!    于是 `element_names` 少一项、`counts.customWorkbookView` 从 1 变 0。
//!
//! 只读 OOXML 的表格那一家。`.ods` 与遗留 `.xls` 的整份输出里查不到这个键：ODF 没有 `workbookPr`
//! / `fileVersion` / `calcPr` 这三枚元素，同类问题写在 `settings.xml` 的
//! `ooo:configuration-settings` 里（实测 15 份 `.ods` 全写 `AutoCalculate` 与 `SyntaxStringRef`，其中 14 份写 true，
//! 而**迭代计算那几格一份都不写** —— 那三格在转格式时丢掉；转换也会改：写了 manual 的那本转成 .ods 后 `AutoCalculate` 成了 false，而那一份另多出一整格 `CodeName`，那一组因此 40 条），那一本的名字与住处已在排版兼容
//! 那一条账里交代；`.xls` 把计算模式记在 BIFF 的 `DBSTAT` / `CALCCOUNT` 等记录里，本机没有第二个
//! 读者能核对那些字段偏移。缺键 = 这一族没这一层，不交一本零格的账冒充读过。

use crate::xmlscan::{self, Node};
use crate::zipread;
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// 在场与属性都要交的那几枚（`fileVersion` 不在内：它可以写好几枚，逐枚进列表；
/// `customWorkbookViews` 也不在内 —— 那一本走 `custom_views` 与 `counts`，不再进属性表）
const SINGLETONS: [&str; 6] = [
    "workbookPr",
    "calcPr",
    "webPublishing",
    "smartTagPr",
    "reviewPr",
    "smartTagTypes",
];
/// 只数条数、正文不读的容器
const COUNTED: [(&str, &str); 2] = [
    ("pivotCaches", "pivotCache"),
    ("externalReferences", "externalReference"),
];
const BOOLEAN_WORDS: [&str; 4] = ["0", "1", "true", "false"];
const PART: &str = "xl/workbook.xml";

fn kids(node: Option<&Node>) -> Vec<&Node> {
    match node {
        Some(one) => one
            .children
            .iter()
            .filter(|kid| kid.name != "#text")
            .collect(),
        None => Vec::new(),
    }
}

/// 一枚元素的属性表（局部名；`xmlns` 那类声明不算属性 —— 标准库的 XML 读者也不放进 attrib）
fn attrs_of(node: &Node) -> BTreeMap<String, String> {
    let mut out: BTreeMap<String, String> = BTreeMap::new();
    for (key, value) in node.attrs.iter() {
        if key == "xmlns" || key.starts_with("xmlns:") {
            continue;
        }
        let local = key.rsplit(':').next().unwrap_or(key).to_string();
        out.insert(local, (*value).to_string());
    }
    out
}

fn table(map: &BTreeMap<String, String>) -> Value {
    let mut out = serde_json::Map::new();
    for (key, value) in map.iter() {
        out.insert(key.clone(), json!(value));
    }
    Value::Object(out)
}

fn names_of(map: &BTreeMap<String, String>) -> Vec<Value> {
    map.keys().map(|one| json!(one)).collect()
}

/// 一棵子树里局部名为 `want` 的元素有多少枚（与读者的 `iter()` 同一口径：不分层、不算自己）
fn count_named(node: &Node, want: &str) -> usize {
    let mut hit = 0usize;
    for one in kids(Some(node)) {
        if one.local() == want {
            hit += 1;
        }
        hit += count_named(one, want);
    }
    hit
}

fn book_root(bytes: &[u8]) -> Option<Node> {
    let member = zipread::member(bytes, PART, zipread::DEFAULT_MEMBER_CAP).ok()?;
    let text = member.as_text();
    if text.trim().is_empty() {
        return None;
    }
    Some(xmlscan::parse_str(&text))
}

/// xlsx：`xl/workbook.xml` 顶上那一排元素，属性值一律按文件写的字面交出去
pub(crate) fn xlsx(bytes: &[u8]) -> Value {
    let doc = match book_root(bytes) {
        Some(one) => one,
        None => return json!({"family": "ooxml", "available": false}),
    };
    // `parse_str` 交的是伪根 `#doc`，真正的 `<workbook>` 在它下面一层
    let book = match kids(Some(&doc))
        .into_iter()
        .find(|one| one.local() == "workbook")
    {
        Some(one) => one,
        None => return json!({"family": "ooxml", "available": false}),
    };
    let mut singles: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    let mut views: Vec<Value> = Vec::new();
    let mut custom: Vec<Value> = Vec::new();
    let mut versions: Vec<Value> = Vec::new();
    for one in kids(Some(book)) {
        let name = one.local();
        if name == "fileVersion" {
            versions.push(table(&attrs_of(one)));
            continue;
        }
        if name == "bookViews" || name == "customWorkbookViews" {
            // 两本视图各有各的容器：`workbookView` 住在 `bookViews`，`customWorkbookView`
            // 住在 `customWorkbookViews` —— 分开收，容器自己也照记一枚属性表
            singles.insert(name.to_string(), attrs_of(one));
            for kid in kids(Some(one)) {
                let row = json!({"element": kid.local(), "attrs": table(&attrs_of(kid))});
                if name == "bookViews" && kid.local() == "workbookView" {
                    views.push(row);
                } else if name == "customWorkbookViews" && kid.local() == "customWorkbookView" {
                    custom.push(row);
                }
            }
            continue;
        }
        if SINGLETONS.contains(&name) {
            // 同名写了两枚时后一枚留下（schema 只许一枚，本仓没有一件真写两枚）
            singles.insert(name.to_string(), attrs_of(one));
        }
        for (host, _kid) in COUNTED.iter() {
            if name == *host {
                singles.insert(name.to_string(), attrs_of(one));
            }
        }
    }
    let mut elements = serde_json::Map::new();
    let mut written = serde_json::Map::new();
    let mut listed: Vec<Value> = Vec::new();
    let mut empty: Vec<Value> = Vec::new();
    for (key, mine) in singles.iter() {
        elements.insert(key.clone(), table(mine));
        written.insert(key.clone(), Value::Array(names_of(mine)));
        listed.push(json!(key));
        if mine.is_empty() && key != "bookViews" {
            empty.push(json!(key));
        }
    }
    // 拼法只数这一层真交了属性表的那些：`workbookView` 的行 + 上面每一枚的属性
    let mut spelled: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    {
        let mut bump = |key: &str, value: &str| {
            let mine = spelled.entry(key.to_string()).or_default();
            *mine.entry(value.to_string()).or_insert(0) += 1;
        };
        for one in views.iter() {
            if let Some(map) = one.get("attrs").and_then(|had| had.as_object()) {
                for (key, value) in map.iter() {
                    if let Some(text) = value.as_str() {
                        if BOOLEAN_WORDS.contains(&text) {
                            bump(key, text);
                        }
                    }
                }
            }
        }
        for mine in singles.values() {
            for (key, value) in mine.iter() {
                if BOOLEAN_WORDS.contains(&value.as_str()) {
                    bump(key, value);
                }
            }
        }
    }
    let mut spellings = serde_json::Map::new();
    for (key, mine) in spelled.iter() {
        let mut inner = serde_json::Map::new();
        for (value, count) in mine.iter() {
            inner.insert(value.clone(), json!(count));
        }
        spellings.insert(key.clone(), Value::Object(inner));
    }
    let mut counts = serde_json::Map::new();
    // `bookViews` 的直接孩子数出来的两本（与读者同一口径：不看嵌套，也不看别处的同名元素）
    counts.insert("customWorkbookView".to_string(), json!(custom.len()));
    counts.insert(
        "definedName".to_string(),
        json!(count_named(book, "definedName")),
    );
    for (host, kid_name) in COUNTED.iter() {
        let hit = if singles.contains_key(*host) {
            count_named(book, kid_name)
        } else {
            0
        };
        counts.insert((*kid_name).to_string(), json!(hit));
    }
    counts.insert("fileVersion".to_string(), json!(versions.len()));
    counts.insert("workbookView".to_string(), json!(views.len()));
    json!({
        "family": "ooxml",
        "available": true,
        "part": PART,
        "elements": Value::Object(elements),
        "element_names": Value::Array(listed),
        "attrs_written": Value::Object(written),
        "empty_elements": Value::Array(empty),
        "views": views,
        "custom_views": custom,
        "file_versions": versions,
        "counts": Value::Object(counts),
        "boolean_spellings": Value::Object(spellings),
    })
}
