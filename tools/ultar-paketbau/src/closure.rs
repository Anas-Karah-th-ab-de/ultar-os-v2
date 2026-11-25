//! The proof that a package is independent: every program in it starts with the package's own
//! loader, and every library it asks for is found inside the package. Nothing is taken from
//! the system the package is later installed on.

use std::path::Path;

/// What a program or library asks the loader for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Elf {
    /// Path of the file.
    pub path: String,
    /// The loader it wants to be started with (programs only).
    pub interp: Option<String>,
    /// Libraries it needs.
    pub needed: Vec<String>,
    /// Directories it tells the loader to search (`$ORIGIN` not yet replaced).
    pub runpath: Vec<String>,
}

/// Reads the loader-relevant parts of an ELF file; `None` if `bytes` is not one.
pub fn read(path: &str, bytes: &[u8]) -> Option<Elf> {
    let elf = goblin::elf::Elf::parse(bytes).ok()?;
    let mut paths: Vec<&str> = elf.runpaths.clone();
    paths.extend(elf.rpaths.iter());
    let runpath = paths
        .iter()
        .flat_map(|p| p.split(':'))
        .filter(|p| !p.is_empty())
        .map(str::to_string)
        .collect();
    Some(Elf {
        path: path.to_string(),
        interp: elf.interpreter.map(str::to_string),
        needed: elf.libraries.iter().map(|s| s.to_string()).collect(),
        runpath,
    })
}

fn inside(path: &str, prefix: &str) -> bool {
    path == prefix || path.starts_with(&format!("{prefix}/"))
}

/// Every way `elf` reaches outside the packages at `roots` – the package itself and the ones
/// it is built on (empty: none). `exists` says whether a file is in one of them.
pub fn leaks(elf: &Elf, roots: &[String], exists: &dyn Fn(&str) -> bool) -> Vec<String> {
    let within = |path: &str| roots.iter().any(|r| inside(path, r));
    let mut out = Vec::new();
    if let Some(interp) = &elf.interp {
        if !within(interp) || !exists(interp) {
            out.push(format!(
                "{}: started by {interp}, not by the package's loader",
                elf.path
            ));
        }
    }
    let origin = Path::new(&elf.path)
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    // a package's loader searches <prefix>/lib after the file's own list
    let mut dirs: Vec<String> = elf
        .runpath
        .iter()
        .map(|d| d.replace("$ORIGIN", &origin).replace("${ORIGIN}", &origin))
        .collect();
    for d in &dirs {
        if !within(d) {
            out.push(format!("{}: searches {d}, outside the package", elf.path));
        }
    }
    dirs.extend(roots.iter().map(|r| format!("{r}/lib")));
    for lib in &elf.needed {
        let found = dirs
            .iter()
            .any(|d| within(d) && exists(&format!("{d}/{lib}")));
        if !found {
            out.push(format!(
                "{}: needs {lib}, which is not in the package",
                elf.path
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const P: &str = "/pkg/probe";

    fn has(files: &'static [&'static str]) -> impl Fn(&str) -> bool {
        move |f| files.contains(&f)
    }

    #[test]
    fn a_program_with_everything_inside_is_closed() {
        let elf = Elf {
            path: "/pkg/probe/bin/tool".into(),
            interp: Some("/pkg/probe/lib/ld-linux-x86-64.so.2".into()),
            needed: vec!["libc.so.6".into(), "libz.so.1".into()],
            runpath: vec!["$ORIGIN/../lib/extra".into()],
        };
        let files = has(&[
            "/pkg/probe/lib/ld-linux-x86-64.so.2",
            "/pkg/probe/lib/libc.so.6",
            "/pkg/probe/bin/../lib/extra/libz.so.1",
        ]);
        assert_eq!(leaks(&elf, &[P.to_string()], &files), Vec::<String>::new());
    }

    #[test]
    fn the_system_loader_a_missing_library_and_an_outside_path_are_leaks() {
        let elf = Elf {
            path: "/pkg/probe/bin/tool".into(),
            interp: Some("/lib64/ld-linux-x86-64.so.2".into()),
            needed: vec!["libc.so.6".into(), "libstdc++.so.6".into()],
            runpath: vec!["/usr/lib/x86_64-linux-gnu".into()],
        };
        let files = has(&["/pkg/probe/lib/libc.so.6"]);
        let l = leaks(&elf, &[P.to_string()], &files);
        assert_eq!(l.len(), 3, "{l:?}");
        assert!(l[0].contains("/lib64/ld-linux"));
        assert!(l[1].contains("/usr/lib/x86_64-linux-gnu"));
        assert!(l[2].contains("libstdc++.so.6"));
    }

    #[test]
    fn a_library_has_no_loader_and_a_look_alike_prefix_is_outside() {
        let lib = Elf {
            path: "/pkg/probe/lib/libz.so.1".into(),
            needed: vec!["libc.so.6".into()],
            runpath: vec!["/pkg/probe-other/lib".into()],
            ..Elf::default()
        };
        let files = has(&["/pkg/probe/lib/libc.so.6", "/pkg/probe-other/lib/libc.so.6"]);
        let l = leaks(&lib, &[P.to_string()], &files);
        assert_eq!(l.len(), 1, "{l:?}");
        assert!(l[0].contains("/pkg/probe-other/lib"));
    }

    #[test]
    fn text_is_not_an_elf_file() {
        assert_eq!(read("x", b"#!/bin/sh\n"), None);
    }

    #[test]
    fn a_package_may_stand_on_another_one() {
        // a program of /pkg/llama started by the loader of the package it is built on
        let base = "/pkg/base";
        let elf = Elf {
            path: "/pkg/demo/bin/tool".into(),
            interp: Some(format!("{base}/lib/ld.so")),
            needed: vec!["libc.so.6".into(), "libown.so".into()],
            runpath: vec!["/pkg/demo/lib".into(), format!("{base}/lib")],
        };
        let files = |f: &str| ["/pkg/base/lib/ld.so", "/pkg/base/lib/libc.so.6", "/pkg/demo/lib/libown.so"].contains(&f);
        assert!(leaks(&elf, &["/pkg/demo".into(), base.into()], &files).is_empty());
        // without naming the base, all of it is outside
        assert_eq!(leaks(&elf, &["/pkg/demo".into()], &files).len(), 3);
    }
}
