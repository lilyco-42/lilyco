//! 「这份文件是按哪个版本的排版规则来排的」——OOXML 把这一格写在 `word/settings.xml` 的
//! `<w:compat>` 里，而**同一类话有两种写法**：具名项
//! `<w:compatSetting w:name="compatibilityMode" w:uri="..." w:val="15"/>`（值在属性上），
//! 与裸开关 `<w:useFELayout/>`（**在场即为开**，身上什么都没有）。
//! ODF 没有 `<w:compat>` 这一格：LibreOffice 把兼容开关**摊平**在 `settings.xml` 的
//! `<config:config-item-set config:name="ooo:configuration-settings">` 里，一条一个具名项，
//! 类型写在 `config:type` 上、值写在正文里 —— 所以这一本交的是另一套词汇，不折算。
//!
//! 实测七条（`## 这些数字从哪来` 那一本，全语料；第 1、3 条按 72 份数，其余按 71 份 .docx 与 41 份 .odt）：
//! 1. `<w:compat>` **只在 `word/settings.xml`**：71 份 .docx 全有，那一份 .docm 也有，共 72 份，
//!    而整批 OOXML（132 份包的 2518 份 xml 部件，含 `.rels`）里再没有第二处，且没有一份是空的
//!    （孩子数 2 到 5）；
//! 2. 裸开关只出现过四个名字（`useFELayout` 33、`doNotUseHTMLParagraphAutoSpacing` 6、
//!    `doNotBreakWrappedTables` 4、`adjustLineHeightInTable` 2），**一枚 `w:val` 都没写** ——
//!    「在场即开」是本族料唯一走得通的读法，`w:val="0"` 那一条读法只有合成件能测；
//! 3. 具名项只出现过六个名字，`w:uri` 全是 `http://schemas.microsoft.com/office/word`；
//!    两种写法的名字**互不重叠**（`names_in_both_encodings` 72 份全空）；
//! 4. `compatibilityMode` 三份件以上各说各话：14（60 份）、15（6 份）、12（5 份）；
//! 5. 按生产者分（`docProps/app.xml` 的 Application）：python-docx 那份模板
//!    （写着 Microsoft Macintosh Word）33 份**都只带 `useFELayout`**；LibreOffice 写的 38 份里
//!    32 份一个裸开关都不写、6 份写另外三个名字 —— 同一个 `<w:compat>` 两套笔迹。
//!    两种写法搭配出**六种形状**（按「几条具名项 + 几枚裸开关」数：`4+1` 33、`4+0` 28、`1+2` 4、
//!    `3+0` 3、`3+2` 2、`2+0` 1），所以「具名项至少四条」是习惯不是规矩，两个数各交各的；
//! 6. ODF 那一本不是常量也不是套话：`ooo:configuration-settings` 在 39/41 份 odt 里（另 2 份
//!    整个没有 `settings.xml`），条数 121 到 123；其中名字里点了 Word 的四条
//!    （`MsWordCompTrailingBlanks` / `MsWordCompMinLineHeightByFly` / `MsWordCompGridMetrics` /
//!    `MsWordUlTrailSpace`）**39 份全写**，可 `MsWordUlTrailSpace` 39 份全 false，
//!    另外三条多数 true，而 `tbox-lo.odt` 三条全 false、`images-float.odt` 只错开一条；
//!    计数那两格也不是套话：39 份的 `booleans_total` 只有 106 / 107 / 108 三种值，
//!    `booleans_true` 从 32 到 63。同一组在另外两族也在（14 份 .ods 恒 39 条、11 份 .odp
//!    写 42 或 43 条），**但那四类名字一条都没有** —— 所以这一本只在 office-doc 交；
//! 7. 与 OOXML 裸开关**同名**的只有一条：`DoNotBreakWrappedTables`（首字母大小写正好差一位），
//!    39 份 odt 里只有 2 份写它，而带那枚开关的 .docx 有 4 份 —— 一问两转，两头各丢。
//!
//! 另外两族不交这个键（缺键 = 这一族没这一层）：RTF 的流里没有这一格；`.doc` 的兼容位在 FIB 的
//! 位段里，而改那些位要 Word 本尊（本族料的 .doc 全出自 LibreOffice），判不住就不报。

use crate::xmlscan::{self, Node};
use crate::zipread::{self, DEFAULT_MEMBER_CAP};
use serde_json::{json, Value};

/// 本族料实测出现过的四个 OOXML 裸开关名。这张表**不是**语义映射，只用来做同名核对：
/// 它回答的是「这一条 ODF 项在 OOXML 那边有没有长一样的名字」，不回答「两者是否同一回事」。
const OOXML_SWITCHES: [&str; 4] = [
    "adjustLineHeightInTable",
    "doNotBreakWrappedTables",
    "doNotUseHTMLParagraphAutoSpacing",
    "useFELayout",
];

/// 同名核对：实测两家正好差在首字母大小写（`doNotBreak...` 对 `DoNotBreak...`），所以按不区分大小写比
fn same_name_as_switch(name: &str) -> bool {
    OOXML_SWITCHES
        .iter()
        .any(|one| one.eq_ignore_ascii_case(name))
}

/// 只数元素孩子。容错解析会把「两个标签之间的一个空格」摊成一枚 `#text`，
/// 而 ElementTree 遍历孩子只给元素 —— 两家对同一份件得数出同一个孩子数。
fn element_kids(node: &Node) -> Vec<&Node> {
    node.children
        .iter()
        .filter(|one| one.name != "#text")
        .collect()
}

/// 「这一份件里到底有没有一个元素」：一个尖括号都没有的部件在 Python 那本是 `ParseError`
/// （解不开），在容错解析下只剩一枚 `#text` —— 两家按同一个判决：没有元素就算部件不在。
fn has_element(parsed: &Option<Node>) -> bool {
    match parsed.as_ref() {
        Some(doc) => doc.children.iter().any(|one| one.name != "#text"),
        None => false,
    }
}

fn member_doc(bytes: &[u8], part: &str) -> Option<Node> {
    zipread::member(bytes, part, DEFAULT_MEMBER_CAP)
        .ok()
        .map(|one| xmlscan::parse_str(&one.as_text()))
}

fn push_unique(rows: &mut Vec<String>, name: &str) {
    if !rows.iter().any(|had| had == name) {
        rows.push(name.to_string());
    }
}

/// OOXML 那一份：两种写法各摊一本，谁也不顶替谁
pub(crate) fn docx(bytes: &[u8], limit: usize) -> Value {
    let parsed = member_doc(bytes, "word/settings.xml");
    let settings_part = has_element(&parsed);
    let holders: Vec<&Node> = match parsed.as_ref() {
        Some(doc) => doc.descendants("compat"),
        None => Vec::new(),
    };
    let kids: Vec<&Node> = match holders.first() {
        Some(one) => element_kids(one),
        None => Vec::new(),
    };
    let mut named: Vec<Value> = Vec::new();
    let mut switches: Vec<Value> = Vec::new();
    let mut named_names: Vec<String> = Vec::new();
    let mut switch_names: Vec<String> = Vec::new();
    let mut uris: Vec<String> = Vec::new();
    let mut mode: Value = Value::Null;
    let mut mode_total = 0usize;
    for one in kids.iter() {
        let local = one.local().to_string();
        let name = one.attr_local("name").map(String::from);
        let uri = one.attr_local("uri").map(String::from);
        let val = one.attr_local("val").map(String::from);
        if local == "compatSetting" {
            if let Some(had) = &name {
                push_unique(&mut named_names, had);
            }
            if let Some(had) = &uri {
                push_unique(&mut uris, had);
            }
            if name.as_deref() == Some("compatibilityMode") {
                if mode_total == 0 {
                    mode = match &val {
                        Some(had) => json!(had),
                        None => Value::Null,
                    };
                }
                mode_total += 1;
            }
            named.push(json!({"name": name, "uri": uri, "val": val}));
            continue;
        }
        // 裸开关：没写 `w:val` 就是在场即开；写了 `0` / `false` 才是明确说不要
        let val_written = val.is_some();
        let on = !matches!(val.as_deref(), Some("0") | Some("false"));
        push_unique(&mut switch_names, &local);
        switches.push(json!({
            "name": local,
            "val_written": val_written,
            "val": val,
            "on": on,
        }));
    }
    uris.sort();
    uris.dedup();
    let mut shared: Vec<String> = named_names
        .iter()
        .filter(|one| switch_names.iter().any(|had| had == *one))
        .cloned()
        .collect();
    shared.sort();
    shared.dedup();
    json!({
        "family": "ooxml",
        "available": true,
        "settings_part": settings_part,
        "compat_written": !holders.is_empty(),
        "compat_total": holders.len(),
        "children_total": kids.len(),
        "mode": mode,
        "mode_total": mode_total,
        "named": named.into_iter().take(limit).collect::<Vec<Value>>(),
        "switches": switches.into_iter().take(limit).collect::<Vec<Value>>(),
        "named_names": named_names,
        "switch_names": switch_names,
        "names_in_both_encodings": shared,
        "uris": uris,
    })
}

/// ODF 那一本：`ooo:configuration-settings` 里的具名项，只挑名字能说清来路的两种
pub(crate) fn odf(bytes: &[u8], limit: usize) -> Value {
    let parsed = member_doc(bytes, "settings.xml");
    let settings_part = has_element(&parsed);
    let sets: Vec<&Node> = match parsed.as_ref() {
        Some(doc) => doc
            .descendants("config-item-set")
            .into_iter()
            .filter(|one| one.attr_local("name") == Some("ooo:configuration-settings"))
            .collect(),
        None => Vec::new(),
    };
    let kids: Vec<&Node> = match sets.first() {
        Some(one) => element_kids(one),
        None => Vec::new(),
    };
    // (名字, 来路, 一行)：先攒后按名字排，与 Python 那本同一个口径
    let mut rows: Vec<(String, &'static str, Value)> = Vec::new();
    let mut kinds: Vec<(Option<String>, usize)> = Vec::new();
    let mut booleans_total = 0usize;
    let mut booleans_true = 0usize;
    for one in kids.iter() {
        let name = one.attr_local("name").map(String::from);
        let kind = one.attr_local("type").map(String::from);
        let value = one.text();
        let value = value.trim().to_string();
        // 计数那一格：同一个类型只有一条，缺 `config:type` 的自成一类（名字交 null）
        let found = kinds.iter().position(|(had, _)| *had == kind);
        match found {
            Some(at) => kinds[at].1 += 1,
            None => kinds.push((kind.clone(), 1)),
        }
        if kind.as_deref() == Some("boolean") {
            booleans_total += 1;
            if value == "true" {
                booleans_true += 1;
            }
        }
        let via = match &name {
            Some(had) if had.starts_with("MsWord") => Some("msword-prefix"),
            Some(had) if same_name_as_switch(had) => Some("same-name"),
            _ => None,
        };
        let via = match via {
            Some(had) => had,
            None => continue,
        };
        rows.push((
            name.clone().unwrap_or_default(),
            via,
            json!({"name": name, "type": kind, "value": value, "via": via}),
        ));
    }
    rows.sort_by_key(|row| row.0.clone());
    let same_name_rows: Vec<String> = rows
        .iter()
        .filter(|(_, via, _)| *via == "same-name")
        .map(|(name, _, _)| name.clone())
        .collect();
    kinds.sort_by_key(|(kind, _)| type_key(kind));
    json!({
        "family": "odf",
        "available": true,
        "settings_part": settings_part,
        "item_set_written": !sets.is_empty(),
        "items_total": kids.len(),
        "booleans_total": booleans_total,
        "booleans_true": booleans_true,
        "types": kinds
            .iter()
            .map(|(kind, count)| json!({"type": kind.clone(), "count": *count}))
            .collect::<Vec<Value>>(),
        "compat_items": rows
            .iter()
            .take(limit)
            .map(|(_, _, one)| one.clone())
            .collect::<Vec<Value>>(),
        "compat_item_total": rows.len(),
        "same_name_rows": same_name_rows,
    })
}

/// 类型名的排序键：缺 `config:type` 的那一类排在最前（Python 那本用同一个串比）
fn type_key(kind: &Option<String>) -> String {
    match kind {
        Some(had) => had.clone(),
        None => "-".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 自己打一个「存储」（不压缩）的包：这一本要的形状（裸开关明写着 `0`、两种写法撞同名、
    /// 一格没有类型）在 215 份真件里一个都没有，只能自己造
    fn packed(parts: &[(&str, &str)]) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();
        let mut starts: Vec<u32> = Vec::new();
        for (name, body) in parts {
            let raw = body.as_bytes();
            let crc = (zipread::crc32(raw) & 0xFFFF_FFFF) as u32;
            starts.push(out.len() as u32);
            out.extend_from_slice(&[b'P', b'K', 3, 4]);
            out.extend_from_slice(&20u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u32.to_le_bytes());
            out.extend_from_slice(&crc.to_le_bytes());
            out.extend_from_slice(&(raw.len() as u32).to_le_bytes());
            out.extend_from_slice(&(raw.len() as u32).to_le_bytes());
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(raw);
        }
        let cd = out.len() as u32;
        for (index, (name, body)) in parts.iter().enumerate() {
            let raw = body.as_bytes();
            let crc = (zipread::crc32(raw) & 0xFFFF_FFFF) as u32;
            out.extend_from_slice(&[b'P', b'K', 1, 2]);
            out.extend_from_slice(&20u16.to_le_bytes());
            out.extend_from_slice(&20u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u32.to_le_bytes());
            out.extend_from_slice(&crc.to_le_bytes());
            out.extend_from_slice(&(raw.len() as u32).to_le_bytes());
            out.extend_from_slice(&(raw.len() as u32).to_le_bytes());
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u32.to_le_bytes());
            out.extend_from_slice(&starts[index].to_le_bytes());
            out.extend_from_slice(name.as_bytes());
        }
        let size = out.len() as u32 - cd;
        let total = parts.len() as u16;
        out.extend_from_slice(&[b'P', b'K', 5, 6]);
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&total.to_le_bytes());
        out.extend_from_slice(&total.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&cd.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    const W_ATTR: &str = "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"";

    /// 一份只有 `<w:compat>` 这一格的 settings
    fn docx_with(compat: &str) -> Value {
        let body = format!(
            "<w:settings {}><w:compat>{}</w:compat></w:settings>",
            W_ATTR, compat
        );
        docx(&packed(&[("word/settings.xml", body.as_str())]), 100)
    }

    const CFG_ATTR: &str = "xmlns:config=\"urn:oasis:names:tc:opendocument:xmlns:config:1.0\"";

    /// 一份只有 `ooo:configuration-settings` 那一组的 settings.xml
    fn odf_with(items: &str) -> Value {
        let body = format!(
            "<office:document-settings {}><config:config-item-set \
             config:name=\"ooo:configuration-settings\">{}</config:config-item-set>\
             </office:document-settings>",
            CFG_ATTR, items
        );
        odf(&packed(&[("settings.xml", body.as_str())]), 100)
    }

    /// 裸开关「在场即开」与「明写不要」是两句话，而 `w:val` 写没写也必须看得见
    #[test]
    fn a_bare_switch_is_on_until_the_file_says_otherwise() {
        let mine = docx_with(
            "<w:compatSetting w:name=\"compatibilityMode\" \
             w:uri=\"http://schemas.microsoft.com/office/word\" w:val=\"15\"/>\
             <w:useFELayout w:val=\"0\"/><w:doNotBreakWrappedTables w:val=\"false\"/>\
             <w:adjustLineHeightInTable/>",
        );
        assert_eq!(mine["settings_part"], json!(true));
        assert_eq!(mine["compat_written"], json!(true));
        assert_eq!(mine["children_total"], json!(4));
        assert_eq!(mine["mode"], json!("15"));
        assert_eq!(mine["mode_total"], json!(1));
        assert_eq!(
            mine["switches"],
            json!([
                {"name": "useFELayout", "val_written": true, "val": "0", "on": false},
                {"name": "doNotBreakWrappedTables", "val_written": true,
                 "val": "false", "on": false},
                {"name": "adjustLineHeightInTable", "val_written": false, "val": null,
                 "on": true},
            ])
        );
        // 真件里 72 份没有一枚裸开关写过 w:val，所以这三行只能自己造
        assert_eq!(mine["names_in_both_encodings"], json!([]));
    }

    /// 两种写法各自成书：真件里名字互不重叠，重叠了要报出来而不是并成一格
    #[test]
    fn two_shapes_that_happen_to_share_a_name_stay_two_rows() {
        let mine =
            docx_with("<w:compatSetting w:name=\"useFELayout\" w:val=\"1\"/><w:useFELayout/>");
        assert_eq!(mine["named_names"], json!(["useFELayout"]));
        assert_eq!(mine["switch_names"], json!(["useFELayout"]));
        assert_eq!(mine["names_in_both_encodings"], json!(["useFELayout"]));
        // 这一枚没写 w:uri，所以 `uris` 是空表（不是「缺省那个 uri」）
        assert_eq!(mine["uris"], json!([]));
        assert_eq!(mine["named"][0]["uri"], Value::Null);
        assert_eq!(mine["mode"], Value::Null);
    }

    /// 同一格写了两遍：数出来两遍，取值取第一遍（不静默覆盖，也不合并）
    #[test]
    fn a_second_saying_of_the_same_switch_is_counted_not_blended() {
        let mine = docx_with(
            "<w:compatSetting w:name=\"compatibilityMode\" w:val=\"14\"/>\
             <w:compatSetting w:name=\"compatibilityMode\" w:val=\"15\"/>\
             <w:compatSetting w:val=\"9\"/>",
        );
        assert_eq!(mine["mode"], json!("14"));
        assert_eq!(mine["mode_total"], json!(2));
        assert_eq!(mine["children_total"], json!(3));
        // 没有 `w:name` 的那一行照样在账上，只是不进名字表
        assert_eq!(mine["named"][2]["name"], Value::Null);
        assert_eq!(mine["named_names"], json!(["compatibilityMode"]));
    }

    /// 「这一族没看」与「看了而一个字没写」是两件事，而空壳是一句说过的话
    #[test]
    fn an_empty_block_and_a_missing_block_are_different_answers() {
        let empty = docx_with("");
        assert_eq!(empty["compat_written"], json!(true));
        assert_eq!(empty["compat_total"], json!(1));
        assert_eq!(empty["children_total"], json!(0));
        assert_eq!(empty["named"], json!([]));
        let body = format!("<w:settings {}><w:zoom/></w:settings>", W_ATTR);
        let none = docx(&packed(&[("word/settings.xml", body.as_str())]), 100);
        assert_eq!(none["settings_part"], json!(true));
        assert_eq!(none["compat_written"], json!(false));
        assert_eq!(none["children_total"], json!(0));
        // 部件在、可一个尖括号都没有：Python 那本解不开（ParseError），两家同一个判决
        let blank = docx(&packed(&[("word/settings.xml", "   \n  ")]), 100);
        assert_eq!(blank["settings_part"], json!(false));
        let absent = docx(&packed(&[("word/document.xml", "<w:document/>")]), 100);
        assert_eq!(absent["settings_part"], json!(false));
        assert_eq!(absent["available"], json!(true));
    }

    /// ODF 那一本：值在正文里、类型在属性上，而「缺类型」自己成一类
    #[test]
    fn flattened_items_are_picked_by_name_and_counted_by_type() {
        let mine = odf_with(
            "<config:config-item config:name=\"MsWordCompTrailingBlanks\" \
             config:type=\"boolean\">true</config:config-item>\
             <config:config-item config:name=\"DoNotBreakWrappedTables\" \
             config:type=\"boolean\"> true </config:config-item>\
             <config:config-item config:name=\"MsWordUlTrailSpace\" \
             config:type=\"boolean\">True</config:config-item>\
             <config:config-item config:name=\"Foo\">bar</config:config-item>",
        );
        assert_eq!(mine["item_set_written"], json!(true));
        assert_eq!(mine["items_total"], json!(4));
        assert_eq!(mine["booleans_total"], json!(3));
        // 第三种写法（大写 T）不算 true：这一格按文件写的原样比，不折算
        assert_eq!(mine["booleans_true"], json!(2));
        assert_eq!(
            mine["types"],
            json!([
                {"type": null, "count": 1},
                {"type": "boolean", "count": 3}
            ])
        );
        assert_eq!(mine["compat_item_total"], json!(3));
        assert_eq!(
            mine["compat_items"],
            json!([
                {"name": "DoNotBreakWrappedTables", "type": "boolean", "value": "true",
                 "via": "same-name"},
                {"name": "MsWordCompTrailingBlanks", "type": "boolean", "value": "true",
                 "via": "msword-prefix"},
                {"name": "MsWordUlTrailSpace", "type": "boolean", "value": "True",
                 "via": "msword-prefix"}
            ])
        );
        assert_eq!(mine["same_name_rows"], json!(["DoNotBreakWrappedTables"]));
    }

    /// 组名不对就不是这一本；`Foo` 那种说不上来路的也不进表（但进了计数）
    #[test]
    fn a_differently_named_item_set_is_not_this_book() {
        let body = "<office:document-settings xmlns:config=\"urn:oasis:names:tc:\
                    opendocument:xmlns:config:1.0\"><config:config-item-set \
                    config:name=\"ooo:foo\"><config:config-item config:name=\"MsWordX\" \
                    config:type=\"boolean\">true</config:config-item></config:config-item-set>\
                    </office:document-settings>";
        let mine = odf(&packed(&[("settings.xml", body)]), 100);
        assert_eq!(mine["settings_part"], json!(true));
        assert_eq!(mine["item_set_written"], json!(false));
        assert_eq!(mine["items_total"], json!(0));
        assert_eq!(mine["compat_item_total"], json!(0));
        assert_eq!(mine["types"], json!([]));
        let absent = odf(
            &packed(&[("content.xml", "<office:document-content/>")]),
            100,
        );
        assert_eq!(absent["settings_part"], json!(false));
        assert_eq!(absent["item_set_written"], json!(false));
        assert_eq!(absent["booleans_total"], json!(0));
    }
}
