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
}
