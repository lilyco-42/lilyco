//! 这一段前面画什么 —— OOXML 把答案写在段自己身上（`a:pPr` 的那几枚孩子），ODF 把它
//! 放在段点名的那份列表样式里，而两份「没写」都不是同一个意思。
//!
//! OOXML：`a:pPr` 里 `buNone`（明确不画）、`buChar/@char`（画这个字）、
//! `buAutoNum/@type`（自动编号，`@startAt` 从几开始）三选一就是答案；一枚都不写是
//! 「这份文件没说」，连 `a:pPr` 都没有也是「没说」，但两者在这一族里是两格
//! （`"(没写)"` 与 `"(无 pPr)"`）。同一枚壳上还写着 `@marL`（文字左边距）、`@indent`
//! （符号 hanging 出去那一段）、`@lvl`（第几级）、`spcBef` / `spcAft` 里那枚点的数，
//! 以及 `buSzPct` / `buSzPts` / `buFont` 那三枚「符号长什么样」的附件。
//!
//! 版式与母版是另外一层：那里的符号写在 `a:lvl1pPr`…`a:lvl9pPr` 上，不挂在任何一段字上。
//! 所以页上「没写」的那些段离「没有符号」还差一层 —— 这一本只交两层的各自事实，
//! **不做那一跳的归属判断**（哪个占位符吃哪一级要把 `p:ph` 的号与名一起对，
//! 而实测重写会把那条手指弄断：见 `placeholders.rs`）。
//!
//! ODF：段自己什么都不写，写的是它外面那层 `text:list` 的 `@text:style-name`；那份
//! `text:list-style` 里每级一枚 `text:list-level-style-bullet/@text:bullet-char` 或
//! `text:list-level-style-number/@text:num-format` 才是答案，样式可以住在 `content.xml`
//! 也可以住在 `styles.xml`（跨部件那一跳与 odt 那本同一待遇）。级别在这一族是嵌套深度，
//! 里层 `text:list` 不写点名。
//!
//! 实测（`bullets.pptx` = python-pptx 打底 + 按 ECMA 写法摆 `a:pPr`；`bullets.odp` =
//! LibreOffice 转出的同一份；`bullets-lo.pptx` = LibreOffice 重写同一份 pptx）：
//! 1. **同一问两种密度**：python-pptx 只在写了符号的段上放 `a:pPr`（那份件 13 段里 7 段有，
//!    标题那一段整个没有），LibreOffice 重写时给每段都补一枚 `a:pPr`（13 段全有），并把 `algn`、`defTabSz`、`lnSpc`、
//!    `spcBef`、`buClr`、`buFont` 一起写下来 —— 「写没写壳」「写没写符号」是两问，各交各的；
//! 2. 同一个数两家可以不一样长：合成的 `marL="342900"`（0.375 英寸）被 LibreOffice 写成
//!    `343080`（它按 EMU 绕了一圈），`spcPts val="1200"` 那一条它照样留着；
//! 3. 第三支只能合成：真件里 `buAutoNum` **一条都没有**（104 份 pptx、11146 枚 `a:pPr`，
//!    buNone 7064、buChar 3672、什么符号都没写 410），所以 `arabicPeriod` 那一支自产件守；
//!    而 `buChar` 的字在真件里以 `•` 为绝对多数（3626）配一枚 `●`（46）；
//! 4. 到 ODF 那一头，同一份稿子变成**一行拆一份 `text:list`**（4 页 7 份列表，每份只一个
//!    `text:list-item` 套一段字），点名的是 `L1`…`L5` 那五份样式（各定义 10 级），
//!    而 `L4`/`L5` 的第 1 级是 `text:list-level-style-number` 且带 `start-value="3"` ——
//!    `buAutoNum/@startAt` 在另一族里的住处；整包 54 份列表样式里页只点了 5 份，`styles_unused` 49 就是
//!    「写了没人用」那一格（`L6`…`L8` 与母版页那十几份都在里头）；
//! 5. 母版页用的那几份列表样式（`ML1`…`ML10`）住在 `styles.xml`，页用的住在 `content.xml`
//!    —— 所以点名要跨两份部件找，`content.xml` 优先；
//! 6. 上面那一层是真实存在而且两家不同形：python-pptx 那本的版式与母版里 134 枚 `a:lvlNpPr`
//!    有 61 枚**一条符号都不写**（`silent` 61），而 LibreOffice 重写那本 88 枚一枚不落（`silent` 0），
//!    同时它把母版的 `p:txBody` 整个丢掉（同一批 24 件里带 `a:lstStyle` 的从 12 件变 11 件）——
//!    所以页上「没写」那些段到底画什么，本仓不替两层挑一个答案。
//!
//! 不做的事：**不换算 EMU 与点**（按写的串交）、**不判继承归属**（页上没写只说明这一件没说，
//! 不替版式补一个符号）、**不比对两族等价性**（同一份稿子在两族的段数与密度本来就不同）、
//! 遗留 `.ppt` 不交这个键（那一族的符号在 TextBytesAtom 的样式里，本仓不猜）。

use crate::xmlscan::{self, Node};
use crate::zipread;
use serde_json::{json, Value};

/// `a:pPr` 里那三枚「画什么」的孩子：名字就是答案
const BULLET_KINDS: [&str; 3] = ["buNone", "buChar", "buAutoNum"];
/// 一段字总坐在某个容器里：形状、图、表格格子的 `a:pPr` 各是各的
const CARRIERS: [&str; 6] = ["sp", "pic", "graphicFrame", "grpSp", "cxnSp", "tc"];
/// ODF 那一族的列表有三种元素名，问的是同一件事
const LISTY: [&str; 3] = ["list", "ordered-list", "unordered-list"];

fn kids(node: &Node) -> Vec<&Node> {
    node.children
        .iter()
        .filter(|one| one.name != "#text")
        .collect()
}

fn first_child<'a>(node: &'a Node, want: &str) -> Option<&'a Node> {
    node.children.iter().find(|one| one.local() == want)
}

fn parse_member(bytes: &[u8], part: &str) -> Option<Node> {
    let member = zipread::member(bytes, part, zipread::DEFAULT_MEMBER_CAP).ok()?;
    let text = member.as_text();
    if text.trim().is_empty() {
        return None;
    }
    Some(xmlscan::parse_str(&text))
}

/// 属性按局部名取；没写交 null，不替文件补规范默认值
fn attr(node: &Node, want: &str) -> Value {
    match node.attr_local(want) {
        Some(had) => json!(had),
        None => Value::Null,
    }
}

/// `spcBef` / `spcAft`：壳在不在与里面那枚点的数是两件事
fn spc(node: &Node, want: &str) -> (Value, bool) {
    let mut written = false;
    let mut first: Option<&Node> = None;
    for one in kids(node) {
        if one.local() != want {
            continue;
        }
        written = true;
        if first.is_none() {
            first = Some(one);
        }
    }
    let mut got = Value::Null;
    if let Some(holder) = first {
        for inner in kids(holder) {
            if inner.local() == "spcPts" || inner.local() == "spcPct" {
                got = attr(inner, "val");
                break;
            }
        }
    }
    (got, written)
}

fn bump(table: &mut serde_json::Map<String, Value>, key: &str) {
    let hit = table.get(key).and_then(|one| one.as_i64()).unwrap_or(0);
    table.insert(key.to_string(), json!(hit + 1));
}

/// 按某一格的字符串值数一份票；那一格是 null 的（「没写」）不进票
fn tally(rows: &[Value], key: &str) -> Value {
    let mut out = serde_json::Map::new();
    for one in rows {
        if let Value::String(had) = &one[key] {
            bump(&mut out, had);
        }
    }
    Value::Object(out)
}

fn said(rows: &[Value], key: &str) -> usize {
    rows.iter().filter(|one| !one[key].is_null()).count()
}

fn trues(rows: &[Value], key: &str) -> usize {
    rows.iter()
        .filter(|one| one[key].as_bool().unwrap_or(false))
        .count()
}

/// 一条 `a:pPr`（或者根本没有）→ 一行格子：键集永远一样
fn ppr_row(ppr: Option<&Node>, at: usize, carrier: &str, shape: Value) -> Value {
    let holder = match ppr {
        Some(had) => had,
        None => {
            return json!({
                "para": at, "carrier": carrier, "shape": shape, "ppr_written": false,
                "kind": "(无 pPr)", "char": Value::Null, "auto_type": Value::Null,
                "start_at": Value::Null, "bu_sz_pct": Value::Null, "bu_sz_pts": Value::Null,
                "bu_font": Value::Null, "lvl": Value::Null, "mar_l": Value::Null,
                "indent": Value::Null, "align": Value::Null, "spc_before": Value::Null,
                "spc_after": Value::Null, "spc_before_written": false,
                "spc_after_written": false, "attrs_written": 0,
            })
        }
    };
    let mut hit: Vec<&Node> = Vec::new();
    for one in kids(holder) {
        if BULLET_KINDS.contains(&one.local()) {
            hit.push(one);
        }
    }
    let kind = if hit.len() == 1 {
        hit[0].local().to_string()
    } else if hit.is_empty() {
        "(没写)".to_string()
    } else {
        hit.iter()
            .map(|one| one.local())
            .collect::<Vec<&str>>()
            .join("+")
    };
    let mine = hit.first().copied();
    let before = spc(holder, "spcBef");
    let after = spc(holder, "spcAft");
    let mut bu_sz_pct = Value::Null;
    let mut bu_sz_pts = Value::Null;
    let mut bu_font = Value::Null;
    for one in kids(holder) {
        let name = one.local();
        if name == "buSzPct" && bu_sz_pct.is_null() {
            bu_sz_pct = attr(one, "val");
        } else if name == "buSzPts" && bu_sz_pts.is_null() {
            bu_sz_pts = attr(one, "val");
        } else if name == "buFont" && bu_font.is_null() {
            bu_font = attr(one, "typeface");
        }
    }
    json!({
        "para": at,
        "carrier": carrier,
        "shape": shape,
        "ppr_written": true,
        "kind": kind,
        "char": mine.and_then(|one| one.attr_local("char")).map(String::from),
        "auto_type": mine.and_then(|one| one.attr_local("type")).map(String::from),
        "start_at": mine.and_then(|one| one.attr_local("startAt")).map(String::from),
        "bu_sz_pct": bu_sz_pct,
        "bu_sz_pts": bu_sz_pts,
        "bu_font": bu_font,
        "lvl": attr(holder, "lvl"),
        "mar_l": attr(holder, "marL"),
        "indent": attr(holder, "indent"),
        "align": attr(holder, "algn"),
        "spc_before": before.0,
        "spc_after": after.0,
        "spc_before_written": before.1,
        "spc_after_written": after.1,
        "attrs_written": holder
            .attrs
            .iter()
            .filter(|(key, _)| key != "xmlns" && !key.starts_with("xmlns:"))
            .count(),
    })
}

/// 前序走一遍：遇到载体就换上下文，遇到 `a:p` 就交一行
fn walk_paras(
    node: &Node,
    carrier: &str,
    shape: Value,
    seen: &mut Vec<String>,
    next: &mut usize,
    out: &mut Vec<Value>,
) {
    for one in kids(node) {
        let name = one.local();
        if CARRIERS.contains(&name) {
            seen.push(name.to_string());
            let mine = json!(*next);
            *next += 1;
            walk_paras(one, name, mine, seen, next, out);
            continue;
        }
        if name == "p" {
            out.push(ppr_row(
                first_child(one, "pPr"),
                out.len(),
                carrier,
                shape.clone(),
            ));
        }
        walk_paras(one, carrier, shape.clone(), seen, next, out);
    }
}

/// 一页（或任何一份部件）里每段前面画什么：只看这一件自己写没写
pub(crate) fn pptx_page(root: &Node, limit: usize) -> Value {
    let mut rows: Vec<Value> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    let mut next = 0usize;
    walk_paras(root, "(没有)", Value::Null, &mut seen, &mut next, &mut rows);
    let total = rows.len();
    let mut carriers = serde_json::Map::new();
    for one in seen.iter() {
        bump(&mut carriers, one);
    }
    json!({
        "family": "ooxml",
        "available": true,
        "paragraphs": total,
        "ppr_written": trues(&rows, "ppr_written"),
        "ppr_missing": total - trues(&rows, "ppr_written"),
        "declared": rows.iter().filter(|one| {
            BULLET_KINDS.contains(&match one["kind"].as_str() {
                Some(had) => had,
                None => "",
            })
        }).count(),
        "silent": rows.iter().filter(|one| {
            matches!(one["kind"].as_str(), Some("(没写)") | Some("(无 pPr)"))
        }).count(),
        "kinds": tally(&rows, "kind"),
        "chars": tally(&rows, "char"),
        "auto_types": tally(&rows, "auto_type"),
        "lvl_written": said(&rows, "lvl"),
        "marl_written": said(&rows, "mar_l"),
        "indent_written": said(&rows, "indent"),
        "bu_sz_written": rows.iter().filter(|one| {
            !one["bu_sz_pct"].is_null() || !one["bu_sz_pts"].is_null()
        }).count(),
        "spc_before_written": trues(&rows, "spc_before_written"),
        "spc_after_written": trues(&rows, "spc_after_written"),
        "carriers_found": seen.len(),
        "carriers_seen": Value::Object(carriers),
        "rows": rows.iter().take(limit).cloned().collect::<Vec<Value>>(),
        "listed": total.min(limit),
        "cut": total > limit,
    })
}

/// 一份版式 / 母件里 `a:lvlNpPr` 那些行：符号在这一层，而不在任何一段字上
fn level_rows(root: &Node, part: &str, layer: &str) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    let mut stack: Vec<&Node> = vec![root];
    // 前序手工栈：把这枚元素的子节点逆序压回去，弹出顺序就是文档顺序
    while let Some(one) = stack.pop() {
        let name = one.local();
        if name.starts_with("lvl") && name.ends_with("pPr") {
            let mut had = ppr_row(Some(one), out.len(), "(版式层)", Value::Null);
            had["level"] = json!(name);
            had["part"] = json!(part);
            had["layer"] = json!(layer);
            out.push(had);
        }
        for kid in kids(one).into_iter().rev() {
            stack.push(kid);
        }
    }
    out
}

/// 版式与母版那两层的符号账（部件名排序走，行按「件 + 文档序」）
pub(crate) fn pptx_layers(bytes: &[u8], limit: usize) -> Value {
    let mut names: Vec<String> = zipread::member_names(bytes)
        .into_iter()
        .filter(|one| {
            (one.starts_with("ppt/slideLayouts/") || one.starts_with("ppt/slideMasters/"))
                && one.ends_with(".xml")
        })
        .collect();
    names.sort();
    let mut rows: Vec<Value> = Vec::new();
    let mut parts = 0usize;
    let mut with_lst = 0usize;
    for name in names.iter() {
        let root = match parse_member(bytes, name.as_str()) {
            Some(one) => one,
            None => continue,
        };
        parts += 1;
        if !root.descendants("lstStyle").is_empty() {
            with_lst += 1;
        }
        let layer = if name.starts_with("ppt/slideLayouts/") {
            "layout"
        } else {
            "master"
        };
        for one in level_rows(&root, name.as_str(), layer) {
            rows.push(one);
        }
    }
    let total = rows.len();
    json!({
        "family": "ooxml",
        "available": true,
        "parts": parts,
        "parts_with_lst_style": with_lst,
        "levels": tally(&rows, "level"),
        "kinds": tally(&rows, "kind"),
        "chars": tally(&rows, "char"),
        "declared": rows.iter().filter(|one| {
            BULLET_KINDS.contains(&match one["kind"].as_str() {
                Some(had) => had,
                None => "",
            })
        }).count(),
        "silent": rows.iter().filter(|one| one["kind"] == "(没写)").count(),
        "marl_written": said(&rows, "mar_l"),
        "indent_written": said(&rows, "indent"),
        "rows": rows.iter().take(limit).cloned().collect::<Vec<Value>>(),
        "rows_total": total,
        "listed": total.min(limit),
        "cut": total > limit,
    })
}

/// 一份 `text:list-style` 里的一级
struct ListLevel {
    kind: String,
    char: Value,
    num_format: Value,
    num_suffix: Value,
    start_value: Value,
}

/// 一包里那些列表样式：名字 → 定义它的那份部件与那几级
pub(crate) struct ListStyle {
    name: String,
    part: String,
    levels: Vec<ListLevel>,
}

fn level_of(one: &Node) -> ListLevel {
    ListLevel {
        kind: one.local().to_string(),
        char: attr(one, "bullet-char"),
        num_format: attr(one, "num-format"),
        num_suffix: attr(one, "num-suffix"),
        start_value: attr(one, "start-value"),
    }
}

/// 点名要跨两份部件找（`content.xml` 优先，与 odt 那本同一口径）
pub(crate) fn odp_styles(bytes: &[u8]) -> Vec<ListStyle> {
    let mut out: Vec<ListStyle> = Vec::new();
    for part in ["content.xml", "styles.xml"] {
        let root = match parse_member(bytes, part) {
            Some(one) => one,
            None => continue,
        };
        for one in root.descendants("list-style") {
            let name = match one.attr_local("name") {
                Some(had) => had.to_string(),
                None => continue,
            };
            if out.iter().any(|had: &ListStyle| had.name == name) {
                continue;
            }
            let mut levels: Vec<ListLevel> = Vec::new();
            for kid in kids(one) {
                if kid.local().starts_with("list-level-style") {
                    levels.push(level_of(kid));
                }
            }
            out.push(ListStyle {
                name,
                part: part.to_string(),
                levels,
            });
        }
    }
    out
}

fn style_named<'a>(styles: &'a [ListStyle], want: &str) -> Option<&'a ListStyle> {
    styles.iter().find(|one| one.name == want)
}

fn first_level(mine: Option<&ListStyle>) -> Option<&ListLevel> {
    mine.and_then(|one| one.levels.first())
}

/// 短名那一边：`list-level-style-bullet` → `bullet`；空壳那份记成 `(空)`
fn short_of(mine: Option<&ListLevel>) -> String {
    match mine {
        Some(had) => {
            let raw = had.kind.rsplit('-').next().unwrap_or("").to_string();
            if raw.is_empty() {
                "(空)".to_string()
            } else {
                raw
            }
        }
        None => "(空)".to_string(),
    }
}

pub(crate) fn odp_page(page: &Node, styles: &[ListStyle], limit: usize) -> Value {
    let mut rows: Vec<Value> = Vec::new();
    let mut lists = 0usize;
    let mut nested = 0usize;
    let mut unnamed = 0usize;
    let mut found = 0usize;
    let mut unfound = 0usize;
    let mut items = 0usize;
    let mut paras = 0usize;
    let mut in_lists = 0usize;
    let mut kinds = serde_json::Map::new();
    let mut chars = serde_json::Map::new();
    let mut stack: Vec<(&Node, usize, Option<&str>)> = vec![(page, 0, None)];
    // 前序手工栈：弹出顺序就是文档序，深度与最近的框随节点带下去
    while let Some((one, depth, frame)) = stack.pop() {
        let name = one.local();
        if LISTY.contains(&name) {
            let want = one.attr_local("style-name");
            let mine = want.and_then(|had| style_named(styles, had));
            let first = first_level(mine);
            if depth > 0 {
                nested += 1;
            }
            match want {
                None => unnamed += 1,
                Some(_) if mine.is_none() => unfound += 1,
                Some(_) => found += 1,
            }
            let key = if want.is_none() {
                "(没点名)".to_string()
            } else {
                match first {
                    Some(had) => short_of(Some(had)),
                    None => "(解不开)".to_string(),
                }
            };
            bump(&mut kinds, key.as_str());
            if let Some(had) = first.and_then(|one| one.char.as_str()) {
                bump(&mut chars, had);
            }
            let mut mine_items = 0usize;
            let mut direct_p = 0usize;
            for kid in kids(one) {
                if kid.local() != "list-item" {
                    continue;
                }
                mine_items += 1;
                direct_p += kids(kid).iter().filter(|had| had.local() == "p").count();
            }
            items += mine_items;
            rows.push(json!({
                "list": rows.len(),
                "element": name,
                "depth": depth,
                "frame": frame.map(String::from),
                "style_name": want.map(String::from),
                "style_found": mine.is_some(),
                "style_part": mine.map(|had| had.part.clone()),
                "levels_defined": mine.map(|had| had.levels.len()).unwrap_or(0),
                "level1_kind": json!(short_of(first)),
                "level1_char": first.map(|had| had.char.clone()).unwrap_or(Value::Null),
                "level1_num_format": first.map(|had| had.num_format.clone()).unwrap_or(Value::Null),
                "level1_start_value": first.map(|had| had.start_value.clone()).unwrap_or(Value::Null),
                "level1_num_suffix": first.map(|had| had.num_suffix.clone()).unwrap_or(Value::Null),
                "items": mine_items,
                "paras_direct": direct_p,
            }));
            lists += 1;
        }
        if name == "p" {
            paras += 1;
            if depth > 0 || LISTY.contains(&name) {
                in_lists += 1;
            }
        }
        let kid_depth = if LISTY.contains(&name) {
            depth + 1
        } else {
            depth
        };
        let kid_frame = if name == "frame" {
            one.attr_local("name")
        } else {
            frame
        };
        for kid in kids(one).into_iter().rev() {
            stack.push((kid, kid_depth, kid_frame));
        }
    }
    let total = rows.len();
    json!({
        "family": "odf",
        "available": true,
        "lists": lists,
        "lists_nested": nested,
        "lists_unnamed": unnamed,
        "styles_found": found,
        "styles_unfound": unfound,
        "items": items,
        "paras": paras,
        "paras_in_lists": in_lists,
        "paras_outside": paras - in_lists,
        "kinds": Value::Object(kinds),
        "chars": Value::Object(chars),
        "styles_defined": styles.len(),
        "rows": rows.iter().take(limit).cloned().collect::<Vec<Value>>(),
        "listed": total.min(limit),
        "cut": total > limit,
    })
}

/// 整册（ODF）：样式一共几份、页点了几份、哪几份没人点
pub(crate) fn odp_ledger(bytes: &[u8], limit: usize) -> Value {
    let styles = odp_styles(bytes);
    let mut used: Vec<(String, usize)> = Vec::new();
    if let Some(root) = parse_member(bytes, "content.xml") {
        for one in root.descendants("list") {
            let want = match one.attr_local("style-name") {
                Some(had) => had.to_string(),
                None => continue,
            };
            match used.iter_mut().find(|had| had.0 == want) {
                Some(hit) => hit.1 += 1,
                None => used.push((want, 1)),
            }
        }
        for name in ["ordered-list", "unordered-list"] {
            for one in root.descendants(name) {
                let want = match one.attr_local("style-name") {
                    Some(had) => had.to_string(),
                    None => continue,
                };
                match used.iter_mut().find(|had| had.0 == want) {
                    Some(hit) => hit.1 += 1,
                    None => used.push((want, 1)),
                }
            }
        }
    }
    let used_of = |want: &str| -> usize {
        used.iter()
            .find(|had| had.0 == want)
            .map(|had| had.1)
            .unwrap_or(0)
    };
    let mut named: Vec<&ListStyle> = styles.iter().collect();
    named.sort_by(|a, b| a.name.cmp(&b.name));
    let mut rows: Vec<Value> = Vec::new();
    let mut kinds = serde_json::Map::new();
    let mut chars = serde_json::Map::new();
    let mut widths = serde_json::Map::new();
    for one in named.iter() {
        let first = first_level(Some(*one));
        let short = short_of(first);
        bump(&mut kinds, short.as_str());
        if let Some(had) = first.and_then(|one| one.char.as_str()) {
            bump(&mut chars, had);
        }
        bump(&mut widths, one.levels.len().to_string().as_str());
        if rows.len() < limit {
            rows.push(json!({
                "name": one.name,
                "part": one.part,
                "levels": one.levels.len(),
                "used_by": used_of(one.name.as_str()),
                "level1_kind": short,
                "level1_char": first.map(|had| had.char.clone()).unwrap_or(Value::Null),
                "level1_num_format": first.map(|had| had.num_format.clone()).unwrap_or(Value::Null),
                "level1_start_value": first.map(|had| had.start_value.clone()).unwrap_or(Value::Null),
            }));
        }
    }
    let used_total = named
        .iter()
        .filter(|one| used_of(one.name.as_str()) > 0)
        .count();
    json!({
        "family": "odf",
        "available": true,
        "styles": styles.len(),
        "styles_used": used_total,
        "styles_unused": styles.len() - used_total,
        "kinds": Value::Object(kinds),
        "chars": Value::Object(chars),
        "levels_per_style": Value::Object(widths),
        "rows": rows,
        "listed": rows.len(),
        "cut": styles.len() > rows.len(),
    })
}
