//! A read page laid out for a width: each block wrapped in its own direction (Arabic from
//! the right), headings larger, list items with a bullet, links underlined; drawn for the
//! part that is scrolled into view, and asked where a link is.

use super::html::{Block, Document, Kind, Link, MediaKind};
use std::rc::Rc;
use ultar_i18n::bidi::direction_of;
use ultar_i18n::{Direction, Lang};
use ultar_ui::canvas::{Canvas, PixelFormat};
use ultar_ui::geom::Rect;
use ultar_ui::icon::IconCache;
use ultar_ui::paint::{PaintEnv, Painter, Resources, TextSpec};
use ultar_ui::text::{FontRole, Overflow, TextAlign, TextEngine, TextLayout, Weight};
use ultar_ui::tokens::{ColorRole, ThemeKind, TypeSize};
use ultar_ui::Color;

/// Widest column of text, in points (longer lines are hard to read).
const COLUMN: f32 = 760.0;
/// Space at the edges, in points.
const MARGIN: f32 = 20.0;
/// Space after a block, in points.
const GAP: f32 = 10.0;
/// Bullet before a list item.
const BULLET: &str = "• ";
/// Highest video box, in points.
const VIDEO_MAX_H: f32 = 420.0;
/// Height of a sound bar, in points.
const AUDIO_H: f32 = 56.0;

struct Placed {
    /// Top of the block in page pixels.
    y: f32,
    /// Left edge of its box, in pixels.
    x: f32,
    /// Width of its box, in pixels.
    w: f32,
    layout: Rc<TextLayout>,
    links: Vec<Link>,
    color: ColorRole,
    /// A media block: what it plays, its file, its box height (pixels).
    media: Option<(MediaKind, String, f32)>,
}

impl Placed {
    /// Height on the page in pixels.
    fn height(&self) -> f32 {
        self.media.as_ref().map_or(self.layout.height, |m| m.2)
    }
}

/// What a media box shows: its state as text and how far it is (0–1), and the newest picture.
pub struct MediaView {
    /// "▶ Play", "Loading 30 %" … (empty while a picture plays).
    pub label: String,
    /// Part played or fetched.
    pub progress: Option<f32>,
    /// The picture of a video.
    pub picture: Option<std::sync::Arc<crate::media::Picture>>,
}

/// A media box on the page: its file and its rectangle in page pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct MediaBox {
    /// The file.
    pub src: String,
    /// What it plays.
    pub kind: MediaKind,
    /// Left, top, width, height in page pixels.
    pub rect: (f32, f32, f32, f32),
}

/// A page laid out for one width.
pub struct Laid {
    placed: Vec<Placed>,
    /// Height of the whole page in pixels.
    pub height: f32,
}

/// Fonts and the environment of the reader's pages.
pub struct Painting {
    resources: Resources,
    env: PaintEnv,
}

fn spec(kind: Kind) -> TextSpec {
    let s = match kind {
        Kind::Heading(1) => TextSpec::title(TypeSize::S32),
        Kind::Heading(2) => TextSpec::title(TypeSize::S24),
        Kind::Heading(_) => TextSpec::new(TypeSize::S20).weight(Weight::Bold),
        Kind::Pre => TextSpec::new(TypeSize::S14).role(FontRole::Mono),
        Kind::Para | Kind::Item | Kind::Quote => TextSpec::new(TypeSize::S16),
        Kind::Media(_) => TextSpec::new(TypeSize::S14),
    };
    s.overflow(Overflow::Wrap)
}

impl Painting {
    /// Fonts for pages in `lang` (line height, fallback direction).
    pub fn new(lang: Lang) -> Option<Painting> {
        Some(Painting {
            resources: Resources {
                text: TextEngine::new().ok()?,
                icons: IconCache::new().ok()?,
            },
            env: PaintEnv::new(lang, ThemeKind::Light),
        })
    }

    /// Lays `doc` out for `width` pixels at `scale`.
    pub fn lay_out(&mut self, doc: &Document, width: u32, scale: f32) -> Laid {
        self.env.scale = scale.max(0.5);
        let s = self.env.scale;
        let w = width as f32;
        let column = (w - 2.0 * MARGIN * s).min(COLUMN * s).max(40.0);
        let origin = ((w - column) / 2.0).max(0.0);
        let mut y = MARGIN * s;
        let mut placed = Vec::with_capacity(doc.blocks.len());
        for block in &doc.blocks {
            if matches!(block.kind, Kind::Heading(_)) {
                y += GAP * s;
            }
            let b = self.place(block, y, origin, column);
            y += b.height() + GAP * s;
            placed.push(b);
        }
        Laid {
            placed,
            height: y + MARGIN * s,
        }
    }

    /// One block at height `y` in the column from `origin`, `column` pixels wide.
    fn place(&mut self, block: &Block, y: f32, origin: f32, column: f32) -> Placed {
        let s = self.env.scale;
        let (text, links) = shown(block);
        let dir = direction_of(&text, Direction::Ltr);
        let indent = if block.kind == Kind::Quote {
            24.0 * s
        } else {
            0.0
        };
        let box_w = column - indent;
        let layout = ultar_ui::paint::layout_directed(
            &mut self.resources,
            &self.env,
            &text,
            spec(block.kind),
            Some(box_w / s),
            dir,
        );
        let color = match block.kind {
            Kind::Quote => ColorRole::TextMuted,
            _ => ColorRole::Text,
        };
        if let (Kind::Media(kind), Some(src)) = (block.kind, block.media_src()) {
            let h = match kind {
                MediaKind::Video => (column * 9.0 / 16.0).min(VIDEO_MAX_H * s),
                MediaKind::Audio => AUDIO_H * s,
            };
            return Placed {
                y,
                x: origin,
                w: column,
                layout,
                links: Vec::new(),
                color,
                media: Some((kind, src.to_owned(), h)),
            };
        }
        // a quote is indented at its start: on the right for Arabic
        let x = if dir == Direction::Rtl {
            origin
        } else {
            origin + indent
        };
        Placed {
            y,
            x,
            w: box_w,
            layout,
            links,
            color,
            media: None,
        }
    }

    /// Draws the part of `laid` from `scroll` (pixels) into a `width` × `height` canvas;
    /// `media` tells what each media box shows (by its file).
    pub fn draw(
        &mut self,
        laid: &Laid,
        (width, height): (u32, u32),
        scroll: f32,
        media: &dyn Fn(&str) -> MediaView,
    ) -> Option<Canvas> {
        let mut canvas = Canvas::new(width.max(1), height.max(1), PixelFormat::Bgrx8).ok()?;
        let env = self.env;
        let s = env.scale;
        {
            let mut p = Painter::new(&mut canvas, &mut self.resources, env);
            let ground = p.color(ColorRole::Surface);
            p.clear(ground);
            let bottom = scroll + height as f32;
            let visible = laid
                .placed
                .iter()
                .filter(|b| b.y + b.height() >= scroll && b.y <= bottom);
            for b in visible {
                let top = b.y - scroll;
                match &b.media {
                    Some((_, src, h)) => {
                        let rect = Rect::new(b.x / s, top / s, b.w / s, h / s);
                        draw_media(&mut p, rect, &b.layout, &media(src));
                    }
                    None => draw_text(&mut p, b, top),
                }
            }
        }
        canvas.take_damage();
        Some(canvas)
    }

    /// Draws `view` over the media box `b` of a drawn page canvas (a new picture of a playing
    /// video) and returns the changed area in canvas pixels.
    pub fn draw_media_box(
        &mut self,
        canvas: &mut Canvas,
        b: &MediaBox,
        scroll: f32,
        view: &MediaView,
        name: &str,
    ) -> Option<(u32, u32, u32, u32)> {
        let s = self.env.scale;
        let (x, y, w, h) = b.rect;
        let top = y - scroll;
        let (cw, ch) = (canvas.width() as f32, canvas.height() as f32);
        if top + h < 0.0 || top > ch {
            return None;
        }
        let mut p = Painter::new(canvas, &mut self.resources, self.env);
        let layout = p.layout(name, spec(Kind::Media(b.kind)), Some(w / s));
        draw_media(
            &mut p,
            Rect::new(x / s, top / s, w / s, h / s),
            &layout,
            view,
        );
        let clamp = |v: f32, max: f32| v.clamp(0.0, max) as u32;
        Some((
            clamp(x, cw),
            clamp(top, ch),
            clamp(x + w, cw),
            clamp(top + h, ch),
        ))
    }
}

/// A text block with its link underlines.
fn draw_text(p: &mut Painter<'_>, b: &Placed, top: f32) {
    let s = p.env().scale;
    let rect = Rect::new(b.x / s, top / s, b.w / s, b.layout.height / s);
    let color = p.color(b.color);
    p.text(&b.layout, rect, TextAlign::Start, color);
    let link_color = p.color(ColorRole::AccentText);
    for (x0, x1, base) in link_spans(b) {
        let y = top + base + 2.0 * s;
        p.fill(
            Rect::new(x0 / s, y / s, (x1 - x0) / s, 1.0),
            None,
            link_color,
        );
    }
}

/// A media box: dark ground, the picture fitted in, the state and the name over it, the bar.
fn draw_media(p: &mut Painter<'_>, rect: Rect, name: &TextLayout, view: &MediaView) {
    p.fill(rect, None, Color::rgb(16, 16, 20));
    if let Some(pic) = &view.picture {
        let (pw, ph) = (pic.width.max(1) as f32, pic.height.max(1) as f32);
        let k = (rect.w / pw).min(rect.h / ph);
        let (w, h) = (pw * k, ph * k);
        let fitted = Rect::new(
            rect.x + (rect.w - w) / 2.0,
            rect.y + (rect.h - h) / 2.0,
            w,
            h,
        );
        p.image(fitted, pic.width, pic.height, &pic.xrgb);
    }
    if !view.label.is_empty() {
        let s = p.env().scale;
        let label = p.layout(&view.label, TextSpec::title(TypeSize::S16), Some(rect.w));
        let (lh, nh) = (label.height / s, name.height / s);
        let at = Rect::new(rect.x, rect.y + (rect.h - lh - nh) / 2.0, rect.w, lh);
        p.text(&label, at, TextAlign::Center, Color::rgb(240, 240, 245));
        let below = Rect::new(rect.x, at.y + lh, rect.w, nh);
        p.text(name, below, TextAlign::Center, Color::rgb(170, 170, 180));
    }
    if let Some(done) = view.progress {
        let bar = Rect::new(
            rect.x,
            rect.y + rect.h - 4.0,
            rect.w * done.clamp(0.0, 1.0),
            4.0,
        );
        let accent = p.color(ColorRole::Accent);
        p.fill(bar, None, accent);
    }
}

/// The media boxes of the page.
pub fn media_boxes(laid: &Laid) -> Vec<MediaBox> {
    laid.placed
        .iter()
        .filter_map(|b| {
            let (kind, src, h) = b.media.as_ref()?;
            Some(MediaBox {
                src: src.clone(),
                kind: *kind,
                rect: (b.x, b.y, b.w, *h),
            })
        })
        .collect()
}

/// The media box at the page point `(x, y)`.
pub fn media_at(laid: &Laid, x: f32, y: f32) -> Option<MediaBox> {
    media_boxes(laid).into_iter().find(|m| {
        let (bx, by, bw, bh) = m.rect;
        x >= bx && x < bx + bw && y >= by && y < by + bh
    })
}

/// The text of a block as shown (list items get a bullet) and its links moved with it.
fn shown(block: &Block) -> (String, Vec<Link>) {
    if block.kind != Kind::Item {
        return (block.text.clone(), block.links.clone());
    }
    let shift = BULLET.len();
    let links = block
        .links
        .iter()
        .map(|l| Link {
            start: l.start + shift,
            end: l.end + shift,
            href: l.href.clone(),
        })
        .collect();
    (format!("{BULLET}{}", block.text), links)
}

/// Underlines of the links of `b`: left, right (pixels) and the baseline from the top.
fn link_spans(b: &Placed) -> Vec<(f32, f32, f32)> {
    let mut out = Vec::new();
    for line in &b.layout.lines {
        let origin = b.x + b.layout.line_offset(line, b.w, TextAlign::Start);
        for link in &b.links {
            let glyphs = b.layout.glyphs[line.glyphs.clone()]
                .iter()
                .filter(|g| (g.cluster as usize) >= link.start && (g.cluster as usize) < link.end);
            let (mut x0, mut x1) = (f32::MAX, f32::MIN);
            for g in glyphs {
                x0 = x0.min(origin + g.x);
                x1 = x1.max(origin + g.x + g.advance);
            }
            if x1 > x0 {
                out.push((x0, x1, line.baseline));
            }
        }
    }
    out
}

/// The link at the page point `(x, y)` (pixels from the page's top left), if any.
pub fn link_at(laid: &Laid, x: f32, y: f32) -> Option<String> {
    let b = laid
        .placed
        .iter()
        .find(|b| y >= b.y && y < b.y + b.layout.height)?;
    let line = b
        .layout
        .lines
        .iter()
        .find(|l| y - b.y >= l.top && y - b.y < l.top + l.height)?;
    let origin = b.x + b.layout.line_offset(line, b.w, TextAlign::Start);
    let g = b.layout.glyphs[line.glyphs.clone()]
        .iter()
        .find(|g| x >= origin + g.x && x < origin + g.x + g.advance.max(1.0))?;
    let at = g.cluster as usize;
    b.links
        .iter()
        .find(|l| at >= l.start && at < l.end)
        .map(|l| l.href.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::reader::html::read;

    #[test]
    fn a_page_is_laid_out_drawn_and_its_links_found() {
        let doc = read(
            "<h1>عنوان</h1><p>نص قبل <a href='/x'>الرابط</a> وبعده</p><ul><li><a href='https://e.org/'>English link</a></li></ul>",
            "https://h.org/",
        );
        let mut p = Painting::new(Lang::Ar).unwrap();
        let laid = p.lay_out(&doc, 800, 1.0);
        assert!(laid.height > 100.0);
        let none = |_: &str| MediaView {
            label: String::new(),
            progress: None,
            picture: None,
        };
        let canvas = p.draw(&laid, (800, 400), 0.0, &none).unwrap();
        assert_eq!(canvas.width(), 800);
        // every underline is on a link
        let mut found = Vec::new();
        for b in &laid.placed {
            for (x0, x1, base) in link_spans(b) {
                let hit = link_at(&laid, (x0 + x1) / 2.0, b.y + base - 4.0);
                found.push(hit.unwrap_or_default());
            }
        }
        assert_eq!(found, ["https://h.org/x", "https://e.org/"]);
        assert_eq!(link_at(&laid, 5.0, 5.0), None);
    }
}
