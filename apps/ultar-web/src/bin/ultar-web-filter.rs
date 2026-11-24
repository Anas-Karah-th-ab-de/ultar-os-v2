//! `ultar-web-filter <out.json> <list.txt>…` – compiles ad-block lists into the
//! content-blocker JSON of the engine host (see `ultar_web::filter`). Writes the file
//! atomically; prints the rule and skip counts. Exit 0 ok, 1 I/O error, 2 usage.

use std::io::Write;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        let _ = writeln!(
            std::io::stderr(),
            "usage: ultar-web-filter <out.json> <list.txt>..."
        );
        return ExitCode::from(2);
    }
    let mut texts = Vec::new();
    for path in &args[1..] {
        match std::fs::read_to_string(path) {
            Ok(t) => texts.push(t),
            Err(e) => {
                let _ = writeln!(std::io::stderr(), "ultar-web-filter: {path}: {e}");
                return ExitCode::from(1);
            }
        }
    }
    let lists: Vec<&str> = texts.iter().map(String::as_str).collect();
    let compiled = ultar_web::filter::compile(&lists);
    let out = std::path::Path::new(&args[0]);
    let temp = out.with_extension("json.tmp");
    let written = std::fs::File::create(&temp)
        .and_then(|mut f| {
            f.write_all(compiled.json.as_bytes())
                .and_then(|_| f.sync_all())
        })
        .and_then(|_| std::fs::rename(&temp, out));
    if let Err(e) = written {
        let _ = writeln!(
            std::io::stderr(),
            "ultar-web-filter: {}: {e}",
            out.display()
        );
        return ExitCode::from(1);
    }
    let _ = writeln!(
        std::io::stdout(),
        "rules={} skipped={}",
        compiled.rules,
        compiled.skipped
    );
    ExitCode::SUCCESS
}
