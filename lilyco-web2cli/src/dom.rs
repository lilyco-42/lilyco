//! 轻量级 HTML5 DOM 树（基于 `html5ever::tokenizer`，无 `Rc<RefCell>` 循环引用）。
//!
//! 职责：
//! 1. 将任意（含畸形）HTML 文本解析为 `Arena` 节点树
//! 2. 提供文本提取、属性读取、常见 CSS 选择器匹配（`tag` / `#id` / `.class` / `[attr]` / 后代组合）

use std::cell::RefCell;
use std::collections::HashMap;

use html5ever::tendril::StrTendril;
use html5ever::tokenizer::{
    BufferQueue, Tag, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer, TokenizerOpts,
};

/// HTML5 空元素（自闭合，不入栈）
const VOID_TAGS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

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
}

impl DomTree {
    pub fn parse(html: &str) -> Self {
        let sink = DomSink::new();
        let tok = Tokenizer::new(sink, TokenizerOpts::default());
        let queue = BufferQueue::default();
        queue.push_back(StrTendril::from(html));
        let _ = tok.feed(&queue);
        tok.end();
        tok.sink.into_tree()
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

    /// 收集子树所有纯文本（跳过 `<script>` / `<style>` / `<noscript>`），折叠多余空白
    pub fn text(&self, id: usize) -> String {
        let mut out = String::new();
        self.collect_text(id, &mut out, true);
        collapse_ws(&out)
    }

    /// 收集子树原始文本（包含 `<script>` 内部代码，用于 API 静态扫描）
    pub fn raw_text(&self, id: usize) -> String {
        let mut out = String::new();
        self.collect_text(id, &mut out, false);
        out
    }

    fn collect_text(&self, id: usize, out: &mut String, skip_noise: bool) {
        match &self.nodes[id].kind {
            NodeKind::Text(t) => {
                out.push_str(t);
            }
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

    /// 遍历全部节点（前序）
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

    /// 查找所有匹配简单 CSS 选择器的元素 ID
    ///
    /// 支持：`tag`、`#id`、`.class`、`tag.class`、`[attr]`、`[attr=val]`、逗号分隔并集、空格分隔后代组合。
    pub fn select(&self, selector: &str) -> Vec<usize> {
        let mut results = Vec::new();
        let groups: Vec<&str> = selector
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        for id in self.iter_ids() {
            if !matches!(self.nodes[id].kind, NodeKind::Element { .. }) {
                continue;
            }
            if groups.iter().any(|g| self.matches_chain(id, g)) {
                results.push(id);
            }
        }
        results
    }

    fn matches_chain(&self, id: usize, chain: &str) -> bool {
        let parts: Vec<&str> = chain.split_whitespace().collect();
        if parts.is_empty() {
            return false;
        }
        let last = parts[parts.len() - 1];
        if !self.matches_simple(id, last) {
            return false;
        }
        let mut cur = id;
        for &part in parts[..parts.len() - 1].iter().rev() {
            let mut found = false;
            while let Some(p) = self.nodes[cur].parent {
                cur = p;
                if matches!(self.nodes[cur].kind, NodeKind::Element { .. })
                    && self.matches_simple(cur, part)
                {
                    found = true;
                    break;
                }
            }
            if !found {
                return false;
            }
        }
        true
    }

    fn matches_simple(&self, id: usize, sel: &str) -> bool {
        let NodeKind::Element { tag, attrs } = &self.nodes[id].kind else {
            return false;
        };
        let mut rest = sel;

        // [attr] or [attr=val]
        while let Some(start) = rest.find('[') {
            let Some(end) = rest[start..].find(']') else {
                return false;
            };
            let inner = &rest[start + 1..start + end];
            if let Some((k, v)) = inner.split_once('=') {
                let k = k.trim().to_ascii_lowercase();
                let v = v.trim().trim_matches('"').trim_matches('\'');
                if attrs.get(&k).map(String::as_str) != Some(v) {
                    return false;
                }
            } else {
                let k = inner.trim().to_ascii_lowercase();
                if !attrs.contains_key(&k) {
                    return false;
                }
            }
            rest = &rest[..start];
            // Note: only trailing [...] handled simply; good enough for standard selectors
            if !rest.is_empty() {
                break;
            }
        }

        // Split remaining by '.' and '#'
        let mut tag_part: Option<&str> = None;
        let mut id_req: Option<&str> = None;
        let mut classes: Vec<&str> = Vec::new();

        // Parse tag#id.cls1.cls2
        let mut buf = rest;
        if let Some(dot_or_hash) = buf.find(['.', '#']) {
            if dot_or_hash > 0 {
                tag_part = Some(&buf[..dot_or_hash]);
            }
            buf = &buf[dot_or_hash..];
            while !buf.is_empty() {
                let marker = buf.as_bytes()[0];
                let after = &buf[1..];
                let next = after.find(['.', '#']).unwrap_or(after.len());
                let token = &after[..next];
                if marker == b'#' {
                    id_req = Some(token);
                } else {
                    classes.push(token);
                }
                buf = &after[next..];
            }
        } else if !buf.is_empty() && buf != "*" {
            tag_part = Some(buf);
        }

        if let Some(t) = tag_part {
            if !tag.eq_ignore_ascii_case(t) {
                return false;
            }
        }
        if let Some(req_id) = id_req {
            if attrs.get("id").map(String::as_str) != Some(req_id) {
                return false;
            }
        }
        if !classes.is_empty() {
            let elem_classes: Vec<&str> = attrs
                .get("class")
                .map(|c| c.split_whitespace().collect())
                .unwrap_or_default();
            for cls in classes {
                if !elem_classes.contains(&cls) {
                    return false;
                }
            }
        }
        true
    }
}

pub fn collapse_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_ws = true;
    for ch in s.chars() {
        if ch.is_whitespace() {
            if !prev_ws {
                out.push(' ');
                prev_ws = true;
            }
        } else {
            out.push(ch);
            prev_ws = false;
        }
    }
    if out.ends_with(' ') {
        out.pop();
    }
    out
}

struct DomSinkInner {
    nodes: Vec<Node>,
    stack: Vec<usize>,
    pending_text: String,
}

struct DomSink {
    inner: RefCell<DomSinkInner>,
}

impl DomSink {
    fn new() -> Self {
        let root = Node {
            id: 0,
            parent: None,
            children: Vec::new(),
            kind: NodeKind::Document,
        };
        Self {
            inner: RefCell::new(DomSinkInner {
                nodes: vec![root],
                stack: vec![0],
                pending_text: String::new(),
            }),
        }
    }

    fn flush_text(inner: &mut DomSinkInner) {
        if inner.pending_text.is_empty() {
            return;
        }
        let parent = *inner.stack.last().unwrap_or(&0);
        let id = inner.nodes.len();
        let text = std::mem::take(&mut inner.pending_text);
        inner.nodes.push(Node {
            id,
            parent: Some(parent),
            children: Vec::new(),
            kind: NodeKind::Text(text),
        });
        inner.nodes[parent].children.push(id);
    }

    fn into_tree(self) -> DomTree {
        let mut inner = self.inner.into_inner();
        Self::flush_text(&mut inner);
        DomTree { nodes: inner.nodes }
    }
}

impl TokenSink for DomSink {
    type Handle = ();

    fn process_token(&self, token: Token, _line_number: u64) -> TokenSinkResult<()> {
        let mut inner = self.inner.borrow_mut();
        match token {
            Token::CharacterTokens(s) => {
                inner.pending_text.push_str(&s);
            }
            Token::TagToken(Tag {
                kind: TagKind::StartTag,
                name,
                self_closing,
                attrs,
                ..
            }) => {
                Self::flush_text(&mut inner);
                let tag = name.as_ref().to_ascii_lowercase();
                let mut attr_map = HashMap::new();
                for a in attrs {
                    attr_map.insert(
                        a.name.local.as_ref().to_ascii_lowercase(),
                        a.value.as_ref().to_string(),
                    );
                }
                let parent = *inner.stack.last().unwrap_or(&0);
                let id = inner.nodes.len();
                inner.nodes.push(Node {
                    id,
                    parent: Some(parent),
                    children: Vec::new(),
                    kind: NodeKind::Element {
                        tag: tag.clone(),
                        attrs: attr_map,
                    },
                });
                inner.nodes[parent].children.push(id);
                if !self_closing && !VOID_TAGS.contains(&tag.as_str()) {
                    inner.stack.push(id);
                }
            }
            Token::TagToken(Tag {
                kind: TagKind::EndTag,
                name,
                ..
            }) => {
                Self::flush_text(&mut inner);
                let tag = name.as_ref().to_ascii_lowercase();
                if let Some(pos) = inner.stack.iter().rposition(|&id| {
                    matches!(&inner.nodes[id].kind, NodeKind::Element { tag: t, .. } if t == &tag)
                }) {
                    if pos > 0 {
                        inner.stack.truncate(pos);
                    }
                }
            }
            Token::NullCharacterToken => {}
            Token::EOFToken => {
                Self::flush_text(&mut inner);
            }
            _ => {}
        }
        TokenSinkResult::Continue
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_forms_links_and_selectors() {
        let html = r#"
        <html>
          <head><title>Test</title></head>
          <body>
            <h1 id="hero" class="title main">Hello <span>World</span></h1>
            <form action="/search" method="get">
              <input type="text" name="q" placeholder="Search...">
              <button type="submit">Go</button>
            </form>
            <a href="/about" class="nav">About</a>
            <script>const secret = "/api/v1/items";</script>
          </body>
        </html>
        "#;
        let dom = DomTree::parse(html);
        assert_eq!(dom.text(dom.select("h1#hero.main")[0]), "Hello World");
        assert_eq!(dom.select("form input[name=q]").len(), 1);
        assert_eq!(dom.attr(dom.select("a.nav")[0], "href"), Some("/about"));
        // script text is excluded from normal text()
        assert!(!dom.text(dom.root()).contains("/api/v1/items"));
        // but available in raw_text()
        let script = dom.select("script")[0];
        assert!(dom.raw_text(script).contains("/api/v1/items"));
    }
}
