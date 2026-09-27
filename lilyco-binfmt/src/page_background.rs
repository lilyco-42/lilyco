//! 这一页的底色是谁给的 —— OOXML 写在页自己身上，ODF 写在页点名的那份样式里，
//! 而两边都能「什么都不写」：那时候这一页的底色来自母版，而**来自母版这件事本身不写在页上**。
//!
//! 一页一份记录，交的是「这一页自己写没写、写了是哪种填法、点到哪几个颜色」，外加
//! 继承那一跳的答案。两族各有各的判不住，都如实交 null：
//!
//! * OOXML：`p:cSld` 的第一枚孩子 `p:bg`，肚子里是 `p:bgPr`（直接给填充）或 `p:bgRef`
//!   （给一个编号 + 一个主题色名）。六个填充族认全（`noFill` / `solidFill` / `gradFill` /
//!   `blipFill` / `pattFill` / `grpFill`），`fill` 那一格是短名（`none` / `solid` /
//!   `gradient` / `blip` / `pattern` / `group`），null 只表示「那两枚孩子里没有填充族」——
//!   `bgRef` 正是这种：它的色直接挂在 `bgRef` 下面，没有填充元素这一层；
//! * 渐变每一站的颜色按文档序列在 `colors`，每行交四格：元素名、`val`、`modifiers`
//!   （那枚颜色元素**自己**的其余属性，`sysClr` 的 `lastClr` 走这一格）与
//!   `modifier_elements`（它的直接孩子，DrawingML 的 `tint` / `shade` / `satMod` 正是这一族
//!   —— 修饰在这一族**不是属性**，只收属性就等于把这句话的证据丢掉）；
//!   站号原样在 `stop_positions` —— 万分比就是万分比的串，不除 1000、不算色；
//! * ODF：`draw:page/@draw:style-name` → 那一份 `style:family="drawing-page"` 的样式 →
//!   `style:drawing-page-properties` 上那几格。两份件都找（自动样式通常在 content.xml，
//!   但本仓对 ODF 不赌这条），住在哪一份写在 `style_part`。
//!
//! 实测（`deck-bg.pptx` 由 python-pptx 写四页：实色 / 显式 `noFill` / 渐变 / 什么都不写；
//! 另两份是 LibreOffice 的同格式重写与 odp 导出）：
//! 1. 「写了 noFill」与「什么都没写」在**两个生产者手里都不可分辨**，方向还相反：
//!    python-pptx 那份两页各写各的（第 2 页有整枚 `<p:bg><p:bgPr><a:noFill/>`、第 4 页
//!    没有 `p:bg`），而 LibreOffice 重写时把第 2 页那枚**整条丢掉**，于是两份在它手里都是
//!    `written: false`。转成 odp 同一件事换了地方：第 2、4 页**共用同一份 dp3**，
//!    那份样式的 `drawing-page-properties` 里一条 `draw:fill` 都没有（`fill_written: false`），
//!    所以整册那一份账里 dp3 挂着 2 页 —— 靠页数才看得出来这一格被共用了；
//! 2. 主题色在两家手里待遇不同：python-pptx 的渐变两站都写 `schemeClr val="accent1"`，
//!    三枚修饰（`tint` 100000/50000、`shade` 100000、`satMod` 130000/350000）写在那枚颜色
//!    元素的**孩子**身上；LibreOffice 重写时**替文件算完了**：两个 `srgbClr` 字面值
//!    （`3E7FCC` / `A4C1FF`）、修饰整批不写（`modifier_elements` 两行都是空表）；同一枚渐变
//!    方向元素两家点的属性名都不一样（`a:lin @scaled` vs `@ang`），所以填充元素的直接孩子
//!    整份交；
//! 3. 空壳 `<a:effectLst/>` 是一句说过的话（段边框那条分支同一条规矩）：python-pptx 每条
//!    底色后面都跟一枚空的，LibreOffice 一枚都不写，所以 `effect_lst_written` 与 `effects`
//!    是两个数 —— 前者「这枚元素在不在」、后者「它肚子里有几个孩子」；
//! 4. 底色坐在哪一层会随生产者搬家：python-pptx 的模板只在**母版**写
//!    `<p:bgRef idx="1001"><a:schemeClr val="bg1"/>`（11 份版式 0 枚），LibreOffice 重写时
//!    把它**摊到 11 份版式上写成字面 `FFFFFF`，而母版自己那枚没了**。这就是整册那本
//!    `layers` 的意义：只看页部件会把这种搬家读成「底色丢了」；
//! 5. ODF 的名字解得开但要两跳：`draw:fill="gradient"` + `draw:fill-gradient-name="msFillGradient_20_1"`
//!    → styles.xml 的 `office:styles` 里那枚 `<draw:gradient draw:name="msFillGradient_20_1"
//!    draw:style="linear" draw:start-color="#3e7fcc" draw:end-color="#a4c1ff">`（元素名是
//!    `draw:gradient`，不是 `style:gradient`）。解不开时 `gradient.found` 是 false；
//! 6. 继承那一跳在 ODF 是三跳：页 `@draw:master-page-name="Blank"` → styles.xml
//!    `<style:master-page style:name="Blank" draw:style-name="Mdp1">` → `Mdp1` 那份样式
//!    （实测 `draw:background-size="border" draw:fill="solid" draw:fill-color="#ffffff"`）。
//!    三跳里任一跳断了都交 `found: false`，不替文件补一个白色。
//!
//! 不做的事：**不替文件算色**（带修饰的主题色就交修饰的原样）、**不判三层谁覆盖谁**
//! （页、版式、母版各自的记录都交，链子按各自族的写法走，不把 OOXML 的层叠搬到 ODF 上，
//! 也不反过来）。遗留 .ppt 不交这个键：它的记录树里没有页底这一层，本机没有凭据。
//! docx / odt 也不交：本机 32 份真件与 105 份自产 fixture 的 `word/document.xml` 里
//! `w:background` 与 `w:displayBackgroundShape` 各 0 处，python-docx 1.2.0 没有这个口，
//! LibreOffice 的 docx 导出也一个字都不写 —— 那是生产者做不出，不是读不出来。

use crate::xmlscan::{self, Node};
use crate::zipread::{self, DEFAULT_MEMBER_CAP};
use serde_json::{json, Value};

/// DrawingML 的六个填充族：元素局部名 → 交出去的短名
const FILL_LOCALS: [(&str, &str); 6] = [
    ("noFill", "none"),
    ("solidFill", "solid"),
    ("gradFill", "gradient"),
    ("blipFill", "blip"),
    ("pattFill", "pattern"),
    ("grpFill", "group"),
];

/// 颜色元素那一族（渐变每一站里坐的就是这几个）
const COLOR_LOCALS: [&str; 6] = [
    "srgbClr",
    "schemeClr",
    "sysClr",
    "prstClr",
    "hslClr",
    "scrgbClr",
];

/// 会写页底的六种部件：名字前缀 → 层名。整段目录名一起比 ——
/// `ppt/slideLayouts/` 与 `ppt/slides/` 都以 `ppt/slide` 开头，只比前缀会串。
const OOXML_PARTS: [(&str, &str); 6] = [
    ("ppt/slides/slide", "slide"),
    ("ppt/slideLayouts/slideLayout", "layout"),
    ("ppt/slideMasters/slideMaster", "master"),
    ("ppt/notesSlides/notesSlide", "notesSlide"),
    ("ppt/notesMasters/notesMaster", "notesMaster"),
    ("ppt/handoutMasters/handoutMaster", "handoutMaster"),
];

/// 一个元素自己的属性，按局部名收（命名空间声明不要，`skip` 里那几个名字也不要）
fn attrs_of(node: &Node, skip: &[&str]) -> Value {
    let mut out = serde_json::Map::new();
    for (key, value) in node.attrs.iter() {
        if key == "xmlns" || key.starts_with("xmlns:") {
            continue;
        }
        let local = key.rsplit(':').next().unwrap_or(key);
        if skip.contains(&local) {
            continue;
        }
        out.insert(local.to_string(), json!(value));
    }
    Value::Object(out)
}

/// 第一枚填充元素：返回（元素名、短名、那个节点）。没有填充族时是 None ——
/// `p:bgRef` 就是这种，它的色挂在 `bgRef` 自己身上
fn first_fill<'a>(inner: &'a Node) -> Option<(&'static str, &'static str, &'a Node)> {
    for (local, short) in FILL_LOCALS {
        for kid in inner.children.iter() {
            if kid.local() == local {
                return Some((local, short, kid));
            }
        }
    }
    None
}

/// 这一份底色点到几个颜色：递归、按文档序（渐变两站就是两条）
fn walk_colors(node: &Node, out: &mut Vec<Value>, limit: usize) {
    for kid in node.children.iter() {
        if out.len() >= limit {
            return;
        }
        if COLOR_LOCALS.contains(&kid.local()) {
            // 修饰在这一族是**孩子元素**（`<a:tint val="100000"/>` 那一串），不是属性：
            // 不交出去就等于把「python-pptx 指着主题色、LibreOffice 替文件算完了」这句话
            // 的证据丢掉一半，所以直接孩子整份交，`modifiers` 只收属性那一半（`sysClr` 的
            // `lastClr` 那种）。
            let mods: Vec<Value> = kid
                .children
                .iter()
                .map(|one| json!({"element": one.local(), "attrs": attrs_of(one, &[])}))
                .collect();
            out.push(json!({
                "element": kid.local(),
                "val": kid.attr_local("val").map(String::from),
                "modifiers": attrs_of(kid, &["val"]),
                "modifier_elements": mods,
            }));
        }
        walk_colors(kid, out, limit);
    }
}

fn first_of<'a>(node: &'a Node, want: &str) -> Option<&'a Node> {
    if let Some(had) = node.child(want) {
        return Some(had);
    }
    node.descendants(want).into_iter().next()
}

/// 一枚 `p:bg` 的记录。`root` 是那一种部件的根（slide / layout / master 同一条路）
fn ooxml_bg(root: &Node, limit: usize) -> Value {
    let holder = match first_of(root, "cSld") {
        Some(had) => had,
        None => return ooxml_unwritten(),
    };
    let bg = match holder.child("bg") {
        Some(had) => had,
        None => return ooxml_unwritten(),
    };
    let inner = bg.child("bgPr").or_else(|| bg.child("bgRef"));
    let mut colors: Vec<Value> = Vec::new();
    walk_colors(bg, &mut colors, limit);
    let mut color_names: Vec<String> = Vec::new();
    for one in colors.iter() {
        if let Some(had) = one["element"].as_str() {
            if !color_names.iter().any(|k: &String| k == had) {
                color_names.push(had.to_string());
            }
        }
    }
    let mut stop_positions: Vec<Value> = Vec::new();
    if let Some(had) = inner {
        for one in had.descendants("gs") {
            if stop_positions.len() >= limit {
                break;
            }
            stop_positions.push(json!(one.attr_local("pos").map(String::from)));
        }
    }
    let fill = inner.and_then(|had| first_fill(had));
    let fill_children: Vec<Value> = match fill {
        Some((_, _, node)) => node
            .children
            .iter()
            .map(|one| json!({"element": one.local(), "attrs": attrs_of(one, &[])}))
            .collect(),
        None => Vec::new(),
    };
    json!({
        "family": "ooxml",
        "available": true,
        "written": true,
        "holder": holder.local(),
        "via": inner.map(|had| had.local()).filter(|one| !one.is_empty()),
        "attrs": attrs_of(bg, &[]),
        "fill": fill.map(|(_, short, _)| short),
        "fill_element": fill.map(|(local, _, _)| local),
        "fill_attrs": fill.map(|(_, _, node)| attrs_of(node, &[])).unwrap_or(Value::Null),
        "fill_children": fill_children,
        "stops": stop_positions.len(),
        "stop_positions": stop_positions,
        "colors": colors,
        "color_names": color_names,
        "effect_lst_written": inner.map(|had| had.child("effectLst").is_some()).unwrap_or(false),
        "effects": inner
            .and_then(|had| had.child("effectLst"))
            .map(|had| had.children.len()),
        "idx": inner.and_then(|had| had.attr_local("idx")).map(String::from),
    })
}

/// 「这一层什么都没写」：null 而不是 0 —— 0 是「写了且数是 0」，null 才是没东西可读
fn ooxml_unwritten() -> Value {
    json!({
        "family": "ooxml",
        "available": true,
        "written": false,
        "holder": Value::Null,
        "via": Value::Null,
        "attrs": Value::Null,
        "fill": Value::Null,
        "fill_element": Value::Null,
        "fill_attrs": Value::Null,
        "fill_children": Value::Null,
        "stops": Value::Null,
        "stop_positions": Value::Null,
        "colors": Value::Null,
        "color_names": Value::Null,
        "effect_lst_written": Value::Null,
        "effects": Value::Null,
        "idx": Value::Null,
    })
}

/// OOXML 那一面：这一页（或这一份版式 / 母版部件）的 `p:bg`
pub(crate) fn pptx(root: &Node, limit: usize) -> Value {
    ooxml_bg(root, limit)
}

fn holder_of(name: &str) -> Option<&'static str> {
    for (prefix, label) in OOXML_PARTS {
        if name.starts_with(prefix) && name.ends_with(".xml") {
            return Some(label);
        }
    }
    None
}

fn parse_member(bytes: &[u8], part: &str) -> Option<Node> {
    let member = match zipread::member(bytes, part, DEFAULT_MEMBER_CAP) {
        Ok(one) => one,
        Err(_) => return None,
    };
    Some(xmlscan::parse_str(&member.as_text()))
}

/// 整册（OOXML）：哪些层写过底色、各是什么填法。页级那一份记录看不见母版与版式之间
/// 那次搬家（实测 LibreOffice 重写会做），所以逐件来一遍。
pub(crate) fn pptx_parts(bytes: &[u8], names: &[String], limit: usize) -> Value {
    let mut entries: Vec<(String, serde_json::Map<String, Value>)> = Vec::new();
    let mut layers: Vec<(String, usize)> = Vec::new();
    let mut fills: Vec<String> = Vec::new();
    let mut scanned = 0usize;
    let mut written = 0usize;
    // 先把序定下来再走：`layers` 与 `fills_seen` 交的是「第一次见到」的顺序，而 zip 里的
    // 存储序跟着生产者走（python-pptx 页在前，LibreOffice 不是）—— 不定序就是两份账
    let mut names: Vec<&str> = names.iter().map(|one| one.as_str()).collect();
    names.sort_unstable();
    for name in names {
        let label = match holder_of(name) {
            Some(one) => one,
            None => continue,
        };
        let root = match parse_member(bytes, name) {
            Some(one) => one,
            None => continue,
        };
        scanned += 1;
        let one = ooxml_bg(&root, limit);
        if one["written"].as_bool() != Some(true) {
            continue;
        }
        written += 1;
        if let Some(had) = one["fill"].as_str() {
            if !fills.iter().any(|k: &String| k == had) {
                fills.push(had.to_string());
            }
        }
        match layers.iter_mut().find(|hit| hit.0 == label) {
            Some(hit) => hit.1 += 1,
            None => layers.push((label.to_string(), 1)),
        }
        if entries.len() < limit {
            let mut row = match one {
                Value::Object(had) => had,
                _ => serde_json::Map::new(),
            };
            row.insert("layer".to_string(), json!(label));
            entries.push((name.to_string(), row));
        }
    }
    // 按部件名排：zip 里的存储序跟着生产者走（python-pptx 是页在前，LibreOffice 不是），
    // 这一本账要比的是「哪一层写的」，所以先把序定下来
    entries.sort_by_key(|one| one.0.clone());
    let entries: Vec<Value> = entries
        .into_iter()
        .map(|(part, mut row)| {
            row.insert("part".to_string(), json!(part));
            Value::Object(row)
        })
        .collect();
    json!({
        "family": "ooxml",
        "available": true,
        "parts_scanned": scanned,
        "parts_with_bg": written,
        "layers": layers
            .into_iter()
            .map(|(name, count)| json!({"layer": name, "parts": count}))
            .collect::<Vec<Value>>(),
        "fills_seen": fills,
        "entries": entries,
        "cut": written > entries.len(),
    })
}

/// 一份 drawing-page 样式里与底色有关的那几格，拆开两堆：
/// `fill*`（`draw:fill` / `draw:fill-color` / `draw:fill-gradient-name`…）与
/// `background*`（`draw:background-size` / `presentation:background-visible`…）。
/// 其余（`display-footer` 那些）不进这本账。
struct DrawStyle {
    part: &'static str,
    fill: serde_json::Map<String, Value>,
    background: serde_json::Map<String, Value>,
    props_written: usize,
}

impl DrawStyle {
    fn silent(part: &'static str) -> DrawStyle {
        DrawStyle {
            part,
            fill: serde_json::Map::new(),
            background: serde_json::Map::new(),
            props_written: 0,
        }
    }

    fn from_props(part: &'static str, props: &Node) -> DrawStyle {
        let mut mine = DrawStyle::silent(part);
        for (key, value) in props.attrs.iter() {
            if key == "xmlns" || key.starts_with("xmlns:") {
                continue;
            }
            // 这一格数的是「那份样式一共说了几句话」，底色那一堆只是其中的子集：
            // dp1 是 7 句（display 三句 + `*-visible` 两句 + fill 两句）、dp3 是 5 句
            // （三句 display + 两句 visible，一句 fill 也没有）。母版那一份 Mdp1 只有 3 句，
            // 但继承那一行只交底色两堆，不交这个数
            mine.props_written += 1;
            let local = key.rsplit(':').next().unwrap_or(key);
            if local == "fill" || local.starts_with("fill-") {
                mine.fill.insert(local.to_string(), json!(value));
            } else if local.starts_with("background") {
                mine.background.insert(local.to_string(), json!(value));
            }
        }
        mine
    }

    /// 这一份样式到底写没写底色（`draw:fill` 那一堆空着就是没写）
    fn fill_written(&self) -> bool {
        !self.fill.is_empty()
    }
}

/// 页点名的那份 drawing-page 样式：两份件都找，跳不通交 None
fn odp_drawing_style(bytes: &[u8], want: &str) -> Option<DrawStyle> {
    for part in ["content.xml", "styles.xml"] {
        let root = match parse_member(bytes, part) {
            Some(one) => one,
            None => continue,
        };
        for one in root.descendants("style") {
            if crate::odsheet::attr_of(one, "family") != Some("drawing-page") {
                continue;
            }
            if crate::odsheet::attr_of(one, "name") != Some(want) {
                continue;
            }
            let props = one
                .descendants("drawing-page-properties")
                .into_iter()
                .next();
            return Some(match props {
                Some(had) => DrawStyle::from_props(part, had),
                // 那份样式在，可它肚子里没有属性表：与「找不到」是两件事，所以交一份空的
                None => DrawStyle::silent(part),
            });
        }
    }
    None
}

/// `draw:fill-gradient-name` 那一跳：定义住在 `office:styles` 里，元素名是 `draw:gradient`
fn odp_gradient(bytes: &[u8], named: &str) -> Value {
    for part in ["styles.xml", "content.xml"] {
        let root = match parse_member(bytes, part) {
            Some(one) => one,
            None => continue,
        };
        for one in root.descendants("gradient") {
            if crate::odsheet::attr_of(one, "name") != Some(named) {
                continue;
            }
            return json!({
                "name": named,
                "found": true,
                "part": part,
                "element": one.local(),
                "attrs": attrs_of(one, &["name"]),
            });
        }
    }
    json!({
        "name": named,
        "found": false,
        "part": Value::Null,
        "element": Value::Null,
        "attrs": Value::Null,
    })
}

/// 继承那一跳：页 → 母版页名 → 那份母版页点名的 drawing-page 样式（三跳，断了交 false）
fn odf_inherited(bytes: &[u8], page: &Node) -> Value {
    let master = match crate::odsheet::attr_of(page, "master-page-name") {
        Some(one) => one.to_string(),
        None => return odf_inherited_row(Value::Null, Value::Null),
    };
    let named = match parse_member(bytes, "styles.xml") {
        Some(root) => root
            .descendants("master-page")
            .into_iter()
            .find(|one| crate::odsheet::attr_of(one, "name") == Some(master.as_str()))
            .and_then(|one| crate::odsheet::attr_of(one, "style-name"))
            .map(String::from),
        None => None,
    };
    let style = match named.as_deref() {
        Some(want) => want,
        // 这一跳断在第二跳上：母版页名写了，可它没点名任何样式 —— 第二格照原样交 null
        None => return odf_inherited_row(json!(master), Value::Null),
    };
    let mine = match odp_drawing_style(bytes, style) {
        Some(one) => one,
        None => return odf_inherited_row(json!(master), json!(named)),
    };
    json!({
        "master": master,
        "master_style": style,
        "found": true,
        "part": mine.part,
        "fill": mine.fill.get("fill").cloned().unwrap_or(Value::Null),
        "fill_attrs": Value::Object(mine.fill.clone()),
        "background_attrs": Value::Object(mine.background.clone()),
    })
}

fn odf_inherited_row(master: Value, named: Value) -> Value {
    json!({
        "master": master,
        "master_style": named,
        "found": false,
        "part": Value::Null,
        "fill": Value::Null,
        "fill_attrs": Value::Null,
        "background_attrs": Value::Null,
    })
}

/// ODF 那一面：这一页点名的那份样式，加上母版那一跳
pub(crate) fn odf(bytes: &[u8], page: &Node) -> Value {
    let named = crate::odsheet::attr_of(page, "style-name").map(String::from);
    let inherited = odf_inherited(bytes, page);
    let mine = match named.as_deref() {
        Some(want) => odp_drawing_style(bytes, want),
        None => None,
    };
    let gradient = match mine
        .as_ref()
        .and_then(|one| one.fill.get("fill-gradient-name"))
    {
        Some(had) => match had.as_str() {
            Some(raw) => odp_gradient(bytes, raw),
            None => Value::Null,
        },
        None => Value::Null,
    };
    json!({
        "family": "odf",
        "available": true,
        "written": mine.as_ref().map(|one| one.fill_written()),
        "page_style": named,
        "style_found": mine.is_some(),
        "style_part": mine.as_ref().map(|one| one.part),
        "fill_written": mine.as_ref().map(|one| one.fill_written()).unwrap_or(false),
        "fill": mine.as_ref().and_then(|one| one.fill.get("fill")).cloned(),
        "fill_attrs": mine.as_ref().map(|one| Value::Object(one.fill.clone())).unwrap_or(Value::Null),
        "background_attrs": mine.as_ref().map(|one| Value::Object(one.background.clone())).unwrap_or(Value::Null),
        "props_written": mine.as_ref().map(|one| one.props_written),
        "gradient": gradient,
        "inherited": inherited,
    })
}

/// 整册（ODF）：几页共用一份样式 —— 「显式不填充」与「什么都没写」在 ODF 里
/// 就是靠这一格露出来的（实测两份不同的页挂在同一个 dp3 上）
pub(crate) fn odf_ledger(bytes: &[u8], limit: usize) -> Value {
    let mut rows: Vec<(String, usize)> = Vec::new();
    let mut pages = 0usize;
    let mut unnamed = 0usize;
    if let Some(root) = parse_member(bytes, "content.xml") {
        // 只认 `draw:page`：同族还有一枚 `draw:page-thumbnail`，局部名不同，不会混进来
        for one in root.descendants("page") {
            pages += 1;
            let want = match crate::odsheet::attr_of(one, "style-name") {
                Some(had) => had,
                None => {
                    unnamed += 1;
                    continue;
                }
            };
            match rows.iter_mut().find(|hit| hit.0 == want) {
                Some(hit) => hit.1 += 1,
                None => rows.push((want.to_string(), 1)),
            }
        }
    }
    let mut entries: Vec<Value> = Vec::new();
    let mut shared = 0usize;
    let mut silent = 0usize;
    let mut unfound = 0usize;
    for (name, count) in rows.into_iter().take(limit) {
        let mine = odp_drawing_style(bytes, &name);
        let written = mine.as_ref().map(|one| one.fill_written()).unwrap_or(false);
        if count > 1 {
            shared += 1;
        }
        if mine.is_some() && !written {
            silent += 1;
        }
        if mine.is_none() {
            unfound += 1;
        }
        entries.push(json!({
            "style": name,
            "pages": count,
            "style_found": mine.is_some(),
            "style_part": mine.as_ref().map(|one| one.part),
            "fill_written": written,
            "fill": mine.as_ref().and_then(|one| one.fill.get("fill")).cloned(),
            "fill_attrs": mine.as_ref().map(|one| Value::Object(one.fill.clone())).unwrap_or(Value::Null),
            "background_attrs": mine.as_ref().map(|one| Value::Object(one.background.clone())).unwrap_or(Value::Null),
        }));
    }
    json!({
        "family": "odf",
        "available": true,
        "pages": pages,
        "pages_unnamed": unnamed,
        "styles": entries,
        "shared_styles": shared,
        "silent_styles": silent,
        "unfound_styles": unfound,
    })
}
