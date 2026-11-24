//! Which i18n key says what: ways, failures, endings, notices. All texts live in
//! `locales/{ar,en,de}.ftl` (Arabic first); [`catalogs`] adds them to a translator.
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

use crate::model::Model;
use ultar_bus::messages::call::{CallInfo, Cellular, End, Failure, Output, Phase, Ultar, Way};
use ultar_i18n::{Lang, ParseError, Translator};
use ultar_ui::widgets::{Label, Tone};

/// Arabic catalog of the phone.
pub const AR: &str = include_str!("../locales/ar.ftl");
/// English catalog.
pub const EN: &str = include_str!("../locales/en.ftl");
/// German catalog.
pub const DE: &str = include_str!("../locales/de.ftl");

/// Keys of the ready messages of "decline with message".
pub const TEXTS: [&str; 3] = ["phone.text.1", "phone.text.2", "phone.text.3"];

/// Adds the phone catalogs to `tr`.
pub fn catalogs(tr: &mut Translator) -> Result<(), ParseError> {
    for (lang, source) in [(Lang::Ar, AR), (Lang::En, EN), (Lang::De, DE)] {
        tr.add_source(lang, source)?;
    }
    Ok(())
}

/// A translator for `lang` with the system and phone catalogs.
pub fn translator(lang: Lang) -> Result<Translator, ParseError> {
    let mut tr = Translator::new(lang);
    catalogs(&mut tr)?;
    Ok(tr)
}

/// Label of a ready message.
pub fn ready_message(key: &str) -> Label {
    Label::key(key)
}

/// Key of a failure reason.
pub fn failure_key(f: Failure) -> &'static str {
    match f {
        Failure::NoNetwork => "phone.why.no_network",
        Failure::NoSim => "phone.why.no_sim",
        Failure::Airplane => "phone.why.airplane",
        Failure::EmergencyOnly => "phone.why.emergency",
        Failure::SignedOut => "phone.why.signed_out",
        Failure::Offline => "phone.why.offline",
        Failure::Unreachable => "phone.why.unreachable",
        Failure::PeerOffline => "phone.why.peer_offline",
        Failure::Media => "phone.why.media",
        Failure::Rejected => "phone.why.rejected",
    }
}

/// "Via Ultar number" / "Via SIM n".
pub fn via(way: Way) -> Label {
    match way {
        Way::Ultar => Label::key("phone.via.ultar"),
        Way::Cellular(slot) => Label::key_with("phone.via.sim", &[("slot", &slot.to_string())]),
    }
}

/// Short name of a way for history rows ("Ultar number" / "SIM 1").
pub fn way_short(way: Way) -> Label {
    match way {
        Way::Ultar => Label::key("phone.way.ultar"),
        Way::Cellular(slot) => {
            Label::key_with("phone.way.cell.detail", &[("slot", &slot.to_string())])
        }
    }
}

/// Key of an audio output.
pub fn output_key(o: Output) -> &'static str {
    match o {
        Output::Earpiece => "phone.output.earpiece",
        Output::Speaker => "phone.output.speaker",
        Output::Headset => "phone.output.headset",
        Output::Bluetooth => "phone.output.bluetooth",
    }
}

/// Notice about the chosen way on the dial pad.
pub fn line_notice(model: &Model) -> Option<(Tone, &'static str)> {
    let lines = &model.state.lines;
    let warn = |k| Some((Tone::Warning, k));
    match model.current_way() {
        Way::Ultar => match lines.ultar {
            Ultar::Ready => None,
            Ultar::SignedOut => Some((Tone::Info, "phone.line.signed_out")),
            Ultar::Offline => warn("phone.line.offline"),
        },
        Way::Cellular(_) => match lines.cellular {
            Cellular::Ready => None,
            Cellular::NoModem => Some((Tone::Info, "phone.line.no_modem")),
            Cellular::NoSim => warn("phone.line.no_sim"),
            Cellular::Airplane => warn("phone.line.airplane"),
            Cellular::NoNetwork => warn("phone.line.no_network"),
            Cellular::EmergencyOnly => warn("phone.line.emergency"),
        },
    }
}

/// Tone and text of the notice about an ended call; `time` is its duration.
pub fn end_label(call: &CallInfo, time: &str) -> (Tone, Label) {
    let who = Model::who(call);
    match call.phase {
        Phase::Ended(End::Local) => (
            Tone::Info,
            Label::key_with("phone.end.local", &[("time", time)]),
        ),
        Phase::Ended(End::Remote) => (
            Tone::Info,
            Label::key_with("phone.end.remote", &[("time", time)]),
        ),
        Phase::Ended(End::Declined) => (Tone::Info, Label::key("phone.end.declined")),
        Phase::Ended(End::Missed) => (
            Tone::Warning,
            Label::key_with("phone.end.missed", &[("name", who)]),
        ),
        Phase::Ended(End::Busy) => (Tone::Warning, Label::key("phone.end.busy")),
        Phase::Ended(End::Blocked) => (Tone::Info, Label::key("phone.end.blocked")),
        Phase::Ended(End::Failed(f)) => (Tone::Danger, Label::key(failure_key(f))),
        _ => (Tone::Info, Label::key("phone.end.declined")),
    }
}
