//! The three things a file-name question can do: save the document under a new name, write a
//! PDF of it, or insert a picture file. Names are looked up in the Documents area only (see
//! [`crate::place`]); an existing file is never overwritten.

use crate::action::Out;
use crate::app::App;
use crate::dialogs::{Ask, Purpose};
use crate::picture::{is_jpeg, is_png, jpeg_size, png_size};
use crate::state::{write_replacing, DocError};
use crate::{place, recovery};
use ultar_office::doc::Picture;
use ultar_ui::ui::UiError;
use ultar_ui::widgets::Label;

/// Largest picture file taken (bytes).
const MAX_PICTURE: u64 = 16 * 1024 * 1024;
/// Widest a picture is first shown (point, about the text width of A4).
const FIRST_WIDTH_PT: f32 = 400.0;

impl App {
    /// The name typed into the question was confirmed.
    pub(crate) fn submit_name(
        &mut self,
        then_close: bool,
        purpose: Purpose,
    ) -> Result<(), UiError> {
        let name = self.field(self.nodes.name_field);
        let result = match purpose {
            Purpose::Save => self.save_named(&name),
            Purpose::Pdf => self.export_pdf(&name),
            Purpose::Picture => self.insert_picture_file(&name),
        };
        self.ask = None;
        match result {
            Ok(label) => {
                self.message = Some(label);
                self.touch_title();
                if then_close && purpose == Purpose::Save {
                    self.io.out.push(Out::Close);
                }
            }
            Err(e) => {
                let key = if purpose == Purpose::Save {
                    "doc.save_failed"
                } else {
                    "doc.file_failed"
                };
                self.message = Some(self.reason(key, &name, e));
            }
        }
        if purpose == Purpose::Picture {
            self.lay = None;
        }
        self.rebuild()
    }

    fn save_named(&mut self, name: &str) -> Result<Label, DocError> {
        let path = place::new_path(name, &self.areas).ok_or(DocError::BadName)?;
        let old = self.doc.path.clone();
        self.doc.save_as(&path)?;
        recovery::remove(old.as_deref(), &self.areas);
        recovery::remove(None, &self.areas);
        self.io.timer.reset();
        self.open_uri = place::uri_of(&path, &self.areas);
        let shown = self.doc.name().unwrap_or_default();
        Ok(Label::key_with("doc.saved", &[("name", &shown)]))
    }

    fn export_pdf(&mut self, name: &str) -> Result<Label, DocError> {
        let path = place::with_ext(name, "pdf", &self.areas).ok_or(DocError::BadName)?;
        if path.exists() {
            return Err(DocError::Exists);
        }
        let title = self.doc.name().unwrap_or_default();
        let bytes = crate::export::to_pdf(&self.doc, &title).map_err(|_| DocError::Io)?;
        write_replacing(&path, &bytes)?;
        let shown = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        Ok(Label::key_with("doc.pdf.done", &[("name", &shown)]))
    }

    fn insert_picture_file(&mut self, name: &str) -> Result<Label, DocError> {
        let path = place::existing(name, &self.areas).ok_or(DocError::BadName)?;
        let size = std::fs::metadata(&path)
            .map_err(|e| DocError::of(&e))?
            .len();
        if size > MAX_PICTURE {
            return Err(DocError::TooBig);
        }
        let bytes = std::fs::read(&path).map_err(|e| DocError::of(&e))?;
        let (ext, px) = if is_png(&bytes) {
            ("png", png_size(&bytes))
        } else if is_jpeg(&bytes) {
            ("jpg", jpeg_size(&bytes))
        } else {
            return Err(DocError::Unsupported);
        };
        let (w, h) = px.ok_or(DocError::Unsupported)?;
        // 96 pixels per inch: one pixel is 0.75 point; first shown no wider than the text.
        let k = (0.75_f32).min(FIRST_WIDTH_PT / w as f32);
        let count = self
            .doc
            .document
            .blocks
            .iter()
            .filter(|b| matches!(b, ultar_office::doc::Block::Picture(_)))
            .count();
        let pic = Picture {
            bytes,
            name: format!("image{}.{ext}", count + 1),
            width_pt: Some(w as f32 * k),
            height_pt: Some(h as f32 * k),
        };
        self.doc.burst_break();
        self.doc.insert_picture(pic);
        self.doc.burst_break();
        Ok(Label::key("doc.picture.done"))
    }

    /// The asking dialog for a picture/PDF/name when nothing else is open.
    pub fn ask_name(&mut self, purpose: Purpose) -> Result<(), UiError> {
        self.ask_for(Ask::Name {
            then_close: false,
            purpose,
        })
    }
}
