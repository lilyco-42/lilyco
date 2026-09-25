//! 「这一页有哪些形状、哪个是组合、按什么顺序叠着」——pptx 的 `p:spTree` 直接孩子就是叠放序，
//! ODF 那一页里 `draw:page` 的孩子是同一句话的另一种写法（组合是 `svg:g`，不是 `draw:group`）
//!
//! 一页一份清单，按文档序（= 叠放序）递归下行。每条一个形状：`kind`（pptx 是 `sp` / `pic` /
//! `graphicFrame` / `grpSp` / `cxnSp`，ODF 是 `frame` / `custom-shape` / `g` / `image` /
//! `control`）、`name` 与 `id`（pptx 只在这个形状自己那枚 `cNvPr` 上取；ODF 这一族没有这个东西，
//! `id` 一律 null）、`depth` / `parent`（父条在本清单里的序号）、`xfrm`（pptx 四格 `off` /
//! `ext` / `chOff` / `chExt`，EMU 原样）、`size_written`（ODF 是**自带单位的串**：
//! `x="0.278cm"`）、`text_carrier`（字装在哪一层：`txBody` / `text-box` / `self` / null）、
//! `paragraphs_direct`（只算这个形状自己那一份）、`children`。
//!
//! 与旧键的关系写清楚：`shapes` / `pictures` / `graphic_frames` 那几个数是**整棵子树**的元素条数，
//! 这一支交的是「形状是谁、在哪一层、谁套着谁」。`nested` 两族还不同义 —— pptx 里深度 >0
//! 只可能是组合里面，ODF 里 `draw:frame` 套 `draw:image` 也算一层（实测 `deck.odp` 第 1 页
//! `nested: 1` 而 `groups: 0`），所以两个数不互相解释。
//!
//! 实测（`deck-gr.pptx` 由 python-pptx 写：第 1 页一个散框 `sp` + 一个 `grpSp` 套三个 `sp`，
//! 第 2 页故意什么都没有；另两份是 LibreOffice 的同格式重写与 odp 导出）：
//! 1. 第 1 页 5 条 = 顶层 2 + 组合里 3，`groups` 1、`max_depth` 1；第 2 页整份清单是空的 ——
//!    「一页零个形状」这一格有凭据（那页的 `spTree` 只剩一个 `grpSpPr`）；
//! 2. 组合自己那枚 `a:xfrm` 四份都在，而 `off` 是 `0,0`、`ext` 与 `chExt` 一模一样 ——
//!    这是生产者写下的样子：按写的交，不替它「修正」成页坐标；
//! 3. LibreOffice 重写：形状、组合、`chOff` / `chExt` 都保住，`id` 从 2..6 整批重排成 61..65，
//!    坐标走那条老换算（`100000` → `100080`、`2900000` → `2899800`），五个名字一字未动；
//!    它还顺手给 `spTree` 自己的那份 `grpSpPr` 补了一个**全 0 的 `a:xfrm`** —— 树不是形状，
//!    那一份不进清单，所以「往下找第一个 `xfrm`」这种写法只能看两层：一份自己的 `xfrm` 都没有的
//!    组合会把孩子的坐标当成自己的；
//! 4. 转成 odp：同一页仍是 5 条、层级与五个名字全对得上（`kinds_seen` 变成
//!    `["custom-shape", "g"]`），但坐标换成 `5.555cm` / `0.278cm` 这种串，`id` 这一族根本没有，
//!    而组合那一层**一个尺寸属性都不写**（`size_written` 是空表，不是 0）；
//! 5. 字装在哪一层在两族各有一种岔路：pptx 这边全是 `txBody`（`pic` 那一条干脆没有 ——
//!    `deck-pictures.pptx` 前两页 `carriers_seen` 是空表）；ODF 同一份件里两种并存 ——
//!    `draw:frame` 把字装在 `draw:text-box` 里，而 `draw:custom-shape` 的 `text:p` **直接挂在
//!    形状自己身上**（`deck.odp` 第 1 页 `carriers_seen` 是 `["text-box", "self"]`）。只按
//!    「有没有 text-box」数段，那份件里三个 custom-shape 各带一段会被读成 0 段。
//!
//! 备注那棵树不进这份账：pptx 只走这一页的 `spTree`（备注在另一个部件），ODF 的白名单里没有
//! `notes` 这个容器，因此走不进去 —— 与页上链接那一条同一规矩。遗留 .ppt 不交这个键：
//! 它的记录树里没有「形状树」这一层（按 0x03EE 容器归页的那本账另在 `records`）。

use crate::xmlscan::Node;
use serde_json::{json, Value};

/// pptx 那五个能出现在 `spTree` 里的形状元素（局部名）
const SHAPE_KINDS: [&str; 5] = ["sp", "pic", "graphicFrame", "grpSp", "cxnSp"];
/// ODF 那一族：容器与叶子都算一条（`g` 是分组，`image` 是 `frame` 里的图，
/// `notes` 不在表里 —— 备注页的形状不进这份账）
const ODP_KINDS: [&str; 5] = ["g", "frame", "custom-shape", "control", "image"];

/// 一个元素的全部属性（局部名；命名空间声明不算属性）
fn attrs_local(node: &Node) -> serde_json::Map<String, Value> {
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

fn first_descendant<'a>(node: &'a Node, want: &str) -> Option<&'a Node> {
    node.descendants(want).into_iter().next()
}

fn has_child(node: &Node, want: &str) -> bool {
    node.children.iter().any(|one| one.local() == want)
}

/// 这个形状**自己**的 `a:xfrm`：pptx 三种挂法 —— `graphicFrame` 直接挂一个孩子，
/// `sp`/`pic` 挂在 `spPr` 里，`grpSp` 挂在 `grpSpPr` 里。只看这两层，不往下钻 ——
/// 因为 LibreOffice 重写时会给 `spTree` 自己的那份 `grpSpPr` 也补一个全 0 的 `a:xfrm`，
/// 一棵「往下找第一个」的树会把孙子那一份算到爷爷头上。
fn own_xfrm(node: &Node) -> Option<&Node> {
    if let Some(had) = node.children.iter().find(|one| one.local() == "xfrm") {
        return Some(had);
    }
    for kid in node.children.iter() {
        let name = kid.local();
        if name.ends_with("Pr") {
            if let Some(had) = kid.children.iter().find(|one| one.local() == "xfrm") {
                return Some(had);
            }
        }
    }
    None
}

/// 这个形状自己的那枚 `p:cNvPr`（`sp`/`grpSp`/`pic`/`graphicFrame` 都是
/// 一个 `nv*Pr` 孩子里面那枚 `nvPr` 的孩子）。名字与号只在这里取，
/// 所以组合不会把孩子的 `cNvPr` 当成自己的。
fn own_cnvpr(node: &Node) -> Option<&Node> {
    for kid in node.children.iter() {
        let name = kid.local();
        if !name.starts_with("nv") || !name.ends_with("Pr") {
            continue;
        }
        if let Some(found) = first_descendant(kid, "cNvPr") {
            return Some(found);
        }
    }
    None
}

/// 这个形状自己是不是占位符（同样只在 `nv*Pr` 里找，不钻孩子）
fn own_placeholder(node: &Node) -> bool {
    for kid in node.children.iter() {
        let name = kid.local();
        if !name.starts_with("nv") || !name.ends_with("Pr") {
            continue;
        }
        if first_descendant(kid, "ph").is_some() {
            return true;
        }
    }
    false
}

/// 这个形状自己的 `a:xfrm` 那四格（有则交，EMU 原样不换算）
fn xfrm_of(node: &Node) -> Value {
    let mut out = serde_json::Map::new();
    if let Some(had) = own_xfrm(node) {
        for kid in had.children.iter() {
            out.insert(kid.local().to_string(), Value::Object(attrs_local(kid)));
        }
    }
    Value::Object(out)
}

/// ODF 那一族的尺寸与位置（自带单位的串，原样交）
fn size_of(node: &Node) -> Value {
    let want = ["x", "y", "width", "height", "z-index", "transform"];
    let mut out = serde_json::Map::new();
    for (key, value) in node.attrs.iter() {
        let local = key.rsplit(':').next().unwrap_or(key);
        if want.contains(&local) {
            out.insert(local.to_string(), json!(value));
        }
    }
    Value::Object(out)
}

/// 这个形状**自己**的段有几段 —— 三家三种挂法，所以先问「字装在哪一层」再数：
/// pptx 装在直接孩子 `p:txBody` 里；ODF 的 `draw:frame` 装在直接孩子
/// `draw:text-box` 里，而 `draw:custom-shape` 干脆把 `text:p` **直接挂在形状自己身上**
/// （实测 `deck-gr.odp` 三个 custom-shape 各带一段，按「有没有 text-box」数出来全是 0）。
/// 只算自己这一层：组合里那些段记在孩子自己那一条上，否则一层报一次、整篇翻倍。
fn paragraphs_of(node: &Node, holders: &[&str]) -> usize {
    let mut n = 0usize;
    for kid in node.children.iter() {
        let name = kid.local();
        if holders.contains(&name) {
            n += kid.descendants("p").len() + kid.descendants("h").len();
        } else if name == "p" || name == "h" {
            n += 1;
        }
    }
    n
}

/// 字装在哪一层：`txBody` / `text-box` / `self`（段直接挂在形状身上）/ null（这一个不装字）
fn text_carrier(node: &Node, holders: &[&str]) -> Value {
    for kid in node.children.iter() {
        if holders.contains(&kid.local()) {
            return json!(kid.local());
        }
    }
    for kid in node.children.iter() {
        let name = kid.local();
        if name == "p" || name == "h" {
            return json!("self");
        }
    }
    Value::Null
}

fn name_attr(node: &Node) -> Option<String> {
    for (key, value) in node.attrs.iter() {
        if key.rsplit(':').next().unwrap_or(key) == "name" {
            return Some(value.clone());
        }
    }
    None
}

/// 一条 pptx 形状（`cNvPr` 与 `txBody` 是这一族才有的东西）
fn pptx_entry(node: &Node, depth: usize, parent: Option<usize>, index: usize) -> Value {
    let cnv = own_cnvpr(node);
    json!({
        "index": index,
        "kind": node.local(),
        "name": cnv.and_then(|had| had.attr_local("name")).map(String::from),
        "id": cnv.and_then(|had| had.attr_local("id")).map(String::from),
        "depth": depth,
        "parent": parent,
        "placeholder": own_placeholder(node),
        "xfrm": xfrm_of(node),
        "size_written": Value::Object(serde_json::Map::new()),
        "text_carrier": text_carrier(node, &["txBody"]),
        "has_text_body": has_child(node, "txBody"),
        "paragraphs_direct": paragraphs_of(node, &["txBody"]),
        "children": node.children.iter().filter(|one| SHAPE_KINDS.contains(&one.local())).count(),
    })
}

/// 一条 ODF 形状：没有 `cNvPr` 这一层，名字与尺寸都写在自己身上
fn odp_entry(node: &Node, depth: usize, parent: Option<usize>, index: usize) -> Value {
    json!({
        "index": index,
        "kind": node.local(),
        "name": name_attr(node),
        "id": Value::Null,
        "depth": depth,
        "parent": parent,
        "placeholder": Value::Null,
        "xfrm": Value::Object(serde_json::Map::new()),
        "size_written": size_of(node),
        "text_carrier": text_carrier(node, &["text-box"]),
        "has_text_body": has_child(node, "text-box"),
        "paragraphs_direct": paragraphs_of(node, &["text-box"]),
        "children": node.children.iter().filter(|one| ODP_KINDS.contains(&one.local())).count(),
    })
}

fn pptx_walk(node: &Node, depth: usize, parent: Option<usize>, rows: &mut Vec<Value>) {
    for kid in node
        .children
        .iter()
        .filter(|one| SHAPE_KINDS.contains(&one.local()))
    {
        let mine = rows.len();
        rows.push(pptx_entry(kid, depth, parent, mine));
        if kid.local() == "grpSp" {
            pptx_walk(kid, depth + 1, Some(mine), rows);
        }
    }
}

fn odp_walk(node: &Node, depth: usize, parent: Option<usize>, rows: &mut Vec<Value>) {
    for kid in node
        .children
        .iter()
        .filter(|one| ODP_KINDS.contains(&one.local()))
    {
        let mine = rows.len();
        rows.push(odp_entry(kid, depth, parent, mine));
        odp_walk(kid, depth + 1, Some(mine), rows);
    }
}

fn ledger(family: &str, rows: Vec<Value>) -> Value {
    let mut kinds: Vec<String> = Vec::new();
    let mut names: Vec<String> = Vec::new();
    let mut carriers: Vec<String> = Vec::new();
    for one in rows.iter() {
        if let Some(had) = one["kind"].as_str() {
            if !kinds.iter().any(|k: &String| k == had) {
                kinds.push(had.to_string());
            }
        }
        if let Some(had) = one["name"].as_str() {
            if !names.iter().any(|k: &String| k == had) {
                names.push(had.to_string());
            }
        }
        if let Some(had) = one["text_carrier"].as_str() {
            if !carriers.iter().any(|k: &String| k == had) {
                carriers.push(had.to_string());
            }
        }
    }
    json!({
        "family": family,
        "available": true,
        "shapes_total": rows.len(),
        "top_level": rows.iter().filter(|one| one["depth"] == json!(0)).count(),
        "nested": rows.iter().filter(|one| one["depth"] != json!(0)).count(),
        "groups": rows.iter().filter(|one| {
            one["kind"] == json!("grpSp") || one["kind"] == json!("g")
        }).count(),
        "unnamed": rows.iter().filter(|one| one["name"].is_null()).count(),
        "placeholders": rows.iter().filter(|one| one["placeholder"] == json!(true)).count(),
        "carriers_seen": carriers,
        "paragraphs_in_shapes": rows.iter()
            .map(|one| one["paragraphs_direct"].as_u64().unwrap_or(0))
            .sum::<u64>(),
        "kinds_seen": kinds,
        "distinct_names": names,
        "max_depth": rows.iter().map(|one| one["depth"].as_u64().unwrap_or(0)).max().unwrap_or(0),
        "shapes": rows,
    })
}

/// pptx 那一面：这一页的 `spTree` 一份清单（叠放序 = 孩子顺序）
pub(crate) fn pptx(slide_root: &Node, limit: usize) -> Value {
    let mut rows: Vec<Value> = Vec::new();
    if let Some(tree) = first_descendant(slide_root, "spTree") {
        pptx_walk(tree, 0, None, &mut rows);
    }
    let rows = take(rows, limit);
    ledger("ooxml", rows)
}

/// ODF 那一面：这一页 `draw:page` 的孩子一份清单
pub(crate) fn odf(page: &Node, limit: usize) -> Value {
    let mut rows: Vec<Value> = Vec::new();
    odp_walk(page, 0, None, &mut rows);
    let rows = take(rows, limit);
    ledger("odf", rows)
}

fn take(rows: Vec<Value>, limit: usize) -> Vec<Value> {
    rows.into_iter().take(limit).collect::<Vec<Value>>()
}
