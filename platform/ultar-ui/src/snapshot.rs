//! Image-comparison tests: render a widget tree to PNG and compare it with a reference image.
//!
//! Reference images live in a directory chosen by the caller (by convention
//! `tests/snapshots/`). A mismatch writes the actual image and a diff image next to them under
//! `failed/` and reports the number of differing pixels. With `ULTAR_UPDATE_SNAPSHOTS=1` in
//! the environment the references are rewritten instead.

use crate::canvas::{Canvas, CanvasError, PixelFormat};
use crate::geom::Size;
use crate::paint::PaintEnv;
use crate::tokens::{ColorRole, InputMode, ThemeKind, DARK};
use crate::ui::{Ui, UiError};
use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use ultar_i18n::{Lang, ParseError, Translator};

/// Environment variable that makes [`Snapshots::assert_snapshot`] rewrite references.
pub const UPDATE_ENV: &str = "ULTAR_UPDATE_SNAPSHOTS";

/// Why a snapshot check failed.
#[derive(Debug)]
pub enum SnapshotError {
    /// The name contains characters other than `a-z`, `0-9`, `-`, `_`.
    InvalidName(String),
    /// Building or rendering the tree failed.
    Ui(UiError),
    /// The canvas could not be created.
    Canvas(CanvasError),
    /// Reading or writing a file failed.
    Io {
        /// The file.
        path: PathBuf,
        /// The cause.
        source: io::Error,
    },
    /// A PNG file is corrupt or not 8-bit RGBA.
    Corrupt {
        /// The file.
        path: PathBuf,
        /// What the decoder reported.
        detail: String,
    },
    /// The image could not be encoded.
    Encode(String),
    /// No reference image exists (run once with `ULTAR_UPDATE_SNAPSHOTS=1`).
    MissingReference(PathBuf),
    /// The rendered image differs from the reference.
    Mismatch {
        /// The reference file.
        reference: PathBuf,
        /// Number of differing pixels (all pixels when the sizes differ).
        differing: usize,
        /// Where the diff image was written.
        diff: PathBuf,
    },
}

impl fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SnapshotError::InvalidName(name) => write!(f, "invalid snapshot name {name:?}"),
            SnapshotError::Ui(e) => write!(f, "snapshot tree: {e}"),
            SnapshotError::Canvas(e) => write!(f, "snapshot canvas: {e}"),
            SnapshotError::Io { path, source } => write!(f, "{}: {source}", path.display()),
            SnapshotError::Corrupt { path, detail } => {
                write!(f, "{}: corrupt image ({detail})", path.display())
            }
            SnapshotError::Encode(detail) => write!(f, "PNG encoding failed: {detail}"),
            SnapshotError::MissingReference(path) => {
                write!(
                    f,
                    "no reference image {} (set {UPDATE_ENV}=1 to create it)",
                    path.display()
                )
            }
            SnapshotError::Mismatch {
                reference,
                differing,
                diff,
            } => write!(
                f,
                "{} differs in {differing} pixels, diff image: {}",
                reference.display(),
                diff.display()
            ),
        }
    }
}

impl std::error::Error for SnapshotError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SnapshotError::Io { source, .. } => Some(source),
            SnapshotError::Ui(e) => Some(e),
            SnapshotError::Canvas(e) => Some(e),
            _ => None,
        }
    }
}

impl From<UiError> for SnapshotError {
    fn from(e: UiError) -> Self {
        SnapshotError::Ui(e)
    }
}

impl From<CanvasError> for SnapshotError {
    fn from(e: CanvasError) -> Self {
        SnapshotError::Canvas(e)
    }
}

/// An RGBA image in memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// `width * height * 4` bytes, RGBA.
    pub rgba: Vec<u8>,
}

impl Image {
    /// The pixels of a canvas.
    pub fn from_canvas(canvas: &Canvas) -> Image {
        Image {
            width: canvas.width(),
            height: canvas.height(),
            rgba: canvas.to_rgba(),
        }
    }

    /// Encodes the image as PNG.
    pub fn encode_png(&self) -> Result<Vec<u8>, SnapshotError> {
        let mut out = Vec::new();
        let mut encoder = png::Encoder::new(&mut out, self.width, self.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Best);
        let encode = |e: png::EncodingError| SnapshotError::Encode(e.to_string());
        let mut writer = encoder.write_header().map_err(encode)?;
        writer.write_image_data(&self.rgba).map_err(encode)?;
        writer.finish().map_err(encode)?;
        Ok(out)
    }

    /// Decodes an 8-bit RGBA PNG; anything else is reported as corrupt. `path` only labels
    /// the error.
    pub fn decode_png(bytes: &[u8], path: &Path) -> Result<Image, SnapshotError> {
        let corrupt = |detail: String| SnapshotError::Corrupt {
            path: path.to_path_buf(),
            detail,
        };
        let mut reader = png::Decoder::new(bytes)
            .read_info()
            .map_err(|e| corrupt(e.to_string()))?;
        let mut rgba = vec![0u8; reader.output_buffer_size()];
        let info = reader
            .next_frame(&mut rgba)
            .map_err(|e| corrupt(e.to_string()))?;
        if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
            return Err(corrupt(format!(
                "{:?} {:?}, expected 8-bit RGBA",
                info.color_type, info.bit_depth
            )));
        }
        rgba.truncate(info.buffer_size());
        if rgba.len() != info.width as usize * info.height as usize * 4 {
            return Err(corrupt("pixel data has the wrong length".to_string()));
        }
        Ok(Image {
            width: info.width,
            height: info.height,
            rgba,
        })
    }

    /// Compares with `reference`: the number of differing pixels and a diff image in which
    /// equal pixels are dimmed and differing ones are marked. Different sizes count as all
    /// pixels differing.
    pub fn diff(&self, reference: &Image) -> (usize, Image) {
        let mark = DARK.get(ColorRole::Danger);
        let mut out = self.clone();
        if (self.width, self.height) != (reference.width, reference.height) {
            for px in out.rgba.chunks_exact_mut(4) {
                px.copy_from_slice(&[mark.r, mark.g, mark.b, u8::MAX]);
            }
            return (self.rgba.len() / 4, out);
        }
        let mut differing = 0;
        for (px, expected) in out
            .rgba
            .chunks_exact_mut(4)
            .zip(reference.rgba.chunks_exact(4))
        {
            if px == expected {
                px[3] /= 4;
            } else {
                differing += 1;
                px.copy_from_slice(&[mark.r, mark.g, mark.b, u8::MAX]);
            }
        }
        (differing, out)
    }
}

/// Writes `bytes` to `path` atomically: temporary file in the same directory, flushed to
/// disk, then renamed over the target.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), SnapshotError> {
    let fail = |source: io::Error| SnapshotError::Io {
        path: path.to_path_buf(),
        source,
    };
    let dir = path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(dir).map_err(fail)?;
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("snapshot");
    let temp = dir.join(format!(".{file_name}.{}.tmp", std::process::id()));
    let result = (|| {
        let mut file = fs::File::create(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temp, path)?;
        // Make the rename itself durable where the platform allows opening a directory.
        if let Ok(handle) = fs::File::open(dir) {
            let _ = handle.sync_all();
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result.map_err(fail)
}

/// Renders the whole UI into a fresh RGBA canvas of its size times its scale.
pub fn render(ui: &mut Ui) -> Result<Canvas, SnapshotError> {
    let scale = ui.env().scale;
    let width = (ui.size().w * scale).round() as u32;
    let height = (ui.size().h * scale).round() as u32;
    let mut canvas = Canvas::new(width, height, PixelFormat::Rgba8)?;
    ui.paint_all(&mut canvas);
    Ok(canvas)
}

/// A directory of reference images and the catalogs the snapshot trees use.
pub struct Snapshots {
    dir: PathBuf,
    update: bool,
    catalogs: Vec<(Lang, String)>,
    mode: InputMode,
}

impl Snapshots {
    /// References in `dir`; rewriting is enabled by [`UPDATE_ENV`]`=1`.
    pub fn new(dir: impl Into<PathBuf>) -> Snapshots {
        Snapshots {
            dir: dir.into(),
            update: std::env::var(UPDATE_ENV).is_ok_and(|v| v == "1"),
            catalogs: Vec::new(),
            mode: InputMode::Pointer,
        }
    }

    /// Forces rewriting on or off regardless of the environment.
    pub fn update(mut self, update: bool) -> Snapshots {
        self.update = update;
        self
    }

    /// Adds a catalog (ultar-i18n format) the trees may use.
    pub fn catalog(mut self, lang: Lang, source: &str) -> Snapshots {
        self.catalogs.push((lang, source.to_string()));
        self
    }

    /// Renders with touch target sizes instead of pointer sizes.
    pub fn input_mode(mut self, mode: InputMode) -> Snapshots {
        self.mode = mode;
        self
    }

    /// A fresh UI of `size` for a language and theme with this set's catalogs.
    pub fn ui(&self, size: Size, lang: Lang, theme: ThemeKind) -> Result<Ui, SnapshotError> {
        let mut translator = Translator::empty(lang);
        for (catalog_lang, source) in &self.catalogs {
            translator
                .add_source(*catalog_lang, source)
                .map_err(|e: ParseError| SnapshotError::Ui(UiError::Catalog(e)))?;
        }
        Ok(Ui::new(
            PaintEnv::new(lang, theme),
            self.mode,
            size,
            translator,
        )?)
    }

    /// The reference file of a snapshot.
    pub fn path(&self, name: &str, lang: Lang, theme: ThemeKind) -> PathBuf {
        self.dir
            .join(format!("{name}-{}-{}.png", lang.code(), theme.name()))
    }

    /// Builds a tree with `tree`, renders it at `size` for `lang` and `theme`, and compares
    /// the result with the reference image `<name>-<lang>-<theme>.png`.
    pub fn assert_snapshot(
        &self,
        name: &str,
        tree: impl FnOnce(&mut Ui) -> Result<(), UiError>,
        size: Size,
        lang: Lang,
        theme: ThemeKind,
    ) -> Result<(), SnapshotError> {
        let mut ui = self.ui(size, lang, theme)?;
        tree(&mut ui)?;
        self.assert_ui(name, &mut ui)
    }

    /// Compares an already built (and possibly interacted-with) UI with its reference.
    pub fn assert_ui(&self, name: &str, ui: &mut Ui) -> Result<(), SnapshotError> {
        let valid = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_';
        if name.is_empty() || !name.chars().all(valid) {
            return Err(SnapshotError::InvalidName(name.to_string()));
        }
        let (lang, theme) = (ui.env().lang, ui.env().theme);
        let actual = Image::from_canvas(&render(ui)?);
        let reference = self.path(name, lang, theme);
        if self.update {
            return write_atomic(&reference, &actual.encode_png()?);
        }
        let bytes = match fs::read(&reference) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Err(SnapshotError::MissingReference(reference))
            }
            Err(source) => {
                return Err(SnapshotError::Io {
                    path: reference,
                    source,
                })
            }
        };
        let expected = Image::decode_png(&bytes, &reference)?;
        let (differing, diff_image) = actual.diff(&expected);
        if differing == 0 {
            return Ok(());
        }
        let stem = format!("{name}-{}-{}", lang.code(), theme.name());
        let failed = self.dir.join("failed");
        let diff = failed.join(format!("{stem}.diff.png"));
        write_atomic(&diff, &diff_image.encode_png()?)?;
        write_atomic(
            &failed.join(format!("{stem}.actual.png")),
            &actual.encode_png()?,
        )?;
        Err(SnapshotError::Mismatch {
            reference,
            differing,
            diff,
        })
    }
}
