//! 图自己那一份账：那串字节说的格式、像素与自带密度，对着文档说的那两个尺寸
//!
//! 与 `scripts/acceptance/lyco_pictures.py` 一份账（第二个读者）。问的是同一句话的两种答案：
//! **文档说这张图多大**（页面上占的那块地方）与**图自己说它多大**（头里那 24 个字节）。
//! 两个数可以差得很远（缩放进来的图），也可以一个根本没有（没有自带密度的 GIF），
//! 所以两边都按原样交，差距用千分比交出去，不替谁圆场。
//!
//! 三条出口，一族一条，「文件说这是什么格式」那一句住在三个完全不同的地方：
//!
//! * OOXML：声明在 `[Content_Types].xml`（按扩展名的 `Default`、按部件名的 `Override`），
//!   地址要顺着 `word/_rels/document.xml.rels` 跳一跳才落到 `word/media/image1.png`；
//! * ODF：声明是 `draw:image/@draw:mime-type`（可以整条不写），地址直接是包内路径；
//! * RTF：声明就是控制字本身（`\pngblip`、`\wmetafile`），尺寸写成「目标 twips × 缩放百分比」，
//!   而它**另外**还写了 `\picw` / `\pich` 两个像素数 —— 那两个数与图自己头里的像素数
//!   是两件事，所以 `px_agrees` 只在写过的族上有答案。
//!
//! 单位一律整数的 0.01mm（`paper` 那一条式子：round half up，无浮点），自带的密度也按文件
//! 写的那个单位交（PNG/BMP 是每米像素、JPEG 是 DPI 或 DPCM、TIFF 是分辨率标记 × 单位、
//! EMF 干脆直接给 0.01mm 的框），换算成自然尺寸时才动一次式子。**除法只做一次**：
//! 长宽比差用交叉相乘（`(w×hₚ 与 h×wₚ)` 的差除以大的那一侧）而不是「除两次再比商」，
//! 后者会把两次舍入叠进判据里。
//!
//! 「没有」在这本账里有四种，而且它们是四句话，所以 `density.state` 交四态：`read` 是文件
//! 写了可用单位；`unitless` 是写了那个字段但说的是「只有长宽比」（JFIF 单位 0、TIFF 单位 1、
//! PNG pHYs 单位不是 1）；`absent` 是这个格式有这一格而这份件没写（或写了零）；`none` 是
//! 这个格式压根没有这一格（GIF、CORE 头 BMP、普通 WMF、SVM）。**没有字节可读**是第五种：
//! 那一格交 null（`eq.odt` 之外的 133 份里没有这种行，而 `tbox.docx` 那一条有）。
//!
//! ## 整库实测（134 份三族出口全开：74 份 word + 42 份 .odt + 18 份 .rtf，71 行）
//!
//! 只有 34 份写了图（18 word + 11 .odt + 5 .rtf），其余 100 份交**零行的整本账** ——
//! 「这份文档没有图」与「这一族没这一层」是两件事，所以那一族三族都恒开这一格。
//! 地址那一本：`read` 70 条、`none` 1 条（`tbox.docx` 那个框里既没 `a:blip` 也没 `r:embed`），
//! `unresolved`/`missing` 本库各 0 —— 那两条分支由合成件守着，不是没人写。
//! 声明与字节**从不互相打脸**：`agrees` 64 真 / 0 假 / 7 判不了（那 7 条是 SVM 六条加
//! 没字节那条，两边都没有可比的名字），`ext_agrees` 51 真 / 0 假 / 20 判不了
//! （RTF 那 13 条压根没有扩展名这一说）。名字对得上不等于写法一样：
//! 扩展名交的是 `tif` 4 条、`jpg` 4 条，而检测出来是 `tiff` / `jpeg` —— 认字表里
//! 那两行同名映射就是为它们写的。
//! 密度那一本 read 24 / unitless 4 / absent 29 / none 13；自然尺寸只有 24 条算得出，
//! 也就是说**三分之二的图自己说不了它有多大**，只能靠文档那一侧。
//! 拉伸只有 4 条（`images-dpi` 那一份件的三家出口各一条，长宽比差 583‰/583‰/583‰/582‰），
//! 而 `at_natural` 也是 4 条（偏差恒 2‰），**两批没有一行重合**：`images-dpi-lo.docx` 与
//! `images-dpi.odt` 里各有一对行顶着同一个部件名（LO 把两张图并成一份字节），
//! 「同一份字节」与「同一行」是两问。
//! RTF 独家那 13 条写了 `\picw`/`\pich`：11 条与字节里的像素数一致，2 条判不了
//! （那两张是 WMF，普通 WMF 的头里没有任何尺寸）；`disagrees` 0 条。
//! 生产者那一侧的两条实测事实留在 fixture 上（`images-dpi.*`）：同一张 300 dpi 的
//! 40×24 PNG，python-docx 写 `wp:extent cx="121920" cy="73152"`（EMU），LibreOffice 转出的
//! .odt 写 `svg:width="0.339cm" svg:height="0.203cm"` —— 换到 0.01mm 两边都是 339×203，
//! 而**串长得不一样**，所以 `placed_mm100.written` 交文件自己写的那一串；Pillow 把 300 dpi
//! 存成 11811 每米像素（`density.written` 就是那一串），且它写 JPEG 时 JFIF 单位给 0
//! （只说长宽比），于是那张 JPEG 落在 `unitless` 而不是 `read`。
//! LO 存 .odt 时按**像素内容**去重帧的部件（9 个框指向 6 个部件，`distinct_parts` 就是
//! 为此而交的一格），还会把 `.jpg` 改名成 `.jpeg`；它存 .rtf 时把 gif/bmp/tif 全转成
//! `\pngblip` 或 `\wmetafile`，于是 RTF 那 13 条的检测分布是 png 9 / jpeg 2 / wmf 2。
//! 那两张 WMF 是**普通** WMF（头里只有记录长度），而 `eq.odt` 那六条式子的字节是 SVM
//! （`VCLMTF\x01\x00`，既没有 mime-type 也没有扩展名）—— 这一族里只有合成件能给的两种形状。
//!
//! ## 封顶这件事
//!
//! 只看图的头：`HEAD_CAP` 64KB（够走完 PNG 的块表与 TIFF 的第一个 IFD），RTF 那一族受群头
//! [`PICTURE_SCAN_CAP`] 的 16KB 限制、十六进制解出来最多 `RTF_HEAD_CAP` 8KB ——
//! 两边的封顶不同本身就是一条实测事实，所以 `read_cap` 那一格把它交出来。头都不够长的
//! 件（被截断的照片）交 null 而不是猜一个尺寸。

use crate::paper;
use crate::xmlscan::Node;
use serde_json::{json, Map, Value};

/// 只看图的头：够走完 PNG 的块表（到 IDAT 为止）、JPEG 的段表（到 SOF 为止）、
/// TIFF 的第一个 IFD，而不会把一张照片整个读进内存
pub(crate) const HEAD_CAP: usize = 64 * 1024;

/// RTF 那一族的封顶：群头只扫 16KB，十六进制解出来最多 8KB（与 `HEAD_CAP` 是两件事）
pub(crate) const RTF_HEAD_CAP: usize = 8 * 1024;

/// 长宽比差与「原尺寸」的判据：千分比。生产者在最后一位上就不一致
/// （python-docx 写 1219200 EMU = 3386.67 个 0.01mm，而 LibreOffice 直接写 0.339cm），
/// 所以不用相等判，用 1‰ 的零头判 —— 那个数比生产者之间的抖动大、比任何真实拉伸小
const ASPECT_TOL: i64 = 10;
const NATURAL_TOL: i64 = 10;

/// 「文件说这是什么格式」的三种拼法换到图自己那一个名字。认不出来交 None，不猜一个
const WORD_FORMAT: [(&str, &str); 16] = [
    ("png", "png"),
    ("jpeg", "jpeg"),
    ("jpg", "jpeg"),
    ("jpe", "jpeg"),
    ("gif", "gif"),
    ("bmp", "bmp"),
    ("dib", "bmp"),
    ("rle", "bmp"),
    ("tif", "tiff"),
    ("tiff", "tiff"),
    ("emf", "emf"),
    ("x-emf", "emf"),
    ("wmf", "wmf"),
    ("x-wmf", "wmf"),
    ("svg", "svg"),
    ("svg+xml", "svg"),
];

/// RTF 那三个不带 `blip` 尾巴的控制字：`\wmetafile` 说的是 WMF，`\dibitmap` 说的是 BMP；
/// `\pictbitmap` 只说「这是一张位图，格式你自己看」，所以它没有名字可给
const RTF_WORDS: [(&str, Option<&str>); 4] = [
    ("wmetafile", Some("wmf")),
    ("dibitmap", Some("bmp")),
    ("macpict", Some("pict")),
    ("pictbitmap", None),
];

/// 声明串可能带的那三个前缀（逐个试，与第二读者同一条走法）
const MIME_PREFIX: [&str; 3] = ["image/", "application/", "drawing/"];

/// 两个字节（`be` 说大端），越界交 None 而不是 0 —— 0 是一个尺寸，None 是「没读到」
fn u16_at(raw: &[u8], at: usize, be: bool) -> Option<i64> {
    let one = raw.get(at..at + 2)?;
    let got = if be {
        u16::from_be_bytes([one[0], one[1]])
    } else {
        u16::from_le_bytes([one[0], one[1]])
    };
    Some(i64::from(got))
}

/// 四个字节，同上（一律按**无符号**读：BMP 的高度那一位可以是很长的数，
/// 而这一本要的是「文件写了什么」，不是「有符号解释成什么」）
fn u32_at(raw: &[u8], at: usize, be: bool) -> Option<i64> {
    let one = raw.get(at..at + 4)?;
    let got = if be {
        u32::from_be_bytes([one[0], one[1], one[2], one[3]])
    } else {
        u32::from_le_bytes([one[0], one[1], one[2], one[3]])
    };
    Some(i64::from(got))
}

/// 有符号的四字节（EMF 的两个框可以有负数）
fn s32_at(raw: &[u8], at: usize) -> Option<i64> {
    let got = u32_at(raw, at, false)?;
    Some(if got >= 1 << 31 {
        got - (1i64 << 32)
    } else {
        got
    })
}

/// 整数除法、half up 进位（生产者的最后一位抖动不靠浮点复现）；分母为 0/None 交 None
fn rhu(num: Option<i64>, den: Option<i64>) -> Option<i64> {
    let num = num?;
    let den = den.filter(|one| *one != 0)?;
    let a = num.checked_mul(2)?.checked_add(den)?;
    let b = den.checked_mul(2)?;
    Some(a.div_euclid(b))
}

/// 千分比：一次除法，half up（不先换成小数再比，那会让两个读者舍到不同的一位）。
/// 与第二读者同一条「零与没有是一回事」的口径：任何一侧是 0 就交 None
fn permille(a: Option<i64>, b: Option<i64>) -> Option<i64> {
    let a = a.filter(|one| *one != 0)?;
    let b = b.filter(|one| *one != 0)?;
    rhu(a.checked_mul(1000), Some(b))
}

fn mul(v: Option<i64>, k: i64) -> Option<i64> {
    v?.checked_mul(k)
}

/// 「右-左+1」那两个框都是这么读的（EMF 的界框含端点，所以宽要加一）
fn span(from: Option<i64>, to: Option<i64>) -> Option<i64> {
    Some(to?.checked_sub(from?)?.checked_add(1)?)
}

/// 文件写了的那一串数字（`written` 那一格交原样，所以 None 也要有个样子）
fn shown(v: Option<i64>) -> String {
    match v {
        Some(one) => one.to_string(),
        None => "None".to_string(),
    }
}

fn hex_of(raw: &[u8]) -> String {
    raw.iter().take(8).map(|one| format!("{one:02x}")).collect()
}

/// 那条声明（`image/png` / `pngblip` / `JPG`）说的格式名，认不出交 None
fn word_format(raw: Option<&str>) -> Option<&'static str> {
    let mut text = raw?.trim().to_lowercase();
    if text.is_empty() {
        return None;
    }
    for prefix in MIME_PREFIX {
        if let Some(rest) = text.strip_prefix(prefix) {
            text = rest.to_string();
        }
    }
    if let Some(one) = RTF_WORDS.iter().find(|one| one.0 == text) {
        return one.1;
    }
    if text.ends_with("blip") && text.len() > 4 {
        text.truncate(text.len() - 4);
    }
    WORD_FORMAT
        .iter()
        .find(|one| one.0 == text)
        .map(|one| one.1)
}

/// 那串字节自己是什么格式。认不出交 "unknown"，空的一交 None
///
/// EMF 与 WMF 的头**同一个数**（都以 `01 00` 开头：一个是记录类型、一个是文件类型），
/// 所以前两字节分不开它们：EMF 的判据在偏移 40 那枚 ` EMF` 签名，
/// WMF 的偏移 2 是头长（9 个字，`09 00`）。
fn detect(raw: &[u8]) -> Option<&'static str> {
    if raw.is_empty() {
        return None;
    }
    if raw.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some("png");
    }
    if raw.starts_with(&[0xFF, 0xD8]) {
        return Some("jpeg");
    }
    if raw.starts_with(b"GIF87a") || raw.starts_with(b"GIF89a") {
        return Some("gif");
    }
    if raw.starts_with(b"BM")
        && matches!(u32_at(raw, 14, false), Some(one)
            if matches!(one, 12 | 40 | 56 | 108 | 124))
    {
        return Some("bmp");
    }
    if raw.starts_with(b"II*\x00") || raw.starts_with(b"MM\x00*") {
        return Some("tiff");
    }
    if u32_at(raw, 0, false) == Some(1) && u32_at(raw, 40, false) == Some(0x464D_4520) {
        return Some("emf");
    }
    if u32_at(raw, 0, false) == Some(0x9AC6_CDD7) {
        return Some("wmf"); // 可放置那一族：头里带单位数与英寸数
    }
    if matches!(u16_at(raw, 0, false), Some(one) if one == 1 || one == 2)
        && u16_at(raw, 2, false) == Some(9)
    {
        return Some("wmf");
    }
    if raw.starts_with(b"VCLMTF\x01\x00") {
        return Some("svm");
    }
    Some("unknown")
}

/// 密度那一条：`state` 说那三种「没有」的差别（见模块说明）
#[derive(Clone)]
struct Dens {
    state: Option<&'static str>,
    unit: Option<&'static str>,
    x: Option<i64>,
    y: Option<i64>,
    written: Option<String>,
}

impl Dens {
    fn of(state: Option<&'static str>) -> Dens {
        Dens {
            state,
            unit: None,
            x: None,
            y: None,
            written: None,
        }
    }

    fn set(
        state: Option<&'static str>,
        unit: Option<&'static str>,
        x: Option<i64>,
        y: Option<i64>,
        written: String,
    ) -> Dens {
        Dens {
            state,
            unit,
            x,
            y,
            written: Some(written),
        }
    }

    fn to_json(&self) -> Value {
        json!({
            "state": self.state,
            "unit": self.unit,
            "x": self.x,
            "y": self.y,
            "written": self.written,
        })
    }
}

/// 一个头的解法：像素数、从哪儿读的、密度
struct Head {
    pw: Option<i64>,
    ph: Option<i64>,
    how: Option<String>,
    dens: Dens,
}

/// 一串字节的整份答案（含算出来的自然尺寸）
struct Got {
    format: Option<&'static str>,
    pw: Option<i64>,
    ph: Option<i64>,
    how: Option<String>,
    dens: Dens,
    nat_w: Option<i64>,
    nat_h: Option<i64>,
}

/// 自然尺寸（0.01mm）：像素数按文件自己说的密度除一次，只除这一次
fn natural(pw: Option<i64>, ph: Option<i64>, dens: &Dens) -> (Option<i64>, Option<i64>) {
    if pw.is_none() || ph.is_none() || dens.state != Some("read") {
        return (None, None);
    }
    match dens.unit {
        Some("ppm") => (rhu(mul(pw, 100_000), dens.x), rhu(mul(ph, 100_000), dens.y)),
        Some("dpi") => (rhu(mul(pw, 2540), dens.x), rhu(mul(ph, 2540), dens.y)),
        Some("dpcm") => (rhu(mul(pw, 1000), dens.x), rhu(mul(ph, 1000), dens.y)),
        // EMF：那个框本来就是 0.01mm，没有除法
        Some("mm100") => (dens.x, dens.y),
        _ => (None, None),
    }
}

/// 一串字节 → 它自己说的那份账：格式、像素、密度、自然尺寸
fn read(raw: &[u8]) -> Got {
    let kind = detect(raw);
    let head = match kind {
        Some("png") => png(raw),
        Some("jpeg") => jpeg(raw),
        Some("gif") => gif(raw),
        Some("bmp") => bmp(raw),
        Some("tiff") => tiff(raw),
        Some("emf") => emf(raw),
        Some("wmf") => wmf(raw),
        // svm / unknown / 压根没有字节：这一族没有可解的头
        _ => Head {
            pw: None,
            ph: None,
            how: None,
            dens: Dens::of(if kind.is_none() { None } else { Some("none") }),
        },
    };
    let (nat_w, nat_h) = natural(head.pw, head.ph, &head.dens);
    Got {
        format: kind,
        pw: head.pw,
        ph: head.ph,
        how: head.how,
        dens: head.dens,
        nat_w,
        nat_h,
    }
}

/// 块表走到 IDAT 为止：IHDR 给像素，pHYs 给密度（单位字节只认 1=每米）
fn png(raw: &[u8]) -> Head {
    let mut pw = None;
    let mut ph = None;
    let mut how = None;
    let mut dens = Dens::of(Some("absent"));
    let mut at = 8usize;
    while at + 8 <= raw.len() {
        let Some(size) = u32_at(raw, at, true) else {
            break;
        };
        let kind = &raw[at + 4..at + 8];
        let from = at + 8;
        let body = &raw[from..raw.len().min(from + size as usize)];
        if kind == &b"IHDR"[..] && body.len() >= 9 {
            pw = u32_at(body, 0, true);
            ph = u32_at(body, 4, true);
            how = Some("IHDR".to_string());
        } else if kind == &b"pHYs"[..] && body.len() >= 9 {
            let unit = body[8];
            let x = u32_at(body, 0, true);
            let y = u32_at(body, 4, true);
            let written = format!("{},{},{}", shown(x), shown(y), unit);
            dens = if unit == 1 {
                Dens::set(Some("read"), Some("ppm"), x, y, written)
            } else {
                // 单位不是 1 就是「这两个数只是比值」，它说不了「一米几个像素」
                Dens::set(Some("unitless"), Some("unknown"), x, y, written)
            };
        } else if kind == &b"IDAT"[..] {
            break;
        }
        if size == 0 {
            break;
        }
        at += 12 + size as usize;
    }
    Head { pw, ph, how, dens }
}

/// 段表走到第一个 SOF 为止：APP0/JFIF 给密度（0 只说长宽比），SOFn 给像素
fn jpeg(raw: &[u8]) -> Head {
    let mut pw = None;
    let mut ph = None;
    let mut how = None;
    let mut dens = Dens::of(Some("absent"));
    let mut at = 2usize;
    while at + 4 <= raw.len() {
        if raw[at] != 0xFF {
            break;
        }
        let marker = raw[at + 1];
        if marker == 0x01 || marker == 0xD8 || (0xD0..=0xD7).contains(&marker) {
            at += 2;
            continue;
        }
        let Some(size) = u16_at(raw, at + 2, true) else {
            break;
        };
        if size < 2 {
            break;
        }
        let from = at + 4;
        let seg = &raw[from..raw.len().min(from + (size as usize).saturating_sub(2))];
        if marker == 0xE0 && seg.starts_with(b"JFIF\x00") && seg.len() >= 12 {
            let unit = seg[7];
            let x = u16_at(seg, 8, true);
            let y = u16_at(seg, 10, true);
            let name = match unit {
                0 => Some("aspect"),
                1 => Some("dpi"),
                2 => Some("dpcm"),
                _ => None,
            };
            // 单位 0 那两个数只是长宽比（Pillow 不带 dpi 参数时也写 JFIF，就是这种），
            // 它说不了「一英寸里几个像素」，所以与「压根没写」同等待遇：不算密度
            let written = format!("JFIF,{unit},{},{}", shown(x), shown(y));
            dens = if matches!(name, Some("dpi") | Some("dpcm")) {
                Dens::set(Some("read"), name, x, y, written)
            } else {
                Dens::set(Some("unitless"), name, x, y, written)
            };
        } else if marker == 0xEE {
            // APP14 只说色彩空间，密度不在这里
        } else if (0xC0..=0xCF).contains(&marker) && ![0xC4, 0xC8, 0xCC].contains(&marker) {
            if seg.len() >= 5 {
                // SOFn：精度 1 字节、高 2、宽 2 —— 高的在前
                pw = u16_at(seg, 3, true);
                ph = u16_at(seg, 1, true);
                how = Some(format!("SOF{:X}", marker - 0xC0));
            }
            break;
        }
        at += 2 + size as usize;
    }
    Head { pw, ph, how, dens }
}

fn gif(raw: &[u8]) -> Head {
    Head {
        pw: u16_at(raw, 6, false),
        ph: u16_at(raw, 8, false),
        how: Some("LogicalScreenDescriptor".to_string()),
        // GIF 的头里压根没有密度那一格（不是没写，是没有这一格）
        dens: Dens::of(Some("none")),
    }
}

/// `BITMAPCOREHEADER`（12）没有密度那一格，`BITMAPINFOHEADER`（40）有；
/// 高度可以是负数（那是「从上往下存」，不是另一个尺寸），所以两边都按无符号读原样交
fn bmp(raw: &[u8]) -> Head {
    let header = u32_at(raw, 14, false);
    let how = Some(format!("BITMAPHEADER{}", shown(header)));
    let dens = if header == Some(12) {
        Dens {
            state: Some("none"),
            unit: None,
            x: None,
            y: None,
            written: Some("core".to_string()),
        }
    } else {
        let x = u32_at(raw, 38, false);
        let y = u32_at(raw, 42, false);
        let written = format!("{},{}", shown(x), shown(y));
        if x.unwrap_or(0) != 0 && y.unwrap_or(0) != 0 {
            Dens::set(Some("read"), Some("ppm"), x, y, written)
        } else {
            // 那两个 0 只留在 `written` 里当证据；既然判成「没说」，x/y 就别装作读出了数
            Dens::set(Some("absent"), None, None, None, written)
        }
    };
    Head {
        pw: u32_at(raw, 18, false),
        ph: u32_at(raw, 22, false),
        how,
        dens,
    }
}

/// IFD 里那几条：256/257 是像素，282/283 是分辨率（RATIONAL，指到别处），
/// 296 是单位（1 没有、2 每英寸、3 每厘米）。SHORT 的值就塞在那 4 个字节的前两个里
fn tiff(raw: &[u8]) -> Head {
    let be = raw.starts_with(b"MM");
    let mut pw = None;
    let mut ph = None;
    let mut dens = Dens::of(Some("absent"));
    let mut unit = None;
    let mut xres: Option<(i64, i64)> = None;
    let mut yres: Option<(i64, i64)> = None;
    let Some(ifd) = u32_at(raw, 4, be).and_then(|one| usize::try_from(one).ok()) else {
        return Head {
            pw,
            ph,
            how: Some("IFD0".to_string()),
            dens,
        };
    };
    if ifd + 2 > raw.len() {
        return Head {
            pw,
            ph,
            how: Some("IFD0".to_string()),
            dens,
        };
    }
    let count = u16_at(raw, ifd, be).unwrap_or(0);
    let mut i = 0i64;
    while i < count {
        let Some(one) = ifd
            .checked_add(2)
            .and_then(|v| v.checked_add(i as usize * 12))
        else {
            break;
        };
        if one + 12 > raw.len() {
            break;
        }
        let tag = u16_at(raw, one, be);
        let typ = u16_at(raw, one + 2, be);
        let val = match typ {
            Some(3) => u16_at(raw, one + 8, be),
            Some(4) => u32_at(raw, one + 8, be),
            _ => None,
        };
        if tag == Some(256) {
            pw = val;
        } else if tag == Some(257) {
            ph = val;
        } else if tag == Some(282) || tag == Some(283) {
            // 类型 5 才是 RATIONAL（Pillow 写的就是 5，实测 `282 5 1 176`）。
            // 那 8 个字节（分子+分母各一个 LONG）塞不进一条项，所以那格是**偏移**，
            // 与 SHORT/LONG 把值就地写下是两件事 —— 按就地那个数读会得到 176
            if matches!(typ, Some(5) | Some(6) | Some(10)) {
                if let Some(where_at) =
                    u32_at(raw, one + 8, be).and_then(|v| usize::try_from(v).ok())
                {
                    if where_at + 8 <= raw.len() {
                        let pair = (u32_at(raw, where_at, be), u32_at(raw, where_at + 4, be));
                        if let (Some(a), Some(b)) = pair {
                            if tag == Some(282) {
                                xres = Some((a, b));
                            } else {
                                yres = Some((a, b));
                            }
                        }
                    }
                }
            }
        } else if tag == Some(296) {
            unit = val;
        }
        i += 1;
    }
    if let (Some(xr), Some(yr)) = (xres, yres) {
        if xr.1 != 0 && yr.1 != 0 {
            // 那条单位标签没写时 TIFF 自己的默认是 2（每英寸），不是「没说」
            let got = match unit.unwrap_or(2) {
                2 => Some("dpi"),
                3 => Some("dpcm"),
                _ => None,
            };
            let written = format!(
                "{}/{},{}/{},{}",
                shown(Some(xr.0)),
                shown(Some(xr.1)),
                shown(Some(yr.0)),
                shown(Some(yr.1)),
                shown(unit)
            );
            dens = if let Some(name) = got {
                Dens::set(
                    Some("read"),
                    Some(name),
                    rhu(Some(xr.0), Some(xr.1)),
                    rhu(Some(yr.0), Some(yr.1)),
                    written,
                )
            } else {
                // 分辨率两个数在场而单位那一格说的是「没有单位」或一个认不出的数：
                // 这不能算 absent（那两格明明写了），只能算 unitless
                Dens::set(
                    Some("unitless"),
                    Some("unknown"),
                    Some(xr.0),
                    Some(yr.0),
                    written,
                )
            };
        }
    }
    Head {
        pw,
        ph,
        how: Some("IFD0".to_string()),
        dens,
    }
}

/// 记录头：`rclBounds` 是像素（含端点，所以宽是 右-左+1），`rclFrame` 已经是 0.01mm，
/// 所以 EMF 是唯一一个不用密度就能报自然尺寸的位图格式
fn emf(raw: &[u8]) -> Head {
    let (left, top, right, bottom) = (
        s32_at(raw, 8),
        s32_at(raw, 12),
        s32_at(raw, 16),
        s32_at(raw, 20),
    );
    let (fl, ft, fr, fb) = (
        s32_at(raw, 24),
        s32_at(raw, 28),
        s32_at(raw, 32),
        s32_at(raw, 36),
    );
    let written = format!("{},{},{},{}", shown(fl), shown(ft), shown(fr), shown(fb));
    Head {
        pw: span(left, right),
        ph: span(top, bottom),
        how: Some("ENHMETAHEADER".to_string()),
        dens: Dens::set(
            Some("read"),
            Some("mm100"),
            span(fl, fr),
            span(ft, fb),
            written,
        ),
    }
}

/// 可放置 WMF（`0x9AC6CDD7` 开头）在头里给单位数与英寸数，普通 WMF 只给记录长度 ——
/// 那一种的头里没有任何尺寸可说，所以像素与自然都交 None，而不是交 0
fn wmf(raw: &[u8]) -> Head {
    if u32_at(raw, 0, false) == Some(0x9AC6_CDD7) {
        let (left, top, right, bottom) = (
            u16_at(raw, 8, false),
            u16_at(raw, 10, false),
            u16_at(raw, 12, false),
            u16_at(raw, 14, false),
        );
        let inch = u16_at(raw, 16, false);
        let state = if inch.unwrap_or(0) == 0 {
            "absent"
        } else {
            "read"
        };
        return Head {
            pw: span(left, right),
            ph: span(top, bottom),
            how: Some("PlaceableHeader".to_string()),
            dens: Dens::set(Some(state), Some("dpi"), inch, inch, shown(inch)),
        };
    }
    Head {
        pw: None,
        ph: None,
        how: Some("Header".to_string()),
        dens: Dens::of(Some("none")),
    }
}

/// 一条账的来料：文档那一侧说了什么（声明、地址、页面上那块地方），加上图的那串字节
#[derive(Default)]
struct Item {
    at: String,
    addr: String,
    word: Option<String>,
    ext: Option<String>,
    bytes: Vec<u8>,
    placed_w: Option<i64>,
    placed_h: Option<i64>,
    placed_from: Option<String>,
    placed_written: Option<Vec<Option<String>>>,
    decl_px_w: Option<i64>,
    decl_px_h: Option<i64>,
    note: Option<String>,
}

impl Item {
    fn new(at: &str, addr: &str) -> Item {
        Item {
            at: at.to_string(),
            addr: addr.to_string(),
            ..Default::default()
        }
    }
}

/// 两边都有名字才判得了：一边没有就是「这句话问不出答案」，不是「答案是不一致」
fn tri(said: Option<&str>, sig: Option<&str>) -> Option<bool> {
    match (said, sig) {
        (Some(a), Some(b)) if b != "unknown" => Some(a == b),
        _ => None,
    }
}

/// 一条账：声明、检测、两边的尺寸、两边的千分比。所有除法只在整数上做一次
fn row(item: &Item) -> Value {
    let said = word_format(item.word.as_deref());
    let ext_name = word_format(item.ext.as_deref());
    let got = read(&item.bytes);
    let sig = got.format;
    let agrees = tri(said, sig);
    let ext_agrees = tri(ext_name, sig);
    // 长宽比差：交叉相乘，不除两次再比商（那会把两次舍入叠进判据），千分比
    let aspect = match (item.placed_w, item.placed_h, got.pw, got.ph) {
        (Some(pw), Some(ph), Some(x), Some(y)) if pw != 0 && ph != 0 && x != 0 && y != 0 => {
            let left = pw.checked_mul(y.abs());
            let right = ph.checked_mul(x.abs());
            match (left, right) {
                (Some(a), Some(b)) => {
                    let peak = a.max(b);
                    if peak == 0 {
                        None
                    } else {
                        rhu((a - b).abs().checked_mul(1000), Some(peak))
                    }
                }
                _ => None,
            }
        }
        _ => None,
    };
    let scale_w = permille(item.placed_w, got.nat_w);
    let scale_h = permille(item.placed_h, got.nat_h);
    let declared = match (item.decl_px_w, item.decl_px_h) {
        (None, None) => None,
        _ => Some((item.decl_px_w, item.decl_px_h)),
    };
    // 只有 RTF 写了文档自己声称的像素数；字节里读不出像素的那两条判不了
    let px_agrees = match (&declared, got.pw) {
        (None, _) => None,
        (Some(_), None) => None,
        (Some(_), Some(0)) => None,
        (Some(want), Some(one)) => Some(want.0 == got.pw && want.1 == got.ph),
    };
    let written = item.placed_written.as_ref().map(|list| {
        list.iter()
            .map(|one| match one {
                Some(text) => Value::from(text.clone()),
                None => Value::Null,
            })
            .collect::<Vec<Value>>()
    });
    json!({
        "where": item.at.clone(),
        "addr": item.addr.clone(),
        "word": item.word.clone(),
        "word_name": said,
        "ext": item.ext.clone(),
        "ext_name": ext_name,
        "sig": sig,
        "agrees": agrees,
        "ext_agrees": ext_agrees,
        "pixels": {"w": got.pw, "h": got.ph},
        "declared_pixels": declared.map(|(w, h)| json!({"w": w, "h": h})),
        "px_agrees": px_agrees,
        "density": got.dens.to_json(),
        "nat_mm100": {"w": got.nat_w, "h": got.nat_h},
        "placed_mm100": {
            "w": item.placed_w,
            "h": item.placed_h,
            "from": item.placed_from.clone(),
            "written": written,
        },
        "scale_permille": {"w": scale_w, "h": scale_h},
        "aspect_permille": aspect,
        "stretched": aspect.map(|one| one > ASPECT_TOL),
        "at_natural": match (scale_w, scale_h) {
            (Some(a), Some(b)) => {
                Some((a - 1000).abs() <= NATURAL_TOL && (b - 1000).abs() <= NATURAL_TOL)
            }
            _ => None,
        },
        "how": got.how.clone(),
        "head_hex": hex_of(&item.bytes),
        "note": item.note.clone(),
    })
}

/// 那一格是不是写着的一个数（不是 null）
fn has(rows: &[Value], key: &str, sub: &str) -> usize {
    rows.iter()
        .filter(|one| {
            one.get(key)
                .and_then(|box_| box_.get(sub))
                .map(|v| !v.is_null())
                == Some(true)
        })
        .count()
}

/// 三态那一列的三本计数（JSON 的键不能是 null，所以摊成 yes/no/undecided）
fn three(rows: &[Value], key: &str) -> Value {
    let yes = rows
        .iter()
        .filter(|one| one.get(key) == Some(&json!(true)))
        .count();
    let no = rows
        .iter()
        .filter(|one| one.get(key) == Some(&json!(false)))
        .count();
    json!({"yes": yes, "no": no, "undecided": rows.len() - yes - no})
}

fn eq(rows: &[Value], key: &str, want: &str) -> usize {
    rows.iter()
        .filter(|one| one.get(key) == Some(&json!(want)))
        .count()
}

fn nested_eq(rows: &[Value], key: &str, sub: &str, want: &str) -> usize {
    rows.iter()
        .filter(|one| {
            one.get(key)
                .and_then(|box_| box_.get(sub))
                .map(|v| v == &json!(want))
                == Some(true)
        })
        .count()
}

/// 那些数的数法：每一条只问一个「文件到底说了没有」的问题，两问不合并
fn tally(rows: &[Value], family: &str, limit: usize, cap: usize) -> Value {
    let total = rows.len();
    let mut detected: BTree = BTree::new();
    for one in rows {
        let key = match one.get("sig") {
            Some(Value::String(text)) => text.clone(),
            _ => "(没读到)".to_string(),
        };
        detected.add(&key);
    }
    let placed_known = rows
        .iter()
        .filter(|one| {
            let placed = one.get("placed_mm100");
            placed.and_then(|box_| box_.get("w")).map(|v| !v.is_null()) == Some(true)
                || placed.and_then(|box_| box_.get("h")).map(|v| !v.is_null()) == Some(true)
        })
        .count();
    let mut parts: Vec<&str> = Vec::new();
    for one in rows {
        if let Some(Value::String(text)) = one.get("where") {
            if !parts.iter().any(|had| *had == text) {
                parts.push(text.as_str());
            }
        }
    }
    json!({
        "family": family,
        "available": true,
        "total": total,
        "listed": total.min(limit),
        "cut": total > limit,
        "read_cap": cap,
        "distinct_parts": parts.len(),
        "addr": {
            "read": eq(rows, "addr", "read"),
            "unresolved": eq(rows, "addr", "unresolved"),
            "missing": eq(rows, "addr", "missing"),
            "none": eq(rows, "addr", "none"),
        },
        "detected": detected.to_json(),
        "agrees": three(rows, "agrees"),
        "ext_agrees": three(rows, "ext_agrees"),
        "pixels": {
            "known": has(rows, "pixels", "w"),
            "unknown": total - has(rows, "pixels", "w"),
        },
        "density": {
            "read": nested_eq(rows, "density", "state", "read"),
            "unitless": nested_eq(rows, "density", "state", "unitless"),
            "absent": nested_eq(rows, "density", "state", "absent"),
            "none": nested_eq(rows, "density", "state", "none"),
        },
        "natural": {
            "known": has(rows, "nat_mm100", "w"),
            "unknown": total - has(rows, "nat_mm100", "w"),
        },
        "placed": {"known": placed_known, "unknown": total - placed_known},
        "stretched": three(rows, "stretched"),
        "at_natural": three(rows, "at_natural"),
        "declared_pixels": {
            "written": rows.iter().filter(|one| !one.get("declared_pixels").map(|v| v.is_null()).unwrap_or(true)).count(),
            "agrees": three(rows, "px_agrees")["yes"].as_u64().unwrap_or(0) as usize,
            "disagrees": three(rows, "px_agrees")["no"].as_u64().unwrap_or(0) as usize,
            "undecided": three(rows, "px_agrees")["undecided"].as_u64().unwrap_or(0) as usize,
        },
        "rows": rows.iter().take(limit).cloned().collect::<Vec<Value>>(),
    })
}

/// 一个小写序的计数表（`detected` 那一格：JSON 里按名字排好，读的人不用自己排）
struct BTree {
    keys: Vec<String>,
    vals: Vec<usize>,
}

impl BTree {
    fn new() -> BTree {
        BTree {
            keys: Vec::new(),
            vals: Vec::new(),
        }
    }

    fn add(&mut self, key: &str) {
        match self.keys.iter().position(|one| one == key) {
            Some(at) => self.vals[at] += 1,
            None => {
                let at = self
                    .keys
                    .iter()
                    .position(|one| one.as_str() > key)
                    .unwrap_or(self.keys.len());
                self.keys.insert(at, key.to_string());
                self.vals.insert(at, 1);
            }
        }
    }

    fn to_json(&self) -> Value {
        let mut out = Map::new();
        for (key, val) in self.keys.iter().zip(self.vals.iter()) {
            out.insert(key.clone(), json!(*val));
        }
        Value::Object(out)
    }
}

/// 文件扩展名（最后一个 `.` 之后那一段，小写）。名字里没有一个点就交 None
fn extension_of(path: &str) -> Option<String> {
    match path.rfind('.') {
        Some(at) => Some(path[at + 1..].to_lowercase()),
        None => None,
    }
}

/// `[Content_Types].xml` 里这一条部件的声明串：`Override` 按部件名、`Default` 按扩展名，
/// 两条都是文件自己写的，缺的那条不猜。
///
/// 这里不用 `ContentTypes::of`：那一只比的是**原样大小写**的 `PartName`，而第二读者两边
/// 都化成小写再比（部件路径本来就是另一套大小写规则）。生产者一旦写出
/// `/word/media/Image1.PNG`，两只就会一个说 png、一个说没声明 —— 所以这一族自己走那一条式子
fn declared_kind(types: &crate::opack::ContentTypes, part: &str) -> Option<String> {
    let lower = part.to_lowercase();
    if let Some((_, one)) = types
        .overrides
        .iter()
        .find(|(name, _)| name.to_lowercase() == lower)
    {
        return Some(one.clone());
    }
    let last = part.rsplit('/').next().unwrap_or(part);
    let ext = match last.rfind('.') {
        Some(at) => last[at + 1..].to_lowercase(),
        None => return None,
    };
    types.defaults.get(&ext).cloned()
}

/// 关系目标 → 包内部件名（第二读者那一条式子：`word/` 开头的原样，其余挂在 `word/` 下）
fn docx_part(target: &str) -> String {
    if target.starts_with("word/") {
        return target.to_string();
    }
    format!("word/{}", target.trim_start_matches('/'))
}

/// `draw:image/@xlink:href` → 包内路径：去掉 `#` 后面那段，再削掉一层 `./` 或 `/`
fn odf_part(href: &str) -> String {
    let mut part = match href.split_once('#') {
        Some((front, _)) => front.to_string(),
        None => href.to_string(),
    };
    if let Some(rest) = part.strip_prefix("./") {
        part = rest.to_string();
    }
    if let Some(rest) = part.strip_prefix('/') {
        part = rest.to_string();
    }
    part
}

/// OOXML 那一份：尺寸取 `wp:extent`（EMU），地址取 `a:blip/@r:embed` 顺关系表那一跳；
/// 声明取 `[Content_Types].xml`。跳不到部件的交 `addr` 而不是把这一行丢掉
pub(crate) fn docx(
    bytes: &[u8],
    document: &Node,
    rels: &[crate::opack::Rel],
    limit: usize,
) -> Value {
    let names = crate::zipread::member_names(bytes);
    if !names.iter().any(|one| one == "word/document.xml") {
        return json!({"family": "docx", "available": false});
    }
    let types = crate::opack::ContentTypes::read(bytes);
    let mut rows: Vec<Value> = Vec::new();
    for drawing in document.descendants("drawing") {
        let frame = match drawing.descendants("inline").into_iter().next() {
            Some(one) => Some(one),
            None => drawing.descendants("anchor").into_iter().next(),
        };
        let Some(frame) = frame else { continue };
        // 尺寸在 `wp:extent` 上：先看直接孩子（写在那儿），再看整棵子树（生产者的写法不止一种）
        let extent = match frame.child("extent") {
            Some(one) => Some(one),
            None => frame.descendants("extent").into_iter().next(),
        };
        let (cx, cy) = match extent {
            Some(one) => (
                one.attr_local("cx").map(|v| v.to_string()),
                one.attr_local("cy").map(|v| v.to_string()),
            ),
            None => (None, None),
        };
        let mut item = Item::new("", "none");
        item.placed_from = Some("wp:extent".to_string());
        match extent {
            Some(_) => {
                item.placed_w = cx.as_deref().and_then(|one| paper::emu(one));
                item.placed_h = cy.as_deref().and_then(|one| paper::emu(one));
                item.placed_written = Some(vec![cx.clone(), cy.clone()]);
            }
            None => item.placed_written = None,
        }
        let blip = frame.descendants("blip").into_iter().next();
        let embed = blip
            .and_then(|one| one.attr_local("embed"))
            .map(|v| v.to_string());
        let Some(id) = embed else {
            item.at = "(no-blip)".to_string();
            rows.push(row(&item));
            continue;
        };
        let found = rels
            .iter()
            .find(|one| one.source == "word/document.xml" && one.id == id);
        let Some(rel) = found else {
            item.at = id;
            item.addr = "unresolved".to_string();
            rows.push(row(&item));
            continue;
        };
        if rel.external {
            item.at = rel.target.clone();
            item.note = Some("external".to_string());
            rows.push(row(&item));
            continue;
        }
        let part = docx_part(&rel.target);
        if !names.iter().any(|one| *one == part) {
            item.at = part;
            item.addr = "missing".to_string();
            rows.push(row(&item));
            continue;
        }
        item.bytes = crate::zipread::member_head(bytes, &part, HEAD_CAP).unwrap_or_default();
        item.word = declared_kind(&types, &part);
        item.ext = extension_of(&part);
        item.at = part;
        item.addr = "read".to_string();
        rows.push(row(&item));
    }
    tally(&rows, "docx", limit, HEAD_CAP)
}

/// ODF 那一份：声明是 `draw:mime-type`（可以整条不写），地址是包内路径，
/// 尺寸是 `draw:frame` 上那两条自带单位的串（`svg:width="0.339cm"`）
pub(crate) fn odf(bytes: &[u8], root: &Node, limit: usize) -> Value {
    let names = crate::zipread::member_names(bytes);
    if !names.iter().any(|one| one == "content.xml") {
        return json!({"family": "odf", "available": false});
    }
    let mut rows: Vec<Value> = Vec::new();
    for frame in root.descendants("frame") {
        // 只数「这个框里直接坐着一个 image」：文本框、OLE 框也是 frame
        let Some(image) = frame.child("image") else {
            continue;
        };
        let w_raw = frame.attr_local("width").map(|v| v.to_string());
        let h_raw = frame.attr_local("height").map(|v| v.to_string());
        let mut item = Item::new("", "none");
        item.placed_w = w_raw.as_deref().and_then(|one| paper::length(one));
        item.placed_h = h_raw.as_deref().and_then(|one| paper::length(one));
        item.placed_from = Some("draw:frame/@svg:width".to_string());
        item.placed_written = Some(vec![w_raw.clone(), h_raw.clone()]);
        item.word = image.attr_local("mime-type").map(|v| v.to_string());
        let href = image
            .attr_local("href")
            .filter(|one| !one.is_empty())
            .map(|v| v.to_string());
        let Some(href) = href else {
            item.at = "(no-href)".to_string();
            rows.push(row(&item));
            continue;
        };
        let part = odf_part(&href);
        item.ext = extension_of(&part);
        item.at = part.clone();
        if !names.iter().any(|one| *one == part) {
            item.addr = "missing".to_string();
            rows.push(row(&item));
            continue;
        }
        item.addr = "read".to_string();
        item.bytes = crate::zipread::member_head(bytes, &part, HEAD_CAP).unwrap_or_default();
        rows.push(row(&item));
    }
    tally(&rows, "odf", limit, HEAD_CAP)
}

/// 每一个 `\pict` 群的群头起点：`{` 紧跟 `\pict` 且词在边界上
///
/// 与 `rtf.rs` 的整群走查是两个不同的走法（那一家在 tokenizer 里认这个词），
/// 数出来的条数可以拿来对账：对不上就是有人把 `\pict` 写在了群中间或正文里
fn pict_heads(bytes: &[u8]) -> Vec<usize> {
    let mut out: Vec<usize> = Vec::new();
    let Some(mut at) = find_byte(bytes, b'{', 0) else {
        return out;
    };
    loop {
        let is_pict =
            bytes.get(at + 1) == Some(&b'\\') && bytes.get(at + 2..at + 6) == Some(&b"pict"[..]);
        if is_pict {
            let boundary = match bytes.get(at + 6) {
                Some(one) => !(one.is_ascii_alphanumeric() || *one == b'-'),
                None => true,
            };
            if boundary {
                out.push(at);
                at += 6;
                continue;
            }
        }
        match find_byte(bytes, b'{', at + 1) {
            Some(next) => at = next,
            None => break,
        }
    }
    out
}

fn find_byte(hay: &[u8], needle: u8, from: usize) -> Option<usize> {
    Some(hay.get(from..)?.iter().position(|one| *one == needle)? + from)
}

/// twips 的目标 × 百分比，一次除法换到 0.01mm（缺省的 100 是 RTF 的规格，不是猜的）
fn goal_mm100(goal: Option<&str>, scale: Option<&str>) -> Option<i64> {
    let base = goal.and_then(|one| one.parse::<i64>().ok())?;
    let factor = match scale.and_then(|one| one.parse::<i64>().ok()) {
        Some(one) => one,
        None => 100,
    };
    let num = base.checked_mul(2540)?.checked_mul(factor)?;
    rhu(Some(num), Some(1440 * 100))
}

fn digits(raw: Option<String>) -> Option<i64> {
    raw.and_then(|one| one.parse::<i64>().ok())
}

/// RTF 那一份：这一族把尺寸拆成「目标 × 缩放百分比」两半，而且**另外**写了像素数
///
/// `\picwgoal` 是 twips 的目标宽、`\picscalex` 是百分数（RTF 的缺省是 100，Word 在
/// 100 时常常整条不写），所以页面上那块地方要一次除法两半合成，不先舍入到中间单位；
/// `\picw` / `\pich` 又写了**像素数** —— 那是文档自己声称的像素，与图自己头里那
/// 两个字节是两件事，所以 `px_agrees` 只在这一族有答案（别族没写过像素，交 None）。
/// 声明是控制字本身（`\pngblip`、`\wmetafile`），地址这一族压根没有（数据就在群里）。
pub(crate) fn rtf(bytes: &[u8], limit: usize) -> Value {
    let mut rows: Vec<Value> = Vec::new();
    for (ordinal, start) in pict_heads(bytes).into_iter().enumerate() {
        // `group_stop` 要从 `{` 的**下一格**起走：它数的是「关掉当前这一群」的那个 `}`，
        // 从 `{` 起走就等于多关了一层，一路走到文档末尾 —— 那么一条没有 blip 的群
        // 会把**下一张**图的控制字与字节读成自己的（两条账说同一句话）
        let stop = crate::rtf::group_stop(bytes, start + 1);
        let end = stop.min(start + crate::rtf::PICTURE_SCAN_CAP).max(start);
        let head = &bytes[start..end];
        let (kind, data_at) = match crate::rtf::blip_word(head) {
            Some(one) => (Some(one.0), Some(one.1)),
            None => (None, None),
        };
        let goal_w = crate::rtf::word_in_group(head, "picwgoal");
        let goal_h = crate::rtf::word_in_group(head, "pichgoal");
        let scale_x = crate::rtf::word_in_group(head, "picscalex");
        let scale_y = crate::rtf::word_in_group(head, "picscaley");
        let raw = match data_at {
            Some(at) => crate::rtf::hex_head(head, at, RTF_HEAD_CAP),
            None => Vec::new(),
        };
        let mut item = Item::new(
            &format!("pict#{ordinal}"),
            if raw.is_empty() { "none" } else { "read" },
        );
        item.bytes = raw;
        item.word = kind;
        item.placed_w = goal_mm100(goal_w.as_deref(), scale_x.as_deref());
        item.placed_h = goal_mm100(goal_h.as_deref(), scale_y.as_deref());
        item.placed_from = Some(r"\picwgoal×\picscalex".to_string());
        item.placed_written = Some(vec![goal_w, goal_h, scale_x, scale_y]);
        item.decl_px_w = digits(crate::rtf::word_in_group(head, "picw"));
        item.decl_px_h = digits(crate::rtf::word_in_group(head, "pich"));
        rows.push(row(&item));
    }
    tally(&rows, "rtf", limit, RTF_HEAD_CAP)
}

#[cfg(test)]
mod tests;
