//! The steps that touch the machine: fetching archives, running builds, walking the package.

use crate::closure::{self, Elf};
use crate::recipe::{Dirs, Kind, Part, Recipe};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Directories of one run.
pub struct Site {
    /// Directory the recipe lies in.
    pub recipe_dir: PathBuf,
    /// Working directory: `sources/`, `build/`, `tools/`, `done/`.
    pub root: PathBuf,
    /// Parallel jobs.
    pub jobs: usize,
}

impl Site {
    fn dirs(&self) -> Dirs {
        Dirs {
            tools: self.root.join("tools").to_string_lossy().into_owned(),
            recipe: self.recipe_dir.to_string_lossy().into_owned(),
            jobs: self.jobs,
        }
    }

    fn archive(&self, part: &Part) -> PathBuf {
        self.root.join("sources").join(part.archive())
    }
}

fn sha256_of(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let digest = Sha256::digest(&bytes);
    Ok(digest.iter().map(|b| format!("{b:02x}")).collect())
}

fn run(cmd: &mut Command, what: &str) -> Result<(), String> {
    let status = cmd.status().map_err(|e| format!("{what}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{what}: failed ({status})"))
    }
}

/// Downloads the archive of `part` unless it is there, and compares its hash with the pinned
/// one. Returns the hash (for a part that is not pinned yet).
pub fn fetch(site: &Site, part: &Part) -> Result<String, String> {
    let file = site.archive(part);
    if !file.exists() {
        std::fs::create_dir_all(site.root.join("sources")).map_err(|e| e.to_string())?;
        let tmp = file.with_extension("part");
        let mut last = String::new();
        // an address that stalls (under 1 kB/s for a minute) counts as failed: the next is
        // tried; what was received is kept and continued (slow lines are normal here)
        let got = part.urls().iter().any(|url| {
            let mut curl = Command::new("curl");
            curl.args([
                "-fL",
                "--retry",
                "5",
                "-sS",
                "--connect-timeout",
                "20",
                "-C",
                "-",
            ]);
            curl.args(["--speed-limit", "1000", "--speed-time", "60", "-o"]);
            curl.arg(&tmp).arg(url);
            match run(&mut curl, &format!("download of {url}")) {
                Ok(()) => true,
                Err(e) => {
                    last = e;
                    false
                }
            }
        });
        if !got {
            let _ = std::fs::remove_file(&tmp);
            return Err(last);
        }
        std::fs::rename(&tmp, &file).map_err(|e| e.to_string())?;
    }
    let hash = sha256_of(&file)?;
    if !part.sha256.is_empty() && hash != part.sha256.to_lowercase() {
        // set aside, so that the next run downloads it again
        let _ = std::fs::rename(&file, file.with_extension("bad"));
        return Err(format!(
            "{}: hash is {hash}, the recipe pins {} – not the pinned release (damaged download or changed upstream file); set aside as .bad",
            part.archive(),
            part.sha256
        ));
    }
    Ok(hash)
}

/// The newest release upstream has tagged, if it is newer than the pinned one.
pub fn newer_upstream(part: &Part) -> Result<Option<String>, String> {
    let out = Command::new("git")
        .args(["ls-remote", "--tags", "--refs", &part.upstream])
        .output()
        .map_err(|e| format!("git ls-remote: {e}"))?;
    if !out.status.success() {
        return Err(format!("{}: tags could not be read", part.upstream));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let tags: Vec<String> = text
        .lines()
        .filter_map(|l| l.split("refs/tags/").nth(1))
        .filter(|t| part.not_ending.is_empty() || !t.ends_with(&part.not_ending))
        .map(str::to_string)
        .collect();
    let newest = crate::version::newest(&tags, &part.tag);
    Ok(newest.filter(|n| crate::version::newer(n, &part.version)))
}

fn marker(site: &Site, part: &Part) -> PathBuf {
    let name = format!(
        "{}-{}-{}",
        part.name,
        part.version,
        &part.sha256[..12.min(part.sha256.len())]
    );
    site.root.join("done").join(name)
}

/// Unpacks, patches and builds `part` (skipped if exactly this release was built before).
pub fn build(site: &Site, recipe: &Recipe, part: &Part) -> Result<bool, String> {
    let done = marker(site, part);
    if done.exists() {
        return Ok(false);
    }
    fetch(site, part)?;
    let src = site.root.join("build").join(&part.name);
    let _ = std::fs::remove_dir_all(&src);
    std::fs::create_dir_all(&src).map_err(|e| e.to_string())?;
    let mut tar = Command::new("tar");
    tar.args(["-xf"])
        .arg(site.archive(part))
        .args(["--strip-components=1", "-C"])
        .arg(&src);
    run(&mut tar, &format!("unpacking {}", part.archive()))?;
    for patch in &part.patches {
        let mut p = Command::new("patch");
        p.args(["-p1", "-i"])
            .arg(site.recipe_dir.join(patch))
            .current_dir(&src);
        run(&mut p, &format!("{}: patch {patch}", part.name))?;
    }
    let dirs = site.dirs();
    let log = site.root.join("build").join(format!("{}.log", part.name));
    for step in &part.build {
        let line = recipe.expand(step, part, &dirs);
        let mut sh = Command::new("sh");
        sh.arg("-c")
            .arg(format!("{{ {line} ; }} >> '{}' 2>&1", log.display()));
        sh.current_dir(&src);
        sh.env(
            "PATH",
            format!(
                "{}/bin:{}",
                dirs.tools,
                std::env::var("PATH").unwrap_or_default()
            ),
        );
        let shipped = (part.kind == Kind::Runtime).then_some(&recipe.runtime_env);
        for (key, value) in recipe.env.iter().chain(shipped.into_iter().flatten()) {
            sh.env(key, recipe.expand(value, part, &dirs));
        }
        run(
            &mut sh,
            &format!("{}: `{line}` (log: {})", part.name, log.display()),
        )?;
    }
    std::fs::create_dir_all(site.root.join("done")).map_err(|e| e.to_string())?;
    std::fs::write(&done, "").map_err(|e| e.to_string())?;
    Ok(true)
}

/// Deletes what the recipe lists under `remove` from the built package.
pub fn trim(recipe: &Recipe) -> Result<(), String> {
    for pattern in &recipe.remove {
        let mut sh = Command::new("sh");
        sh.arg("-c")
            .arg(format!("rm -rf -- {}/{pattern}", recipe.prefix));
        run(&mut sh, &format!("remove {pattern}"))?;
    }
    Ok(())
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let path = e.path();
        match e.file_type() {
            Ok(t) if t.is_dir() => walk(&path, out),
            Ok(t) if t.is_file() => out.push(path),
            _ => {}
        }
    }
}

/// Every way the files under the package's prefix reach outside it, and how many programs and
/// libraries were looked at.
pub fn closure_of(prefix: &str) -> (usize, Vec<String>) {
    let mut files = Vec::new();
    walk(Path::new(prefix), &mut files);
    let exists = |f: &str| Path::new(f).exists();
    let mut leaks = Vec::new();
    let mut count = 0;
    for f in files {
        let Ok(bytes) = std::fs::read(&f) else {
            continue;
        };
        if !bytes.starts_with(b"\x7fELF") {
            continue;
        }
        let Some(elf): Option<Elf> = closure::read(&f.to_string_lossy(), &bytes) else {
            continue;
        };
        // static programs and plain objects ask the loader for nothing
        if elf.interp.is_none() && elf.needed.is_empty() {
            continue;
        }
        count += 1;
        leaks.extend(closure::leaks(&elf, prefix, &exists));
    }
    (count, leaks)
}

/// The list of contents written into the package: what it is made of, from where, under which
/// licence, with which of our patches.
pub fn contents(recipe: &Recipe) -> String {
    let mut out = format!(
        "# {} {} ({}) – made from these upstream releases\n\n",
        recipe.name, recipe.version, recipe.arch
    );
    for p in recipe.parts.iter().filter(|p| p.kind == Kind::Runtime) {
        out += &format!(
            "[[part]]\nname = {:?}\nversion = {:?}\nsource = {:?}\nsha256 = {:?}\nlicense = {:?}\nwhy = {:?}\npatches = {:?}\n\n",
            p.name, p.version, p.url(), p.sha256, p.license, p.why, p.patches
        );
    }
    out
}
