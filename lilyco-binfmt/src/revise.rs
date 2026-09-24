//! 修订（tracked changes）这一份账：「谁在什么时候、改了哪一段的什么」。
//!
//! OOXML 与 ODF 各存各的，两边的坑不一样，所以两套走法、一份账：
//! 1. **一次编辑不等于一个元素**。LibreOffice 写 OOXML 会把「插入 124000 元」拆成两个
//!    `w:ins`（数字与单位各一条），只按元素数就报成「两处插入」。所以除了元素个数，还要把
//!    **相邻**且同（类型 + 作者 + 时间 + 所在段）的元素并成一条逻辑改动。这条合并规则不是我
//!    自己拍的：同一份文件 LibreOffice 自己导出的 ODF 里是 4 个 `text:changed-region`，
//!    与合并后的条数、类型、作者、时间逐条一致（fixture README 里记着这次实测）。
//! 2. **段落标记自己也可以被插入或删除**（`w:pPr/w:rPr/w:ins`）。LibreOffice 导出 OOXML 时
//!    把它丢了（正文那条还在），python-docx 手写的样本里却有 —— 所以单列 `paragraph_marks`，
//!    不跟正文那条混在一起数。
//! 3. ODF 反过来：删掉的文字存在 region 里面，插入的文字存在正文里 `change-start` 与
//!    `change-end` 之间。要取插入的字就得按顺序走正文，靠的是 xmlscan 把直接文本存成
//!    `#text` 子节点（顺序保住了）—— 拼成一份 `direct` 就只能拿到「前后中」。

use serde_json::{json, Value};
use std::collections::BTreeMap;

use crate::odsheet::attr_of;
use crate::xmlscan::Node;

/// 一条逻辑改动（OOXML 侧相邻同类型同作者同时间的元素已合并）
#[derive(Debug, Clone)]
pub struct Change {
    pub kind: &'static str,
    pub author: String,
    pub date: String,
    /// 在第几段（与调用方数段落用的是同一份列表）；ODF 里找不到引用就是 None
    pub paragraph: Option<usize>,
    pub text: String,
    /// 这条逻辑改动吃掉几个原始元素：生产者把一次编辑拆开时大于 1
    pub elements: usize,
    /// 改的是段落标记本身（`w:pPr/w:rPr` 里那条），不是段里的字
    pub mark: bool,
}

/// 一份账
#[derive(Debug, Default)]
pub struct Ledger {
    pub changes: Vec<Change>,
    pub insertions: usize,
    pub deletions: usize,
    pub format_changes: usize,
    pub moves: usize,
    pub marks: usize,
    /// 文件自己有没有开着「继续记录修订」；没有那个设置就是 None
    pub track_changes: Option<bool>,
    pub notes: Vec<String>,
}

/// 一个原始元素（合并之前）
struct Raw {
    kind: &'static str,
    author: String,
    date: String,
    text: String,
    paragraph: usize,
    mark: bool,
}

impl Ledger {
    fn count(&mut self, kind: &str, mark: bool) {
        match kind {
            "insertion" => self.insertions += 1,
            "deletion" => self.deletions += 1,
            "format-change" => self.format_changes += 1,
            _ => self.moves += 1,
        }
        if mark {
            self.marks += 1;
        }
    }

    fn push_raw(&mut self, one: Raw) {
        self.count(one.kind, one.mark);
        let merged = match self.changes.last_mut() {
            Some(last)
                if last.kind == one.kind
                    && last.author == one.author
                    && last.date == one.date
                    && last.mark == one.mark
                    && last.paragraph == Some(one.paragraph) =>
            {
                last.elements += 1;
                last.text.push_str(&one.text);
                true
            }
            _ => false,
        };
        if !merged {
            self.changes.push(Change {
                kind: one.kind,
                author: one.author,
                date: one.date,
                paragraph: Some(one.paragraph),
                text: one.text,
                elements: 1,
                mark: one.mark,
            });
        }
    }

    fn finish(&mut self) {
        let elements = self.insertions + self.deletions + self.format_changes + self.moves;
        if elements > self.changes.len() {
            self.notes.push(format!(
                "{elements} 个修订元素合成 {} 条逻辑改动：生产者会把一次编辑拆进几个 run，\
                 相邻且同类型、同作者、同时间、同一段的才合",
                self.changes.len()
            ));
        }
    }

    pub fn to_json(&self, limit: usize) -> Value {
        let mut authors: BTreeMap<&str, usize> = BTreeMap::new();
        for one in &self.changes {
            *authors.entry(one.author.as_str()).or_insert(0) += 1;
        }
        json!({
            "changes": self.changes.iter().enumerate().take(limit).map(|(index, one)| json!({
                "index": index,
                "kind": one.kind,
                "author": one.author,
                "date": one.date,
                "paragraph": one.paragraph,
                "text": one.text,
                "elements": one.elements,
                "paragraph_mark": one.mark,
            })).collect::<Vec<Value>>(),
            "changes_total": self.changes.len(),
            "changes_listed": self.changes.len().min(limit),
            "elements": {
                "insertions": self.insertions,
                "deletions": self.deletions,
                "format_changes": self.format_changes,
                "moves": self.moves,
            },
            "paragraph_marks": self.marks,
            "authors": authors.iter().map(|(name, hits)| json!({
                "name": name, "changes": hits,
            })).collect::<Vec<Value>>(),
            "track_changes": self.track_changes,
            "notes": self.notes,
        })
    }
}

/// OOXML 里哪些名字算修订、算哪一类（表格那几条与字符级的分不开，只能靠名字）
fn docx_kind(local: &str) -> Option<&'static str> {
    Some(match local {
        "ins" | "cellIns" | "rowIns" => "insertion",
        "del" | "cellDel" | "rowDel" => "deletion",
        "moveFrom" | "moveTo" => "move",
        "rPrChange" | "pPrChange" | "tblPrChange" | "trPrChange" | "tcPrChange"
        | "sectPrChange" | "numberingChange" | "cellMerge" => "format-change",
        _ => return None,
    })
}

/// 一段修订带进来的字：只收 `w:t` 与 `w:delText`，按文档顺序
fn collect_docx_text(node: &Node, out: &mut String) {
    if matches!(node.local(), "t" | "delText") {
        out.push_str(&node.text());
        return;
    }
    for one in &node.children {
        collect_docx_text(one, out);
    }
}

fn walk_docx(node: &Node, paragraph: usize, mark: bool, out: &mut Vec<Raw>) {
    for one in &node.children {
        let local = one.local();
        // 嵌套的段落（文本框里那种）按它自己的序号算，这里不重复收
        if local == "p" {
            continue;
        }
        if let Some(kind) = docx_kind(local) {
            let mut text = String::new();
            if kind != "format-change" {
                collect_docx_text(one, &mut text);
            }
            out.push(Raw {
                kind,
                author: one.attr_local("author").unwrap_or_default().to_string(),
                date: one.attr_local("date").unwrap_or_default().to_string(),
                text,
                paragraph,
                mark,
            });
        }
        // 段落标记的修订住在 pPr 的 rPr 里：下去的时候要带着这个记号
        walk_docx(one, paragraph, mark || local == "pPr", out);
    }
}

/// docx：`paragraphs` 要与调用方数段落用的同一份列表，序号才对得上
pub fn docx_ledger(paragraphs: &[&Node], settings: Option<&Node>) -> Ledger {
    let mut raws: Vec<Raw> = Vec::new();
    for (index, one) in paragraphs.iter().enumerate() {
        walk_docx(one, index, false, &mut raws);
    }
    let mut ledger = Ledger::default();
    for one in raws {
        ledger.push_raw(one);
    }
    ledger.track_changes = settings.map(|one| !one.descendants("trackChanges").is_empty());
    ledger.finish();
    ledger
}

/// ODF 的 region：类型认得出来就归四类，认不出来照实记下局部名
fn odt_region(node: &Node) -> Option<&'static str> {
    let local = node.local();
    Some(match local {
        "insertion" => "insertion",
        "deletion" => "deletion",
        "format-change" => "format-change",
        "move-from" | "move-to" => "move",
        _ => return None,
    })
}

/// region 里自己带的字（删掉的那些在里面），但要跳过 `change-info` ——
/// 作者与时间就坐在那段字中间，不跳就会把「89000 元」读成「89000 元李四2026-…」
fn odt_own_text(node: &Node, out: &mut String) {
    let local = node.local();
    if local == "change-info" {
        return;
    }
    if local == "#text" {
        out.push_str(&node.direct);
        return;
    }
    for one in &node.children {
        odt_own_text(one, out);
    }
}

/// 正文里的引用：`change` 是一个点（删除、格式改动标在它作用的位置），
/// `change-start` / `change-end` 之间夹着的就是插入的字
fn walk_odt_body(node: &Node, open: &mut Vec<String>, hits: &mut OdtRefs) {
    let local = node.local();
    if local == "#text" {
        for id in open.iter() {
            if let Some(kept) = hits.ranges.get_mut(id) {
                kept.push_str(&node.direct);
            }
        }
        return;
    }
    if matches!(local, "change" | "change-start" | "change-end") {
        let Some(id) = attr_of(node, "change-id") else {
            return;
        };
        let id = id.to_string();
        let here = hits.paragraph;
        hits.at.entry(id.clone()).or_insert(here);
        match local {
            "change-start" => {
                hits.ranges.entry(id.clone()).or_default();
                open.push(id);
            }
            "change-end" => {
                if let Some(hit) = open.iter().position(|one| one == &id) {
                    open.remove(hit);
                }
            }
            _ => {}
        }
        return;
    }
    for one in &node.children {
        walk_odt_body(one, open, hits);
    }
}

/// 一遍正文走出来的两张表：每个 id 落在第几段、区间的字是什么
#[derive(Default)]
struct OdtRefs {
    at: BTreeMap<String, usize>,
    ranges: BTreeMap<String, String>,
    paragraph: usize,
}

/// odt：`text_body` 是 `office:text`（`text:tracked-changes` 挂在它下面），
/// `paragraphs` 是调用方那份正文段落
pub fn odt_ledger(text_body: &Node, paragraphs: &[&Node]) -> Ledger {
    let mut refs = OdtRefs::default();
    for (index, one) in paragraphs.iter().enumerate() {
        refs.paragraph = index;
        let mut open: Vec<String> = Vec::new();
        walk_odt_body(one, &mut open, &mut refs);
    }
    let mut found: Vec<Change> = Vec::new();
    let mut ledger = Ledger::default();
    for region in text_body.descendants("changed-region") {
        let Some(kind) = region.children.iter().find_map(|one| odt_region(one)) else {
            continue;
        };
        // 两个 id 属性（xml:id 与 text:id）在 LibreOffice 写的文件里是同一个值
        let id = attr_of(region, "id").unwrap_or_default().to_string();
        let mut own = String::new();
        for one in &region.children {
            odt_own_text(one, &mut own);
        }
        let from_body = refs.ranges.get(&id).cloned().unwrap_or_default();
        ledger.count(kind, false);
        found.push(Change {
            kind,
            author: region
                .descendants("creator")
                .into_iter()
                .next()
                .map(|one| one.text())
                .unwrap_or_default(),
            date: region
                .descendants("date")
                .into_iter()
                .next()
                .map(|one| one.text())
                .unwrap_or_default(),
            paragraph: refs.at.get(&id).copied(),
            text: if own.is_empty() { from_body } else { own },
            elements: 1,
            mark: false,
        });
    }
    found.sort_by(|a, b| {
        a.paragraph
            .unwrap_or(usize::MAX)
            .cmp(&b.paragraph.unwrap_or(usize::MAX))
    });
    ledger.changes = found;
    ledger.track_changes = text_body
        .all("tracked-changes")
        .into_iter()
        .next()
        .and_then(|one| attr_of(one, "track-changes"))
        .map(|one| one == "true");
    if ledger.changes.is_empty() && !refs.at.is_empty() {
        ledger.notes.push(
            "正文里有 change 引用，却没有对应的 changed-region —— 那份文件没把修订表写全"
                .to_string(),
        );
    }
    ledger
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(src: &str) -> Node {
        crate::xmlscan::parse_str(src)
    }

    /// 正文段落那份列表：与 office-doc 用的是同一个函数（region 里那份删掉的段落不算）
    fn odt_body(node: &Node) -> Vec<&Node> {
        let mut out: Vec<&Node> = Vec::new();
        crate::office_text::odf_paragraphs(node, &mut out);
        out
    }

    /// 一次编辑可能被写成几个元素：相邻、同类型、同作者、同时间、同一段才并成一条，
    /// 并把两条的字接起来（这条合并规则的出处是 LibreOffice 自己导出的 ODF：
    /// 那边一份编辑就是一个 region）
    #[test]
    fn adjacent_runs_of_one_edit_become_one_change() {
        let root = parse(
            r#"<w:document><w:body>
               <w:p><w:r><w:t>预算总额为</w:t></w:r>
               <w:ins w:id="1" w:author="张三" w:date="2026-03-05T09:12:00Z"><w:r><w:t>124000 </w:t></w:r></w:ins>
               <w:ins w:id="2" w:author="张三" w:date="2026-03-05T09:12:00Z"><w:r><w:t>元</w:t></w:r></w:ins>
               <w:del w:id="3" w:author="李四" w:date="2026-03-06T11:45:00Z"><w:r><w:delText>89000 </w:delText></w:r></w:del>
               <w:del w:id="4" w:author="李四" w:date="2026-03-06T11:45:00Z"><w:r><w:delText>元</w:delText></w:r></w:del>
               <w:r><w:rPr><w:rPrChange w:id="5" w:author="王五" w:date="2026-03-07T08:00:00Z"><w:rPr><w:b w:val="0"/></w:rPr></w:rPrChange></w:rPr><w:t>，请复核。</w:t></w:r></w:p>
               </w:body></w:document>"#,
        );
        let body = root.child("document").unwrap().child("body").unwrap();
        let paragraphs: Vec<&Node> = body.descendants("p");
        let ledger = docx_ledger(&paragraphs, None);
        assert_eq!(ledger.insertions, 2, "{:?}", ledger.changes);
        assert_eq!(ledger.deletions, 2);
        assert_eq!(ledger.format_changes, 1);
        assert_eq!(ledger.changes.len(), 3, "{:?}", ledger.changes);
        assert_eq!(ledger.changes[0].text, "124000 元");
        assert_eq!(ledger.changes[0].elements, 2);
        assert_eq!(ledger.changes[0].author, "张三");
        assert_eq!(ledger.changes[0].paragraph, Some(0));
        assert_eq!(ledger.changes[1].kind, "deletion");
        assert_eq!(ledger.changes[1].text, "89000 元");
        assert_eq!(ledger.changes[2].kind, "format-change");
        // 改格式那条不带字：它的正文是原来那段，不是新加的字
        assert_eq!(ledger.changes[2].text, "");
        let note = ledger.notes.join(" ");
        assert!(note.contains("5 个修订元素合成 3 条"), "{note}");
        assert!(ledger.track_changes.is_none(), "没给 settings 就不假装有值");
    }

    /// 段落标记自己也可以是被插入的那一个：它与段里的正文是同一个人同一时间写的，
    /// 但不是同一处改动，所以不许合并
    #[test]
    fn a_paragraph_mark_insertion_stays_its_own_entry() {
        let root = parse(
            r#"<w:document><w:body>
               <w:p><w:pPr><w:rPr><w:ins w:id="9" w:author="张三" w:date="2026-03-05T09:20:00Z"/></w:rPr></w:pPr>
               <w:ins w:id="8" w:author="张三" w:date="2026-03-05T09:20:00Z"><w:r><w:t>整段是新加的。</w:t></w:r></w:ins></w:p>
               </w:body></w:document>"#,
        );
        let body = root.child("document").unwrap().child("body").unwrap();
        let paragraphs: Vec<&Node> = body.descendants("p");
        let ledger = docx_ledger(&paragraphs, None);
        assert_eq!(ledger.insertions, 2);
        assert_eq!(ledger.marks, 1, "段落标记那条要单独记");
        assert_eq!(ledger.changes.len(), 2, "{:?}", ledger.changes);
        assert!(ledger.changes[0].mark);
        assert_eq!(ledger.changes[1].text, "整段是新加的。");
        assert!(!ledger.changes[1].mark);
    }

    /// ODF 把删掉的字存在 region 里、把插入的字存在正文的两个标记之间：
    /// 两处都要读，账才与 OOXML 那边同形状
    #[test]
    fn odt_regions_are_read_back_from_both_places() {
        let root = parse(
            r#"<office:text>
               <text:tracked-changes text:track-changes="false">
                 <text:changed-region text:id="c1"><text:insertion>
                   <office:change-info><dc:creator>张三</dc:creator><dc:date>2026-03-05T09:12:00</dc:date></office:change-info>
                 </text:insertion></text:changed-region>
                 <text:changed-region text:id="c2"><text:deletion>
                   <office:change-info><dc:creator>李四</dc:creator><dc:date>2026-03-06T11:45:00</dc:date></office:change-info>
                   <text:p>89000 元</text:p>
                 </text:deletion></text:changed-region>
               </text:tracked-changes>
               <text:p>第一段没人动。</text:p>
               <text:p>预算总额为<text:change-start text:change-id="c1"/>124000 元<text:change-end text:change-id="c1"/><text:change text:change-id="c2"/>，请复核。</text:p>
               </office:text>"#,
        );
        let paragraphs = odt_body(&root);
        // 递的是 `office:text` 那一层，与 office-doc 的调用点一致：`track-changes`
        // 那个开关挂在它的直接孩子上，`all()` 只看一层，递伪根就什么都看不见
        let text = root.child("text").expect("office:text 在");
        let ledger = odt_ledger(text, &paragraphs);
        assert_eq!(ledger.changes.len(), 2, "{:?}", ledger.changes);
        assert_eq!(paragraphs.len(), 2, "region 里那份被删的段落不算正文段落");
        assert_eq!(ledger.track_changes, Some(false));
        let first = &ledger.changes[0];
        assert_eq!(first.kind, "insertion");
        assert_eq!(first.author, "张三");
        assert_eq!(first.date, "2026-03-05T09:12:00");
        assert_eq!(first.text, "124000 元", "插入的字在正文的区间里");
        assert_eq!(first.paragraph, Some(1), "段落序号按正文那份列表");
        let second = &ledger.changes[1];
        assert_eq!(second.kind, "deletion");
        assert_eq!(second.author, "李四");
        assert_eq!(
            second.text, "89000 元",
            "删掉的字在 region 里，作者与时间不算字"
        );
        assert_eq!(ledger.insertions, 1);
        assert_eq!(ledger.deletions, 1);
    }

    /// 引用找不到 region（文件没写全）要说出来，不静默交一张空账
    #[test]
    fn a_dangling_odt_reference_is_said_out_loud() {
        let root = parse(
            r#"<office:text><text:p>这里引了一个不存在的改动<text:change text:change-id="nope"/></text:p></office:text>"#,
        );
        let paragraphs = odt_body(&root);
        // 与 office-doc 的调用点同一层：`odt_ledger` 拿的是 `office:text`，不是伪根
        let text = root.child("text").expect("office:text 在");
        let ledger = odt_ledger(text, &paragraphs);
        assert!(ledger.changes.is_empty());
        assert!(
            ledger.notes.join(" ").contains("没把修订表写全"),
            "{:?}",
            ledger.notes
        );
    }
}
