//! The output channel of `ultar-init`: lines a service printed arrive on
//! their own socket, are stored as free text under the service's name,
//! cleaned, limited per service, and shown only to readers who see
//! everything.

mod common;

use common::{temp_dir, Rig};
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::sync::mpsc::sync_channel;
use std::sync::Arc;
use std::time::Duration;
use ultar_bus::messages::log::{Query, SourceKind};
use ultar_logd::chain::Chain;
use ultar_logd::core::{CoreConfig, Scope};
use ultar_logd::ingest::{Delivery, Ingest, IngestLimits, ListenerSpec, SourceNames};
use ultar_service::log::output::{encode_frame, OutputLine, Stream};

fn line(service: &str, stream: Stream, text: &str) -> OutputLine {
    OutputLine {
        service: service.to_owned(),
        stream,
        text: text.to_owned(),
    }
}

#[test]
fn output_lines_are_stored_cleaned_and_shown_to_administrators_only() {
    let mut rig = Rig::new(CoreConfig::STANDARD, Chain::unkeyed());
    rig.core
        .ingest_output(&line("logd", Stream::Out, "Log service ready"));
    rig.core
        .ingest_output(&line("bus", Stream::Err, "bad \u{202e}line\u{1b}[2J"));
    let all = rig.core.query(&Query::default(), &Scope::All).records;
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].source, SourceKind::Output);
    assert_eq!(all[0].key, "output.stdout");
    assert_eq!(all[0].text.as_deref(), Some("Log service ready"));
    assert_eq!(all[1].key, "output.stderr");
    let text = all[1].text.clone().unwrap();
    assert!(
        !text.contains('\u{202e}') && !text.contains('\u{1b}'),
        "{text:?}"
    );
    // A service sees none of it, not even lines under a name like its own.
    let own = rig
        .core
        .query(&Query::default(), &Scope::Only("logd".into()))
        .records;
    assert!(own.is_empty());
}

#[test]
fn a_chatty_service_is_limited_by_its_own_name() {
    let mut rig = Rig::new(CoreConfig::STANDARD, Chain::unkeyed());
    for n in 0..1000 {
        rig.core
            .ingest_output(&line("chatty", Stream::Out, &format!("line {n}")));
    }
    let quiet = rig
        .core
        .ingest_output(&line("quiet", Stream::Out, "still here"));
    assert!(quiet.is_some(), "the other service's line was refused");
}

#[test]
fn the_output_listener_takes_output_frames_and_refuses_records() {
    let dir = temp_dir("output");
    let (sink, queue) = sync_channel::<Delivery>(64);
    let spec = ListenerSpec {
        path: dir.join("init.sock"),
        names: SourceNames::Output,
        mode: Some(0o600),
        group: None,
    };
    let limits = IngestLimits {
        max_connections: 1,
        ..IngestLimits::STANDARD
    };
    let ingest = Ingest::start(&[spec], limits, Arc::new(sink)).unwrap();
    let mut init = UnixStream::connect(dir.join("init.sock")).unwrap();
    let frame = encode_frame(&line("devd", Stream::Err, "probe failed")).unwrap();
    init.write_all(&frame).unwrap();
    // A record of the logging API is not an output line.
    let record = ultar_service::log::Record::new(ultar_service::log::Level::Info, "x.y");
    init.write_all(&ultar_service::log::wire::encode_frame(&record).unwrap())
        .unwrap();
    drop(init);
    let mut lines = Vec::new();
    let mut malformed = 0;
    while let Ok(delivery) = queue.recv_timeout(Duration::from_secs(5)) {
        match delivery {
            Delivery::Output { line, .. } => lines.push(line),
            Delivery::Counts { counts, .. } => {
                malformed += counts.malformed;
                break;
            }
            _ => {}
        }
    }
    drop(ingest);
    assert_eq!(lines, vec![line("devd", Stream::Err, "probe failed")]);
    assert_eq!(malformed, 1);
}
