//! `ultar-i18n-check <dir>` – checks a directory of `.ftl` catalogs for CI.
//!
//! Exit codes: 0 = complete and consistent, 1 = problems found, 2 = wrong usage or unreadable directory.
//! Output language follows `ULTAR_LANG`, `LC_ALL`, `LC_MESSAGES`, `LANG` (Arabic by default).
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::process::ExitCode;

use ultar_i18n::check_dir::{check_directory, Report};
use ultar_i18n::{Arg, Lang, Translator};

const EXIT_PROBLEMS: u8 = 1;
const EXIT_USAGE: u8 = 2;

fn print_report(report: &Report, translator: &Translator) {
    let files = report.files.join(", ");
    println!(
        "{}",
        translator.t_args("check.checked", &[("files", Arg::Str(&files))])
    );
    for problem in &report.problems {
        println!("{}", problem.describe(translator));
    }
    if report.is_clean() {
        println!("{}", translator.t("check.ok"));
    } else {
        let count = i64::try_from(report.problems.len()).unwrap_or(i64::MAX);
        println!(
            "{}",
            translator.t_args("check.summary", &[("count", Arg::Int(count))])
        );
    }
}

fn main() -> ExitCode {
    let translator = Translator::new(Lang::from_env());
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let directory = match arguments.as_slice() {
        [flag] if flag == "--help" || flag == "-h" => {
            println!("{}", translator.t("check.usage"));
            return ExitCode::SUCCESS;
        }
        [directory] => directory,
        _ => {
            eprintln!("{}", translator.t("check.usage"));
            return ExitCode::from(EXIT_USAGE);
        }
    };
    match check_directory(Path::new(directory)) {
        Err(error) => {
            eprintln!("{}", error.describe(&translator));
            ExitCode::from(EXIT_USAGE)
        }
        Ok(report) => {
            print_report(&report, &translator);
            if report.is_clean() {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(EXIT_PROBLEMS)
            }
        }
    }
}
