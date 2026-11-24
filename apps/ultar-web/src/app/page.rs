//! The page in the window: its pixels in the page picture, its size following the window,
//! and files of finished downloads.

use super::{AppError, Browser};
use crate::engine::{DownloadId, FrameDamage, TabId, WebEngine};
use crate::model::downloads::State;
use ultar_ui::canvas::Canvas;
use ultar_ui::geom::PixelRect;

impl<E: WebEngine> Browser<E> {
    fn shown(&self) -> Option<TabId> {
        self.state
            .tabs
            .active()
            .filter(|t| !t.on_start_page())
            .map(|t| t.engine)
    }

    /// Copies the whole newest frame of the shown tab into the page picture.
    pub(super) fn show_page_frame(&mut self) -> Result<(), AppError> {
        let (Some(tab), Some(page)) = (self.shown(), self.chrome.page) else {
            return Ok(());
        };
        let Some(frame) = self.engine.frame(tab) else {
            return Ok(());
        };
        let row = frame.width as usize * 4;
        let mut packed = Vec::with_capacity(row * frame.height as usize);
        for y in 0..frame.height as usize {
            let start = y * frame.stride;
            match frame.pixels.get(start..start + row) {
                Some(line) => packed.extend_from_slice(line),
                None => return Ok(()),
            }
        }
        let canvas = Canvas::from_bgrx(frame.width, frame.height, packed)
            .map_err(|_| ultar_ui::ui::UiError::InvalidSize)?;
        self.ui.set_picture(page, canvas)?;
        Ok(())
    }

    pub(super) fn frame_arrived(&mut self, tab: TabId, d: FrameDamage) -> Result<(), AppError> {
        if self.shown() != Some(tab) {
            return Ok(());
        }
        let Some(page) = self.chrome.page else {
            return Ok(());
        };
        let size = self.engine.frame(tab).map(|f| (f.width, f.height));
        if size.is_none() || self.ui.picture_size(page) != size {
            return self.show_page_frame();
        }
        let Some(frame) = self.engine.frame(tab) else {
            return Ok(());
        };
        let start = d.y0 as usize * frame.stride + d.x0 as usize * 4;
        let Some(rows) = frame.pixels.get(start..) else {
            return Ok(());
        };
        let area = PixelRect::new(d.x0 as i32, d.y0 as i32, d.x1 as i32, d.y1 as i32);
        let stride = frame.stride;
        let rows = rows.to_vec();
        self.ui.write_picture(page, area, &rows, stride)?;
        Ok(())
    }

    /// Tells the engine the size of the page area (device pixels) when it changed.
    pub(super) fn resize_page(&mut self) -> Result<(), AppError> {
        let (Some(tab), Some(page)) = (self.shown(), self.chrome.page) else {
            return Ok(());
        };
        self.ui.layout();
        let Some(rect) = self.ui.rect(page) else {
            return Ok(());
        };
        let scale = self.ui.env().scale;
        let (w, h) = (
            (rect.w * scale).round().max(1.0) as u32,
            (rect.h * scale).round().max(1.0) as u32,
        );
        if self.page_size != Some((tab, w, h)) {
            self.page_size = Some((tab, w, h));
            self.engine.resize(tab, w, h, scale)?;
        }
        Ok(())
    }

    /// Opens a finished download in a new tab (the page cell can read the downloads folder).
    pub fn open_download(&mut self, id: DownloadId) -> Result<(), AppError> {
        let Some(paths) = self.paths.clone() else {
            return Ok(());
        };
        let done = self
            .state
            .downloads
            .get(id)
            .is_some_and(|d| d.state == State::Done);
        let Some(path) = self
            .state
            .downloads
            .path(&paths.downloads, id)
            .filter(|_| done)
        else {
            return Ok(());
        };
        let encoded: String = path
            .to_string_lossy()
            .split('/')
            .map(crate::url::encode_path_segment)
            .collect::<Vec<_>>()
            .join("/");
        self.new_tab(false)?;
        let Some(tab) = self.state.tabs.active_mut() else {
            return Ok(());
        };
        let url = format!("file://{encoded}");
        tab.url = url.clone();
        let engine = tab.engine;
        self.engine.load(engine, &url)?;
        self.refresh()
    }

    /// Shows a finished download in the Files app.
    pub fn show_download(&mut self, id: DownloadId) -> Result<(), AppError> {
        let Some(paths) = self.paths.clone() else {
            return Ok(());
        };
        if let Some(path) = self.state.downloads.path(&paths.downloads, id) {
            if self.handoff.show_in_folder(&path).is_err() {
                self.notify("web.handoff.missing");
            }
        }
        Ok(())
    }
}
