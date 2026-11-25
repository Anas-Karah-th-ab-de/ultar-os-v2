//! ultar-paketbau – builds an Ultar package from pinned upstream releases.
//!
//! ```text
//! ultar-paketbau check    <recipe.toml>            is the recipe sound?
//! ultar-paketbau fetch    <recipe.toml> <workdir>  download the archives, compare or print hashes
//! ultar-paketbau outdated <recipe.toml>            which parts have a newer upstream release?
//! ultar-paketbau build    <recipe.toml> <workdir>  build every part, then pack
//! ultar-paketbau pack     <recipe.toml> <workdir>  the shipped copy: trimmed, closure proven, checks run
//!                                                   in a system that holds nothing but the package
//! ultar-paketbau closure  <recipe.toml>            closure of the built tree
//! ```
//! The package is built at its final place (`/pkg/<id>`) on the build machine; signing and
//! packing into a `.uap` is `upkg`'s job (`system/core/ultar-pkg`).

mod closure;
mod recipe;
mod version;
mod work;

use recipe::Recipe;
use std::path::{Path, PathBuf};
use work::Site;

fn load(path: &str) -> Result<Recipe, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    Recipe::parse(&text)
}

fn sound(recipe: &Recipe, pinned: bool) -> Result<(), String> {
    let problems = recipe.problems(pinned);
    if problems.is_empty() {
        return Ok(());
    }
    Err(problems.join("\n"))
}

fn site(recipe_path: &str, workdir: &str) -> Site {
    let recipe_dir = Path::new(recipe_path)
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    Site {
        recipe_dir: std::fs::canonicalize(&recipe_dir).unwrap_or(recipe_dir),
        root: PathBuf::from(workdir),
        jobs: std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4),
    }
}

fn fetch(recipe: &Recipe, site: &Site) -> Result<(), String> {
    for part in &recipe.parts {
        let hash = work::fetch(site, part)?;
        let note = if part.sha256.is_empty() {
            "  <- not pinned yet"
        } else {
            ""
        };
        println!("{:<18} {:<12} {hash}{note}", part.name, part.version);
    }
    Ok(())
}

fn outdated(recipe: &Recipe) -> Result<(), String> {
    let mut behind = 0;
    for part in &recipe.parts {
        match work::newer_upstream(part) {
            Ok(Some(new)) => {
                behind += 1;
                println!("{:<18} {:<12} -> {new}", part.name, part.version);
            }
            Ok(None) => println!("{:<18} {:<12} current", part.name, part.version),
            Err(e) => println!("{:<18} {:<12} ? ({e})", part.name, part.version),
        }
    }
    println!(
        "{behind} of {} parts have a newer upstream release",
        recipe.parts.len()
    );
    Ok(())
}

fn closure(recipe: &Recipe, tree: &Path) -> Result<(), String> {
    let (count, leaks) = work::closure_of(tree, &recipe.prefix);
    if count == 0 {
        return Err(format!("{}: nothing built there", tree.display()));
    }
    if leaks.is_empty() {
        println!(
            "closure: {count} programs and libraries, nothing reaches outside {}",
            recipe.prefix
        );
        return Ok(());
    }
    Err(format!(
        "closure broken ({} findings):\n{}",
        leaks.len(),
        leaks.join("\n")
    ))
}

fn host_ready(recipe: &Recipe) -> Result<(), String> {
    for tool in &recipe.host_needs {
        let found = std::process::Command::new("sh")
            .args(["-c", &format!("command -v {tool} >/dev/null")])
            .status()
            .is_ok_and(|s| s.success());
        if !found {
            return Err(format!("the build machine has no `{tool}` (host_needs)"));
        }
    }
    Ok(())
}

/// Builds every part, then packs: the shipped copy, its closure, its checks.
fn build(recipe: &Recipe, site: &Site) -> Result<(), String> {
    host_ready(recipe)?;
    for part in &recipe.parts {
        let t0 = std::time::Instant::now();
        let built = work::build(site, recipe, part)?;
        let state = match built {
            true => format!("built in {} s", t0.elapsed().as_secs()),
            false => "already built".into(),
        };
        println!("{:<22} {:<12} {state}", part.name, part.version);
    }
    pack(recipe, site)
}

fn pack(recipe: &Recipe, site: &Site) -> Result<(), String> {
    work::build_checks(site, recipe)?;
    let out = work::pack(site, recipe)?;
    closure(recipe, &out)?;
    for check in &recipe.checks {
        let text = work::run_check(site, recipe, check)?;
        println!("check ok: {}\n{}", check.name, text.trim_end());
    }
    println!("package tree: {}", out.display());
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or("");
    let result = load(arg(2)).and_then(|recipe| match (arg(1), arg(3)) {
        ("check", _) => sound(&recipe, false).map(|_| println!("recipe ok: {} parts", recipe.parts.len())),
        ("outdated", _) => sound(&recipe, false).and_then(|_| outdated(&recipe)),
        ("closure", _) => closure(&recipe, Path::new(&recipe.prefix)),
        ("pack", dir) if !dir.is_empty() => sound(&recipe, true).and_then(|_| pack(&recipe, &site(arg(2), dir))),
        ("fetch", dir) if !dir.is_empty() => sound(&recipe, false).and_then(|_| fetch(&recipe, &site(arg(2), dir))),
        ("build", dir) if !dir.is_empty() => sound(&recipe, true).and_then(|_| build(&recipe, &site(arg(2), dir))),
        _ => Err("usage: ultar-paketbau check|outdated|closure <recipe.toml>  |  fetch|build|pack <recipe.toml> <workdir>".into()),
    });
    if let Err(e) = result {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
