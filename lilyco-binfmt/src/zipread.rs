//! ZIP 成员字节的读取：中央目录给的位置 → 本地头 → 按需解压 → 拿条目自己声明的 CRC-32 复核。
//!
//! 为什么本域非得解压：`docx` / `xlsx` / `pptx` / `odt` 的正文就是包里的一个 XML 部件，
//! 而生产这些文件的程序（Word、LibreOffice、python-docx）默认把部件 deflate 压掉。
//! 只列成员表等于只能报「有个文件叫 word/document.xml」，报不出文件里写了什么。
//!
//! 解压不越本域的界：**只在内存里展开，不落盘、不写回、不执行**（safety 仍是 T0）。
//! 三条硬规矩：
//! 1. 每个解压结果都要过它自己那条 CRC-32 与「解压后长度」，对不上就 `verified=false`
//!    并把原因交出去 —— 不许悄悄把这条丢掉；
//! 2. 解压上限（`cap`）先算再解：`take(cap+1)` 让 zip 炸弹在自己的上限处停下，
//!    而不是先把 4 GB 摊在内存里再看一眼；
//! 3. 尺寸与 CRC 一律信**中央目录**而不是本地头 —— 带 data descriptor 的流式打包
//!    会把本地头里的长度与 CRC 写成 0。

use std::io::Read;

use crate::read::{central_directory, le16, ZipEntry};

/// 解压后允许的最大字节数：办公文件的单个 XML 部件超过这个数就不是「文档」了
pub const DEFAULT_MEMBER_CAP: u64 = 64 * 1024 * 1024;

/// 一个成员：字节 + 它自己声明的那套账（压缩方法 / 长度 / CRC）。
/// `data` 是**解压后**的字节（stored 成员就是原样）。
#[derive(Debug)]
pub struct Member {
    pub name: String,
    pub method: u64,
    pub method_name: String,
    pub size: u64,
    pub compressed: u64,
    pub crc_stated: u64,
    pub crc_actual: u64,
    pub verified: bool,
    pub note: String,
    pub data: Vec<u8>,
}

impl Member {
    /// 按 UTF-8 读这份字节（OOXML / ODF 的部件都是 UTF-8）。解不出来时给 lossy 结果，
    /// 但「解不出来」这件事留在调用方自己判断 —— 见 `is_utf8`。
    pub fn as_text(&self) -> String {
        match std::str::from_utf8(&self.data) {
            Ok(text) => text.to_string(),
            Err(_) => String::from_utf8_lossy(&self.data).into_owned(),
        }
    }

    pub fn is_utf8(&self) -> bool {
        std::str::from_utf8(&self.data).is_ok()
    }
}

/// CRC-32（IEEE 802.3 反射式，ZIP 用的那个多项式）。逐位、不建表：
/// 成员体积已经被 `cap` 压住，而少一份静态表就少一处能写错的地方。
pub fn crc32(bytes: &[u8]) -> u64 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for one in bytes {
        crc ^= u32::from(*one);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (mask & 0xEDB8_8320);
        }
    }
    u64::from(!crc)
}

/// 中央目录（连同它自报条数与真实条数是否一致的说法）
pub fn entries(zip: &[u8]) -> (Vec<ZipEntry>, Vec<String>) {
    central_directory(zip)
}

/// 只要名字，不解压（列部件表用，成本就是一次中央目录扫描）
pub fn member_names(zip: &[u8]) -> Vec<String> {
    let (dirs, _) = central_directory(zip);
    dirs.into_iter().map(|one| one.name).collect()
}

/// 名字里含 `needle` 的成员（不解压，只给元数据）
pub fn members_containing(zip: &[u8], needle: &str) -> Vec<ZipEntry> {
    let (dirs, _) = central_directory(zip);
    dirs.into_iter()
        .filter(|one| one.name.contains(needle))
        .collect()
}

/// 在已扫出的条目里按名字找一条。OPC 的部件名永远是 `/` 分隔、不带前导 `/`，
/// 但有些打包器会写成 `./word/document.xml`，所以两个写法都认。
pub fn find_in<'a>(dirs: &'a [ZipEntry], want: &str) -> Option<&'a ZipEntry> {
    let clean = want.trim_start_matches("./");
    dirs.iter().find(|one| {
        let name = one.name.trim_start_matches("./");
        name == clean
    })
}

/// 按名字找条目（自己扫一遍中央目录）。批量读部件请改用 `entries` + `find_in` +
/// `read_member`，别把目录扫 N 遍。
pub fn find_entry(zip: &[u8], want: &str) -> Option<ZipEntry> {
    let (dirs, _) = central_directory(zip);
    find_in(&dirs, want).cloned()
}

/// 读到某个成员的字节并解压。找不到就 `Err`，调用方决定这算「没有这个部件」还是「包坏了」。
pub fn member(zip: &[u8], want: &str, cap: u64) -> Result<Member, String> {
    let (dirs, _) = central_directory(zip);
    let e = find_in(&dirs, want).ok_or_else(|| format!("包里没有成员 `{want}`"))?;
    read_member(zip, e, cap)
}

/// 读一个已知条目。长度与 CRC 一律信中央目录（见模块注释第 3 条）。
pub fn read_member(zip: &[u8], e: &ZipEntry, cap: u64) -> Result<Member, String> {
    let cap = if cap == 0 { DEFAULT_MEMBER_CAP } else { cap };
    let off = usize::try_from(e.offset).map_err(|_| format!("offset {} 放不下", e.offset))?;
    let head = zip
        .get(off..off + 30)
        .ok_or_else(|| format!("`{}` 的本地头越界", e.name))?;
    if &head[0..4] != b"PK\x03\x04" {
        return Err(format!("`{}` 的本地头不在它自报的 offset {off} 上", e.name));
    }
    // 本地头自己声明的名字/扩展字段长度：位置是 off+26 与 off+28，不是文件头的 26 / 28
    let nlen =
        usize::try_from(le16(off + 26)(zip).ok_or("本地头读不到名字长度")?).unwrap_or(usize::MAX);
    let elen = usize::try_from(le16(off + 28)(zip).ok_or("本地头读不到扩展字段长度")?)
        .unwrap_or(usize::MAX);
    let csize = usize::try_from(e.compressed).map_err(|_| "压缩长度放不下".to_string())?;
    let Some(start) = off
        .checked_add(30)
        .and_then(|one| one.checked_add(nlen))
        .and_then(|one| one.checked_add(elen))
    else {
        return Err(format!("`{}` 的本地头字段长度溢出", e.name));
    };
    let Some(stop) = start.checked_add(csize) else {
        return Err(format!("`{}` 的压缩长度溢出", e.name));
    };
    let raw = zip.get(start..stop).ok_or_else(|| {
        format!(
            "`{}` 自报压缩长度 {csize} 从 offset {start} 起超出了文件",
            e.name
        )
    })?;
    let (data, method_name) = match e.method {
        0 => (raw.to_vec(), "stored".to_string()),
        8 => (inflate(raw, cap, &e.name)?, "deflate".to_string()),
        other => {
            return Err(format!(
                "`{}` 用的是压缩方法 {other}，本域只读 stored(0) 与 deflate(8)",
                e.name
            ))
        }
    };
    Ok(check(e, data, method_name))
}

/// 把「算出来的」与「条目自报的」并排放，不一致就写进 `note`，不丢掉字节
fn check(e: &ZipEntry, data: Vec<u8>, method_name: String) -> Member {
    let actual = crc32(&data);
    let mut note = String::new();
    if e.crc != actual {
        note.push_str(&format!(
            "CRC-32 对不上：条目自报 {:08x}，解压后算到 {actual:08x}",
            e.crc & 0xFFFF_FFFF
        ));
    }
    if u64::try_from(data.len()).unwrap_or(u64::MAX) != e.size {
        if !note.is_empty() {
            note.push('；');
        }
        note.push_str(&format!(
            "解压后 {} 字节，条目自报 {} 字节",
            data.len(),
            e.size
        ));
    }
    Member {
        name: e.name.clone(),
        method: e.method,
        method_name,
        size: e.size,
        compressed: e.compressed,
        crc_stated: e.crc,
        crc_actual: actual,
        verified: note.is_empty(),
        note,
        data,
    }
}

/// raw deflate（ZIP 存的是不带 zlib 头的裸流，所以是 `DeflateDecoder` 而不是 `ZlibDecoder`）
fn inflate(raw: &[u8], cap: u64, name: &str) -> Result<Vec<u8>, String> {
    let mut decoder = flate2::read::DeflateDecoder::new(raw).take(cap.saturating_add(1));
    let mut out = Vec::new();
    if decoder.read_to_end(&mut out).is_err() {
        return Err(format!("`{name}` 的 deflate 流解不开"));
    }
    if u64::try_from(out.len()).unwrap_or(u64::MAX) > cap {
        return Err(format!(
            "`{name}` 解压后超过上限 {cap} 字节，已在上限处中止（zip 炸弹闸门）"
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 定长 Huffman（BTYPE=01）编码：只出字面量码 + EOB，够把一段 ASCII 编成合法的
    /// raw deflate。测试要的是「真走 deflate 这条路」，不是自己实现一个压缩器。
    /// 码表（RFC 1951 3.2.6）：0-143 → 8 bit = 值+48；144-255 → 9 bit = 值-144+0x190；
    /// 256 (EOB) → 7 bit = 0。Huffman 码按 MSB-first 送出，而 deflate 的比特流
    /// 按 LSB-first 装字节，所以每个码要**位反转**后再写。
    fn deflate_fixed(text: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut buf: u32 = 0;
        let mut cnt: u32 = 0;
        emit(&mut out, &mut buf, &mut cnt, 1, 1); // BFINAL = 1
        emit(&mut out, &mut buf, &mut cnt, 1, 2); // BTYPE = 01（定长码）
        for one in text {
            let (code, len) = if *one <= 143 {
                (u32::from(*one) + 48, 8)
            } else {
                (u32::from(*one) - 144 + 0x190, 9)
            };
            emit(&mut out, &mut buf, &mut cnt, reverse(code, len), len);
        }
        emit(&mut out, &mut buf, &mut cnt, reverse(0, 7), 7); // EOB
        if cnt > 0 {
            out.push(buf as u8);
        }
        out
    }

    fn emit(out: &mut Vec<u8>, buf: &mut u32, cnt: &mut u32, bits: u32, len: u32) {
        *buf |= bits << *cnt;
        *cnt += len;
        while *cnt >= 8 {
            out.push((*buf & 0xFF) as u8);
            *buf >>= 8;
            *cnt -= 8;
        }
    }

    fn reverse(mut value: u32, len: u32) -> u32 {
        let mut out = 0u32;
        for _ in 0..len {
            out = (out << 1) | (value & 1);
            value >>= 1;
        }
        out
    }

    /// 手搓一个 zip：一条 stored、一条 deflate，本地头与中央目录都按自报的算。
    /// 只有 CRC 与长度全对，`read_member` 才会说 `verified=true` —— 这就是这条测试的落点。
    fn zip_fixture() -> Vec<u8> {
        let parts = [
            ("hello.txt", 0u64, "hello office".as_bytes().to_vec()),
            (
                "word/document.xml",
                8u64,
                b"<w:body><w:t>hi</w:t></w:body>".to_vec(),
            ),
        ];
        let mut out = Vec::new();
        let mut central = Vec::new();
        let declared = parts.len();
        for (name, method, body) in parts {
            let offset = out.len() as u64;
            let crc = crc32(&body) as u32;
            let compressed = if method == 8 {
                deflate_fixed(&body)
            } else {
                body.clone()
            };
            out.extend_from_slice(b"PK\x03\x04");
            out.extend_from_slice(&20u16.to_le_bytes()); // version needed
            out.extend_from_slice(&0u16.to_le_bytes()); // flags
            out.extend_from_slice(&(method as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes()); // mod time
            out.extend_from_slice(&0u16.to_le_bytes()); // mod date
            out.extend_from_slice(&crc.to_le_bytes());
            out.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
            out.extend_from_slice(&(body.len() as u32).to_le_bytes());
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes()); // extra len
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(&compressed);

            central.extend_from_slice(b"PK\x01\x02");
            central.extend_from_slice(&20u16.to_le_bytes()); // version made by
            central.extend_from_slice(&20u16.to_le_bytes()); // version needed
            central.extend_from_slice(&0u16.to_le_bytes()); // flags
            central.extend_from_slice(&(method as u16).to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes()); // mod time
            central.extend_from_slice(&0u16.to_le_bytes()); // mod date
            central.extend_from_slice(&crc.to_le_bytes());
            central.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
            central.extend_from_slice(&(body.len() as u32).to_le_bytes());
            central.extend_from_slice(&(name.len() as u16).to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes()); // extra len
            central.extend_from_slice(&0u16.to_le_bytes()); // comment len
            central.extend_from_slice(&0u16.to_le_bytes()); // disk number start
            central.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
            central.extend_from_slice(&0u32.to_le_bytes()); // external attrs
            central.extend_from_slice(&(offset as u32).to_le_bytes());
            central.extend_from_slice(name.as_bytes());
        }
        let cd_at = out.len() as u32;
        out.extend_from_slice(&central);
        out.extend_from_slice(b"PK\x05\x06");
        out.extend_from_slice(&0u16.to_le_bytes()); // this disk
        out.extend_from_slice(&0u16.to_le_bytes()); // disk with cd start
        out.extend_from_slice(&(declared as u16).to_le_bytes());
        out.extend_from_slice(&(declared as u16).to_le_bytes());
        out.extend_from_slice(&(central.len() as u32).to_le_bytes());
        out.extend_from_slice(&cd_at.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // comment len
        out
    }

    #[test]
    fn reads_both_stored_and_deflated_members() {
        let zip = zip_fixture();
        let (dirs, broken) = entries(&zip);
        assert!(broken.is_empty(), "中央目录不自证: {broken:?}");
        assert_eq!(dirs.len(), 2);

        let stored = member(&zip, "hello.txt", 1 << 20).expect("stored 成员读得出");
        assert!(stored.verified, "{}", stored.note);
        assert_eq!(stored.method_name, "stored");
        assert_eq!(stored.data, b"hello office");

        let xml = member(&zip, "word/document.xml", 1 << 20).expect("deflate 成员读得出");
        assert!(xml.verified, "{}", xml.note);
        assert_eq!(xml.method_name, "deflate");
        assert_eq!(xml.as_text(), "<w:body><w:t>hi</w:t></w:body>");
        assert!(xml.is_utf8());
    }

    #[test]
    fn a_missing_member_says_so_instead_of_returning_nothing() {
        let zip = zip_fixture();
        let why = member(&zip, "word/nope.xml", 1 << 20).unwrap_err();
        assert!(why.contains("word/nope.xml"), "{why}");
    }

    /// 中央目录里那条 CRC 被改掉：字节照样读得出来，但 `verified` 必须翻成 false，
    /// 并且把自报值与实算值都写进 note —— 只说「不匹配」等于让人无法判断坏在哪。
    #[test]
    fn a_crc_that_does_not_match_is_reported_not_swallowed() {
        let mut zip = zip_fixture();
        let at = find_cd_field(&zip, "hello.txt", 16).expect("找得到这条中央目录记录");
        zip[at..at + 4].copy_from_slice(&0xDEAD_BEEFu32.to_le_bytes());
        let one = member(&zip, "hello.txt", 1 << 20).expect("字节仍然读得出来");
        assert!(!one.verified);
        assert!(one.note.contains("deadbeef"), "{}", one.note);
        assert!(one.note.contains("CRC-32"), "{}", one.note);
    }

    /// 中央目录里改掉长度自报值：解压后的真实长度与它对不上，也要报出来
    #[test]
    fn a_size_that_does_not_match_is_reported_too() {
        let mut zip = zip_fixture();
        let at = find_cd_field(&zip, "hello.txt", 24).expect("找得到这条中央目录记录");
        zip[at..at + 4].copy_from_slice(&999u32.to_le_bytes());
        let one = member(&zip, "hello.txt", 1 << 20).expect("字节仍然读得出来");
        assert!(!one.verified);
        assert!(one.note.contains("999"), "{}", one.note);
    }

    /// 中央目录记录里偏移 `which` 处的字段（记录起点由名字匹配定出来）
    fn find_cd_field(zip: &[u8], name: &str, which: usize) -> Option<usize> {
        let mut at = 0usize;
        while at + 46 <= zip.len() {
            if zip[at..at + 4] != *b"PK\x01\x02" {
                at += 1;
                continue;
            }
            let nlen = usize::from(le16(at + 28)(zip)? as u16);
            let elen = usize::from(le16(at + 30)(zip)? as u16);
            let clen = usize::from(le16(at + 32)(zip)? as u16);
            let got = String::from_utf8_lossy(zip.get(at + 46..at + 46 + nlen)?).into_owned();
            if got == name {
                return Some(at + which);
            }
            at += 46 + nlen + elen + clen;
        }
        None
    }

    /// 上限闸门：解压到 `cap` 就停，不把整个炸弹摊开
    #[test]
    fn the_cap_stops_a_blowup_before_it_is_fully_expanded() {
        let mut text = Vec::new();
        for _ in 0..4096 {
            text.extend_from_slice(b"aaaaaaaaaaaaaaaa");
        }
        let body = deflate_fixed(&text);
        let why = inflate(&body, 1024, "BOMB.TXT").unwrap_err();
        assert!(why.contains("zip 炸弹闸门"), "{why}");
        let big = inflate(&body, 1 << 20, "BOMB.TXT").expect("上限够就解得开");
        assert_eq!(big.len(), text.len());
        assert_eq!(crc32(&big), crc32(&text));
    }

    #[test]
    fn crc32_matches_the_known_vector() {
        // check 值：CRC-32("123456789") = 0xCBF43926（ZIP / gzip 用的是同一个）
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    /// 本地头不在自报 offset 上（截断包 / offset 被改）要报错，不能把别的字节当成员解压
    #[test]
    fn a_bad_offset_is_an_error_not_a_garbage_member() {
        let zip = zip_fixture();
        let (mut dirs, _) = entries(&zip);
        let one = dirs.first_mut().expect("有第一条");
        one.offset = 4; // 那里是「version needed」，不是 PK\x03\x04
        let why = read_member(&zip, one, 1 << 20).unwrap_err();
        assert!(why.contains("本地头"), "{why}");
    }

    /// `./` 前缀的写法要认得同一件部件（打包器不一致是常态，不是异常）
    #[test]
    fn dotted_slash_names_resolve_to_the_same_part() {
        let zip = zip_fixture();
        let (dirs, _) = entries(&zip);
        assert!(find_in(&dirs, "./word/document.xml").is_some());
    }
}
