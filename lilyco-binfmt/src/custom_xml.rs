//! 包里那几份自定义 XML 存储 —— `customXml/itemN.xml` 那一族部件各自还剩多少字、
//! 它自己说自己是哪个 schema、正文有没有一条手指着它，以及那一跳（件自己的 `.rels`
//! → `itemPropsN.xml` → 一个 `ds:itemID`）断没断。
//!
//! 两条链分开交，因为它们各会各的断：
//!
//! * 存储 →（`customXml/_rels/itemN.xml.rels` 里那条 `customXmlProps`）→ 那一份 props
//!   →（`ds:itemID` 与 `ds:schemaRefs/ds:schemaRef/@ds:uri`）→ 号与 schema；
//!   那一跳断了交 `props_found: false`，不替文件猜一份；
//! * 正文那两条手指各是一问：`w:customXml` 圈住一段字（它自己带 `w:schemaLoc`），
//!   `w:sdt` 上的 `w:dataBinding/@w:storeItemID` 指的就是 props 里那个号 ——
//!   号对得上几条、对不上几条，都按整包扫出来的数交。
//!
//! 实测（`customxml.docx` 由 python-docx 打底 + 按真件形状加部件，`customxml-lo.docx`
//! 是 LibreOffice 重写同一份；另两份是自带的：`notes.docx` 与本机 25 份真件同一形状）：
//! 1. **`python-docx` 那份打底模板本来就带着一份存储**（`customXml/item1.xml` 里
//!    一个 `b:Sources`、0 条孩子、props 里 `ds:itemID` 是一个 GUID 加一条 bibliography
//!    的 uri）—— 本机 25 份真件 docx 里那一份的形状与它一字不差，因为那些件也是
//!    Word 的引用管理器写的：这一族在真件里就是「存储躺在包里、正文一条手指都不写」
//!    （25/25 份 `w:customXml` 与 `w:dataBinding` 各 0 处）；
//! 2. 「部件在」与「部件里还有字」是两问：LibreOffice 重写时把 `item1.xml`、`item2.xml`
//!    **整件清空成 0 字节**（三件全空），而 `itemPropsN.xml` 三份都留着、
//!    `ds:itemID` 也留着 —— 所以 `items` 3 / `items_empty` 3 / 每行 `root` 与 `children`
//!    是 null。清空不是丢失：那份件在包里的名字、内容类型、那一跳到 props 都还在；
//! 3. 同一件事被它**多写了一份**：打底那两份存储变三份（多出 `item3.xml` 与
//!    `itemProps3.xml`），而 `itemProps1` 与 `itemProps2` 里 `ds:itemID` **是同一个号**
//!    —— 于是正文那一条 `w:dataBinding` 在重写后同时指着两份存储（`bound` 两行都是 1）。
//!    「一条手指解到哪一份」在这一副件里判不住，如实交两个 1 而不是替文件挑一份；
//! 4. 另一条手指它不认：`w:customXml` 那一条被 LibreOffice **整条丢掉**
//!    （`anchors_custom_xml` 1 → 0），`w:sdt` 上那条 `dataBinding` 照样留着（还自己补了一枚
//!    `<w:text/>`）—— 同一族里两条手指两种待遇；
//! 5. `customXml/itemN.xml` 自己**不在** `[Content_Types].xml` 上点名（只有 props 那一份点），
//!    它靠 `Default Extension="xml"` 兜着 —— 所以 `declared` 恒 false 而 `default_for_xml`
//!    恒 true，这两格合起来才是「这一族部件怎么被包承认」的答案。
//!
//! 不做的事：**不解释存储里的字**（那是一份别人定义的 schema，本仓不猜它的意思），
//! **不解 XPath**（`w:xpath` 原样交在 `binding_ids` 旁边那一格之外，这里只交号），
//! ODF 那一族不交这个键（ODF 没有 OPC 包，`customXml/` 这一层不存在）。

use crate::xmlscan;
use crate::zipread;
use serde_json::{json, Value};

/// 那一族部件的目录名：存储、props 与 rels 全住在它下面
const CX_FOLDER: &str = "customXml/";

fn is_item(name: &str) -> bool {
    let tail = match name.strip_prefix(CX_FOLDER) {
        Some(one) => one,
        None => return false,
    };
    let body = match tail.strip_suffix(".xml") {
        Some(one) => one,
        None => return false,
    };
    let digits = match body.strip_prefix("item") {
        Some(one) => one,
        None => return false,
    };
    !digits.is_empty() && digits.chars().all(|one| one.is_ascii_digit())
}

fn member_parse(bytes: &[u8], part: &str) -> Option<xmlscan::Node> {
    let member = zipread::member(bytes, part, zipread::DEFAULT_MEMBER_CAP).ok()?;
    let text = member.as_text();
    if text.trim().is_empty() {
        return None;
    }
    Some(xmlscan::parse_str(&text))
}

/// `#doc` 那枚伪根下面的第一枚元素：属性与局部名都挂在它身上，而 `parse_str` 交回来的是伪根
fn doc_element(node: &xmlscan::Node) -> Option<&xmlscan::Node> {
    element_kids(node).into_iter().next()
}

/// 只数元素孩子：容错解析会把「两个标签之间的一个空格」摊成一枚 `#text`，
/// 而 ElementTree 遍历孩子只给元素 —— 两家对同一份件得数出同一个孩子数
fn element_kids(node: &xmlscan::Node) -> Vec<&xmlscan::Node> {
    node.children
        .iter()
        .filter(|one| one.name != "#text")
        .collect()
}

/// `[Content_Types].xml`：点过名的 customXml 部件，与有没有一条 `Default Extension="xml"`
fn content_types(root: &xmlscan::Node) -> (Vec<String>, bool) {
    let mut named: Vec<String> = Vec::new();
    let mut has_default = false;
    for one in element_kids(root) {
        let local = one.local();
        if local == "Override" {
            if let Some(had) = one.attr_local("PartName") {
                if let Some(tail) = had.strip_prefix("/customXml/") {
                    named.push(format!("customXml/{}", tail));
                }
            }
        } else if local == "Default" {
            if let Some(had) = one.attr_local("Extension") {
                if had.eq_ignore_ascii_case("xml") {
                    has_default = true;
                }
            }
        }
    }
    (named, has_default)
}

/// 那一件存储自己的 `.rels`：找那条 `customXmlProps`，把 Target 解成包内全名
fn props_of(bytes: &[u8], part: &str) -> Option<String> {
    let folder = match part.rsplit_once('/') {
        Some((head, _)) => head.to_string(),
        None => String::new(),
    };
    let rels = format!("{}._rels/{}.rels", folder, part.rsplit('/').next()?);
    let root = member_parse(bytes, rels.as_str())?;
    for one in root.descendants("Relationship") {
        let kind = one.attr_local("Type")?.rsplit('/').next()?;
        if kind != "customXmlProps" {
            continue;
        }
        let target = one.attr_local("Target")?;
        let joined = if let Some(had) = target.strip_prefix('/') {
            had.to_string()
        } else if folder.is_empty() {
            target.to_string()
        } else {
            format!("{}/{}", folder, target)
        };
        return Some(joined);
    }
    None
}

/// 整包扫那两条手指：`w:customXml` 的条数，与每条 `w:dataBinding` 写的 `storeItemID`
fn body_pointers(bytes: &[u8], names: &[String], limit: usize) -> (usize, Vec<Value>) {
    let mut xmls = 0usize;
    let mut ids: Vec<Value> = Vec::new();
    let mut scanned = 0usize;
    for name in names {
        if !name.ends_with(".xml") || name.starts_with(CX_FOLDER) || name.ends_with(".rels") {
            continue;
        }
        if name == "[Content_Types].xml" {
            continue;
        }
        if scanned >= limit * 4 {
            break;
        }
        scanned += 1;
        let root = match member_parse(bytes, name) {
            Some(one) => one,
            None => continue,
        };
        xmls += root.descendants("customXml").len();
        for one in root.descendants("dataBinding") {
            if ids.len() >= limit {
                break;
            }
            ids.push(match one.attr_local("storeItemID") {
                Some(had) => Value::String(had.to_string()),
                None => Value::Null,
            });
        }
    }
    (xmls, ids)
}

/// 一份包一本账：`customXml/` 那一族部件与正文那两条手指
pub(crate) fn ledger(bytes: &[u8], limit: usize) -> Value {
    let names = zipread::member_names(bytes);
    let names: Vec<String> = names;
    let mut items: Vec<&String> = names.iter().filter(|one| is_item(one)).collect();
    items.sort_unstable();
    let parts_total = names
        .iter()
        .filter(|one| one.starts_with(CX_FOLDER))
        .count();
    let ct_doc = member_parse(bytes, "[Content_Types].xml");
    let (named, has_default) = match ct_doc.as_ref().and_then(doc_element) {
        Some(root) => content_types(&root),
        None => (Vec::new(), false),
    };
    let (xmls, ids) = body_pointers(bytes, &names, limit);
    let mut entries: Vec<Value> = Vec::new();
    let mut empty = 0usize;
    let mut rows: Vec<(Option<String>, Option<String>)> = Vec::new();
    for part in items.iter() {
        let raw = match zipread::member(bytes, part.as_str(), zipread::DEFAULT_MEMBER_CAP) {
            Ok(one) => one.as_text(),
            Err(_) => String::new(),
        };
        if raw.trim().is_empty() {
            empty += 1;
        }
        let parsed = member_parse(bytes, part.as_str());
        let (root_name, children) = match parsed.as_ref().and_then(doc_element) {
            Some(node) => (
                Some(node.local().to_string()),
                Some(element_kids(node).len()),
            ),
            None => (None, None),
        };
        let hop = props_of(bytes, part);
        let props_name = hop.as_ref().and_then(|had| {
            if names.iter().any(|one| one == had) {
                Some(had.rsplit('/').next().unwrap_or(had).to_string())
            } else {
                None
            }
        });
        let found = hop
            .as_ref()
            .is_some_and(|had| names.iter().any(|one| one == had));
        let mut item_id: Option<String> = None;
        let mut uris: Vec<Value> = Vec::new();
        if let Some(had) = &hop {
            let props_doc = member_parse(bytes, had);
            if let Some(head) = props_doc.as_ref().and_then(doc_element) {
                item_id = head.attr_local("itemID").map(|one| one.to_string());
                for one in head.descendants("schemaRef") {
                    if uris.len() >= limit {
                        break;
                    }
                    uris.push(match one.attr_local("uri") {
                        Some(raw2) => Value::String(raw2.to_string()),
                        None => Value::Null,
                    });
                }
            }
        }
        rows.push((hop.clone(), item_id.clone()));
        let bound = match &item_id {
            Some(had) => ids
                .iter()
                .filter(|one| one.as_str() == Some(had.as_str()))
                .count(),
            None => 0,
        };
        if entries.len() < limit {
            entries.push(json!({
                "part": part.to_string(),
                "size": raw.len(),
                "root": root_name,
                "children": children,
                "props_rel": props_name,
                "props_found": found,
                "item_id": item_id,
                "schema_uris": uris,
                "bound": bound,
                "declared": named.iter().any(|one| one == *part),
            }));
        }
    }
    let known: Vec<Option<String>> = rows.iter().map(|one| one.1.clone()).collect();
    let unresolved = ids
        .iter()
        .filter(|one| match one.as_str() {
            Some(had) => !known.iter().any(|hit| hit.as_deref() == Some(had)),
            None => true,
        })
        .count();
    json!({
        "family": "ooxml",
        "available": true,
        "parts_total": parts_total,
        "items": items.len(),
        "items_empty": empty,
        "overrides": named.len(),
        "default_for_xml": has_default,
        "anchors_custom_xml": xmls,
        "anchors_data_binding": ids.len(),
        "binding_ids": ids,
        "unresolved_bindings": unresolved,
        "entries": entries,
        "cut": items.len() > entries.len(),
    })
}
