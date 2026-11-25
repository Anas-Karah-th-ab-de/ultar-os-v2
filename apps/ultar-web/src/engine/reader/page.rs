//! A read page laid out for a width: each block wrapped in its own direction (Arabic from
//! the right), headings larger, list items with a bullet, links underlined; drawn for the
//! part that is scrolled into view, and asked where a link is.

use super::html::{Block, Document, Kind, Link};
use std::rc::Rc;
use ultar_i18n::bidi::direction_of;
use ultar_i18n::{Direction, Lang};
use ultar_ui::canvas::{Canvas, PixelFormat};
use ultar_ui::geom::Rect;
use ultar_ui::icon::IconCache;
use ultar_ui::paint::{PaintEnv, Painter, Resources, TextSpec};
use ultar_ui::text::{FontRole, Overflow, TextAlign, TextEngine, TextLayout, Weight};
use ultar_ui::tokens::{ColorRole, ThemeKind, TypeSize};

/// Widest column of text, in points (longer lines are hard to read).
const COLUMN: f32 = 760.0;
/// Space at the edges, in points.
const MARGIN: f32 = 20.0;
/// Space after a block, in points.
const GAP: f32 = 10.0;
/// Bullet before a list item.
const BULLET: &str = "• ";

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
            let x = if dir == Direction::Rtl {
                origin
            } else {
                origin + indent
            };
            let color = match block.kind {
                Kind::Quote => ColorRole::TextMuted,
                _ => ColorRole::Text,
            };
            let before = match block.kind {
                Kind::Heading(_) => GAP * s,
                _ => 0.0,
            };
            y += before;
            let height = layout.height;
            placed.push(Placed {
                y,
                x,
                w: box_w,
                layout,
                links,
                color,
            });
            y += height + GAP * s;
        }
        Laid {
            placed,
            height: y + MARGIN * s,
        }
    }

    /// Draws the part of `laid` from `scroll` (pixels) into a `width` × `height` canvas.
    pub fn draw(&mut self, laid: &Laid, width: u32, height: u32, scroll: f32) -> Option<Canvas> {
        let mut canvas = Canvas::new(width.max(1), height.max(1), PixelFormat::Bgrx8).ok()?;
        let env = self.env;
        let s = env.scale;
        {
            let mut p = Painter::new(&mut canvas, &mut self.resources, env);
            let ground = p.color(ColorRole::Surface);
            p.clear(ground);
            let link_color = p.color(ColorRole::AccentText);
            let bottom = scroll + height as f32;
            for b in laid
                .placed
                .iter()
                .filter(|b| b.y + b.layout.height >= scroll && b.y <= bottom)
            {
                let top = b.y - scroll;
                let rect = Rect::new(b.x / s, top / s, b.w / s, b.layout.height / s);
                let color = p.color(b.color);
                p.text(&b.layout, rect, TextAlign::Start, color);
                for (x0, x1, base) in link_spans(b) {
                    let y = top + base + 2.0 * s;
                    p.fill(
                        Rect::new(x0 / s, y / s, (x1 - x0) / s, 1.0),
                        None,
                        link_color,
                    );
                }
            }
        }
        canvas.take_damage();
        Some(canvas)
    }
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
        let canvas = p.draw(&laid, 800, 400, 0.0).unwrap();
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
