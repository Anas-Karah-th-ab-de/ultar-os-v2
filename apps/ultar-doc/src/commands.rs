//! The commands of the tool bar: which exist, which the three layout classes show, their
//! texts, whether they are "on" at the caret, and what they do.

use crate::app::App;
use crate::dialogs::{Ask, Purpose};
use crate::state::Doc;
use ultar_office::doc::{Align, Block, Dir, Para, Style};
use ultar_ui::widgets::Label;

/// One command of the tool bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Cmd {
    New,
    Save,
    SaveAs,
    Undo,
    Redo,
    Bold,
    Italic,
    Underline,
    Strike,
    SizeDown,
    SizeUp,
    Color,
    Highlight,
    Normal,
    H1,
    H2,
    H3,
    Title,
    AlignStart,
    AlignCenter,
    AlignEnd,
    AlignJustify,
    Spacing,
    Bullet,
    Number,
    Indent,
    Outdent,
    DirToggle,
    Table,
    RowAdd,
    RowDel,
    ColAdd,
    ColDel,
    TableDel,
    Picture,
    PicBigger,
    PicSmaller,
    PageBreak,
    Paper,
    ZoomIn,
    ZoomOut,
    PageToggle,
    Find,
    Pdf,
    More,
}

use Cmd::*;

/// All commands in tool bar order.
pub const ALL: [Cmd; 45] = [
    New,
    Save,
    Undo,
    Redo,
    Bold,
    Italic,
    Underline,
    Bullet,
    Number,
    DirToggle,
    Find,
    Strike,
    SizeDown,
    SizeUp,
    Color,
    Highlight,
    Normal,
    H1,
    H2,
    H3,
    Title,
    AlignStart,
    AlignCenter,
    AlignEnd,
    AlignJustify,
    Spacing,
    Indent,
    Outdent,
    ZoomIn,
    ZoomOut,
    PageToggle,
    SaveAs,
    Table,
    RowAdd,
    RowDel,
    ColAdd,
    ColDel,
    TableDel,
    Picture,
    PicBigger,
    PicSmaller,
    PageBreak,
    Paper,
    Pdf,
    More,
];

/// The three layout classes of the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    Compact,
    Medium,
    Wide,
}

impl Class {
    /// The class of a window `w` points wide.
    pub fn of(w: f32) -> Class {
        if w < 480.0 {
            Class::Compact
        } else if w < 800.0 {
            Class::Medium
        } else {
            Class::Wide
        }
    }

    fn tiers(self) -> u8 {
        match self {
            Class::Compact => 0,
            Class::Medium => 1,
            Class::Wide => 2,
        }
    }
}

/// 0 = on every class, 1 = medium and wide, 2 = wide only (the others reach it through "More").
fn tier(cmd: Cmd) -> u8 {
    match cmd {
        New | Save | Undo | Redo | Bold | Italic | Underline | Bullet | Number | DirToggle
        | Find | More => 0,
        Strike | SizeDown | SizeUp | Color | Highlight | Normal | H1 | H2 | H3 | Title
        | AlignStart | AlignCenter | AlignEnd | AlignJustify | Spacing | Indent | Outdent
        | ZoomIn | ZoomOut | PageToggle => 1,
        _ => 2,
    }
}

/// The commands the tool bar of `class` shows (with "More" unless everything fits).
pub fn shown(class: Class) -> Vec<Cmd> {
    let limit = class.tiers();
    ALL.iter()
        .copied()
        .filter(|&c| tier(c) <= limit && (c != More || class != Class::Wide))
        .collect()
}

/// The commands "More" holds in `class`.
pub fn more(class: Class) -> Vec<Cmd> {
    ALL.iter()
        .copied()
        .filter(|&c| tier(c) > class.tiers())
        .collect()
}

/// The text key of a command.
pub fn key(cmd: Cmd) -> &'static str {
    match cmd {
        New => "doc.new",
        Save => "doc.save",
        SaveAs => "doc.saveas",
        Undo => "doc.undo",
        Redo => "doc.redo",
        Bold => "doc.bold",
        Italic => "doc.italic",
        Underline => "doc.underline",
        Strike => "doc.strike",
        SizeDown => "doc.size.down",
        SizeUp => "doc.size.up",
        Color => "doc.color",
        Highlight => "doc.highlight",
        Normal => "doc.style.normal",
        H1 => "doc.style.h1",
        H2 => "doc.style.h2",
        H3 => "doc.style.h3",
        Title => "doc.style.title",
        AlignStart => "doc.align.start",
        AlignCenter => "doc.align.center",
        AlignEnd => "doc.align.end",
        AlignJustify => "doc.align.justify",
        Spacing => "doc.spacing",
        Bullet => "doc.list.bullet",
        Number => "doc.list.number",
        Indent => "doc.indent",
        Outdent => "doc.outdent",
        DirToggle => "doc.dir.toggle",
        Table => "doc.table",
        RowAdd => "doc.table.row_add",
        RowDel => "doc.table.row_del",
        ColAdd => "doc.table.col_add",
        ColDel => "doc.table.col_del",
        TableDel => "doc.table.del",
        Picture => "doc.picture",
        PicBigger => "doc.picture.bigger",
        PicSmaller => "doc.picture.smaller",
        PageBreak => "doc.pagebreak",
        Paper => "doc.paper",
        ZoomIn => "doc.zoom.in",
        ZoomOut => "doc.zoom.out",
        PageToggle => "doc.page",
        Find => "doc.find",
        Pdf => "doc.pdf",
        More => "doc.more",
    }
}

impl Doc {
    /// The paragraph the caret is in (none inside a table cell or at a picture).
    pub fn caret_para(&self) -> Option<&Para> {
        if self.cursor.cell.is_some() {
            return None;
        }
        match self.document.blocks.get(self.cursor.block) {
            Some(Block::Para(p)) => Some(p),
            _ => None,
        }
    }

    /// `true` when the command is "on" at the caret (shown as a pressed button).
    pub fn is_on(&self, cmd: Cmd) -> bool {
        let look = self.look_at_caret();
        let p = self.caret_para();
        match cmd {
            Bold => look.bold,
            Italic => look.italic,
            Underline => look.underline,
            Strike => look.strike,
            Bullet => p.is_some_and(|p| p.style == Style::Bullet),
            Number => p.is_some_and(|p| p.style == Style::Number),
            Normal => p.is_some_and(|p| p.style == Style::Normal),
            H1 => p.is_some_and(|p| p.style == Style::H1),
            H2 => p.is_some_and(|p| p.style == Style::H2),
            H3 => p.is_some_and(|p| p.style == Style::H3),
            Title => p.is_some_and(|p| p.style == Style::Title),
            AlignStart => p.is_some_and(|p| p.align == Align::Start),
            AlignCenter => p.is_some_and(|p| p.align == Align::Center),
            AlignEnd => p.is_some_and(|p| p.align == Align::End),
            AlignJustify => p.is_some_and(|p| p.align == Align::Justify),
            DirToggle => p.is_some_and(Para::rtl),
            PageToggle => self.page_view,
            Paper => self.is_letter(),
            _ => false,
        }
    }

    /// A number that changes when any `is_on` state may have (the tool bar is redrawn then).
    pub fn format_key(&self) -> u64 {
        ALL.iter()
            .enumerate()
            .fold(0u64, |acc, (i, &c)| acc | (u64::from(self.is_on(c)) << i))
            ^ (self
                .look_at_caret()
                .size_pt
                .map_or(0, |s| (s * 10.0) as u64)
                << 48)
    }
}

impl App {
    /// Runs a tool bar command.
    pub fn do_cmd(&mut self, cmd: Cmd) -> Result<(), ultar_ui::ui::UiError> {
        match cmd {
            New => return self.new_doc(),
            Save => return self.save(false),
            SaveAs => {
                return self.ask_for(Ask::Name {
                    then_close: false,
                    purpose: Purpose::Save,
                })
            }
            Find => return self.ask_for(Ask::Find),
            Pdf => {
                return self.ask_for(Ask::Name {
                    then_close: false,
                    purpose: Purpose::Pdf,
                })
            }
            Picture => {
                return self.ask_for(Ask::Name {
                    then_close: false,
                    purpose: Purpose::Picture,
                })
            }
            Color => return self.ask_for(Ask::Color),
            Highlight => return self.ask_for(Ask::Highlight),
            More => return self.ask_for(Ask::More),
            Undo | Redo => {
                let done = if cmd == Undo {
                    self.doc.undo()
                } else {
                    self.doc.redo()
                };
                return if done { self.changed() } else { Ok(()) };
            }
            ZoomIn => self.zoom(1.25),
            ZoomOut => self.zoom(0.8),
            PageToggle => self.doc.page_view = !self.doc.page_view,
            _ => {
                self.doc.burst_break();
                if !self.edit_cmd(cmd) {
                    return Ok(());
                }
            }
        }
        self.doc.burst_break();
        self.changed()
    }

    /// Commands that change the document; `false` when they could not (a message says why).
    fn edit_cmd(&mut self, cmd: Cmd) -> bool {
        let d = &mut self.doc;
        match cmd {
            Bold => d.toggle_run(|r| r.bold, |r, v| r.bold = v),
            Italic => d.toggle_run(|r| r.italic, |r, v| r.italic = v),
            Underline => d.toggle_run(|r| r.underline, |r, v| r.underline = v),
            Strike => d.toggle_run(|r| r.strike, |r, v| r.strike = v),
            SizeUp => d.change_size(1.0),
            SizeDown => d.change_size(-1.0),
            Normal => d.set_style(Style::Normal),
            H1 => d.set_style(Style::H1),
            H2 => d.set_style(Style::H2),
            H3 => d.set_style(Style::H3),
            Title => d.set_style(Style::Title),
            AlignStart => d.set_align(Align::Start),
            AlignCenter => d.set_align(Align::Center),
            AlignEnd => d.set_align(Align::End),
            AlignJustify => d.set_align(Align::Justify),
            Spacing => {
                let next = match d.caret_para().and_then(|p| p.line_spacing) {
                    None => Some(1.15),
                    Some(s) if s < 1.3 => Some(1.5),
                    Some(s) if s < 1.8 => Some(2.0),
                    _ => None,
                };
                d.set_line_spacing(next);
            }
            Bullet => d.toggle_list(true),
            Number => d.toggle_list(false),
            Indent => d.indent(36.0),
            Outdent => d.indent(-36.0),
            DirToggle => {
                let rtl = d.caret_para().is_some_and(Para::rtl);
                d.set_dir(if rtl { Dir::Ltr } else { Dir::Rtl });
            }
            Table => d.insert_table(3, 3),
            PageBreak => d.insert_page_break(),
            Paper => {
                let letter = d.is_letter();
                d.set_paper(!letter);
            }
            RowAdd | RowDel | ColAdd | ColDel | TableDel => return self.table_cmd(cmd),
            PicBigger | PicSmaller => {
                let factor = if cmd == PicBigger { 1.25 } else { 0.8 };
                if !d.resize_picture(factor) {
                    let _ = self.say(Label::key("doc.picture.need"));
                    return false;
                }
            }
            _ => return false,
        }
        true
    }

    fn table_cmd(&mut self, cmd: Cmd) -> bool {
        if !self.doc.in_table() {
            let _ = self.say(Label::key("doc.table.need"));
            return false;
        }
        let done = match cmd {
            RowAdd => self.doc.table_add_row(),
            RowDel => self.doc.table_remove_row(),
            ColAdd => self.doc.table_add_col(),
            ColDel => self.doc.table_remove_col(),
            _ => self.doc.table_delete(),
        };
        if !done {
            let _ = self.say(Label::key("doc.table.last"));
        }
        done
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_classes_show_more_and_more_commands_and_the_rest_sits_in_more() {
        let (c, m, w) = (
            shown(Class::Compact),
            shown(Class::Medium),
            shown(Class::Wide),
        );
        assert!(c.len() < m.len() && m.len() < w.len());
        assert!(c.contains(&Bold) && c.contains(&Find) && c.contains(&More));
        assert!(!w.contains(&More));
        assert!(more(Class::Wide).is_empty());
        for class in [Class::Compact, Class::Medium, Class::Wide] {
            let mut every: Vec<Cmd> = shown(class);
            every.retain(|&c| c != More);
            every.extend(more(class));
            assert_eq!(every.len(), ALL.len() - 1, "{class:?}: nothing is lost");
        }
    }

    #[test]
    fn every_command_has_a_key_and_the_keys_differ() {
        let mut keys: Vec<&str> = ALL.iter().map(|&c| key(c)).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), ALL.len());
    }

    #[test]
    fn class_limits() {
        assert_eq!(Class::of(360.0), Class::Compact);
        assert_eq!(Class::of(600.0), Class::Medium);
        assert_eq!(Class::of(1280.0), Class::Wide);
    }

    #[test]
    fn on_states_follow_the_caret() {
        let mut d = Doc::new();
        d.insert("x");
        assert!(!d.is_on(Bold) && d.is_on(Normal));
        d.select_all();
        d.toggle_run(|r| r.bold, |r, v| r.bold = v);
        assert!(d.is_on(Bold));
        let before = d.format_key();
        d.set_style(Style::H1);
        assert_ne!(before, d.format_key());
    }
}
