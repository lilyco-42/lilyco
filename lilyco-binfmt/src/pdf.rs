//! PDF 的读法：一张对象表 + 一层对象流，外加字符串那三件事。
//!
//! 这一族与 [`crate::opack`] 那四类容器（OPC / ODF / CFB / RTF）没有共同结构，
//! 所以它不走 `Family` 那条分派，自成一份读者。
//!
//! 为什么不追交叉引用表：对象在文件里是以 `N G obj … endobj` 明写出来的，
//! xref 只是索引。真坏的文件（xref 指错而对象齐全）反而要靠这种容忍读法。
//! 但这条读法必须补两层，否则报出来的数是假的 —— 两层的规则都是从真件里量的：
//!
//! - **对象流**（`/Type /ObjStm`）：qpdf 存的那份里，68 个对象只有 17 个是明写的，
//!   另外 51 个挤在一个压缩流里 —— 只扫 `obj` 会报「这文件没有页」。头部是一串
//!   「对象号 体内偏移」成对出现，偏移相对 `/First`（这条是从一份真的 Word 2013
//!   文件里对出来的：它的 `/N 6 /First 39`，头部 `14 0 13 51 10 101 …`）。
//! - **没有 `trailer` 这个词的文件**：PDF 1.5 起 trailer 的键可以整个搬进
//!   `/Type /XRef` 的流字典。所以找 `/Root`、`/Info` 要两处都找。
//!
//! 字符串的三件事（`(...)` 里的 `\(` `\)` `\\` 与八进制 `\ddd`、括号可嵌套；
//! `<...>` 是字节串，`FEFF` 开的是 UTF-16BE）见 [`literal`] 与 [`decode_text`]。
//! 流正文按 `endstream` 定界而不是 `/Length` —— 与见证读者同一条规则，两边才有
//! 可比性；代价是流的二进制正文里真出现 `endstream` 这九个字节时会截错。

use flate2::read::ZlibDecoder;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;

/// 一个对象字典最多留这么多字节：结构事实全在字典里，`/Widths` 那种长数组也远不到
/// 这个数；不设上限的话一份大文件会把整本对象表连正文一起搬进内存
const DICT_CAP: usize = 1 << 16;
/// 单个流解压上限：压缩比可以很高，这里挡住「四十千字节解出四个吉字节」那一路
pub const STREAM_CAP: u64 = 1 << 26;

pub fn is_pdf(bytes: &[u8]) -> bool {
    bytes.starts_with(b"%PDF-")
}

fn is_name_char(one: u8) -> bool {
    one.is_ascii_alphanumeric() || matches!(one, b'.' | b'-' | b'_' | b'+' | b'%')
}

fn is_space(one: u8) -> bool {
    matches!(one, b' ' | b'\t' | b'\r' | b'\n' | 0x0c | 0x00)
}

fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || from + needle.len() > hay.len() {
        return None;
    }
    let mut at = from;
    while at + needle.len() <= hay.len() {
        if hay[at..].starts_with(needle) {
            return Some(at);
        }
        at += 1;
    }
    None
}

/// `/Key` 出现的位置：整个名字要匹配 —— `/Page` 不能算 `/Pages`，`/L` 不能算 `/Length`
fn key_positions(body: &[u8], key: &[u8]) -> Vec<usize> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while let Some(found) = find(body, key, at) {
        let after = found + key.len();
        let tail_ok = body.get(after).is_none_or(|one| !is_name_char(*one));
        let head_ok = found == 0 || !is_name_char(body[found - 1]);
        if tail_ok && head_ok {
            out.push(found);
        }
        at = found + 1;
    }
    out
}

fn key_present(body: &[u8], key: &[u8]) -> bool {
    !key_positions(body, key).is_empty()
}

fn count_key(body: &[u8], key: &[u8]) -> usize {
    key_positions(body, key).len()
}

fn skip_spaces(body: &[u8], mut at: usize) -> usize {
    while at < body.len() && is_space(body[at]) {
        at += 1;
    }
    at
}

/// 紧跟在 `/Key` 后的整数（中间允许空白或 `[`、`(`、`<` 这类分隔）
fn int_after(body: &[u8], key: &[u8]) -> Option<i64> {
    for at in key_positions(body, key) {
        let mut i = skip_spaces(body, at + key.len());
        if matches!(body.get(i), Some(b'[') | Some(b'(') | Some(b'<')) {
            i = skip_spaces(body, i + 1);
        }
        let start = i;
        if matches!(body.get(i), Some(b'-') | Some(b'+')) {
            i += 1;
        }
        let digits_at = i;
        while i < body.len() && body[i].is_ascii_digit() {
            i += 1;
        }
        if i > digits_at {
            if let Ok(done) = String::from_utf8_lossy(&body[start..i])
                .trim()
                .parse::<i64>()
            {
                return Some(done);
            }
        }
    }
    None
}

/// 紧跟在 `/Key` 后的名字（`/Subtype/Image` 里的 `Image`）
fn name_after(body: &[u8], key: &[u8]) -> Option<String> {
    for at in key_positions(body, key) {
        let mut i = skip_spaces(body, at + key.len());
        if body.get(i) != Some(&b'/') {
            continue;
        }
        i += 1;
        let start = i;
        while i < body.len() && is_name_char(body[i]) {
            i += 1;
        }
        if i > start {
            return Some(String::from_utf8_lossy(&body[start..i]).into_owned());
        }
    }
    None
}

/// 这个键出现了几次、其中名字等于 want 的有几次（风险面按**出现次数**算，不按对象数）
fn count_name(body: &[u8], key: &[u8], want: &str) -> usize {
    let mut hit = 0usize;
    for at in key_positions(body, key) {
        let mut i = skip_spaces(body, at + key.len());
        if body.get(i) != Some(&b'/') {
            continue;
        }
        i += 1;
        let start = i;
        while i < body.len() && is_name_char(body[i]) {
            i += 1;
        }
        if i > start && String::from_utf8_lossy(&body[start..i]) == want {
            hit += 1;
        }
    }
    hit
}

/// `/Key 12 0 R` 里的对象号
fn ref_after(body: &[u8], key: &[u8]) -> Option<u64> {
    for at in key_positions(body, key) {
        let mut i = skip_spaces(body, at + key.len());
        let start = i;
        while i < body.len() && body[i].is_ascii_digit() {
            i += 1;
        }
        if i == start {
            continue;
        }
        let Ok(number) = String::from_utf8_lossy(&body[start..i]).parse::<u64>() else {
            continue;
        };
        let gen_at = skip_spaces(body, i);
        let mut j = gen_at;
        while j < body.len() && body[j].is_ascii_digit() {
            j += 1;
        }
        if j == gen_at {
            continue;
        }
        if body.get(j) == Some(&b'R') {
            return Some(number);
        }
    }
    None
}

const ESCAPES: &[(u8, u8)] = &[
    (b'n', 0x0a),
    (b'r', 0x0d),
    (b't', 0x09),
    (b'b', 0x08),
    (b'f', 0x0c),
    (b'(', b'('),
    (b')', b')'),
    (b'\\', b'\\'),
];

/// 从开头的 `(` 之后读到配对的 `)`：括号可嵌套，反斜杠转义吃掉下一个字符，
/// 八进制 `ddd` 至多三位。读到结尾还没配对上号时，把手里的东西照实交回 ——
/// 截断的文件也要能读出字，而不是整份作废
pub fn literal(body: &[u8], mut at: usize) -> (Vec<u8>, usize) {
    let mut out: Vec<u8> = Vec::new();
    let mut depth = 1usize;
    while at < body.len() {
        let ch = body[at];
        if ch == b'\\' {
            let nxt = body.get(at + 1).copied().unwrap_or(0);
            if let Some((_from, to)) = ESCAPES.iter().find(|(from, _to)| *from == nxt) {
                out.push(*to);
                at += 2;
                continue;
            }
            if (b'0'..=b'7').contains(&nxt) {
                let mut digits: Vec<u8> = Vec::new();
                let mut j = at + 1;
                while j < body.len() && digits.len() < 3 && (b'0'..=b'7').contains(&body[j]) {
                    digits.push(body[j]);
                    j += 1;
                }
                let value = u32::from_str_radix(&String::from_utf8_lossy(&digits), 8).unwrap_or(0);
                out.push((value & 0xff) as u8);
                at = j;
                continue;
            }
            out.push(nxt);
            at += 2;
            continue;
        }
        if ch == b'(' {
            depth += 1;
        } else if ch == b')' {
            depth -= 1;
            if depth == 0 {
                return (out, at + 1);
            }
        }
        out.push(ch);
        at += 1;
    }
    (out, at)
}

fn hex_bytes(raw: &[u8]) -> Option<Vec<u8>> {
    let mut digits: Vec<u8> = raw
        .iter()
        .copied()
        .filter(|one| one.is_ascii_hexdigit())
        .collect();
    if digits.len() % 2 != 0 {
        digits.push(b'0');
    }
    let text = String::from_utf8(digits).ok()?;
    (0..text.len() / 2)
        .map(|at| u8::from_str_radix(&text[at * 2..at * 2 + 2], 16).ok())
        .collect()
}

/// `/Key (…) 或 /Key <…>` 的值，按出现顺序交回解好的字节。
/// 定界符只看、不吃掉：`(Literal)` 与 `<Hex>` 本身就是下一个要解析的东西
pub fn strings_of(body: &[u8], key: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    for at in key_positions(body, key) {
        let i = skip_spaces(body, at + key.len());
        match body.get(i) {
            Some(b'(') => {
                let (raw, _next) = literal(body, i + 1);
                out.push(raw);
            }
            Some(b'<') => {
                let Some(close) = find(body, b">", i + 1) else {
                    break;
                };
                if let Some(raw) = hex_bytes(&body[i + 1..close]) {
                    out.push(raw);
                }
            }
            _ => {}
        }
    }
    out
}

/// `FEFF` 开的是 UTF-16BE，`FFFE` 开的是 UTF-16LE，其余按 Latin-1 交出去。
/// 最后这一条是**近似**：PDF 的字节串自己不声明非 Unicode 文本用什么码页
pub fn decode_text(raw: &[u8]) -> String {
    if raw.starts_with(b"\xfe\xff") {
        let units: Vec<u16> = raw[2..]
            .chunks(2)
            .filter(|one| one.len() == 2)
            .map(|one| u16::from_be_bytes([one[0], one[1]]))
            .collect();
        return String::from_utf16_lossy(&units);
    }
    if raw.starts_with(b"\xff\xfe") {
        let units: Vec<u16> = raw[2..]
            .chunks(2)
            .filter(|one| one.len() == 2)
            .map(|one| u16::from_le_bytes([one[0], one[1]]))
            .collect();
        return String::from_utf16_lossy(&units);
    }
    raw.iter().map(|one| *one as char).collect()
}

/// 这个键的第一个字符串值（`/Title (…)`、`/Lang (en-US)`、`/Producer <…>`）
pub fn one_string(body: &[u8], key: &[u8]) -> Option<String> {
    strings_of(body, key)
        .first()
        .cloned()
        .map(|raw| decode_text(&raw))
}

/// `stream` 关键字：前面得是行尾，后面得跟一个行尾 —— 按这三个字母切会被
/// 字典里的字（`/Title(Obj stream fixture)`）撞倒，这条规则两边读者共用
fn stream_keyword(body: &[u8]) -> Option<usize> {
    let mut at = 0usize;
    while let Some(found) = find(body, b"stream", at) {
        let before_ok = found == 0 || matches!(body[found - 1], b'\r' | b'\n');
        let after = found + b"stream".len();
        let after_ok = match body.get(after) {
            Some(b'\n') => true,
            Some(b'\r') => matches!(body.get(after + 1), Some(b'\n')),
            _ => false,
        };
        if before_ok && after_ok {
            return Some(found);
        }
        at = found + 1;
    }
    None
}

/// 一个对象：字典部分（可能截到 DICT_CAP）与流正文在文件里的字节区间
#[derive(Debug, Clone)]
pub struct Object {
    pub dict: Vec<u8>,
    pub stream: Option<(usize, usize)>,
    /// 这个对象来自哪个对象流；None 表示文件里明写的
    pub from_stream: Option<u64>,
}

/// 一个对象流的自报情况：对不上就不解，并说明为什么
#[derive(Debug, Clone)]
pub struct ObjectStream {
    pub id: u64,
    pub declared_n: Option<i64>,
    pub first: Option<usize>,
    pub header_pairs: usize,
    pub ok: usize,
    pub error: Option<String>,
}

/// 加密字典能读到的部分。本域**不解密**：没有口令，也不该有
#[derive(Debug, Clone)]
pub struct Encryption {
    pub id: u64,
    pub filter: String,
    pub v: Option<i64>,
    pub revision: Option<i64>,
    pub length_bits: Option<i64>,
    pub has_o: bool,
    pub has_u: bool,
    pub has_owner_entries: bool,
    pub restricted: bool,
}

#[derive(Debug, Clone)]
pub struct Font {
    pub id: u64,
    pub base_font: String,
    pub subtype: String,
    pub encoding: String,
    pub to_unicode: bool,
    pub from_object_stream: bool,
}

#[derive(Debug, Clone)]
pub struct Image {
    pub id: u64,
    pub width: i64,
    pub height: i64,
    pub filter: String,
    pub color_space: String,
    pub bits: i64,
}

#[derive(Debug, Clone)]
pub struct Page {
    pub id: u64,
    pub media_box: Option<String>,
    /// MediaBox 是从父节点继承来的（自己没写）
    pub inherited_box: bool,
    pub rotate: i64,
    pub contents_ref: Option<u64>,
    /// 直接写在页字典里的批注数组长度；写成间接引用时这里是 None（不猜）
    pub annots: Option<usize>,
    /// 批注数组是间接引用时指向的对象号
    pub annots_ref: Option<u64>,
}

pub struct Pdf {
    pub version: String,
    pub binary_comment: bool,
    pub plain: usize,
    pub duplicates: usize,
    pub objects: BTreeMap<u64, Object>,
    pub object_streams: Vec<ObjectStream>,
    pub trailers: Vec<Vec<u8>>,
    pub xref_dicts: Vec<(u64, Vec<u8>)>,
    pub encryption: Option<Encryption>,
    pub notes: Vec<String>,
}

impl Pdf {
    pub fn read(data: &[u8]) -> Pdf {
        let mut notes: Vec<String> = Vec::new();
        let version = if is_pdf(data) {
            let tail = &data[5..std::cmp::min(data.len(), 24)];
            String::from_utf8_lossy(tail)
                .chars()
                .take_while(|ch| ch.is_ascii_digit() || *ch == '.')
                .collect()
        } else {
            String::new()
        };
        let head = &data[..std::cmp::min(data.len(), 32)];
        let binary_comment = find(head, b"%\xe2\xe3\xcf\xdc", 0).is_some();
        let mut plain: BTreeMap<u64, Object> = BTreeMap::new();
        let mut duplicates = 0usize;
        scan_objects(data, &mut plain, &mut duplicates);
        let (inner, streams, stream_notes) = unpack_object_streams(data, &plain);
        notes.extend(stream_notes);
        let mut objects = plain;
        for (id, one) in inner {
            objects.entry(id).or_insert(one);
        }
        // trailer 有两种存在方式：`trailer` 关键字，或者 /Type /XRef 的流字典
        let mut trailers: Vec<Vec<u8>> = Vec::new();
        for at in key_positions(data, b"trailer") {
            let from = at + b"trailer".len();
            let stop = std::cmp::min(data.len(), from + DICT_CAP);
            let body = &data[from..stop];
            let end = find(body, b">>", 0)
                .map(|one| one + 2)
                .unwrap_or(body.len());
            trailers.push(body[..end].to_vec());
        }
        let xref_dicts: Vec<(u64, Vec<u8>)> = objects
            .iter()
            .filter(|(_id, one)| name_after(&one.dict, b"/Type").as_deref() == Some("XRef"))
            .map(|(id, one)| (*id, one.dict.clone()))
            .collect();
        let encryption = find_encryption(&xref_dicts, &trailers, &objects);
        if let Some(one) = &encryption {
            notes.push(format!(
                "这份 PDF 是加密的（/Encrypt 是 {} 0，Filter/{}，V {:?} R {:?}，密钥 {} 位）：\
                 字符串与流正文都是密文，元数据与文本读出来会是乱码，所以这边只报结构、不解密\
                 （没有口令，也不该有）",
                one.id,
                one.filter,
                one.v,
                one.revision,
                one.length_bits.unwrap_or(0)
            ));
        }
        if duplicates > 0 {
            notes.push(format!(
                "有 {duplicates} 个对象号出现了不止一次（修订追加的旧版本）：同号只取第一次出现的那个"
            ));
        }
        if objects.is_empty() {
            notes.push("一个对象都没扫到：文件可能整体损坏，或者对象全在加密层里".to_string());
        }
        let plain_count = objects
            .values()
            .filter(|one| one.from_stream.is_none())
            .count();
        Pdf {
            version,
            binary_comment,
            plain: plain_count,
            duplicates,
            objects,
            object_streams: streams,
            trailers,
            xref_dicts,
            encryption,
            notes,
        }
    }

    /// `/Info` 指向的对象：先信 trailer / XRef 流里的引用，指不到再按内容猜。
    /// 交回对象号与字典本体，两边都要能报出来
    pub fn info_dict(&self) -> Option<(u64, &Object)> {
        for dict in self
            .xref_dicts
            .iter()
            .map(|(_id, dict)| dict)
            .chain(self.trailers.iter())
        {
            if let Some(id) = ref_after(dict, b"/Info") {
                if let Some(one) = self.objects.get(&id) {
                    return Some((id, one));
                }
            }
        }
        self.objects
            .iter()
            .find(|(_id, one)| {
                key_present(&one.dict, b"/Producer") || key_present(&one.dict, b"/CreationDate")
            })
            .map(|(id, one)| (*id, one))
    }

    /// 各个 trailer / XRef 流字典自报的 `/Size`：与 `total_seen` 对账用
    /// （真件里这个数比扫到的对象多 1 是常事 —— 0 号是空闲对象，不占正文）
    pub fn trailer_sizes(&self) -> Vec<i64> {
        self.xref_dicts
            .iter()
            .map(|(_id, dict)| dict.as_slice())
            .chain(self.trailers.iter().map(|one| one.as_slice()))
            .filter_map(|one| int_after(one, b"/Size"))
            .collect()
    }

    /// `/Info` 的对象号（可能指着一个不存在的对象：那也要照实报）
    pub fn info_ref(&self) -> Option<u64> {
        for dict in self
            .xref_dicts
            .iter()
            .map(|(_id, dict)| dict)
            .chain(self.trailers.iter())
        {
            if let Some(id) = ref_after(dict, b"/Info") {
                return Some(id);
            }
        }
        None
    }

    /// 找 `/Root`：同样两处都找。交回对象号，页树要从这里走
    pub fn root_id(&self) -> Option<u64> {
        for dict in self
            .xref_dicts
            .iter()
            .map(|(_id, dict)| dict)
            .chain(self.trailers.iter())
        {
            if let Some(id) = ref_after(dict, b"/Root") {
                return Some(id);
            }
        }
        None
    }

    pub fn catalogs(&self) -> Vec<u64> {
        self.objects
            .iter()
            .filter(|(_id, one)| name_after(&one.dict, b"/Type").as_deref() == Some("Catalog"))
            .map(|(id, _one)| *id)
            .collect()
    }

    /// 页对象：`/Type/Page`。`/Pages` 是树节点，不算页 —— 这条区别全靠
    /// [`key_positions`] 的「名字整段比对」，用子串匹配一定会数错
    pub fn pages(&self) -> Vec<u64> {
        self.type_ids(b"Page")
    }

    pub fn pages_nodes(&self) -> Vec<u64> {
        self.type_ids(b"Pages")
    }

    fn type_ids(&self, want: &[u8]) -> Vec<u64> {
        let want = String::from_utf8_lossy(want).into_owned();
        self.objects
            .iter()
            .filter(|(_id, one)| name_after(&one.dict, b"/Type").as_deref() == Some(want.as_str()))
            .map(|(id, _one)| *id)
            .collect()
    }

    /// 沿 `/Parent` 往上找第一个写了这项的节点：`MediaBox`、`Rotate` 是可继承的。
    /// 最多 8 跳，且同一个父节点不走两遍（文件自己成环时要停）
    fn inherited(
        &self,
        page: &Object,
        want: fn(&[u8]) -> Option<String>,
    ) -> (Option<String>, bool) {
        let mut current = page.dict.clone();
        let mut seen: BTreeSet<u64> = BTreeSet::new();
        let mut hops = 0usize;
        loop {
            if let Some(value) = want(&current) {
                return (Some(value), hops > 0);
            }
            hops += 1;
            if hops > 8 {
                return (None, false);
            }
            let Some(parent) = ref_after(&current, b"/Parent") else {
                return (None, false);
            };
            if !seen.insert(parent) {
                return (None, false);
            }
            match self.objects.get(&parent) {
                Some(one) => current = one.dict.clone(),
                None => return (None, false),
            }
        }
    }

    /// 每页一份账：盒子（含继承）、旋转、正文流、批注条数
    pub fn page_facts(&self) -> Vec<Page> {
        let mut out = Vec::new();
        for id in self.pages() {
            let Some(one) = self.objects.get(&id) else {
                continue;
            };
            let (media_box, inherited_box) = self.inherited(one, |dict| box_of(dict, b"/MediaBox"));
            let (rotate, _was) = self.inherited(one, |dict| {
                int_after(dict, b"/Rotate").map(|one| one.to_string())
            });
            out.push(Page {
                id,
                media_box,
                inherited_box,
                rotate: rotate
                    .unwrap_or_else(|| "0".to_string())
                    .parse()
                    .unwrap_or(0),
                contents_ref: ref_after(&one.dict, b"/Contents"),
                annots: refs_in_array(&one.dict, b"/Annots"),
                annots_ref: ref_after(&one.dict, b"/Annots"),
            });
        }
        out
    }

    /// 按对象号找对象
    pub fn object(&self, id: u64) -> Option<&Object> {
        self.objects.get(&id)
    }

    /// 页树 `/Kids` 的真实顺序 —— 对象号顺序不等于阅读顺序。
    /// 树走不通（缺 `/Root`、缺 `/Kids`、或全指向看不见的对象）时退回对象号顺序，
    /// 第二个返回值说明这次是不是从树来的
    pub fn page_order(&self) -> (Vec<u64>, bool) {
        let root = match self.root_id().and_then(|id| self.objects.get(&id)) {
            Some(one) => one,
            None => return (self.pages(), false),
        };
        let start = match ref_after(&root.dict, b"/Pages") {
            Some(one) => one,
            None => return (self.pages(), false),
        };
        let mut order: Vec<u64> = Vec::new();
        let mut queue: Vec<u64> = vec![start];
        let mut seen: BTreeSet<u64> = BTreeSet::new();
        let mut steps = 0usize;
        while !queue.is_empty() {
            let node = queue.remove(0);
            steps += 1;
            if steps > 200_000 || !seen.insert(node) {
                continue;
            }
            let one = match self.objects.get(&node) {
                Some(done) => done,
                None => continue,
            };
            if name_after(&one.dict, b"/Type").as_deref() == Some("Page") {
                order.push(node);
                continue;
            }
            let mut kids = refs_of(&one.dict, b"/Kids");
            kids.reverse();
            for kid in kids {
                queue.insert(0, kid);
            }
        }
        if order.is_empty() {
            return (self.pages(), false);
        }
        (order, true)
    }

    /// 这一页叫得到的字体名 → 对象号。两道跳转都可能写成间接引用：`/Resources N 0 R`
    /// 与 `/Font 66 0 R`（LibreOffice 两处都用间接）。只认内联那一种就会整页解不出字，
    /// 而且解不出得很安静 —— 这条是量出来的，不是想到的。
    pub fn page_font_names(&self, page: &Object) -> BTreeMap<String, u64> {
        let mut out = BTreeMap::new();
        let resource = match ref_after(&page.dict, b"/Resources") {
            Some(id) => match self.objects.get(&id) {
                Some(one) => one,
                None => page,
            },
            None => page,
        };
        let table: Vec<u8> = match ref_after(&resource.dict, b"/Font") {
            Some(id) => match self.objects.get(&id) {
                Some(one) => one.dict.clone(),
                None => return out,
            },
            None => {
                let at = match key_positions(&resource.dict, b"/Font").first() {
                    Some(one) => *one,
                    None => return out,
                };
                let from = skip_spaces(&resource.dict, at + b"/Font".len());
                let start = match resource.dict.get(from) {
                    Some(b'<') => from + 2,
                    _ => return out,
                };
                let stop = find(&resource.dict, b">>", start).unwrap_or(resource.dict.len());
                resource.dict[start..stop].to_vec()
            }
        };
        for (name, id) in name_refs(&table) {
            out.insert(name, id);
        }
        out
    }

    /// 每页一份正文，按 `/Kids` 的顺序
    pub fn page_texts(&self, data: &[u8]) -> (Vec<(u64, String)>, bool) {
        let (order, from_tree) = self.page_order();
        let mut out: Vec<(u64, String)> = Vec::new();
        let mut fonts: BTreeMap<String, FontMap> = BTreeMap::new();
        for id in order {
            let page = match self.objects.get(&id) {
                Some(one) => one,
                None => continue,
            };
            for (name, target) in self.page_font_names(page) {
                if let Some(one) = self.objects.get(&target) {
                    fonts.insert(name, font_map(data, self, one));
                }
            }
            let mut runs: Vec<TextRun> = Vec::new();
            for content in refs_of(&page.dict, b"/Contents") {
                let one = match self.objects.get(&content) {
                    Some(done) => done,
                    None => continue,
                };
                let raw = match stream_body(data, one) {
                    Some(done) => done,
                    None => continue,
                };
                runs.extend(text_runs(&raw, &fonts));
            }
            let size = runs.iter().fold(
                12.0f64,
                |best, one| if one.size > best { one.size } else { best },
            );
            out.push((id, layout(&runs, size)));
        }
        (out, from_tree)
    }

    /// `/Count` 自报的页数：与真的 `/Type/Page` 对象数对账用
    pub fn declared_counts(&self) -> Vec<i64> {
        self.pages_nodes()
            .iter()
            .filter_map(|id| self.objects.get(id))
            .filter_map(|one| int_after(&one.dict, b"/Count"))
            .collect()
    }

    pub fn fonts(&self) -> Vec<Font> {
        let mut out = Vec::new();
        for (id, one) in &self.objects {
            if name_after(&one.dict, b"/Type").as_deref() != Some("Font") {
                continue;
            }
            out.push(Font {
                id: *id,
                base_font: name_after(&one.dict, b"/BaseFont").unwrap_or_default(),
                subtype: name_after(&one.dict, b"/Subtype").unwrap_or_default(),
                encoding: name_after(&one.dict, b"/Encoding").unwrap_or_default(),
                to_unicode: ref_after(&one.dict, b"/ToUnicode").is_some(),
                from_object_stream: one.from_stream.is_some(),
            });
        }
        out.sort_by_key(|one| one.id);
        out
    }

    pub fn images(&self) -> Vec<Image> {
        let mut out = Vec::new();
        for (id, one) in &self.objects {
            if name_after(&one.dict, b"/Subtype").as_deref() != Some("Image") {
                continue;
            }
            out.push(Image {
                id: *id,
                width: int_after(&one.dict, b"/Width").unwrap_or(0),
                height: int_after(&one.dict, b"/Height").unwrap_or(0),
                filter: name_after(&one.dict, b"/Filter").unwrap_or_default(),
                color_space: name_after(&one.dict, b"/ColorSpace").unwrap_or_default(),
                bits: int_after(&one.dict, b"/BitsPerComponent").unwrap_or(0),
            });
        }
        out.sort_by_key(|one| one.id);
        out
    }

    /// 风险面：PDF 的「宏」是脚本与动作，不是 VBA
    pub fn features(&self) -> Features {
        let mut out = Features::default();
        for one in self.objects.values() {
            let dict = &one.dict;
            out.javascript += count_key(dict, b"/JavaScript");
            out.launch += count_name(dict, b"/S", "Launch");
            out.submit_form += count_name(dict, b"/S", "SubmitForm");
            out.import_data += count_name(dict, b"/S", "ImportData");
            out.goto_remote += count_name(dict, b"/S", "GoToR");
            out.uri += count_name(dict, b"/S", "URI");
            out.filespec += count_name(dict, b"/Type", "Filespec");
            out.acroform += count_key(dict, b"/AcroForm");
            out.embedded_files_tree += count_key(dict, b"/EmbeddedFiles");
            out.open_action += count_key(dict, b"/OpenAction");
            out.additional_actions += count_key(dict, b"/AA");
            out.fields += count_key(dict, b"/FT");
            out.xfa += count_key(dict, b"/XFA");
            if let Some(kind) = name_after(dict, b"/Subtype") {
                out.note_annot(&kind);
            }
        }
        out.dangerous = out.javascript
            + out.launch
            + out.submit_form
            + out.import_data
            + out.goto_remote
            + out.additional_actions;
        out
    }
}

/// 批注与动作的账：只认这一族里常见的名字，其余归到 other（不照名字表穷举）
#[derive(Debug, Default, Clone)]
pub struct Features {
    pub javascript: usize,
    pub launch: usize,
    pub submit_form: usize,
    pub import_data: usize,
    pub goto_remote: usize,
    pub uri: usize,
    pub filespec: usize,
    pub acroform: usize,
    pub embedded_files_tree: usize,
    pub open_action: usize,
    pub additional_actions: usize,
    pub fields: usize,
    pub xfa: usize,
    pub links: usize,
    pub widgets: usize,
    pub other_annot: usize,
    pub dangerous: usize,
}

impl Features {
    fn note_annot(&mut self, kind: &str) {
        match kind {
            "Link" => self.links += 1,
            "Widget" => self.widgets += 1,
            "Popup" | "Text" | "Stamp" | "FreeText" | "Highlight" | "Underline" | "Squiggly"
            | "StrikeOut" | "Caret" | "FileAttachment" => self.other_annot += 1,
            _ => {}
        }
    }
}

fn box_of(dict: &[u8], key: &[u8]) -> Option<String> {
    for at in key_positions(dict, key) {
        let from = skip_spaces(dict, at + key.len());
        if dict.get(from) != Some(&b'[') {
            continue;
        }
        let Some(close) = find(dict, b"]", from) else {
            continue;
        };
        let raw = String::from_utf8_lossy(&dict[from + 1..close]);
        return Some(raw.split_whitespace().collect::<Vec<_>>().join(" "));
    }
    None
}

/// `/Annots[5 0 R 6 0 R]` 里有几个引用：按单独一个 `R` 这个词数，数字母 R 会数错。
/// 键后面第一个非空白字符不是 `[` 时（写成间接引用）交回 None，不去后面找方括号
fn refs_in_array(dict: &[u8], key: &[u8]) -> Option<usize> {
    let want = b"R".as_slice();
    for at in key_positions(dict, key) {
        let from = skip_spaces(dict, at + key.len());
        if dict.get(from) != Some(&b'[') {
            return None;
        }
        let close = find(dict, b"]", from)?;
        return Some(
            dict[from + 1..close]
                .split(|one| is_space(*one))
                .filter(|one| *one == want)
                .count(),
        );
    }
    None
}

/// 文件里明写的 `N G obj … endobj`
fn scan_objects(data: &[u8], out: &mut BTreeMap<u64, Object>, duplicates: &mut usize) {
    let mut at = 0usize;
    while at < data.len() {
        let digit_at = match data[at..].iter().position(|one| one.is_ascii_digit()) {
            Some(step) => at + step,
            None => break,
        };
        if digit_at > 0 && !is_space(data[digit_at - 1]) {
            at = digit_at + 1;
            continue;
        }
        let mut i = digit_at;
        while i < data.len() && data[i].is_ascii_digit() {
            i += 1;
        }
        let gen_at = skip_spaces(data, i);
        let mut j = gen_at;
        while j < data.len() && data[j].is_ascii_digit() {
            j += 1;
        }
        if j == gen_at {
            at = i + 1;
            continue;
        }
        let word_at = skip_spaces(data, j);
        let obj_here =
            data[word_at..].starts_with(b"obj") && !data.get(word_at + 3).is_some_and(is_name_char);
        if !obj_here {
            at = i + 1;
            continue;
        }
        let id: u64 = String::from_utf8_lossy(&data[digit_at..i])
            .parse()
            .unwrap_or(0);
        let after_obj = word_at + b"obj".len();
        let end = find(data, b"endobj", after_obj).unwrap_or(data.len());
        let body = &data[after_obj..end];
        let dict_end = stream_keyword(body).unwrap_or(body.len());
        let dict = body[..std::cmp::min(dict_end, DICT_CAP)].to_vec();
        let stream = if dict_end < body.len() {
            let keyword = after_obj + dict_end + b"stream".len();
            let data_at = match data.get(keyword) {
                Some(b'\r') => keyword + 2,
                Some(_one) => keyword + 1,
                None => keyword,
            };
            let stop = find(data, b"endstream", data_at).unwrap_or(end);
            Some((data_at, stop.max(data_at)))
        } else {
            None
        };
        if out.contains_key(&id) {
            *duplicates += 1;
        } else {
            out.insert(
                id,
                Object {
                    dict,
                    stream,
                    from_stream: None,
                },
            );
        }
        at = end + b"endobj".len();
    }
}

/// 按 `endstream` 定界取出流正文，需要时 zlib 解压（FlateDecode 带 zlib 头，
/// 与 ZIP 里的裸 deflate 不同，所以这里用 `ZlibDecoder`）
fn stream_body(data: &[u8], one: &Object) -> Option<Vec<u8>> {
    let (from, stop) = one.stream?;
    let raw = data.get(from..stop)?.to_vec();
    let mut raw = raw;
    while raw.ends_with(b"\n") || raw.ends_with(b"\r") {
        raw.pop();
    }
    if !key_present(&one.dict, b"/FlateDecode") {
        return Some(raw);
    }
    let mut decoder = ZlibDecoder::new(&raw[..]).take(STREAM_CAP);
    let mut done = Vec::new();
    if decoder.read_to_end(&mut done).is_err() {
        return None;
    }
    Some(done)
}

/// 对象流那一层：`/N`、`/First` 与头部数组三样必须自洽，否则整流作废并说明原因
fn unpack_object_streams(
    data: &[u8],
    plain: &BTreeMap<u64, Object>,
) -> (BTreeMap<u64, Object>, Vec<ObjectStream>, Vec<String>) {
    let mut inner: BTreeMap<u64, Object> = BTreeMap::new();
    let mut streams: Vec<ObjectStream> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    for (id, one) in plain {
        if name_after(&one.dict, b"/Type").as_deref() != Some("ObjStm") {
            continue;
        }
        let declared_n = int_after(&one.dict, b"/N");
        let first = int_after(&one.dict, b"/First").map(|one| one.max(0) as usize);
        let mut record = ObjectStream {
            id: *id,
            declared_n,
            first,
            header_pairs: 0,
            ok: 0,
            error: None,
        };
        let raw = match stream_body(data, one) {
            Some(done) => done,
            None => {
                record.error = Some("流正文解不出来".to_string());
                notes.push(format!(
                    "{id} 0 是对象流（/Type /ObjStm），但它的流正文解不出来：住在里面的对象一个也看不见"
                ));
                streams.push(record);
                continue;
            }
        };
        let first_at = match first {
            Some(done) if done <= raw.len() => done,
            _ => {
                record.error = Some("缺 /First 或 /First 超出正文".to_string());
                notes.push(format!(
                    "{id} 0 对象流的 /First 用不了（正文 {} 字节），无法定位里面的对象",
                    raw.len()
                ));
                streams.push(record);
                continue;
            }
        };
        let pairs = split_numbers(&raw[..first_at]);
        record.header_pairs = pairs.len();
        if let Some(n) = declared_n {
            if n.max(0) as usize != pairs.len() {
                record.error = Some("头部数组的个数与 /N 不符".to_string());
                notes.push(format!(
                    "{id} 0 对象流自报 /N {n}，头部数组里却有 {} 对 —— 按数组实际算",
                    pairs.len()
                ));
            }
        }
        let mut bounds: Vec<usize> = pairs.iter().map(|(_num, off)| first_at + off).collect();
        bounds.push(raw.len());
        let mut taken = 0usize;
        for index in 0..pairs.len() {
            let (num, _off) = pairs[index];
            let from = (*bounds.get(index).unwrap_or(&raw.len())).min(raw.len());
            let stop = (*bounds.get(index + 1).unwrap_or(&raw.len()))
                .max(from)
                .min(raw.len());
            let body = raw[from..stop].to_vec();
            let dict_end = stream_keyword(&body).unwrap_or(body.len());
            inner.insert(
                num,
                Object {
                    dict: body[..std::cmp::min(dict_end, DICT_CAP)].to_vec(),
                    stream: None,
                    from_stream: Some(*id),
                },
            );
            taken += 1;
        }
        record.ok = taken;
        streams.push(record);
    }
    (inner, streams, notes)
}

/// 头部数组就是一串「对象号 偏移」：落单的号（末尾少一个偏移）丢掉，不错配到下一对
fn split_numbers(raw: &[u8]) -> Vec<(u64, usize)> {
    let mut out = Vec::new();
    let mut index = 0usize;
    while index < raw.len() {
        if !raw[index].is_ascii_digit() {
            index += 1;
            continue;
        }
        let start = index;
        while index < raw.len() && raw[index].is_ascii_digit() {
            index += 1;
        }
        let value: u64 = String::from_utf8_lossy(&raw[start..index])
            .parse()
            .unwrap_or(0);
        let mut next = index;
        while next < raw.len() && !raw[next].is_ascii_digit() {
            next += 1;
        }
        let tail_start = next;
        while next < raw.len() && raw[next].is_ascii_digit() {
            next += 1;
        }
        if tail_start == next {
            continue;
        }
        let off: usize = String::from_utf8_lossy(&raw[tail_start..next])
            .parse()
            .unwrap_or(0);
        out.push((value, off));
        index = next;
    }
    out
}

/// `/MarkInfo<</Marked true>>`：`true` 是关键字不是字符串，所以要看看 `/Marked`
/// 后面那个词。写成间接引用（`/MarkInfo 30 0 R`）时这里读不到，交回 false
pub fn marked_true(dict: &[u8]) -> bool {
    key_positions(dict, b"/Marked")
        .iter()
        .any(|at| dict[skip_spaces(dict, at + b"/Marked".len())..].starts_with(b"true"))
}

/// 这个目录里有没有某一项（`/StructTreeRoot` 这类只问在不在的键）
pub fn has_key(dict: &[u8], key: &[u8]) -> bool {
    key_present(dict, key)
}

/// `/Encrypt` 通常在 trailer（或 XRef 流字典）里。只报「加密了、参数是什么」，不解密
fn find_encryption(
    xref_dicts: &[(u64, Vec<u8>)],
    trailers: &[Vec<u8>],
    objects: &BTreeMap<u64, Object>,
) -> Option<Encryption> {
    let places = xref_dicts
        .iter()
        .map(|(_id, dict)| dict.as_slice())
        .chain(trailers.iter().map(|one| one.as_slice()));
    let id = places
        .filter_map(|one| ref_after(one, b"/Encrypt"))
        .next()?;
    let one = objects.get(&id)?;
    let dict = &one.dict;
    Some(Encryption {
        id,
        filter: name_after(dict, b"/Filter").unwrap_or_default(),
        v: int_after(dict, b"/V"),
        revision: int_after(dict, b"/R"),
        length_bits: int_after(dict, b"/Length"),
        has_o: key_present(dict, b"/O"),
        has_u: key_present(dict, b"/U"),
        has_owner_entries: key_present(dict, b"/OE") || key_present(dict, b"/UE"),
        restricted: key_present(dict, b"/P"),
    })
}

// ── 内容流：文本抽取那一层 ──────────────────────────────────────────
//
// 这一层的难度不在「认字」（认字是 `/ToUnicode` 那张表的事），在**位置**上。
// 位置这条路上有三个坑，每一个都是拿真件与 `pdftotext` 逐行对出来的：
//
// 1. `BT` 把文本矩阵与文本行矩阵都复位成单位阵。不复位就会把上一行的坐标一路累加，
//    同一行的三段会被分到三个「行」里；
// 2. 字形前进只沿 x 走：`e' = e + a·step`、`f' = f + b·step`。拿 `d` 去加就变成每个字
//    往上飘 —— 第一版就是这么把 `124000` 六个数字摆成一条斜线的；
// 3. **`TJ` 数组里那个数是反着用的**：正数把笔往左推、负数往右推。符号当 normal 处理，
//    `一级标题：预算口径` 会被排成 `一：算口径级标题预` —— 字全对、顺序全错，
//    这种错最像「读得出但读不通」。
//
// 还有两条明说的界：图形矩阵（`cm`）与字距缩放（`Tz` / `Tw` / `Tc`）不跟；CID 字体
// （2 字节码、宽度住在 `/W` 数组里）的宽度也不跟，那种字体认得出字但位置会偏。
// 手上五份 fixture 里没有这两种，所以是照实说「没做」，不是假装做过。

/// 内容流的一个记号
#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Str(Vec<u8>),
    Name(String),
    Num(f64),
    ArrayStart,
    ArrayEnd,
    /// `<<` 或 `>>`：属性字典（`/Span<</MCID 0>>BDC`）与文本无关，见到就丢自变量
    Dict,
    Op(String),
}

fn is_delimiter(one: u8) -> bool {
    is_space(one)
        || matches!(
            one,
            b'/' | b'<' | b'>' | b'[' | b']' | b'(' | b')' | b'{' | b'}' | b'%'
        )
}

pub fn tokens(raw: &[u8]) -> Vec<Tok> {
    let mut out: Vec<Tok> = Vec::new();
    let mut at = 0usize;
    while at < raw.len() {
        let ch = raw[at];
        if ch == b'%' {
            at = find(raw, b"\n", at).map(|one| one + 1).unwrap_or(raw.len());
            continue;
        }
        if is_space(ch) || ch == b'{' || ch == b'}' {
            at += 1;
            continue;
        }
        if ch == b'(' {
            let (value, next) = literal(raw, at + 1);
            out.push(Tok::Str(value));
            at = next;
            continue;
        }
        if ch == b'<' {
            if raw.get(at + 1) == Some(&b'<') {
                out.push(Tok::Dict);
                at += 2;
                continue;
            }
            let Some(close) = find(raw, b">", at) else {
                break;
            };
            if let Some(value) = hex_bytes(&raw[at + 1..close]) {
                out.push(Tok::Str(value));
            }
            at = close + 1;
            continue;
        }
        if ch == b'>' {
            if raw.get(at + 1) == Some(&b'>') {
                out.push(Tok::Dict);
                at += 2;
            } else {
                at += 1;
            }
            continue;
        }
        if ch == b'[' {
            out.push(Tok::ArrayStart);
            at += 1;
            continue;
        }
        if ch == b']' {
            out.push(Tok::ArrayEnd);
            at += 1;
            continue;
        }
        if ch == b'/' {
            let start = at + 1;
            let mut i = start;
            while i < raw.len() && !is_delimiter(raw[i]) {
                i += 1;
            }
            out.push(Tok::Name(
                String::from_utf8_lossy(&raw[start..i]).into_owned(),
            ));
            at = i;
            continue;
        }
        let start = at;
        let mut i = at;
        while i < raw.len() && !is_delimiter(raw[i]) {
            i += 1;
        }
        if i == start {
            at += 1;
            continue;
        }
        let word = String::from_utf8_lossy(&raw[start..i]).into_owned();
        out.push(match word.parse::<f64>() {
            Ok(done) => Tok::Num(done),
            Err(_why) => Tok::Op(word),
        });
        at = i;
    }
    out
}

/// `<FEFF…>` 那一类 UTF-16BE 的十六进制目标：两个字节一个码元
fn utf16_be(raw: &[u8]) -> String {
    if raw.len() % 2 != 0 {
        return raw.iter().map(|one| *one as char).collect();
    }
    let units: Vec<u16> = raw
        .chunks(2)
        .map(|one| u16::from_be_bytes([one[0], one[1]]))
        .collect();
    String::from_utf16_lossy(&units)
}

fn as_int(raw: &[u8]) -> i64 {
    let mut done = [0u8; 8];
    let start = 8usize.saturating_sub(raw.len());
    done[start..].copy_from_slice(raw);
    i64::from_be_bytes(done)
}

fn trim_width(raw: &[u8]) -> usize {
    raw.len().clamp(1, 8)
}

fn key_with_width(value: i64, width: usize) -> Vec<u8> {
    let done = value.to_be_bytes();
    done[8 - width..].to_vec()
}

/// 一张字体的度量与码表：`/ToUnicode` 的 codespace 宽度、bfchar / bfrange 的映射，
/// 外加 `/FirstChar` 与 `/Widths`（算前进量用）
#[derive(Debug, Clone)]
pub struct FontMap {
    pub code_bytes: usize,
    pub table: BTreeMap<Vec<u8>, String>,
    pub first: i64,
    pub widths: Vec<i64>,
}

impl Default for FontMap {
    fn default() -> Self {
        FontMap {
            code_bytes: 1,
            table: BTreeMap::new(),
            first: 0,
            widths: Vec::new(),
        }
    }
}

impl FontMap {
    /// 一段字形码 → 文本：按 codespace 自长向短试查表，查不到按 Latin-1 交回
    pub fn decode(&self, raw: &[u8]) -> String {
        if self.table.is_empty() {
            return raw.iter().map(|one| *one as char).collect();
        }
        let mut out = String::new();
        let mut at = 0usize;
        while at < raw.len() {
            let longest = std::cmp::min(self.code_bytes.max(1), raw.len() - at);
            let mut hit: Option<(String, usize)> = None;
            for size in (1..=longest).rev() {
                if let Some(found) = self.table.get(&raw[at..at + size]) {
                    hit = Some((found.clone(), size));
                    break;
                }
            }
            match hit {
                Some((text, size)) => {
                    out.push_str(&text);
                    at += size;
                }
                None => {
                    out.push(raw[at] as char);
                    at += 1;
                }
            }
        }
        out
    }

    /// 这段字形码前进多少 em（`/Widths` 里没有的就是 0；CID 字体的 `/W` 不跟）
    fn advance_em(&self, raw: &[u8]) -> f64 {
        let mut out = 0.0;
        let mut at = 0usize;
        while at < raw.len() {
            let longest = std::cmp::min(self.code_bytes.max(1), raw.len() - at);
            let mut size = 1usize;
            for want in (1..=longest).rev() {
                if self.table.contains_key(&raw[at..at + want]) {
                    size = want;
                    break;
                }
            }
            let code = as_int(&raw[at..at + size]);
            let index = code - self.first;
            if index >= 0 {
                if let Some(done) = self.widths.get(index as usize) {
                    out += *done as f64 / 1000.0;
                }
            }
            at += size;
        }
        out
    }
}

/// 一段里的 `<十六进制>`、`[`、`]`，按出现顺序交回（CMap 的正文就这三种东西）
enum HexTok {
    Hex(Vec<u8>),
    Start,
    End,
}

fn hex_stream(raw: &[u8]) -> Vec<HexTok> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while at < raw.len() {
        match raw[at] {
            b'[' => {
                out.push(HexTok::Start);
                at += 1;
            }
            b']' => {
                out.push(HexTok::End);
                at += 1;
            }
            b'<' => {
                let Some(close) = find(raw, b">", at) else {
                    break;
                };
                if let Some(done) = hex_bytes(&raw[at + 1..close]) {
                    out.push(HexTok::Hex(done));
                }
                at = close + 1;
            }
            _ => at += 1,
        }
    }
    out
}

fn blocks<'a>(raw: &'a [u8], begin: &[u8], end: &[u8]) -> Vec<&'a [u8]> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while let Some(from) = find(raw, begin, at) {
        let body = from + begin.len();
        let Some(stop) = find(raw, end, body) else {
            break;
        };
        out.push(&raw[body..stop]);
        at = stop + end.len();
    }
    out
}

fn number_list(raw: &[u8]) -> Vec<i64> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while at < raw.len() {
        let mut i = at;
        let mut sign = 1i64;
        if raw.get(i) == Some(&b'-') {
            sign = -1;
            i += 1;
        }
        let start = i;
        while i < raw.len() && raw[i].is_ascii_digit() {
            i += 1;
        }
        if i == start {
            at += 1;
            continue;
        }
        let digits = String::from_utf8_lossy(&raw[start..i])
            .parse::<i64>()
            .unwrap_or(0);
        out.push(sign * digits);
        at = i;
    }
    out
}

/// 读一张字体的 `/ToUnicode`（没有就只带宽度表来）
pub fn font_map(data: &[u8], doc: &Pdf, font: &Object) -> FontMap {
    let dict = &font.dict;
    let mut done = FontMap::default();
    done.first = int_after(dict, b"/FirstChar").unwrap_or(0);
    if let Some(at) = key_positions(dict, b"/Widths").first() {
        let from = skip_spaces(dict, *at + b"/Widths".len());
        if dict.get(from) == Some(&b'[') {
            if let Some(close) = find(dict, b"]", from) {
                done.widths = number_list(&dict[from + 1..close]);
            }
        }
    }
    let target = match ref_after(dict, b"/ToUnicode") {
        Some(one) => one,
        None => return done,
    };
    let cmap = match doc.object(target) {
        Some(one) => one,
        None => return done,
    };
    let raw = match stream_body(data, cmap) {
        Some(one) => one,
        None => return done,
    };
    for one in blocks(&raw, b"begincodespacerange", b"endcodespacerange") {
        if let Some(HexTok::Hex(lower)) = hex_stream(one).first() {
            done.code_bytes = lower.len().max(1);
        }
    }
    for one in blocks(&raw, b"beginbfchar", b"endbfchar") {
        let items: Vec<Vec<u8>> = hex_stream(one)
            .into_iter()
            .filter_map(|tok| match tok {
                HexTok::Hex(done) => Some(done),
                _ => None,
            })
            .collect();
        for pair in items.chunks(2) {
            if pair.len() == 2 {
                done.table.insert(pair[0].clone(), utf16_be(&pair[1]));
            }
        }
    }
    for one in blocks(&raw, b"beginbfrange", b"endbfrange") {
        // `<lo> <hi> <dst>` 与 `<lo> <hi> [<d1> <d2> …]` 两种都要认：前者是区间里
        // 每个码连续加一，后者逐个对应。只认一种就会整段错位。
        let toks = hex_stream(one);
        let mut at = 0usize;
        while at + 1 < toks.len() {
            let (lo, hi) = match (&toks[at], &toks[at + 1]) {
                (HexTok::Hex(a), HexTok::Hex(b)) => (a.clone(), b.clone()),
                _ => {
                    at += 1;
                    continue;
                }
            };
            let low = as_int(&lo);
            let high = as_int(&hi);
            let width = trim_width(&lo);
            let span = (high - low + 1).max(0) as usize;
            match toks.get(at + 2) {
                Some(HexTok::Hex(dst)) => {
                    let base = as_int(dst);
                    for step in 0..span {
                        if let Some(text) = char_of(base + step as i64) {
                            done.table
                                .insert(key_with_width(low + step as i64, width), text.to_string());
                        }
                    }
                    at += 3;
                }
                Some(HexTok::Start) => {
                    let mut index = at + 3;
                    let mut step = 0usize;
                    while index < toks.len() {
                        match &toks[index] {
                            HexTok::End => break,
                            HexTok::Hex(piece) => {
                                if step < span {
                                    done.table.insert(
                                        key_with_width(low + step as i64, width),
                                        utf16_be(piece),
                                    );
                                    step += 1;
                                }
                            }
                            HexTok::Start => {}
                        }
                        index += 1;
                    }
                    at = index + 1;
                }
                _ => at += 2,
            }
        }
    }
    done
}

fn char_of(value: i64) -> Option<char> {
    u32::try_from(value).ok().and_then(char::from_u32)
}

/// 页上一段字：起点、终点、字号与解出来的文本
#[derive(Debug, Clone)]
pub struct TextRun {
    pub x: f64,
    pub y: f64,
    pub end: f64,
    pub size: f64,
    pub text: String,
}

fn moved(matrix: &[f64; 6], tx: f64, ty: f64) -> [f64; 6] {
    let [a, b, c, d, e, f] = *matrix;
    [a, b, c, d, a * tx + c * ty + e, b * tx + d * ty + f]
}

fn push_run(codes: &[u8], font: &FontMap, size: f64, tm: &mut [f64; 6], runs: &mut Vec<TextRun>) {
    if codes.is_empty() {
        return;
    }
    let text = font.decode(codes);
    if text.is_empty() {
        return;
    }
    let (x, y) = (tm[4], tm[5]);
    let step = font.advance_em(codes) * size;
    tm[4] += step * tm[0];
    tm[5] += step * tm[1];
    runs.push(TextRun {
        x,
        y,
        end: tm[4],
        size,
        text,
    });
}

fn bump(kern: f64, size: f64, tm: &mut [f64; 6]) {
    // 符号相反：正数往左、负数往右
    let step = -kern / 1000.0 * size;
    tm[4] += step * tm[0];
    tm[5] += step * tm[1];
}

/// 走一遍内容流，交回每段字的位置
pub fn text_runs(raw: &[u8], fonts: &BTreeMap<String, FontMap>) -> Vec<TextRun> {
    let mut runs: Vec<TextRun> = Vec::new();
    let fallback = FontMap::default();
    let mut font = &fallback;
    let mut size = 10.0f64;
    let mut leading = 0.0f64;
    let mut tm = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    let mut tl = tm;
    let mut operands: Vec<Tok> = Vec::new();
    let mut array: Vec<Tok> = Vec::new();
    let mut in_array = false;

    for token in tokens(raw) {
        match token {
            Tok::ArrayStart => {
                array.clear();
                in_array = true;
            }
            Tok::ArrayEnd => {
                in_array = false;
            }
            Tok::Dict => {
                operands.clear();
            }
            Tok::Op(word) => {
                let list: Vec<f64> = operands
                    .iter()
                    .filter_map(|one| match one {
                        Tok::Num(done) => Some(*done),
                        _ => None,
                    })
                    .collect();
                let last_name = operands.iter().rev().find_map(|one| match one {
                    Tok::Name(done) => Some(done.clone()),
                    _ => None,
                });
                let last_string = operands.iter().rev().find_map(|one| match one {
                    Tok::Str(done) => Some(done.clone()),
                    _ => None,
                });
                match word.as_str() {
                    "BT" => {
                        tm = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
                        tl = tm;
                    }
                    "Td" => {
                        if list.len() >= 2 {
                            tm = moved(&tm, list[list.len() - 2], list[list.len() - 1]);
                            tl = tm;
                        }
                    }
                    "TD" => {
                        if list.len() >= 2 {
                            leading = -list[list.len() - 1];
                            tm = moved(&tm, list[list.len() - 2], list[list.len() - 1]);
                            tl = tm;
                        }
                    }
                    "Tm" => {
                        if list.len() >= 6 {
                            tm = [list[0], list[1], list[2], list[3], list[4], list[5]];
                            tl = tm;
                        }
                    }
                    "TL" => {
                        if let Some(done) = list.last() {
                            leading = *done;
                        }
                    }
                    "T*" | "'" | "\"" => {
                        tm = moved(&tl, 0.0, -leading);
                        tl = tm;
                        if word != "T*" {
                            if let Some(codes) = last_string {
                                push_run(&codes, font, size, &mut tm, &mut runs);
                            }
                        }
                    }
                    "Tf" => {
                        if let Some(name) = last_name {
                            font = fonts.get(&name).unwrap_or(&fallback);
                            if let Some(done) = list.last() {
                                size = *done;
                            }
                        }
                    }
                    "Tj" => {
                        if let Some(codes) = last_string {
                            push_run(&codes, font, size, &mut tm, &mut runs);
                        }
                    }
                    "TJ" => {
                        for piece in array.clone() {
                            match piece {
                                Tok::Str(codes) => push_run(&codes, font, size, &mut tm, &mut runs),
                                Tok::Num(kern) => bump(kern, size, &mut tm),
                                _ => {}
                            }
                        }
                        array.clear();
                    }
                    _ => {}
                }
                operands.clear();
            }
            other => {
                if in_array {
                    array.push(other);
                } else {
                    operands.push(other);
                }
            }
        }
    }
    runs
}

/// 一行行拼出来：y 相近的算同一行（容差跟字号走），行内按起点 x 排
pub fn layout(runs: &[TextRun], size: f64) -> String {
    if runs.is_empty() {
        return String::new();
    }
    let tolerance = if size * 0.5 > 2.0 { size * 0.5 } else { 2.0 };
    let mut lines: Vec<(f64, Vec<(f64, f64, String)>)> = Vec::new();
    for one in runs {
        let hit = lines
            .iter_mut()
            .find(|(base, _items)| (*base - one.y).abs() <= tolerance);
        match hit {
            Some((_base, items)) => items.push((one.x, one.end, one.text.clone())),
            None => lines.push((one.y, vec![(one.x, one.end, one.text.clone())])),
        }
    }
    lines.sort_by(|left, right| compare_y(right.0, left.0));
    let mut out: Vec<String> = Vec::new();
    for (_base, mut items) in lines {
        items.sort_by(|left, right| compare_x(left.0, right.0));
        let mut line = String::new();
        let mut edge: Option<f64> = None;
        for (x, end, text) in items {
            if let Some(previous) = edge {
                if x - previous > 1.0 && !line.ends_with(' ') {
                    line.push(' ');
                }
            }
            line.push_str(&text);
            edge = Some(end);
        }
        out.push(line);
    }
    out.join("\n")
}

fn compare_y(left: f64, right: f64) -> std::cmp::Ordering {
    left.partial_cmp(&right)
        .unwrap_or(std::cmp::Ordering::Equal)
}

fn compare_x(left: f64, right: f64) -> std::cmp::Ordering {
    left.partial_cmp(&right)
        .unwrap_or(std::cmp::Ordering::Equal)
}

/// `/Key[a 0 R b 0 R]` 与 `/Key N G R` 两种写法都收：交回引用到的对象号
fn refs_of(body: &[u8], key: &[u8]) -> Vec<u64> {
    let mut out = Vec::new();
    for at in key_positions(body, key) {
        let from = skip_spaces(body, at + key.len());
        if body.get(from) == Some(&b'[') {
            let stop = find(body, b"]", from).unwrap_or(body.len());
            out.extend(refs_in(&body[from + 1..stop]));
            continue;
        }
        let tail = &body[at..];
        if let Some(done) = ref_after(tail, key) {
            out.push(done);
        }
    }
    out
}

/// 一串里成对出现的 `/名字 N G R`：字体资源表就这个形状
fn name_refs(body: &[u8]) -> Vec<(String, u64)> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while let Some(found) = find(body, b"/", at) {
        let start = found + 1;
        let mut i = start;
        while i < body.len() && is_name_char(body[i]) {
            i += 1;
        }
        if i == start {
            at = found + 1;
            continue;
        }
        let name = String::from_utf8_lossy(&body[start..i]).into_owned();
        let digits_at = skip_spaces(body, i);
        let mut j = digits_at;
        while j < body.len() && body[j].is_ascii_digit() {
            j += 1;
        }
        let gen_at = skip_spaces(body, j);
        let mut k = gen_at;
        while k < body.len() && body[k].is_ascii_digit() {
            k += 1;
        }
        if j > digits_at && k > gen_at && body.get(k) == Some(&b'R') {
            if let Ok(done) = String::from_utf8_lossy(&body[digits_at..j]).parse::<u64>() {
                out.push((name, done));
            }
            at = k + 1;
            continue;
        }
        at = i;
    }
    out
}

/// 一串 `N G R` 里的对象号
fn refs_in(raw: &[u8]) -> Vec<u64> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while at < raw.len() {
        if !raw[at].is_ascii_digit() {
            at += 1;
            continue;
        }
        let start = at;
        while at < raw.len() && raw[at].is_ascii_digit() {
            at += 1;
        }
        let gen_at = skip_spaces(raw, at);
        let mut j = gen_at;
        while j < raw.len() && raw[j].is_ascii_digit() {
            j += 1;
        }
        let word_at = skip_spaces(raw, j);
        if j > gen_at && raw[word_at..].starts_with(b"R") {
            if let Ok(done) = String::from_utf8_lossy(&raw[start..at]).parse::<u64>() {
                out.push(done);
            }
            at = word_at + 1;
            continue;
        }
        at += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(text: &[u8]) -> Vec<u8> {
        text.to_vec()
    }

    #[test]
    fn literal_strings_honor_escapes_and_nested_parens() {
        let body = bytes(b"(a\\(b\\)c\\\\d\\101e)");
        let (raw, next) = literal(&body, 1);
        assert_eq!(raw, b"a(b)c\\Ae");
        assert_eq!(next, body.len());
    }

    #[test]
    fn the_delimiter_is_not_eaten_before_the_string_starts() {
        // `/Key(value)` 里那个 `(` 既是分隔符又是串的开头：吃掉它就只能读出空串
        let body = bytes(b"<</Lang(en-US)/Producer(LibreOffice)>>");
        assert_eq!(one_string(&body, b"/Lang").as_deref(), Some("en-US"));
        assert_eq!(
            one_string(&body, b"/Producer").as_deref(),
            Some("LibreOffice")
        );
    }

    #[test]
    fn hex_strings_carry_utf16_and_odd_digit_counts() {
        let body = bytes(b"<</Title<FEFF5B57>/Subject<616263> >>");
        assert_eq!(one_string(&body, b"/Title").as_deref(), Some("字"));
        assert_eq!(one_string(&body, b"/Subject").as_deref(), Some("abc"));
    }

    #[test]
    fn a_name_key_matches_the_whole_name() {
        let body = bytes(b"<</Pages 1 0 R/Page 2 0 R/Length 3/PageSize[0 0 1 1]>>");
        assert_eq!(name_after(&body, b"/Type"), None);
        assert_eq!(int_after(&body, b"/Length"), Some(3));
        assert_eq!(box_of(&body, b"/PageSize").as_deref(), Some("0 0 1 1"));
        assert!(!key_present(&body, b"/PageX"));
        // `/Page` 与 `/Pages` 是两个键：整段比对才算数
        assert_eq!(ref_after(&body, b"/Pages"), Some(1));
        assert_eq!(ref_after(&body, b"/Page"), Some(2));
    }

    #[test]
    fn the_stream_keyword_needs_line_ends_on_both_sides() {
        // 字典里的字带「stream」时不能当成关键字（见证读者第一版就栽在这儿）
        let body = bytes(b"<</Title(Obj stream fixture)>>");
        assert_eq!(stream_keyword(&body), None);
        let real = bytes(b"<</Length 4>>\nstream\nabcd\nendstream");
        assert_eq!(stream_keyword(&real), Some(15));
    }

    #[test]
    fn the_object_scan_needs_whitespace_before_the_number() {
        let data = bytes(b"%PDF-1.4\n1 0 obj\n<<>>\nendobj\n");
        let mut out = BTreeMap::new();
        let mut dup = 0usize;
        scan_objects(&data, &mut out, &mut dup);
        assert_eq!(out.keys().copied().collect::<Vec<_>>(), vec![1]);
        assert_eq!(dup, 0);
        // 数字粘在上一个词上时不算对象
        let glued = bytes(b"%PDF-1.4\nx12 0 objx\n");
        let mut none = BTreeMap::new();
        scan_objects(&glued, &mut none, &mut dup);
        assert!(none.is_empty());
    }

    #[test]
    fn duplicate_object_numbers_are_counted_not_merged() {
        let data = bytes(b"%PDF-1.4\n1 0 obj\n<</A 1>>\nendobj\n1 0 obj\n<</A 2>>\nendobj\n");
        let mut out = BTreeMap::new();
        let mut dup = 0usize;
        scan_objects(&data, &mut out, &mut dup);
        assert_eq!(out.len(), 1);
        assert_eq!(dup, 1);
    }

    #[test]
    fn an_object_stream_header_pairs_number_with_offset_from_first() {
        let raw = bytes(b"14 0 13 51 10 101");
        assert_eq!(split_numbers(&raw), vec![(14, 0), (13, 51), (10, 101)]);
        // 落单的号（末尾少一个偏移）丢掉，不错配到下一对
        let odd = bytes(b"3 0 4");
        assert_eq!(split_numbers(&odd), vec![(3, 0)]);
    }

    #[test]
    fn page_boxes_follow_the_parent_chain() {
        // risk.pdf 的形状：页自己不写 MediaBox / Rotate，从 /Pages 继承
        let data = bytes(
            b"%PDF-1.7\n1 0 obj\n<</Type/Pages/Kids[2 0 R]/Count 1/MediaBox[0 0 612 792]/Rotate 90>>\nendobj\n\
               2 0 obj\n<</Type/Page/Parent 1 0 R>>\nendobj\n",
        );
        let doc = Pdf::read(&data);
        let pages = doc.page_facts();
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].media_box.as_deref(), Some("0 0 612 792"));
        assert!(pages[0].inherited_box);
        assert_eq!(pages[0].rotate, 90);
        assert_eq!(doc.declared_counts(), vec![1]);
    }

    #[test]
    fn a_parent_cycle_stops_instead_of_spinning() {
        let data = bytes(
            b"%PDF-1.7\n1 0 obj\n<</Type/Page/Parent 2 0 R>>\nendobj\n\
               2 0 obj\n<</Type/Pages/Parent 1 0 R>>\nendobj\n",
        );
        let doc = Pdf::read(&data);
        let pages = doc.page_facts();
        assert_eq!(pages[0].media_box, None);
        assert!(!pages[0].inherited_box);
    }

    #[test]
    fn content_tokens_separate_strings_names_numbers_and_arrays() {
        let raw = bytes(b"BT /F1 14 Tf [<01>-2999<02>]TJ (hi) Tj ET");
        let got = tokens(&raw);
        let shape: Vec<String> = got
            .iter()
            .map(|one| match one {
                Tok::Str(_one) => "str".to_string(),
                Tok::Name(done) => format!("name:{done}"),
                Tok::Num(done) => format!("num:{done}"),
                Tok::ArrayStart => "[".to_string(),
                Tok::ArrayEnd => "]".to_string(),
                Tok::Dict => "dict".to_string(),
                Tok::Op(done) => format!("op:{done}"),
            })
            .collect();
        assert_eq!(
            shape,
            vec![
                "op:BT",
                "name:F1",
                "num:14",
                "op:Tf",
                "[",
                "str",
                "num:-2999",
                "str",
                "]",
                "op:TJ",
                "str",
                "op:Tj",
                "op:ET"
            ]
        );
    }

    #[test]
    fn a_tj_kern_moves_the_pen_the_other_way() {
        // 负数往右推。符号当成正常的那一种，第二段就会落到第一段左边，
        // 一行中文会被排成「字全对、顺序全错」那种最像读通了的答案。
        let mut one = FontMap::default();
        one.table.insert(vec![1u8], "甲".to_string());
        one.table.insert(vec![2u8], "乙".to_string());
        one.widths = vec![0, 1000, 1000];
        let mut fonts = BTreeMap::new();
        fonts.insert("F1".to_string(), one);
        let raw = bytes(b"BT 10 20 Td /F1 10 Tf [<01>-500<02>]TJ ET");
        let runs = text_runs(&raw, &fonts);
        assert_eq!(runs.len(), 2);
        assert!((runs[0].x - 10.0).abs() < 1e-9, "{:?}", runs);
        assert!((runs[1].x - 25.0).abs() < 1e-6, "{:?}", runs);
        assert_eq!(runs[1].text, "乙");
    }

    #[test]
    fn bt_restarts_the_text_matrix_instead_of_carrying_it_over() {
        let raw = bytes(b"BT 10 20 Td (a) Tj ET BT 30 40 Td (b) Tj ET");
        let runs = text_runs(&raw, &BTreeMap::new());
        assert_eq!(runs.len(), 2);
        assert!((runs[1].x - 30.0).abs() < 1e-9, "{:?}", runs);
        assert!((runs[1].y - 40.0).abs() < 1e-9, "{:?}", runs);
    }

    #[test]
    fn glyph_advance_walks_x_and_never_y() {
        // 前进量只加在 e 上（用 d 加就会每字往上飘，124000 被摆成一条斜线）
        let mut one = FontMap::default();
        one.table.insert(vec![1u8], "1".to_string());
        one.widths = vec![0, 553];
        let mut fonts = BTreeMap::new();
        fonts.insert("F5".to_string(), one);
        let raw = bytes(b"BT 311.5 584.8 Td /F5 11 Tf [<01><01><01>]TJ ET");
        let runs = text_runs(&raw, &fonts);
        assert_eq!(runs.len(), 3);
        assert!(
            runs.iter().all(|item| (item.y - 584.8).abs() < 1e-9),
            "{:?}",
            runs
        );
        assert!(runs[2].x > runs[0].x);
    }

    #[test]
    fn lines_group_by_y_with_a_tolerance_tied_to_the_font_size() {
        let runs = vec![
            TextRun {
                x: 95.5,
                y: 580.5,
                end: 128.5,
                size: 11.0,
                text: "服务器".to_string(),
            },
            TextRun {
                x: 311.5,
                y: 584.8,
                end: 348.0,
                size: 11.0,
                text: "124000".to_string(),
            },
            TextRun {
                x: 90.1,
                y: 660.85,
                end: 266.1,
                size: 11.0,
                text: "另一行".to_string(),
            },
        ];
        assert_eq!(layout(&runs, 11.0), "服务器 124000\n另一行");
    }

    #[test]
    fn a_property_dict_does_not_leak_into_the_next_font_choice() {
        // /Span<</MCID 0>>BDC 里的 MCID 不是字体名：属性字典要把自变量清掉
        let raw = bytes(b"/Span<</MCID 0>>BDC BT 10 20 Td /F1 12 Tf (x) Tj ET EMC");
        let mut one = FontMap::default();
        one.table.insert(vec![b'x'], "字".to_string());
        let mut fonts = BTreeMap::new();
        fonts.insert("F1".to_string(), one);
        let runs = text_runs(&raw, &fonts);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].text, "字");
    }

    #[test]
    fn cmap_bfchar_and_both_bfrange_forms_are_parsed() {
        let cmap = b"begincodespacerange\n<00> <FF>\nendcodespacerange\n\
                     2 beginbfchar\n<01> <7532>\n<02> <4E59>\nendbfchar\n\
                     1 beginbfrange\n<10> <12> <0041>\nendbfrange\n\
                     1 beginbfrange\n<20> <21> [<6f62> <6364>]\nendbfrange\n";
        let mut body = Vec::new();
        body.extend_from_slice(b"%PDF-1.4\n");
        body.extend_from_slice(
            b"1 0 obj\n<</Type/Font/Subtype/TrueType/ToUnicode 2 0 R/FirstChar 0/Widths[0 1000 1000 1000]>>\nendobj\n",
        );
        body.extend_from_slice(b"2 0 obj\n<</Length ");
        body.extend_from_slice(cmap.len().to_string().into_bytes());
        body.extend_from_slice(b">>\nstream\n");
        body.extend_from_slice(cmap);
        body.extend_from_slice(b"\nendstream\nendobj\n");
        let doc = Pdf::read(&body);
        let font = doc.object(1).expect("字体对象在");
        let done = font_map(&body, &doc, font);
        assert_eq!(done.code_bytes, 1);
        assert_eq!(done.table.get(&[1u8][..]).map(String::as_str), Some("甲"));
        assert_eq!(done.table.get(&[2u8][..]).map(String::as_str), Some("乙"));
        // 连续加一的那种：0x10→A、0x11→B、0x12→C
        assert_eq!(done.table.get(&[0x10u8][..]).map(String::as_str), Some("A"));
        assert_eq!(done.table.get(&[0x12u8][..]).map(String::as_str), Some("C"));
        // 数组那种：逐个对应，两个字节一个码元
        assert_eq!(
            done.table.get(&[0x20u8][..]).map(String::as_str),
            Some("ob")
        );
        assert_eq!(
            done.table.get(&[0x21u8][..]).map(String::as_str),
            Some("cd")
        );
        assert_eq!(done.advance_em(&[1u8, 2u8]), 2.0);
    }
}
