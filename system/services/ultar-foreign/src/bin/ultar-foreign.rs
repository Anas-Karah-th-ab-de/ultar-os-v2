//! `ultar-foreign` – the service for apps from other systems (see the library
//! documentation).
//!
//! ```text
//! ultar-foreign --bus PATH --root DIR [--sources DIR:DIR] [--trusted-keys DIR] [--log-socket PATH] [--token-file PATH]
//! ultar-foreign --cell …        (its own cells' stage 1, started by itself)
//! ultar-foreign --cell-init …   (stage 2)
//! ```

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::process::ExitCode;
use ultar_foreign::cellrun::{stage_one, stage_two, CELL_ARG, CELL_INIT_ARG};
use ultar_foreign::detect::SOURCES;
use ultar_foreign::ForeignService;
use ultar_service::harden::{Right, RightSet};
use ultar_svckit::msg::Msg;

fn main() -> ExitCode {
    let words: Vec<String> = std::env::args().skip(1).collect();
    match words.first().map(String::as_str) {
        Some(CELL_ARG) => return code(stage_one(&words[1..])),
        Some(CELL_INIT_ARG) => return code(stage_two(&words[1..])),
        _ => {}
    }
    // mount: the cells' views; set_id: nobody in the inspection cell, root of the
    // installation cell; cell_chown: handed to the installation cell alone (owners of the
    // packages' files on its writable area, see cellrun.rs)
    let rights = Some(RightSet::of(&[
        Right::Mount,
        Right::SetId,
        Right::CellChown,
    ]));
    ultar_svckit::main(
        "ultar-foreign",
        &["root", "sources", "trusted-keys"],
        rights,
        |args, _| {
            let root = args.required_path("root").map_err(|_| Msg::BadArguments)?;
            let helper = std::env::current_exe().map_err(|_| Msg::BadArguments)?;
            // where the person's files may come from: their areas and drives (tests name
            // their own places); every entry must be absolute
            let sources: Vec<PathBuf> = match args.option("sources") {
                Some(list) => list.split(':').map(PathBuf::from).collect(),
                None => SOURCES.iter().map(PathBuf::from).collect(),
            };
            if sources.is_empty() || sources.iter().any(|s| !s.is_absolute()) {
                return Err(Msg::BadArguments);
            }
            let service = ForeignService::new(root, helper, sources);
            // tests name their own keys; a device checks components against its built-in ones
            Ok(
                match args.path("trusted-keys").map_err(|_| Msg::BadArguments)? {
                    Some(dir) => service.with_trusted_keys(dir),
                    None => service,
                },
            )
        },
    )
}

fn code(n: i32) -> ExitCode {
    ExitCode::from(u8::try_from(n).unwrap_or(u8::MAX))
}
