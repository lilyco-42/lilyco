//! Image inspection without decoders: magic sniffing, dimensions and a
//! downscale plan that keeps vision tokens predictable.
//!
//! Only the first bytes of a file are needed, so this works on a stream and
//! never allocates.

/// Formats the agent can hand to a vision model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Png,
    Jpeg,
    Gif,
    WebP,
    Bmp,
    Svg,
    Unknown,
}

impl Format {
    /// Short lowercase name used in trace rows and CSS class names.
    pub const fn label(self) -> &'static str {
        match self {
            Format::Png => "png",
            Format::Jpeg => "jpeg",
            Format::Gif => "gif",
            Format::WebP => "webp",
            Format::Bmp => "bmp",
            Format::Svg => "svg",
            Format::Unknown => "unknown",
        }
    }

    /// MIME type for the data URL or MCP content block.
    pub const fn mime(self) -> &'static str {
        match self {
            Format::Png => "image/png",
            Format::Jpeg => "image/jpeg",
            Format::Gif => "image/gif",
            Format::WebP => "image/webp",
            Format::Bmp => "image/bmp",
            Format::Svg => "image/svg+xml",
            Format::Unknown => "application/octet-stream",
        }
    }

    /// True when the format has an intrinsic pixel size in its header.
    pub const fn is_raster(self) -> bool {
        matches!(self, Format::Png | Format::Jpeg | Format::Gif | Format::WebP | Format::Bmp)
    }
}

/// Largest edge the vision pipeline will keep.
pub const VISION_MAX_EDGE: u32 = 1568;

/// Identify a format from its leading bytes.
pub fn sniff(head: &[u8]) -> Format {
    if head.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Format::Png;
    }
    if head.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Format::Jpeg;
    }
    if head.starts_with(b"GIF87a") || head.starts_with(b"GIF89a") {
        return Format::Gif;
    }
    if head.len() >= 12 && &head[0..4] == b"RIFF" && &head[8..12] == b"WEBP" {
        return Format::WebP;
    }
    if head.starts_with(b"BM") {
        return Format::Bmp;
    }
    if looks_like_svg(head) {
        return Format::Svg;
    }
    Format::Unknown
}

/// Read pixel dimensions when the header carries them.
pub fn dimensions(fmt: Format, head: &[u8]) -> Option<(u32, u32)> {
    match fmt {
        Format::Png => png_dims(head),
        Format::Gif => gif_dims(head),
        Format::Bmp => bmp_dims(head),
        Format::WebP => webp_dims(head),
        Format::Jpeg => jpeg_dims(head),
        _ => None,
    }
}

/// Scale down to `max_edge` keeping aspect; never upscales.
pub fn fit(max_edge: u32, w: u32, h: u32) -> (u32, u32) {
    if w == 0 || h == 0 || max_edge == 0 {
        return (0, 0);
    }
    let edge = w.max(h);
    if edge <= max_edge {
        return (w, h);
    }
    let nw = (u64::from(w) * u64::from(max_edge) / u64::from(edge)) as u32;
    let nh = (u64::from(h) * u64::from(max_edge) / u64::from(edge)) as u32;
    (nw.max(1), nh.max(1))
}

/// One-line description for the model, e.g. `png 1024x768`.
pub fn describe<const N: usize>(
    fmt: Format,
    dims: Option<(u32, u32)>,
    out: &mut heapless::String<N>,
) -> Result<(), &'static str> {
    use ::core::fmt::Write as _;
    let name = fmt.label();
    match dims {
        Some((w, h)) => ::core::write!(out, "{} {}x{}", name, w, h).map_err(|_| "buffer full"),
        None => out.push_str(name).map_err(|_| "buffer full"),
    }
}

fn looks_like_svg(head: &[u8]) -> bool {
    let bom = [0xEF, 0xBB, 0xBF];
    let body = head.strip_prefix(&bom[..]).unwrap_or(head);
    let mut i = 0usize;
    while i < body.len() && body[i].is_ascii_whitespace() {
        i += 1;
    }
    let rest = &body[i..];
    rest.starts_with(b"<svg") || rest.starts_with(b"<?xml") || rest.starts_with(b"<!DOCTYPE svg")
}

fn be16(b: &[u8], off: usize) -> Option<u16> {
    let hi = *b.get(off)?;
    let lo = *b.get(off + 1)?;
    Some(u16::from(hi) << 8 | u16::from(lo))
}

fn le16(b: &[u8], off: usize) -> Option<u16> {
    let lo = *b.get(off)?;
    let hi = *b.get(off + 1)?;
    Some(u16::from(hi) << 8 | u16::from(lo))
}

fn le24(b: &[u8], off: usize) -> Option<u32> {
    let a = u32::from(*b.get(off)?);
    let c = u32::from(*b.get(off + 1)?);
    let d = u32::from(*b.get(off + 2)?);
    Some(a | (c << 8) | (d << 16))
}

fn be32(b: &[u8], off: usize) -> Option<u32> {
    let a = *b.get(off)?;
    let c = *b.get(off + 1)?;
    let d = *b.get(off + 2)?;
    let e = *b.get(off + 3)?;
    Some(u32::from(a) << 24 | u32::from(c) << 16 | u32::from(d) << 8 | u32::from(e))
}

fn le32(b: &[u8], off: usize) -> Option<u32> {
    let a = *b.get(off)?;
    let c = *b.get(off + 1)?;
    let d = *b.get(off + 2)?;
    let e = *b.get(off + 3)?;
    Some(u32::from(a) | u32::from(c) << 8 | u32::from(d) << 16 | u32::from(e) << 24)
}

fn png_dims(b: &[u8]) -> Option<(u32, u32)> {
    if b.len() < 24 {
        return None;
    }
    Some((be32(b, 16)?, be32(b, 20)?))
}

fn gif_dims(b: &[u8]) -> Option<(u32, u32)> {
    Some((u32::from(le16(b, 6)?), u32::from(le16(b, 8)?)))
}

fn bmp_dims(b: &[u8]) -> Option<(u32, u32)> {
    let w = le32(b, 18)?;
    let h = le32(b, 22)?;
    Some((w, h & 0x7FFF_FFFF))
}

fn webp_dims(b: &[u8]) -> Option<(u32, u32)> {
    if b.len() < 30 || &b[12..16] != b"VP8X" {
        return None;
    }
    let w = le24(b, 24)? + 1;
    let h = le24(b, 27)? + 1;
    Some((w, h))
}

fn jpeg_dims(b: &[u8]) -> Option<(u32, u32)> {
    let mut i = 2usize;
    while i + 9 < b.len() {
        if b[i] != 0xFF {
            return None;
        }
        let marker = b[i + 1];
        if (0xC0..=0xC3).contains(&marker) {
            let h = u32::from(be16(b, i + 5)?);
            let w = u32::from(be16(b, i + 7)?);
            return Some((w, h));
        }
        let size = usize::from(be16(b, i + 2)?);
        if size < 2 {
            return None;
        }
        i += size + 2;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: [u8; 24] = [
        0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, //
        0, 0, 0, 13, b'I', b'H', b'D', b'R', //
        0, 0, 0x02, 0x80, // width  = 640 (4-byte big endian)
        0, 0, 0x01, 0xE0, // height = 480
    ];

    #[test]
    fn sniffs_known_formats() {
        assert_eq!(sniff(&PNG), Format::Png);
        assert_eq!(sniff(&[0xFF, 0xD8, 0xFF, 0xE0]), Format::Jpeg);
        assert_eq!(sniff(b"GIF89a...."), Format::Gif);
        assert_eq!(sniff(b"BM......"), Format::Bmp);
        assert_eq!(sniff(b"<svg xmlns=\"...\">"), Format::Svg);
        assert_eq!(sniff(b"not an image"), Format::Unknown);
    }

    #[test]
    fn reads_png_dimensions() {
        assert_eq!(dimensions(Format::Png, &PNG), Some((640, 480)));
    }

    #[test]
    fn reads_gif_dimensions() {
        let mut b = [0u8; 10];
        b[0..6].copy_from_slice(b"GIF89a");
        b[6] = 0x20;
        b[7] = 0x03;
        b[8] = 0x58;
        b[9] = 0x02;
        assert_eq!(dimensions(Format::Gif, &b), Some((800, 600)));
    }

    #[test]
    fn fit_never_upscales() {
        assert_eq!(fit(1568, 100, 50), (100, 50));
        assert_eq!(fit(100, 400, 200), (100, 50));
        assert_eq!(fit(0, 10, 10), (0, 0));
    }

    #[test]
    fn describe_is_compact() {
        let mut buf: heapless::String<32> = heapless::String::new();
        describe(Format::Png, Some((8, 8)), &mut buf).unwrap();
        assert_eq!(buf.as_str(), "png 8x8");
    }

    #[test]
    fn mime_types_are_canonical() {
        assert_eq!(Format::Png.mime(), "image/png");
        assert_eq!(Format::Svg.mime(), "image/svg+xml");
        assert!(Format::Png.is_raster());
        assert!(!Format::Svg.is_raster());
    }
}
