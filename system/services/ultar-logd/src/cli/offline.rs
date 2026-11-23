//! The parts of `ulog` that work on a log directory without the service:
//! `verify` and `export`.

use super::{Streams, EXIT_FAILED, EXIT_OK};
use crate::chain::Chain;
use crate::export::{export, ExportOptions};
use crate::msg::{text, Msg};
use crate::storage::DirStorage;
use crate::verify::verify;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use ultar_service::{read_secret_file, SecretFilePolicy};

/// Largest segment file read by `verify` and `export`.
const MAX_SEGMENT_BYTES: u64 = 64 << 20;

fn chain(key_file: Option<&Path>, io: &mut Streams<'_>) -> Result<Chain, i32> {
    let Some(path) = key_file else {
        return Ok(Chain::unkeyed());
    };
    read_secret_file(path, &SecretFilePolicy::for_current_user())
        .map(|key| Chain::keyed(key.expose()))
        .map_err(|error| {
            io.say_detail(Msg::SecretRejected, &error);
            EXIT_FAILED
        })
}

fn open_dir(dir: &Path, io: &mut Streams<'_>) -> Result<DirStorage, i32> {
    // `DirStorage::open` would create the directory; a log that is to be
    // checked must exist.
    let exists = std::fs::metadata(dir).map(|meta| meta.is_dir());
    match exists {
        Ok(true) => DirStorage::open(dir).map_err(|error| {
            io.say_detail(Msg::DirectoryUnreadable, &error);
            EXIT_FAILED
        }),
        _ => {
            io.say_detail(Msg::DirectoryUnreadable, &dir.display());
            Err(EXIT_FAILED)
        }
    }
}

pub(super) fn run_verify(dir: &Path, key_file: Option<&Path>, io: &mut Streams<'_>) -> i32 {
    let (chain, storage) = match (chain(key_file, io), open_dir(dir, io)) {
        (Ok(chain), Ok(storage)) => (chain, storage),
        (Err(code), _) | (_, Err(code)) => return code,
    };
    let report = verify(&storage, &chain, MAX_SEGMENT_BYTES);
    let _ = writeln!(
        io.out,
        "segments={} records={} first={} last={} head={}",
        report.segments,
        report.records,
        report.first_seq.unwrap_or(0),
        report.last_seq.unwrap_or(0),
        report
            .head
            .map_or_else(|| "-".to_owned(), |seq| seq.to_string())
    );
    for (first, last) in &report.declared_gaps {
        let _ = writeln!(io.out, "{} {first}-{last}", text(io.lang, Msg::VerifyGap));
    }
    if report.torn_tail {
        let _ = writeln!(io.out, "{}", text(io.lang, Msg::VerifyTornTail));
    }
    for problem in &report.problems {
        let _ = writeln!(io.out, "problem={} {problem:?}", problem.key());
    }
    if !chain.is_keyed() {
        let _ = writeln!(io.out, "{}", text(io.lang, Msg::VerifyUnkeyedNote));
    }
    if report.is_intact() {
        let _ = writeln!(io.out, "{}", text(io.lang, Msg::VerifyIntact));
        EXIT_OK
    } else {
        let _ = writeln!(io.out, "{}", text(io.lang, Msg::VerifyBroken));
        EXIT_FAILED
    }
}

/// Writes the export next to its final place and renames it there, so a
/// half-written export never has the final name. The file is private.
pub(super) fn run_export(
    dir: &Path,
    out: &Path,
    key_file: Option<&Path>,
    options: ExportOptions,
    io: &mut Streams<'_>,
) -> i32 {
    let (chain, storage) = match (chain(key_file, io), open_dir(dir, io)) {
        (Ok(chain), Ok(storage)) => (chain, storage),
        (Err(code), _) | (_, Err(code)) => return code,
    };
    let mut temp = out.as_os_str().to_owned();
    temp.push(".tmp");
    let temp = PathBuf::from(temp);
    let written = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&temp)?;
        let mut buffered = std::io::BufWriter::new(&mut file);
        let report = export(&storage, &chain, options, MAX_SEGMENT_BYTES, &mut buffered)?;
        buffered.flush()?;
        drop(buffered);
        file.sync_all()?;
        std::fs::rename(&temp, out)?;
        Ok::<_, std::io::Error>(report)
    })();
    match written {
        Ok(report) => {
            let _ = writeln!(
                io.out,
                "{} {}",
                text(io.lang, Msg::ExportDone),
                out.display()
            );
            let _ = writeln!(
                io.out,
                "{} {}",
                text(io.lang, Msg::ExportRedacted),
                report.kernel_text_redacted
            );
            let key = if report.chain.is_intact() {
                Msg::VerifyIntact
            } else {
                Msg::VerifyBroken
            };
            let _ = writeln!(io.out, "{}", text(io.lang, key));
            EXIT_OK
        }
        Err(error) => {
            let _ = std::fs::remove_file(&temp);
            io.say_detail(Msg::ExportFailed, &error);
            EXIT_FAILED
        }
    }
}
