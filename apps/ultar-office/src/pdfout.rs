//! A small PDF writer: pages with text, rectangles and pictures. Text is drawn from shaped
//! glyphs (glyph numbers of the TrueType fonts the caller shaped with), so Arabic comes out
//! as laid out; the fonts are embedded whole (no subsetting) as CID fonts with a `ToUnicode`
//! map, so a viewer can copy and search the text. Streams are Flate compressed.
//!
//! It writes PDF 1.7 with a classic cross-reference table, no encryption, no forms. Reading
//! is in [`crate::pdf`]. Coordinates are in point with the origin at the top left of a page
//! (the writer flips them).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

/// What can go wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PdfError {
    /// The font file is not a TrueType file the writer understands.
    BadFont,
    /// The picture data is not usable.
    BadImage,
    /// A page, font or picture number that does not exist.
    Missing,
}

struct Font {
    data: &'static [u8],
    name: String,
    info: TtfInfo,
    used: BTreeMap<u16, String>,
}

enum ImageData {
    Rgb(Vec<u8>),
    Jpeg(Vec<u8>, u8),
}

struct Image {
    w: u32,
    h: u32,
    data: ImageData,
}

/// One page under construction.
pub struct Page {
    w: f32,
    h: f32,
    content: String,
    fonts: BTreeSet<usize>,
    images: BTreeSet<usize>,
}

/// A document under construction.
pub struct Pdf {
    fonts: Vec<Font>,
    images: Vec<Image>,
    pages: Vec<Page>,
    title: Option<String>,
}

#[derive(Clone, Copy, Default)]
struct TtfInfo {
    units: u16,
    bbox: [i16; 4],
    ascent: i16,
    descent: i16,
    metrics: u16,
    hmtx: (usize, usize),
}

fn be16(d: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*d.get(at)?, *d.get(at + 1)?]))
}

fn be32(d: &[u8], at: usize) -> Option<usize> {
    Some(u32::from_be_bytes([
        *d.get(at)?,
        *d.get(at + 1)?,
        *d.get(at + 2)?,
        *d.get(at + 3)?,
    ]) as usize)
}

fn table(d: &[u8], tag: &[u8; 4]) -> Option<(usize, usize)> {
    let n = be16(d, 4)? as usize;
    (0..n).find_map(|i| {
        let rec = 12 + i * 16;
        (d.get(rec..rec + 4)? == tag).then(|| Some((be32(d, rec + 8)?, be32(d, rec + 12)?)))?
    })
}

fn ttf_info(d: &[u8]) -> Option<TtfInfo> {
    if !matches!(d.get(..4)?, [0, 1, 0, 0] | b"true") {
        return None;
    }
    let (head, _) = table(d, b"head")?;
    let (hhea, _) = table(d, b"hhea")?;
    let (hmtx, hlen) = table(d, b"hmtx")?;
    let s = |at: usize| be16(d, at).map(|v| v as i16);
    let info = TtfInfo {
        units: be16(d, head + 18)?,
        bbox: [s(head + 36)?, s(head + 38)?, s(head + 40)?, s(head + 42)?],
        ascent: s(hhea + 4)?,
        descent: s(hhea + 6)?,
        metrics: be16(d, hhea + 34)?,
        hmtx: (hmtx, hlen),
    };
    (info.units > 0 && info.metrics > 0).then_some(info)
}

impl TtfInfo {
    fn advance(&self, d: &[u8], gid: u16) -> u16 {
        let i = gid.min(self.metrics - 1) as usize;
        let at = self.hmtx.0 + i * 4;
        if at + 2 > self.hmtx.0 + self.hmtx.1 {
            return self.units;
        }
        be16(d, at).unwrap_or(self.units)
    }

    fn per_mille(&self, v: i32) -> i32 {
        (i64::from(v) * 1000 / i64::from(self.units)) as i32
    }
}

fn num(v: f32) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

fn rgb_ops(c: [u8; 3]) -> String {
    format!(
        "{} {} {}",
        num(f32::from(c[0]) / 255.0),
        num(f32::from(c[1]) / 255.0),
        num(f32::from(c[2]) / 255.0)
    )
}

fn utf16_hex(s: &str) -> String {
    let mut out = String::new();
    for u in s.encode_utf16() {
        let _ = write!(out, "{u:04X}");
    }
    out
}

fn flate(data: &[u8]) -> Vec<u8> {
    miniz_oxide::deflate::compress_to_vec_zlib(data, 6)
}

fn jpeg_info(d: &[u8]) -> Option<(u32, u32, u8)> {
    if d.get(..2)? != [0xFF, 0xD8] {
        return None;
    }
    let mut i = 2;
    while i + 9 < d.len() {
        if d[i] != 0xFF {
            i += 1;
            continue;
        }
        let m = d[i + 1];
        if m == 0xFF {
            i += 1;
            continue;
        }
        let len = usize::from(be16(d, i + 2)?);
        if matches!(m, 0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF) {
            let (h, w, n) = (be16(d, i + 5)?, be16(d, i + 7)?, *d.get(i + 9)?);
            return (w > 0 && h > 0 && matches!(n, 1 | 3)).then_some((
                u32::from(w),
                u32::from(h),
                n,
            ));
        }
        i += 2 + len;
    }
    None
}

impl Page {
    /// A filled rectangle.
    pub fn fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32, rgb: [u8; 3]) {
        let _ = writeln!(
            self.content,
            "{} rg {} {} {} {} re f",
            rgb_ops(rgb),
            num(x),
            num(self.h - y - h),
            num(w),
            num(h)
        );
    }

    /// The outline of a rectangle.
    pub fn stroke_rect(&mut self, x: f32, y: f32, w: f32, h: f32, width: f32, rgb: [u8; 3]) {
        let _ = writeln!(
            self.content,
            "{} RG {} w {} {} {} {} re S",
            rgb_ops(rgb),
            num(width),
            num(x),
            num(self.h - y - h),
            num(w),
            num(h)
        );
    }

    /// A picture (from [`Pdf::add_rgb`] / [`Pdf::add_jpeg`]) in a box.
    pub fn image(&mut self, image: usize, x: f32, y: f32, w: f32, h: f32) {
        self.images.insert(image);
        let _ = writeln!(
            self.content,
            "q {} 0 0 {} {} {} cm /I{image} Do Q",
            num(w),
            num(h),
            num(x),
            num(self.h - y - h)
        );
    }

    pub fn size(&self) -> (f32, f32) {
        (self.w, self.h)
    }
}

impl Default for Pdf {
    fn default() -> Pdf {
        Pdf::new()
    }
}

impl Pdf {
    pub fn new() -> Pdf {
        Pdf {
            fonts: Vec::new(),
            images: Vec::new(),
            pages: Vec::new(),
            title: None,
        }
    }

    pub fn set_title(&mut self, title: &str) {
        self.title = Some(title.to_owned());
    }

    /// Registers a TrueType font (embedded whole when a glyph of it is used).
    pub fn add_font(&mut self, name: &str, data: &'static [u8]) -> Result<usize, PdfError> {
        let info = ttf_info(data).ok_or(PdfError::BadFont)?;
        let name: String = name.chars().filter(char::is_ascii_alphanumeric).collect();
        self.fonts.push(Font {
            data,
            name,
            info,
            used: BTreeMap::new(),
        });
        Ok(self.fonts.len() - 1)
    }

    /// Registers a picture of 8-bit RGB pixels.
    pub fn add_rgb(&mut self, w: u32, h: u32, rgb: Vec<u8>) -> Result<usize, PdfError> {
        if w == 0 || h == 0 || rgb.len() != (w as usize) * (h as usize) * 3 {
            return Err(PdfError::BadImage);
        }
        self.images.push(Image {
            w,
            h,
            data: ImageData::Rgb(rgb),
        });
        Ok(self.images.len() - 1)
    }

    /// Registers a JPEG file (embedded as it is).
    pub fn add_jpeg(&mut self, bytes: &[u8]) -> Result<usize, PdfError> {
        let (w, h, n) = jpeg_info(bytes).ok_or(PdfError::BadImage)?;
        self.images.push(Image {
            w,
            h,
            data: ImageData::Jpeg(bytes.to_vec(), n),
        });
        Ok(self.images.len() - 1)
    }

    /// Starts a page of `w` x `h` point; returns its number.
    pub fn add_page(&mut self, w: f32, h: f32) -> usize {
        self.pages.push(Page {
            w,
            h,
            content: String::new(),
            fonts: BTreeSet::new(),
            images: BTreeSet::new(),
        });
        self.pages.len() - 1
    }

    pub fn page(&mut self, i: usize) -> Option<&mut Page> {
        self.pages.get_mut(i)
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// One shaped glyph on a page: font number, glyph number, size in point, the pen position
    /// of its baseline (top-left origin), a slant (0 = upright, 0.2 = italic) and the text it
    /// stands for (for copying and searching).
    #[allow(clippy::too_many_arguments)]
    pub fn glyph(
        &mut self,
        page: usize,
        font: usize,
        gid: u16,
        size: f32,
        at: (f32, f32),
        slant: f32,
        rgb: [u8; 3],
        text: &str,
    ) {
        let Some(f) = self.fonts.get_mut(font) else {
            return;
        };
        f.used.entry(gid).or_insert_with(|| text.to_owned());
        let Some(p) = self.pages.get_mut(page) else {
            return;
        };
        p.fonts.insert(font);
        let _ = writeln!(
            p.content,
            "BT {} rg /F{font} {} Tf 1 0 {} 1 {} {} Tm <{gid:04X}> Tj ET",
            rgb_ops(rgb),
            num(size),
            num(slant),
            num(at.0),
            num(p.h - at.1)
        );
    }

    /// The finished file.
    pub fn finish(mut self) -> Vec<u8> {
        if self.pages.is_empty() {
            self.add_page(595.0, 842.0);
        }
        let mut w = Out::default();
        w.bytes.extend_from_slice(b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n");
        // Object numbers: 1 catalog, 2 page tree, 3 info; then fonts, images, pages.
        let (catalog, tree, info) = (1, 2, 3);
        let mut next = 4;
        let font_ids: Vec<usize> = (0..self.fonts.len()).map(|_| take(&mut next, 5)).collect();
        let image_ids: Vec<usize> = (0..self.images.len()).map(|_| take(&mut next, 1)).collect();
        let page_ids: Vec<usize> = (0..self.pages.len()).map(|_| take(&mut next, 2)).collect();
        w.object(catalog, &format!("<< /Type /Catalog /Pages {tree} 0 R >>"));
        let kids: Vec<String> = page_ids.iter().map(|p| format!("{p} 0 R")).collect();
        w.object(
            tree,
            &format!(
                "<< /Type /Pages /Count {} /Kids [{}] >>",
                page_ids.len(),
                kids.join(" ")
            ),
        );
        let title = self
            .title
            .as_deref()
            .map_or(String::new(), |t| format!("/Title <FEFF{}> ", utf16_hex(t)));
        w.object(info, &format!("<< {title}/Producer (ultar-office) >>"));
        for (f, id) in self.fonts.iter().zip(&font_ids) {
            write_font(&mut w, f, *id);
        }
        for (im, id) in self.images.iter().zip(&image_ids) {
            write_image(&mut w, im, *id);
        }
        for (p, id) in self.pages.iter().zip(&page_ids) {
            let res_f: String = p
                .fonts
                .iter()
                .map(|&i| format!("/F{i} {} 0 R ", font_ids[i]))
                .collect();
            let res_i: String = p
                .images
                .iter()
                .map(|&i| format!("/I{i} {} 0 R ", image_ids[i]))
                .collect();
            w.object(
                *id,
                &format!(
                    "<< /Type /Page /Parent {tree} 0 R /MediaBox [0 0 {} {}] /Contents {} 0 R /Resources << /Font << {res_f}>> /XObject << {res_i}>> >> >>",
                    num(p.w),
                    num(p.h),
                    id + 1
                ),
            );
            w.stream(id + 1, "", &flate(p.content.as_bytes()), true);
        }
        w.trailer(catalog, info)
    }
}

fn take(next: &mut usize, n: usize) -> usize {
    let at = *next;
    *next += n;
    at
}

#[derive(Default)]
struct Out {
    bytes: Vec<u8>,
    offsets: BTreeMap<usize, usize>,
}

impl Out {
    fn object(&mut self, id: usize, body: &str) {
        self.offsets.insert(id, self.bytes.len());
        self.bytes
            .extend_from_slice(format!("{id} 0 obj\n{body}\nendobj\n").as_bytes());
    }

    fn stream(&mut self, id: usize, dict: &str, data: &[u8], flated: bool) {
        self.offsets.insert(id, self.bytes.len());
        let filter = if flated { "/Filter /FlateDecode " } else { "" };
        self.bytes.extend_from_slice(
            format!(
                "{id} 0 obj\n<< {dict}{filter}/Length {} >>\nstream\n",
                data.len()
            )
            .as_bytes(),
        );
        self.bytes.extend_from_slice(data);
        self.bytes.extend_from_slice(b"\nendstream\nendobj\n");
    }

    fn trailer(mut self, root: usize, info: usize) -> Vec<u8> {
        let start = self.bytes.len();
        let size = self.offsets.keys().next_back().map_or(1, |m| m + 1);
        let mut x = format!("xref\n0 {size}\n0000000000 65535 f \n");
        for id in 1..size {
            let off = self.offsets.get(&id).copied().unwrap_or(0);
            let _ = writeln!(x, "{off:010} 00000 n ");
        }
        let _ = write!(x, "trailer\n<< /Size {size} /Root {root} 0 R /Info {info} 0 R >>\nstartxref\n{start}\n%%EOF\n");
        self.bytes.extend_from_slice(x.as_bytes());
        self.bytes
    }
}

fn write_font(w: &mut Out, f: &Font, id: usize) {
    let (desc, file, cmap, cid) = (id + 1, id + 2, id + 3, id + 4);
    let base = format!("{}{}", subset_tag(id), f.name);
    w.object(
        id,
        &format!("<< /Type /Font /Subtype /Type0 /BaseFont /{base} /Encoding /Identity-H /DescendantFonts [{cid} 0 R] /ToUnicode {cmap} 0 R >>"),
    );
    let t = &f.info;
    let b = t.bbox.map(|v| t.per_mille(i32::from(v)));
    w.object(
        desc,
        &format!(
            "<< /Type /FontDescriptor /FontName /{base} /Flags 4 /FontBBox [{} {} {} {}] /ItalicAngle 0 /Ascent {} /Descent {} /CapHeight {} /StemV 80 /FontFile2 {file} 0 R >>",
            b[0],
            b[1],
            b[2],
            b[3],
            t.per_mille(i32::from(t.ascent)),
            t.per_mille(i32::from(t.descent)),
            t.per_mille(i32::from(t.ascent)) * 7 / 10,
        ),
    );
    w.stream(
        file,
        &format!("/Length1 {} ", f.data.len()),
        &flate(f.data),
        true,
    );
    w.stream(cmap, "", &flate(to_unicode(&f.used).as_bytes()), true);
    let widths: String = f
        .used
        .keys()
        .map(|&g| format!("{g} [{}] ", t.per_mille(i32::from(t.advance(f.data, g)))))
        .collect();
    w.object(
        cid,
        &format!(
            "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /{base} /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /FontDescriptor {desc} 0 R /DW 1000 /W [{widths}] /CIDToGIDMap /Identity >>"
        ),
    );
}

/// Six capital letters from the font number (the "subset tag" a PDF font name carries).
fn subset_tag(n: usize) -> String {
    (0..6)
        .map(|i| char::from(b'A' + ((n / 26usize.pow(i)) % 26) as u8))
        .collect::<String>()
        + "+"
}

fn to_unicode(used: &BTreeMap<u16, String>) -> String {
    let mut s = String::from(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n/CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
    );
    let all: Vec<(&u16, &String)> = used.iter().collect();
    for chunk in all.chunks(100) {
        let _ = writeln!(s, "{} beginbfchar", chunk.len());
        for (g, t) in chunk {
            let _ = writeln!(s, "<{g:04X}> <{}>", utf16_hex(t));
        }
        s.push_str("endbfchar\n");
    }
    s.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    s
}

fn write_image(w: &mut Out, im: &Image, id: usize) {
    match &im.data {
        ImageData::Rgb(px) => w.stream(
            id,
            &format!("/Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceRGB /BitsPerComponent 8 ", im.w, im.h),
            &flate(px),
            true,
        ),
        ImageData::Jpeg(bytes, n) => {
            let cs = if *n == 1 { "/DeviceGray" } else { "/DeviceRGB" };
            w.stream(
                id,
                &format!("/Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace {cs} /BitsPerComponent 8 /Filter /DCTDecode ", im.w, im.h),
                bytes,
                false,
            );
        }
    }
}

#[cfg(test)]
mod tests;
