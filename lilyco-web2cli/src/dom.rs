//! HTML5 DOM 树与 CSS3 选择器封装（直接复用 `scraper` crate，不再手写 HTML tokenizer 与选择器引擎）。
//!
//! 职责：
//! 1. 使用 `scraper::Html::parse_document` 解析任意（含畸形）HTML 为索引节点树
//! 2. 使用 `scraper::Selector::parse` 提供完整 CSS3 选择器查询能力
//! 3. 保留向后兼容的 `DomTree` Arena 接口供 `distill` / `discover` / `exec` 消费

use std::collections::HashMap;

use ego_tree::{NodeId, NodeRef};
use scraper::{ElementRef, Html, Node as ScraperNode, Selector};

#[derive(Debug, Clone)]
pub enum NodeKind {
    Document,
    Element {
        tag: String,
        attrs: HashMap<String, String>,
    },
    Text(String),
}

#[derive(Debug, Clone)]
pub struct Node {
    pub id: usize,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub kind: NodeKind,
}

#[derive(Debug, Clone)]
pub struct DomTree {
    pub nodes: Vec<Node>,
    doc: Html,
    /// 将 `scraper` 的 `NodeId` 映射到本 Arena 的 `usize` 索引，实现 O(1) CSS3 选择器桥接。
    id_map: HashMap<NodeId, usize>,
}

impl DomTree {
    pub fn parse(html: &str) -> Self {
        let doc = Html::parse_document(html);
        let mut nodes = Vec::new();
        let mut id_map = HashMap::new();
        Self::build_from_scraper(doc.tree.root(), None, &mut nodes, &mut id_map);
        Self { nodes, doc, id_map }
    }

    fn build_from_scraper(
        node_ref: NodeRef<'_, ScraperNode>,
        parent: Option<usize>,
        nodes: &mut Vec<Node>,
        id_map: &mut HashMap<NodeId, usize>,
    ) -> Option<usize> {
        let kind = match node_ref.value() {
            ScraperNode::Document | ScraperNode::Fragment => NodeKind::Document,
            ScraperNode::Element(el) => {
                let mut attrs = HashMap::new();
                for (k, v) in el.attrs() {
                    attrs.insert(k.to_ascii_lowercase(), v.to_string());
                }
                NodeKind::Element {
                    tag: el.name().to_ascii_lowercase(),
                    attrs,
                }
            }
            ScraperNode::Text(t) => NodeKind::Text(t.text.to_string()),
            _ => return None,
        };

        let id = nodes.len();
        nodes.push(Node {
            id,
            parent,
            children: Vec::new(),
            kind,
        });
        id_map.insert(node_ref.id(), id);

        for child in node_ref.children() {
            if let Some(cid) = Self::build_from_scraper(child, Some(id), nodes, id_map) {
                nodes[id].children.push(cid);
            }
        }
        Some(id)
    }

    pub fn root(&self) -> usize {
        0
    }

    pub fn node(&self, id: usize) -> &Node {
        &self.nodes[id]
    }

    pub fn tag(&self, id: usize) -> Option<&str> {
        match &self.nodes[id].kind {
            NodeKind::Element { tag, .. } => Some(tag.as_str()),
            _ => None,
        }
    }

    pub fn attr(&self, id: usize, name: &str) -> Option<&str> {
        match &self.nodes[id].kind {
            NodeKind::Element { attrs, .. } => {
                attrs.get(&name.to_ascii_lowercase()).map(String::as_str)
            }
            _ => None,
        }
    }

    pub fn has_attr(&self, id: usize, name: &str) -> bool {
        self.attr(id, name).is_some()
    }

    /// 收集子树所有纯文本（跳过 `<script>` / `<style>` / `<noscript>` / `<svg>`），折叠多余空白。
    pub fn text(&self, id: usize) -> String {
        let mut out = String::new();
        self.collect_text(id, &mut out, true);
        collapse_ws(&out)
    }

    /// 收集子树原始文本（包含 `<script>` 内部代码，用于 API 静态扫描）。
    pub fn raw_text(&self, id: usize) -> String {
        let mut out = String::new();
        self.collect_text(id, &mut out, false);
        out
    }

    fn collect_text(&self, id: usize, out: &mut String, skip_noise: bool) {
        match &self.nodes[id].kind {
            NodeKind::Text(t) => out.push_str(t),
            NodeKind::Element { tag, .. } => {
                if skip_noise && matches!(tag.as_str(), "script" | "style" | "noscript" | "svg") {
                    return;
                }
                let is_block = matches!(
                    tag.as_str(),
                    "p" | "div"
                        | "section"
                        | "article"
                        | "li"
                        | "tr"
                        | "br"
                        | "hr"
                        | "h1"
                        | "h2"
                        | "h3"
                        | "h4"
                        | "h5"
                        | "h6"
                        | "blockquote"
                        | "pre"
                );
                if is_block && !out.ends_with(' ') && !out.is_empty() {
                    out.push(' ');
                }
                for &child in &self.nodes[id].children {
                    self.collect_text(child, out, skip_noise);
                }
                if is_block && !out.ends_with(' ') {
                    out.push(' ');
                }
            }
            NodeKind::Document => {
                for &child in &self.nodes[id].children {
                    self.collect_text(child, out, skip_noise);
                }
            }
        }
    }

    /// 前序遍历全部节点 ID。
    pub fn iter_ids(&self) -> Vec<usize> {
        let mut out = Vec::with_capacity(self.nodes.len());
        self.walk_preorder(0, &mut out);
        out
    }

    fn walk_preorder(&self, id: usize, out: &mut Vec<usize>) {
        out.push(id);
        for &c in &self.nodes[id].children {
            self.walk_preorder(c, out);
        }
    }

    /// 查找所有匹配完整 CSS3 选择器的元素 ID（底层由 `scraper::Selector` 执行）。
    pub fn select(&self, selector: &str) -> Vec<usize> {
        let Ok(sel) = Selector::parse(selector) else {
            return Vec::new();
        };
        self.doc
            .select(&sel)
            .filter_map(|el: ElementRef<'_>| self.id_map.get(&el.id()).copied())
            .collect()
    }
}

/// 折叠连续空白字符为单个空格并裁剪首尾。
pub fn collapse_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = true;
    for ch in s.chars() {
        if ch.is_whitespace() {
            if !prev_space {
                out.push(' ');
                prev_space = true;
            }
        } else {
            out.push(ch);
            prev_space = false;
        }
    }
    if out.ends_with(' ') {
        out.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_and_selects_css3() {
        let html = r#"
        <html><head><title>Demo</title><style>.x{color:red}</style></head>
        <body>
            <div id="main" class="box active">
                <p>Hello <b>World</b></p>
                <a href="/next" data-role="nav">Next</a>
            </div>
            <script>const x = 1;</script>
        </body></html>"#;
        let dom = DomTree::parse(html);
        assert_eq!(dom.select("#main").len(), 1);
        assert_eq!(dom.select("div.box.active > a[data-role='nav']").len(), 1);
        let a_id = dom.select("div.box a[data-role=nav]")[0];
        assert_eq!(dom.attr(a_id, "href"), Some("/next"));
        assert_eq!(dom.text(a_id), "Next");
        assert!(!dom.text(dom.root()).contains("const x = 1"));
    }
}
