//! What every click does (docs/ui/CLICKS.md, sections PhoneDial, PhoneIncoming, PhoneInCall).
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

use crate::app::PhoneApp;
use crate::click::{menu, Click, OUTPUTS};
use crate::model::{Dialog, Filter, Page, Screen};
use crate::port::{CallPort, Host, Request};
use crate::text::{output_key, TEXTS};
use ultar_bus::messages::call::{
    Answer, Block, ClearLog, Decline, Dial, Dtmf, Forget, Hangup, Hold, Mute, NextCamera, Others,
    Phase, SendText, SetOutput, SetVideo, Share, Way,
};
use ultar_ui::event::ActionKind;
use ultar_ui::icon::IconId;
use ultar_ui::ui::NodeId;
use ultar_ui::widgets::{Label, Menu, MenuItem, Tone};

impl<P: CallPort, H: Host> PhoneApp<P, H> {
    /// Sends a request; failures become a toast. True when the model changed.
    pub(crate) fn send(&mut self, request: Request) -> bool {
        match self.port.send(request) {
            Ok(_) => false,
            Err(e) => {
                self.report(e);
                true
            }
        }
    }

    fn dial(&mut self, way: Way, number: String, video: bool) -> bool {
        match self.port.send(Request::Dial(Dial { way, number, video })) {
            Ok(_) => {
                self.model.digits.clear();
                self.model.dismissed = None;
                true
            }
            Err(e) => {
                self.report(e);
                true
            }
        }
    }

    /// Carries out `click` on `node`; true when the tree must be rebuilt.
    pub(crate) fn on_click(&mut self, node: NodeId, click: Click, kind: ActionKind) -> bool {
        match (click, kind) {
            (Click::Way, ActionKind::Selected(i)) => {
                let way = if i == 0 {
                    Way::Ultar
                } else {
                    self.model.cellular_way()
                };
                self.model.choose_way(way);
                true
            }
            (Click::Filter, ActionKind::Selected(i)) => {
                self.model.filter = if i == 0 { Filter::All } else { Filter::Missed };
                true
            }
            (Click::Page, ActionKind::Selected(i)) => {
                self.model.page = if i == 0 { Page::Dial } else { Page::Log };
                true
            }
            (Click::Number, ActionKind::TextChanged) => {
                let text = self.ui.text(node).map(str::to_owned).unwrap_or_default();
                let before = self.model.number();
                self.model.set_typed(&text);
                // Rebuild only when the field must show something else than what was typed.
                self.model.number() != text || before != self.model.number()
            }
            (Click::Number, ActionKind::Submitted) => self.call(false),
            // The backspace inside the number field (CLICKS.md `phone.backspace`).
            (Click::Number, ActionKind::Activated) => self.model.backspace(),
            (_, ActionKind::Activated) => self.activate(node, click),
            _ => false,
        }
    }

    fn call(&mut self, video: bool) -> bool {
        if !self.model.can_call() || (video && !self.model.can_video()) {
            return false;
        }
        let way = self.model.current_way();
        let number = self.model.number();
        self.dial(way, number, video)
    }

    fn activate(&mut self, node: NodeId, click: Click) -> bool {
        match click {
            Click::CopyNumber => self.copy_number(),
            Click::Window(op) => {
                self.host.window(op);
                false
            }
            Click::Backspace => self.model.backspace(),
            Click::Suggest => {
                let number = self.model.suggestion().map(|e| e.number.clone());
                number.is_some_and(|n| {
                    self.model.set_typed(&n);
                    true
                })
            }
            Click::Digit(key) => self.digit(key),
            Click::CallVideo => self.call(true),
            Click::Call => self.call(false),
            Click::Settings => {
                self.host.open_settings();
                false
            }
            Click::Callback(id) => self.callback(id),
            Click::EntryMore(id) => self.open_entry_menu(node, id),
            Click::Output => self.open_output_menu(node),
            _ => self.call_control(click),
        }
    }

    fn call_control(&mut self, click: Click) -> bool {
        let muted = self.model.state.audio.muted;
        match click {
            Click::Accept(id) => self.send(Request::Answer(Answer {
                id,
                others: Others::Hold,
            })),
            Click::AcceptEnd(id) => self.send(Request::Answer(Answer {
                id,
                others: Others::End,
            })),
            Click::Decline(id) => self.send(Request::Decline(Decline { id })),
            Click::DeclineMessage(id) => self.decline_with_message(id),
            Click::Mute => self.send(Request::Mute(Mute { on: !muted })),
            Click::Keypad => {
                self.model.keypad = true;
                true
            }
            Click::Hold(id) => {
                let on = self
                    .model
                    .call(id)
                    .is_some_and(|c| c.phase == Phase::Active);
                self.send(Request::Hold(Hold { id, on }))
            }
            Click::Swap(id) => self.send(Request::Hold(Hold { id, on: false })),
            Click::Video(id) => {
                let on = !self.model.call(id).is_some_and(|c| c.media.camera_on);
                self.send(Request::Video(SetVideo { id, on }))
            }
            Click::Camera(id) => self.send(Request::Camera(NextCamera { id })),
            Click::Share(id) => self.share(id),
            Click::ShareConfirm(id) => {
                self.model.dialog = None;
                self.send(Request::Share(Share { id, on: true }));
                true
            }
            Click::Hangup(id) => self.send(Request::Hangup(Hangup { id })),
            Click::Again => self.again(),
            Click::Text(i) => self.send_ready_text(i),
            Click::CloseDialog => self.close_dialog(),
            _ => false,
        }
    }

    fn copy_number(&mut self) -> bool {
        let Some(own) = self.model.state.lines.own_number.clone() else {
            return false;
        };
        if self.host.copy_text(&own) {
            self.notify(Tone::Success, Label::key("phone.copied"));
        } else {
            self.notify(Tone::Danger, Label::key("phone.error.generic"));
        }
        true
    }

    fn digit(&mut self, key: char) -> bool {
        match self.model.screen() {
            Screen::InCall(id) if self.model.keypad => {
                let tone = u8::try_from(key).unwrap_or(b'0');
                let failed = self.send(Request::Dtmf(Dtmf { id, key: tone }));
                if !failed {
                    self.model.tones.push(key);
                }
                true
            }
            Screen::Dial => self.model.push_key(key),
            _ => false,
        }
    }

    fn callback(&mut self, id: u32) -> bool {
        let Some(entry) = self.model.log.iter().find(|e| e.id == id).cloned() else {
            return false;
        };
        self.dial(entry.way, entry.number, false)
    }

    fn again(&mut self) -> bool {
        let Some(ended) = self.model.state.ended.clone() else {
            return false;
        };
        self.model.dismissed = Some(ended.id);
        self.dial(ended.way, ended.number, ended.media.video_call)
    }

    fn share(&mut self, id: u32) -> bool {
        if self.model.call(id).is_some_and(|c| c.media.sharing) {
            return self.send(Request::Share(Share { id, on: false }));
        }
        // CLICKS.md: the permission prompt comes first; sharing starts on "Share".
        self.model.dialog = Some(Dialog::Share(id));
        true
    }

    fn decline_with_message(&mut self, id: u32) -> bool {
        let Some(call) = self.model.call(id).cloned() else {
            return false;
        };
        self.send(Request::Decline(Decline { id }));
        let name = crate::model::Model::who(&call).to_owned();
        self.model.dialog = Some(Dialog::Text {
            way: call.way,
            number: call.number,
            name,
        });
        true
    }

    fn send_ready_text(&mut self, index: usize) -> bool {
        let (Some(Dialog::Text { way, number, .. }), Some(key)) =
            (self.model.dialog.clone(), TEXTS.get(index))
        else {
            return false;
        };
        let text = self.tr.t(key).into_owned();
        self.model.dialog = None;
        match self
            .port
            .send(Request::Text(SendText { way, number, text }))
        {
            Ok(_) => self.notify(Tone::Success, Label::key("phone.text.sent")),
            Err(_) => self.notify(Tone::Danger, Label::key("phone.error.text")),
        }
        true
    }

    pub(crate) fn close_dialog(&mut self) -> bool {
        let open = self.model.dialog.is_some() || self.model.keypad;
        self.model.dialog = None;
        self.model.keypad = false;
        self.model.tones.clear();
        open
    }

    fn open_entry_menu(&mut self, node: NodeId, id: u32) -> bool {
        let items = vec![
            MenuItem::new(menu::CONTACT, Label::key("phone.entry.contact")).icon(IconId::User),
            MenuItem::new(menu::BLOCK, Label::key("phone.entry.block")),
            MenuItem::new(menu::DELETE, Label::key("phone.entry.delete"))
                .icon(IconId::Trash)
                .danger()
                .separated(),
            MenuItem::new(menu::CLEAR, Label::key("phone.entry.clear"))
                .icon(IconId::Trash)
                .danger(),
        ];
        if self.ui.open_menu(node, Menu::new(items)).is_ok() {
            self.menu = Some(Click::EntryMore(id));
        }
        false
    }

    fn open_output_menu(&mut self, node: NodeId) -> bool {
        let audio = &self.model.state.audio;
        let items = OUTPUTS
            .iter()
            .enumerate()
            .filter(|(_, o)| audio.outputs.contains(o))
            .map(|(i, o)| {
                let id = menu::OUTPUT + u32::try_from(i).unwrap_or(0);
                MenuItem::new(id, Label::key(output_key(*o))).checked(audio.output == *o)
            })
            .collect();
        if self.ui.open_menu(node, Menu::new(items)).is_ok() {
            self.menu = Some(Click::Output);
        }
        false
    }

    pub(crate) fn menu_chosen(&mut self, item: u32) -> bool {
        match self.menu.take() {
            Some(Click::Output) => {
                let index = usize::try_from(item.saturating_sub(menu::OUTPUT)).unwrap_or(0);
                match OUTPUTS.get(index) {
                    Some(o) => self.send(Request::Output(SetOutput { output: *o })),
                    None => false,
                }
            }
            Some(Click::EntryMore(id)) => self.entry_action(id, item),
            _ => false,
        }
    }

    fn entry_action(&mut self, id: u32, item: u32) -> bool {
        let Some(entry) = self.model.log.iter().find(|e| e.id == id).cloned() else {
            return false;
        };
        match item {
            menu::CONTACT => {
                self.host.create_contact(&entry.number);
                false
            }
            menu::BLOCK => {
                let r = self.port.send(Request::Block(Block {
                    number: entry.number,
                    on: true,
                }));
                match r {
                    Ok(_) => self.notify(Tone::Success, Label::key("phone.blocked")),
                    Err(e) => self.report(e),
                }
                true
            }
            menu::CLEAR => {
                if let Err(e) = self.port.send(Request::Clear(ClearLog)) {
                    self.report(e);
                }
                self.reload_log();
                true
            }
            menu::DELETE => {
                if let Err(e) = self.port.send(Request::Forget(Forget { id })) {
                    self.report(e);
                }
                self.reload_log();
                true
            }
            _ => false,
        }
    }
}
