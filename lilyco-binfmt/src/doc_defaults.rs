//! 「这份文档没写样式的字长什么样」——OOXML 把这句话写在 `word/styles.xml` 的 `<w:docDefaults>` 里，
//! 固定两层：`<w:rPrDefault>` 里一个 `<w:rPr>`、`<w:pPrDefault>` 里一个 `<w:pPr>`。
//! ODF 不写这一格：同一问摊在 `styles.xml` 的 `<style:default-style style:family="…">` 上，**一族一条**。
//!
//! 实测（数法：72 份 word 件 = 71 份 .docx + 1 份 .docm；67 份 odf = 41 份 .odt + 14 份 .ods + 12 份 .odp）：
//! 1. **72 份全有且恰好一块**：`word/styles.xml` 里 `docDefaults` 恒一枚、孩子恒
//!    `rPrDefault + pPrDefault` 这两条（孩子数没有一份是 1 或 3），而 39 份 .xlsx 与 21 份 .pptx
//!    一份都没有 —— 所以这一本只在 office-doc 交；
//! 2. **34 份在第二个部件又写了一遍**：`word/stylesWithEffects.xml`（生产者是
//!    `docProps/app.xml` 写着 Microsoft Macintosh Word 的那一家）里第二块与第一块的孩子名、
//!    属性、序**完全一致**，只差原始文本的缩进（372 对 468 字节）。所以账本交「几块、各在哪个部件、
//!    每块说了什么」，而不是只交第一块然后宣称其余的一样；
//! 3. `rPr` 三种形状（都按写的序 `rFonts, sz, szCs, lang` 打头，`lang` 在最后）：66 份就这四条、
//!    4 份在 `rFonts` 后多插一条 `kern`、2 份多插一条 `color`；`sz` 与 `szCs` 在 72 份里**恒等**
//!    （22 的 66 份、24 的 6 份）但两枚分开交；`w:lang` 只有两组属性
//!    （66 份 `en-US/en-US/ar-SA` 对 6 份 `en-US/zh-CN/hi-IN`）；
//! 4. `rFonts` 的属性名实测只有三种组合：34 份只写四条主题指针
//!    （`asciiTheme/eastAsiaTheme/hAnsiTheme/cstheme` —— 最后一条是**小写**开头的 `cstheme`，
//!    按前缀 `theme` 认会一条不中，所以按结尾认）、28 份主题与字面名两套都写、
//!    10 份只写 `ascii/eastAsia/hAnsi/cs`；而且**字面名可以是空串**（`w:cs=""` 在 27 份里在场），
//!    所以「写了这个属性」与「写了个字体名」是两件事，账本两列各交各的；
//! 5. 第二层在 `<w:style w:styleId="Normal">`：Word 那 34 份的 Normal **一个 `rPr`/`pPr` 都不写**
//!    （全靠 docDefaults），而 LibreOffice 的 38 份两个都写、并把上面那套**摊平**进去
//!    （`rPr` 六条 rFonts/color/kern/sz/szCs/lang 的 36 份，另有 2 份只写 sz+lang；
//!    `pPr` 五种种形）。同一句话在不同生产者手里住在不同的格子里，所以两层的账都得交；
//! 6. ODF 那一本按文件种类给三种数：.odt 恒四条（写的序 graphic / paragraph / table / table-row，
//!    39 份）、.ods 恒两条（写的序 table-cell / graphic，14 份）、.odp 一条（graphic，11 份），
//!    另有 3 份（`pnum.odt` / `tbox.odt` / `eqs.odp`）**有 `styles.xml` 而一条 default-style 都不写** ——
//!    「零条」与「没有这个部件」是两件事，两列分开交；
//! 7. ODF 的属性住在孩子的孩子上（`style:text-properties` 等）。**字体名、字号、语言各分三格**
//!    （`latin` / `asian` / `complex`，按局部名 `font-name`、`font-size`、`language` 收），
//!    和 docx 的 `rFonts` 四条与 `w:lang` 三条是同一句话的两种写法。67 份共 195 条 default-style，
//!    其中 117 条带 `text-properties`：**字号与语言三格全写满**（各 117 条），
//!    **字体名却只写 103 / 102 / 103 条** —— 差额 14 / 15 / 14 正是「只写号不写名」的条数
//!    （.ods 的 graphic 一族 14 条一个名字都不写，`book.ods` 就是其一）；
//!    语言实测 latin 116 条 `en/US`、asian `en/US` 72 对 `zh/CN` 44、
//!    complex `ar/SA` 72 对 `hi/IN` 44。所以三格各交各的，写名与写号两本分交、不互推。
//!    段落那一族写 `style:font-name`（odt 36 份 Cambria1 / 3 份 Liberation Serif）与 `fo:font-size`（36 份 11pt / 3 份 12pt）。
//!    那串连字符设置（十三条名字一组）**只出现在 paragraph 那一族**（67 份里 38 份写；
//!    `tbox-lo.odt` 有 paragraph 一条却一个都不写），table 一族只写 `table:border-model`、
//!    table-row 一族只写 `fo:keep-together`；67 份的 content.xml 里 `default-style` 出现 **0 次**
//!    —— 这一格只走 styles.xml 那一跳，但计数两列都留着（断在另一头也要数得出）。
//!
//! 另外两族不交这个键（缺键 = 这一族没这一层）：RTF 把默认值混在 `{\s0 …}` 那一条 Normal 样式里，
//! 归属判不住就不报；遗留 `.doc` 的默认值在 styles heap 里，本族料的 .doc 全出自 LibreOffice，
//! 没有第二个读者能核对，就不照一个没核过的读法写。

use crate::xmlscan::{self, Node};
use crate::zipread::{self, DEFAULT_MEMBER_CAP};
use serde_json::{json, Value};

/// 那四条常项：`extras` 交的是「除这四条以外还写了什么」，不是「一共写了什么」
const RPR_CANON: [&str; 4] = ["lang", "rFonts", "sz", "szCs"];
/// 字面字体名那四个属性名（按写的样子比，实测只有这一种写法）
const FONT_LITERAL: [&str; 4] = ["ascii", "eastAsia", "hAnsi", "cs"];
/// ODF 那一层的三个槽：`(槽名, 字体名属性, 字号属性)`，都按局部名比
const ODF_FONT_SLOTS: [(&str, &str, &str); 3] = [
    ("latin", "font-name", "font-size"),
    ("asian", "font-name-asian", "font-size-asian"),
    ("complex", "font-name-complex", "font-size-complex"),
];
/// 语言那三个槽：`(槽名, language 属性, country 属性)`
const ODF_LANG_SLOTS: [(&str, &str, &str); 3] = [
    ("latin", "language", "country"),
    ("asian", "language-asian", "country-asian"),
    ("complex", "language-complex", "country-complex"),
];

/// 一个性质元素：名字（局部名）+ 它的属性表（局部名）
struct Prop {
    name: String,
    attrs: serde_json::Map<String, Value>,
}

/// 只数元素孩子：容错解析会把「两个标签之间的一个空格」摊成一枚 `#text`，
/// 而 ElementTree 遍历孩子只给元素 —— 两家对同一份件得数出同一个孩子数
fn element_kids(node: &Node) -> Vec<&Node> {
    node.children
        .iter()
        .filter(|one| one.name != "#text")
        .collect()
}

fn has_elements(node: Option<&Node>) -> bool {
    match node {
        Some(one) => one.children.iter().any(|kid| kid.name != "#text"),
        None => false,
    }
}

/// 「这一份件里到底有没有一个元素」：一个尖括号都没有的部件在 Python 那本是 `ParseError`
/// （解不开、按部件不在处理），在容错解析下只剩一枚 `#text` —— 两家按同一个判决
fn has_element(parsed: &Option<Node>) -> bool {
    has_elements(parsed.as_ref())
}

fn member_doc(bytes: &[u8], part: &str) -> Option<Node> {
    zipread::member(bytes, part, DEFAULT_MEMBER_CAP)
        .ok()
        .map(|one| xmlscan::parse_str(&one.as_text()))
}

/// 一枚元素的属性表（局部名；`xmlns` 那类声明不算属性 —— 标准库的 XML 读者也不放进 attrib）
fn attrs_of(node: &Node) -> serde_json::Map<String, Value> {
    let mut out = serde_json::Map::new();
    for (key, value) in node.attrs.iter() {
        if key == "xmlns" || key.starts_with("xmlns:") {
            continue;
        }
        let local = key.rsplit(':').next().unwrap_or(key).to_string();
        out.insert(local, json!(value));
    }
    out
}

/// 第一个局部名等于 `name` 的直接元素孩子；没有就 None（与「找到了一个空元素」是两件事）
fn direct_kid<'a>(node: Option<&'a Node>, name: &str) -> Option<&'a Node> {
    match node {
        Some(one) => element_kids(one)
            .into_iter()
            .find(|kid| kid.local() == name),
        None => None,
    }
}

/// `<w:rPrDefault><w:rPr>` 那一跳：名字写死，两跳都可能断
fn wrapper_kid<'a>(block: Option<&'a Node>, wrapper: &str, inner: &str) -> Option<&'a Node> {
    direct_kid(direct_kid(block, wrapper), inner)
}

fn child_names(node: &Node) -> Vec<String> {
    element_kids(node)
        .iter()
        .map(|one| one.local().to_string())
        .collect()
}

/// 一个孩子一族性质元素：摊成 `Prop` 表（文档序，重复名字留着）
fn props(node: Option<&Node>) -> Vec<Prop> {
    match node {
        Some(one) => element_kids(one)
            .into_iter()
            .map(|kid| Prop {
                name: kid.local().to_string(),
                attrs: attrs_of(kid),
            })
            .collect(),
        None => Vec::new(),
    }
}

/// 去重后的名字表（文档序）
fn prop_names(rows: &[Prop]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for one in rows.iter() {
        if !out.iter().any(|had| *had == one.name) {
            out.push(one.name.clone());
        }
    }
    out
}

fn props_json(rows: &[Prop], limit: usize) -> Vec<Value> {
    rows.iter()
        .take(limit)
        .map(|one| json!({"name": one.name.clone(), "attrs": Value::Object(one.attrs.clone())}))
        .collect()
}

/// 「第一条说了算」：同一份件里同一类性质写了两遍时不合并、不覆盖（与 Python 那本同一口径）
fn first_prop<'a>(rows: &'a [Prop], name: &str) -> Option<&'a Prop> {
    rows.iter().find(|one| one.name == name)
}

fn first_attr(rows: &[Prop], name: &str, key: &str) -> Value {
    match first_prop(rows, name).and_then(|one| one.attrs.get(key)) {
        Some(had) => had.clone(),
        None => Value::Null,
    }
}

fn attr_of(map: &serde_json::Map<String, Value>, key: &str) -> Value {
    match map.get(key) {
        Some(had) => had.clone(),
        None => Value::Null,
    }
}

/// OOXML 那一份：docDefaults 那一块（两块时取主的那一块）摊成两本，另加 Normal 样式那一层
pub(crate) fn docx(bytes: &[u8], limit: usize) -> Value {
    let styles = member_doc(bytes, "word/styles.xml");
    let effects = member_doc(bytes, "word/stylesWithEffects.xml");
    // 部件序写死：styles.xml 在前，所以 blocks[0] 就是「主的那一块」
    let mut blocks: Vec<(&str, &Node)> = Vec::new();
    for (part, parsed) in [
        ("word/styles.xml", &styles),
        ("word/stylesWithEffects.xml", &effects),
    ] {
        let doc = match parsed.as_ref() {
            Some(one) if has_elements(Some(one)) => Some(one),
            _ => None,
        };
        let doc = match doc {
            Some(one) => one,
            None => continue,
        };
        for one in doc.descendants("docDefaults") {
            blocks.push((part, one));
        }
    }
    let mut shapes: Vec<Value> = Vec::new();
    for (part, one) in blocks.iter() {
        let rrows = props(wrapper_kid(Some(*one), "rPrDefault", "rPr"));
        let prows = props(wrapper_kid(Some(*one), "pPrDefault", "pPr"));
        shapes.push(json!({
            "part": part.to_string(),
            "children": child_names(*one),
            "rpr_names": prop_names(&rrows),
            "ppr_names": prop_names(&prows),
        }));
    }
    let first: Option<&Node> = match blocks.first() {
        Some((_, one)) => Some(*one),
        None => None,
    };
    let rpr_rows = props(wrapper_kid(first, "rPrDefault", "rPr"));
    let ppr_rows = props(wrapper_kid(first, "pPrDefault", "pPr"));
    let font_attrs = match first_prop(&rpr_rows, "rFonts") {
        Some(one) => one.attrs.clone(),
        None => serde_json::Map::new(),
    };
    let mut parts_seen: Vec<String> = Vec::new();
    for (part, _) in blocks.iter() {
        let had = part.to_string();
        if !parts_seen.iter().any(|one| *one == had) {
            parts_seen.push(had);
        }
    }
    json!({
        "family": "ooxml",
        "available": true,
        "styles_part": has_element(&styles),
        "styles_effects_part": has_element(&effects),
        "parts_with_block": parts_seen,
        "blocks_total": blocks.len(),
        "block_shapes": shapes.into_iter().take(limit).collect::<Vec<Value>>(),
        "children_total": match first {
            Some(one) => child_names(one).len(),
            None => 0usize,
        },
        "rpr_names": prop_names(&rpr_rows),
        "ppr_names": prop_names(&ppr_rows),
        "rpr_rows": props_json(&rpr_rows, limit),
        "ppr_rows": props_json(&ppr_rows, limit),
        "wrote_theme": font_attrs
            .keys()
            .any(|one| one.to_lowercase().ends_with("theme")),
        "wrote_literal": font_attrs
            .keys()
            .any(|one| FONT_LITERAL.iter().any(|had| *had == *one)),
        "font_ascii_written": attr_of(&font_attrs, "ascii"),
        "font_ascii_theme": attr_of(&font_attrs, "asciiTheme"),
        // 属性在场、值却是空串：实测 LibreOffice 有 27 份这样写 `w:cs=""`，
        // 也就是「写了这个属性」不等于「点了字体名」
        "font_blank_attrs": FONT_LITERAL
            .iter()
            .copied()
            .filter(|one| {
                matches!(font_attrs.get(*one), Some(Value::String(had)) if had.is_empty())
            })
            .map(|one| one.to_string())
            .collect::<Vec<String>>(),
        "size_written": first_attr(&rpr_rows, "sz", "val"),
        "size_cs_written": first_attr(&rpr_rows, "szCs", "val"),
        "lang_written": match first_prop(&rpr_rows, "lang") {
            Some(one) => Value::Object(one.attrs.clone()),
            None => Value::Null,
        },
        "extras": prop_names(&rpr_rows)
            .iter()
            .filter(|one| !RPR_CANON.iter().any(|had| *had == **one))
            .cloned()
            .collect::<Vec<String>>(),
        "normal_style": normal_style(styles.as_ref(), limit),
    })
}

/// `<w:style w:styleId="Normal" w:type="paragraph">` 那一层：Word 不写、LibreOffice 摊平进来。
/// 两个属性都得对上 —— 只点 `styleId` 会撞上字符样式那一族（真件里字符样式也叫 Normal）
fn normal_style(styles: Option<&Node>, limit: usize) -> Value {
    let quiet = json!({"found": false, "children": [], "rpr_rows": [], "ppr_rows": []});
    let doc = match styles {
        Some(one) if has_elements(Some(one)) => one,
        _ => return quiet,
    };
    let hit = doc.descendants("style").into_iter().find(|one| {
        let got = attrs_of(one);
        match (got.get("styleId"), got.get("type")) {
            (Some(id), Some(kind)) => {
                id.as_str() == Some("Normal") && kind.as_str() == Some("paragraph")
            }
            _ => false,
        }
    });
    let node = match hit {
        Some(one) => one,
        None => return quiet,
    };
    // Normal 这一条直接把 rPr / pPr 挂在 style 上，没有 docDefaults 那层包装；
    // 取出来之后仍按「性质元素 + 属性表」那一本交，与上面同一口径
    json!({
        "found": true,
        "children": child_names(node),
        "rpr_rows": props_json(&props(direct_kid(Some(node), "rPr")), limit),
        "ppr_rows": props_json(&props(direct_kid(Some(node), "pPr")), limit),
    })
}

/// 一行的合并属性表（按孩子的序、同一个局部名**先到先得**）与孩子身上属性的总条数。
/// 实测 67 份 odf 里同一行的孩子之间没有撞名，所以这条规矩只在合成件里看得见
fn merged_props(node: &Node) -> (Vec<(String, String)>, usize) {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut total = 0usize;
    for kid in element_kids(node).iter() {
        let got = attrs_of(kid);
        total += got.len();
        for (key, value) in got.iter() {
            if out.iter().any(|(had, _)| *had == *key) {
                continue;
            }
            out.push((key.clone(), value.as_str().unwrap_or("").to_string()));
        }
    }
    (out, total)
}

fn merged_get(props: &[(String, String)], name: &str) -> Value {
    match props.iter().find(|(had, _)| had == name) {
        Some((_, value)) => json!(value),
        None => Value::Null,
    }
}

/// 属性在不在。与「值是不是空串」是两件事，所以 ODF 的字族槽要用这一条而不是 `merged_get`：
/// 实测 .ods 的 graphic 行有 14 条一个字体名都不写，写与不写不能靠 `null` 分辨
fn merged_has(props: &[(String, String)], name: &str) -> bool {
    props.iter().any(|(had, _)| had.as_str() == name)
}

/// ODF 那一本：`style:default-style` 一族一条，属性住在孩子的孩子上
pub(crate) fn odf(bytes: &[u8], limit: usize) -> Value {
    let content = member_doc(bytes, "content.xml");
    let styles = member_doc(bytes, "styles.xml");
    // 部件序与 Python 那本一致：content.xml 在前（实测 67 份里它一条 default-style 都不写）
    let mut rows: Vec<Value> = Vec::new();
    let mut fonts: Vec<Value> = Vec::new();
    let mut sizes: Vec<Value> = Vec::new();
    let mut langs: Vec<Value> = Vec::new();
    let mut hyphen: Vec<Value> = Vec::new();
    let mut families: Vec<String> = Vec::new();
    let mut styles_total = 0usize;
    let mut content_total = 0usize;
    for (part, parsed) in [("content.xml", &content), ("styles.xml", &styles)] {
        let doc = match parsed.as_ref() {
            Some(one) if has_elements(Some(one)) => Some(one),
            _ => None,
        };
        let doc = match doc {
            Some(one) => one,
            None => continue,
        };
        for one in doc.descendants("default-style") {
            let (found, total) = merged_props(one);
            let family = match one.attr_local("family") {
                Some(had) => had.to_string(),
                None => String::new(),
            };
            rows.push(json!({
                "part": part.to_string(),
                "family": family.clone(),
                "children": child_names(one),
                "props_attrs_total": total,
                "font_name": merged_get(&found, "font-name"),
                "font_name_asian": merged_get(&found, "font-name-asian"),
                "font_name_complex": merged_get(&found, "font-name-complex"),
                "font_size": merged_get(&found, "font-size"),
                "font_size_asian": merged_get(&found, "font-size-asian"),
                "font_size_complex": merged_get(&found, "font-size-complex"),
                "language": merged_get(&found, "language"),
                "country": merged_get(&found, "country"),
                "language_asian": merged_get(&found, "language-asian"),
                "country_asian": merged_get(&found, "country-asian"),
                "language_complex": merged_get(&found, "language-complex"),
                "country_complex": merged_get(&found, "country-complex"),
            }));
            // 三槽各数各的：实测每一槽都可以只写大小、不写名字，所以两本分交
            for &(slot, name_attr, size_attr) in ODF_FONT_SLOTS.iter() {
                if merged_has(&found, name_attr) {
                    fonts.push(json!({"part": part.to_string(), "family": family.clone(),
                                      "slot": slot, "value": merged_get(&found, name_attr)}));
                }
                if merged_has(&found, size_attr) {
                    sizes.push(json!({"part": part.to_string(), "family": family.clone(),
                                      "slot": slot, "value": merged_get(&found, size_attr)}));
                }
            }
            for &(slot, lang_attr, country_attr) in ODF_LANG_SLOTS.iter() {
                if merged_has(&found, lang_attr) || merged_has(&found, country_attr) {
                    langs.push(json!({"part": part.to_string(), "family": family.clone(),
                                      "slot": slot,
                                      "language": merged_get(&found, lang_attr),
                                      "country": merged_get(&found, country_attr)}));
                }
            }
            let mut keys: Vec<String> = found.iter().map(|(key, _)| key.clone()).collect();
            keys.sort();
            for key in keys.iter() {
                if !key.to_lowercase().contains("hyphen") {
                    continue;
                }
                let value = match found.iter().find(|(had, _)| had == key) {
                    Some((_, value)) => value.clone(),
                    None => String::new(),
                };
                hyphen.push(json!({"part": part.to_string(), "family": family.clone(),
                                   "name": key.clone(), "value": value}));
            }
            if !family.is_empty() && !families.iter().any(|had| *had == family) {
                families.push(family);
            }
            if part == "styles.xml" {
                styles_total += 1;
            } else {
                content_total += 1;
            }
        }
    }
    families.sort();
    families.dedup();
    let mut hyphen_names: Vec<String> = Vec::new();
    for one in hyphen.iter() {
        let name = match one["name"].as_str() {
            Some(had) => had.to_string(),
            None => String::new(),
        };
        if !hyphen_names.iter().any(|had| *had == name) {
            hyphen_names.push(name);
        }
    }
    json!({
        "family": "odf",
        "available": true,
        "styles_part": has_element(&styles),
        "defaults_total": styles_total,
        "defaults_in_content": content_total,
        "families": families,
        "rows": rows.into_iter().take(limit).collect::<Vec<Value>>(),
        "fonts_written": fonts.into_iter().take(limit).collect::<Vec<Value>>(),
        "sizes_written": sizes.into_iter().take(limit).collect::<Vec<Value>>(),
        "langs_written": langs.into_iter().take(limit).collect::<Vec<Value>>(),
        "hyphenation_names": hyphen_names,
        "hyphenation_rows": hyphen.into_iter().take(limit).collect::<Vec<Value>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 自己打一个「存储」（不压缩）的包：这一本要的几种形状（同一格写两遍、
    /// content.xml 里也写 default-style、两个孩子撞同一个局部名）在 215 份真件里一个都没有
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

    /// 一份只有 `<w:docDefaults>` 与 Normal 样式的 styles.xml
    fn docx_with(defaults: &str, normal: &str) -> Value {
        let body = format!(
            "<w:styles {W}>{defaults}<w:style w:type=\"paragraph\" w:styleId=\"Normal\">{normal}\
             </w:style></w:styles>",
            W = W_ATTR,
            defaults = defaults,
            normal = normal
        );
        docx(
            &packed(&[
                ("word/document.xml", "<w:document/>"),
                ("word/styles.xml", body.as_str()),
            ]),
            100,
        )
    }

    /// 两跳都可能断：没有 `pPrDefault` 不等于「段落默认值是空的」，
    /// 而 `w:rFonts` 一个属性都没写也算「写了这个元素」
    #[test]
    fn a_missing_wrapper_is_not_an_empty_one() {
        let mine = docx_with(
            "<w:rPrDefault><w:rPr><w:rFonts/><w:sz w:val=\"20\"/></w:rPr></w:rPrDefault>",
            "",
        );
        assert_eq!(mine["children_total"], json!(1));
        assert_eq!(mine["blocks_total"], json!(1));
        assert_eq!(mine["rpr_names"], json!(["rFonts", "sz"]));
        // pPrDefault 整条没有：ppr_names 是空表，而不是 null 也不是「默认值」
        assert_eq!(mine["ppr_names"], json!([]));
        assert_eq!(mine["ppr_rows"], json!([]));
        assert_eq!(mine["size_written"], json!("20"));
        assert_eq!(mine["size_cs_written"], Value::Null);
        // `w:rFonts` 在场而一个属性都没写：两列指针都是 false，两列取值都是 null
        assert_eq!(mine["wrote_theme"], json!(false));
        assert_eq!(mine["wrote_literal"], json!(false));
        assert_eq!(mine["font_ascii_written"], Value::Null);
        assert_eq!(mine["lang_written"], Value::Null);
        assert_eq!(mine["extras"], json!([]));
        assert_eq!(mine["normal_style"]["found"], json!(true));
        assert_eq!(mine["normal_style"]["children"], json!([]));
    }

    /// 同一格写了两遍（第二块在另一个部件）：数出来两遍、两块各记各的账，取值取主的那一块
    #[test]
    fn a_second_block_in_the_other_part_is_counted_not_blended() {
        let body = format!(
            "<w:styles {W}><w:docDefaults><w:rPrDefault><w:rPr><w:sz w:val=\"22\"/></w:rPr>\
             </w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after=\"200\"/></w:pPr>\
             </w:pPrDefault></w:docDefaults></w:styles>",
            W = W_ATTR
        );
        let other = body.replace(
            "<w:sz w:val=\"22\"/>",
            "<w:sz w:val=\"28\"/><w:kern w:val=\"2\"/>",
        );
        let mine = docx(
            &packed(&[
                ("word/document.xml", "<w:document/>"),
                ("word/styles.xml", body.as_str()),
                ("word/stylesWithEffects.xml", other.as_str()),
            ]),
            100,
        );
        assert_eq!(mine["styles_effects_part"], json!(true));
        assert_eq!(mine["blocks_total"], json!(2));
        assert_eq!(
            mine["parts_with_block"],
            json!(["word/styles.xml", "word/stylesWithEffects.xml"])
        );
        // 第一块说话算（不合并、不被第二块覆盖）
        assert_eq!(mine["size_written"], json!("22"));
        assert_eq!(mine["extras"], json!([]));
        assert_eq!(mine["block_shapes"][0]["rpr_names"], json!(["sz"]));
        assert_eq!(mine["block_shapes"][1]["rpr_names"], json!(["sz", "kern"]));
        assert_eq!(
            mine["block_shapes"][1]["part"],
            json!("word/stylesWithEffects.xml")
        );
    }

    /// 主题指针按**结尾**认（`cstheme` 是小写开头那一种），字面名按整名认；
    /// 而写了空串与没写是两件事
    #[test]
    fn theme_pointers_are_recognised_by_their_tail() {
        let mine = docx_with(
            "<w:rPrDefault><w:rPr><w:rFonts w:cstheme=\"minorBidi\" w:cs=\"\"/>\
             <w:sz w:val=\"24\"/></w:rPr></w:rPrDefault><w:pPrDefault/>",
            "",
        );
        assert_eq!(mine["wrote_theme"], json!(true));
        assert_eq!(mine["wrote_literal"], json!(true));
        assert_eq!(mine["font_ascii_written"], Value::Null);
        assert_eq!(mine["rpr_rows"][0]["attrs"]["cs"], json!(""));
        // `<w:pPrDefault/>` 是「有这一条、里面没写东西」，不是「没有这一条」
        assert_eq!(mine["children_total"], json!(2));
        assert_eq!(mine["ppr_names"], json!([]));
    }

    /// Normal 那一层要两个属性都点上：`styleId="Normal"` 而 `type` 是 character 的那一条不算
    #[test]
    fn the_normal_style_needs_both_name_and_type() {
        let body = format!(
            "<w:styles {W}><w:style w:type=\"character\" w:styleId=\"Normal\">\
             <w:rPr><w:sz w:val=\"18\"/></w:rPr></w:style></w:styles>",
            W = W_ATTR
        );
        let mine = docx(
            &packed(&[
                ("word/document.xml", "<w:document/>"),
                ("word/styles.xml", body.as_str()),
            ]),
            100,
        );
        assert_eq!(mine["normal_style"]["found"], json!(false));
        assert_eq!(mine["blocks_total"], json!(0));
        assert_eq!(mine["children_total"], json!(0));
        assert_eq!(mine["parts_with_block"], json!([]));
        assert_eq!(mine["block_shapes"], json!([]));
    }

    const ODF_ATTR: &str = "xmlns:style=\"urn:oasis:names:tc:opendocument:xmlns:style:1.0\" \
                            xmlns:fo=\"urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0\"";

    /// 一份只有 default-style 的 styles.xml
    fn odf_with(rows: &str) -> Value {
        let body = format!(
            "<office:document-styles {}>{}</office:document-styles>",
            ODF_ATTR, rows
        );
        odf(
            &packed(&[
                ("content.xml", "<office:text/>"),
                ("styles.xml", body.as_str()),
            ]),
            100,
        )
    }

    /// 一族一条：`families` 是排过序的名字表，而 `rows` 保持**写的序**（实测 .ods 是 table-cell 在前）
    #[test]
    fn one_row_per_family_keeps_the_written_order() {
        let mine = odf_with(
            "<style:default-style style:family=\"table-cell\">\
             <style:table-cell-properties fo:padding=\"0.2cm\"/></style:default-style>\
             <style:default-style style:family=\"graphic\">\
             <style:graphic-properties/><style:text-properties style:font-name=\"Cambria1\" \
             fo:font-size=\"12pt\" fo:language=\"en\" fo:country=\"US\"/>\
             </style:default-style>",
        );
        assert_eq!(mine["defaults_total"], json!(2));
        assert_eq!(mine["defaults_in_content"], json!(0));
        assert_eq!(mine["families"], json!(["graphic", "table-cell"]));
        assert_eq!(mine["rows"][0]["family"], json!("table-cell"));
        assert_eq!(mine["rows"][1]["family"], json!("graphic"));
        assert_eq!(mine["rows"][0]["font_name"], Value::Null);
        // 只写大小不写名字的那一族（实测 book.ods 的 graphic 一条就是这样）：两本各一条
        assert_eq!(
            mine["fonts_written"].as_array().map(|one| one.len()),
            Some(1)
        );
        assert_eq!(
            mine["sizes_written"].as_array().map(|one| one.len()),
            Some(1)
        );
        assert_eq!(mine["langs_written"][0]["country"], json!("US"));
        assert_eq!(mine["hyphenation_rows"], json!([]));
    }

    /// 三格各交各的：`latin` 写了名字，`asian` 只写号，`complex` 只写语言 ——
    /// 实测 67 份里字号与语言三格各 117 条写满，字体名却是 103 / 102 / 103，
    /// 差额 14 / 15 / 14 就是这种「只有号没有名」的条数
    #[test]
    fn a_size_can_be_written_without_a_font_name() {
        let mine = odf_with(
            "<style:default-style style:family=\"paragraph\">\
             <style:text-properties style:font-name=\"Cambria1\" fo:font-size=\"11pt\" \
             fo:font-size-asian=\"10.5pt\" style:font-name-complex=\"F\" fo:font-size-complex=\"11pt\" \
             fo:language=\"en\" fo:country=\"US\" fo:language-complex=\"ar\" fo:country-complex=\"SA\"/>\
             </style:default-style>",
        );
        let row = &mine["rows"][0];
        assert_eq!(row["font_name"], json!("Cambria1"));
        assert_eq!(row["font_name_asian"], Value::Null);
        assert_eq!(row["font_name_complex"], json!("F"));
        assert_eq!(row["font_size_asian"], json!("10.5pt"));
        assert_eq!(row["language_asian"], Value::Null);
        assert_eq!(row["country_complex"], json!("SA"));
        // 三本各自数：名字 2 条、字号 3 条、语言 2 条，且每条带自己那一格的名字
        assert_eq!(
            mine["fonts_written"].as_array().map(|one| one
                .iter()
                .map(|row| row["slot"].as_str().unwrap())
                .collect::<Vec<_>>()),
            Some(vec!["latin", "complex"])
        );
        assert_eq!(
            mine["sizes_written"].as_array().map(|one| one
                .iter()
                .map(|row| row["slot"].as_str().unwrap())
                .collect::<Vec<_>>()),
            Some(vec!["latin", "asian", "complex"])
        );
        assert_eq!(
            mine["langs_written"].as_array().map(|one| one
                .iter()
                .map(|row| row["slot"].as_str().unwrap())
                .collect::<Vec<_>>()),
            Some(vec!["latin", "complex"])
        );
    }

    /// content.xml 里也能写：两列计数分开，「零条」与「没有这个部件」是两件事
    #[test]
    fn a_default_style_can_come_from_the_body_part_too() {
        let body = format!(
            "<office:text {}><style:default-style style:family=\"paragraph\">\
             <style:text-properties fo:hyphenate=\"false\" fo:hyphenation-keep=\"column\"/>\
             </style:default-style></office:text>",
            ODF_ATTR
        );
        let mine = odf(
            &packed(&[
                ("content.xml", body.as_str()),
                ("styles.xml", "<office:document-styles/>"),
            ]),
            100,
        );
        assert_eq!(mine["styles_part"], json!(true));
        assert_eq!(mine["defaults_total"], json!(0));
        assert_eq!(mine["defaults_in_content"], json!(1));
        assert_eq!(mine["rows"][0]["part"], json!("content.xml"));
        assert_eq!(
            mine["hyphenation_names"],
            json!(["hyphenate", "hyphenation-keep"])
        );
        assert_eq!(mine["hyphenation_rows"][0]["value"], json!("false"));
        assert_eq!(mine["hyphenation_rows"][1]["value"], json!("column"));
    }

    /// 孩子之间撞名（67 份真件一个都没有）：先到的那个孩子说话算
    #[test]
    fn the_first_child_wins_a_shared_attribute_name() {
        let mine = odf_with(
            "<style:default-style style:family=\"paragraph\">\
             <style:text-properties style:font-name=\"First\"/>\
             <style:paragraph-properties style:font-name=\"Second\" fo:font-size=\"11pt\"/>\
             </style:default-style>",
        );
        assert_eq!(mine["rows"][0]["font_name"], json!("First"));
        assert_eq!(mine["rows"][0]["props_attrs_total"], json!(3));
        assert_eq!(mine["fonts_written"][0]["value"], json!("First"));
    }

    /// 部件在场而一条都没写：整本空账，`styles_part` 仍是 true（不是「部件不在」）
    #[test]
    fn an_empty_part_is_still_a_part() {
        let mine = odf_with("");
        assert_eq!(mine["styles_part"], json!(true));
        assert_eq!(mine["defaults_total"], json!(0));
        assert_eq!(mine["families"], json!([]));
        assert_eq!(mine["rows"], json!([]));
        assert_eq!(mine["fonts_written"], json!([]));
        assert_eq!(mine["hyphenation_names"], json!([]));
    }
}
