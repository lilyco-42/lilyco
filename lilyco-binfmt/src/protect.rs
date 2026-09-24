//! 「这份文件还能动吗」这一份账。四个地方各存各的，而且**没有一个写着密码**：
//!
//! 1. OOXML 的文档保护在 `word/settings.xml` 的 `w:documentProtection` 上：`w:edit` 是
//!    限制类型（只读 / 只允许批注 / 只允许修订…），`w:enforcement` 才是开没开。
//! 2. OOXML 的表格保护分两层：`xl/workbook.xml` 里的 `w:workbookProtection`（结构锁、
//!    窗口锁）与每张表自己的 `sheetProtection`。
//! 3. ODF 的表格保护是 `table:table` 上的属性（`table:protected` + `table:protection-key`
//!    + 摘要算法 URI）；文档级的「保护表单/书签/字段」在 `settings.xml` 的 config-item 里。
//! 4. 遗留的 `.doc` / `.xls` / `.ppt`：保护写在表流的记录里（BIFF 的 PROTECT、Word 的
//!    flag 位），本模块读不出那些，就照实交回 null，不猜。
//!
//! 布尔值的写法两家不一样，这条是实测出来的：openpyxl 写 `sheet="1" formatCells="0"`，
//! LibreOffice 重写同一份东西时写 `sheet="true" formatCells="false"`（还把等于默认的
//! `insertRows="1"` 整个省掉）—— 所以判开关要两种拼法都认，见 [`on_off`]。
//! 哈希与密钥只报「在不在」与算法名：那是校验值，不是能还原的东西，也不必搬进答案里。

use serde_json::{json, Value};

use crate::odsheet::attr_of;
use crate::xmlscan::Node;

/// `1`/`0` 与 `true`/`false` 两种拼法都要认（上面那条实测），认不出来给 None
pub fn on_off(raw: Option<&str>) -> Option<bool> {
    match raw.map(|one| one.trim()) {
        None => None,
        Some("1") | Some("true") => Some(true),
        Some("0") | Some("false") => Some(false),
        Some(_) => None,
    }
}

/// 把元素上写着的开关原样抄下来（没写的不补默认值）
fn switches(node: &Node, names: &[&str]) -> serde_json::Map<String, Value> {
    let mut out = serde_json::Map::new();
    for name in names {
        if let Some(hit) = on_off(attr_of(node, name)) {
            out.insert((*name).to_string(), json!(hit));
        }
    }
    out
}

/// OOXML 的一张表：`sheetProtection` 在不在、开了什么、有没有校验值
pub fn xlsx_sheet(root: &Node, name: &str) -> Value {
    let Some(one) = root.descendants("sheetProtection").into_iter().next() else {
        return json!({"name": name, "element": false, "protected": false});
    };
    let locked = on_off(attr_of(one, "sheet")).unwrap_or(true);
    json!({
        "name": name,
        "element": true,
        "protected": locked,
        "password": attr_of(one, "password").is_some_and(|one| !one.is_empty()),
        "written": switches(one, &[
            "objects", "scenarios", "formatCells", "formatColumns", "formatRows",
            "insertColumns", "insertRows", "insertHyperlinks", "deleteColumns",
            "deleteRows", "selectLockedCells", "sort", "autoFilter", "pivotTables",
            "selectUnlockedCells",
        ]),
    })
}

/// OOXML 的工作簿级：`workbookProtection` 这个元素**在场但什么都没写**是合法且常见的
/// （openpyxl 就写一个空的），那不等于锁上了 —— 也不等于没写
pub fn xlsx_workbook(root: &Node) -> Value {
    let Some(one) = root.descendants("workbookProtection").into_iter().next() else {
        return json!({"element": false});
    };
    json!({
        "element": true,
        "lock_structure": on_off(attr_of(one, "lockStructure")),
        "lock_windows": on_off(attr_of(one, "lockWindows")),
        "book_password": attr_of(one, "password").is_some_and(|one| !one.is_empty()),
    })
}

/// ODF 的一张表：保护是 `table:table` 自己身上的属性
pub fn ods_table(one: &Node, name: &str) -> Value {
    let locked = on_off(attr_of(one, "protected")).unwrap_or(false);
    json!({
        "name": name,
        "protected": locked,
        "password": attr_of(one, "protection-key").is_some_and(|one| !one.is_empty()),
        // 摘要算法写成一个 URI：只留最后一段（那是人要看的东西），整串留在文件里
        "digest": attr_of(one, "protection-key-digest-algorithm")
            .map(|one| one.rsplit('/').next().unwrap_or(one).to_string()),
    })
}

/// ODF 的文档级：`settings.xml` 里那三个 config-item（表单 / 书签 / 字段）
pub fn odt_document(settings: &Node) -> Value {
    let wanted = [
        "ProtectForm",
        "ProtectBookmarks",
        "ProtectFields",
        "ProtectBook",
    ];
    let mut found = serde_json::Map::new();
    for one in settings.descendants("config-item") {
        let Some(key) = attr_of(one, "name") else {
            continue;
        };
        if !wanted.iter().any(|one| *one == key) {
            continue;
        }
        let hit = one
            .descendants("#text")
            .into_iter()
            .next()
            .map(|one| one.direct.trim().to_string())
            .unwrap_or_default();
        found.insert(key.to_string(), json!(hit == "true"));
    }
    let any = found.values().any(|one| *one == json!(true));
    json!({
        "items": found,
        "protected": any,
    })
}

/// docx 的文档保护：`w:edit` 是限制类型，`w:enforcement` 才是开没开
pub fn docx_document(settings: Option<&Node>) -> Value {
    let Some(root) = settings else {
        return json!({"element": false, "protected": false});
    };
    let Some(one) = root.descendants("documentProtection").into_iter().next() else {
        return json!({"element": false, "protected": false});
    };
    let enforced = on_off(attr_of(one, "enforcement")).unwrap_or(true);
    json!({
        "element": true,
        "protected": enforced,
        "edit": attr_of(one, "edit"),
        "enforcement": enforced,
        "formatted_text": on_off(attr_of(one, "formatting")),
        "style_restriction": on_off(attr_of(one, "formattingRestrictions")),
        "password": attr_of(one, "hash").is_some_and(|one| !one.is_empty()),
        "algorithm": attr_of(one, "cryptAlgorithmType"),
        "spin_count": attr_of(one, "cryptSpinCount"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(src: &str) -> Node {
        crate::xmlscan::parse_str(src)
    }

    /// 同一句话的两种拼法：openpyxl 写 `1`/`0`，LibreOffice 重写同一份东西时写
    /// `true`/`false` 并且把等于默认的开关省掉 —— 两种都要认，且省掉的不补成假的
    #[test]
    fn both_spellings_of_a_switch_are_read() {
        assert_eq!(on_off(Some("1")), Some(true));
        assert_eq!(on_off(Some("true")), Some(true));
        assert_eq!(on_off(Some("0")), Some(false));
        assert_eq!(on_off(Some("false")), Some(false));
        assert_eq!(on_off(Some("yes")), None, "不认识的拼法不猜");
        assert_eq!(on_off(None), None);

        let hand = parse(
            r#"<worksheet><sheetData/><sheetProtection sheet="1" formatCells="0" insertRows="1" password="6E4E"/><mergeCells/></worksheet>"#,
        );
        let one = xlsx_sheet(&hand, "预算表");
        assert_eq!(one["protected"], json!(true), "{one}");
        assert_eq!(one["password"], json!(true));
        assert_eq!(one["written"]["formatCells"], json!(false));
        assert_eq!(one["written"]["insertRows"], json!(true));

        let lo = parse(
            r#"<worksheet><sheetProtection sheet="true" password="6e4e" formatCells="false"/></worksheet>"#,
        );
        let two = xlsx_sheet(&lo, "预算表");
        assert_eq!(two["protected"], json!(true));
        assert_eq!(two["written"]["formatCells"], json!(false));
        // LibreOffice 省掉了 insertRows：这里就该是「没有这一项」，不是 false
        assert_eq!(two["written"].get("insertRows"), None, "{two}");

        let none = parse(r#"<worksheet><sheetData/></worksheet>"#);
        let three = xlsx_sheet(&none, "说明");
        assert_eq!(three["element"], json!(false));
        assert_eq!(three["protected"], json!(false));
    }

    /// 空的 `<workbookProtection/>` 是合法且常见的（openpyxl 就这么写）：
    /// 「元素在场」与「锁上了」是两件事，分开报
    #[test]
    fn an_empty_workbook_protection_is_not_a_lock() {
        let bare = parse(r#"<workbook><workbookProtection/><bookViews/></workbook>"#);
        let one = xlsx_workbook(&bare);
        assert_eq!(one["element"], json!(true), "{one}");
        assert_eq!(
            one["lock_structure"],
            Value::Null,
            "没写就是没写，不补默认值"
        );
        assert_eq!(one["book_password"], json!(false));

        let locked = parse(
            r#"<workbook><workbookProtection lockStructure="1" password="1234"/></workbook>"#,
        );
        let two = xlsx_workbook(&locked);
        assert_eq!(two["lock_structure"], json!(true));
        assert_eq!(two["book_password"], json!(true));

        let missing = parse(r#"<workbook><bookViews/></workbook>"#);
        assert_eq!(xlsx_workbook(&missing)["element"], json!(false));
    }

    /// 文档级保护：`w:edit` 说的是限制成什么，`w:enforcement` 才是开没开
    #[test]
    fn document_protection_reports_the_kind_and_whether_it_is_on() {
        let on = parse(
            r#"<w:settings><w:documentProtection w:edit="readOnly" w:enforcement="1" w:cryptAlgorithmType="typeAny" w:cryptSpinCount="100000" w:hash="AAAA"/></w:settings>"#,
        );
        let one = docx_document(Some(&on));
        assert_eq!(one["protected"], json!(true), "{one}");
        assert_eq!(one["edit"], json!("readOnly"));
        assert_eq!(one["password"], json!(true));
        assert_eq!(one["algorithm"], json!("typeAny"));
        assert_eq!(one["spin_count"], json!("100000"));

        let off = parse(
            r#"<w:settings><w:documentProtection w:edit="trackedChanges" w:enforcement="0"/></w:settings>"#,
        );
        let two = docx_document(Some(&off));
        assert_eq!(two["element"], json!(true), "元素在，只是没开着");
        assert_eq!(two["protected"], json!(false));
        assert_eq!(two["edit"], json!("trackedChanges"));

        assert_eq!(docx_document(None)["element"], json!(false));
        let plain = parse("<w:settings/>");
        assert_eq!(docx_document(Some(&plain))["protected"], json!(false));
    }

    /// ODF 的表保护是属性，文档级保护在 settings.xml 的 config-item 里；
    /// 从 docx 转过来时那三个全是 false（这条是 fixture 实测的，见 README）
    #[test]
    fn opendocument_keeps_table_locks_on_the_table() {
        // `ods_table` 拿的是**那张表**（保护是表上的属性），不是伪根：
        // 递 `#doc` 进去看到的是「没有这些属性」，那不是文件说的话
        let doc = parse(
            r#"<table:table table:name="预算表" table:protected="true" table:protection-key="abc==" table:protection-key-digest-algorithm="http://docs.oasis-open.org/office/ns/table/legacy-hash-excel"/>"#,
        );
        let one = doc.child("table:table").expect("表元素在");
        let done = ods_table(one, "预算表");
        assert_eq!(done["protected"], json!(true), "{done}");
        assert_eq!(done["password"], json!(true));
        assert_eq!(
            done["digest"],
            json!("legacy-hash-excel"),
            "URI 只留最后一段"
        );

        let bare = parse(r#"<table:table table:name="说明"/>"#);
        let two = bare.child("table:table").expect("表元素在");
        assert_eq!(ods_table(two, "说明")["protected"], json!(false));

        let settings = parse(
            r#"<office:settings><config:config-item-set>
               <config:config-item config:name="ProtectForm" config:type="boolean">false</config:config-item>
               <config:config-item config:name="ProtectBookmarks" config:type="boolean">true</config:config-item>
             </config:config-item-set></office:settings>"#,
        );
        let done = odt_document(&settings);
        assert_eq!(done["protected"], json!(true), "{done}");
        assert_eq!(done["items"]["ProtectForm"], json!(false));
        assert_eq!(done["items"]["ProtectBookmarks"], json!(true));
    }
}
