//! Software composition of one output: only the damaged rectangles are
//! redrawn, bottom to top, starting at the topmost surface that covers a
//! rectangle completely (nothing hidden is drawn).

use crate::decor::{TitleKey, TITLEBAR};
use crate::geom::{Rect, Region};
use crate::output::Output;
use crate::pixels::{self, CornerMask, Image, Shadow};
use crate::state::{Placed, State};
use std::collections::HashMap;
use ultar_ui::paint::PaintEnv;
use ultar_ui::tokens::{self, ColorRole, Radius};

pub use crate::cursor::{cursor_image, cursor_rect, CURSOR};

/// The composer: frames per output, cached pointer and corner masks.
#[derive(Debug, Default)]
pub struct Renderer {
    frames: HashMap<u32, Image>,
    /// Pointer images by output scale and pointer size.
    cursors: HashMap<(u32, ultar_surface::proto::PointerSize), Image>,
    masks: HashMap<u32, pixels::CornerMask>,
    shadow_cache: HashMap<u32, HashMap<ShadowKey, Shadow>>,
    /// Display number cards by (number, scale) while numbers are shown.
    numbers: HashMap<(u8, u32), Image>,
}

/// What a shadow depends on: device frame, blur, offset, opacity bits.
type ShadowKey = (Rect, i32, i32, u32);

/// Device rectangle of a logical one on an output.
pub fn to_device(o: &Output, r: Rect) -> Rect {
    r.translate(-o.x, -o.y).scale_round(o.scale())
}

impl Renderer {
    /// An empty renderer.
    pub fn new() -> Renderer {
        Renderer::default()
    }

    /// The last frame of an output.
    pub fn frame(&self, output: u32) -> Option<&Image> {
        self.frames.get(&output)
    }

    /// Forgets the frame of a removed output.
    pub fn forget(&mut self, output: u32) {
        self.frames.remove(&output);
        self.shadow_cache.remove(&output);
    }

    /// Composes the damage of `output` into its frame. Returns the damaged
    /// rectangles (empty: nothing to present).
    pub fn render(&mut self, state: &mut State, output: &Output) -> Vec<Rect> {
        state.refresh_culled();
        let mut damage = state.damage.remove(&output.id).unwrap_or_default();
        let screen = Rect::new(0, 0, output.width as i32, output.height as i32);
        let fresh = !self
            .frames
            .get(&output.id.0)
            .is_some_and(|f| f.width() == output.width && f.height() == output.height);
        if fresh {
            match Image::new(output.width, output.height, 0) {
                Some(img) => self.frames.insert(output.id.0, img),
                None => return Vec::new(),
            };
            damage = Region::new();
            damage.add(screen);
        }
        damage.clip(&screen);
        if damage.is_empty() {
            return Vec::new();
        }
        refresh_titles(state, output);
        self.refresh_numbers(state, output);
        let rects = damage.rects().to_vec();
        let Some(mut frame) = self.frames.remove(&output.id.0) else {
            return Vec::new();
        };
        let scene = state.scene();
        let shadows = self.shadows(state, output, &scene);
        for rect in &rects {
            for tile in tiles(rect) {
                self.compose(state, output, (&scene, &shadows), &mut frame, &tile);
            }
        }
        self.frames.insert(output.id.0, frame);
        rects
    }

    /// Composes the whole output into a new image (reference for tests:
    /// incremental composition must give the same pixels).
    pub fn render_full(&mut self, state: &mut State, output: &Output) -> Option<Image> {
        let mut frame = Image::new(output.width, output.height, 0)?;
        refresh_titles(state, output);
        self.refresh_numbers(state, output);
        let scene = state.scene();
        let shadows = self.shadows(state, output, &scene);
        let clip = frame.bounds();
        self.compose(state, output, (&scene, &shadows), &mut frame, &clip);
        Some(frame)
    }

    fn compose(
        &mut self,
        state: &State,
        o: &Output,
        (scene, shadows): (&[Placed], &[Option<Shadow>]),
        frame: &mut Image,
        clip: &Rect,
    ) {
        let start = scene
            .iter()
            .rposition(|p| covers(state, o, p, clip))
            .unwrap_or(0);
        if start == 0 && !scene.first().is_some_and(|p| covers(state, o, p, clip)) {
            let ground = state.decor.as_ref().map_or(0xff00_0000, |d| d.ground());
            pixels::fill(frame, *clip, clip, ground);
        }
        let radius = (Radius::Card.points() as f64 * o.scale()).round() as u32;
        let mask = self
            .masks
            .entry(radius)
            .or_insert_with(|| pixels::CornerMask::new(radius))
            .clone();
        let mut dimmed = false;
        for (placed, shadow) in scene[start..].iter().zip(&shadows[start..]) {
            let secure = placed
                .window
                .and_then(|i| state.windows.get(i))
                .is_some_and(|w| w.secure);
            if secure && !dimmed {
                // Everything below a system dialog is dimmed: apps cannot
                // dim the shell's panels, so the dialog is recognisable.
                dimmed = true;
                let scrim = state.decor.as_ref().map_or(0x8000_0000, |d| d.scrim());
                if let Some(tile) = Image::new(1, 1, scrim) {
                    pixels::scale(frame, *clip, clip, &tile, false);
                }
            }
            paint(state, o, placed, frame, clip, (&mask, shadow.as_ref()));
        }
        if state.session.indicator_shown {
            paint_indicator(state, o, frame, clip);
        }
        self.paint_number(state, o, frame, clip);
        if let Some(preview) = state.pointer.snap_preview {
            let env = PaintEnv::new(state.config.lang, state.config.theme);
            let accent = crate::decor::to_pixel(
                env.palette()
                    .get(ColorRole::Accent)
                    .with_alpha(tokens::state::PRESSED_OVERLAY),
            );
            let d = to_device(o, preview);
            if let Some(r) = d.intersect(clip) {
                let tile = Image::new(1, 1, accent);
                if let Some(tile) = tile {
                    pixels::scale(frame, r, clip, &tile, false);
                }
            }
        }
        self.paint_cursor(state, o, frame, clip);
    }

    /// The shadow of every floating window of the scene; profiles are kept
    /// while the window keeps its frame (only the shadows of this frame are
    /// kept, so the cache is bounded by the number of windows).
    fn shadows(&mut self, state: &State, o: &Output, scene: &[Placed]) -> Vec<Option<Shadow>> {
        let blur = (f64::from(tokens::shadow::BLUR) * o.scale()) as i32;
        let off = (f64::from(tokens::shadow::OFFSET_Y) * o.scale()) as i32;
        let alpha = state.decor.as_ref().map_or(0.0, |d| d.shadow_alpha());
        let mut cache = self.shadow_cache.remove(&o.id.0).unwrap_or_default();
        let mut kept = HashMap::new();
        let list = scene
            .iter()
            .map(|p| {
                let w = p.window.and_then(|i| state.windows.get(i))?;
                if !w.floating() {
                    return None;
                }
                let key = (to_device(o, w.frame), blur, off, alpha.to_bits());
                let shadow = cache
                    .remove(&key)
                    .or_else(|| kept.get(&key).cloned())
                    .unwrap_or_else(|| Shadow::new(key.0, blur, off, alpha));
                kept.insert(key, shadow.clone());
                Some(shadow)
            })
            .collect();
        self.shadow_cache.insert(o.id.0, kept);
        list
    }

    /// Makes the number cards of `o` (and drops them when none is shown).
    fn refresh_numbers(&mut self, state: &mut State, o: &Output) {
        let Some(number) = number_of(state, o) else {
            if state.session.manage.identify.is_none() {
                self.numbers.clear();
            }
            return;
        };
        let key = (number, o.scale_milli);
        if self.numbers.contains_key(&key) {
            return;
        }
        if let Some(img) = state
            .decor
            .as_mut()
            .and_then(|d| d.render_number(number, o.scale_milli))
        {
            self.numbers.insert(key, img);
        }
    }

    /// The display's number in the middle of the output (`display.identify`).
    fn paint_number(&self, state: &State, o: &Output, frame: &mut Image, clip: &Rect) {
        let Some(img) = number_of(state, o).and_then(|n| self.numbers.get(&(n, o.scale_milli)))
        else {
            return;
        };
        let x = (o.width as i32 - img.width() as i32) / 2;
        let y = (o.height as i32 - img.height() as i32) / 2;
        pixels::blend(frame, x, y, img, img.bounds(), clip);
    }

    fn paint_cursor(&mut self, state: &State, o: &Output, frame: &mut Image, clip: &Rect) {
        if !state.config.cursor || state.pointer.hidden {
            return;
        }
        let at = to_device(
            o,
            Rect::new(
                state.pointer.x.floor() as i32,
                state.pointer.y.floor() as i32,
                1,
                1,
            ),
        );
        if !Rect::new(0, 0, o.width as i32, o.height as i32).contains(at.x, at.y) {
            return;
        }
        let env = PaintEnv::new(state.config.lang, state.config.theme);
        let size = state.pointer.size;
        let key = (o.scale_milli, size);
        if let std::collections::hash_map::Entry::Vacant(slot) = self.cursors.entry(key) {
            let fill = crate::decor::to_pixel(env.palette().get(ColorRole::Text));
            let outline = crate::decor::to_pixel(env.palette().get(ColorRole::Ground));
            if let Some(img) = cursor_image(o.scale() * size.factor(), fill, outline) {
                slot.insert(img);
            }
        }
        if let Some(img) = self.cursors.get(&key) {
            pixels::blend(frame, at.x, at.y, img, img.bounds(), clip);
        }
    }
}

/// The number shown on an output now, if any.
fn number_of(state: &State, o: &Output) -> Option<u8> {
    let identify = state.session.manage.identify.as_ref()?;
    identify
        .numbers
        .iter()
        .find(|(id, _)| *id == o.id)
        .map(|(_, n)| *n)
}

/// Draws the recording indicator (a dot in the danger colour).
fn paint_indicator(state: &State, o: &Output, frame: &mut Image, clip: &Rect) {
    let at = to_device(o, crate::cursor::indicator_rect(o, state.rtl()));
    if at.intersect(clip).is_none() {
        return;
    }
    let env = PaintEnv::new(state.config.lang, state.config.theme);
    let color = crate::decor::to_pixel(env.palette().get(ColorRole::Danger));
    if let Some(dot) = crate::cursor::dot_image(at.w.max(1) as u32, color) {
        pixels::blend(frame, at.x, at.y, &dot, dot.bounds(), clip);
    }
}

/// Paints one placed surface (with shadow and title bar for windows).
fn paint(
    state: &State,
    o: &Output,
    placed: &Placed,
    frame: &mut Image,
    clip: &Rect,
    (mask, shadow): (&CornerMask, Option<&Shadow>),
) {
    let Some(surface) = state.surface(placed.key) else {
        return;
    };
    let Some(image) = surface.image.as_ref() else {
        return;
    };
    let content = to_device(o, placed.content);
    if content.intersect(clip).is_none() && placed.window.is_none() {
        return;
    }
    let window = placed.window.and_then(|i| state.windows.get(i));
    let corners = window.filter(|w| w.floating()).map(|_| mask);
    if let Some(w) = window {
        let f = to_device(o, w.frame);
        if let Some(shadow) = shadow {
            shadow.paint(frame, clip, &f);
        }
        if let Some((_, title)) = w.title.cache.as_ref().filter(|_| w.decorated()) {
            let t = Rect::new(f.x, f.y, f.w, content.y - f.y);
            let piece = Piece {
                dest: t,
                src: title,
                opaque: true,
            };
            paint_part(frame, clip, &piece, corners.map(|m| (m, false)));
        }
    }
    if image.width() as i32 == content.w && image.height() as i32 == content.h {
        let piece = Piece {
            dest: content,
            src: image,
            opaque: surface.opaque,
        };
        paint_part(frame, clip, &piece, corners.map(|m| (m, true)));
    } else {
        pixels::scale(frame, content, clip, image, surface.opaque);
    }
}

/// Pixels to put somewhere.
struct Piece<'a> {
    dest: Rect,
    src: &'a Image,
    opaque: bool,
}

/// Paints one part of a window (title bar or content); `corners` rounds
/// the top corners, or the bottom ones when its flag is set.
fn paint_part(
    frame: &mut Image,
    clip: &Rect,
    piece: &Piece<'_>,
    corners: Option<(&CornerMask, bool)>,
) {
    let dest = piece.dest;
    let Some(visible) = dest.intersect(clip) else {
        return;
    };
    let draw = |frame: &mut Image, area: Rect| {
        if let Some(a) = area.intersect(&visible) {
            let src_rect = a.translate(-dest.x, -dest.y);
            if piece.opaque {
                pixels::copy(frame, a.x, a.y, piece.src, src_rect, &a);
            } else {
                pixels::blend(frame, a.x, a.y, piece.src, src_rect, &a);
            }
        }
    };
    let Some((mask, bottom)) = corners else {
        draw(frame, dest);
        return;
    };
    let n = mask.radius() as i32;
    if dest.h < n || dest.w < 2 * n {
        draw(frame, dest);
        return;
    }
    // Rows with corners: middle strip; other rows: everything.
    let (corner_y, rest_y) = if bottom {
        (dest.max_y() - n, dest.y)
    } else {
        (dest.y, dest.y + n)
    };
    draw(frame, Rect::new(dest.x + n, corner_y, dest.w - 2 * n, n));
    draw(frame, Rect::new(dest.x, rest_y, dest.w, dest.h - n));
    mask.apply(frame, dest, &visible, piece.src, bottom);
}

/// Width and height of the tiles a damaged rectangle is composed in: small
/// enough that a tile is often covered by one window (nothing below it is
/// drawn), large enough that rows stay long.
const TILE: (i32, i32) = (512, 64);

/// Splits a rectangle into tiles.
fn tiles(rect: &Rect) -> impl Iterator<Item = Rect> + '_ {
    let rows = (rect.h + TILE.1 - 1) / TILE.1;
    let cols = (rect.w + TILE.0 - 1) / TILE.0;
    (0..rows).flat_map(move |r| {
        (0..cols).map(move |c| {
            let x = rect.x + c * TILE.0;
            let y = rect.y + r * TILE.1;
            Rect::new(
                x,
                y,
                TILE.0.min(rect.max_x() - x),
                TILE.1.min(rect.max_y() - y),
            )
        })
    })
}

/// Whether `placed` hides everything below it inside `clip`.
fn covers(state: &State, o: &Output, placed: &Placed, clip: &Rect) -> bool {
    let Some(s) = state.surface(placed.key) else {
        return false;
    };
    if !s.opaque || s.image.is_none() {
        return false;
    }
    let content = to_device(o, placed.content);
    let window = placed.window.and_then(|i| state.windows.get(i));
    let solid = match window {
        Some(w) if w.floating() => {
            let r = (Radius::Card.points() as f64 * o.scale()).ceil() as i32;
            Rect::new(content.x, content.y, content.w, content.h - r)
        }
        _ => content,
    };
    let scaled = s
        .image
        .as_ref()
        .is_some_and(|i| i.width() as i32 != content.w || i.height() as i32 != content.h);
    !scaled && solid.contains_rect(clip)
}

/// Renders the title bars of visible windows on `o` whose look changed.
fn refresh_titles(state: &mut State, o: &Output) {
    let focus = state.focus;
    let mut work = Vec::new();
    for (i, w) in state.windows.iter().enumerate() {
        if !w.visible() || !w.decorated() || w.frame.intersect(&o.logical_rect()).is_none() {
            continue;
        }
        let title = state
            .surface(w.key)
            .map(|s| s.title.clone())
            .unwrap_or_default();
        let width = to_device(o, w.frame).w.max(1) as u32;
        let key = TitleKey {
            width,
            scale_milli: o.scale_milli,
            title,
            focused: focus == Some(w.key),
            hover: w.title.buttons.hover,
            pressed: w.title.buttons.pressed,
            maximized: w.state == ultar_surface::proto::WindowState::Maximized,
            unresponsive: w.unresponsive,
            secure: w.secure,
        };
        if w.title.cache.as_ref().is_none_or(|(k, _)| *k != key) {
            work.push((i, key));
        }
    }
    for (i, key) in work {
        let image = state.decor.as_mut().and_then(|d| d.render(&key));
        if let (Some(image), Some(w)) = (image, state.windows.get_mut(i)) {
            w.title.cache = Some((key, image));
        }
    }
    let _ = TITLEBAR;
}
