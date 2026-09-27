//! 合成字节的那一份账：期望值全部来自第二读者 `scripts/acceptance/lyco_pictures.py`
//! 在这个模块的字节上跑出来的输出（`.scratch/pb_synth.txt`、`.scratch/pb_rtf_ledger.txt`），
//! 一条也没有手写。EMF、可放置 WMF、`BITMAPHEADER12`、`SOF2`、大端 TIFF 这几种形状
//! 155 份真件里一份都没有，只能这样凭空摆出来。

use super::*;

/// 那串字节的形状与第二读者 `read()` 交的那本一模一样，好逐格对
fn said(raw: &[u8]) -> Value {
    let one = read(raw);
    json!({
        "format": one.format,
        "px": {"w": one.pw, "h": one.ph},
        "how": one.how,
        "density": one.dens.to_json(),
        "nat_mm100": {"w": one.nat_w, "h": one.nat_h},
    })
}

/// 从 hex 还原：这些 blob 由第二读者的脚本自己拼出来，这里只抄它的字节
fn bytes_of(src: &str) -> Vec<u8> {
    (0..src.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&src[at..at + 2], 16).expect("hex"))
        .collect()
}

const PNG_PPM: &str =
    "89504e470d0a1a0a0000000d494844520000002800000018080200000007e628970000000970485973\
00002e2300002e230178a53f760000000e49444154187363606098000000ffff030005140276ec";
const PNG_ASPECT: &str =
    "89504e470d0a1a0a0000000d494844520000002800000018080200000007e628970000000970485973\
0000000100000001004f25c4d60000000e49444154187363606098000000ffff030005140276ec";
const PNG_NOPHYS: &str =
    "89504e470d0a1a0a0000000d494844520000002800000018080200000007e628970000000e49444154\
187363606098000000ffff030005140276ec";
const JPEG_DPI: &str = "ffd8ffe000104a46494600010101012c012c0000ffc0000a0800180028011100";
const JPEG_ASPECT: &str = "ffd8ffe000104a46494600010100000100020000ffc0000a0800180028011100";
const JPEG_EXT: &str = "ffd8ffe000104a46494600010101004800480000ffc2000a0800180028011100";
const GIF: &str = "47494638396120001000000000";
const BMP_PPM: &str =
    "424d000000000000000000000000280000002000000010000000010018000000000000000000130b0000130b0000";
const BMP_ZERO: &str =
    "424d0000000000000000000000002800000020000000100000000100180000000000000000000000000000000000";
const BMP_CORE: &str = "424d0000000000000000000000000c000000200000001000000001001800";
const TIFF_DPI: &str =
    "49492a000800000005000001040001000000200000000101040001000000100000001a010500010000004a0000\
001b010500010000004a00000028010300010000000200000000000000c0c62d0010270000c0c62d0010270000";
const TIFF_NOUNIT: &str =
    "49492a000800000004000001030001000000200000000101030001000000100000001a010500010000003e00\
00001b010500010000003e00000000000000130b000001000000130b000001000000";
const TIFF_BE: &str =
    "4d4d002a000000080005010000030000000100200000010100030000000100100000011a0005000000010000004a\
011b0005000000010000004a0128000300000001000300000000000000000060000000010000006000000001";
const TIFF_ASPECT: &str =
    "49492a000800000005000001040001000000200000000101040001000000100000001a010500010000004a00\
00001b010500010000004a0000002801030001000000010000000000000060000000010000006000000001000000";
const EMF: &str =
    "010000005800000000000000000000002700000017000000a0fdffff6afeffff650500009b03000020454d46";
const WMF_PLACE: &str = "d7cdc69a00000000000000001f000f006000";
const WMF_PLACE_ZERO: &str = "d7cdc69a00000000000000001f000f000000";
const WMF_PLAIN: &str = "01000900574600000100000000000000000000000000000000000000";
const SVM: &str = "56434c4d54460100000000000000000000000000000000000000000000000000";
const JUNK: &str = "00010203";

/// PNG：pHYs 的第三个字节是单位（只认 1=每米），写了 0 就是「没有单位」而不是 1
#[test]
fn a_png_chunk_table_stops_at_the_pixel_data() {
    assert_eq!(
        said(&bytes_of(PNG_PPM)),
        json!({"format": "png", "px": {"w": 40, "h": 24}, "how": "IHDR",
               "density": {"state": "read", "unit": "ppm", "x": 11811, "y": 11811,
                           "written": "11811,11811,1"},
               "nat_mm100": {"w": 339, "h": 203}}),
        "Pillow 存的 300 dpi 就是 11811 每米：{PNG_PPM}"
    );
    assert_eq!(
        said(&bytes_of(PNG_ASPECT)),
        json!({"format": "png", "px": {"w": 40, "h": 24}, "how": "IHDR",
               "density": {"state": "unitless", "unit": "unknown", "x": 1, "y": 1,
                           "written": "1,1,0"},
               "nat_mm100": {"w": null, "h": null}}),
        "没单位的两个数算不出尺寸，交 null 而不是猜 1:1：{PNG_ASPECT}"
    );
    assert_eq!(
        said(&bytes_of(PNG_NOPHYS)),
        json!({"format": "png", "px": {"w": 40, "h": 24}, "how": "IHDR",
               "density": {"state": "absent", "unit": null, "x": null, "y": null,
                           "written": null},
               "nat_mm100": {"w": null, "h": null}}),
        "整块 pHYs 不在：这一格连写过的字符串都没有：{PNG_NOPHYS}"
    );
}

/// JPEG：密度在 APP0 的 JFIF 里，像素在**它自己那一个** SOF 里（扩展式也认）
#[test]
fn a_jpeg_reads_the_sof_segment_it_actually_uses() {
    assert_eq!(
        said(&bytes_of(JPEG_DPI)),
        json!({"format": "jpeg", "px": {"w": 40, "h": 24}, "how": "SOF0",
               "density": {"state": "read", "unit": "dpi", "x": 300, "y": 300,
                           "written": "JFIF,1,300,300"},
               "nat_mm100": {"w": 339, "h": 203}}),
        "{JPEG_DPI}"
    );
    assert_eq!(
        said(&bytes_of(JPEG_ASPECT)),
        json!({"format": "jpeg", "px": {"w": 40, "h": 24}, "how": "SOF0",
               "density": {"state": "unitless", "unit": "aspect", "x": 1, "y": 2,
                           "written": "JFIF,0,1,2"},
               "nat_mm100": {"w": null, "h": null}}),
        "unit 0 只是长宽比，写进 unit 那一格而不是当成 dpi：{JPEG_ASPECT}"
    );
    assert_eq!(
        said(&bytes_of(JPEG_EXT)),
        json!({"format": "jpeg", "px": {"w": 40, "h": 24}, "how": "SOF2",
               "density": {"state": "read", "unit": "dpi", "x": 72, "y": 72,
                           "written": "JFIF,1,72,72"},
               "nat_mm100": {"w": 1411, "h": 847}}),
        "渐进式（SOF2）也是像素所在的那一格：{JPEG_EXT}"
    );
}

/// GIF 的头里根本没有密度（所以是 `none`：这一族没这一格），BMP 有两种头
#[test]
fn a_gif_has_no_density_while_a_bmp_has_two_kinds_of_header() {
    assert_eq!(
        said(&bytes_of(GIF)),
        json!({"format": "gif", "px": {"w": 32, "h": 16}, "how": "LogicalScreenDescriptor",
               "density": {"state": "none", "unit": null, "x": null, "y": null, "written": null},
               "nat_mm100": {"w": null, "h": null}}),
        "{GIF}"
    );
    assert_eq!(
        said(&bytes_of(BMP_PPM)),
        json!({"format": "bmp", "px": {"w": 32, "h": 16}, "how": "BITMAPHEADER40",
               "density": {"state": "read", "unit": "ppm", "x": 2835, "y": 2835,
                           "written": "2835,2835"},
               "nat_mm100": {"w": 1129, "h": 564}}),
        "{BMP_PPM}"
    );
    assert_eq!(
        said(&bytes_of(BMP_ZERO)),
        json!({"format": "bmp", "px": {"w": 32, "h": 16}, "how": "BITMAPHEADER40",
               "density": {"state": "absent", "unit": null, "x": null, "y": null,
                           "written": "0,0"},
               "nat_mm100": {"w": null, "h": null}}),
        "写了两个零等于没说，可它毕竟写了：{BMP_ZERO}"
    );
    assert_eq!(
        said(&bytes_of(BMP_CORE)),
        json!({"format": "bmp", "px": {"w": 32, "h": 16}, "how": "BITMAPHEADER12",
               "density": {"state": "none", "unit": null, "x": null, "y": null,
                           "written": "core"},
               "nat_mm100": {"w": null, "h": null}}),
        "12 字节的旧头里没有宽高以外的字段：{BMP_CORE}"
    );
}

/// TIFF：RATIONAL 那个字段是**偏移**不是数值；单位格（296）不写时按 2=dpi 算
#[test]
fn a_tiff_ratio_is_an_offset_and_its_unit_defaults_to_dpi() {
    assert_eq!(
        said(&bytes_of(TIFF_DPI)),
        json!({"format": "tiff", "px": {"w": 32, "h": 16}, "how": "IFD0",
               "density": {"state": "read", "unit": "dpi", "x": 300, "y": 300,
                           "written": "3000000/10000,3000000/10000,2"},
               "nat_mm100": {"w": 271, "h": 135}}),
        "{TIFF_DPI}"
    );
    assert_eq!(
        said(&bytes_of(TIFF_NOUNIT)),
        json!({"format": "tiff", "px": {"w": 32, "h": 16}, "how": "IFD0",
               "density": {"state": "read", "unit": "dpi", "x": 2835, "y": 2835,
                           "written": "2835/1,2835/1,None"},
               "nat_mm100": {"w": 29, "h": 14}}),
        "没写 296 就按规范缺省当 dpi，但 `written` 里那个 None 要留着：{TIFF_NOUNIT}"
    );
    assert_eq!(
        said(&bytes_of(TIFF_BE)),
        json!({"format": "tiff", "px": {"w": 32, "h": 16}, "how": "IFD0",
               "density": {"state": "read", "unit": "dpcm", "x": 96, "y": 96,
                           "written": "96/1,96/1,3"},
               "nat_mm100": {"w": 333, "h": 167}}),
        "大端那份的两个数与那枚单位都要按大端读：{TIFF_BE}"
    );
    assert_eq!(
        said(&bytes_of(TIFF_ASPECT)),
        json!({"format": "tiff", "px": {"w": 32, "h": 16}, "how": "IFD0",
               "density": {"state": "unitless", "unit": "unknown", "x": 96, "y": 96,
                           "written": "96/1,96/1,1"},
               "nat_mm100": {"w": null, "h": null}}),
        "{TIFF_ASPECT}"
    );
}

/// 两个元文件各说各的单位：EMF 那个框本来就是 0.01mm，可放置 WMF 说的是 inch
#[test]
fn a_metafile_answers_in_its_own_unit() {
    assert_eq!(
        said(&bytes_of(EMF)),
        json!({"format": "emf", "px": {"w": 40, "h": 24}, "how": "ENHMETAHEADER",
               "density": {"state": "read", "unit": "mm100", "x": 1990, "y": 1330,
                           "written": "-608,-406,1381,923"},
               "nat_mm100": {"w": 1990, "h": 1330}}),
        "边界含两端，所以各加一个像素；自然尺寸不做除法：{EMF}"
    );
    assert_eq!(
        said(&bytes_of(WMF_PLACE)),
        json!({"format": "wmf", "px": {"w": 32, "h": 16}, "how": "PlaceableHeader",
               "density": {"state": "read", "unit": "dpi", "x": 96, "y": 96,
                           "written": "96"},
               "nat_mm100": {"w": 847, "h": 423}}),
        "{WMF_PLACE}"
    );
    assert_eq!(
        said(&bytes_of(WMF_PLACE_ZERO)),
        json!({"format": "wmf", "px": {"w": 32, "h": 16}, "how": "PlaceableHeader",
               "density": {"state": "absent", "unit": "dpi", "x": 0, "y": 0,
                           "written": "0"},
               "nat_mm100": {"w": null, "h": null}}),
        "那个 0 是「没填」，不能拿去做除数：{WMF_PLACE_ZERO}"
    );
    assert_eq!(
        said(&bytes_of(WMF_PLAIN)),
        json!({"format": "wmf", "px": {"w": null, "h": null}, "how": "Header",
               "density": {"state": "none", "unit": null, "x": null, "y": null, "written": null},
               "nat_mm100": {"w": null, "h": null}}),
        "普通 WMF 的头里连像素数都没有：{WMF_PLAIN}"
    );
}

/// 「认不出」与「没有字节」是两件事：前者 format 是 unknown，后者是 null
#[test]
fn an_unreadable_blob_is_not_the_same_as_no_blob() {
    assert_eq!(
        said(&bytes_of(SVM)),
        json!({"format": "svm", "px": {"w": null, "h": null}, "how": null,
               "density": {"state": "none", "unit": null, "x": null, "y": null, "written": null},
               "nat_mm100": {"w": null, "h": null}}),
        "StarView 认得签名、没有解码器：{SVM}"
    );
    assert_eq!(
        said(&bytes_of(JUNK)),
        json!({"format": "unknown", "px": {"w": null, "h": null}, "how": null,
               "density": {"state": "none", "unit": null, "x": null, "y": null, "written": null},
               "nat_mm100": {"w": null, "h": null}}),
        "{JUNK}"
    );
    let empty = said(&[]);
    assert_eq!(empty["format"], Value::Null, "一串空字节：detect 交 None");
    assert!(
        empty["density"]["state"].is_null(),
        "根本没有字节可读，所以 density 整格 null（不是 none）：{empty}"
    );
}

/// 一条账把两边的凭据都交出来：文档说的、部件名写的、字节自己签的
#[test]
fn a_row_keeps_three_witnesses_and_only_compares_what_both_sides_said() {
    let mut item = Item::new("word/media/x.png", "read");
    item.word = Some("image/png".to_string());
    item.ext = Some("png".to_string());
    item.bytes = bytes_of(PNG_PPM);
    item.placed_w = Some(339);
    item.placed_h = Some(203);
    item.placed_from = Some("wp:extent".to_string());
    item.placed_written = Some(vec![Some("121920".to_string()), Some("73152".to_string())]);
    let mine = row(&item);
    assert_eq!(
        mine,
        json!({"where": "word/media/x.png", "addr": "read", "word": "image/png",
               "word_name": "png", "ext": "png", "ext_name": "png", "sig": "png",
               "agrees": true, "ext_agrees": true,
               "pixels": {"w": 40, "h": 24}, "declared_pixels": null, "px_agrees": null,
               "density": {"state": "read", "unit": "ppm", "x": 11811, "y": 11811,
                           "written": "11811,11811,1"},
               "nat_mm100": {"w": 339, "h": 203},
               "placed_mm100": {"w": 339, "h": 203, "from": "wp:extent",
                                "written": ["121920", "73152"]},
               "scale_permille": {"w": 1000, "h": 1000}, "aspect_permille": 2,
               "stretched": false, "at_natural": true,
               "how": "IHDR", "head_hex": "89504e470d0a1a0a", "note": null}),
        "EMU 121920×73152 换出来正好是自然尺寸：{mine}"
    );
}

/// 字节里读不出像素的那一条（普通 WMF）：文档自己声称的像素数没法比，交 null
#[test]
fn pixels_the_bytes_cannot_show_stay_undecided() {
    let mut item = Item::new("pict#0", "read");
    item.word = Some("wmetafile".to_string());
    item.bytes = bytes_of(WMF_PLAIN);
    item.placed_w = Some(677);
    item.placed_h = Some(339);
    item.placed_from = Some(r"\picwgoal×\picscalex".to_string());
    item.placed_written = Some(vec![
        Some("384".to_string()),
        Some("192".to_string()),
        Some("100".to_string()),
        Some("100".to_string()),
    ]);
    item.decl_px_w = Some(40);
    item.decl_px_h = Some(24);
    let mine = row(&item);
    assert_eq!(
        mine,
        json!({"where": "pict#0", "addr": "read", "word": "wmetafile",
               "word_name": "wmf", "ext": null, "ext_name": null, "sig": "wmf",
               "agrees": true, "ext_agrees": null,
               "pixels": {"w": null, "h": null},
               "declared_pixels": {"w": 40, "h": 24}, "px_agrees": null,
               "density": {"state": "none", "unit": null, "x": null, "y": null, "written": null},
               "nat_mm100": {"w": null, "h": null},
               "placed_mm100": {"w": 677, "h": 339, "from": "\\picwgoal×\\picscalex",
                                "written": ["384", "192", "100", "100"]},
               "scale_permille": {"w": null, "h": null}, "aspect_permille": null,
               "stretched": null, "at_natural": null,
               "how": "Header", "head_hex": "0100090057460000", "note": null}),
        "{mine}"
    );
    // 三本比对的账里，只有「两边都认得」的那两本有答案
    assert_eq!(mine["agrees"], json!(true));
    assert_eq!(mine["ext_agrees"], Value::Null, "这一族没有扩展名那一格");
    assert_eq!(mine["px_agrees"], Value::Null, "字节里没写像素数");
}

/// 一个框里没有任何字节（`tbox.docx` 的那一条）：这一行还在，密度整格 null
#[test]
fn a_shape_with_no_bytes_still_makes_a_row() {
    let mut item = Item::new("(no-blip)", "none");
    item.placed_w = Some(5001);
    item.placed_h = Some(2401);
    item.placed_from = Some("wp:extent".to_string());
    item.placed_written = Some(vec![
        Some("1800225".to_string()),
        Some("864235".to_string()),
    ]);
    let mine = row(&item);
    assert_eq!(
        mine,
        json!({"where": "(no-blip)", "addr": "none", "word": null, "word_name": null,
               "ext": null, "ext_name": null, "sig": null,
               "agrees": null, "ext_agrees": null,
               "pixels": {"w": null, "h": null}, "declared_pixels": null, "px_agrees": null,
               "density": {"state": null, "unit": null, "x": null, "y": null, "written": null},
               "nat_mm100": {"w": null, "h": null},
               "placed_mm100": {"w": 5001, "h": 2401, "from": "wp:extent",
                                "written": ["1800225", "864235"]},
               "scale_permille": {"w": null, "h": null}, "aspect_permille": null,
               "stretched": null, "at_natural": null,
               "how": null, "head_hex": "", "note": null}),
        "尺寸照交，密度那一格不是 none 而是 null：{mine}"
    );
}

/// RTF 那一条流上的四张图：三种单位合成一个尺寸，而没写 blip 的那一群**不借**下一张的字节
///
/// 期望值是整个 blob 交给第二读者 `rtf_ledger` 跑出来的那份（`.scratch/pb_rtf_ledger.txt`）
#[test]
fn a_pict_group_never_borrows_the_next_pictures_bytes() {
    let doc = RTF_DOC
        .replace("PNGHEX", PNG_PPM)
        .replace("WMFHEX", WMF_PLAIN)
        .replace("JPGHEX", JPEG_DPI);
    let mine = rtf(doc.as_bytes(), 100);
    assert_eq!(mine["total"], json!(4), "{doc}");
    assert_eq!(
        mine["addr"],
        json!({"read": 3, "unresolved": 0, "missing": 0, "none": 1}),
        "第三群一个字节也没写"
    );
    assert_eq!(
        mine["detected"],
        json!({"(没读到)": 1, "jpeg": 1, "png": 1, "wmf": 1})
    );
    assert_eq!(
        mine["declared_pixels"],
        json!({"written": 3, "agrees": 2, "disagrees": 0, "undecided": 2}),
        "只有 RTF 写了文档自己声称的像素数"
    );
    assert_eq!(
        mine["rows"][0],
        json!({"where": "pict#0", "addr": "read", "word": "pngblip", "word_name": "png",
               "ext": null, "ext_name": null, "sig": "png",
               "agrees": true, "ext_agrees": null,
               "pixels": {"w": 40, "h": 24}, "declared_pixels": {"w": 40, "h": 24},
               "px_agrees": true,
               "density": {"state": "read", "unit": "ppm", "x": 11811, "y": 11811,
                           "written": "11811,11811,1"},
               "nat_mm100": {"w": 339, "h": 203},
               "placed_mm100": {"w": 17498, "h": 10495, "from": "\\picwgoal×\\picscalex",
                                "written": ["1984", "1190", "500", "500"]},
               "scale_permille": {"w": 51617, "h": 51700}, "aspect_permille": 0,
               "stretched": false, "at_natural": false,
               "how": "IHDR", "head_hex": "89504e470d0a1a0a", "note": null}),
        "1984 twips × 500% 一次除到位：{mine}"
    );
    assert_eq!(
        mine["rows"][2],
        json!({"where": "pict#2", "addr": "none", "word": null, "word_name": null,
               "ext": null, "ext_name": null, "sig": null,
               "agrees": null, "ext_agrees": null,
               "pixels": {"w": null, "h": null}, "declared_pixels": null, "px_agrees": null,
               "density": {"state": null, "unit": null, "x": null, "y": null, "written": null},
               "nat_mm100": {"w": null, "h": null},
               "placed_mm100": {"w": 3500, "h": 2099, "from": "\\picwgoal×\\picscalex",
                                "written": ["1984", "1190", null, null]},
               "scale_permille": {"w": null, "h": null}, "aspect_permille": null,
               "stretched": null, "at_natural": null,
               "how": null, "head_hex": "", "note": null}),
        "这一群自己没写 blip，就不能拿后面那张图的字节当自己的：{mine}"
    );
    assert_eq!(mine["rows"][3]["sig"], json!("jpeg"));
    assert_eq!(mine["rows"][3]["head_hex"], json!("ffd8ffe000104a46"));
}

/// LibreOffice 写的形状：每群自己一个 blip，`\picscalex` 缺省按 100 算
const RTF_DOC: &str = r"{\rtf1\ansi\deff0{\pict{\*\picprop}\picw40\pich24\picwgoal1984\pichgoal1190\picscalex500\picscaley500\pngblip PNGHEX}{\pict\picw40\pich24\picwgoal384\pichgoal192\wmetafile WMFHEX}{\pict\picwgoal1984\pichgoal1190}{\pict\picw40\pich24\picwgoal1984\pichgoal1190\jpegblip JPGHEX}}";
