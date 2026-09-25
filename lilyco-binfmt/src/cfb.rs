//! MS-CFB（Compound File Binary）：`.doc` / `.xls` / `.ppt` 这些遗留办公文件的容器。
//!
//! 与 [`crate::zipread`] 一样的规矩：**只读字节，不写回、不执行**。这里没有第三种语言
//! 帮忙，所以链的走法完全按规范自己实现，且每一条不自洽都要说出来而不是猜：
//!
//! - 扇区 `n` 的文件偏移永远是 `(n+1) * sector_size`（头占一个扇区）；
//! - `0xFFFFFFFE` 是链结束，`0xFFFFFFFF` 是「没有下一个」（目录项里的 start 用它表示空流），
//!   `0xFFFFFFFD` / `0xFFFFFFFC` 在 FAT 里分别表示「这个扇区是 FAT」与「是 DIFAT」；
//! - 流走哪条链由**大小**决定：`size < mini_cutoff`（通常 4096）的走 miniFAT + 根流的
//!   迷你扇区，其余走 FAT。这条界是容器自己声明的，不是我们选的；
//! - v3 的目录扇区数字段必须是 0，v4 才有 64 位流长度 —— 版本号不对就报错，不猜。
//!
//! 本模块与 `scripts/acceptance/office_reader.py` 里的同名实现是两份独立实现，
//! 两边对同一批真实生产者文件（LibreOffice 写的 .doc/.xls/.ppt）必须给出同样的答案。

use crate::read::{le16, le32, le64};

/// CFB 签名：D0 CF 11 E0 A1 B1 1A E1
pub const SIGNATURE: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

const END_OF_CHAIN: u32 = 0xFFFF_FFFE;
const NO_SECT: u32 = 0xFFFF_FFFF;
/// 链最长走这么多步：畸形文件里一个环就能让读取转到天荒地老
const CHAIN_LIMIT: usize = 1 << 20;

/// 目录项：`kind` 是 spec 的四种之一
#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    pub kind: &'static str,
    pub start: u32,
    pub size: u64,
    pub clsid: String,
    pub created: u64,
    pub modified: u64,
    pub left: u32,
    pub right: u32,
    pub child: u32,
}

impl Entry {
    pub fn is_stream(&self) -> bool {
        self.kind == "stream"
    }
}

/// 一个已打开的复合文档
#[derive(Debug)]
pub struct Cfb {
    pub minor: u64,
    pub major: u64,
    pub sector_size: usize,
    pub mini_sector_size: usize,
    pub mini_cutoff: u64,
    pub fat_sectors: usize,
    pub difat_chained: bool,
    pub entries: Vec<Entry>,
    /// 容器自己说的和实际读到的不一致之处，一律留在这里
    pub notes: Vec<String>,
    fat: Vec<u32>,
    mini_fat: Vec<u32>,
    mini_stream: Vec<u8>,
}

pub fn is_cfb(b: &[u8]) -> bool {
    b.len() >= 512 && b[..8] == SIGNATURE
}

fn type_name(raw: u8) -> &'static str {
    match raw {
        0 => "unknown",
        1 => "storage",
        2 => "stream",
        5 => "root",
        _ => "other",
    }
}

fn utf16_name(b: &[u8], at: usize, byte_len: usize) -> String {
    let mut units: Vec<u16> = Vec::new();
    let mut i = at;
    while i + 1 < at + byte_len {
        units.push(u16::from_le_bytes([b[i], b[i + 1]]));
        i += 2;
    }
    // 名字长度含结尾的 NUL，那一格不要进名字
    String::from_utf16_lossy(&units)
}

/// 打开一个复合文档（只解析目录与两张 FAT，流字节按需再读）
pub fn open(b: &[u8]) -> Result<Cfb, String> {
    if b.len() < 512 {
        return Err(format!(
            "文件只有 {} 字节，放不下 512 字节的 CFB 头",
            b.len()
        ));
    }
    if b[..8] != SIGNATURE {
        return Err("开头八个字节不是 CFB 签名".to_string());
    }
    let minor = le16(24)(b).ok_or("读不到 minor version")?;
    let major = le16(26)(b).ok_or("读不到 major version")?;
    let byte_order = le16(28)(b).ok_or("读不到字节序标记")?;
    if byte_order != 0xFFFE {
        return Err(format!(
            "字节序标记是 {byte_order:#x}，CFB 只允许小端（0xFFFE）"
        ));
    }
    let shift = le16(30)(b).ok_or("读不到扇区移位")?;
    let mini_shift = le16(32)(b).ok_or("读不到迷你扇区移位")?;
    let sector = 1usize
        .checked_shl(u32::try_from(shift).map_err(|_| "扇区移位太大".to_string())?)
        .ok_or("扇区大小算不出来")?;
    let mini_sector = 1usize
        .checked_shl(u32::try_from(mini_shift).map_err(|_| "迷你扇区移位太大".to_string())?)
        .ok_or("迷你扇区大小算不出来")?;
    if major != 3 && major != 4 {
        return Err(format!("主版本号 {major} 不认识（只有 3 与 4）"));
    }
    if sector != 512 && sector != 4096 {
        return Err(format!("扇区大小 {sector} 不对（只有 512 与 4096）"));
    }
    let mut notes: Vec<String> = Vec::new();
    let dir_sector_count = le32(40)(b).unwrap_or(0);
    if major == 3 && dir_sector_count != 0 {
        notes.push("v3 规定目录扇区数字段必须为 0，这个文件写了非 0".to_string());
    }
    let fat_count = le32(44)(b).unwrap_or(0) as usize;
    let dir_start = le32(48)(b).unwrap_or(0) as u32;
    let mini_cutoff = le32(56)(b).unwrap_or(0);
    let mini_fat_start = le32(60)(b).unwrap_or(0) as u32;
    let difat_start = le32(68)(b).unwrap_or(0) as u32;
    let difat_count = le32(72)(b).unwrap_or(0) as usize;

    // DIFAT：头里 109 条，不够再由链上的扇区继续给（每个扇区最后一格是「下一个」）
    let mut fat_sectors: Vec<u32> = Vec::new();
    for i in 0..109usize {
        let Some(one) = le32(76 + i * 4)(b) else {
            break;
        };
        let one = one as u32;
        if one != NO_SECT {
            fat_sectors.push(one);
        }
    }
    let mut next = difat_start;
    let mut guard = 0usize;
    while next != END_OF_CHAIN && next != NO_SECT && guard < 4096 {
        guard += 1;
        let Some(base) = sector_offset(next, sector) else {
            notes.push("DIFAT 链指向了放不下扇区的位置".to_string());
            break;
        };
        if b.get(base..base + sector).is_none() {
            notes.push("DIFAT 链越界，后面的 FAT 扇区读不到".to_string());
            break;
        }
        let room = sector / 4 - 1;
        for i in 0..room {
            let Some(one) = le32(base + i * 4)(b) else {
                break;
            };
            let one = one as u32;
            if one != NO_SECT {
                fat_sectors.push(one);
            }
        }
        next = le32(base + room * 4)(b).unwrap_or(END_OF_CHAIN as u64) as u32;
    }
    if fat_sectors.len() != fat_count {
        notes.push(format!(
            "头部说 FAT 扇区有 {fat_count} 个，DIFAT 链实际给出 {} 个",
            fat_sectors.len()
        ));
    }

    let mut fat: Vec<u32> = Vec::new();
    for one in sorted_sectors(&fat_sectors) {
        let Some(base) = sector_offset(one, sector) else {
            notes.push(format!("FAT 扇区 {one} 的偏移算不出来"));
            continue;
        };
        if b.get(base..base + sector).is_none() {
            notes.push(format!("FAT 扇区 {one} 越界"));
            continue;
        }
        for i in 0..sector / 4 {
            match le32(base + i * 4)(b) {
                Some(value) => fat.push(value as u32),
                None => break,
            }
        }
    }

    // 目录：一串扇区，每 128 字节一项
    let dir = read_chain(b, dir_start, &fat, sector, &mut notes);
    let mut entries: Vec<Entry> = Vec::new();
    for i in 0..dir.len() / 128 {
        let one = &dir[i * 128..(i + 1) * 128];
        let name_bytes = le16(64)(one).unwrap_or(0) as usize;
        let chars = name_bytes.saturating_sub(2) / 2;
        let kind = type_name(one[66]);
        entries.push(Entry {
            name: utf16_name(one, 0, chars * 2),
            kind,
            start: le32(116)(one).unwrap_or(0) as u32,
            size: le64(120)(one).unwrap_or(0),
            clsid: hex(&one[80..96]),
            created: le64(100)(one).unwrap_or(0),
            modified: le64(108)(one).unwrap_or(0),
            left: le32(68)(one).unwrap_or(0) as u32,
            right: le32(72)(one).unwrap_or(0) as u32,
            child: le32(76)(one).unwrap_or(0) as u32,
        });
    }
    let root = entries.iter().find(|one| one.kind == "root").cloned();
    let mut mini_notes: Vec<String> = Vec::new();
    let mini_stream = match root.as_ref() {
        Some(one) => read_chain(b, one.start, &fat, sector, &mut mini_notes),
        None => {
            notes.push("没有 root 存储项，小流的迷你扇区无处可取".to_string());
            Vec::new()
        }
    };
    let mini_stream = match root.as_ref() {
        Some(one) => {
            let want = usize::try_from(one.size).unwrap_or(usize::MAX);
            if mini_stream.len() < want && want != usize::MAX {
                notes.push(format!(
                    "root 流自报 {} 字节，只能读到 {} 字节（后面的扇区不在文件里）",
                    want,
                    mini_stream.len()
                ));
            }
            mini_stream[..want.min(mini_stream.len())].to_vec()
        }
        None => Vec::new(),
    };
    notes.extend(mini_notes);

    let mut mini_fat: Vec<u32> = Vec::new();
    let mut cursor: Vec<String> = Vec::new();
    let sectors = read_chain(b, mini_fat_start, &fat, sector, &mut cursor);
    notes.extend(cursor);
    for chunk in sectors.chunks(4) {
        if chunk.len() == 4 {
            mini_fat.push(u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
        }
    }

    Ok(Cfb {
        minor,
        major,
        sector_size: sector,
        mini_sector_size: mini_sector,
        mini_cutoff,
        fat_sectors: fat_sectors.len(),
        difat_chained: difat_count > 0,
        entries,
        notes,
        fat,
        mini_fat,
        mini_stream,
    })
}

fn sorted_sectors(sectors: &[u32]) -> Vec<u32> {
    let mut out = sectors.to_vec();
    out.sort_unstable();
    out.dedup();
    out
}

fn sector_offset(which: u32, sector: usize) -> Option<usize> {
    usize::try_from(which)
        .ok()?
        .checked_add(1)?
        .checked_mul(sector)
}

/// 沿 FAT 收集扇区内容；环、越界、超上限都记进 `notes` 并就地停下
fn read_chain(
    b: &[u8],
    start: u32,
    fat: &[u32],
    sector: usize,
    notes: &mut Vec<String>,
) -> Vec<u8> {
    let mut out = Vec::new();
    let mut cur = start;
    let mut seen: Vec<u32> = Vec::new();
    while cur != END_OF_CHAIN && cur != NO_SECT {
        if seen.contains(&cur) {
            notes.push(format!("链在 {cur} 处成环，已就地截断"));
            break;
        }
        if seen.len() >= CHAIN_LIMIT {
            notes.push(format!("链超过 {CHAIN_LIMIT} 个扇区，已就地截断"));
            break;
        }
        seen.push(cur);
        let Some(base) = sector_offset(cur, sector) else {
            notes.push(format!("扇区 {cur} 的偏移算不出来"));
            break;
        };
        match b.get(base..base + sector) {
            Some(one) => out.extend_from_slice(one),
            None => {
                notes.push(format!("扇区 {cur} 指向文件之外（截断的包？）"));
                break;
            }
        }
        match fat.get(usize::try_from(cur).unwrap_or(usize::MAX)) {
            Some(next) => cur = *next,
            None => {
                notes.push(format!("FAT 里没有 {cur} 这一格"));
                break;
            }
        }
    }
    out
}

impl Cfb {
    /// 目录项总数（含 root 与 storage：它们也是目录项，报出来才对得上文件的账）
    pub fn directory_entry_count(&self) -> usize {
        self.entries.len()
    }

    pub fn stream_names(&self) -> Vec<String> {
        self.entries
            .iter()
            .filter(|one| one.is_stream())
            .map(|one| one.name.clone())
            .collect()
    }

    /// 名字比对按不区分大小写：CFB 规范说存储名大小写不敏感，
    /// 而 `WordDocument` / `WORDDOCUMENT` 在真实文件里都出现过
    pub fn find(&self, want: &str) -> Option<&Entry> {
        let lower = want.to_lowercase();
        self.entries
            .iter()
            .find(|one| one.is_stream() && one.name.to_lowercase() == lower)
    }

    /// 读一条流的字节（小流走迷你扇区，大流走 FAT —— 界由容器自己声明）
    pub fn read(&self, b: &[u8], want: &str) -> Option<Vec<u8>> {
        let one = self.find(want)?;
        self.read_entry(b, one)
    }

    /// 读**这一条目录项**的字节：`.doc` 的 ObjectPool 里六枚对象各有一条
    /// `Equation Native`，按名字取只会拿到其中一条（第二读者同一条判据）
    pub fn read_entry(&self, b: &[u8], one: &Entry) -> Option<Vec<u8>> {
        let size = usize::try_from(one.size).ok()?;
        if size == 0 {
            return Some(Vec::new());
        }
        if one.size < self.mini_cutoff {
            let mut out: Vec<u8> = Vec::with_capacity(size);
            let mut cur = one.start;
            let mut seen: Vec<u32> = Vec::new();
            while cur != END_OF_CHAIN && cur != NO_SECT {
                if seen.contains(&cur) || seen.len() >= CHAIN_LIMIT {
                    break;
                }
                seen.push(cur);
                let index = usize::try_from(cur).ok()?;
                let from = index.checked_mul(self.mini_sector_size)?;
                let to = from + self.mini_sector_size;
                out.extend_from_slice(self.mini_stream.get(from..to)?);
                cur = *self.mini_fat.get(index)?;
            }
            if out.len() < size {
                return None;
            }
            return Some(out[..size].to_vec());
        }
        let mut notes: Vec<String> = Vec::new();
        let out = read_chain(b, one.start, &self.fat, self.sector_size, &mut notes);
        if out.len() < size {
            return None;
        }
        Some(out[..size].to_vec())
    }

    /// 这条流走哪条链（报出来的时候要说清，不然读者以为所有流一个读法）
    pub fn via(&self, one: &Entry) -> &'static str {
        if one.size < self.mini_cutoff {
            "mini"
        } else {
            "fat"
        }
    }

    pub fn stream_count(&self) -> usize {
        self.entries.iter().filter(|one| one.is_stream()).count()
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|one| format!("{one:02x}"))
        .collect::<Vec<_>>()
        .join("")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/office")
            .join(name)
    }

    fn read(name: &str) -> Vec<u8> {
        std::fs::read(fixture(name)).unwrap_or_else(|why| panic!("读 fixture {name}: {why}"))
    }

    /// LibreOffice 写的 .doc：七条流、五 12 字节的目录项、扇区 512 ——
    /// 这些数字全部由 `office_reader.py` 独立算出来，改一个就要两边一起改
    #[test]
    fn opens_a_real_word_document() {
        let b = read("notes.doc");
        let cfb = open(&b).expect("LO 写的 .doc 该打得开");
        assert_eq!(cfb.major, 3);
        assert_eq!(cfb.minor, 59);
        assert_eq!(cfb.fat_sectors, 1);
        assert!(!cfb.difat_chained, "小文件不该出现 DIFAT 链");
        assert_eq!(cfb.sector_size, 512);
        assert_eq!(cfb.mini_sector_size, 64);
        assert_eq!(cfb.mini_cutoff, 4096);
        assert_eq!(cfb.directory_entry_count(), 8);
        assert_eq!(cfb.stream_count(), 7, "{:?}", cfb.stream_names());
        assert!(cfb.find("WordDocument").is_some());
        assert!(cfb.find("1Table").is_some());
        assert!(cfb.notes.is_empty(), "{:?}", cfb.notes);
    }

    /// 大流走 FAT、小流走迷你扇区：这条界是文件自己划的，读法必须跟着分岔
    #[test]
    fn streams_are_routed_by_their_own_size() {
        let b = read("notes.doc");
        let cfb = open(&b).expect("打开");
        let big = cfb.find("WordDocument").expect("有 WordDocument");
        assert_eq!(cfb.via(big), "fat");
        assert_eq!(big.size, 4157);
        let small = cfb.find("Data").expect("有 Data");
        assert_eq!(cfb.via(small), "mini");
        assert_eq!(small.size, 525);
        for name in [
            "WordDocument",
            "1Table",
            "Data",
            "\u{1}Ole",
            "\u{5}SummaryInformation",
        ] {
            let got = cfb
                .read(&b, name)
                .unwrap_or_else(|| panic!("{name} 读不出字节"));
            let want = cfb.find(name).expect("有这条流").size;
            assert_eq!(got.len() as u64, want, "{name} 的字节数与目录项对不上");
        }
    }

    /// 名字大小写不敏感：`worddocument` 也指到同一条流
    #[test]
    fn names_match_case_insensitively() {
        let b = read("notes.doc");
        let cfb = open(&b).expect("打开");
        let a = cfb.find("WordDocument").expect("原名");
        let c = cfb.find("worddocument").expect("小写名");
        assert_eq!(a.size, c.size);
    }

    #[test]
    fn an_excel_book_and_a_presentation_open_too() {
        let b = read("book.xls");
        let cfb = open(&b).expect("LO 写的 .xls");
        assert_eq!(cfb.stream_count(), 5, "{:?}", cfb.stream_names());
        let wb = cfb.find("Workbook").expect("有 Workbook 流");
        assert_eq!(wb.size, 2974);
        assert_eq!(cfb.via(wb), "mini");

        let p = read("deck.ppt");
        let cfb = open(&p).expect("LO 写的 .ppt");
        let doc = cfb
            .find("PowerPoint Document")
            .expect("有 PowerPoint Document");
        assert_eq!(cfb.via(doc), "fat");
        assert_eq!(doc.size, 55348);
        // 这条 .ppt 的属性集比正文还大（LibreOffice 把预览图塞进了 SummaryInformation）：
        // 44 万字节走 FAT，正好验大流那条路
        let sum = cfb.find("\u{5}SummaryInformation").expect("有属性集");
        assert_eq!(sum.size, 590232);
        assert_eq!(
            cfb.read(&p, "\u{5}SummaryInformation").map(|one| one.len()),
            Some(590232)
        );
    }

    /// 签名不对就不要硬解：报错要指着签名说
    #[test]
    fn refuses_anything_without_the_signature() {
        let why = open(b"PK\x03\x04 not a compound file at all").unwrap_err();
        assert!(why.contains("512") || why.contains("签名"), "{why}");
        let mut fake = vec![0u8; 512];
        fake[..8].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        let why = open(&fake).unwrap_err();
        assert!(why.contains("签名"), "{why}");
    }

    /// 截断的包：FAT 指向文件之外时要少报，但已经能读的部分要照读
    #[test]
    fn a_truncated_container_says_what_it_lost() {
        let b = read("notes.doc");
        let cut = &b[..b.len() - 3000];
        let cfb = open(cut).expect("头还在，目录还要读得出来");
        assert!(!cfb.notes.is_empty(), "越界的扇区必须留下话");
        let note = cfb.notes.join("；");
        assert!(note.contains("文件之外") || note.contains("FAT"), "{note}");
    }

    /// 大端 / 未知版本的头要顶回去，而不是按小端猜一遍
    #[test]
    fn rejects_unknown_versions_and_endians() {
        let mut b = read("notes.doc");
        b[26..28].copy_from_slice(&9u16.to_le_bytes());
        assert!(open(&b).unwrap_err().contains("主版本"), "版本号 9 不能认");
        let mut b = read("notes.doc");
        b[28..30].copy_from_slice(&0xFEFFu16.to_le_bytes());
        assert!(
            open(&b).unwrap_err().contains("字节序"),
            "大端标记不能按小端读"
        );
    }
}
