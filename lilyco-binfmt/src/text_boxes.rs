//! 「文档里有几个文本框、框里写了什么」——一家的框在 `w:drawing`（DrawingML）与 `w:pict`
//! （VML）两种容器里各写一遍，一家的框是一个 `draw:frame` 里套一个 `draw:text-box`
//!
//! ## 为什么这一问值得单独一本账
//!
//! 框里的字不是正文的字 —— 它与批注、脚注同一类：一个框自己带段，所以「这份文档有几段」有两个
//! 答案（正文那些、与整棵树数出来的）。而页面上只出现一次的那些字，在 OOXML 的件里可以**存两份**
//! （DrawingML 与 VML 两个分支各一份、内容一字不差）：把「几份字」当「几个框」就把同一句话说成了
//! 两遍。所以这里 `text_boxes`（那份格子元素的数）、`distinct_texts`（说了几句不同的话）与
//! `paragraphs_direct_of_body` / `paragraphs_anywhere` 三个数各记各的，不合成。
//!
//! ## 两家实测的形状（`tbox.odt` 与 LibreOffice 转出的 `tbox.docx`）
//!
//! OOXML 一个框写两种容器：`w:drawing` → `wp:inline`（尺寸在它下面的 `wp:extent cx/cy`，EMU）
//! → … → `w:txbx` → `w:txbxContent`；以及 `w:pict` → `v:shape` → `v:textbox` → 又一个
//! `w:txbxContent`。实测这份件里 `drawing 1 / pict 1 / txbxContent 2` 而两句字一模一样 ——
//! 所以聚合里 `boxes_total` 是 2、`distinct_text_count` 是 1。
//! 尺寸**只有 DrawingML 那一份写了**（`cx="1800225" cy="864235"`），VML 那一份的 `v:shape`
//! 连 `style` 都没有 → `shape_style: null`。这不是"它把尺寸丢了"（那是另一问），而是两份副本
//! 自己就不一样：按写的交，一处有一处没有都如实交出去。
//!
//! ODF 是另一种形状：`draw:frame` 带 `draw:name`、`text:anchor-type`、`svg:width/height`（这一族
//! 的尺寸是**自带单位的串**），字在里面那个 `draw:text-box`。LibreOffice 把同一份 odt 重写一遍时
//! 会挂上 `draw:style-name="Frame"`、**把 `svg:x`/`svg:y` 与 `draw:z-index` 整个丢掉**、并把
//! `5cm` / `2.4cm` 换成 `5.001cm` / `2.401cm` —— 写了什么就是什么，不替它接回来。
//!
//! ## 为什么 RTF 不交这个键
//!
//! LibreOffice 的 RTF 导出里既没有 `SHAPPIE` 也没有 `\pict`（实测各 0 次），框这个形状在那条流里
//! 根本不存在，字直接落进正文段落 —— 与制表位、行距、段边框同一族教训：判不出「这一段在框里」，
//! 这一支就不交这个键（缺键 = 这一族没看，不是 0）。

use crate::xmlscan::{self, Node};
use serde_json::{json, Value};

/// 一个容器往下数：里面几份「写字的格子」，格子们自己带了几段（直接孩子 / 整棵树），以及说了什么
fn box_ledger(holder: &Node, want: &str) -> (usize, usize, usize, Vec<String>) {
    let boxes = holder.descendants(want);
    let mut direct = 0usize;
    let mut anywhere = 0usize;
    let mut parts: Vec<String> = Vec::new();
    for one in boxes.iter() {
        direct += one.children.iter().filter(|kid| kid.local() == "p").count();
        anywhere += one.descendants("p").len();
        let words = xmlscan::inline_text(one);
        if !words.is_empty() {
            parts.push(words);
        }
    }
    (boxes.len(), direct, anywhere, parts)
}

/// 一句话第一次出现才收下（重复的那些就是同一句话又写了一份）
fn keep(seen: &mut Vec<String>, words: &str) {
    if !words.is_empty() && !seen.iter().any(|had| had == words) {
        seen.push(words.to_string());
    }
}

/// OOXML 那一份：两种容器各走一条 list（同一棵树上两类容器排不出同一个序，就不硬排）
pub(crate) fn docx(body: &Node, limit: usize) -> Value {
    let mut rows: Vec<Value> = Vec::new();
    let mut boxes_total = 0usize;
    let mut drawings = 0usize;
    let mut picts = 0usize;
    let mut texts: Vec<String> = Vec::new();
    let mut text_p = 0usize;
    let mut text_any = 0usize;
    for (index, holder) in body.descendants("drawing").into_iter().enumerate() {
        let (count, direct, anywhere, parts) = box_ledger(holder, "txbxContent");
        let joined = parts.join(" / ");
        boxes_total += count;
        text_p += direct;
        text_any += anywhere;
        if count > 0 {
            drawings += 1;
            keep(&mut texts, &joined);
        }
        rows.push(json!({
            "kind": "drawing",
            "index": index,
            "text_boxes": count,
            "paragraphs_direct": direct,
            "paragraphs_anywhere": anywhere,
            "text": joined,
            "anchor_element": holder.descendants("anchor").into_iter().next().map(|had| had.local().to_string()),
            "inline_element": holder.descendants("inline").into_iter().next().map(|had| had.local().to_string()),
            "extent_written": holder.descendants("extent").into_iter().next().map(|had| json!({
                "cx": had.attr_local("cx").map(String::from),
                "cy": had.attr_local("cy").map(String::from),
            })),
        }));
    }
    for (index, holder) in body.descendants("pict").into_iter().enumerate() {
        let (count, direct, anywhere, parts) = box_ledger(holder, "txbxContent");
        let joined = parts.join(" / ");
        boxes_total += count;
        text_p += direct;
        text_any += anywhere;
        if count > 0 {
            picts += 1;
            keep(&mut texts, &joined);
        }
        rows.push(json!({
            "kind": "pict",
            "index": index,
            "text_boxes": count,
            "paragraphs_direct": direct,
            "paragraphs_anywhere": anywhere,
            "text": joined,
            "textbox_elements": holder.descendants("textbox").len(),
            "shape_style": holder
                .descendants("shape")
                .into_iter()
                .next()
                .and_then(|had| had.attr_local("style"))
                .map(String::from),
        }));
    }
    json!({
        "family": "ooxml",
        "available": true,
        "boxes_total": boxes_total,
        "drawings_with_boxes": drawings,
        "picts_with_boxes": picts,
        "distinct_text_count": texts.len(),
        "distinct_texts": texts,
        "paragraphs_direct_of_body": body
            .children
            .iter()
            .filter(|kid| kid.local() == "p")
            .count(),
        "paragraphs_anywhere": body.descendants("p").len(),
        "paragraphs_in_boxes_direct": text_p,
        "paragraphs_in_boxes_anywhere": text_any,
        "boxes": rows.into_iter().take(limit).collect::<Vec<Value>>(),
    })
}

/// ODF 那一份：框是 `draw:frame`，字在里面那个 `draw:text-box`
pub(crate) fn odf(content: &Node, limit: usize) -> Value {
    let mut rows: Vec<Value> = Vec::new();
    let frames = content.descendants("frame");
    let frames_total = frames.len();
    let mut with_boxes = 0usize;
    let mut box_total = 0usize;
    let mut texts: Vec<String> = Vec::new();
    let mut text_p = 0usize;
    let mut text_any = 0usize;
    for (index, holder) in frames.iter().enumerate() {
        let (count, direct, anywhere, parts) = box_ledger(holder, "text-box");
        let joined = parts.join(" / ");
        box_total += count;
        text_p += direct;
        text_any += anywhere;
        if count > 0 {
            with_boxes += 1;
            keep(&mut texts, &joined);
        }
        rows.push(json!({
            "kind": "frame",
            "index": index,
            "text_boxes": count,
            "paragraphs_direct": direct,
            "paragraphs_anywhere": anywhere,
            "text": joined,
            "name_written": holder.attr_local("name").map(String::from),
            "style_written": holder.attr_local("style-name").map(String::from),
            "anchor_written": holder.attr_local("anchor-type").map(String::from),
            "width_written": holder.attr_local("width").map(String::from),
            "height_written": holder.attr_local("height").map(String::from),
            "x_written": holder.attr_local("x").map(String::from),
            "y_written": holder.attr_local("y").map(String::from),
            "z_index_written": holder.attr_local("z-index").map(String::from),
        }));
    }
    // 「正文有几段」这一族不能靠两跳 `child("body")?.child("text")`：那一条路在本读者里取不到
    // 节点（CI 量出来是 0，而标准库读者从 XML 的真实根数到 3）。改成按局部名找一个**真有 `p`
    // 孩子**的 `text` 元素 —— 两个读者从各自的根出发也走到同一个节点。
    let direct_body = content
        .descendants("text")
        .into_iter()
        .find(|had| had.children.iter().any(|kid| kid.local() == "p"))
        .map(|had| had.children.iter().filter(|kid| kid.local() == "p").count())
        .unwrap_or(0);
    json!({
        "family": "odf",
        "available": true,
        "frames_total": frames_total,
        "frames_with_boxes": with_boxes,
        "text_box_elements": box_total,
        "distinct_text_count": texts.len(),
        "distinct_texts": texts,
        "paragraphs_direct_of_text": direct_body,
        "paragraphs_anywhere": content.descendants("p").len(),
        "paragraphs_in_boxes_direct": text_p,
        "paragraphs_in_boxes_anywhere": text_any,
        "boxes": rows.into_iter().take(limit).collect::<Vec<Value>>(),
    })
}
