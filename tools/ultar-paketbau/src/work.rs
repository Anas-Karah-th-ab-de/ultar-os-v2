//! The steps that touch the machine: fetching archives, running builds, walking the package.

use crate::closure::{self, Elf};
use crate::recipe::{Check, Dirs, Kind, Part, Recipe};
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
            build: self.root.join("build").to_string_lossy().into_owned(),
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
    if !part.follows.is_empty() {
        return Err(format!("follows {}", part.follows));
    }
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

/// How `part` is built: its commands, patches and environment as one hash. A part is built
/// again when this changes (the release alone does not say whether an option was turned off).
fn way(recipe: &Recipe, part: &Part) -> String {
    let mut h = Sha256::new();
    let env = recipe.env.iter().chain((part.kind == Kind::Runtime).then_some(&recipe.runtime_env).into_iter().flatten());
    for text in part.build.iter().chain(&part.patches).cloned().chain(env.map(|(k, v)| format!("{k}={v}"))) {
        h.update(text.as_bytes());
        h.update([0]);
    }
    h.finalize().iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// Unpacks, patches and builds `part` (skipped if exactly this release was built this way
/// before; a marker from before the way was recorded counts as built).
pub fn build(site: &Site, recipe: &Recipe, part: &Part) -> Result<bool, String> {
    let done = marker(site, part);
    let way = way(recipe, part);
    if std::fs::read_to_string(&done).is_ok_and(|was| was.is_empty() || was == way) {
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
    let _ = std::fs::remove_file(&log);
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
    std::fs::write(&done, way).map_err(|e| e.to_string())?;
    Ok(true)
}

/// Where the package as it is shipped lies after [`pack`].
pub fn shipped(site: &Site, recipe: &Recipe) -> PathBuf {
    site.root
        .join("out")
        .join(recipe.prefix.trim_start_matches('/'))
}

/// Copies the built package and deletes from the copy what the recipe lists under `remove`.
/// The built tree itself stays whole: later parts and the next update build against it.
pub fn pack(site: &Site, recipe: &Recipe) -> Result<PathBuf, String> {
    let out = shipped(site, recipe);
    let _ = std::fs::remove_dir_all(&out);
    let parent = out.parent().ok_or("bad prefix")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let mut cp = Command::new("cp");
    cp.arg("-a").arg(&recipe.prefix).arg(&out);
    run(&mut cp, "copy of the built package")?;
    for pattern in &recipe.remove {
        let mut sh = Command::new("sh");
        sh.arg("-c")
            .arg(format!("rm -rf -- {}/{pattern}", out.display()));
        run(&mut sh, &format!("remove {pattern}"))?;
    }
    let list = out.join("share/ultar");
    std::fs::create_dir_all(&list).map_err(|e| e.to_string())?;
    std::fs::write(list.join("contents.toml"), contents(recipe)).map_err(|e| e.to_string())?;
    Ok(out)
}

/// Runs the build commands of the recipe's checks (against the whole built tree).
pub fn build_checks(site: &Site, recipe: &Recipe) -> Result<(), String> {
    let dirs = site.dirs();
    for check in &recipe.checks {
        for step in &check.build {
            let line = recipe.expand_plain(step, &dirs);
            let mut sh = Command::new("sh");
            sh.arg("-c").arg(&line);
            sh.env(
                "PATH",
                format!(
                    "{}/bin:{}",
                    dirs.tools,
                    std::env::var("PATH").unwrap_or_default()
                ),
            );
            for (key, value) in recipe.env.iter().chain(&recipe.runtime_env) {
                sh.env(key, recipe.expand_plain(value, &dirs));
            }
            run(&mut sh, &format!("check {:?}: `{line}`", check.name))?;
        }
    }
    Ok(())
}

/// Runs a check in a system that holds nothing but the shipped package (its own mount
/// namespace: an empty root with the package at its prefix, read-only). Needs root.
pub fn run_check(site: &Site, recipe: &Recipe, check: &Check) -> Result<String, String> {
    let dirs = site.dirs();
    let root = site.root.join("empty");
    let at = root.join(recipe.prefix.trim_start_matches('/'));
    let shared: String = check
        .share
        .iter()
        .map(|d| format!("mkdir -p \"$R{d}\" && mount --rbind '{d}' \"$R{d}\" && "))
        .collect();
    // the packages it is built on: their shipped tree if this work directory built them
    let based: String = recipe
        .uses
        .iter()
        .map(|u| {
            let built = site.root.join("out").join(u.trim_start_matches('/'));
            let from = if built.exists() { built.display().to_string() } else { u.clone() };
            format!("mkdir -p \"$R{u}\" && mount --bind '{from}' \"$R{u}\" && mount -o remount,ro,bind \"$R{u}\" && ")
        })
        .collect();
    let script = format!(
        "R='{}'; rm -rf \"$R\"; mkdir -p '{}' && mount --bind '{}' '{}' && mount -o remount,ro,bind '{}' && {based}{shared} chroot \"$R\" {}",
        root.display(),
        at.display(),
        shipped(site, recipe).display(),
        at.display(),
        at.display(),
        recipe.expand_plain(&check.run, &dirs)
    );
    let out = Command::new("unshare")
        .args(["-m", "sh", "-c", &script])
        .output()
        .map_err(|e| format!("unshare: {e}"))?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    if text.contains(&check.expect) {
        Ok(text)
    } else {
        Err(format!(
            "check {:?}: expected {:?}, got:\n{text}",
            check.name, check.expect
        ))
    }
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

/// Every way the files of the package reach outside it, and how many programs and libraries
/// were looked at. `tree` is where the package lies now, `prefix` where it will live.
/// `uses`: prefixes of the packages it is built on (looked up where they are installed).
pub fn closure_of(tree: &Path, prefix: &str, uses: &[String]) -> (usize, Vec<String>) {
    let mut roots = vec![prefix.to_string()];
    roots.extend(uses.iter().cloned());
    let mut files = Vec::new();
    walk(tree, &mut files);
    let here = |f: &str| tree.join(f.strip_prefix(prefix).unwrap_or(f).trim_start_matches('/'));
    let exists = |f: &str| {
        if f.starts_with(prefix) {
            here(f).exists()
        } else {
            uses.iter().any(|u| f.starts_with(u.as_str())) && Path::new(f).exists()
        }
    };
    let mut leaks = Vec::new();
    let mut count = 0;
    for f in files {
        let Ok(bytes) = std::fs::read(&f) else {
            continue;
        };
        if !bytes.starts_with(b"\x7fELF") {
            continue;
        }
        let inside = f
            .strip_prefix(tree)
            .unwrap_or(&f)
            .to_string_lossy()
            .into_owned();
        let Some(elf): Option<Elf> = closure::read(&format!("{prefix}/{inside}"), &bytes) else {
            continue;
        };
        // static programs and plain objects ask the loader for nothing
        if elf.interp.is_none() && elf.needed.is_empty() {
            continue;
        }
        count += 1;
        leaks.extend(closure::leaks(&elf, &roots, &exists));
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
