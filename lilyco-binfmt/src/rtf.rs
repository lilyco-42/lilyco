//! RTF 正文提取：目标群感知、字符集感知的最小解释器。
//!
//! 与 `scripts/acceptance/lyco_rtf.py` 是同一份规范的两套独立实现，CI 上互相核对。
//! 五件真正决定成败的事（前四件都是这份实现在真实文件上错过的位置）：
//!
//! 1. 控制字按**词边界**结束：`\pard` 不许当成 `\par` 加一个字面 `d`，否则样式名与
//!    字体名会整段漏进正文；
//! 2. 目标群整群跳过：`fonttbl` / `colortbl` / `stylesheet` / `\*\...` 里的东西不是正文；
//!    子群继承父群的跳过状态；
//! 3. `\uN` 之后要按 `\ucN` 丢掉**等价回退字符**，而 `\'hh` 只算一个字符 ——
//!    少这一步，中文段落会剩下一串 `'3f`；
//! 4. 连续的 `\'hh` 攒成字节串，再按文件自己声明的 `\ansicpgNNNN` 解：
//!    UTF-8(65001) 直接解，1252/0 逐字节当 latin-1，**其余字符集本版本不内置**
//!    （936/GBK 之类的多字节编码不在 std 里）—— 那就逐字节给出来并说明用的哪档，
//!    绝不假装解对了；
//! 5. 一切「说不清」都留在计数与 `notes` 里，不悄悄丢字节。

use serde_json::{json, Value};

/// 见到这些控制字就把所在群整群跳过：它们是表 / 元数据 / 域指令原文，不是正文
const SKIP_DESTINATIONS: &[&str] = &[
    "fonttbl",
    "colortbl",
    "stylesheet",
    "info",
    "generator",
    "listtable",
    "listoverridetable",
    "themedata",
    "colorschememapping",
    "datastore",
    "panorama",
    "latentstyles",
    "rsidtbl",
    "xmlnstbl",
    "filetbl",
    "upr",
    "bkmkstart",
    "bkmkend",
    "nonshppict",
    "pntxtb",
    "pntext",
    "mmathPr",
    "docPr",
    "atrsnc",
    "atrspr",
    "operator",
];

/// 这几群仍然整群跳过（里面的一个字都不是页面上的字），但本域**认得**它们，
/// 所以跳过之前先前瞻读一遍里面的定义 —— 与 `\*` 那条同一个道理：
/// 「不认识才跳」不等于「认识了就不许跳」，只是认识了就别连里面的名字一起丢。
/// `listtable` 与 `listoverridetable` 也走这一条：列表定义不是页面上的字，
/// 但段上那个 `\ls` 点的就是这里的某一份，不读等于把号的来源丢掉
const DEF_DESTINATIONS: &[&str] = &["fonttbl", "stylesheet", "listtable", "listoverridetable"];

/// 书签那两群：名字不是页面上的字，所以照样整群跳过，只是跳之前把名字读出来 ——
/// 与 `\*` 那条同一个道理：认得一个群不等于要把它当正文
const BK_WORDS: &[&str] = &["bkmkstart", "bkmkend"];

/// 断点类：输出一个换行
const BREAK_WORDS: &[&str] = &["par", "line", "sect", "page", "pbb"];
/// 字符格式那一群控制字：这一族把「这几个字长什么样」写在**群头**上（`\b`、`\cf23`、
/// `\fs18`…），而不是像 OOXML 那样给每一串字立一个元素。`\b0` 与 `\i0` 是这一族说
/// 「明确不」的拼法 —— 否定写在数字参数上，不在别的地方
const RUN_WORDS: &[&str] = &[
    "b",
    "i",
    "ul",
    "uld",
    "aul",
    "iul",
    "outl",
    "strike",
    "sub",
    "super",
    "cf",
    "cb",
    "highlight",
    "fs",
    "afs",
    "f",
    "af",
    "kerning",
    "expnd",
    "caps",
    "scaps",
    "ulc",
    "cs",
    "ab",
    "ai",
];
/// 下划线这一族有四个口袋（普通 / 双 / 日文 / 意大利体），文件点哪个就用哪个
const RUN_UNDERLINE: [&str; 4] = ["ul", "uld", "aul", "iul"];
/// 这三个只说「这是哪一种文种的字」（`\hich` 高文种、`\dbch` 双字节文种、`\loch` 低文种），
/// 不是格式，所以单独交一个布尔，不混进 `format`
const RUN_DIRECT: &[&str] = &["loch", "hich", "dbch", "rtlch", "ltrch"];
/// 逐串账本最多存几串（`--limit` 在那之外还要再截一次，与图那本同一口径）
const RUN_ROW_CAP: usize = 512;
/// 页眉与页脚的目标群：字是真的，但它们不是正文。
/// 注意 `\headery` / `\footery` 是「页眉高度」这种**格式**控制字，
/// 控制字读到字母为止，所以整名匹配不会把它们误当成目标
const PAGE_DESTINATIONS: &[&str] = &[
    "header", "headerl", "headerr", "headert", "headerf", "footer", "footerl", "footerr",
    "footert", "footerf",
];

/// 目标属于页眉还是页脚
fn page_kind(word: &str) -> &'static str {
    if word.starts_with("header") {
        "header"
    } else {
        "footer"
    }
}

/// 从 `at` 起找到关闭**当前这一群**的那个 `}`：交回它的位置与群里的字节。
/// `\{` 与 `\}` 是字面花括号，不参与配对；找不到就走到尾（坏文件不咬人）
fn group_end(bytes: &[u8], at: usize) -> (usize, Vec<u8>) {
    let mut depth = 0i32;
    let mut i = at;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => {
                if depth == 0 {
                    return (i, bytes[at..i].to_vec());
                }
                depth -= 1;
            }
            b'\\' => {
                if matches!(bytes.get(i + 1), Some(&b'{') | Some(&b'}')) {
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    (bytes.len(), bytes[at.min(bytes.len())..].to_vec())
}

/// 链接：`{\field{\*\fldinst HYPERLINK "地址" }{\fldrslt {显示文字}}}`。
/// 这两个前缀是文件里字面写的样子（一个反斜杠 + 一个星号 + 群名）
const FLDINST_HEAD: &[u8] = b"{\\*\\fldinst";
const FLDRSLT_HEAD: &[u8] = b"{\\fldrslt";

/// 从一群 `\field …` 里读出一条链接：域指令里的地址 + `\fldrslt` 的显示文字。
/// 读不出 HYPERLINK 就交回 None（页码、日期那些域不是链接）
fn field_link(group: &[u8]) -> Option<Value> {
    let at = windows_position(group, 0, FLDINST_HEAD)?;
    let (_stop, instruction) = group_end(group, at + FLDINST_HEAD.len());
    let target = hyperlink_target(&instruction)?;
    let mut text = String::new();
    if let Some(nxt) = windows_position(group, at, FLDRSLT_HEAD) {
        let (_stop2, result) = group_end(group, nxt + FLDRSLT_HEAD.len());
        text = extract(&result).text;
    }
    Some(json!({ "target": target, "text": text }))
}

/// 一群 `\field …` 里的**域指令原文**（`{\*\fldinst { TOC \\o "1-2" \\h}}` 那一段）。
/// 这里要解一遍，是因为文件里的开关必须写成双反斜杠（单反斜杠会开出一个控制字，
/// `\o` 就不再是指令里的字母 o）；解完那一串 `TOC \o "1-2" \h` 与 docx 的
/// `w:instrText` **逐字同一个形状**，所以「收几级」那把读取器两家共用一把。
/// 解不出字（空群）交回 None，不替文件补一条指令
fn field_instruction(group: &[u8]) -> Option<String> {
    let at = windows_position(group, 0, FLDINST_HEAD)?;
    let (_stop, instruction) = group_end(group, at + FLDINST_HEAD.len());
    let had = extract(&instruction).text;
    let had = had.trim();
    if had.is_empty() {
        return None;
    }
    Some(had.to_string())
}

/// `HYPERLINK "地址"` 里那段引号包住的地址。指令原文的大小写各家不同，这里按
/// ASCII 大小写无关找字面量；地址本身照文件写的字节交回
fn hyperlink_target(inst: &[u8]) -> Option<String> {
    const WORD: &[u8] = b"HYPERLINK";
    let low: Vec<u8> = inst.iter().map(|one| one.to_ascii_lowercase()).collect();
    let at = windows_position(&low, 0, &WORD.to_ascii_lowercase())?;
    let mut k = at + WORD.len();
    while matches!(
        inst.get(k),
        Some(b' ') | Some(b'\t') | Some(b'\r') | Some(b'\n')
    ) {
        k += 1;
    }
    if inst.get(k) != Some(&b'"') {
        return None;
    }
    k += 1;
    let start = k;
    while k < inst.len() && inst[k] != b'"' {
        k += 1;
    }
    Some(String::from_utf8_lossy(&inst[start..k]).into_owned())
}

/// 一群之内的**顶层**子群：交回每个子群的内容（不含外面那对花括号）
fn child_groups(text: &[u8]) -> Vec<Vec<u8>> {
    let mut out: Vec<Vec<u8>> = Vec::new();
    let mut i = 0usize;
    while i < text.len() {
        if text[i] != b'{' {
            i += 1;
            continue;
        }
        let (stop, inner) = group_end(text, i + 1);
        out.push(inner);
        i = stop + 1;
    }
    out
}

/// 一群的开头是不是控制字 `want`（`\list{` 与 `\listlevel{` 不是一回事，
/// 所以认完名字还要看下一个字节还是不是字母数字）
fn starts_word(group: &[u8], want: &str) -> bool {
    let rest = match group.strip_prefix(b"\\") {
        Some(one) => one,
        None => return false,
    };
    let name = rest
        .iter()
        .take_while(|one| one.is_ascii_alphabetic())
        .copied()
        .collect::<Vec<u8>>();
    if name != want.as_bytes() {
        return false;
    }
    match rest.get(name.len()) {
        None => true,
        Some(one) => !one.is_ascii_alphanumeric(),
    }
}

/// 开头那个控制字占了几个字节：名字、紧跟的数字参数（`-` 也算，`\fi-360` 是负数），
/// 以及跟在后面的那**一个**空格（RTF 里它是分隔符，不是字）。
/// `\'hh` 那种转义不是控制字，只占那四个字节
fn word_len(group: &[u8]) -> usize {
    if group.first() != Some(&b'\\') {
        if group.first() == Some(&b'\'') {
            return 4.min(group.len());
        }
        return 0;
    }
    let mut k = 1usize;
    while k < group.len() && group[k].is_ascii_alphabetic() {
        k += 1;
    }
    while k < group.len() && (group[k].is_ascii_digit() || group[k] == b'-') {
        k += 1;
    }
    if k < group.len() && group[k] == b' ' {
        k += 1;
    }
    k
}

/// 去掉开头那个控制字之后的部分：`{\leveltext \'\02\'01.;}` 里真正要说的是
/// `\'\02\'01.;` 这一段，而它按文件写的字节交，不替它解成「第 1 级后面一个点」
fn payload_of(group: &[u8]) -> &[u8] {
    &group[word_len(group).min(group.len())..]
}

/// 一群去掉开头控制字之后的原样串（`\'hh` 与 `\uN` 都按字节交，不猜字面）。
/// 用在**整群交出来**的那些子群上（`{\leveltext …}`）；标签那一句不走这里，
/// 因为读到 `\listtext` 时已经站在群里，群里的字一个控制字也没多剥
fn written_of(group: &[u8]) -> String {
    String::from_utf8_lossy(payload_of(group)).into_owned()
}

/// 一群里第一个叫 `want` 的控制字：交回紧跟它的那串数字（没有数字时交空串），
/// 整群没有才交 None
fn word_in_group(group: &[u8], want: &str) -> Option<String> {
    let mut i = 0usize;
    while i < group.len() {
        if group[i] != b'\\' {
            i += 1;
            continue;
        }
        let mut k = i + 1;
        let mut name: Vec<u8> = Vec::new();
        while k < group.len() && group[k].is_ascii_alphabetic() {
            name.push(group[k]);
            k += 1;
        }
        let from = k;
        while k < group.len() && (group[k].is_ascii_digit() || group[k] == b'-') {
            k += 1;
        }
        if name == want.as_bytes() {
            return Some(String::from_utf8_lossy(&group[from..k]).into_owned());
        }
        i = if k > i + 1 { k } else { i + 1 };
    }
    None
}

/// 一张 `{\pict …}` 群的**群头**最多看这么多字节：两个生产者都把形状写在数据之前
/// （`{\*\picprop …}` 那格、`\picscalex` 一串、最后才是 `\pngblip` 与那几个兆的
/// 十六进制），而数据本身可以长到几万个字符 —— 抄一遍不值，扫到底也不值
const PICTURE_SCAN_CAP: usize = 16 * 1024;

/// 逐张账本最多存几张图（`pictures` 那个条数不受它影响，一直数到底）
const PICTURE_ROW_CAP: usize = 512;

/// 跳过 `\*` 那个「不认识就整群跳过」的标记：`{\*\picprop …}` 那一群的内容开头是
/// `\*\picprop`，而这一族确实认得 picprop 里写的是什么，所以要能看见那个名字
fn unstar(group: &[u8]) -> &[u8] {
    if group.starts_with(b"\\*") {
        return &group[2..];
    }
    group
}

/// 从 `at` 起走到「关掉当前这一群」的那个 `}`，只交那个下标（不抄整群）
fn group_stop(bytes: &[u8], at: usize) -> usize {
    let mut depth = 0i32;
    let mut i = at;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => {
                if depth == 0 {
                    return i;
                }
                depth -= 1;
            }
            b'\\' => {
                if matches!(bytes.get(i + 1), Some(&b'{') | Some(&b'}')) {
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    bytes.len()
}

/// 一群里第一个「说这是哪种图」的控制字（`\pngblip`、`\jpegblip`、`\dibitmap` 这些）：
/// 交回它自己的名字与它写完之后的位置 —— 紧跟其后的就是那串十六进制
fn blip_word(group: &[u8]) -> Option<(String, usize)> {
    let mut i = 0usize;
    while i < group.len() {
        if group[i] != b'\\' {
            i += 1;
            continue;
        }
        let mut k = i + 1;
        let mut name: Vec<u8> = Vec::new();
        while k < group.len() && group[k].is_ascii_alphabetic() {
            name.push(group[k]);
            k += 1;
        }
        while k < group.len() && (group[k].is_ascii_digit() || group[k] == b'-') {
            k += 1;
        }
        let text = String::from_utf8_lossy(&name).into_owned();
        if text.ends_with("blip") || matches!(text.as_str(), "dibitmap" | "pictbitmap" | "macpict")
        {
            return Some((text, k));
        }
        i = if k > i + 1 { k } else { i + 1 };
    }
    None
}

/// 一个十六进制字符的值（不是十六进制就 None —— 这里要分「读到了」与「读不下去了」）
fn hex_nibble(one: u8) -> Option<u8> {
    match one {
        b'0'..=b'9' => Some(one - b'0'),
        b'a'..=b'f' => Some(one - b'a' + 10),
        b'A'..=b'F' => Some(one - b'A' + 10),
        _ => None,
    }
}

/// 从 `from` 起那串十六进制的前 8 个字节。数据行里可以夹着换行与空格（实测
/// LibreOffice 就是这么折行的），所以空白跳过而不是停下；碰上一个不是十六进制
/// 也不是空白的字节才停（那已经是群里的别的东西了）
fn hex_head(group: &[u8], from: usize) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    let mut high: Option<u8> = None;
    let mut i = from;
    while i < group.len() && out.len() < 8 {
        let one = group[i];
        if matches!(one, b' ' | b'\t' | b'\r' | b'\n') {
            i += 1;
            continue;
        }
        let Some(digit) = hex_nibble(one) else { break };
        match high {
            None => high = Some(digit),
            Some(first) => {
                out.push(first << 4 | digit);
                high = None;
            }
        }
        i += 1;
    }
    out
}

/// 那几个字节是什么图。认不出名字的交 "unknown"（读到了字节但不替它编名字），
/// 一个字节都没读到才交 None
fn picture_kind(head: &[u8]) -> Option<&'static str> {
    if head.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
        return Some("png");
    }
    if head.starts_with(&[0xFF, 0xD8]) {
        return Some("jpeg");
    }
    if head.starts_with(&[0x42, 0x4D]) {
        return Some("bmp");
    }
    if head.starts_with(&[0x47, 0x49, 0x46, 0x38]) {
        return Some("gif");
    }
    if head.starts_with(&[0xD7, 0xCD, 0xC6, 0x9A]) {
        return Some("emf");
    }
    if head.starts_with(&[0x49, 0x49, 0x2A, 0x00]) || head.starts_with(&[0x4D, 0x4D, 0x00, 0x2A]) {
        return Some("tiff");
    }
    if head.is_empty() {
        return None;
    }
    Some("unknown")
}

/// 「文件说这是什么格式」与「那串字节自己说这是什么格式」对不对得上。
/// 只在两边说的是同一个词表里的东西时才比：`\pngblip` 的词干是 `png`，
/// 而 `\dibitmap` 那种没有词干可读 —— 那种交 null 而不是猜一个「不一致」
fn blip_agrees(kind: &str, sig: Option<&str>) -> Option<bool> {
    let said = kind.strip_suffix("blip")?;
    let got = sig?;
    if said.is_empty() || got == "unknown" {
        return None;
    }
    Some(said == got)
}

/// `{\*\picprop …}` 那一格里是一堆 `{\sp{\sn 名字}{\sv 值}}`：按文件的顺序一对一条交，
/// 名字与值都解掉 `\uN` 与 `\'hh`。值可以是**空串**（`notes.rtf` 的两条都是），
/// 那与「这一对根本没写 `{\sv}` 那一格」是两件事，所以另给 `value_written`。
/// 第一个布尔说「有没有见过 picprop 那一格」—— 「整个没有」与「有而一条都不认得」
/// 也不是同一句话
fn shape_props(group: &[u8]) -> (bool, Vec<Value>) {
    let mut seen = false;
    let mut out: Vec<Value> = Vec::new();
    for holder in child_groups(group) {
        if !starts_word(unstar(&holder), "picprop") {
            continue;
        }
        seen = true;
        for one in child_groups(&holder) {
            let mut name: Option<String> = None;
            let mut value: Option<String> = None;
            for had in child_groups(&one) {
                let body = unstar(&had);
                if starts_word(body, "sn") {
                    name = Some(extract(payload_of(body)).text);
                } else if starts_word(body, "sv") {
                    value = Some(extract(payload_of(body)).text);
                }
            }
            out.push(json!({
                "name": name,
                "value": value.clone(),
                "value_written": value.is_some(),
            }));
        }
    }
    (seen, out)
}

/// 一张 `{\pict …}` 群的账。这一族把「多大」写在**三种单位**上：`picw`/`pich` 是像素、
/// `picwgoal`/`pichgoal` 是 twips（换成 0.01mm，与「那张纸」同一条整数式子）、
/// `picscalex`/`picscaley` 是百分比。文件里没有一个地方写 DPI，所以像素那两个
/// 不换算法；而页面上那一个尺寸要把三者合起来才得到 —— 那是推算，不交，
/// 三个数各按各的原样给出（实测 `images.rtf` 的 `480` twips 与同一批字的 docx 里
/// 那个 `1440000` EMU 不是同一个数，这一族把尺寸拆成了「目标 × 缩放」两半）。
/// 「这是什么格式的图」有两份凭据：`pngblip` 那个控制字说的，与紧跟其后那串
/// 十六进制自己带的前八个字节，两个都交，再给一个只在两边都认得时才比的 `sig_agrees`。
/// 替代文字住在 `{\*\picprop}` 的 `wzDescription` 那一条里（`notes.rtf` 写的是空值，
/// 所以 `alt_written` 与「alt 非空」是两件事）。
fn picture_ledger(bytes: &[u8], at: usize) -> Value {
    let stop = group_stop(bytes, at);
    let from = at.min(bytes.len());
    let to = stop.min(from + PICTURE_SCAN_CAP).max(from);
    let head = &bytes[from..to];
    let blip = blip_word(head);
    let kind: Option<String> = blip.as_ref().map(|one| one.0.clone());
    let read: Vec<u8> = match blip.as_ref().map(|one| one.1) {
        Some(at_data) => hex_head(head, at_data),
        None => Vec::new(),
    };
    let sig = picture_kind(&read);
    let agrees = match kind.as_deref() {
        Some(raw) => blip_agrees(raw, sig),
        None => None,
    };
    let side = |want: &str| -> Option<String> { word_in_group(head, want) };
    let mm = |want: &str| -> Option<i64> {
        word_in_group(head, want).and_then(|raw| crate::paper::twips(&raw))
    };
    let (props_written, props) = shape_props(head);
    let alt = props
        .iter()
        .find(|one| one["name"].as_str() == Some("wzDescription"))
        .and_then(|one| one["value"].as_str().map(|raw| raw.to_string()));
    json!({
        "blip": kind,
        "sig": sig,
        "sig_agrees": agrees,
        "head_hex": read.iter().map(|one| format!("{:02x}", one)).collect::<String>(),
        "pixels": {"w": side("picw"), "h": side("pich")},
        "goal": {
            "w": side("picwgoal"),
            "h": side("pichgoal"),
            "unit": "twips",
            "mm_w": mm("picwgoal"),
            "mm_h": mm("pichgoal"),
        },
        "scale": {"x": side("picscalex"), "y": side("picscaley")},
        "crop": {
            "left": side("piccropl"),
            "right": side("piccropr"),
            "top": side("piccropt"),
            "bottom": side("piccropb"),
        },
        "props_written": props_written,
        "props": props,
        "alt": alt,
        "alt_written": props
            .iter()
            .any(|one| one["name"].as_str() == Some("wzDescription")),
        "truncated": stop > at + PICTURE_SCAN_CAP,
    })
}

/// `{\colortbl;\red0\green0\blue0;…}` 那一群：一格一个号，**第一个空位就是 0 号**
/// （Word 与 LibreOffice 都这么写：开头那一对分号之间什么都没有，那是 `auto`）。
/// 三条 `\red` `\green` `\blue` 齐了才算一个颜色，缺一条的那一格交 null 而不是补 0 ——
/// 补 0 就是替文件说它没说过的话
fn color_table(inner: &[u8]) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    let mut parts: [Option<u64>; 3] = [None, None, None];
    let mut at = 0usize;
    while at < inner.len() {
        if inner[at] == b';' {
            out.push(match parts {
                [Some(r), Some(g), Some(b)] => json!(format!("{r:02X}{g:02X}{b:02X}")),
                _ => Value::Null,
            });
            parts = [None, None, None];
            at += 1;
            continue;
        }
        if inner[at] != b'\\' {
            at += 1;
            continue;
        }
        let word = peek_word(inner, at + 1);
        let which = match word.as_str() {
            "red" => 0usize,
            "green" => 1usize,
            "blue" => 2usize,
            _ => {
                at += 1;
                continue;
            }
        };
        let stop = at + 1 + word.len();
        parts[which] = digits_after(inner, stop);
        at = stop;
    }
    out
}

/// 这一族说「开 / 关」只看那个数字参数：`\b` 与 `\b1` 都是开，`\b0` 是明确写着不
fn word_switch(words: &[(String, String)], name: &str) -> Value {
    match words.iter().find(|one| one.0.as_str() == name) {
        None => Value::Null,
        Some(one) => json!(one.1.is_empty() || one.1 != "0"),
    }
}

/// 下划线那四个口袋：文件点哪个算哪个，一个都没点才是「没说」
fn underline_switch(words: &[(String, String)]) -> (Value, Value) {
    let hits: Vec<&(String, String)> = words
        .iter()
        .filter(|one| RUN_UNDERLINE.contains(&one.0.as_str()))
        .collect();
    if hits.is_empty() {
        return (Value::Null, Value::Null);
    }
    (
        json!(hits.iter().any(|one| one.1.is_empty() || one.1 != "0")),
        json!(hits[0].0.clone()),
    )
}

/// 号 → 那张表里的那一格。号没写、表没读到、号越界，都交 `resolved: false` 与原样那个号，
/// 不拿最近的一格顶上
fn color_hop(words: &[(String, String)], name: &str, colors: &[Value]) -> Value {
    let digits = match words.iter().find(|one| one.0.as_str() == name) {
        Some(one) => one.1.clone(),
        None => return Value::Null,
    };
    let read = digits.parse::<usize>().ok().and_then(|at| colors.get(at));
    match read {
        Some(had) if !had.is_null() => json!({"index": digits, "resolved": true, "rgb": had}),
        _ => json!({"index": digits, "resolved": false, "rgb": Value::Null}),
    }
}

/// 字体号 → `{\fonttbl` 里的那一条。名字解不动（非 ANSI 字符集里的非 ASCII 字节）
/// 那条本来就叫 `name: null`，所以这里 `resolved` 说的是「表里有这一条」，
/// 而名字照旧交 null —— 表查到了不等于名字读出来了
fn font_hop(words: &[(String, String)], name: &str, fonts: &[Value]) -> Value {
    let digits = match words.iter().find(|one| one.0.as_str() == name) {
        Some(one) => one.1.clone(),
        None => return Value::Null,
    };
    let want = match digits.parse::<u64>() {
        Ok(one) => one,
        Err(_) => return json!({"index": digits, "resolved": false, "name": Value::Null}),
    };
    match fonts.iter().find(|one| one["index"] == json!(want)) {
        Some(one) => json!({"index": digits, "resolved": true, "name": one["name"]}),
        None => json!({"index": digits, "resolved": false, "name": Value::Null}),
    }
}

/// 字符样式号 → `\stylesheet` 里那一条 `\*\csN`。与字体那一跳同一个口径：
/// `resolved` 说的是「表里有这一条」，名字解不动仍交 null。
/// 实测 LibreOffice 把 `Strong` 写成 `\cs34`，而群头上**同时**把这个样式自己的
/// `\b` 抄了一遍 —— 两处都在文件上，所以两边都交，不合成一个「这串字是粗的」
fn style_hop(words: &[(String, String)], name: &str, styles: &[Value]) -> Value {
    let digits = match words.iter().find(|one| one.0.as_str() == name) {
        Some(one) => one.1.clone(),
        None => return Value::Null,
    };
    let want = match digits.parse::<u64>() {
        Ok(one) => one,
        Err(_) => return json!({"index": digits, "resolved": false, "name": Value::Null}),
    };
    match styles
        .iter()
        .find(|one| one["kind"] == json!("character") && one["index"] == json!(want))
    {
        Some(one) => json!({"index": digits, "resolved": true, "name": one["name"]}),
        None => json!({"index": digits, "resolved": false, "name": Value::Null}),
    }
}

/// 群头上那一串控制字之后解出来的两本账：`switches` 是四个开关各读一次，
/// `values` 是那三个号（颜色、字号、字体）按文件自己的表跳一跳。
/// 走这一趟是在整条流读完**之后**，所以表的先后顺序不参与判断
fn resolve_runs(rows: &mut Vec<Value>, colors: &[Value], fonts: &[Value], styles: &[Value]) {
    for row in rows.iter_mut() {
        let words: Vec<(String, String)> = match row["words"].as_array() {
            Some(had) => had
                .iter()
                .map(|one| {
                    (
                        one[0].as_str().unwrap_or_default().to_string(),
                        one[1].as_str().unwrap_or_default().to_string(),
                    )
                })
                .collect(),
            None => Vec::new(),
        };
        let (underline, which_word) = underline_switch(&words);
        let position = ["super", "sub"]
            .iter()
            .find(|name| words.iter().any(|one| one.0.as_str() == **name))
            .map(|one| json!(one.to_string()))
            .unwrap_or(Value::Null);
        let said = |want: &str| -> Option<String> {
            words
                .iter()
                .find(|one| one.0.as_str() == want)
                .map(|one| one.1.clone())
        };
        row["switches"] = json!({
            "bold": word_switch(&words, "b"),
            "italic": word_switch(&words, "i"),
            "strike": word_switch(&words, "strike"),
            "underline": underline,
        });
        row["values"] = json!({
            "color": color_hop(&words, "cf", colors),
            "fill": color_hop(&words, "cb", colors),
            "highlight": color_hop(&words, "highlight", colors),
            "underline_word": which_word,
            // 字号按原样交（半磅），与 docx 那个 `w:sz` 是同一个单位、同一个数，所以不换算法
            "size": said("fs").map(|raw| json!(raw)).unwrap_or(Value::Null),
            "asian_size": said("afs").map(|raw| json!(raw)).unwrap_or(Value::Null),
            "position": position,
            "font": font_hop(&words, "f", fonts),
            "asian_font": font_hop(&words, "af", fonts),
            "character_style": style_hop(&words, "cs", styles),
        });
        row["format"] = Value::Array(
            words
                .iter()
                .map(|(one, two)| {
                    json!({
                        "element": one.as_str(),
                        "digits": if two.is_empty() { Value::Null } else { json!(two) },
                    })
                })
                .collect::<Vec<Value>>(),
        );
    }
}

/// `{\listlevel\levelnfc0…{\leveltext …;}{\levelnumbers…;}\fi-360\li1080}` 一群：
/// 这一级的账。级别号是**这一份 list 里第几个 `{\listlevel`**（实测 LibreOffice 不在
/// 级上写 `\ilvl`，一份也没有），所以号是读者按顺序给的，不是文件写的
fn list_level_of(group: &[u8], at: usize) -> Value {
    let kids = child_groups(group);
    let pick = |want: &str| -> Option<String> {
        kids.iter()
            .find(|one| starts_word(one, want))
            .map(|one| written_of(one))
    };
    json!({
        "at": at,
        "nfc": word_in_group(group, "levelnfc"),
        "jc": word_in_group(group, "leveljc"),
        "startat": word_in_group(group, "levelstartat"),
        "follow": word_in_group(group, "levelfollow"),
        "font": word_in_group(group, "f").filter(|one| !one.is_empty()),
        "first_indent": word_in_group(group, "fi"),
        "indent": word_in_group(group, "li"),
        "level_text": pick("leveltext"),
        "level_numbers": pick("levelnumbers"),
        "children": kids.into_iter().map(|one| written_of(&one)).collect::<Vec<String>>(),
    })
}

/// `{\list\listtemplateid1 {…九级…}\listid1}` 一群：一份列表定义。
/// **`\listid` 写在群的最后**（实测：按 `{\list\listid` 去抓一条也抓不到，
/// 而全文 `\listid` 有 14 次 —— 这里 7 次、`listoverridetable` 里 7 次），
/// 所以整群读完才拿得到号
fn list_definition_of(group: &[u8]) -> Value {
    let levels: Vec<Value> = child_groups(group)
        .into_iter()
        .filter(|one| starts_word(one, "listlevel"))
        .enumerate()
        .map(|(at, one)| list_level_of(&one, at))
        .collect();
    let nfc: Vec<Value> = levels.iter().map(|one| one["nfc"].clone()).collect();
    json!({
        "template_id": word_in_group(group, "listtemplateid"),
        "list_id": word_in_group(group, "listid"),
        "levels": levels.len(),
        "nfc": nfc,
        "list_level": levels,
    })
}

/// `{\listoverride\listid4\listoverridecount0\ls4}` 一群：段上那个 `\ls` 号
/// 指的是哪一份定义。群里的 `listoverridecount` 说的是「这一条覆写了几级」
/// （实测 LibreOffice 写 0 —— 它只换个号，一级也没改），与群名不是一回事
fn list_override_of(group: &[u8]) -> Option<Value> {
    let ls = word_in_group(group, "ls")?;
    Some(json!({
        "ls": ls,
        "list_id": word_in_group(group, "listid"),
        "override_count": word_in_group(group, "listoverridecount"),
    }))
}

/// 一段自己说过的「列表上的话」（与 `marks` 一条一条对着收，所以段号不会分家）
struct ParaFlow {
    ilvl: Option<String>,
    ls: Option<String>,
    li: Option<String>,
    fi: Option<String>,
    label: Option<Value>,
}

/// 一条 `{\f0 … Times New Roman;}` / `{\s1 … heading 1;}` / `{\*\cs15 … Name;}`：
/// 种类、编号、字符集与名字。名字用本模块自己解（控制字不产字，
/// 嵌套的 `{\*\falt …}` 那一群按「不认识就跳」跳掉），末尾那个分号去掉。
fn definition_of(child: &[u8]) -> Option<Value> {
    const CHARSET_HEAD: &[u8] = b"\\fcharset";
    let rest = if let Some(tail) = child.strip_prefix(b"\\*\\") {
        tail
    } else if let Some(tail) = child.strip_prefix(b"\\") {
        tail
    } else {
        return None;
    };
    let (prefix, kind) = if rest.starts_with(b"cs") {
        (2usize, "character")
    } else if rest.starts_with(b"s") {
        (1usize, "paragraph")
    } else if rest.starts_with(b"f") {
        (1usize, "font")
    } else {
        return None;
    };
    let index = digits_after(rest, prefix)?;
    let mut charset = 0u64;
    if let Some(at) = windows_position(child, 0, CHARSET_HEAD) {
        charset = digits_after(child, at + CHARSET_HEAD.len()).unwrap_or(0);
    }
    // 名字要**剥掉开头那个 `\*` 再解**：字符样式的定义统统写成 `{\*\csN … 名字;}`，
    // 而 `extract` 见 `\*` 就整群跳（那是它该有的行为），于是一个名字也剩不下来 ——
    // 「不认识才跳」不等于「这一族认得它的名字，还是让它把名字跳掉」（脚注那一条同一个道理）
    let name = extract(unstar(child))
        .text
        .trim()
        .trim_end_matches(';')
        .trim_end()
        .to_string();
    // 全 ASCII 的名字与字符集无关（RTF 用的每一种字符集都含 ASCII）；
    // 否则只有 ANSI(0) 那份能按 cp1252 读。非 ANSI 又含非 ASCII 的名字交回 null ——
    // 按 cp1252 硬解会得出「‚l‚r ƒSƒVƒbƒN」这种串（文件写的是 Shift-JIS），
    // 那不是名字，是我们解错了
    let ascii_name = name.chars().all(|one| (one as u32) < 0x80);
    let shown = if name.is_empty() || (kind == "font" && charset != 0 && !ascii_name) {
        Value::Null
    } else {
        json!(name)
    };
    let kind_of = if kind == "font" {
        json!(charset)
    } else {
        Value::Null
    };
    Some(json!({"kind": kind, "index": index, "charset": kind_of, "name": shown}))
}

/// 批注住在星号群（`{\*\…}`）里，本域认得其中两个词：注的那一群带着正文与自己的号
/// （`{\*\atnref N}` 与 `{\*\atndate D}` 就坐在它里面），紧挨在它前面那条
/// `{\*\atnauthor …}` 是「谁写的」。`{\*\atnid …}` 那个字母（批注框里显示什么）没人问，
/// 锚区两头的 `{\*\atrfstart N}` / `{\*\atrfend N}` 也不单独进账 —— 那个号已经在注
/// 自己的 `ref` 里交出来了，两边按号对。
/// Word 那一族另写 `atncluster` / `atnatom` 一条链，手上没有那种生产者，这里不猜
const ATN_WORDS: &[&str] = &["annotation", "atnauthor"];
/// 注的那一群里另坐着自己的两个值（它们不单独进账，跟着这一条注交）
const ATN_CHILDREN: &[&str] = &["atnref", "atndate"];

/// 断点词按**词边界**数：子串数会骗人（`\pard` 里有 `\par`、`\sectd` 与 `\sectx`
/// 里有 `\sect`）。键固定这六个，一个都没出现也交 0 —— 「数过了，没有」与
/// 「没数」不是一件事。这一族把「换页」写成三种词：`\page` 在这里换页，
/// `\pagebb` / `\pbb` 是「这一段之前换页」，而 Word 那条 `w:br w:type="page"`
/// 在 LibreOffice 的 RTF 导出里就是 `\pagebb`（三份件都这么量到，`page` 反而一条没有）
pub const BREAK_WORD_NAMES: [&str; 6] = ["par", "line", "page", "pagebb", "pbb", "sect"];

/// 这个词是断点词吗（是的话排在第几位）
fn break_word_index(word: &str) -> Option<usize> {
    BREAK_WORD_NAMES.iter().position(|one| *one == word)
}

/// 从 `from`（紧跟 `\*\` 的那个控制字的第一个字母）起那一群：交回「解过一遍的字」
/// 与群里那几个认识的子群（`{\*\atnref 0}` 那种）。**前瞻**用的 —— 调用方照旧把
/// 这一群整群跳过，所以这里的 extract 只读那一段，正文一个字也不会多
fn starred_body(bytes: &[u8], from: usize, named: &str) -> (String, Vec<(String, String)>) {
    let (_stop, inner) = group_end(bytes, from);
    // 群内容从这个控制字的位置起截，那几个字母本身要剥掉才剩下值
    let body = match inner.get(named.len()..) {
        Some(one) => one,
        None => &inner[..0],
    };
    let text = extract(body).text;
    let kids: Vec<(String, String)> = child_groups(body)
        .iter()
        .filter_map(|one| atn_child(one))
        .collect();
    (text.trim().to_string(), kids)
}

/// 一个子群是不是 `{\*\atnref N}` / `{\*\atndate D}` 那种「控制字紧跟一个值」的形状
fn atn_child(child: &[u8]) -> Option<(String, String)> {
    let rest = child.strip_prefix(b"\\*\\")?;
    let mut name: Vec<u8> = Vec::new();
    let mut k = 0usize;
    while k < rest.len() && rest[k].is_ascii_alphabetic() {
        name.push(rest[k]);
        k += 1;
    }
    let name = String::from_utf8_lossy(&name).into_owned();
    if !ATN_CHILDREN.contains(&name.as_str()) {
        return None;
    }
    let had = extract(&rest[k..]).text;
    let had = had.trim();
    if had.is_empty() {
        return None;
    }
    Some((name, had.to_string()))
}

/// 从 `from` 起那一串数字（没有数字就交回 None）
fn digits_after(bytes: &[u8], from: usize) -> Option<u64> {
    let mut num: Vec<u8> = Vec::new();
    let mut k = from;
    while k < bytes.len() && bytes[k].is_ascii_digit() {
        num.push(bytes[k]);
        k += 1;
    }
    if num.is_empty() {
        return None;
    }
    Some(String::from_utf8_lossy(&num).parse().unwrap_or(0))
}

/// 样式表里那个号的名字写成 `heading N` 时，N 就是段落的层级。
/// 形状之外什么都不认：`heading` 与数字之间至少一个空格、数字后面不能再有别的东西
/// （`Heading1`、`heading 1a` 都不算 —— 那是文件里另一个名字，不是我们的推断）。
/// 大小写各家不同（Word 写 `Heading 1`，LibreOffice 的 RTF 导出写 `heading 1`），
/// 所以只有这个词不分大小写。只查段落样式：`\sN` 与 `\csN` 是两个各自的编号空间
fn heading_level(styles: &[Value], index: u64) -> Option<usize> {
    let named = styles
        .iter()
        .find(|one| one["kind"] == json!("paragraph") && one["index"] == json!(index))?;
    let name = named["name"].as_str()?.trim();
    let bytes = name.as_bytes();
    if bytes.len() < 7 || !bytes[..7].eq_ignore_ascii_case(b"heading") {
        return None;
    }
    let after = &name[7..];
    if !after
        .as_bytes()
        .first()
        .is_some_and(|one| one.is_ascii_whitespace())
    {
        return None;
    }
    let rest = after.trim_start();
    let end = rest
        .find(|one: char| !one.is_ascii_digit())
        .unwrap_or(rest.len());
    if end == 0 || !rest[end..].trim().is_empty() {
        return None;
    }
    rest[..end].parse().ok()
}

/// 单元格分隔：输出一个制表符
const TAB_WORDS: &[&str] = &["tab", "cell", "nestcell"];
/// 制表位那一族的两种前缀：它们**只管紧跟的那一个** `\tx`（实测 LibreOffice 的 RTF 导出
/// 写成 `\tldot\tqr\tx1701\tlul\tx5102` —— 第二条的位置没有对齐前缀，就是这一族的默认）
const TAB_ALIGN_WORDS: &[&str] = &["tq", "tqc", "tqr", "tqdec", "tqbar"];
const TAB_LEADER_WORDS: &[&str] = &["tldot", "tleq", "tlhyph", "tlth", "tlul", "tlbtk"];
/// 行结束：一行表格就是一行文本 —— 把 \row 当制表符会把整张表挤成一行
const ROW_WORDS: &[&str] = &["row", "nestrow"];
/// 嵌套对象类：整群跳过并计数（办公文件里最常见的是 OLE 对象）
const OBJECT_WORDS: &[&str] = &[
    "object", "objattph", "objdata", "objclass", "objname", "objemb", "objhide",
];

/// 文档级「那张纸」写在哪几个控制字上（单位是 twips，1/1440 英寸）。
/// `landscape` 是个旗标（写了就是横的），其余都带一个数字参数
const PAPER_WORDS: &[&str] = &[
    "paperw",
    "paperh",
    "margl",
    "margr",
    "margt",
    "margb",
    "landscape",
];
/// 注的两个口袋。**LibreOffice 的 RTF 导出只用 `footnote` 这一个**，
/// 尾注靠群里的 `\ftnalt` 反标志区分（Word 那族还会另写 `endnote` 口袋），
/// 所以两个词都认、再看那个标志
const NOTE_DESTINATIONS: &[&str] = &["footnote", "endnote"];

/// 注的**排版定义**（分隔符、续分符、编号占位）不是一条注：
/// 这份件里就明写着 `{\\*\\ftnsep\\chftnsep}` —— 当成注就会凭空多出几条空注
const NOTE_DEFINITION_WORDS: &[&str] = &[
    "ftnsep", "ftnsepc", "ftncn", "aftnsep", "aftnsepc", "aftncn",
];

/// `\*` 之后紧跟的那个控制字叫什么（可能隔着空白与另一个反斜杠）
fn peek_word(bytes: &[u8], at: usize) -> String {
    let mut k = at;
    while k < bytes.len()
        && (bytes[k] == b' ' || bytes[k] == b'\r' || bytes[k] == b'\n' || bytes[k] == b'\\')
    {
        k += 1;
    }
    let mut word: Vec<u8> = Vec::new();
    while k < bytes.len() && bytes[k].is_ascii_alphabetic() {
        word.push(bytes[k]);
        k += 1;
    }
    String::from_utf8_lossy(&word).into_owned()
}

#[derive(Debug, Clone)]
pub struct Rtf {
    pub text: String,
    pub lines: Vec<String>,
    pub declared_codepage: u32,
    pub hex_bytes: usize,
    pub unicode_escapes: usize,
    pub pictures: usize,
    /// 逐张图的账本（`picture_ledger`）：条数与 `pictures` 是同一趟走出来的，
    /// 只是这一本多存到 `PICTURE_ROW_CAP` 张为止
    pub picture_rows: Vec<Value>,
    /// 逐串的字符格式：一条 = 一个**说过格式控制字的群**（群头 + 群里解出来的字）。
    /// 这一族没有「一串字」这个元素，所以没说过话的群不进账本（那是判不住，不是 0）
    pub run_rows: Vec<Value>,
    /// 格式控制字落在**所有群之外**（段前缀那一层）的条数：那一条归属于段，
    /// 不归属于任何一串字，所以只数不挂。实测 LibreOffice 每一段都重发一份样式默认值
    /// （`\cf0`、`\fs22`、`\kerning0`…），这个数就是那一份的量，它同时说出
    /// 「这一族的段落级格式与字符级格式共用同一批控制字」
    pub run_words_stray: usize,
    /// `\colortbl` 那张表：号 → `"RRGGBB"`，`0` 号那个空位与没写全三条的那一格交 null。
    /// 那一群照旧整群跳过（里面一个字节都不是页面上的字），这里只是前瞻读一眼
    pub colors: Vec<Value>,
    pub embedded_objects: usize,
    pub skipped_destinations: usize,
    /// 页眉与页脚的字：它们与正文混在同一个流里，靠目标群分开。
    /// 一条一节会同时写进 `\header`、`\headerl`、`\headert` 好几个口袋，
    /// 所以每条都带着自己是哪个口袋（slot），不替文件合并
    pub headers: Vec<Value>,
    pub footers: Vec<Value>,
    pub page_destinations: usize,
    /// 脚注与尾注：每条带 `kind`（footnote / endnote）、`slot`（文件用的哪个口袋名）与字。
    /// 注的字**不混进正文** —— 它住在正文流里的一个目标群里，位置就在引用点后面
    pub note_list: Vec<Value>,
    pub note_destinations: usize,
    /// 链接：`{\field{\*\fldinst HYPERLINK "地址" }{\fldrslt {显示文字}}}` 那一群读出来的。
    /// 域指令那一群仍然是跳过的（它不是页面上的字），这里只是**前瞻**读它一眼，
    /// 不推进游标 —— 显示文字照旧留在正文里
    pub links: Vec<Value>,
    /// `\field` 出现了几次（一份文档里域比链接多：页码、日期都是域）
    pub fields: usize,
    /// 每个域自己写的指令原文，按文件里的顺序（`TOC \o "1-2" \h`、`PAGEREF _Toc… \h`…）。
    /// 与 `fields` 是两本账：那一条只数控制字 `\field`，这一条要群里真有指令才算。
    /// `\fldinst` 那一群照旧是「不认识就跳」的目标群 —— 这里只**前瞻**读一眼，
    /// 一个字不进正文
    pub field_instructions: Vec<String>,
    /// 字体表与样式表里的定义（前瞻读出来的，那一群照旧不进正文）。
    /// 每条是 `{kind, index, charset, name}`；字体条目自己声明了非 ANSI 字符集
    /// （`\fcharset128` 是 Shift-JIS）而名字里又有非 ASCII 字节时交回 `name: null` ——
    /// 按 cp1252 硬解出来的那串不是名字，是我们解错了
    pub fonts: Vec<Value>,
    pub styles: Vec<Value>,
    /// 正文里用了哪些样式号、各几次（样式表那一群自己不算）
    pub style_uses: Vec<Value>,
    /// 标题：样式名写成 `heading N` 的那些段，层级就写在名字里。
    /// 段用的是哪个样式号在段属性里（`\pard\s1`），名字在样式表里，两边一接才有层级
    pub headings: Vec<Value>,
    /// 文档级那张纸的**原样**：`\paperw12240` 这类控制字，每个词只留没被跳过的那一层里
    /// 第一次写的那一个（后面 `{\*\sectx …}` 里的那些是某一节的覆写，而这一族不判分节归属）。
    /// 换算在 `crate::paper`（三家同一条式子），这里不预先换成毫米
    pub paper_writes: Vec<(String, String)>,
    /// 批注（`{\*\annotation …}` 那一群）。每条是 `{author, text, ref, date_written}`：
    /// 作者是紧跟在注之前那条 `{\*\atnauthor …}`（按文件的顺序配，配不上就是 null），
    /// `ref` 是注自己那条 `{\*\atnref N}` —— 那个号与锚区两头的 `{\*\atrfstart N}` /
    /// `{\*\atrfend N}` 是同一个数，所以「钉在哪一段」有文件自己的号可查，不靠我们猜。
    /// `date` 一律 null：那一群写的 `{\*\atndate …}` 两个样本都对不上 docx 那边的
    /// `w:date`（一份 1743371367、一份 -2014723526），解不动就只交原样那串（`date_written`）
    /// 书签的名字（`{\*\bkmkstart 名}` 那一群里读出来的，按文件顺序）
    pub bookmarks: Vec<String>,
    /// 两条列表各数一遍：start 与 end 不等就是文件自己没配上
    pub bookmark_starts: usize,
    pub bookmark_ends: usize,
    pub annotations: Vec<Value>,
    /// `{\*\atnauthor …}` 出现了几条：与 `annotations.len()` 不等就是文件自己没配上
    /// （与 .xls 那两支列表同一个做法 —— 配不上时把两个数都交出来，不替它对齐）
    pub annotation_authors: usize,
    /// 断点词的条数，按 `BREAK_WORD_NAMES` 那六个键的顺序（只在没被跳过的那一层数：
    /// 页眉里那条 `\par` 不是正文的一段）
    pub break_words: [usize; 6],
    /// 制表位这一族：`\tx` 逐条交（每条带上它前面那两个「只管这一条」的前缀），
    /// `\tab` 是**字符**（与定义分开的另一本账），那两个计数是前缀词各出现几次 ——
    /// 前缀比 `\tx` 多就是有条前缀没配上位置（这一族允许这么写）
    pub tab_rows: Vec<Value>,
    pub tab_chars: usize,
    pub tab_align_words: usize,
    pub tab_leader_words: usize,
    /// 表那份账。这六个数都是**控制字本身的条数**（`\trowd` / `\row` / `\cell` / `\intbl`
    /// 与嵌套表那两个），不是「有几张表」的推断 —— 那条规则拿两份件试过：
    /// 一张 2×2 的对，两张（3×2 与 2×2）的把两张数成一张，所以这里只交数得清的
    pub table_row_defines: usize,
    pub table_rows: usize,
    pub table_cells: usize,
    pub table_cell_paras: usize,
    pub nested_table_rows: usize,
    pub nested_table_cells: usize,
    /// 这一群里出现过 `\ftnalt`：LibreOffice 用它把 `footnote` 口袋标成尾注。
    /// 只在提取子群时用来判 kind，不单独交出去
    pub ftnalt: bool,
    /// `{\listtext…}` 那种群一共出现了几次（与「几个段带标签」是两个数：
    /// 一段里如果有两条，只交第一条，另一条只在这个数里）
    pub label_words: usize,
    /// 列表那一份账（`structure.numbering` 的 RTF 那一支）：段的账 + 号本 + 定义的账
    pub numbering: Value,
    pub notes: Vec<String>,
}

impl Rtf {
    /// 站内跳转那一份账（anchors、落得地的、落不了的、站外的）：`to_json` 与
    /// `office-doc` 那条分支要的是同一个东西，两处各算一遍迟早分家 —— 所以算一次
    pub(crate) fn anchor_ledger(&self) -> (Vec<String>, usize, usize, usize) {
        let anchors: Vec<String> = self
            .field_instructions
            .iter()
            .filter_map(|raw| match raw.strip_prefix("HYPERLINK \"") {
                Some(rest) => rest.strip_suffix('"').and_then(|one| one.strip_prefix('#')),
                None => None,
            })
            .map(String::from)
            .collect();
        let external = self
            .field_instructions
            .iter()
            .filter(|raw| raw.starts_with("HYPERLINK \"") && !raw.contains("\"#"))
            .count();
        let found = anchors
            .iter()
            .filter(|raw| self.bookmarks.iter().any(|had| had == *raw))
            .count();
        let missing = anchors.len() - found;
        (anchors, found, missing, external)
    }

    pub fn to_json(&self) -> Value {
        // 站内跳转的地址住在指令里（解过转义的那一份），书签住在自己那一群里：
        // 两处的名字对上才算这一跳落得地，对不上就是一条坏跳转 —— 都只按写的比
        let (anchors, found, missing, external) = self.anchor_ledger();
        json!({
            "anchors": anchors,
            "anchors_found": found,
            "anchors_missing": missing,
            "links_external": external,
            "text": self.text,
            "lines": self.lines,
            "headers": self.headers,
            "footers": self.footers,
            "page_destinations": self.page_destinations,
            "note_list": self.note_list,
            "note_destinations": self.note_destinations,
            "links": self.links,
            "fields": self.fields,
            "field_instructions": self.field_instructions,
            "fonts": self.fonts,
            "styles": self.styles,
            "style_uses": self.style_uses,
            "headings": self.headings,
            "bookmarks": self.bookmarks,
            "bookmark_starts": self.bookmark_starts,
            "bookmark_ends": self.bookmark_ends,
            "annotations": self.annotations,
            "annotation_authors": self.annotation_authors,
            "break_words": {
                "par": self.break_words[0],
                "line": self.break_words[1],
                "page": self.break_words[2],
                "pagebb": self.break_words[3],
                "pbb": self.break_words[4],
                "sect": self.break_words[5],
            },
            "paper_writes": self.paper_writes,
            "line_count": self.lines.len(),
            "chars": self.text.chars().count(),
            "declared_codepage": self.declared_codepage,
            "hex_bytes": self.hex_bytes,
            "unicode_escapes": self.unicode_escapes,
            "pictures": self.pictures,
            "picture_list": self.picture_rows.clone(),
            "embedded_objects": self.embedded_objects,
            "skipped_destinations": self.skipped_destinations,
            "table_row_defines": self.table_row_defines,
            "table_rows": self.table_rows,
            "table_cells": self.table_cells,
            "table_cell_paras": self.table_cell_paras,
            "nested_table_rows": self.nested_table_rows,
            "nested_table_cells": self.nested_table_cells,
            "label_words": self.label_words,
            "numbering": self.numbering.clone(),
            "notes": self.notes,
        })
    }
}

/// 解一份 RTF。RTF 是 8 位文本协议，这里一律按字节走，非 ASCII 字节只出现在
/// `\'hh` 与文本里，所以用 `Vec<u8>` 而不是 `&str` 能避免一半越界问题。
pub fn extract(bytes: &[u8]) -> Rtf {
    let mut out: Vec<u8> = Vec::new();
    let mut pending: Vec<u8> = Vec::new();
    let mut skip: Vec<bool> = vec![false];
    // 与 `skip` 同步进出的一对栈：每一群自己那份「群头上说过的格式控制字」，
    // 以及这一群开群时 `out` 走到哪儿（收尾时那一段就是这一群的字）
    let mut word_stack: Vec<Vec<(String, String)>> = vec![Vec::new()];
    let mut run_start: Vec<usize> = vec![0];
    let mut codepage: u32 = 1252;
    let mut ucount: usize = 1;
    let mut notes: Vec<String> = Vec::new();
    // 正文里用到的样式号（样式表那一群自己不算，跳过的区域里也不算）
    let mut uses: Vec<u64> = Vec::new();
    // 段那一份账：每段收尾时记下「这一段的字从哪儿到哪儿」与「这一段用的是哪个样式号」。
    // 收尾点就是段控制字（`\par` 那几个）与行控制字（`\row`）；两端都记下，是因为切片
    // 只写起点会把后面整篇字都当成这一段的内容（那个 bug 由 CI 抓出来：标题变成整份文档）。
    // 这一段与 `lines` 是两本账：这里空段也留
    let mut marks: Vec<(usize, usize, Option<u64>)> = Vec::new();
    // 与 `marks` 一条一条对着来的段属性：`\ilvl` / `\ls` / `\li` / `\fi`，以及那段
    // 正文前那个 `{\listtext…}` 群。收在同一处、跟着同一次走，段号才不会分家
    let mut flows: Vec<ParaFlow> = Vec::new();
    let mut para_start = 0usize;
    let mut para_style: Option<u64> = None;
    let mut para_ilvl: Option<String> = None;
    let mut para_ls: Option<String> = None;
    let mut para_li: Option<String> = None;
    let mut para_fi: Option<String> = None;
    let mut para_label: Option<Value> = None;
    // 列表那两群里读出来的东西：定义与号本，各自按文件里的顺序
    let mut list_defs: Vec<Value> = Vec::new();
    let mut list_over: Vec<Value> = Vec::new();
    // 文档级那张纸的原样（`paper_writes`）：只收第一次写的那一个，见下面那条判断
    let mut paper: Vec<(String, String)> = Vec::new();
    // 刚读到、还没配上注的那条 `{\*\atnauthor …}`：文件把作者写在注的前面一格，
    // 所以「读到注」时取走它；取不到就交 null（有一格没作者就是文件的账，不补）
    let mut pending_author: Option<String> = None;
    // 刚读到、还没配上位置的那两个制表位前缀：这一族它们只管**紧跟的那一个** `\tx`，
    // 所以配上一个就清空（`\tx` 自己不带前缀就是这一族的默认，交 null）
    let mut tab_align: Option<String> = None;
    let mut tab_leader: Option<String> = None;
    let mut me = Rtf {
        text: String::new(),
        lines: Vec::new(),
        declared_codepage: 1252,
        hex_bytes: 0,
        unicode_escapes: 0,
        pictures: 0,
        picture_rows: Vec::new(),
        run_rows: Vec::new(),
        run_words_stray: 0,
        colors: Vec::new(),
        embedded_objects: 0,
        skipped_destinations: 0,
        headers: Vec::new(),
        footers: Vec::new(),
        page_destinations: 0,
        note_list: Vec::new(),
        note_destinations: 0,
        links: Vec::new(),
        fields: 0,
        field_instructions: Vec::new(),
        bookmarks: Vec::new(),
        bookmark_starts: 0,
        bookmark_ends: 0,
        fonts: Vec::new(),
        styles: Vec::new(),
        style_uses: Vec::new(),
        headings: Vec::new(),
        annotations: Vec::new(),
        annotation_authors: 0,
        break_words: [0; 6],
        tab_rows: Vec::new(),
        tab_chars: 0,
        tab_align_words: 0,
        tab_leader_words: 0,
        paper_writes: Vec::new(),
        table_row_defines: 0,
        table_rows: 0,
        table_cells: 0,
        table_cell_paras: 0,
        nested_table_rows: 0,
        nested_table_cells: 0,
        ftnalt: false,
        label_words: 0,
        numbering: Value::Null,
        notes: Vec::new(),
    };
    let mut i = 0usize;
    while i < bytes.len() {
        let ch = bytes[i];
        match ch {
            b'{' => {
                flush(&mut out, &mut pending, codepage, &mut notes);
                skip.push(*skip.last().unwrap_or(&false));
                word_stack.push(Vec::new());
                run_start.push(out.len());
                i += 1;
                continue;
            }
            b'}' => {
                flush(&mut out, &mut pending, codepage, &mut notes);
                // 一群收尾：群头上说过格式控制字、群里又有字，才是一条「这几串字长什么样」。
                // 弹栈与 `skip` 同一条规则（第 0 层那个占位不弹），所以闭群时 `skip.last()`
                // 说的正是**这一群**自己是不是被跳过的目标群
                let words = if word_stack.len() > 1 {
                    word_stack.pop().unwrap_or_default()
                } else {
                    Vec::new()
                };
                let start = if run_start.len() > 1 {
                    run_start.pop().unwrap_or(out.len())
                } else {
                    out.len()
                };
                // 第 2 层是文档群，它自己的「字」是整篇 —— 那不是串，所以只数第 3 层往下
                if skip.len() >= 3 && !*skip.last().unwrap_or(&false) && !words.is_empty() {
                    let direct = words
                        .iter()
                        .any(|(one, _)| RUN_DIRECT.contains(&one.as_str()));
                    let said: Vec<(String, String)> = words
                        .into_iter()
                        .filter(|(one, _)| RUN_WORDS.contains(&one.as_str()))
                        .collect();
                    let text = out
                        .get(start..)
                        .map(|had| String::from_utf8_lossy(had).trim().to_string())
                        .unwrap_or_default();
                    if !said.is_empty() && !text.is_empty() && me.run_rows.len() < RUN_ROW_CAP {
                        me.run_rows.push(json!({
                            "para": marks.len(),
                            "text": text,
                            "direct": direct,
                            "words": said
                                .iter()
                                .map(|(one, two)| json!([one.as_str(), two.as_str()]))
                                .collect::<Vec<Value>>(),
                        }));
                    }
                }
                if skip.len() > 1 {
                    skip.pop();
                }
                i += 1;
                continue;
            }
            b'\r' | b'\n' => {
                i += 1;
                continue;
            }
            _ => {}
        }
        if ch != b'\\' {
            flush(&mut out, &mut pending, codepage, &mut notes);
            if !*skip.last().unwrap_or(&false) {
                out.push(ch);
            }
            i += 1;
            continue;
        }
        // 反斜杠开头
        if bytes.get(i + 1) == Some(&b'*') {
            // `\*` 说的是「**紧跟它的那个目标群**你不认识就整群跳过」。
            // 所以先看清那是个什么群：认识的就不跳 —— LibreOffice 的脚注与尾注恰恰写成
            // `{\\*\\footnote …}`，一见 `\*` 就跳会把整条注丢掉（这份件就是这么发现的）。
            // 不认识才跳：fldinst（域指令原文）、userprops 都从这一条走。批注那两群
            // 认得，但**照样跳**（值只前瞻读一份，见下面那条），所以跳过的笔账不变
            flush(&mut out, &mut pending, codepage, &mut notes);
            let named = peek_word(bytes, i + 2);
            if NOTE_DESTINATIONS.contains(&named.as_str())
                || PAGE_DESTINATIONS.contains(&named.as_str())
            {
                i += 2;
                continue;
            }
            // 批注那两个词：认得，但这一群仍然整个跳过（注的字不是页面上的正文）——
            // 只**前瞻**把值读出来，所以 skipped_destinations 一个也不因为这个改动而变
            // `{{\}*\}listtable`：这一族的列表定义写在**星号群**里（实测 LibreOffice 在
            // 同一份件里把 `listtable` 带星号写、把 `listoverridetable` 不带星号写 ——
            // 只认一条路径就会一份读到、一份读不到）。整群照旧跳，`skipped_destinations`
            // 那一笔也不动，只是里面的定义不再跟着群一起丢
            if DEF_DESTINATIONS.contains(&named.as_str()) && !*skip.last().unwrap_or(&false) {
                let mut head = i + 2 + named.len();
                if bytes.get(head) == Some(&b' ') {
                    head += 1;
                }
                let (_stop, inner) = group_end(bytes, head);
                if named == "listtable" {
                    for child in child_groups(&inner) {
                        if starts_word(&child, "list") {
                            list_defs.push(list_definition_of(&child));
                        }
                    }
                } else if named == "listoverridetable" {
                    for child in child_groups(&inner) {
                        if let Some(one) = list_override_of(&child) {
                            list_over.push(one);
                        }
                    }
                }
            }
            if ATN_WORDS.contains(&named.as_str()) && !*skip.last().unwrap_or(&false) {
                let mut head = i + 2;
                while head < bytes.len() && matches!(bytes[head], b' ' | b'\r' | b'\n' | b'\\') {
                    head += 1;
                }
                let (had, kids) = starred_body(bytes, head, &named);
                let find = |key: &str| -> Value {
                    match kids.iter().find(|(one, _)| one == key) {
                        Some((_, value)) => json!(value),
                        None => Value::Null,
                    }
                };
                if named == "annotation" {
                    me.annotations.push(json!({
                        "author": match pending_author.take() {
                            Some(one) => json!(one),
                            None => Value::Null,
                        },
                        "text": had,
                        "ref": find("atnref"),
                        "date_written": find("atndate"),
                    }));
                } else if !had.is_empty() {
                    me.annotation_authors += 1;
                    pending_author = Some(had);
                }
            }
            if BK_WORDS.contains(&named.as_str()) && !*skip.last().unwrap_or(&false) {
                let mut head = i + 2;
                // 与批注那一路同一个跳过集：空白与控制字前面那一个反斜杠都要越过去，
                // 停在词的第一个字母上（starred_body 是按「从第一个字母起」切掉词名的）
                while matches!(
                    bytes.get(head),
                    Some(&b' ') | Some(&b'\r') | Some(&b'\n') | Some(&b'\\')
                ) {
                    head += 1;
                }
                let (had, _) = starred_body(bytes, head, &named);
                if named == "bkmkstart" {
                    me.bookmark_starts += 1;
                    if !had.is_empty() {
                        me.bookmarks.push(had);
                    }
                } else {
                    me.bookmark_ends += 1;
                }
            }
            let last = skip.len() - 1;
            skip[last] = true;
            me.skipped_destinations += 1;
            i += 2;
            continue;
        }
        if bytes.get(i + 1) == Some(&b'\'') {
            me.hex_bytes += 1;
            let hi = hex_digit(bytes.get(i + 2).copied().unwrap_or(b'0'));
            let lo = hex_digit(bytes.get(i + 3).copied().unwrap_or(b'0'));
            if !*skip.last().unwrap_or(&false) {
                pending.push(hi * 16 + lo);
            }
            i += 4;
            continue;
        }
        let mut j = i + 1;
        let mut word: Vec<u8> = Vec::new();
        while j < bytes.len() && bytes[j].is_ascii_alphabetic() {
            word.push(bytes[j]);
            j += 1;
        }
        let word = String::from_utf8_lossy(&word).into_owned();
        if word.is_empty() {
            // 单字符转义：\{ \} \( \) 与单独一个反斜杠
            flush(&mut out, &mut pending, codepage, &mut notes);
            if j < bytes.len() {
                if !*skip.last().unwrap_or(&false) {
                    out.push(bytes[j]);
                }
                i = j + 1;
            } else {
                i = j;
            }
            continue;
        }
        let mut digits: Vec<u8> = Vec::new();
        while j < bytes.len() && (bytes[j].is_ascii_digit() || bytes[j] == b'-') {
            digits.push(bytes[j]);
            j += 1;
        }
        let digits = String::from_utf8_lossy(&digits).into_owned();
        if bytes.get(j) == Some(&b' ') {
            j += 1; // 控制字后的那个空格属于控制字，不是正文
        }
        flush(&mut out, &mut pending, codepage, &mut notes);
        let skipping = *skip.last().unwrap_or(&false);
        match word.as_str() {
            "uc" => {
                ucount = digits.parse::<usize>().unwrap_or(1);
                i = j;
                continue;
            }
            "ansicpg" => {
                if let Ok(one) = digits.parse::<u32>() {
                    codepage = one;
                }
                i = j;
                continue;
            }
            "u" => {
                if !digits.is_empty() {
                    if !skipping {
                        match digits.parse::<i64>() {
                            Ok(raw) => {
                                let value = if raw < 0 { 65536 + raw } else { raw };
                                match u32::try_from(value).ok().and_then(char::from_u32) {
                                    Some(one) => push_char(&mut out, one),
                                    None => notes.push(format!("\\u{raw} 不是一个合法的码位")),
                                }
                                me.unicode_escapes += 1;
                            }
                            Err(_) => notes.push(format!("\\u 的数字 {digits} 读不出来")),
                        }
                    }
                    j = skip_rtf_chars(bytes, j, ucount);
                }
                i = j;
                continue;
            }
            _ => {}
        }
        if RUN_WORDS.contains(&word.as_str()) || RUN_DIRECT.contains(&word.as_str()) {
            // 格式控制字写在群头上：落在嵌套群里的那一条归属于那一群的字，落在群外
            // （段前缀那一层）的那一条归属于**段**，不属于任何一串字，所以只数不挂。
            // 实测 LibreOffice 的 RTF 导出给每一段都重发一份样式默认值（`\cf0`、`\fs22`、
            // `\kerning0`…），那些条数全落在 run_words_stray 上，不进这份账本
            if !skipping {
                if skip.len() >= 3 {
                    if let Some(top) = word_stack.last_mut() {
                        top.push((word.clone(), digits.clone()));
                    }
                } else if RUN_WORDS.contains(&word.as_str()) {
                    me.run_words_stray += 1;
                }
            }
        }
        if word == "ftnalt" {
            // 只在「这是注的群」时被读到；判 kind 用
            me.ftnalt = true;
        }
        if !skipping && NOTE_DESTINATIONS.contains(&word.as_str()) {
            // 注住在自己的群里（`\footnote` 或 `{\*\footnote …}`，外层已经由 `\*` 那一支放行）：
            // 整群提出来单独交账，正文里一份不留
            let (stop, inner) = group_end(bytes, j);
            let sub = extract(&inner);
            let kind = if word == "endnote" || sub.ftnalt {
                "endnote"
            } else {
                "footnote"
            };
            for line in sub.lines {
                me.note_list
                    .push(json!({"kind": kind, "slot": word.clone(), "text": line}));
            }
            me.note_destinations += 1;
            // 这一跳吃掉了收尾的那个 `}`，群里层的跳过标记要自己弹掉
            if skip.len() > 1 {
                skip.pop();
                word_stack.pop();
                run_start.pop();
            }
            i = stop + 1;
            continue;
        }
        if !skipping && PAGE_DESTINATIONS.contains(&word.as_str()) {
            // 目标群从这一位起，到关掉「当前这一群」的那个 `}` 止。
            // 里面递归走一遍：页眉也会有 \par、\u 与字段。
            let (stop, inner) = group_end(bytes, j);
            let sub = extract(&inner);
            let slot = word.clone();
            let into: &mut Vec<Value> = if page_kind(word.as_str()) == "header" {
                &mut me.headers
            } else {
                &mut me.footers
            };
            for line in sub.lines {
                into.push(json!({"slot": slot.clone(), "text": line}));
            }
            me.page_destinations += 1;
            // 那个 `}` 被这一跳吃掉了，群里层的跳过标记要自己弹掉
            if skip.len() > 1 {
                skip.pop();
                word_stack.pop();
                run_start.pop();
            }
            i = stop + 1;
            continue;
        }
        if SKIP_DESTINATIONS.contains(&word.as_str())
            || NOTE_DEFINITION_WORDS.contains(&word.as_str())
        {
            if !skipping && DEF_DESTINATIONS.contains(&word.as_str()) {
                // 前瞻：不推进游标、不改这里的 skip —— 那一群照旧整群跳过，
                // 只是里面的 `{\fN …名字;}` / `{\sN …名字;}` / `{\list…}` 别再丢了
                let (_stop, inner) = group_end(bytes, j);
                if word == "listtable" {
                    for child in child_groups(&inner) {
                        if starts_word(&child, "list") {
                            list_defs.push(list_definition_of(&child));
                        }
                    }
                } else if word == "listoverridetable" {
                    for child in child_groups(&inner) {
                        if let Some(one) = list_override_of(&child) {
                            list_over.push(one);
                        }
                    }
                } else {
                    for child in child_groups(&inner) {
                        if let Some(one) = definition_of(&child) {
                            if word == "fonttbl" {
                                me.fonts.push(one);
                            } else {
                                me.styles.push(one);
                            }
                        }
                    }
                }
            }
            if !skipping && word == "colortbl" {
                // 前瞻那一张颜色表（游标不动、skip 不改 —— 那一群照旧整群跳过，
                // 一个字节也不进正文，所以 skipped_destinations 一笔也不动）：
                // 段上那个 `\cfN` 点的就是这里的第 N 格，不解它就只能交一个号
                let (_stop, inner) = group_end(bytes, j);
                me.colors = color_table(&inner);
            }
            let last = skip.len() - 1;
            skip[last] = true;
            me.skipped_destinations += 1;
        } else if word == "listtext" {
            // 前瞻那一句标签：`{\listtext\pard\plain  1.\tab}`。那是生产者**算好之后
            // 写进文件**的号，不是我们数出来的，所以解出来的字（`text`）、它点的字体
            // （`font`）、后面那条 `\tab` 在不在（`tab`）与文件那一串原样（`written`）一起交。
            // 字照旧留在正文里（这一群不跳），所以 lines 与段落数都不因为这个变
            if !skipping {
                me.label_words += 1;
                if para_label.is_none() {
                    // `\listtext` 就是这一群开群之后的第一个控制字，所以**我们已经站在
                    // 群里了**：从 `j` 起到关掉这一群的那个 `}` 就是那一句标签。
                    // （找下一个 `{` 是错的 —— 那会一路读到后面好几个段）
                    let (_stop, inner) = group_end(bytes, j);
                    para_label = Some(json!({
                        "text": extract(&inner).text,
                        "font": word_in_group(&inner, "f").filter(|one| !one.is_empty()),
                        "tab": word_in_group(&inner, "tab").is_some(),
                        "written": String::from_utf8_lossy(&inner).into_owned(),
                    }));
                }
            }
        } else if word == "pict" {
            let last = skip.len() - 1;
            skip[last] = true;
            me.pictures += 1;
            // 前瞻这一群的群头（不推进游标、也不改 skip —— 那串数据照旧一个字节
            // 都不进正文，所以 skipped_destinations 一个也没因为这个改动而变）：
            // 三种单位、格式的两种凭据与那格形状属性都写在数据之前
            if me.picture_rows.len() < PICTURE_ROW_CAP {
                me.picture_rows.push(picture_ledger(bytes, j));
            }
        } else if OBJECT_WORDS.contains(&word.as_str()) {
            let last = skip.len() - 1;
            skip[last] = true;
            me.embedded_objects += 1;
        } else if word == "field" {
            // 前瞻一步：找到紧跟这一群的那个 `{`，从群里读出链接。
            // 不推进游标，也不改 skip —— `\fldrslt` 的显示文字是页面上的字，
            // 而 `{\*\fldinst …}` 那一群照旧由 `\*` 那条规则处理（本域不认识就不进正文）
            me.fields += 1;
            if !skipping {
                let brace = (j..bytes.len()).find(|&k| bytes[k] == b'{');
                if let Some(at) = brace {
                    // 先把这一群量到收尾，再在群内找 —— 不看后面域的字
                    let (_stop, inner) = group_end(bytes, at);
                    if let Some(link) = field_link(&inner) {
                        me.links.push(link);
                    }
                    if let Some(had) = field_instruction(&inner) {
                        me.field_instructions.push(had);
                    }
                }
            }
        } else if !skipping {
            if BREAK_WORDS.contains(&word.as_str()) || ROW_WORDS.contains(&word.as_str()) {
                out.push(b'\n');
                marks.push((para_start, out.len(), para_style));
                flows.push(ParaFlow {
                    ilvl: para_ilvl.take(),
                    ls: para_ls.take(),
                    li: para_li.take(),
                    fi: para_fi.take(),
                    label: para_label.take(),
                });
                para_start = out.len();
                para_style = None;
            } else if TAB_WORDS.contains(&word.as_str()) {
                out.push(b'\t');
            }
            // 表那份账：数的就是这几个控制字本身（在不在跳过区由上面那条顺序决定）。
            // 「几张表」不在这儿 —— 那条推断规则拿两张件的对照试过，判不住
            match word.as_str() {
                "trowd" => me.table_row_defines += 1,
                "row" => me.table_rows += 1,
                "cell" => me.table_cells += 1,
                "intbl" => me.table_cell_paras += 1,
                "nestrow" => me.nested_table_rows += 1,
                "nestcell" => me.nested_table_cells += 1,
                _ => {}
            }
            // 断点词：这六个各数各的（合不合是调用方的事，这里只交文件写了几条）
            if let Some(which) = break_word_index(word.as_str()) {
                me.break_words[which] += 1;
            }
            // 制表位这一族：`\tx` 是一个位置（没带数字那一形是「清掉一个位置」），它前面那两个
            // 前缀只管紧跟的这一个；`\tab` 是段里的制表**字符** —— 与「定义了哪几个位置」两本账
            if word == "tx" {
                me.tab_rows.push(json!({
                    "position_written": (!digits.is_empty()).then(|| digits.clone()),
                    "align_written": tab_align.take(),
                    "leader_written": tab_leader.take(),
                }));
            } else if TAB_ALIGN_WORDS.contains(&word.as_str()) {
                me.tab_align_words += 1;
                tab_align = Some(word.clone());
            } else if TAB_LEADER_WORDS.contains(&word.as_str()) {
                me.tab_leader_words += 1;
                tab_leader = Some(word.clone());
            }
            if word == "tab" {
                me.tab_chars += 1;
            }
            // 样式被用了几次：正文里的 `\sN`（数字参数就在 digits 里）。
            // 同一处也记下「这一段现在用的是哪个样式」—— 段属性就在收尾之前
            if word == "s" {
                if let Some(index) = digits_after(digits.as_bytes(), 0) {
                    uses.push(index);
                    para_style = Some(index);
                }
            }
            // 段自己说过的号：`\ilvl` 与 `\ls`（号本在 listoverridetable 那一头，
            // 这里只记文件写在段上的那两串，不拿号当号用）。`\li` / `\fi` 也照最后
            // 写下的那个收 —— 这一族段上写了一份，列表定义里另有一份
            if word == "ilvl" && !digits.is_empty() {
                para_ilvl = Some(digits.clone());
            } else if word == "ls" && !digits.is_empty() {
                para_ls = Some(digits.clone());
            } else if word == "li" && !digits.is_empty() {
                para_li = Some(digits.clone());
            } else if word == "fi" && !digits.is_empty() {
                para_fi = Some(digits.clone());
            }
            // 那张纸写在文档级的属性里。每个词只记第一次写的，而且只看没被跳过的那一层 ——
            // 后面 `{\*\sectx …}` 里的那些是某一节的覆写，`\header` 那种已知目标群整个另读，
            // 都不算文档默认值。`landscape` 没有数字参数，记 "1" 表示「写了」
            if PAPER_WORDS.contains(&word.as_str())
                && !paper.iter().any(|(one, _)| one == &word)
                && (word == "landscape" || !digits.is_empty())
            {
                let value = if word == "landscape" {
                    "1".to_string()
                } else {
                    digits.clone()
                };
                paper.push((word, value));
            }
        }
        i = j;
    }
    flush(&mut out, &mut pending, codepage, &mut notes);
    me.declared_codepage = codepage;
    me.notes = notes;
    me.paper_writes = paper;
    // 用了几次的样式按名字合：文件写的是 `\s1`，名字在样式表那一群里
    uses.sort_unstable();
    let mut seen: Vec<(u64, usize)> = Vec::new();
    for one in &uses {
        match seen.last_mut() {
            Some(last) if last.0 == *one => last.1 += 1,
            _ => seen.push((*one, 1)),
        }
    }
    let resolved: Vec<Value> = seen
        .iter()
        .map(|(index, count)| {
            let named = me
                .styles
                .iter()
                .find(|one| one["kind"] == json!("paragraph") && one["index"] == json!(*index));
            // 用了却没定义的样式号也照交：名字给 `sN` 这种占位，不替文件编一个
            let name = match named.and_then(|one| one["name"].as_str()) {
                Some(text) => text.to_string(),
                None => format!("s{}", index),
            };
            json!({"index": index, "name": name, "count": count})
        })
        .collect();
    me.style_uses = resolved;
    // 列表那份账：段上的 `\ls` → `listoverridetable` 里那一条 → 它点名的 `\listid` →
    // `listtable` 里那一份定义 → 定义里第 `\ilvl` 个 `{\listlevel`。
    // 四步各自一个布尔，指不到就交到那一步为止，不拿邻居的数顶上
    let name_of = |index: Option<u64>| -> Option<String> {
        let want = index?;
        let named = me
            .styles
            .iter()
            .find(|one| one["kind"] == json!("paragraph") && one["index"] == json!(want));
        named
            .and_then(|one| one["name"].as_str())
            .map(|one| one.to_string())
    };
    let mut entries: Vec<Value> = Vec::new();
    let mut checked = 0usize;
    let mut with_ilvl = 0usize;
    let mut with_ls = 0usize;
    let mut with_label = 0usize;
    let mut override_found = 0usize;
    let mut definition_found = 0usize;
    let mut level_found = 0usize;
    for (at, (start, end, style)) in marks.iter().enumerate() {
        let had = match flows.get(at) {
            Some(one) => one,
            None => continue,
        };
        checked += 1;
        if had.ilvl.is_none() && had.ls.is_none() && had.label.is_none() {
            continue;
        }
        with_ilvl += usize::from(had.ilvl.is_some());
        with_ls += usize::from(had.ls.is_some());
        with_label += usize::from(had.label.is_some());
        let over = had.ls.as_ref().and_then(|want| {
            list_over
                .iter()
                .find(|one| one["ls"].as_str() == Some(want.as_str()))
        });
        let list_id = over.and_then(|one| one["list_id"].as_str());
        let held = list_id.and_then(|want| {
            list_defs
                .iter()
                .find(|one| one["list_id"].as_str() == Some(want))
        });
        let level = match (
            held,
            had.ilvl.as_ref().and_then(|raw| raw.parse::<usize>().ok()),
        ) {
            (Some(one), Some(want)) => one["list_level"].get(want),
            _ => None,
        };
        override_found += usize::from(over.is_some());
        definition_found += usize::from(held.is_some());
        level_found += usize::from(level.is_some());
        let said = out
            .get(*start..*end)
            .map(|raw| String::from_utf8_lossy(raw).trim().to_string())
            .unwrap_or_default();
        entries.push(json!({
            "at": at,
            "text": said,
            "style_index": style.clone(),
            "style_name": name_of(*style),
            "ilvl": had.ilvl.clone(),
            "ls": had.ls.clone(),
            "indent": {"li": had.li.clone(), "fi": had.fi.clone()},
            "override_found": over.is_some(),
            "list_id": list_id.map(|one| one.to_string()),
            "template_id": held
                .and_then(|one| one["template_id"].as_str())
                .map(String::from),
            "definition_found": held.is_some(),
            "level_found": level.is_some(),
            "level": level.cloned(),
            "label": had
                .label
                .as_ref()
                .and_then(|one| one["text"].as_str())
                .map(String::from),
            "label_font": had
                .label
                .as_ref()
                .and_then(|one| one["font"].as_str())
                .map(String::from),
            "label_tab": had
                .label
                .as_ref()
                .map(|one| one["tab"] == json!(true))
                .unwrap_or(false),
            "label_written": had
                .label
                .as_ref()
                .and_then(|one| one["written"].as_str())
                .map(String::from),
        }));
    }
    let listed = entries.len();
    let want_used = |want: &str| -> usize {
        entries
            .iter()
            .filter(|had| had["list_id"].as_str() == Some(want))
            .count()
    };
    let defs: Vec<Value> = list_defs
        .iter()
        .enumerate()
        .map(|(at, one)| {
            json!({
                "at": at,
                "list_id": one["list_id"],
                "template_id": one["template_id"],
                "levels": one["levels"],
                "nfc": one["nfc"],
                "used_by": want_used(one["list_id"].as_str().unwrap_or_default()),
            })
        })
        .collect();
    let levels_all: usize = list_defs
        .iter()
        .map(|one| one["levels"].as_u64().unwrap_or(0) as usize)
        .sum();
    // `checked` 就是「这一族按 par / row 切出来看了几段」—— 表里的 `at` 是这个序号，
    // 与 `structure.paragraphs`（去掉空段的那本）是两个数，所以两个都在表上
    me.numbering = json!({
        "checked": checked,
        "listed": listed,
        "with_ilvl": with_ilvl,
        "with_ls": with_ls,
        "with_label": with_label,
        "label_words": me.label_words,
        "list_definitions": list_defs.len(),
        "overrides": list_over.len(),
        // 号本也整份交（与 docx 那一份 `definitions` 是同一类东西：段的号先落在这里）
        "override_list": list_over.clone(),
        "levels": levels_all,
        "override_found": override_found,
        "definition_found": definition_found,
        "level_found": level_found,
        "resolved": definition_found,
        "definitions": defs,
        "list": entries,
    });
    // 标题：样式名写成 `heading N` 的那些段。层级不是猜出来的 —— 某一段用的是哪个样式号
    // 写在段属性里，那个号叫什么名字写在样式表里，两头都在文件上。
    // 用了却没定义的号（样式表里查不到）不算标题，也不给它编一个名字
    let headings: Vec<Value> = marks
        .iter()
        .filter_map(|(start, end, style)| {
            let level = heading_level(&me.styles, (*style)?)?;
            let text = String::from_utf8_lossy(out.get(*start..*end)?)
                .trim()
                .to_string();
            if text.is_empty() {
                return None;
            }
            Some(json!({"level": level, "text": text}))
        })
        .collect();
    me.headings = headings;
    let text = String::from_utf8_lossy(&out).into_owned();
    let text = text.trim().to_string();
    me.lines = text
        .lines()
        .map(|one| one.trim().to_string())
        .filter(|one| !one.is_empty())
        .collect();
    // 号 → 那两张表：走完整条流才跳，所以表写在前面还是后面都不影响能不能跳通
    let colors = me.colors.clone();
    let fonts = me.fonts.clone();
    let styles = me.styles.clone();
    resolve_runs(&mut me.run_rows, &colors, &fonts, &styles);
    me.text = text;
    me
}

fn push_char(out: &mut Vec<u8>, one: char) {
    let mut buf = [0u8; 4];
    out.extend_from_slice(one.encode_utf8(&mut buf).as_bytes());
}

/// 从 `at` 起丢掉 `how_many` 个 **RTF 字符**：`\'hh` 是一个，控制字连同数字参数也是一个
fn skip_rtf_chars(bytes: &[u8], mut at: usize, how_many: usize) -> usize {
    while at < bytes.len() && matches!(bytes[at], b'\r' | b'\n') {
        at += 1;
    }
    let mut left = how_many;
    while left > 0 && at < bytes.len() {
        if bytes[at] == b'\\' && bytes.get(at + 1) == Some(&b'\'') {
            at += 4;
        } else if bytes[at] == b'\\'
            && bytes
                .get(at + 1)
                .is_some_and(|one| one.is_ascii_alphabetic())
        {
            at += 1;
            while at < bytes.len() && bytes[at].is_ascii_alphabetic() {
                at += 1;
            }
            while at < bytes.len() && (bytes[at].is_ascii_digit() || bytes[at] == b'-') {
                at += 1;
            }
            if bytes.get(at) == Some(&b' ') {
                at += 1;
            }
        } else {
            at += 1;
        }
        left -= 1;
    }
    at
}

fn hex_digit(one: u8) -> u8 {
    match one {
        b'0'..=b'9' => one - b'0',
        b'a'..=b'f' => one - b'a' + 10,
        b'A'..=b'F' => one - b'A' + 10,
        _ => 0,
    }
}

/// 一串 `\'hh` 字节按声明的字符集解释，然后清空缓冲
fn flush(out: &mut Vec<u8>, pending: &mut Vec<u8>, codepage: u32, notes: &mut Vec<String>) {
    if pending.is_empty() {
        return;
    }
    if codepage == 65001 {
        match std::str::from_utf8(pending) {
            Ok(text) => out.extend_from_slice(text.as_bytes()),
            Err(_) => {
                notes.push("声明是 UTF-8 但字节解不过去，按 lossy 给出".to_string());
                out.extend_from_slice(String::from_utf8_lossy(pending).as_bytes());
            }
        }
    } else if codepage == 1252 || codepage == 0 {
        // cp1252 与 latin-1 只在 0x80-0x9F 一段不同，办公文件的属性与正文里
        // 走这一段的是少数；真出现时下面那档会明确说用的是哪档
        for one in pending.iter() {
            push_char(out, char::from(*one));
        }
    } else {
        notes.push(format!(
            "字符集 {codepage} 本版本不内置，\'hh 字节按 latin-1 逐字节给出"
        ));
        for one in pending.iter() {
            push_char(out, char::from(*one));
        }
    }
    pending.clear();
}

/// `\info` 一带的元数据：键由控制字自己说，值在它所引导的那一群里。
///
/// 三条判据都是从真件上踩出来的（`notes.rtf`，LibreOffice 由 python-docx 的 docx 转出）：
/// * `\upr{A}{B}` 的第一群是 7 位回退文本，这份文件在那儿只留下 `?`，真值在第二群
///   `\*\ud{...}` 里 —— 正文抽取把 `\*` 群整群跳过是对的，读元数据时照搬就会丢掉标题。
///   所以这里只跳 `\upr` 的第一群，`\*` 反而不跳。
/// * `\info{}` 在这份文件里只包住了标题那一对 `\upr`，其余键（subject / keywords /
///   doccomm / author / creatim / userprops）紧跟在同一层。只认「第一个群」会漏掉九成
///   元数据，所以扫到下一个**非元数据目标**（`\stylesheet` 那类）为止，并有硬上界。
/// * 文字要**按群整块交账**：逐字交账会把 `AppVersion` 变成十条自定义属性。
#[derive(Debug, Clone)]
pub struct Prop {
    pub name: String,
    pub kind: Option<i64>,
    pub value: Option<String>,
}

#[derive(Debug, Default)]
pub struct Info {
    pub found: bool,
    pub fields: std::collections::BTreeMap<String, String>,
    pub props: Vec<Prop>,
    pub notes: Vec<String>,
    pub codepage: u32,
}

impl Info {
    pub fn to_json(&self) -> Value {
        json!({
            "found": self.found,
            "codepage": self.codepage,
            "fields": self.fields,
            "user_props": self.props.iter().map(|one| json!({
                "name": one.name, "type": one.kind, "value": one.value,
            })).collect::<Vec<Value>>(),
            "notes": self.notes,
        })
    }
}

const INFO_TEXT_KEYS: &[(&str, &str)] = &[
    ("title", "title"),
    ("subject", "subject"),
    ("author", "author"),
    ("operator", "operator"),
    ("keywords", "keywords"),
    ("doccomm", "comment"),
    ("comments", "comment"),
    ("lastsavedby", "last_saved_by"),
    ("nchars", "chars"),
    ("nwords", "words"),
    ("npages", "pages"),
    ("nparas", "paragraphs"),
    ("nlines", "lines"),
    ("version", "version"),
    ("category", "category"),
    ("manager", "manager"),
    ("company", "company"),
    ("propname", "propname"),
    ("staticval", "staticval"),
];

const INFO_TIME_KEYS: &[(&str, &str)] = &[
    ("creatim", "created"),
    ("revtim", "modified"),
    ("printim", "printed"),
];

const INFO_TIME_PARTS: &[&str] = &["yr", "mo", "dy", "hr", "min", "sec"];

const INFO_STOP: &[&str] = &[
    "stylesheet",
    "fonttbl",
    "colortbl",
    "generator",
    "listtable",
    "listoverridetable",
    "themedata",
    "colorschememapping",
    "datastore",
    "rsidtbl",
    "xmlnstbl",
    "filetbl",
    "header",
    "footer",
    "pict",
    "object",
    "sect",
    "latentstyles",
    "bkmkstart",
    "atncluster",
    "mmathPr",
];

const INFO_SCAN_CAP: usize = 65536;

/// 找一个作为**完整控制字**出现的目标（`\info` 不算 `\information` 的一部分）
fn find_control(bytes: &[u8], name: &[u8]) -> Option<usize> {
    let mut at = 0usize;
    while let Some(found) = windows_position(bytes, at, name) {
        if found >= 1
            && bytes[found - 1] == b'\\'
            && !bytes
                .get(found + name.len())
                .is_some_and(|one| one.is_ascii_alphabetic())
        {
            return Some(found - 1);
        }
        at = found + 1;
    }
    None
}

fn windows_position(bytes: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    bytes
        .get(from..)?
        .windows(needle.len())
        .position(|one| one == needle)
        .map(|one| one + from)
}

fn key_for(word: &str) -> Option<&'static str> {
    INFO_TEXT_KEYS
        .iter()
        .find(|(one, _)| *one == word)
        .map(|(_, key)| *key)
}

fn time_key(word: &str) -> Option<&'static str> {
    INFO_TIME_KEYS
        .iter()
        .find(|(one, _)| *one == word)
        .map(|(_, key)| *key)
}

/// `\ansicpg` 声明的字符集：取最后一次声明，跟 `extract` 的走法一致
fn declared_codepage(head: &[u8]) -> u32 {
    let mut found: Option<usize> = None;
    let mut at = 0usize;
    while let Some(hit) = windows_position(head, at, b"ansicpg") {
        if hit >= 1 && head[hit - 1] == b'\\' {
            found = Some(hit + b"ansicpg".len());
        }
        at = hit + 1;
    }
    let Some(mut i) = found else { return 1252 };
    let mut digits: String = String::new();
    while i < head.len() && head[i].is_ascii_digit() {
        digits.push(head[i] as char);
        i += 1;
    }
    digits.parse::<u32>().unwrap_or(1252)
}

struct Frame {
    key: Option<&'static str>,
    skip: bool,
    buf: Vec<u8>,
}

/// 读出 `\info` 一带的元数据；没有 `\info` 时返回 `found: false`，不编字段
pub fn parse_info(bytes: &[u8]) -> Info {
    let Some(start) = find_control(bytes, b"info") else {
        let mut info = Info::default();
        info.notes.push("这份 RTF 没有 \\info 群".to_string());
        return info;
    };
    let head = bytes.get(..start).unwrap_or_default();
    let codepage = declared_codepage(head);
    let mut info = Info {
        found: true,
        fields: std::collections::BTreeMap::new(),
        props: Vec::new(),
        notes: Vec::new(),
        codepage,
    };
    let mut stack: Vec<Frame> = vec![Frame {
        key: None,
        skip: false,
        buf: Vec::new(),
    }];
    let mut next_key: Option<&'static str> = None;
    let mut skip_next = false;
    let mut current_time: Option<&'static str> = None;
    let mut times: std::collections::BTreeMap<String, [u32; 6]> = std::collections::BTreeMap::new();
    let mut pending: Vec<u8> = Vec::new();
    let mut ucount = 1usize;
    let end = (start + INFO_SCAN_CAP).min(bytes.len());
    let mut i = start;

    while i < end {
        let ch = bytes[i];
        if ch == b'{' {
            flush_into(&mut stack, &mut pending, codepage, &mut info.notes);
            let parent = stack.last().and_then(|one| one.key);
            let key = next_key.or(parent);
            stack.push(Frame {
                key,
                skip: skip_next,
                buf: Vec::new(),
            });
            next_key = None;
            skip_next = false;
            i += 1;
            continue;
        }
        if ch == b'}' {
            flush_into(&mut stack, &mut pending, codepage, &mut info.notes);
            if stack.len() > 1 {
                let frame = stack.pop();
                if let Some(frame) = frame {
                    commit(frame, &mut info);
                }
            }
            i += 1;
            continue;
        }
        if ch == b'\r' || ch == b'\n' {
            i += 1;
            continue;
        }
        if ch != b'\\' {
            flush_into(&mut stack, &mut pending, codepage, &mut info.notes);
            if let Some(frame) = stack.last_mut() {
                if frame.key.is_some() && !frame.skip {
                    push_char(&mut frame.buf, ch as char);
                }
            }
            i += 1;
            continue;
        }
        if bytes.get(i + 1) == Some(&b'*') {
            i += 2;
            continue;
        }
        if bytes.get(i + 1) == Some(&b'\'') {
            if let Some(frame) = stack.last() {
                if frame.key.is_some() && !frame.skip {
                    let high = bytes.get(i + 2).copied().unwrap_or(b'0');
                    let low = bytes.get(i + 3).copied().unwrap_or(b'0');
                    pending.push(hex_digit(high) * 16 + hex_digit(low));
                }
            }
            i += 4;
            continue;
        }
        let mut j = i + 1;
        let mut word: Vec<u8> = Vec::new();
        while j < bytes.len() && bytes[j].is_ascii_alphabetic() {
            word.push(bytes[j]);
            j += 1;
        }
        let word = String::from_utf8_lossy(&word).into_owned();
        if word.is_empty() {
            flush_into(&mut stack, &mut pending, codepage, &mut info.notes);
            if let Some(frame) = stack.last_mut() {
                if frame.key.is_some() && !frame.skip {
                    if let Some(one) = bytes.get(j) {
                        frame.buf.push(*one);
                    }
                }
            }
            i = if j < bytes.len() { j + 1 } else { j };
            continue;
        }
        let mut digits: Vec<u8> = Vec::new();
        while j < bytes.len() && (bytes[j].is_ascii_digit() || bytes[j] == b'-') {
            digits.push(bytes[j]);
            j += 1;
        }
        if bytes.get(j) == Some(&b' ') {
            j += 1;
        }
        flush_into(&mut stack, &mut pending, codepage, &mut info.notes);
        let digits = String::from_utf8_lossy(&digits).into_owned();
        if word == "uc" {
            ucount = digits.parse::<usize>().unwrap_or(1);
        } else if word == "u" && !digits.is_empty() {
            let raw = digits.parse::<i64>().unwrap_or(0);
            let value = if raw < 0 { raw + 65536 } else { raw };
            if let Some(frame) = stack.last_mut() {
                if frame.key.is_some() && !frame.skip {
                    if let Some(one) = u32::try_from(value).ok().and_then(char::from_u32) {
                        push_char(&mut frame.buf, one);
                    } else {
                        info.notes.push(format!("\\u{digits} 这个数不是有效码位"));
                    }
                }
            }
            i = skip_rtf_chars(bytes, j, ucount);
            continue;
        } else if let Some(key) = key_for(&word) {
            // 这类控制字出现在它自己那一群的开头（{\title 文字}），定的是当前这一帧的键
            let frame_is_open = stack.last().map(|one| one.key.is_none()).unwrap_or(false);
            if frame_is_open {
                if let Some(frame) = stack.last_mut() {
                    frame.key = Some(key);
                }
            } else {
                next_key = Some(key);
            }
            if key == "version" && !digits.is_empty() {
                info.fields.insert("version".to_string(), digits.clone());
            }
        } else if let Some(key) = time_key(&word) {
            current_time = Some(key);
            times.insert(key.to_string(), [0u32; 6]);
        } else if INFO_TIME_PARTS.contains(&word.as_str()) && current_time.is_some() {
            let slot = match word.as_str() {
                "yr" => 0,
                "mo" => 1,
                "dy" => 2,
                "hr" => 3,
                "min" => 4,
                _ => 5,
            };
            let name = current_time.unwrap_or_default().to_string();
            if let Some(one) = times.get_mut(&name) {
                one[slot] = digits.parse::<u32>().unwrap_or(0);
            }
        } else if word == "proptype" {
            if !digits.is_empty() {
                if let Some(last) = info.props.last_mut() {
                    last.kind = digits.parse::<i64>().ok();
                }
            }
        } else if word == "upr" {
            skip_next = true;
        } else if INFO_STOP.contains(&word.as_str()) {
            break;
        }
        i = j;
    }
    flush_into(&mut stack, &mut pending, codepage, &mut info.notes);
    if let Some(frame) = stack.pop() {
        commit(frame, &mut info);
    }
    for (name, stamp) in &times {
        if stamp[0] == 0 {
            info.notes
                .push(format!("{name} 在文件里是全零（等于没写这个时间）"));
            continue;
        }
        info.fields.insert(
            name.clone(),
            format!(
                "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
                stamp[0], stamp[1], stamp[2], stamp[3], stamp[4], stamp[5]
            ),
        );
    }
    info
}

fn flush_into(
    stack: &mut Vec<Frame>,
    pending: &mut Vec<u8>,
    codepage: u32,
    notes: &mut Vec<String>,
) {
    if pending.is_empty() {
        return;
    }
    let mut out: Vec<u8> = Vec::new();
    flush(&mut out, pending, codepage, notes);
    if let Some(frame) = stack.last_mut() {
        frame.buf.extend_from_slice(&out);
    }
}

fn commit(frame: Frame, info: &mut Info) {
    let Some(key) = frame.key else { return };
    if frame.buf.is_empty() {
        return;
    }
    let text = String::from_utf8_lossy(&frame.buf).into_owned();
    match key {
        "propname" => info.props.push(Prop {
            name: text,
            kind: None,
            value: None,
        }),
        "staticval" => {
            if let Some(last) = info.props.last_mut() {
                let joined = match last.value.take() {
                    Some(had) => had + &text,
                    None => text,
                };
                last.value = Some(joined);
            }
        }
        "created" | "modified" | "printed" => {}
        other => {
            let joined = match info.fields.get(other) {
                Some(had) => had.clone() + &text,
                None => text,
            };
            info.fields.insert(other.to_string(), joined);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(name: &str) -> Vec<u8> {
        std::fs::read(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/office")
                .join(name),
        )
        .expect("读 fixture")
    }

    fn rtf(input: &str) -> Rtf {
        extract(input.as_bytes())
    }

    /// `\*` 修饰的是**紧跟它的那个群**：认识的（脚注、尾注、页眉）就不跳。
    /// LibreOffice 把脚注与尾注都写进 `\footnote` 口袋，尾注只多一个 `\ftnalt`；
    /// 注的排版定义（`\ftnsep` 那一族）不是一条注
    #[test]
    fn star_destinations_are_read_when_the_domain_knows_them() {
        let one = rtf(
            "{\\rtf1\\ansi 正文{\\super \\chftn{\\*\\footnote \\chftn 脚注的字。}}{\\super \\chftn{\\*\\footnote\\ftnalt \\chftn 尾注的字。}}}",
        );
        assert_eq!(one.lines, vec!["正文".to_string()], "{:?}", one.lines);
        let kinds: Vec<&str> = one
            .note_list
            .iter()
            .map(|had| had["kind"].as_str().unwrap_or_default())
            .collect();
        assert_eq!(kinds, ["footnote", "endnote"], "{:?}", one.note_list);
        assert_eq!(one.note_destinations, 2);
        // 分隔符定义里没有「一条注」
        let sep = rtf("{\\rtf1\\ansi 正文{\\*\\ftnsep\\chftnsep}{\\*\\ftncn\\chftncn}}");
        assert!(sep.note_list.is_empty(), "{:?}", sep.note_list);
        assert_eq!(sep.lines, vec!["正文".to_string()], "{:?}", sep.lines);
        // 真件：一份 LibreOffice 写的 RTF，两条脚注一条尾注
        let real = extract(&fixture("notes-end.rtf"));
        assert_eq!(real.note_destinations, 3, "{:?}", real.note_list);
        assert_eq!(
            real.note_list
                .iter()
                .filter(|had| had["kind"] == json!("endnote"))
                .count(),
            1,
            "{:?}",
            real.note_list
        );
        assert!(
            !real.text.contains("gross"),
            "注的字混进正文了：{}",
            real.text
        );
    }

    /// 词边界：`\pard` 不是 `\par` 加一个字面 `d`
    #[test]
    fn control_words_end_at_a_word_boundary() {
        let one = rtf("{\\pard\\plain \\rtlpar Hello\\par}");
        assert_eq!(one.text, "Hello");
        assert!(!one.text.contains('d'), "{}", one.text);
    }

    /// 目标群里的东西不是正文
    #[test]
    fn destinations_are_not_body_text() {
        let one = rtf("{\\rtf1\\ansi{\\fonttbl{\\f0 Cambria;}}{\\colortbl;\\red0\\green0;}正文}");
        assert_eq!(one.text, "正文");
        assert!(
            one.skipped_destinations >= 2,
            "{}",
            one.skipped_destinations
        );
    }

    /// `\*` 的语义：不认识就整群跳过 —— 域指令原文与用户属性都从这里过
    #[test]
    fn starred_groups_are_skipped() {
        let one = rtf("{\\rtf1{\\*\\fldinst HYPERLINK \"https://example.com\"}{\\fldrslt 链接}{\\*\\userprops{\\propname AppVersion}}完}");
        assert_eq!(one.text, "链接\n完".replace('\n', ""), "域指令文本不许出现");
    }

    /// 断点词按**词边界**数：`\pard` 不是 `\par`，`\sectd` 不是 `\sect`。
    /// 顺序照 `BREAK_WORD_NAMES`：par / line / page / pagebb / pbb / sect
    #[test]
    fn break_words_count_at_word_boundaries_only() {
        let one = rtf("{\\rtf1\\pard\\plain 第一段\\pagebb 第二段\\par 第三\\sectd 段\\par}");
        assert_eq!(one.break_words, [2, 0, 0, 1, 0, 0], "{:?}", one.break_words);
        // 真件：Word 那条 `w:br w:type="page"` 在 LibreOffice 的 RTF 导出里是 `\pagebb`，
        // 整份文件一个 `\page` 都没有 —— 子串数会把这条换页算成 `\page`，也可能反过来
        let real = extract(&fixture("notes.rtf"));
        assert_eq!(
            real.break_words,
            [7, 0, 0, 1, 0, 0],
            "{:?}",
            real.break_words
        );
        let hf = extract(&fixture("notes-hf.rtf"));
        assert_eq!(hf.break_words, [4, 0, 0, 1, 0, 1], "{:?}", hf.break_words);
        // 只在没被跳过的那一层数：页眉里那些 `\par` 不是正文的一段
        let bytes = fixture("notes-hf.rtf");
        let seen = bytes.windows(4).filter(|one| *one == *b"\\par").count();
        assert!(seen > 8, "这份件的跳过区里还有一把 \\par：{seen}");
        assert_eq!(hf.lines.len(), 3, "{:?}", hf.lines);
    }

    /// 批注住在一个星号群里，而作者写在注的**前面那一格**：两条列表按文件的顺序配，
    /// 配不上就交 null。`{\*\atndate …}` 那串两个样本都对不上同一批字的 docx 里的
    /// `w:date`，所以这里只交原样（`date_written`），解不动的事由调用方交 null
    #[test]
    fn annotations_come_from_the_starred_group_with_their_own_author() {
        let one = rtf(
            "{\\rtf1正文{{\\*\\atnauthor 张三}\\chatn{\\*\\annotation{\\*\\atnref 0}{\\*\\atndate 123}注的字。}}}",
        );
        assert_eq!(one.text, "正文", "注的字不许进正文：{}", one.text);
        assert_eq!(one.annotations.len(), 1, "{:?}", one.annotations);
        assert_eq!(
            one.annotations[0]["author"],
            json!("张三"),
            "{:?}",
            one.annotations
        );
        assert_eq!(
            one.annotations[0]["text"], "注的字。",
            "{:?}",
            one.annotations
        );
        assert_eq!(
            one.annotations[0]["ref"],
            json!("0"),
            "{:?}",
            one.annotations
        );
        assert_eq!(
            one.annotations[0]["date_written"],
            json!("123"),
            "{:?}",
            one.annotations
        );
        assert_eq!(one.annotation_authors, 1);
        // 没有作者的注：交 null，不拿别处的名字顶上去；群里没写 atnref / atndate
        // 也各自 null，不补 0
        let bare = rtf("{\\rtf1{\\*\\annotation 只有注}尾}");
        assert_eq!(bare.text, "尾", "{:?}", bare.text);
        assert_eq!(bare.annotations.len(), 1, "{:?}", bare.annotations);
        assert!(
            bare.annotations[0]["author"].is_null(),
            "{:?}",
            bare.annotations
        );
        assert!(
            bare.annotations[0]["ref"].is_null(),
            "{:?}",
            bare.annotations
        );
        assert!(
            bare.annotations[0]["date_written"].is_null(),
            "{:?}",
            bare.annotations
        );
        assert_eq!(bare.annotation_authors, 0);
        // 真件：两条注。第二条的作者中文名 LibreOffice 在 RTF 里写不出来（两个问号），
        // 而它自己的 docx 导出把「刘奇」照抄 —— 那一份差在 office_doc 的探针里钉住
        let real = extract(&fixture("comments.rtf"));
        let authors: Vec<Value> = real
            .annotations
            .iter()
            .map(|one| one["author"].clone())
            .collect();
        assert_eq!(
            authors,
            vec![json!("liuqi"), json!("??")],
            "{:?}",
            real.annotations
        );
        assert_eq!(real.annotation_authors, 2, "{:?}", real.annotations);
        assert_eq!(
            real.annotations
                .iter()
                .map(|one| one["text"].as_str().unwrap_or_default())
                .collect::<Vec<&str>>(),
            vec![
                "这里要补上不含税口径",
                "这个数要找财务确认一下，第二行接着写"
            ],
            "{:?}",
            real.annotations
        );
        assert_eq!(
            real.annotations
                .iter()
                .map(|one| one["ref"].clone())
                .collect::<Vec<Value>>(),
            vec![json!("0"), json!("1")],
            "注自己的号与锚区两头是同一个数：{:?}",
            real.annotations
        );
        // 注的字一份都不许落到正文里
        assert_eq!(
            real.lines,
            vec![
                "第一段：不含税口径".to_string(),
                "第二段：金额待确认".to_string(),
                "第三段：这一段没有批注".to_string(),
            ],
            "{:?}",
            real.lines
        );
    }

    /// 域指令原文要**解掉那一双反斜杠**再交：文件里 `\\o` 是两个字节（单反斜杠会开出
    /// 一个控制字），解完才是指令本身 `\o` —— 与 docx 的 `w:instrText` 逐字同一个形状。
    /// 没有 `\*\fldinst` 的域只算条数，不替它编一条指令
    #[test]
    fn field_instructions_unescape_their_backslashes() {
        let one = rtf(
            "{\\rtf1{\\field{\\*\\fldinst { TOC \\\\o \"1-2\" \\\\h}}{\\fldrslt {目录的字}}}完}",
        );
        assert_eq!(
            one.field_instructions,
            vec!["TOC \\o \"1-2\" \\h".to_string()],
            "{:?}",
            one.field_instructions
        );
        assert_eq!(one.fields, 1);
        assert_eq!(one.text, "目录的字完", "指令原文不许进正文：{:?}", one.text);
        // 链接的指令也在同一本账上（这一族不止认 TOC）
        let link = rtf(
            "{\\rtf1{\\field{\\*\\fldinst HYPERLINK \"https://example.com\"}{\\fldrslt 链接}}}",
        );
        assert_eq!(
            link.field_instructions,
            vec!["HYPERLINK \"https://example.com\"".to_string()],
            "{:?}",
            link.field_instructions
        );
        assert_eq!(link.links.len(), 1, "{:?}", link.links);
        // 有域没指令：条数照记，指令表留空
        let bare = rtf("{\\rtf1{\\field{\\fldrslt 只有结果}}}");
        assert_eq!(bare.fields, 1);
        assert!(
            bare.field_instructions.is_empty(),
            "{:?}",
            bare.field_instructions
        );
        // 真件：LibreOffice 从 toc.docx 转出来的那一份，两条域一条 TOC 一条 HYPERLINK
        let real = extract(&fixture("toc.rtf"));
        assert_eq!(real.fields, 2, "{:?}", real.field_instructions);
        assert_eq!(
            real.field_instructions,
            vec![
                "TOC \\o \"1-2\" \\h".to_string(),
                "HYPERLINK \"https://example.com/budget\"".to_string(),
            ],
            "{:?}",
            real.field_instructions
        );
        // 指令里不许有换行/制表：那说明群切错了，把正文卷了进来
        for had in &real.field_instructions {
            assert!(!had.contains('\n') && !had.contains('\t'), "{had:?}");
            assert!(!had.contains("\\\\o"), "{had:?} 双反斜杠没解掉");
        }
    }

    /// `\uN` 之后的回退字节要按 `\ucN` 丢，且 `\'hh` 算一个字符
    #[test]
    fn unicode_escapes_drop_their_fallback() {
        let one = rtf("{\\uc1\\u20013\\'39\\u22826\\'41 x}");
        assert_eq!(
            one.text, "中太 x",
            "控制字后的那个空格属于控制字，正文里的空格要留下"
        );
        assert_eq!(one.unicode_escapes, 2);
        assert_eq!(one.hex_bytes, 0, "被丢掉的回退字节不该计数成 hex");
        // `\uc0` 说的是「一个回退字符都不跳」：那个 ? 就是正文，不是回退垃圾
        let zero = rtf("{\\uc0\\u20013?}");
        assert_eq!(
            zero.text, "中?",
            "\\uc0 被当成默认的 1 就会吞掉正文里的问号"
        );
    }

    /// 连续 `\'hh` 攒起来按文件声明的字符集解：UTF-8 那档能还原中文
    #[test]
    fn consecutive_hex_bytes_decode_with_the_declared_codepage() {
        // “中” 的 UTF-8 三字节：E4 B8 AD
        let one = rtf("{\\ansicpg65001\\'e4\\'b8\\'ad}");
        assert_eq!(one.declared_codepage, 65001);
        assert_eq!(one.text, "中");
        assert_eq!(one.hex_bytes, 3);
    }

    /// 内置之外的字符集不许假装成功：给 latin-1 并把用的哪档说出来
    #[test]
    fn an_unsupported_codepage_says_so_instead_of_guessing() {
        let one = rtf("{\\ansicpg936\\'a1\\'a2}");
        assert!(!one.notes.is_empty(), "936 要留下话");
        assert!(one.notes.join("；").contains("936"), "{:?}", one.notes);
        assert_eq!(one.text, "\u{a1}\u{a2}");
    }

    /// 表格：单元格与行分隔出制表符，正文按行分开
    #[test]
    fn table_cells_become_tabs_and_paragraphs_newlines() {
        let one = rtf("{\\trowd\\trgaph108\\cellx1000 科目\\cell 金额\\cell\\row 服务器\\cell 124000\\cell\\row}");
        assert_eq!(one.lines.len(), 2, "{:?}", one.lines);
        assert_eq!(one.lines[0], "科目\t金额");
        assert_eq!(one.lines[1], "服务器\t124000");
    }

    /// LibreOffice 从 python-docx 的 docx 转出来的 RTF：正文必须与 docx 的段落对得上
    /// （期望值来自 `office_reader.py` 对同一份文件的独立读取；表格两行各算一行）
    #[test]
    fn reads_a_real_producer_rtf() {
        let one = extract(&fixture("notes.rtf"));
        assert_eq!(one.lines.len(), 7, "{:?}", one.lines);
        assert_eq!(one.lines[0], "一级标题：预算口径");
        assert_eq!(one.lines[1], "第三季度服务器预算为十二万四千元");
        assert_eq!(one.lines[2], "二级标题：明细");
        assert_eq!(one.lines[3], "科目\t金额");
        assert_eq!(one.lines[4], "服务器\t124000");
        assert_eq!(one.lines[5], "口径见 预算制度");
        assert_eq!(one.lines[6], "最后一页说明：数字为含税口径");
        assert_eq!(one.pictures, 1, "文档里那张 PNG 是以 pict 的形式进来的");
        assert_eq!(one.hex_bytes, 183);
        assert_eq!(one.unicode_escapes, 60);
        assert_eq!(one.declared_codepage, 1252);
        assert!(one.text.contains("预算制度"), "超链接的结果文本要在");
        assert!(
            !one.text.contains("https://example.com"),
            "域指令原文不属于正文，混进来就是跳过没生效"
        );
        assert!(
            !one.text.contains("\\p"),
            "不许有没吃完的控制字：{}",
            one.text
        );
        assert!(one.notes.is_empty(), "{:?}", one.notes);
    }

    /// 标题的层级只从样式名来：`heading 1` 算、`Heading 2` 也算（这个词不分大小写），
    /// `heading3`（中间没空格）、`heading 1a`（数字后面还有字）、`Summary`（自定义名）都不算；
    /// 同号的字符样式 `{\*\cs1 heading 3;}` 不能顶掉段落样式的名字 —— `\sN` 与 `\csN`
    /// 是两个各自的编号空间；末尾没有 `\par` 的那一段不收（段以回车收，与 `lines` 同一口径）。
    /// 期望值来自 `lyco_rtf.py` 对同一串字的独立读取
    #[test]
    fn headings_come_only_from_the_style_name() {
        let one = rtf(
            "{\\rtf1\\ansi{\\stylesheet {\\s0 Normal;}{\\s1 heading 1;}{\\s2 Heading 2;}{\\s3 heading3;}{\\s4 heading 1a;}{\\s5 Summary;}{\\*\\cs1 heading 3;}}{\\pard\\s1 one\\par}{\\pard\\s2 two\\par}{\\pard\\s3 nospace\\par}{\\pard\\s4 trailing\\par}{\\pard\\s5 custom\\par}{\\pard\\s0 body\\par}{\\pard\\s1 noendpar}}",
        );
        assert_eq!(
            one.headings,
            vec![
                json!({"level": 1, "text": "one"}),
                json!({"level": 2, "text": "two"})
            ],
            "{:?}",
            one.headings
        );
        // 那一段仍然在正文里 —— 不认它当标题，不等于把它丢了
        assert_eq!(one.lines.len(), 7, "{:?}", one.lines);
        assert_eq!(one.lines[6], "noendpar", "{:?}", one.lines);
        // 真件：LibreOffice 的 RTF 导出把样式名写成小写 `heading 1`，两本账都要对
        let real = extract(&fixture("notes.rtf"));
        assert_eq!(
            real.headings,
            vec![
                json!({"level": 1, "text": "一级标题：预算口径"}),
                json!({"level": 2, "text": "二级标题：明细"})
            ],
            "{:?}",
            real.headings
        );
        // 一份全用 Normal 的：空数组，不是 null
        assert!(
            extract(&fixture("notes-end.rtf")).headings.is_empty(),
            "{:?}",
            extract(&fixture("notes-end.rtf")).headings
        );
        // 一条标题就是**一段**：换行与制表都不该混进它的字里。
        // 这一条要单独守 —— 上一版把切片写成「从起点到文末」，层级数全对、文本却吃掉整篇文档，
        // 只比条数与层级的断言抓不住
        for one in real.headings.iter().chain(one.headings.iter()) {
            let text = one["text"].as_str().unwrap_or_default();
            assert!(!text.contains('\n') && !text.contains('\t'), "{text}");
        }
    }

    /// 空输入与只有控制字的输入：给空文本，而不是 panic
    #[test]
    fn degenerate_inputs_answer_empty() {
        assert_eq!(extract(b"").text, "");
        assert_eq!(extract(b"{\\rtf1}").text, "");
        assert_eq!(extract(b"\\").text, "");
        assert_eq!(
            extract(b"{unclosed").text,
            "unclosed",
            "没有控制字时文本就是文本"
        );
    }

    /// `\info` 群：标题在 `\upr` 的 `\*\ud` 那一边，其余键与 `\info{}` 平级排着。
    /// 期望值来自 `lyco_rtf.py` 的 `rtf_info`，CI 里逐字段对账。
    #[test]
    fn info_group_metadata_comes_out_whole() {
        let bytes = fixture("notes.rtf");
        let info = parse_info(&bytes);
        assert!(info.found);
        assert_eq!(info.codepage, 1252);
        assert_eq!(
            info.fields.get("title").map(|one| one.as_str()),
            Some("季度预算说明")
        );
        assert_eq!(
            info.fields.get("subject").map(|one| one.as_str()),
            Some("季度预算")
        );
        assert_eq!(
            info.fields.get("keywords").map(|one| one.as_str()),
            Some("budget, quarterly")
        );
        assert_eq!(
            info.fields.get("comment").map(|one| one.as_str()),
            Some("fixture produced by python-docx")
        );
        assert_eq!(
            info.fields.get("author").map(|one| one.as_str()),
            Some("liuqi")
        );
        assert_eq!(
            info.fields.get("created").map(|one| one.as_str()),
            Some("2013-12-23T23:15:00")
        );
        assert_eq!(info.fields.len(), 7, "{:?}", info.fields);
        // 四条自定义属性：名字要整块交账，逐字交账会变成几十条
        let names: Vec<String> = info.props.iter().map(|one| one.name.clone()).collect();
        assert_eq!(
            names,
            vec![
                "AppVersion".to_string(),
                "OOXMLCorePropertyCategory".to_string(),
                "口径".to_string(),
                "预算额度".to_string()
            ],
            "{names:?}"
        );
        assert_eq!(
            info.props[0].value.as_deref(),
            Some("14.0000"),
            "{:?}",
            info.props[0]
        );
        assert_eq!(info.props[3].kind, Some(3));
        assert_eq!(info.props[3].value.as_deref(), Some("124000"));
        // printim 全零：不能把 0000-00-00 当成一个真时间报出去
        assert!(!info.fields.contains_key("printed"), "{:?}", info.fields);
        assert!(
            info.notes.iter().any(|one| one.contains("printed")),
            "{:?}",
            info.notes
        );
    }

    /// 没有 `\info` 的文件要照实说，不能给一个空 map 装作读过
    #[test]
    fn a_document_without_info_says_so() {
        let info = parse_info(br"{\ansi hello}");
        assert!(!info.found);
        assert!(info.fields.is_empty());
        assert!(!info.notes.is_empty());
    }

    /// `\upr` 的第一群是 7 位回退文本：只认它就会拿到一串问号
    #[test]
    fn the_upr_fallback_is_not_the_value() {
        let src = br"\{\ansi\ansicpg1252\info{\upr{\title \'3f\'3f}{\*\ud{\title \u26381\'3f\u21153\'3f}}}";
        let info = parse_info(src);
        assert_eq!(
            info.fields.get("title").map(|one| one.as_str()),
            Some("服务"),
            "{:?}",
            info.fields
        );
    }

    /// 书签那一群：跳之前把名字读出来（名字照样不进正文），而站内跳转的地址住在指令里 ——
    /// 两处的名字对上才算这一跳落得地，对不上就是一条坏跳转
    #[test]
    fn a_bookmark_name_is_read_without_leaking_into_the_text() {
        let one = extract(&fixture("fields.rtf"));
        assert_eq!(one.bookmark_starts, 1);
        assert_eq!(one.bookmark_ends, 1);
        assert_eq!(one.bookmarks, vec!["表锚点".to_string()]);
        let j = one.to_json();
        assert_eq!(j["anchors"], json!(["表锚点", "没这个书签"]));
        assert_eq!(j["anchors_found"], 1);
        assert_eq!(j["anchors_missing"], 1, "{j}");
        assert_eq!(j["fields"], 5);
        // 这两条跳转都是站内的（两个 # 开头），所以「站外几条」在这里是 0
        assert_eq!(j["links_external"], 0, "{j}");
        // 读名字不改跳过：那一个名字不在页面上的任何一句话里（六行一个字都没多）
        assert_eq!(
            j["lines"],
            json!([
                "域与跳转",
                "题注：1",
                "跳到那张表跳一个坏了的名（站内跳转，不占关系表；第二条点的是一个不存在的名）",
                "被内部链接指着的那一段",
                "自动日期：2026-09-25",
                "1（页码写在正文里一次，页脚里一次）"
            ])
        );
        // 站外那一条与书签无关：两条列表都空着，只交地址
        let toc = extract(&fixture("toc.rtf"));
        let ct = toc.to_json();
        assert!(ct["bookmarks"].as_array().is_some_and(|had| had.is_empty()));
        assert!(ct["anchors"].as_array().is_some_and(|had| had.is_empty()));
        assert_eq!(ct["links_external"], 1);
        assert_eq!(toc.bookmark_starts, 0, "这份件里一个书签也没有");
    }
}
