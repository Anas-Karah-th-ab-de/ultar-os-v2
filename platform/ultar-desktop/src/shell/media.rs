//! A drive with media was plugged in (`volumes.state`): what `apps.removable_media` does –
//! ask (a notice with an "Open" button), open it in Files at once, or nothing.

use super::bar::FILES;
use super::{Effect, Opening, Outcome, Shell};
use crate::model::quick::RemovableMedia;

/// Reacts to the drives that appeared since the last look. A first report after start is
/// not an insertion (those drives were already there); the shell remembers the ids it saw.
pub(super) fn inserted(s: &mut Shell) -> Outcome {
    let Some(drives) = s.d.svc.drives.clone() else {
        return Outcome::none();
    };
    let fresh: Vec<_> = match &s.media_seen {
        None => Vec::new(),
        Some(seen) => drives
            .iter()
            .filter(|d| !seen.contains(&d.id))
            .cloned()
            .collect(),
    };
    s.media_seen = Some(drives.iter().map(|d| d.id.clone()).collect());
    if fresh.is_empty() {
        return Outcome::none();
    }
    let now = s.clock.now_ms();
    let mut out = Outcome::surfaces();
    for drive in fresh {
        match s.d.values.removable_media {
            RemovableMedia::Ask => s.d.notes.post_media(&drive.name, &drive.uri, now),
            RemovableMedia::OpenFiles => out.effects.push(Effect::Launch {
                app: FILES.to_owned(),
                open: Opening::uri(drive.uri),
            }),
            RemovableMedia::Nothing => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::time::Clock;
    use crate::model::Desktop;
    use std::sync::Arc;
    use ultar_bus::messages::volumes::Mounted;
    use ultar_i18n::Lang;

    struct FixedClock;
    impl Clock for FixedClock {
        fn now_ms(&self) -> u64 {
            1_000
        }
        fn offset_s(&self) -> i32 {
            0
        }
    }

    fn shell() -> Shell {
        Shell::new(Desktop::new(Lang::En), Arc::new(FixedClock), "amal")
    }

    fn drive(id: &str) -> Mounted {
        Mounted {
            id: id.into(),
            name: "STICK".into(),
            uri: format!("file:///media/{id}"),
        }
    }

    #[test]
    fn open_files_launches_files_at_the_drive() {
        let mut s = shell();
        s.d.values.removable_media = RemovableMedia::OpenFiles;
        s.d.svc.set_drives(vec![]);
        assert!(inserted(&mut s).effects.is_empty());
        s.d.svc.set_drives(vec![drive("sdb1")]);
        let out = inserted(&mut s);
        assert_eq!(
            out.effects,
            vec![Effect::Launch {
                app: FILES.to_owned(),
                open: Opening::uri("file:///media/sdb1"),
            }]
        );
        // looked at again: nothing new
        assert!(inserted(&mut s).effects.is_empty());
    }

    #[test]
    fn ask_posts_a_notice_with_an_open_button() {
        let mut s = shell();
        s.d.svc.set_drives(vec![]);
        assert!(inserted(&mut s).effects.is_empty());
        s.d.svc.set_drives(vec![drive("sdb1")]);
        let out = inserted(&mut s);
        assert!(out.effects.is_empty());
        let note = s.d.notes.list().first().expect("a notice");
        assert_eq!(note.title.key, "desk.media.title");
        assert_eq!(note.actions.len(), 1);
        assert_eq!(note.actions[0].id, "file:///media/sdb1");
    }

    #[test]
    fn nothing_does_nothing_and_a_first_report_is_no_insertion() {
        let mut s = shell();
        s.d.values.removable_media = RemovableMedia::Nothing;
        // the first report after start: drives already there, nothing happens
        s.d.svc.set_drives(vec![drive("sda1")]);
        assert!(inserted(&mut s).effects.is_empty());
        assert!(s.d.notes.list().is_empty());
        // a new drive with "nothing": still nothing
        s.d.svc.set_drives(vec![drive("sda1"), drive("sdb1")]);
        let out = inserted(&mut s);
        assert!(out.effects.is_empty() && s.d.notes.list().is_empty());
    }
}
