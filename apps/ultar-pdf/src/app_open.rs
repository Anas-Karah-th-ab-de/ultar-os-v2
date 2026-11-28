//! Opening documents in the reader: from a path or URI, again with a password, or failing.

use crate::app::{App, Out};
use crate::doc::Doc;
use crate::layout::Layout;
use crate::place::{self, ReadError};
use ultar_ui::ui::UiError;

impl App {
    /// Opens the file `entry` names (a `files://` URI or `area/name` path).
    pub fn open_entry(&mut self, entry: &str) -> Result<(), UiError> {
        self.ask = None;
        let doc = match place::uri_of_entry(entry) {
            Some(uri) => App::open(Some(&uri), &self.areas),
            None => Doc::unreadable(None, ReadError::Missing),
        };
        self.replace_doc(doc)
    }

    /// Opens the same file again with the password the person typed.
    pub fn retry_password(&mut self, password: &str) -> Result<(), UiError> {
        let doc = self.doc.with_password(password);
        self.ask = None;
        self.replace_doc(doc)?;
        self.tried_password = true;
        Ok(())
    }

    /// Swaps in a freshly started document and resets the view.
    pub(crate) fn replace_doc(&mut self, doc: Doc) -> Result<(), UiError> {
        self.tried_password = false;
        self.view.start_page = doc.uri.as_deref().map_or(0, place::page_of);
        if doc.read_error.is_none() {
            if let Some(u) = &doc.uri {
                self.recents.push(place::without_page(u));
                self.recents.save(&self.data);
            }
        }
        self.doc = doc;
        self.pick.selection = None;
        self.pick.drag = None;
        self.find.hits.clear();
        self.view.scroll = 0.0;
        self.view.scroll_x = 0.0;
        self.view.page = self.view.start_page;
        self.view.layout = Layout::default();
        self.rail.thumbs = Layout::default();
        self.rail.scroll = 0.0;
        self.rebuild()?;
        let title = self.title();
        self.out.push(Out::Title(title));
        Ok(())
    }

    /// Shows why a file did not open (the empty state names the reason).
    pub fn open_failed(&mut self, why: ReadError) -> Result<(), UiError> {
        let uri = self.doc.uri.clone();
        self.doc = Doc::unreadable(uri, why);
        self.rebuild()
    }
}
