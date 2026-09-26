//! 主题部件那一份账：`theme1.xml` 里到底写了什么。
//!
//! 办公文件的颜色与字体有两处写法：正文点一个**名字**（`text1`、`majorHAnsi`、`theme="4"`），
//! 而那个名字坐在包里另一个部件里。老账本只走到一半 —— `office-doc` 的字体那一本会跳一次
//! （把 `asciiTheme` 落到字面字体名），`office-slide` 只交一份主题部件的**名字清单**，
//! 而颜色那一跳整个没做（`about` 里自己承认「解它要开 theme1.xml，而这一本不开」）。
//! 这一本就是把那个部件读出来，按文件写着的交。
//!
//! 三家的包都有这个部件（`word/`、`xl/`、`ppt/theme/themeN.xml`），ODF 一家没有主题这个概念，
//! 所以那三族交出来的是一本零条的账而不是缺键。一份件里可以有很多个主题部件
//! （一个母版一个，实测最满的那份 pptx 有 12 个），因此账是**逐件**的，合计另算一本。
//!
//! 实测（132 份件里的 194 个主题部件：word 家 72、sheet 家 39、slide 家 83 —— 一份 pptx 有 12 个；
//! ODF 那 67 份（odt 41 / ods 14 / odp 12）一件都没有，交零条的账）：
//! * 十二格的**顺序与名字**在两个生产者手里都对（194/194 canonical），但 dk1/lt1 这两格有
//!   **两种写法**：68 个部件写 `<a:sysClr val="windowText" lastClr="000000"/>`（`lt1` 那一路是
//!   `window` / `FFFFFF`，实测只有这两个组合），126 个部件写 `<a:srgbClr val="000000"/>`。
//!   `lastClr` 只是缓存的猜测，`val` 才是「跟着系统走」那一句，所以两个键都留，不合成一个数；
//!   顺带一条自证：写 `extraClrSchemeLst` 的那 68 个与 dk1 用 `sysClr` 的那 68 个**是同一批**
//!   （例外 0 个），所以「MS 那一路」在这两个记号上同进同出；
//! * 一个部件里的三个 `@name` 是**三个各说各的**：`a:theme@name` 有 `Office Theme`（187）与
//!   `Office`（7）两种，`a:clrScheme@name` 有 `Office`（186）与 `LibreOffice`（8），
//!   而 `a:fontScheme@name` 194 个全写 `Office` —— LibreOffice 重写时改的就是中间那一个；
//!   `a:fmtScheme@name` 只在那 68 个里有，另 126 个不写；
//! * `ea` / `cs` 两个字体槽：**MS 那一路写 `typeface=""`（写了，但是空的 —— 这一路没选脸），
//!   LibreOffice 自己重打的那一批写 `DejaVu Sans`**。空串与不在场是两件事，账里两者分列；
//! * 按书写系统分的 `a:font script="…"` 那一批只在 MS 那一路写，一套 29 或 30 条（差的是一枚
//!   `Geor`），而 `script="Hans"` 那一条实测全是 `宋体` —— 中文用户问「主题说中文用什么脸」，
//!   答案在这批里而不在 `ea` 里；LibreOffice 重打的那 82 个部件一条都不写；
//! * `fmtScheme` 实测四列各三条（194/194 与规范一致），但这一本交的是**数出来**的四列。
//!
//! 不做的事：**不替文件算色**。正文的指针可以带 `themeTint` / `themeShade` / `<a:tint>`，
//! 真件量下来 tint 那一支与「线性混白」逐格吻合（480/480），而 shade 那一支在 2136 条里
//! 与八种候选算法（RGB 与 HSL 两个空间 × 四种取整）都对不齐 —— 所以对不上的就交不出的，
//! 这一本只按写着的交，不交一个算出来的色。

use serde_json::{json, Value};

use crate::xmlscan::{self, Node};
use crate::zipread;

/// 十二格按规范说的顺序；文件写的顺序与这个一致就叫 canonical（实测全都一致）
const CANON: [&str; 12] = [
    "dk1", "lt1", "dk2", "lt2", "accent1", "accent2", "accent3", "accent4", "accent5", "accent6",
    "hlink", "folHlink",
];

/// 主题部件的名字：`theme1.xml`、`theme11.xml`、`theme.xml` 都算（`.xml` 后缀必须有）
fn is_theme_part(name: &str) -> bool {
    let tail = name.rsplit('/').next().unwrap_or(name);
    let Some(body) = tail.strip_suffix(".xml") else {
        return false;
    };
    let Some(rest) = body.strip_prefix("theme") else {
        return false;
    };
    rest.chars().all(|one| one.is_ascii_digit())
}

/// 往一张计数表里加一笔（按写的值当键，没写的数成一格，键叫 `(没写)`）
fn tally_into(map: &mut serde_json::Map<String, Value>, raw: Option<&str>) {
    let key = raw.unwrap_or("(没写)").to_string();
    let next = map.get(&key).and_then(Value::as_u64).unwrap_or(0) + 1;
    map.insert(key, json!(next));
}

/// 计数：键不在场就当 0 起算（`themes` 先把所有键声明成 0，所以这条只在合计里走）
fn bump(sum: &mut serde_json::Map<String, Value>, key: &str, add: usize) {
    if add == 0 {
        return;
    }
    let next = sum.get(key).and_then(Value::as_u64).unwrap_or(0) + add as u64;
    sum.insert(key.to_string(), json!(next));
}

/// 槽位那三个键的账：`(在场且非空, 在场但空, 不在场)` 一一分列
fn face_slot(row: &Value, which: &str) -> &'static str {
    match row.get(which) {
        None | Some(Value::Null) => "missing",
        Some(face) if face.as_str() == Some("") => "blank",
        Some(_) => "written",
    }
}

/// 一个色格：`kind` 是那一路选色元素的局部名，`written` 是文件自己拼出的十六进制色，
/// `system` 只在 `sysClr` 那一路有（`windowText` / `window` 那一串）。
/// 空槽（一个子元素都没有）三个键全交 null —— 没有生产者这么写过，但解得出来就得说
fn slot_row(one: &Node) -> Value {
    let name = one.local().to_string();
    let Some(head) = one.children.first() else {
        return json!({"slot": name, "kind": Value::Null, "written": Value::Null,
                      "system": Value::Null});
    };
    let kind = head.local().to_string();
    if kind == "srgbClr" {
        return json!({"slot": name, "kind": kind,
                      "written": head.attr_local("val"), "system": Value::Null});
    }
    if kind == "sysClr" {
        return json!({"slot": name, "kind": kind,
                      "written": head.attr_local("lastClr"),
                      "system": head.attr_local("val")});
    }
    json!({"slot": name, "kind": kind, "written": Value::Null, "system": Value::Null})
}

/// 一套字（major 或 minor）：`kids` 按文件写的顺序交孩子元素名，
/// `latin` / `ea` / `cs` 三个槽各按写着的交 —— 元素不在场交 null，在场而 `typeface` 为空交 `""`
fn face_row(role: &str, node: &Node) -> Value {
    let kids: Vec<String> = node
        .children
        .iter()
        .map(|one| one.local().to_string())
        .collect();
    let faces: Vec<Value> = node
        .all("font")
        .iter()
        .map(|one| {
            json!({"script": one.attr_local("script"),
                   "typeface": one.attr_local("typeface")})
        })
        .collect();
    let face_total = faces.len();
    let pick = |which: &str| -> Value {
        match node.child(which) {
            Some(one) => json!(one.attr_local("typeface")),
            None => Value::Null,
        }
    };
    json!({
        "role": role,
        "kids": kids,
        "faces": faces,
        "face_total": face_total,
        "latin": pick("latin"),
        "ea": pick("ea"),
        "cs": pick("cs"),
    })
}

/// `fmtScheme` 下那几列（fillStyleLst / lnStyleLst / effectStyleLst / bgFillStyleLst）
/// 各自有几个孩子：规范说四列各三个，而这一本数出来的是文件写的
fn fmt_rows(node: &Node) -> Vec<Value> {
    node.children
        .iter()
        .map(|one| json!({"list": one.local(), "entries": one.children.len()}))
        .collect()
}

/// 解不出来的那一份件：键要与解得出来的**一样多**，只是每一格都没内容。
/// 「读不到这个部件」与「读到了但里面没东西」在账上是同一种沉默，两种都不能缺键
fn unread_row(name: &str) -> Value {
    json!({"part": name, "unread": true, "theme_name": Value::Null,
           "root_children": [], "scheme_name": Value::Null,
           "font_name": Value::Null, "fmt_name": Value::Null,
           "slots": [], "slot_total": 0, "sys_clr": 0, "srgb_clr": 0,
           "other_kind": 0, "empty_slot": 0, "canonical": false,
           "fonts": [], "font_roles": 0, "fmt": [], "fmt_lists": 0})
}

/// 一个主题部件的账，同时把合计往 `sum` 里加。
/// `children` 里只有元素（文本进 `direct`，注释与处理指令被跳过），所以这里不用再筛
fn one_part(name: &str, raw: &[u8], sum: &mut serde_json::Map<String, Value>) -> Value {
    bump(sum, "theme_parts", 1);
    let parsed = xmlscan::parse(raw);
    let Some(root) = parsed.children.first() else {
        bump(sum, "unread", 1);
        return unread_row(name);
    };
    let written: Vec<String> = root
        .children
        .iter()
        .map(|one| one.local().to_string())
        .collect();
    let theme_name = root.attr_local("name");
    let elements = root.child("themeElements");
    let scheme = elements.and_then(|had| had.child("clrScheme"));
    let font_scheme = elements.and_then(|had| had.child("fontScheme"));
    let fmt_scheme = elements.and_then(|had| had.child("fmtScheme"));
    let slots: Vec<Value> = match scheme {
        Some(had) => had.children.iter().map(slot_row).collect(),
        None => Vec::new(),
    };
    let slot_total = slots.len();
    let sys_clr = slots
        .iter()
        .filter(|one| one.get("kind").and_then(Value::as_str) == Some("sysClr"))
        .count();
    let srgb_clr = slots
        .iter()
        .filter(|one| one.get("kind").and_then(Value::as_str) == Some("srgbClr"))
        .count();
    let empty_slot = slots
        .iter()
        .filter(|one| one.get("kind").and_then(Value::as_str).is_none())
        .count();
    let other_kind = slot_total - sys_clr - srgb_clr - empty_slot;
    let names: Vec<&str> = slots
        .iter()
        .filter_map(|one| one.get("slot").and_then(Value::as_str))
        .collect();
    let canonical = names.as_slice() == CANON.as_slice();
    let mut fonts: Vec<Value> = Vec::new();
    if let Some(had) = font_scheme {
        for role in ["majorFont", "minorFont"] {
            if let Some(one) = had.child(role) {
                fonts.push(face_row(role, one));
            }
        }
    }
    let fmt: Vec<Value> = match fmt_scheme {
        Some(had) => fmt_rows(had),
        None => Vec::new(),
    };
    let scheme_name = scheme.and_then(|had| had.attr_local("name"));
    let fmt_name = fmt_scheme.and_then(|had| had.attr_local("name"));
    let font_roles = fonts.len();
    let fmt_lists = fmt.len();
    bump(sum, "slots", slot_total);
    bump(sum, "sys_clr", sys_clr);
    bump(sum, "srgb_clr", srgb_clr);
    bump(sum, "other_kind", other_kind);
    bump(sum, "empty_slot", empty_slot);
    bump(sum, if canonical { "canon" } else { "off_canon" }, 1);
    bump(sum, "font_roles", font_roles);
    bump(sum, "fmt_lists", fmt.len());
    bump(
        sum,
        if fmt_name.is_some() {
            "fmt_named"
        } else {
            "fmt_unnamed"
        },
        1,
    );
    bump(
        sum,
        "extra_clr_scheme",
        if written.iter().any(|one| one == "extraClrSchemeLst") {
            1
        } else {
            0
        },
    );
    bump(
        sum,
        "object_defaults",
        if written.iter().any(|one| one == "objectDefaults") {
            1
        } else {
            0
        },
    );
    for one in &fonts {
        let total = one.get("face_total").and_then(Value::as_u64).unwrap_or(0) as usize;
        bump(sum, "faces", total);
        bump(
            sum,
            if total > 0 {
                "roles_with_faces"
            } else {
                "roles_without_faces"
            },
            1,
        );
        for which in ["latin", "ea", "cs"] {
            bump(sum, &format!("{}_{}", which, face_slot(one, which)), 1);
        }
    }
    for (key, raw) in [
        ("by_theme_name", theme_name),
        ("by_scheme_name", scheme_name),
    ] {
        let had = sum
            .entry(key.to_string())
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .expect("计数表一定是对象");
        tally_into(had, raw);
    }
    json!({
        "part": name,
        "unread": false,
        "theme_name": theme_name,
        "root_children": written,
        "scheme_name": scheme_name,
        "font_name": font_scheme.and_then(|had| had.attr_local("name")),
        "fmt_name": fmt_name,
        "slots": slots,
        "slot_total": slot_total,
        "sys_clr": sys_clr,
        "srgb_clr": srgb_clr,
        "other_kind": other_kind,
        "empty_slot": empty_slot,
        "canonical": canonical,
        "fonts": fonts,
        "font_roles": font_roles,
        "fmt": fmt,
        "fmt_lists": fmt_lists,
    })
}

/// 一册包的主题账：逐个主题部件一本，合计一本。`limit` 只截清单，不截算术
pub fn themes(zip: &[u8], limit: usize) -> Value {
    let (dirs, _) = zipread::entries(zip);
    let mut sum = new_totals();
    let mut listed: Vec<Value> = Vec::new();
    let mut total = 0usize;
    for one in &dirs {
        let clean = one.name.trim_start_matches("./");
        if !is_theme_part(clean) {
            continue;
        }
        total += 1;
        let row = match zipread::read_member(zip, one, zipread::DEFAULT_MEMBER_CAP) {
            Ok(member) => one_part(clean, &member.data, &mut sum),
            Err(_) => {
                bump(&mut sum, "theme_parts", 1);
                bump(&mut sum, "unread", 1);
                unread_row(clean)
            }
        };
        if listed.len() < limit {
            listed.push(row);
        }
    }
    json!({
        "parts": listed,
        "total": total,
        "listed": listed.len(),
        "cut": total > limit,
        "totals": sum,
    })
}

/// 合计那二十七本先声明成 0：缺键与零条是两件事
fn new_totals() -> serde_json::Map<String, Value> {
    let mut sum = serde_json::Map::new();
    for key in [
        "theme_parts",
        "unread",
        "slots",
        "sys_clr",
        "srgb_clr",
        "other_kind",
        "empty_slot",
        "canon",
        "off_canon",
        "font_roles",
        "latin_written",
        "latin_blank",
        "latin_missing",
        "ea_written",
        "ea_blank",
        "ea_missing",
        "cs_written",
        "cs_blank",
        "cs_missing",
        "faces",
        "roles_with_faces",
        "roles_without_faces",
        "fmt_lists",
        "fmt_named",
        "fmt_unnamed",
        "extra_clr_scheme",
        "object_defaults",
    ] {
        sum.insert(key.to_string(), json!(0));
    }
    sum.insert("by_theme_name".to_string(), json!({}));
    sum.insert("by_scheme_name".to_string(), json!({}));
    sum
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 九个合计按固定顺序摊成一行，测试里一次看全
    fn agg(ledger: &Value) -> Value {
        Value::Array(
            [
                "theme_parts",
                "unread",
                "slots",
                "sys_clr",
                "srgb_clr",
                "other_kind",
                "empty_slot",
                "canon",
                "off_canon",
            ]
            .iter()
            .map(|key| ledger.get(*key).cloned().unwrap_or(Value::Null))
            .collect::<Vec<Value>>(),
        )
    }

    fn book(src: &str) -> Value {
        one_part("word/theme/theme1.xml", src.as_bytes(), &mut new_totals())
    }

    #[test]
    fn part_names_are_matched_the_way_packages_write_them() {
        assert!(is_theme_part("ppt/theme/theme1.xml"));
        assert!(is_theme_part("word/theme/theme12.xml"));
        assert!(is_theme_part("theme.xml"));
        assert!(is_theme_part("./xl/theme/theme1.xml"));
        assert!(!is_theme_part("ppt/theme/_rels/theme1.xml.rels"));
        assert!(!is_theme_part("ppt/theme/themeX.xml"));
        assert!(!is_theme_part("ppt/theme/theme1.XML"));
        // 名字这一关只看尾巴，不看目录：包里的部件名再由 read_member 按全名去找
        assert!(is_theme_part("anything/else/theme1.xml"));
    }

    #[test]
    fn both_spellings_of_a_slot_survive_separately() {
        let row = book("<a:theme xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" name=\"Office Theme\"><a:themeElements><a:clrScheme name=\"Office\"><a:dk1><a:sysClr val=\"windowText\" lastClr=\"000000\"/></a:dk1><a:lt1><a:srgbClr val=\"FFFFFF\"/></a:lt1></a:clrScheme></a:themeElements></a:theme>");
        assert_eq!(row["slot_total"], 2);
        assert_eq!(row["sys_clr"], 1);
        assert_eq!(row["srgb_clr"], 1);
        assert_eq!(
            row["slots"],
            json!([
                {"slot": "dk1", "kind": "sysClr", "written": "000000", "system": "windowText"},
                {"slot": "lt1", "kind": "srgbClr", "written": "FFFFFF", "system": null}
            ]),
            "`lastClr` 与 `val` 是两个键：一个是缓存的猜测，一个是「跟着系统走」"
        );
        assert_eq!(row["canonical"], false, "两格不是那十二格，就不能说顺序对");
        assert_eq!(row["theme_name"], "Office Theme");
    }

    #[test]
    fn an_empty_slot_says_so_instead_of_guessing() {
        let row = book("<a:theme xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"><a:themeElements><a:clrScheme><a:dk1/><a:lt1><a:pctFill/></a:lt1></a:clrScheme></a:themeElements></a:theme>");
        assert_eq!(row["empty_slot"], 1);
        assert_eq!(
            row["other_kind"], 1,
            "认不出的选色路子也留个名，不并进 srgbClr"
        );
        assert_eq!(
            row["slots"][0],
            json!({"slot": "dk1", "kind": null, "written": null, "system": null})
        );
        assert_eq!(row["slots"][1]["kind"], "pctFill");
        assert_eq!(row["slots"][1]["written"], Value::Null);
        assert_eq!(row["slot_total"], 2);
    }

    #[test]
    fn blank_typeface_is_not_the_same_as_a_missing_one() {
        let row = book("<a:theme xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"><a:themeElements><a:fontScheme name=\"Office\"><a:majorFont><a:latin typeface=\"\"/><a:ea typeface=\"MS Mincho\"/><a:font script=\"Hans\" typeface=\"微软雅黑\"/></a:majorFont></a:fontScheme></a:themeElements></a:theme>");
        assert_eq!(row["font_roles"], 1, "只有 major 在场就只交一条");
        assert_eq!(
            row["fonts"][0]["latin"], "",
            "写了而空 —— 与没写这一路是两件事"
        );
        assert_eq!(row["fonts"][0]["ea"], "MS Mincho");
        assert_eq!(row["fonts"][0]["cs"], Value::Null);
        assert_eq!(row["fonts"][0]["kids"], json!(["latin", "ea", "font"]));
        assert_eq!(row["fonts"][0]["face_total"], 1);
        assert_eq!(
            row["fonts"][0]["faces"],
            json!([{"script": "Hans", "typeface": "微软雅黑"}]),
            "中文用户问的那一路坐在 a:font 上，不在 ea 上"
        );
    }

    #[test]
    fn the_three_names_in_one_part_answer_separately() {
        let row = book("<a:theme xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" name=\"Office\"><a:themeElements><a:clrScheme name=\"LibreOffice\"/><a:fontScheme/><a:fmtScheme/></a:themeElements><a:objectDefaults/><a:extraClrSchemeLst/></a:theme>");
        assert_eq!(row["theme_name"], "Office");
        assert_eq!(row["scheme_name"], "LibreOffice");
        assert_eq!(
            row["font_name"],
            Value::Null,
            "没写 @name 就交 null，不拿邻居的值顶"
        );
        assert_eq!(row["fmt_name"], Value::Null);
        assert_eq!(
            row["root_children"],
            json!(["themeElements", "objectDefaults", "extraClrSchemeLst"]),
            "根下多出来的两列是生产者的签名，按写的顺序交"
        );
        assert_eq!(row["slot_total"], 0);
        assert_eq!(row["fmt_lists"], 0);
    }

    #[test]
    fn a_part_without_theme_elements_still_opens_a_ledger() {
        let row = book("<a:theme xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" name=\"Office Theme\"><a:thing/></a:theme>");
        assert_eq!(row["unread"], false);
        assert_eq!(row["slot_total"], 0);
        assert_eq!(row["font_roles"], 0);
        assert_eq!(row["fmt_lists"], 0);
        assert_eq!(row["canonical"], false);
        assert_eq!(row["root_children"], json!(["thing"]));
    }

    #[test]
    fn the_format_scheme_lists_are_counted_not_believed() {
        let row = book("<a:theme xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"><a:themeElements><a:fmtScheme name=\"Office\"><a:fillStyleLst><a:solidFill/><a:gradFill/></a:fillStyleLst><a:lnStyleLst><a:solidFill/></a:lnStyleLst></a:fmtScheme></a:themeElements></a:theme>");
        assert_eq!(row["fmt_lists"], 2);
        assert_eq!(
            row["fmt"],
            json!([{"list": "fillStyleLst", "entries": 2},
                   {"list": "lnStyleLst", "entries": 1}]),
            "规范说四列各三个，而这一本数出来的是文件写的"
        );
        assert_eq!(row["fmt_name"], "Office");
    }

    #[test]
    fn the_totals_add_across_parts_and_the_list_gets_cut() {
        let src = "<a:theme xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" name=\"Office Theme\"><a:themeElements><a:clrScheme name=\"Office\"><a:dk1><a:sysClr val=\"windowText\" lastClr=\"000000\"/></a:dk1><a:lt1><a:srgbClr val=\"FFFFFF\"/></a:lt1><a:accent1><a:srgbClr val=\"4F81BD\"/></a:accent1></a:clrScheme><a:fontScheme name=\"Office\"><a:majorFont><a:latin typeface=\"Cambria\"/><a:ea typeface=\"\"/><a:cs typeface=\"\"/></a:majorFont></a:fontScheme><a:fmtScheme><a:fillStyleLst><a:solidFill/></a:fillStyleLst></a:fmtScheme></a:themeElements></a:theme>";
        let mut sum = new_totals();
        for _ in 0..3 {
            one_part("ppt/theme/theme1.xml", src.as_bytes(), &mut sum);
        }
        assert_eq!(
            agg(&json!(sum)),
            json!([3, 0, 9, 3, 6, 0, 0, 0, 3]),
            "三个部件各 3 格：不是那十二格，所以 canon 0 / off_canon 3"
        );
        assert_eq!(sum["slots"], 9);
        assert_eq!(sum["font_roles"], 3);
        assert_eq!(sum["latin_written"], 3);
        assert_eq!(sum["ea_blank"], 3);
        assert_eq!(sum["cs_blank"], 3);
        assert_eq!(sum["latin_missing"], 0, "declared 的零不是缺键");
        assert_eq!(sum["roles_without_faces"], 3);
        assert_eq!(sum["fmt_lists"], 3);
        assert_eq!(sum["fmt_unnamed"], 3);
        assert_eq!(sum["extra_clr_scheme"], 0);
        assert_eq!(sum["object_defaults"], 0);
        assert_eq!(sum["by_theme_name"], json!({"Office Theme": 3}));
        assert_eq!(sum["by_scheme_name"], json!({"Office": 3}));
    }

    #[test]
    fn a_package_that_is_not_a_zip_answers_with_a_zero_book() {
        let ledger = themes(b"not a zip at all", 8);
        assert_eq!(ledger["total"], 0);
        assert_eq!(ledger["parts"], json!([]));
        assert_eq!(ledger["cut"], false);
        assert_eq!(agg(&ledger["totals"]), json!([0, 0, 0, 0, 0, 0, 0, 0, 0]));
        assert_eq!(ledger["totals"]["by_theme_name"], json!({}));
        assert_eq!(ledger["totals"]["unread"], 0);
    }

    fn fixture(name: &str) -> Vec<u8> {
        std::fs::read(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/office")
                .join(name),
        )
        .expect("读 fixture")
    }

    /// 从一本账里挑出某个色格那一行（按 `slot` 的名字，文档顺序的第一个）
    fn slot(ledger: &Value, which: &str) -> Value {
        ledger["parts"][0]["slots"]
            .as_array()
            .and_then(|rows| {
                rows.iter()
                    .find(|one| one.get("slot").and_then(Value::as_str) == Some(which))
                    .cloned()
            })
            .expect("这一格在场")
    }

    /// 从一本账里挑出 `a:font script=` 那一批里的某一条
    fn face(ledger: &Value, role: usize, script: &str) -> Value {
        ledger["parts"][0]["fonts"][role]["faces"]
            .as_array()
            .and_then(|rows| {
                rows.iter()
                    .find(|one| one.get("script").and_then(Value::as_str) == Some(script))
                    .cloned()
            })
            .expect("这一条字在场")
    }

    #[test]
    fn a_word_producer_writes_both_slot_spellings() {
        let had = themes(&fixture("bkmks.docx"), 400);
        assert_eq!(agg(&had["totals"]), json!([1, 0, 12, 2, 10, 0, 0, 1, 0]));
        assert_eq!(had["total"], 1);
        assert_eq!(had["cut"], false);
        assert_eq!(had["parts"][0]["part"], "word/theme/theme1.xml");
        assert_eq!(
            had["parts"][0]["root_children"],
            json!(["themeElements", "objectDefaults", "extraClrSchemeLst"]),
            "`objectDefaults` 与 `extraClrSchemeLst` 是这一路的两个记号，实测与 dk1 用 sysClr 同进同出"
        );
        assert_eq!(
            had["parts"][0]["fmt_name"], "Office",
            "这一路四个 scheme 都点名"
        );
        assert_eq!(
            slot(&had, "dk1"),
            json!({"slot": "dk1", "kind": "sysClr", "written": "000000", "system": "windowText"})
        );
        assert_eq!(slot(&had, "accent1")["written"], "4F81BD");
        // 中文那一路的字坐在 `a:font script=` 那批里，而不是 `ea` 槽（这里 `ea` 是空串）
        assert_eq!(face(&had, 0, "Hans")["typeface"], "宋体");
        assert_eq!(
            had["parts"][0]["fonts"][0]["ea"], "",
            "写了，但是空的：这一路没选脸"
        );
        assert_eq!(had["totals"]["faces"], 60);
        assert_eq!(had["totals"]["roles_without_faces"], 0);
    }

    #[test]
    fn the_other_producer_rewrites_the_two_slots_and_drops_two_names() {
        let had = themes(&fixture("bkmks-lo.docx"), 400);
        assert_eq!(agg(&had["totals"]), json!([1, 0, 12, 0, 12, 0, 0, 1, 0]));
        assert_eq!(had["parts"][0]["root_children"], json!(["themeElements"]));
        assert_eq!(
            had["parts"][0]["fmt_name"],
            Value::Null,
            "重写那一本不点格式化方案的名"
        );
        assert_eq!(
            slot(&had, "dk1"),
            json!({"slot": "dk1", "kind": "srgbClr", "written": "000000", "system": Value::Null}),
            "同一个黑色换了写法：`system` 那一格随之没有 —— 「跟着系统走」那一句被写成了字面色"
        );
        assert_eq!(
            slot(&had, "accent1")["written"],
            "4F81BD",
            "重写的不是这一格，原样留着"
        );
        // 这一本改的是颜色那一路，字体那一批照搬：60 条 `a:font` 与两个空串槽都还在
        assert_eq!(had["totals"]["faces"], 60);
        assert_eq!(had["totals"]["ea_blank"], 2);
        assert_eq!(had["totals"]["extra_clr_scheme"], 0);
        assert_eq!(had["totals"]["object_defaults"], 0);
        assert_eq!(had["totals"]["fmt_unnamed"], 1);
    }

    #[test]
    fn a_rebuilt_deck_writes_dejaVu_and_no_font_list() {
        let had = themes(&fixture("deck-lo.pptx"), 400);
        assert_eq!(had["total"], 12, "一个母版一个主题部件，这一份有 12 个");
        assert_eq!(had["listed"], 12);
        assert_eq!(had["cut"], false);
        assert_eq!(
            agg(&had["totals"]),
            json!([12, 0, 144, 0, 144, 0, 0, 12, 0])
        );
        assert_eq!(had["totals"]["faces"], 0, "重打的那一批不写 `a:font`");
        assert_eq!(had["totals"]["roles_without_faces"], 24);
        assert_eq!(had["totals"]["ea_written"], 24);
        assert_eq!(
            had["totals"]["ea_blank"], 0,
            "空串与写了 DejaVu Sans 是两本账"
        );
        assert_eq!(had["parts"][0]["fonts"][0]["ea"], "DejaVu Sans");
        assert_eq!(
            had["parts"][0]["fonts"][0]["kids"],
            json!(["latin", "ea", "cs"])
        );
        assert_eq!(
            had["totals"]["by_scheme_name"],
            json!({"Office": 11, "LibreOffice": 1}),
            "三个 `@name` 各说各的：这一份只有一个 clrScheme 改了名"
        );
        assert_eq!(had["totals"]["by_theme_name"], json!({"Office Theme": 12}));
    }

    #[test]
    fn a_limit_cuts_the_list_but_not_the_arithmetic() {
        let had = themes(&fixture("deck-lo.pptx"), 5);
        assert_eq!(had["total"], 12);
        assert_eq!(had["listed"], 5);
        assert_eq!(had["cut"], true);
        assert_eq!(
            had["totals"]["theme_parts"], 12,
            "合计是整份的账，不是清单那几本的账"
        );
        assert_eq!(had["totals"]["slots"], 144);
        assert_eq!(
            had["totals"]["by_scheme_name"],
            json!({"Office": 11, "LibreOffice": 1})
        );
    }

    #[test]
    fn the_sheet_family_uses_the_same_slots_and_odf_has_none() {
        let had = themes(&fixture("book.xlsx"), 400);
        assert_eq!(had["parts"][0]["part"], "xl/theme/theme1.xml");
        assert_eq!(agg(&had["totals"]), json!([1, 0, 12, 2, 10, 0, 0, 1, 0]));
        assert_eq!(slot(&had, "lt1")["system"], "window");
        // ODF 三家没有主题这个概念：交一本零条的账，而不是缺这个键
        for name in ["bkmks.odt", "book.ods", "deck.odp"] {
            let none = themes(&fixture(name), 400);
            assert_eq!(none["total"], 0, "{} 里没有主题部件", name);
            assert_eq!(none["parts"], json!([]));
            assert_eq!(agg(&none["totals"]), json!([0, 0, 0, 0, 0, 0, 0, 0, 0]));
            assert_eq!(none["totals"]["by_theme_name"], json!({}));
        }
    }
}
