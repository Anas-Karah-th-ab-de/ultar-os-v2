//! `apk-report FILE…` – what the service's APK reader (the one that runs in the inspection
//! cell) makes of Android packages: the report lines and the refusal, if any. A tool for
//! choosing test apps (`tools/foreign/android-e2e.sh`); it reads only, bounded like the cell.

#![allow(clippy::unwrap_used, clippy::expect_used)]

fn main() {
    let mut bad = false;
    for path in std::env::args().skip(1) {
        println!("== {path}");
        match std::fs::File::open(&path)
            .ok()
            .and_then(ultar_foreign::apk::analyze)
        {
            Some(report) => {
                for line in report.to_lines().lines() {
                    if !line.starts_with("icon=") {
                        println!("{line}");
                    }
                }
                println!("refusal={}", report.refusal().map_or("none", |r| r.word()));
            }
            None => {
                println!("unreadable");
                bad = true;
            }
        }
    }
    std::process::exit(i32::from(bad));
}
