//! MS-PPT（PowerPoint 97）：`PowerPoint Document` 流是一棵记录树，本模块把里面的
//! 文本原子读出来。
//!
//! 这一族的坑不在偏移表上，而在**怎么知道自己在读一条记录**：
//! 1. 记录头 8 字节 —— `+0` 是 recVer（容器 `0xF`、原子 `0x0`），`+2` 是 recType，
//!    `+4` 是正文长度。判据不是背下来的：这份 fixture 里三个已知原子
//!    （`0x0FA0` TextCharsAtom / `0x0F9F` TextFooterAtom / `0x0FAA` TextHeaderAtom）
//!    都落在 `+2` 上，且按这个布局整条流**严丝合缝地铺满**（1427 条记录，零处错位）。
//! 2. **容器与原子不能靠 recVer 的位来定**（生产者并不老实）：唯一的判据是
//!    「正文自己能不能再走成一条完整的记录流」。能就下去，不能就只当它是正文。
//!    这条自证让整棵树在深度 6~7 处拿到幻灯片文字，而不是把未知类型当成黑洞。
//! 3. `TextCharsAtom` 的规范写法是「16 位字符、低字节 Windows-1252」，而真实生产者
//!    对非 ASCII 写的是 **UTF-16**。用字节自己判：高字节里出现过非零就按 UTF-16 读，
//!    否则两种读法结果相同 —— 于是一份中英混排的文档不会被读成天书。
//!
//! 与 `scripts/acceptance/lyco_legacy.py` 的 `ppt_text()` 是同一套规范的两份实现，
//! CI 里对同一份 LibreOffice 写的 `deck.ppt` 逐条比文本。

use serde_json::{json, Value};

use crate::cfb::Cfb;
use crate::read::{le16, le32};
use crate::word::decode_cp1252;

pub const TEXT_CHARS: u64 = 0x0FA0;
pub const TEXT_BYTES: u64 = 0x0FA8;
pub const C_STRING: u64 = 0x0FBA;
const SLIDE_CONTAINER: u64 = 0x03F8;
/// 一页一个的这种容器装着该页的文字原子（归属关系是拿 pptx 那副面孔对出来的，
/// 不是我给这个数值起的名字 —— 手上没有 [MS-PPT] 的规范文本，所以这里只报数值）
const SLIDE_RECORD: u64 = 0x03EE;
const MAX_RECORDS: usize = 200_000;
// 真件里幻灯片文字在第 6~7 层；再深就分不清「子容器」和「碰巧能铺满的 blob」了
const MAX_DEPTH: usize = 8;

#[derive(Debug, Clone)]
pub struct TextAtom {
    /// `text-chars` / `text-bytes` / `c-string`
    pub kind: &'static str,
    /// 在**所在那一层正文**里的偏移（同一层从 0 计，所以跨层会重复）
    pub offset: usize,
    pub depth: usize,
    pub text: String,
}

#[derive(Debug)]
pub struct Deck {
    pub records: usize,
    pub atoms: Vec<TextAtom>,
    pub containers: usize,
    pub slide_containers: usize,
    /// 按页归好的一组（recType 0x03EE 的容器，一个一页）
    pub slides: Vec<Slide>,
    pub notes: Vec<String>,
}

impl Deck {
    /// 原子按段落记不划算：一个原子里可能有好几段（`\r` 分隔），这里摊平成行
    pub fn lines(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for one in &self.atoms {
            for line in one.text.split(['\r', '\n', '\u{b}']) {
                if !line.trim().is_empty() {
                    out.push(line.to_string());
                }
            }
        }
        out
    }

    pub fn to_json(&self) -> Value {
        json!({
            "records": self.records,
            "text_atoms": self.atoms.len(),
            "containers": self.containers,
            "slide_containers": self.slide_containers,
            "slides": self.slides.iter().map(|one| json!({
                "record_offset": one.offset, "depth": one.depth, "name": one.name,
                "text_atoms": one.atoms, "lines": one.lines,
            })).collect::<Vec<Value>>(),
            "atoms": self.atoms.iter().map(|one| json!({
                "kind": one.kind, "depth": one.depth, "offset": one.offset, "text": one.text,
            })).collect::<Vec<Value>>(),
            "notes": self.notes,
        })
    }
}

fn head(buf: &[u8], at: usize) -> Option<(u64, usize)> {
    let kind = le16(at + 2)(buf)?;
    let len = usize::try_from(le32(at + 4)(buf)?).ok()?;
    Some((kind, len))
}

/// 这一段能不能再走成一条完整的记录流（走完正好停在末尾才算）
fn tiles(buf: &[u8]) -> bool {
    if buf.len() < 8 {
        return false;
    }
    let mut at = 0usize;
    let mut seen = 0usize;
    while at + 8 <= buf.len() {
        let Some((_, len)) = head(buf, at) else {
            return false;
        };
        let Some(next) = at.checked_add(8).and_then(|base| base.checked_add(len)) else {
            return false;
        };
        if next > buf.len() {
            return false;
        }
        at = next;
        seen += 1;
        if seen > MAX_RECORDS {
            return false;
        }
    }
    at == buf.len()
}

fn kind_name(kind: u64) -> Option<&'static str> {
    match kind {
        TEXT_CHARS => Some("text-chars"),
        TEXT_BYTES => Some("text-bytes"),
        C_STRING => Some("c-string"),
        _ => None,
    }
}

/// 文本原子的解码：见模块注释第 3 条
fn decode_text(kind: u64, body: &[u8]) -> String {
    if kind == TEXT_BYTES {
        return decode_cp1252(body);
    }
    let wide = body.chunks(2).any(|pair| pair.len() == 2 && pair[1] != 0);
    if wide {
        let mut units: Vec<u16> = Vec::new();
        for pair in body.chunks(2) {
            if pair.len() == 2 {
                units.push(u16::from_le_bytes([pair[0], pair[1]]));
            }
        }
        String::from_utf16_lossy(&units)
    } else {
        decode_cp1252(&body.iter().step_by(2).copied().collect::<Vec<u8>>())
    }
}

fn walk_tree(buf: &[u8], depth: usize, current: Option<usize>, deck: &mut Walk) {
    let mut at = 0usize;
    while at + 8 <= buf.len() && deck.records < MAX_RECORDS {
        let Some((kind, len)) = head(buf, at) else {
            break;
        };
        let Some(end) = at.checked_add(8).and_then(|base| base.checked_add(len)) else {
            deck.notes.push(format!(
                "深度 {depth} 处第 {} 条记录的自报长度超出这一层",
                deck.records
            ));
            return;
        };
        if end > buf.len() {
            deck.notes
                .push(format!("深度 {depth} 处一条记录伸出这一层末尾（在 {at}）"));
            return;
        }
        deck.records += 1;
        let body = &buf[at + 8..end];
        let tiles_here = depth < MAX_DEPTH && !body.is_empty() && tiles(body);
        // 按页归位：recType 0x03EE 的容器**恰好一页一个**，页的文字原子都在它下面。
        // 这条不是照规范背的（名字我也没有），是拿同一份文档的另一副面孔对出来的：
        // deck.ppt 里这样的容器有 2 个，各自的文字与 deck.pptx 的
        // ppt/slides/slide1.xml / slide2.xml 逐张一致。
        let mut next = current;
        if kind == SLIDE_RECORD && tiles_here {
            deck.slides.push(Slide {
                offset: at,
                depth,
                name: String::new(),
                atoms: 0,
                lines: Vec::new(),
            });
            next = Some(deck.slides.len() - 1);
        }
        if let Some(name) = kind_name(kind) {
            let text = decode_text(kind, body);
            deck.atoms.push(TextAtom {
                kind: name,
                offset: at,
                depth,
                text: text.clone(),
            });
            if let Some(index) = next {
                let slide = &mut deck.slides[index];
                slide.atoms += 1;
                if kind == C_STRING {
                    // 每页那一条 CString 是 LibreOffice 写的**版式名**（`___PPT10`），
                    // 不是页面上的字：单列出来，别混进正文行
                    if slide.name.is_empty() {
                        slide.name = text;
                    }
                } else {
                    for line in text.split(['\r', '\n', '\u{b}']) {
                        if !line.trim().is_empty() {
                            slide.lines.push(line.to_string());
                        }
                    }
                }
            }
        }
        if kind == SLIDE_CONTAINER {
            deck.slide_containers += 1;
        }
        if tiles_here {
            deck.containers += 1;
            walk_tree(body, depth + 1, next, deck);
        }
        at = end;
    }
}

/// 一页：`offset` 是那一层里的偏移（与原子同一口径），`lines` 是页面上的字
#[derive(Debug, Clone)]
pub struct Slide {
    pub offset: usize,
    pub depth: usize,
    /// LibreOffice 写在这条记录里的那个内部版式名（`___PPT10`），不是正文
    pub name: String,
    pub atoms: usize,
    pub lines: Vec<String>,
}

struct Walk {
    records: usize,
    atoms: Vec<TextAtom>,
    containers: usize,
    slide_containers: usize,
    slides: Vec<Slide>,
    notes: Vec<String>,
}

pub fn read(cfb: &Cfb, bytes: &[u8]) -> Result<Deck, String> {
    let stream = cfb
        .read(bytes, "PowerPoint Document")
        .ok_or("容器里没有 PowerPoint Document 流")?;
    let mut deck = Walk {
        records: 0,
        atoms: Vec::new(),
        containers: 0,
        slide_containers: 0,
        slides: Vec::new(),
        notes: Vec::new(),
    };
    walk_tree(&stream, 0, None, &mut deck);
    let mut notes = deck.notes;
    if deck.records >= MAX_RECORDS {
        notes.push(format!("记录数到了上限 {MAX_RECORDS}，后面的没再走"));
    }
    Ok(Deck {
        records: deck.records,
        slide_containers: deck.slide_containers,
        slides: deck.slides,
        atoms: deck.atoms,
        containers: deck.containers,
        notes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open(name: &str) -> (Vec<u8>, Cfb) {
        let bytes = std::fs::read(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/office")
                .join(name),
        )
        .expect("读 fixture");
        let cfb = crate::cfb::open(&bytes).expect("打开复合文档");
        (bytes, cfb)
    }

    /// 按页归位：recType 0x03EE 的容器恰好一页一个，页里的文字与这份文档的 pptx
    /// 那一份（`ppt/slides/slide1.xml` / `slide2.xml`）逐张一致 —— 归属关系有独立
    /// 出处，而 recType 的**规范名字**我没有，所以这里只报数值与内容，不编名字
    #[test]
    fn a_slide_record_holds_exactly_one_slide_s_text() {
        let (bytes, cfb) = open("deck.ppt");
        let deck = read(&cfb, &bytes).expect("读得出记录树");
        assert_eq!(deck.slides.len(), 2, "0x03EE 的容器应当一页一个");
        assert_eq!(
            deck.slides[0].lines,
            vec!["预算评审", "新增两台 64 核应用服务器", "第二条要点"]
        );
        assert_eq!(
            deck.slides[1].lines,
            vec!["第二页：数字", "科目", "金额", "服务器", "124000"]
        );
        // LibreOffice 每页写一条 CString 的版式名：它不是页面上的字，单列出来
        assert_eq!(deck.slides[0].name, "___PPT10");
        assert_eq!(deck.slides[1].name, "___PPT10");
        assert!(deck
            .slides
            .iter()
            .all(|one| { !one.lines.iter().any(|line| line.starts_with("___PPT")) }));
        // 母版与版式里的占位文字不归任何一页：全流原子比按页归好的多
        let grouped: usize = deck.slides.iter().map(|one| one.atoms).sum();
        assert!(
            grouped < deck.atoms.len(),
            "{grouped} vs {}",
            deck.atoms.len()
        );
    }

    /// LibreOffice 由 deck.pptx 转出的真件：幻灯片文字一条不少、顺序对，
    /// 整棵记录树走满（期望值来自 `lyco_legacy.py` 的 `ppt_text`）
    #[test]
    fn reads_the_text_a_powerpoint_record_tree_hides() {
        let (bytes, cfb) = open("deck.ppt");
        let deck = read(&cfb, &bytes).expect("读得出记录树");
        assert_eq!(deck.records, 1427, "{}", deck.records);
        assert_eq!(deck.atoms.len(), 67, "{}", deck.atoms.len());
        assert_eq!(deck.containers, 381, "{}", deck.containers);
        assert_eq!(deck.slide_containers, 11, "SlideContainer 的个数不是页数");
        assert!(deck.notes.is_empty(), "{:?}", deck.notes);
        let lines = deck.lines();
        for want in [
            "预算评审",
            "新增两台 64 核应用服务器",
            "第二条要点",
            "第二页：数字",
            "科目",
            "金额",
            "服务器",
            "评审时先讲口径再讲数字",
        ] {
            assert!(lines.iter().any(|one| one == want), "缺 {want}：{lines:?}");
        }
        // 幻灯片母版的占位文字也在（它确实是文件里的文字，不该被当成正文丢掉，
        // 但要点是：读出来的东西和 pptx 那边逐条对得上）
        assert!(lines
            .iter()
            .any(|one| one == "Click to edit Master title style"));
    }

    /// 高字节全零时两种读法必须给出同一个结果，非零时必须走 UTF-16
    #[test]
    fn the_width_of_text_atoms_is_decided_by_its_own_bytes() {
        let ascii: Vec<u8> = "abc".bytes().flat_map(|one| [one, 0]).collect::<Vec<u8>>();
        assert_eq!(decode_text(TEXT_CHARS, &ascii), "abc");
        let cjk: Vec<u8> = "预算"
            .encode_utf16()
            .flat_map(|one| one.to_le_bytes())
            .collect::<Vec<u8>>();
        assert_eq!(decode_text(TEXT_CHARS, &cjk), "预算");
        assert_eq!(decode_text(TEXT_BYTES, b"8/4"), "8/4");
    }

    /// 不是演示文稿的复合文档要报错
    #[test]
    fn a_non_presentation_says_so() {
        let (bytes, cfb) = open("notes.doc");
        let why = read(&cfb, &bytes).unwrap_err();
        assert!(why.contains("PowerPoint Document"), "{why}");
    }
}
