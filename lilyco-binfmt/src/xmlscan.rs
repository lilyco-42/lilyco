//! 够用就好的 XML 读取：OOXML 与 ODF 的部件是普通 XML，本域要的是「某个元素下的文本」
//! 和「某个属性的值」，不是一份能过校验的 DOM。
//!
//! 故意不做的四件事，以及为什么：
//! 1. **命名空间 URI 解析**：OOXML 的元素名以 `w:p` / `a:t` 这种前缀形式才对人可读，
//!    所以前缀原样留着，比对时可以按「局部名」匹配（`child_local` / `attr_local`）。
//!    真正需要区分命名空间的地方（同名不同 ns）在办公文件里没遇到过，遇到再说；
//! 2. **DTD 与外部实体**：办公文件不该有（有就是 XXE 的攻击面）。这里不展开任何外部实体，
//!    `<!DOCTYPE>` 整段跳过 —— 读别人的文档不该反过来请求它的资源；
//! 3. **schema 校验**：不校验就说「不校验」，各命令把属性缺失当成「文件没写」而不是报错；
//! 4. **XInclude / 符号实体**：同上，不做。
//!
//! 解析是容错的：标签不配对时按噪声跳过并继续，因为我们要服务的场景之一就是
//! 「这份文件坏了一半，还能读出多少」—— 静默中断等于什么都没读到。

/// 一个元素。`name` 是原文里的名字（带前缀），`direct` 是它的直接文本段拼起来（已解实体）。
pub struct Node {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub direct: String,
    pub children: Vec<Node>,
}

impl Node {
    fn leaf(name: &str) -> Node {
        Node {
            name: name.to_string(),
            attrs: Vec::new(),
            direct: String::new(),
            children: Vec::new(),
        }
    }

    /// 属性值，名字精确匹配（含前缀）
    pub fn attr(&self, want: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(one, _)| one == want)
            .map(|(_, value)| value.as_str())
    }

    /// 属性值，按局部名匹配（`r:id` 与 `id` 都算 `id`）—— 同一份 OOXML 里
    /// 前缀是文档自己声明的，`r:` 不是常量
    pub fn attr_local(&self, want: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(one, _)| local_of(one) == want)
            .map(|(_, value)| value.as_str())
    }

    /// 局部名（去掉 `prefix:`）
    pub fn local(&self) -> &str {
        local_of(&self.name)
    }

    pub fn is(&self, want: &str) -> bool {
        self.name == want || self.local() == want
    }

    /// 第一个直接子元素（`want` 可以是全名或局部名）
    pub fn child(&self, want: &str) -> Option<&Node> {
        self.children.iter().find(|one| one.is(want))
    }

    /// 所有直接子元素
    pub fn all(&self, want: &str) -> Vec<&Node> {
        self.children.iter().filter(|one| one.is(want)).collect()
    }

    /// 所有后代元素（不含自己），按文档顺序
    pub fn descendants(&self, want: &str) -> Vec<&Node> {
        let mut out = Vec::new();
        self.collect(want, &mut out);
        out
    }

    fn collect(&self, want: &str, out: &mut Vec<&Node>) {
        for one in &self.children {
            if one.is(want) {
                out.push(one);
            }
            one.collect(want, out);
        }
    }

    /// 整棵子树的文本拼起来（`<a:t>` / `<w:t>` 里的字就是这么读的）
    pub fn text(&self) -> String {
        let mut out = self.direct.clone();
        for one in &self.children {
            out.push_str(&one.text());
        }
        out
    }

    /// 子树文本的行数（段落数）：各命令里「这篇文档有多少段」的最省事口径
    pub fn line_count(&self) -> usize {
        self.text().lines().count()
    }
}

/// 解析一份 XML 文档。返回伪根 `#doc`：办公文件的根元素外面还有 `<?xml?>` 与注释，
/// 有伪根就不用让调用方处理「可能没有根元素」。
pub fn parse(src: &[u8]) -> Node {
    let mut at = 0usize;
    let mut root = Node::leaf("#doc");
    parse_children(src, &mut at, &mut root, "");
    root
}

/// 同理，但入参是字符串（部件读出来多半已经是文本）
pub fn parse_str(src: &str) -> Node {
    parse(src.as_bytes())
}

fn local_of(name: &str) -> &str {
    match name.find(':') {
        Some(at) => &name[at + 1..],
        None => name,
    }
}

const WS: [u8; 4] = [b' ', b'\t', b'\n', b'\r'];

fn is_ws(byte: u8) -> bool {
    WS.contains(&byte)
}

fn starts(src: &[u8], at: usize, pat: &[u8]) -> bool {
    src.len() >= at + pat.len() && &src[at..at + pat.len()] == pat
}

fn find_from(src: &[u8], from: usize, pat: &[u8]) -> Option<usize> {
    if from >= src.len() {
        return None;
    }
    let hit = src[from..].windows(pat.len()).position(|one| one == pat)?;
    Some(from + hit)
}

/// 解析 `parent` 的内容，直到看见名为 `stop` 的结束标签（`stop` 为空表示到文件尾）。
/// `at` 一定前进，所以畸形输入不会转圈。
fn parse_children(src: &[u8], at: &mut usize, parent: &mut Node, stop: &str) {
    loop {
        if *at >= src.len() {
            return;
        }
        if src[*at] != b'<' {
            let Some(p) = find_from(src, *at, b"<") else {
                parent.push_text(&src[*at..]);
                *at = src.len();
                return;
            };
            parent.push_text(&src[*at..p]);
            *at = p;
        }
        if *at + 2 > src.len() {
            return;
        }
        match src[*at + 1] {
            b'!' => {
                if starts(src, *at, b"<!--") {
                    match find_from(src, *at + 4, b"-->") {
                        Some(end) => *at = end + 3,
                        None => {
                            *at = src.len();
                            return;
                        }
                    }
                } else if starts(src, *at, b"<![CDATA[") {
                    match find_from(src, *at + 9, b"]]>") {
                        Some(end) => {
                            parent.push_text(&src[*at + 9..end]);
                            *at = end + 3;
                        }
                        None => {
                            *at = src.len();
                            return;
                        }
                    }
                } else {
                    // <!DOCTYPE ...>：可能带内部子集 `[...]`，两者都跳过，绝不请求实体
                    *at = skip_markup_decl(src, *at);
                }
            }
            b'?' => match find_from(src, *at, b"?>") {
                Some(end) => *at = end + 2,
                None => {
                    *at = src.len();
                    return;
                }
            },
            b'/' => {
                let Some(end) = find_from(src, *at + 2, b">") else {
                    *at = src.len();
                    return;
                };
                let name = String::from_utf8_lossy(&src[*at + 2..end])
                    .trim()
                    .to_string();
                *at = end + 1;
                if !stop.is_empty() && (name == stop || local_of(&name) == local_of(stop)) {
                    return;
                }
                // 名字不配对：畸形输入，继续读而不是整体放弃
            }
            _ => {
                let Some((name, attrs, self_closed, next)) = read_tag(src, *at) else {
                    *at = src.len();
                    return;
                };
                *at = next;
                let mut node = Node {
                    name,
                    attrs,
                    direct: String::new(),
                    children: Vec::new(),
                };
                if !self_closed {
                    let stop_name = node.name.clone();
                    parse_children(src, at, &mut node, &stop_name);
                }
                parent.children.push(node);
            }
        }
    }
}

/// 跳过 `<!` 开头的声明：内部子集 `[ ... ]` 里可以有 `>`
fn skip_markup_decl(src: &[u8], at: usize) -> usize {
    let mut i = at + 2;
    let mut depth = 0usize;
    while i < src.len() {
        match src[i] {
            b'[' => depth += 1,
            b']' => depth = depth.saturating_sub(1),
            b'>' if depth == 0 => return i + 1,
            _ => {}
        }
        i += 1;
    }
    src.len()
}

/// 读一个开始标签：返回（名字, 属性, 是否自闭合, 下一个位置）
fn read_tag(src: &[u8], at: usize) -> Option<(String, Vec<(String, String)>, bool, usize)> {
    let mut i = at + 1;
    let name_start = i;
    while i < src.len() && !is_ws(src[i]) && src[i] != b'>' && src[i] != b'/' {
        i += 1;
    }
    if i == name_start {
        return None;
    }
    let name = String::from_utf8_lossy(&src[name_start..i]).into_owned();
    let mut attrs: Vec<(String, String)> = Vec::new();
    let mut self_closed = false;
    loop {
        while i < src.len() && is_ws(src[i]) {
            i += 1;
        }
        if i >= src.len() {
            return None;
        }
        if src[i] == b'>' {
            i += 1;
            break;
        }
        if src[i] == b'/' {
            if src.get(i + 1) == Some(&b'>') {
                self_closed = true;
                i += 2;
                break;
            }
            i += 1;
            continue;
        }
        let key_start = i;
        while i < src.len() && src[i] != b'=' && !is_ws(src[i]) && src[i] != b'>' && src[i] != b'/'
        {
            i += 1;
        }
        if i == key_start {
            i += 1; // 认不出来的字节跳过去，别在同一个位置打转
            continue;
        }
        let key = String::from_utf8_lossy(&src[key_start..i]).into_owned();
        while i < src.len() && is_ws(src[i]) {
            i += 1;
        }
        if i >= src.len() || src[i] != b'=' {
            attrs.push((key, String::new())); // 裸属性（XML 里不合法，但别整体放弃）
            continue;
        }
        i += 1;
        while i < src.len() && is_ws(src[i]) {
            i += 1;
        }
        if i >= src.len() {
            return None;
        }
        let quote = src[i];
        let value = if quote == b'"' || quote == b'\'' {
            i += 1;
            let value_start = i;
            while i < src.len() && src[i] != quote {
                i += 1;
            }
            let raw = String::from_utf8_lossy(&src[value_start..i]).into_owned();
            if i < src.len() {
                i += 1;
            }
            raw
        } else {
            let value_start = i;
            while i < src.len() && !is_ws(src[i]) && src[i] != b'>' {
                i += 1;
            }
            String::from_utf8_lossy(&src[value_start..i]).into_owned()
        };
        attrs.push((key, unescape(&value)));
    }
    Some((name, attrs, self_closed, i))
}

impl Node {
    /// 一段直接文本。存成 `#text` 子节点而不是拼进 `direct`，是为了保住顺序：
    /// `<text:p>前<span>中</span>后</text:p>` 读出来必须是「前中后」，
    /// 拼成一份 direct 就只能得到「前后中」。
    fn push_text(&mut self, raw: &[u8]) {
        if raw.is_empty() {
            return;
        }
        let mut one = Node::leaf("#text");
        one.direct = unescape(&String::from_utf8_lossy(raw));
        self.children.push(one);
    }
}

/// 解 XML 实体：五个具名 + 数字引用（十进制与十六进制）。其它 `&xxx;` 原样留着 ——
/// 那是没声明的实体，替文件猜一个值比承认「这里有个我不认的实体」更糟。
pub fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    loop {
        let Some(at) = rest.find('&') else {
            out.push_str(rest);
            return out;
        };
        out.push_str(&rest[..at]);
        let tail = &rest[at..];
        let Some(semi) = tail.find(';') else {
            out.push_str(tail);
            return out;
        };
        let entity = &tail[1..semi];
        match entity {
            "amp" => out.push('&'),
            "lt" => out.push('<'),
            "gt" => out.push('>'),
            "quot" => out.push('"'),
            "apos" => out.push('\''),
            _ => {
                let decoded = if let Some(hex) = entity.strip_prefix("#x") {
                    u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
                } else if let Some(dec) = entity.strip_prefix('#') {
                    dec.parse::<u32>().ok().and_then(char::from_u32)
                } else {
                    out.push_str(&tail[..semi + 1]);
                    rest = &rest[at + semi + 1..];
                    continue;
                };
                match decoded {
                    Some(one) => out.push(one),
                    None => out.push_str(&tail[..semi + 1]),
                }
            }
        }
        rest = &rest[at + semi + 1..];
    }
}

/// 子树文本压掉首尾空白与换行折叠 —— 表格里读单元格时用的口径
pub fn inline_text(node: &Node) -> String {
    let flat = node.text();
    let mut out = String::with_capacity(flat.len());
    let mut space = false;
    for one in flat.chars() {
        if one == ' ' || one == '\t' || one == '\n' || one == '\r' {
            space = true;
        } else {
            if space && !out.is_empty() {
                out.push(' ');
            }
            space = false;
            out.push(one);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_elements_attributes_and_text() {
        let src = r#"<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    <w:p w14:paraId="7A"><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>标题</w:t></w:r></w:p>
    <w:p/>
  </w:body>
</w:document>"#;
        let doc = parse_str(src);
        let body = doc.descendants("body").remove(0);
        let paras = body.all("p");
        assert_eq!(paras.len(), 2, "含自闭合的那条也要算进来");
        assert_eq!(paras[0].text().trim(), "标题");
        assert_eq!(paras[1].children.len(), 0);
        let style = paras[0].descendants("pStyle").remove(0);
        assert_eq!(style.attr("w:val"), Some("Heading1"));
        assert_eq!(style.attr_local("val"), Some("Heading1"));
        assert_eq!(paras[0].attr_local("paraId"), Some("7A"));
    }

    #[test]
    fn entities_and_numeric_references_are_decoded() {
        let doc = parse_str(r#"<a x="&amp;&lt;&#65;&#x42;">A &amp; B &#20013;</a>"#);
        let one = doc.child("a").expect("有 a");
        assert_eq!(one.attr("x"), Some("&<AB"));
        assert_eq!(one.text(), "A & B 中");
    }

    /// 未声明的实体不许被猜成某个字符：原样留着才看得出文件里写了什么
    #[test]
    fn unknown_entities_survive_verbatim() {
        let doc = parse_str("<a>&nbsp;&unknown;</a>");
        assert_eq!(doc.child("a").expect("有 a").text(), "&nbsp;&unknown;");
    }

    #[test]
    fn cdata_is_text_and_comments_are_not() {
        let doc = parse_str("<a><![CDATA[<not markup>]]><!-- <a/> comment --></a>");
        let a = doc.child("a").expect("有 a");
        assert_eq!(a.text(), "<not markup>");
        assert_eq!(a.children.len(), 1, "CDATA 是一段文本，注释不是元素");
        assert_eq!(a.children[0].name, "#text");
    }

    /// 混合内容的顺序：`前<span>中</span>后` 读出来必须是「前中后」
    #[test]
    fn mixed_content_keeps_document_order() {
        let doc = parse_str("<p>前<span>中</span>后</p>");
        assert_eq!(doc.child("p").expect("有 p").text(), "前中后");
    }

    /// DOCTYPE 的内部子集里有 `>`：按 `>` 切会把声明后的真元素切掉
    #[test]
    fn doctype_with_internal_subset_does_not_eat_the_document() {
        let doc = parse_str("<!DOCTYPE note [<!ENTITY x \"<y>\">]><note><body>hi</body></note>");
        assert_eq!(
            doc.descendants("body").remove(0).text(),
            "hi",
            "子集里的 < > 不能当成标签结束"
        );
    }

    /// 标签不配对（截断文件常见）：已读出来的部分要保住，别整体放弃
    #[test]
    fn mismatched_closing_tags_do_not_abort_the_parse() {
        let doc = parse_str("<a><b>one</c><b>two</b></a>");
        assert_eq!(doc.descendants("b").len(), 2);
        assert_eq!(doc.descendants("b")[1].text(), "two");
    }

    #[test]
    fn truncated_document_still_yields_what_arrived() {
        let doc = parse_str("<a><b>one</b><b>two");
        assert_eq!(doc.descendants("b").len(), 2);
        assert_eq!(doc.descendants("b")[0].text(), "one");
    }

    #[test]
    fn inline_text_collapses_whitespace_runs() {
        let doc = parse_str("<c>  a\n  b  <d>c d</d> </c>");
        assert_eq!(inline_text(doc.child("c").expect("有 c")), "a b c d");
    }

    #[test]
    fn an_empty_document_has_only_the_pseudo_root() {
        let doc = parse_str("   ");
        assert_eq!(doc.name, "#doc");
        assert!(doc.children.is_empty());
        assert!(doc.text().trim().is_empty());
    }
}
