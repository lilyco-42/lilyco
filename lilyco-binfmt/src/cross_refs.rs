//! 题注与交叉引用那一份账（三家）：一句「引用谁」写在三种地方，查的书也各有三本
//!
//! 这一本不重读一遍文件：**行的来源是已经交过的那本域账**（`field_ledger`），这里只把
//! 「有目标这一问」的那几条挑出来，再问一句「它指的东西在这份件里找得到吗」。认得的种类
//! 只有五个词（`REF` / `PAGEREF` / `NOTEREF` / `STYLEREF` / `SEQ`）与 ODF 的两枚元素名，
//! 认不得的一条也不丢进账 —— 它们不在这本账的问题范围内，不是「引用坏了」。
//!
//! 三本查的书（`book` 那一列说这一条查哪一本）：`bookmark`（REF / PAGEREF / NOTEREF，
//! ODF 是 `text:bookmark-ref` 一枚顶两家）查书签名；`style`（STYLEREF）查**样式名**；
//! `sequence`（SEQ）查序列声明 —— 而**只有 ODF 有声明这一层**。
//!
//! 「这一族有没有声明这一层」是一等公民，所以 `resolves` 有三态：true / false / **null**。
//! SEQ 在 OOXML 与 RTF 交 null —— 不是「查不到」，是这一族压根没有可查的那本书；把它当成
//! false 就等于替文件编一条「引用坏了」。
//!
//! OOXML 的目标**只住在指令串里**：种类词后面那一个「词」就是名字，它可以是带引号的
//! 一整串（`STYLEREF "标题 1"` 名字里带空格，只能靠引号切开），也可以根本没有（`SEQ 图
//! \* ARABIC` 后面直接是开关）。所以四条分开交：`target`、`target_written_quoted`、
//! `next_token_is_switch`、`target_unterminated`（引号开了没关）。实测（155 份）：全库
//! 没有任何元素的**局部名**里带 seq / caption —— 这一族的序列号就是指令里的一个词；
//! 题注**样式**却是有的，71 份 .docx 里 69 份在 `word/styles.xml` 声明一条
//! `w:styleId="Caption"`（它的 `w:name` 值是 `caption`，两个名字大小写不同，所以两本都查、
//! `matched_on` 说在哪一本查到的），而正文里 `w:pStyle` 指到它的是 **0 段** —— 生产者写了
//! 样式却一段没用过；剩下那 2 份（`pnum.docx` / `tbox.docx`）有样式表、只写 3 条 / 2 条、
//! 一条题注都不写，所以「部件在但没声明」与「没有这个部件」两件事分开交（`styles_part`）。
//! 书签那一本扫**所有文本部件**（与域那一本同一个范围）：`REF` 指到页眉里的书签完全写得
//! 出来，只扫正文就会把一条成立的引用报成坏的。
//!
//! ODF 恰好相反：目标写在**属性**上，而两只名字不通用（实测）—— `text:sequence` 用自己的
//! `text:name` 说「我给哪一条序列编号」，`text:bookmark-ref` 用 `text:ref-name` 说「我引用
//! 谁」，被引用那枚书签自己的名字却写在 `text:name` 上。于是每行把七个属性位都摊出来
//! （`reference_format` / `formula` / `ref_name` / `own_name` / `num_format` /
//! `seq_sub_formula`），谁与谁相等交给数的人看。REF 与 PAGEREF 在这里塌成同一枚元素，只有
//! `text:reference-format`（`number` / `page`）分得开。声明那一层另交一本 `declarations`：
//! 41 份 .odt 里会编号的那 2 份（`fields.odt` / `fields-mix.odt`）六条全写，另 37 份只写
//! 五条英文的、一条都没用到，还有 2 份（`pnum.odt` / `tbox.odt`）一份不写 ——
//! 「声明了没人用」是本族常态（`declared_unused` 在 39 份里恒 5 条），所以它与
//! `used_without_decl`（本库恒 0）两本分交。`text:sequence-ref`（引用一个序列号）在本库
//! 出现 **0 次**，而认字表里根本没有它，所以它一行也开不出来 —— 那一格只交零计数，
//! 等真件来推翻。书签这一本数的是**元素枚数**（一对 start + end 就是两枚：`fields.odt`
//! 一枚书签 → `bookmark_marks` 是 2），名字那一本却按去重交，两个数本来就不该相等。
//!
//! RTF 与 OOXML 同一个形状（指令串解掉转义后逐字同形，所以那把切目标的尺子两家共用一只），
//! 不一样的是书签名**已经解过 `\u`**：`fields.rtf` 的 `\bkmkstart 表锚点` 与指令里那个名字
//! 是同一串字，中文书签在第三族配得上。样式那本书本族有名字（`\s55` 那条叫 `caption`），
//! 而 17 份里只有 `fields-mix.rtf` 写了一条 STYLEREF（`标题 1 (user)` 不在名字那本里，所以判 false）；样式号只按文件自己写的交（`fields*.rtf` 是 55，
//! `toc.rtf` 是 109），`style_ids` 因此一律 null —— `\sN` 那个号不是声明名。
//!
//! 三家共用的收尾那三格是把「引用成不成立」与「缓存值在不在」叉起来数：
//! `resolving_but_no_cache`（指得到却拿不出东西）、`cached_but_unresolved`（引用按写的比
//! 不成立、缓存值却躺在里面 —— `fields.docx` 的 `STYLEREF "标题 1"` 与库里英文声明名
//! `heading 1` 就是这一对，缓存值那格躺着 `标题一：给 STYLEREF 用`）、`cache_missing`。
//! `cache_values` 按**文件写的那串字**原样 census，不认它的语义：同一格里并存过空串、正常
//! 值与生产者留下的错误串 `错误: 引用源未找到`，判「这是不是一条错误」要给一张语言相关的表，
//! 这里一个都不编，只把字交出来。
//!
//! 还有一格是给「每条出口都摊一本名单」踩的刹车：`books` 里样式那两本**只交条数**（一份带
//! 模板的 docx 在 `word/styles.xml` 里写 164 条样式，整本名单摊进每条出口会把账本撑大两个
//! 数量级，而「这一条查得到吗」的答案在 `resolves` 那一列，不在名单里）。See fact 135.

use crate::field_ledger::{self, attr_local_in, bump, docx_text_part, field_kind};
use crate::xmlscan::{self, Node};
use crate::zipread::{self, DEFAULT_MEMBER_CAP};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

/// 种类词 → 查哪本书。名字按各家自己写的那个词收，不折成同一个词
fn book_of_instr(kind: &str) -> Option<&'static str> {
    match kind.to_ascii_uppercase().as_str() {
        "REF" | "PAGEREF" | "NOTEREF" => Some("bookmark"),
        "STYLEREF" => Some("style"),
        "SEQ" => Some("sequence"),
        _ => None,
    }
}

/// ODF 那一族种类就是元素名（`text:` 前缀在局部名里已经去掉）
fn book_of_odf(kind: &str) -> Option<&'static str> {
    match kind {
        "sequence" => Some("sequence"),
        "bookmark-ref" => Some("bookmark"),
        _ => None,
    }
}

/// 目标住在哪一枚属性上：`text:sequence` 用 `text:name`，`text:bookmark-ref` 用
/// `text:ref-name` —— 两只不通用，拿一只去问另一族就一律是「没写目标」
fn odf_target_attr(kind: &str) -> &'static str {
    if kind == "sequence" {
        "name"
    } else {
        "ref-name"
    }
}

/// 题注样式的认法：名字**含**这一个词根（小写比），两家语言各一条
fn is_caption(name: Option<&str>) -> bool {
    let Some(raw) = name else { return false };
    let low = raw.to_lowercase();
    low.contains("caption") || low.contains("题注")
}

/// 种类词后面那一个「词」就是目标名 —— 它可以是带引号的一整串，也可以根本不存在。
///
/// 返回 `(target, written_quoted, next_is_switch, unterminated)`：`target` 为 null 是
/// 「没写目标」（可能下一个 token 是开关，也可能整串到此为止），`""` 是**写了两个引号中间
/// 什么也没有** —— 这两件事在真件里都存在过，不能并成一个。
fn ref_target(instr: Option<&str>, kind: &str) -> (Option<String>, bool, bool, bool) {
    let mut head = instr.unwrap_or("").trim_start();
    let key = kind.to_ascii_uppercase();
    if head
        .as_bytes()
        .get(..key.len())
        .is_some_and(|one| one.eq_ignore_ascii_case(key.as_bytes()))
    {
        head = &head[key.len()..];
    }
    let head = head.trim_start();
    if head.is_empty() {
        return (None, false, false, false);
    }
    if let Some(rest) = head.strip_prefix('"') {
        return match rest.find('"') {
            None => (Some(rest.to_string()), true, false, true),
            Some(at) => (Some(rest[..at].to_string()), true, false, false),
        };
    }
    let token = head.split(' ').next().unwrap_or(head);
    if token.starts_with('\\') {
        return (None, false, true, false);
    }
    (Some(token.to_string()), false, false, false)
}

/// 那一本「名字书」里查得到吗（三态：这一族没有这本书时交 null，不编一条坏引用）
fn aims_at(
    which: &str,
    target: Option<&str>,
    bookmarks: &BTreeSet<String>,
    style_names: &BTreeSet<String>,
    style_ids: Option<&BTreeSet<String>>,
) -> Option<bool> {
    match which {
        "bookmark" => Some(target.is_some_and(|raw| bookmarks.iter().any(|one| one == raw))),
        "style" => Some(target.is_some_and(|raw| {
            style_names.contains(raw) || style_ids.is_some_and(|ids| ids.contains(raw))
        })),
        _ => None,
    }
}

/// 按字符串那一列数条数（种类、查哪本书、缓存值那三本）
fn census(rows: &[Value], want: &str) -> Value {
    let mut out = serde_json::Map::new();
    for one in rows {
        if let Value::String(had) = &one[want] {
            bump(&mut out, had.clone());
        }
    }
    Value::Object(out)
}

/// 三态那一列摊成显式三格：JSON 的键不能是 null，所以不按键数
fn tri(rows: &[Value], want: &str) -> Value {
    json!({
        "true": rows.iter().filter(|one| one[want] == json!(true)).count(),
        "false": rows.iter().filter(|one| one[want] == json!(false)).count(),
        "null": rows.iter().filter(|one| one[want].is_null()).count(),
    })
}

/// 那一条叉起来的计数：`resolves` 是这一边、缓存值拿不拿得出是另一边
///
/// 「拿不出」= 没写缓存值**或**写了空串 —— 两家生产者都写过这一格，而它们不是同一句话
fn cross(rows: &[Value], resolves: bool, blank_cache: bool) -> usize {
    rows.iter()
        .filter(|one| {
            one["resolves"] == json!(resolves)
                && (one["cached"].is_null() || one["cached"] == json!("")) == blank_cache
        })
        .count()
}

/// 三家共用的收尾：三条对账计数 + 缓存值那一本原名 census + 按 `limit` 截行
fn ledger(
    family: &str,
    rows: Vec<Value>,
    books: Value,
    caption: Value,
    declarations: Value,
    notes: Value,
    limit: usize,
) -> Value {
    let total = rows.len();
    let cached: Vec<Value> = rows
        .iter()
        .filter(|one| !one["cached"].is_null())
        .cloned()
        .collect();
    let shown = rows.iter().take(limit).cloned().collect::<Vec<Value>>();
    json!({
        "family": family,
        "available": true,
        "target_rows": total,
        "listed": total.min(limit),
        "cut": total > limit,
        "books": books,
        "kinds": census(&rows, "kind"),
        "target_books": census(&rows, "book"),
        "resolves": tri(&rows, "resolves"),
        "quoted": tri(&rows, "target_written_quoted"),
        "resolving_but_no_cache": cross(&rows, true, true),
        "cached_but_unresolved": cross(&rows, false, false),
        "cache_missing": rows.iter().filter(|one| one["cached"].is_null()).count(),
        "cache_values": census(&cached, "cached"),
        "caption_styles": caption,
        "declarations": declarations,
        "notes": notes,
        "rows": shown,
    })
}

/// 题注样式那三格：本族声明了几条、那些条被几段用着、逐条摊开
fn caption_ledger(rows: Vec<Value>) -> Value {
    json!({
        "declared": rows.len(),
        "paragraphs_using_them": rows
            .iter()
            .map(|one| one["paragraphs_using_it"].as_u64().unwrap_or(0))
            .sum::<u64>(),
        "rows": rows,
    })
}

/// 题注与交叉引用那一份账（OOXML）：目标只住在指令串里，SEQ 没有声明层可查
pub(crate) fn docx(bytes: &[u8], document: &Node, styles: Option<&Node>, limit: usize) -> Value {
    // 行来自域账，而这里要**全部**行（域账那本按 `--limit` 截过，截掉的不是「没引用的」）
    let book = field_ledger::docx(bytes, document, usize::MAX);
    if book["available"] != json!(true) {
        return json!({"family": "ooxml", "available": false});
    }
    let mut names: Vec<String> = zipread::member_names(bytes)
        .into_iter()
        .filter(|one| docx_text_part(one))
        .collect();
    names.sort();
    let mut owned: Vec<(String, Node)> = Vec::new();
    for name in &names {
        if name == "word/document.xml" {
            continue;
        }
        let Ok(had) = zipread::member(bytes, name, DEFAULT_MEMBER_CAP) else {
            continue;
        };
        owned.push((name.clone(), xmlscan::parse_str(&had.as_text())));
    }
    let mut marks = 0usize;
    let mut bookmarks: BTreeSet<String> = BTreeSet::new();
    let mut uses: BTreeMap<String, u64> = BTreeMap::new();
    for name in &names {
        let root: &Node = if name == "word/document.xml" {
            document
        } else {
            match owned.iter().find(|(one, _)| one == name) {
                Some((_, node)) => node,
                None => continue,
            }
        };
        // 书签那一本扫**所有文本部件**，与域那一本同一个范围：`REF` 指到页眉里的书签
        // 是完全写得出来的，只扫正文就会把一条成立的引用报成坏的
        for one in root.descendants("bookmarkStart") {
            marks += 1;
            if let Some(had) = one.attr_local("name") {
                bookmarks.insert(had.to_string());
            }
        }
        for one in root.descendants("pStyle") {
            if let Some(had) = one.attr_local("val") {
                *uses.entry(had.to_string()).or_insert(0) += 1;
            }
        }
    }
    // 样式表**不在**正文那一组部件里（`word/styles.xml` 的基名不是 document/header/…）：
    // 只扫上面那一组就拿不到任何一条声明，而账本会把它读成「这份文档没写样式」
    let mut style_defs = 0usize;
    let mut declared_ids: BTreeSet<String> = BTreeSet::new();
    let mut declared_names: BTreeSet<String> = BTreeSet::new();
    let mut caption_rows: Vec<Value> = Vec::new();
    if let Some(sheet) = styles {
        for one in sheet.descendants("style") {
            style_defs += 1;
            // `w:type` 是 `w:style` **自己的一枚属性**，不是孩子：拿孩子的局部名去找
            // 会一律拿到 None，于是这一列整本都是「没写类型」，看着像文件偷懒
            let style_id = one.attr_local("styleId").map(String::from);
            let named = one
                .child("name")
                .and_then(|kid| kid.attr_local("val"))
                .map(String::from);
            let written = one.attr_local("type").map(String::from);
            if let Some(raw) = &style_id {
                declared_ids.insert(raw.clone());
            }
            if let Some(raw) = &named {
                declared_names.insert(raw.clone());
            }
            let mut matched: Vec<&str> = Vec::new();
            if is_caption(style_id.as_deref()) {
                matched.push("styleId");
            }
            if is_caption(named.as_deref()) {
                matched.push("name");
            }
            if matched.is_empty() {
                continue;
            }
            caption_rows.push(json!({
                "part": "word/styles.xml",
                "style_id": style_id,
                "declared_name": named,
                // 两个名字大小写不同（`Caption` 对 `caption`），所以两个都查、各自报是
                // 在哪一本查到的；`paragraphs_using_it` 那一列的 0 就是「声明了没人用」
                "matched_on": matched,
                "type": written,
                "paragraphs_using_it": style_id
                    .as_deref()
                    .and_then(|raw| uses.get(raw))
                    .copied()
                    .unwrap_or(0),
            }));
        }
    }
    let mut rows: Vec<Value> = Vec::new();
    for one in book["rows"].as_array().into_iter().flatten() {
        let Some(kind) = one["kind"].as_str() else {
            continue;
        };
        let Some(which) = book_of_instr(kind) else {
            continue;
        };
        let instruction = one["instruction"].as_str();
        let (target, quoted, after, unterminated) = ref_target(instruction, kind);
        let resolves = aims_at(
            which,
            target.as_deref(),
            &bookmarks,
            &declared_names,
            Some(&declared_ids),
        );
        rows.push(json!({
            "part": one["part"].clone(),
            "index": one["index"].clone(),
            "kind": one["kind"].clone(),
            "instruction": one["instruction"].clone(),
            "target": target,
            "target_written_quoted": quoted,
            "next_token_is_switch": after,
            "target_unterminated": unterminated,
            "book": which,
            "resolves": resolves,
            "cached": one["cached"].clone(),
            "cache_written": !one["cached"].is_null(),
        }));
    }
    let mut caption = caption_ledger(caption_rows);
    // 「没有样式表这一份部件」与「有部件但一条题注样式都不写」是两件事，分开交
    caption["styles_part"] = json!(styles.is_some());
    ledger(
        "ooxml",
        rows,
        json!({
            "bookmarks": bookmarks.into_iter().collect::<Vec<String>>(),
            "bookmark_marks": marks,
            "style_defs": style_defs,
            "style_ids": declared_ids.len(),
            "style_names": declared_names.len(),
            "sequences": Value::Null,
        }),
        caption,
        Value::Null,
        json!(["这一族没有序列声明这一层：SEQ 的 resolves 一律 null"]),
        limit,
    )
}

/// ODF 那一份扫描：声明、书签、序列引用、样式一次走到底（顺序就是文件写的顺序）
#[derive(Default)]
struct OdfScan {
    declared: Vec<String>,
    wrappers: Vec<Value>,
    marks: usize,
    mark_names: BTreeSet<String>,
    seq_refs: Vec<Value>,
    styles: Vec<(String, Option<String>, Option<String>)>,
}

fn odf_scan(node: &Node, part: &str, acc: &mut OdfScan) {
    match node.local() {
        "sequence-decl" => {
            acc.declared
                .push(node.attr_local("name").unwrap_or_default().to_string());
        }
        "sequence-decls" => acc.wrappers.push(json!({
            "part": part,
            "count": node.all("sequence-decl").len(),
        })),
        // 一枚 `text:bookmark` 可以把名字省了（这一族的 start/end 那一对容许没名字），
        // 那是「写了一枚没名字的书签」，不是「没写书签」，所以两本分交
        "bookmark" | "bookmark-start" | "bookmark-end" => {
            acc.marks += 1;
            if let Some(had) = node.attr_local("name") {
                acc.mark_names.insert(had.to_string());
            }
        }
        "sequence-ref" => acc.seq_refs.push(json!({
            "part": part,
            "name": node.attr_local("sequence-name").map(String::from),
        })),
        "style" => acc.styles.push((
            part.to_string(),
            node.attr_local("name").map(String::from),
            node.attr_local("family").map(String::from),
        )),
        _ => {}
    }
    for one in &node.children {
        odf_scan(one, part, acc);
    }
}

/// 题注与交叉引用那一份账（ODF）：目标写在属性上，序列声明这一层只有这里有
pub(crate) fn odf(root: &Node, styles: Option<&Node>, limit: usize) -> Value {
    let book = field_ledger::odf(root, styles, usize::MAX);
    if book["available"] != json!(true) {
        return json!({"family": "odf", "available": false});
    }
    // 页眉页脚住在 styles.xml，所以两份件都要走（与域那一本同一个范围、同一个先后的序）
    let mut parts: Vec<(&str, &Node)> = vec![("content.xml", root)];
    if let Some(sheet) = styles {
        parts.push(("styles.xml", sheet));
    }
    let mut scan = OdfScan::default();
    for (part, node) in &parts {
        odf_scan(node, part, &mut scan);
    }
    let mut uses: BTreeMap<String, u64> = BTreeMap::new();
    for (_part, node) in &parts {
        for one in node.descendants("p") {
            if let Some(had) = one.attr_local("style-name") {
                *uses.entry(had.to_string()).or_insert(0) += 1;
            }
        }
    }
    let mut style_names: BTreeSet<String> = BTreeSet::new();
    let mut caption_rows: Vec<Value> = Vec::new();
    for (part, named, family) in &scan.styles {
        if let Some(raw) = named {
            style_names.insert(raw.clone());
        }
        if !is_caption(named.as_deref()) {
            continue;
        }
        caption_rows.push(json!({
            "part": part,
            // 这一族一枚样式只有一个名字：`style:name` 既是声明名也是 id，没有两本可分
            "style_id": Value::Null,
            "declared_name": named,
            "matched_on": ["name"],
            "type": family,
            "paragraphs_using_it": named
                .as_deref()
                .and_then(|raw| uses.get(raw))
                .copied()
                .unwrap_or(0),
        }));
    }
    let declared: BTreeSet<String> = scan.declared.iter().cloned().collect();
    let mut rows: Vec<Value> = Vec::new();
    let mut used: BTreeSet<String> = BTreeSet::new();
    for one in book["rows"].as_array().into_iter().flatten() {
        let Some(kind) = one["kind"].as_str() else {
            continue;
        };
        let Some(which) = book_of_odf(kind) else {
            continue;
        };
        let target = attr_local_in(one, odf_target_attr(kind));
        let written = target.is_some();
        let resolves = target.as_deref().is_some_and(|raw| {
            if which == "bookmark" {
                scan.mark_names.iter().any(|one| one == raw)
            } else {
                declared.contains(raw)
            }
        });
        if which == "sequence" {
            if let Some(raw) = &target {
                used.insert(raw.clone());
            }
        }
        rows.push(json!({
            "part": one["part"].clone(),
            "index": one["index"].clone(),
            "kind": one["kind"].clone(),
            "element": one["element"].clone(),
            "target": target,
            "target_written": written,
            // 这一族没有引号这一问：目标是属性，属性值里没有「靠引号切开」这件事
            "target_written_quoted": Value::Null,
            "reference_format": attr_local_in(one, "reference-format"),
            "formula": attr_local_in(one, "formula"),
            "ref_name": attr_local_in(one, "ref-name"),
            "own_name": attr_local_in(one, "name"),
            "num_format": attr_local_in(one, "num-format"),
            "seq_sub_formula": attr_local_in(one, "seq-sub-formula"),
            "book": which,
            "resolves": resolves,
            "cached": one["cached"].clone(),
            // 这一族的缓存值就是元素体内那些字：没有「写了域却没结果」那一格
            "cache_written": true,
        }));
    }
    let declared_sorted: Vec<String> = declared.iter().cloned().collect();
    let unused: Vec<String> = declared_sorted
        .iter()
        .filter(|one| !used.contains(one.as_str()))
        .cloned()
        .collect();
    let orphan: Vec<String> = used
        .iter()
        .filter(|one| !declared.contains(one.as_str()))
        .cloned()
        .collect();
    let mark_names: Vec<String> = scan.mark_names.into_iter().collect();
    ledger(
        "odf",
        rows,
        json!({
            "bookmarks": mark_names,
            "bookmark_marks": scan.marks,
            "style_defs": scan.styles.len(),
            "style_ids": Value::Null,
            "style_names": style_names.len(),
            "sequences": declared_sorted,
        }),
        caption_ledger(caption_rows),
        json!({
            // 「声明盒子有几个、每个盒子里几条」与「一共几条声明」三问各交各的
            "elements": scan.declared.len(),
            "wrappers": scan.wrappers,
            "declared_unused": unused,
            "used_without_decl": orphan,
            "sequence_ref_elements": scan.seq_refs.len(),
            "sequence_ref_uses": scan.seq_refs,
        }),
        json!([]),
        limit,
    )
}

/// 题注与交叉引用那一份账（RTF）：指令那一串与 docx 逐字同形，书签名却已经解过 `\u`
pub(crate) fn rtf(
    field_rows: &[Value],
    styles: &[Value],
    style_uses: &[Value],
    bookmarks: &[String],
    bookmark_starts: usize,
    limit: usize,
) -> Value {
    let para: Vec<&Value> = styles
        .iter()
        .filter(|one| one["kind"] == json!("paragraph"))
        .collect();
    let mut style_names: BTreeSet<String> = BTreeSet::new();
    let mut caption_rows: Vec<Value> = Vec::new();
    for one in para.iter().copied() {
        let named = one["name"].as_str().map(String::from);
        // 空名字与没名字在这一族是同一句话（样式定义没名字时交的是空串），都不算一个名字
        if let Some(raw) = named.as_deref() {
            if !raw.is_empty() {
                style_names.insert(raw.to_string());
            }
        }
        if !is_caption(named.as_deref()) {
            continue;
        }
        let want = one["index"].clone();
        caption_rows.push(json!({
            "part": "stylesheet",
            "style_id": want,
            "declared_name": named,
            "matched_on": ["name"],
            "type": "paragraph",
            // 用了几个段按文件自己写的样式号配：`\sN` 的那一个 N 与段上写的 N
            "paragraphs_using_it": style_uses
                .iter()
                .rev()
                .find(|had| had["index"] == want)
                .and_then(|had| had["count"].as_u64())
                .unwrap_or(0),
        }));
    }
    let named: BTreeSet<String> = bookmarks.iter().cloned().collect();
    let mut rows: Vec<Value> = Vec::new();
    for (index, one) in field_rows.iter().enumerate() {
        let instruction = one["instruction"].as_str();
        let Some(kind) = field_kind(instruction) else {
            continue;
        };
        let Some(which) = book_of_instr(&kind) else {
            continue;
        };
        let (target, quoted, after, unterminated) = ref_target(instruction, &kind);
        let resolves = aims_at(which, target.as_deref(), &named, &style_names, None);
        rows.push(json!({
            "part": "stream",
            "index": index,
            "kind": kind,
            "instruction": one["instruction"].clone(),
            "target": target,
            "target_written_quoted": quoted,
            "next_token_is_switch": after,
            "target_unterminated": unterminated,
            "book": which,
            "resolves": resolves,
            "cached": one["cached"].clone(),
            "cache_written": one["has_result"] == json!(true),
        }));
    }
    ledger(
        "rtf",
        rows,
        json!({
            "bookmarks": named.into_iter().collect::<Vec<String>>(),
            "bookmark_marks": bookmark_starts,
            "style_defs": para.len(),
            // `\sN` 那个号不是声明名：STYLEREF 指令里写的从来是名字
            "style_ids": Value::Null,
            "style_names": style_names.len(),
            "sequences": Value::Null,
        }),
        caption_ledger(caption_rows),
        Value::Null,
        json!(["这一族也没有序列声明这一层：SEQ 的 resolves 一律 null"]),
        limit,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 自己打一个「存储」（不压缩）的包：这一本要的几种形状（书签在页脚、样式表只写
    /// 两条、引号开了没关）在 155 份真件里凑不齐一份
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
    const T_ATTR: &str =
        "xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\" xmlns:style=\"urn:oasis:names:tc:opendocument:xmlns:style:1.0\"";

    /// 正文 + 样式表（+ 页脚）三份部件打成一份 docx；样式表按在场与否传进去，
    /// 所以 `sheet` 为 None 就是「这个部件根本没有」
    fn docx_of(body: &str, sheet: Option<&str>, foot: Option<&str>, limit: usize) -> Value {
        let mut parts: Vec<(&str, &str)> = vec![("word/document.xml", body)];
        if let Some(one) = sheet {
            parts.push(("word/styles.xml", one));
        }
        if let Some(one) = foot {
            parts.push(("word/footer1.xml", one));
        }
        let bytes = packed(&parts);
        let root = xmlscan::parse_str(body);
        let document = root.child("document").unwrap_or(&root);
        let sheet_node = sheet.map(xmlscan::parse_str);
        docx(&bytes, document, sheet_node.as_ref(), limit)
    }

    /// SEQ 没有声明层可查：`resolves` 交 null 而不是 false，而题注样式那两本各查各的
    #[test]
    fn a_sequence_number_in_ooxml_has_no_book_to_look_in() {
        let mine = docx_of(
            &format!(
                "<w:document {W}><w:body><w:p><w:fldSimple w:instr=\" SEQ 图 \\* ARABIC\">\
                 <w:r><w:t>1</w:t></w:r></w:fldSimple></w:p></w:body></w:document>",
                W = W_ATTR
            ),
            Some(&format!(
                "<w:styles {W}><w:style w:type=\"paragraph\" w:styleId=\"Caption\">\
                 <w:name w:val=\"caption\"/></w:style></w:styles>",
                W = W_ATTR
            )),
            None,
            100,
        );
        // 这个助手得真的写出一条域：少了它下面每条断言读的都是「一条也没有」那一份答案
        assert_eq!(mine["target_rows"], json!(1), "助手没写出域：{mine}");
        assert_eq!(mine["family"], json!("ooxml"), "{mine}");
        assert_eq!(
            mine["resolves"],
            json!({"true": 0, "false": 0, "null": 1}),
            "这一族没有那本书，所以不是 false：{mine}"
        );
        assert_eq!(mine["kinds"], json!({"SEQ": 1}), "{mine}");
        assert_eq!(mine["target_books"], json!({"sequence": 1}), "{mine}");
        assert_eq!(mine["books"]["sequences"], Value::Null, "{mine}");
        assert_eq!(mine["declarations"], Value::Null, "{mine}");
        assert_eq!(mine["books"]["style_defs"], json!(1), "{mine}");
        assert_eq!(mine["books"]["style_ids"], json!(1), "{mine}");
        assert_eq!(mine["books"]["style_names"], json!(1), "{mine}");
        assert_eq!(
            mine["caption_styles"]["rows"][0]["matched_on"],
            json!(["styleId", "name"]),
            "两个名字大小写不同，所以两本都查：{mine}"
        );
        assert_eq!(
            mine["caption_styles"]["paragraphs_using_them"],
            json!(0),
            "声明了没人用是本族常态：{mine}"
        );
        assert_eq!(mine["rows"][0]["target"], json!("图"), "{mine}");
        assert_eq!(
            mine["rows"][0]["next_token_is_switch"],
            json!(false),
            "{mine}"
        );
        assert_eq!(mine["cache_values"], json!({"1": 1}), "{mine}");
    }

    /// 书签住在页脚里，正文那条 REF 照样成立；只扫正文就会替文件编一条坏引用
    #[test]
    fn a_bookmark_written_in_the_footer_still_opens_the_reference() {
        let mine = docx_of(
            &format!(
                "<w:document {W}><w:body>\
                 <w:p><w:fldSimple w:instr=\" REF 表锚点 \\h\"><w:r><w:t>1</w:t></w:r></w:fldSimple></w:p>\
                 <w:p><w:fldSimple w:instr=\" REF 没有那枚\"><w:r><w:t>错误: 引用源未找到</w:t></w:r></w:fldSimple></w:p>\
                 <w:p><w:fldSimple w:instr=' STYLEREF \"标题 1\"'><w:r><w:t>标题</w:t></w:r></w:fldSimple></w:p>\
                 </w:body></w:document>",
                W = W_ATTR
            ),
            Some(&format!(
                "<w:styles {W}><w:style w:type=\"paragraph\" w:styleId=\"Heading1\">\
                 <w:name w:val=\"heading 1\"/></w:style></w:styles>",
                W = W_ATTR
            )),
            Some(&format!(
                "<w:ftr {W}><w:p><w:bookmarkStart w:id=\"0\" w:name=\"表锚点\"/></w:p></w:ftr>",
                W = W_ATTR
            )),
            100,
        );
        assert_eq!(mine["target_rows"], json!(3), "{mine}");
        assert_eq!(mine["books"]["bookmark_marks"], json!(1), "{mine}");
        assert_eq!(mine["books"]["bookmarks"], json!(["表锚点"]), "{mine}");
        assert_eq!(
            mine["resolves"],
            json!({"true": 1, "false": 2, "null": 0}),
            "页脚里那枚书签要认得：{mine}"
        );
        assert_eq!(
            mine["quoted"],
            json!({"true": 1, "false": 2, "null": 0}),
            "{mine}"
        );
        // 引用不成立、缓存值却在：这一对就是要交的形状
        assert_eq!(mine["cached_but_unresolved"], json!(2), "{mine}");
        assert_eq!(mine["resolving_but_no_cache"], json!(0), "{mine}");
        assert_eq!(
            mine["cache_values"],
            json!({"1": 1, "错误: 引用源未找到": 1, "标题": 1}),
            "缓存值按文件写的那串字原样交，不认语义：{mine}"
        );
        // 名字里带空格只能靠引号切开；它查的是声明名那本书，`标题 1` 不在库里
        assert_eq!(mine["rows"][2]["target"], json!("标题 1"), "{mine}");
        assert_eq!(
            mine["rows"][2]["target_written_quoted"],
            json!(true),
            "{mine}"
        );
        assert_eq!(mine["rows"][2]["resolves"], json!(false), "{mine}");
        assert_eq!(
            mine["rows"][0]["part"],
            json!("word/document.xml"),
            "{mine}"
        );
    }

    /// 序列查得到声明、书签引用查名字，两本各判各的；「声明了没人用」单列一本
    #[test]
    fn odf_targets_are_attributes_and_the_declaration_book_exists() {
        let content = format!(
            "<office:document-content {T}><office:body><office:text>\
             <text:sequence-decls><text:sequence-decl text:display-outline-level=\"0\" text:name=\"图\"/></text:sequence-decls>\
             <text:p><text:bookmark text:name=\"表锚点\"/><text:sequence text:name=\"图\" text:formula=\"ooow:图+1\" text:ref-name=\"ref图0\" text:num-format=\"1\">1</text:sequence>\
             <text:bookmark-ref text:reference-format=\"page\" text:ref-name=\"表锚点\">3</text:bookmark-ref></text:p>\
             <text:p><text:bookmark-ref text:ref-name=\"没有那枚\">x</text:bookmark-ref></text:p>\
             </office:text></office:body></office:document-content>",
            T = T_ATTR
        );
        let sheet = format!(
            "<office:document-styles {T}><office:styles>\
             <style:style style:name=\"Caption\" style:family=\"paragraph\"/>\
             </office:styles></office:document-styles>",
            T = T_ATTR
        );
        let root = xmlscan::parse_str(&content);
        let styles = xmlscan::parse_str(&sheet);
        let mine = odf(&root, Some(&styles), 100);
        assert_eq!(mine["family"], json!("odf"), "{mine}");
        assert_eq!(mine["target_rows"], json!(3), "{mine}");
        assert_eq!(
            mine["kinds"],
            json!({"sequence": 1, "bookmark-ref": 2}),
            "{mine}"
        );
        assert_eq!(
            mine["resolves"],
            json!({"true": 2, "false": 1, "null": 0}),
            "这一族的 resolves 是真能判的：{mine}"
        );
        assert_eq!(
            mine["quoted"]["null"],
            json!(3),
            "属性没有引号这一问：{mine}"
        );
        assert_eq!(mine["books"]["bookmark_marks"], json!(1), "{mine}");
        assert_eq!(mine["books"]["bookmarks"], json!(["表锚点"]), "{mine}");
        assert_eq!(mine["books"]["sequences"], json!(["图"]), "{mine}");
        assert_eq!(mine["books"]["style_ids"], Value::Null, "{mine}");
        assert_eq!(mine["declarations"]["elements"], json!(1), "{mine}");
        assert_eq!(
            mine["declarations"]["wrappers"],
            json!([{"part": "content.xml", "count": 1}]),
            "{mine}"
        );
        assert_eq!(mine["declarations"]["declared_unused"], json!([]), "{mine}");
        assert_eq!(
            mine["declarations"]["used_without_decl"],
            json!([]),
            "{mine}"
        );
        assert_eq!(
            mine["declarations"]["sequence_ref_elements"],
            json!(0),
            "认字表里没有 text:sequence-ref，所以这一格只交零计数：{mine}"
        );
        assert_eq!(mine["caption_styles"]["declared"], json!(1), "{mine}");
        assert_eq!(
            mine["caption_styles"]["rows"][0]["part"],
            json!("styles.xml"),
            "题注样式住在另一个部件里：{mine}"
        );
        assert!(
            mine["caption_styles"].get("styles_part").is_none(),
            "那一格只属于 OOXML：{mine}"
        );
        assert_eq!(mine["rows"][0]["own_name"], json!("图"), "{mine}");
        assert_eq!(mine["rows"][0]["formula"], json!("ooow:图+1"), "{mine}");
        assert_eq!(mine["rows"][1]["reference_format"], json!("page"), "{mine}");
        assert_eq!(mine["rows"][1]["target"], json!("表锚点"), "{mine}");
        assert_eq!(mine["rows"][1]["own_name"], Value::Null, "{mine}");
        assert_eq!(mine["rows"][2]["resolves"], json!(false), "{mine}");
    }

    /// 三族共用的那把切目标的尺子：null、`""`、开关、未闭合四种答案各是各的
    #[test]
    fn the_target_splitter_keeps_four_different_answers() {
        assert_eq!(
            ref_target(Some(" SEQ 图 \\* ARABIC"), "SEQ"),
            (Some("图".to_string()), false, false, false)
        );
        assert_eq!(
            ref_target(Some(" STYLEREF \"标题 1\""), "STYLEREF"),
            (Some("标题 1".to_string()), true, false, false)
        );
        assert_eq!(
            ref_target(Some(" STYLEREF \"\""), "STYLEREF"),
            (Some(String::new()), true, false, false)
        );
        assert_eq!(
            ref_target(Some(" STYLEREF \"没关"), "STYLEREF"),
            (Some("没关".to_string()), true, false, true)
        );
        // 种类词后面直接是开关：目标没写，而「没写」不等于「写了空串」
        assert_eq!(
            ref_target(Some(" REF \\h"), "REF"),
            (None, false, true, false)
        );
        assert_eq!(ref_target(Some(" SEQ"), "SEQ"), (None, false, false, false));
        assert_eq!(ref_target(None, "REF"), (None, false, false, false));
        // 引号里的空格留着、引号外的只到第一个空格
        assert_eq!(
            ref_target(Some("REF \"a b\" \\h"), "REF"),
            (Some("a b".to_string()), true, false, false)
        );
    }

    /// RTF：目标与 docx 同一个形状，书签名已经解过 `\u`，样式号只按文件自己写的交
    #[test]
    fn rtf_looks_up_three_books_and_keeps_its_own_style_numbers() {
        let mine = rtf(
            &[
                json!({"index": 0, "instruction": "REF 表锚点 \\h", "has_result": true, "cached": "1"}),
                json!({"index": 1, "instruction": "SEQ 表 \\* ARABIC", "has_result": true, "cached": "2"}),
                json!({"index": 2, "instruction": "STYLEREF \"标题 1\"", "has_result": true, "cached": "错误: 引用源未找到"}),
                json!({"index": 3, "instruction": " PAGE ", "has_result": true, "cached": "7"}),
            ],
            &[
                json!({"kind": "paragraph", "index": 55, "name": "caption"}),
                json!({"kind": "character", "index": 24, "name": "emphasized"}),
            ],
            &[json!({"index": 55, "name": "caption", "count": 3})],
            &["表锚点".to_string()],
            1,
            100,
        );
        assert_eq!(mine["family"], json!("rtf"), "{mine}");
        assert_eq!(
            mine["target_rows"],
            json!(3),
            "PAGE 不在这本账的问题范围里：{mine}"
        );
        assert_eq!(
            mine["resolves"],
            json!({"true": 1, "false": 1, "null": 1}),
            "{mine}"
        );
        assert_eq!(
            mine["books"]["style_defs"],
            json!(1),
            "只数段落样式：{mine}"
        );
        assert_eq!(mine["books"]["style_names"], json!(1), "{mine}");
        assert_eq!(mine["books"]["sequences"], Value::Null, "{mine}");
        assert_eq!(mine["caption_styles"]["declared"], json!(1), "{mine}");
        assert_eq!(
            mine["caption_styles"]["rows"][0]["style_id"],
            json!(55),
            "样式号只按文件自己写的交：{mine}"
        );
        assert_eq!(
            mine["caption_styles"]["paragraphs_using_them"],
            json!(3),
            "{mine}"
        );
        assert_eq!(mine["rows"][2]["resolves"], json!(false), "{mine}");
        assert_eq!(mine["rows"][0]["part"], json!("stream"), "{mine}");
        assert_eq!(mine["cached_but_unresolved"], json!(1), "{mine}");
        assert_eq!(mine["cache_missing"], json!(0), "{mine}");
    }

    /// 截行不截账：`limit` 只砍 `rows`，四条计数仍按全部行算
    #[test]
    fn a_cut_row_book_still_counts_every_row() {
        let body = format!(
            "<w:document {W}><w:body>\
             <w:p><w:fldSimple w:instr=\" SEQ 图 \\* ARABIC\"><w:r><w:t>1</w:t></w:r></w:fldSimple></w:p>\
             <w:p><w:fldSimple w:instr=\" SEQ 表 \\* ARABIC\"><w:r><w:t>2</w:t></w:r></w:fldSimple></w:p>\
             </w:body></w:document>",
            W = W_ATTR
        );
        let mine = docx_of(&body, None, None, 100);
        assert_eq!(mine["target_rows"], json!(2), "{mine}");
        assert_eq!(
            mine["caption_styles"]["styles_part"],
            json!(false),
            "{mine}"
        );
        let cut = docx_of(&body, None, None, 1);
        assert_eq!(cut["target_rows"], json!(2), "{cut}");
        assert_eq!(cut["listed"], json!(1), "{cut}");
        assert_eq!(cut["cut"], json!(true), "{cut}");
        assert_eq!(cut["rows"].as_array().map(Vec::len), Some(1), "{cut}");
        assert_eq!(cut["kinds"], json!({"SEQ": 2}), "账不跟着行一起截：{cut}");
    }
}
