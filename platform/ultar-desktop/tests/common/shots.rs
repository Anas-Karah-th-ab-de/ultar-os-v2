//! Composing and comparing pictures of the shell's surfaces: the snapshot tests draw every
//! surface like the compositor does and compare it with its reference in `tests/snapshots`.
//! `ULTAR_UPDATE_SNAPSHOTS=1` writes the references; `ULTAR_SHOTS_DIR=dir` also writes
//! every picture there (side-by-side with the boards).

use super::render;
use std::path::PathBuf;
use ultar_desktop::view::View;
use ultar_i18n::Lang;
use ultar_ui::geom::Size;
use ultar_ui::snapshot::Image;
use ultar_ui::tokens::{ColorRole, ThemeKind, DARK, LIGHT};

/// A screen of `size` in the ground colour.
pub fn blank_of(theme: ThemeKind, size: Size) -> Image {
    let g = ground(theme);
    Image {
        width: size.w as u32,
        height: size.h as u32,
        rgba: g.repeat((size.w * size.h) as usize),
    }
}

pub const VARIANTS: [(Lang, ThemeKind); 4] = [
    (Lang::Ar, ThemeKind::Dark),
    (Lang::Ar, ThemeKind::Light),
    (Lang::En, ThemeKind::Dark),
    (Lang::En, ThemeKind::Light),
];

pub fn ground(theme: ThemeKind) -> [u8; 4] {
    let c = if theme == ThemeKind::Dark {
        DARK
    } else {
        LIGHT
    }
    .get(ColorRole::Ground);
    [c.r, c.g, c.b, 255]
}

/// Draws `view` (RGBA, premultiplied) over `screen` at (x, y).
pub fn over(screen: &mut Image, view: &mut View, size: Size, (x, y): (i32, i32)) {
    let c = render(view, size);
    let src = c.to_rgba();
    for row in 0..c.height() as i32 {
        for col in 0..c.width() as i32 {
            let (sx, sy) = (x + col, y + row);
            if sx < 0 || sy < 0 || sx >= screen.width as i32 || sy >= screen.height as i32 {
                continue;
            }
            let s = ((row as u32 * c.width() + col as u32) * 4) as usize;
            let d = ((sy as u32 * screen.width + sx as u32) * 4) as usize;
            let a = u32::from(src[s + 3]);
            for k in 0..3 {
                let v = u32::from(src[s + k]) + u32::from(screen.rgba[d + k]) * (255 - a) / 255;
                screen.rgba[d + k] = v.min(255) as u8;
            }
        }
    }
}

pub fn check(name: &str, lang: Lang, theme: ThemeKind, img: &Image, problems: &mut Vec<String>) {
    let file = format!(
        "{name}-{}-{}.png",
        if lang == Lang::Ar { "ar" } else { "en" },
        if theme == ThemeKind::Dark {
            "dark"
        } else {
            "light"
        }
    );
    let png = img.encode_png().unwrap();
    if let Ok(dir) = std::env::var("ULTAR_SHOTS_DIR") {
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(PathBuf::from(dir).join(&file), &png).unwrap();
    }
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/snapshots")
        .join(&file);
    if std::env::var("ULTAR_UPDATE_SNAPSHOTS").is_ok() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &png).unwrap();
        return;
    }
    match std::fs::read(&path) {
        Ok(bytes) => {
            let reference = Image::decode_png(&bytes, &path).unwrap();
            let (n, _) = img.diff(&reference);
            if n > 0 {
                problems.push(format!("{file}: {n} pixels differ"));
            }
        }
        Err(_) => problems.push(format!("{file}: no reference")),
    }
}
