//! The recipe of a package: which upstream releases it is made of, where they come from, how
//! each is built – and nothing else. The package stays independent because every source is an
//! unmodified upstream release pinned by its hash, and stays easy to update because a new
//! release means two changed lines (version, hash) and our own changes live in patch files
//! next to the recipe, never in a copy of the source.

use serde::Deserialize;
use std::collections::BTreeSet;

/// What a part is for.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// Needed to build, not shipped (installed into the tools directory).
    Tool,
    /// Shipped in the package (installed into the package's prefix).
    Runtime,
}

/// One upstream release.
#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Part {
    pub name: String,
    pub kind: Kind,
    pub version: String,
    /// Download address of the release archive; `{version}` is replaced.
    pub source: String,
    /// A second address of the same archive, tried when the first one fails or stalls.
    #[serde(default)]
    pub mirror: String,
    /// SHA-256 of the archive. Empty only while a recipe is being written (`fetch` prints it).
    #[serde(default)]
    pub sha256: String,
    /// Where upstream publishes its tags, and what a release tag looks like (`v{version}`).
    pub upstream: String,
    pub tag: String,
    /// Name of the part this one is tied to: its source is the exact state that part asks
    /// for (a commit upstream names, not a release of its own), its version is that part's
    /// version, and it is updated together with it.
    #[serde(default)]
    pub follows: String,
    /// Tags that match but are no releases end like this (glibc marks its development
    /// branch `2.44.9000`).
    #[serde(default)]
    pub not_ending: String,
    pub license: String,
    /// Why the part is in the package (one line, shown in the list of contents).
    pub why: String,
    /// Our changes, as patch files next to the recipe, applied in this order. Best: none.
    #[serde(default)]
    pub patches: Vec<String>,
    /// Shell commands run in the unpacked source; see [`Recipe::expand`] for the variables.
    pub build: Vec<String>,
}

/// A package.
#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    pub id: String,
    pub name: String,
    pub version: String,
    pub arch: String,
    /// Where the package lives on the device (and on the build machine while it is built).
    pub prefix: String,
    /// Programs the build machine must have; everything else is built from source here.
    pub host_needs: Vec<String>,
    /// Environment for every build command (values may use the variables).
    #[serde(default)]
    pub env: Vec<(String, String)>,
    /// Environment only for the parts that are shipped: compiler and linker settings that
    /// make them use the package's own C library and loader instead of the build machine's.
    #[serde(default)]
    pub runtime_env: Vec<(String, String)>,
    /// What is deleted from the package after the build (paths below the prefix; shell
    /// patterns allowed): files only needed to build, not to run.
    #[serde(default)]
    pub remove: Vec<String>,
    #[serde(rename = "part")]
    pub parts: Vec<Part>,
    /// Programs that prove the package works with nothing but itself.
    #[serde(default, rename = "check")]
    pub checks: Vec<Check>,
}

/// A program built against the package and run in a system that holds only the package.
#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Check {
    pub name: String,
    /// Commands that build the program into the package (the variables and the environment of
    /// the shipped parts apply).
    pub build: Vec<String>,
    /// The program and its arguments.
    pub run: String,
    /// What its output must contain.
    pub expect: String,
    /// Directories of the build machine handed into that system (for a check that needs
    /// the GPU: its device files and the kernel's description of the devices).
    #[serde(default)]
    pub share: Vec<String>,
}

fn plain_name(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '+')
}

impl Part {
    /// The download address with the version filled in.
    pub fn url(&self) -> String {
        crate::version::fill(&self.source, &self.version)
    }

    /// Every address of the archive, in the order they are tried.
    pub fn urls(&self) -> Vec<String> {
        let mut all = vec![self.url()];
        if !self.mirror.is_empty() {
            all.push(crate::version::fill(&self.mirror, &self.version));
        }
        all
    }

    /// File name the archive is kept under: the part's name and version, with the ending of
    /// the address (addresses of different parts may end in the same file name).
    pub fn archive(&self) -> String {
        let url = self.url();
        let file = url.rsplit('/').next().unwrap_or("");
        let ending = [".tar.xz", ".tar.gz", ".tar.bz2", ".tar.zst", ".tgz"]
            .into_iter()
            .find(|e| file.ends_with(e))
            .unwrap_or(".tar");
        format!("{}-{}{ending}", self.name, self.version)
    }

    fn problems(&self, out: &mut Vec<String>) {
        let p = &self.name;
        if !plain_name(p) {
            out.push(format!(
                "part name {p:?}: lower-case letters, digits, - and + only"
            ));
        }
        if crate::version::parse(&self.version).is_none() {
            out.push(format!(
                "{p}: version {:?} is not a release number",
                self.version
            ));
        }
        let tied = !self.follows.is_empty();
        if !self.source.starts_with("https://") || !(tied || self.source.contains("{version")) {
            out.push(format!("{p}: source must be https and contain {{version}}"));
        }
        if !self.mirror.is_empty() && !self.mirror.starts_with("https://") {
            out.push(format!("{p}: mirror must be https"));
        }
        let hex = self.sha256.len() == 64 && self.sha256.chars().all(|c| c.is_ascii_hexdigit());
        if !self.sha256.is_empty() && !hex {
            out.push(format!("{p}: sha256 is not 64 hex digits"));
        }
        if !self.upstream.starts_with("https://") || !(tied || self.tag.contains("{version")) {
            out.push(format!(
                "{p}: upstream must be https, tag must contain {{version}}"
            ));
        }
        if self.license.is_empty() || self.why.is_empty() || self.build.is_empty() {
            out.push(format!("{p}: license, why and build must not be empty"));
        }
        if self
            .patches
            .iter()
            .any(|f| f.contains('/') || !f.ends_with(".patch"))
        {
            out.push(format!("{p}: patches are *.patch files next to the recipe"));
        }
    }
}

impl Recipe {
    /// Reads a recipe.
    pub fn parse(text: &str) -> Result<Recipe, String> {
        toml::from_str(text).map_err(|e| format!("recipe: {e}"))
    }

    /// Everything that is wrong with the recipe (empty: nothing). `pinned`: every part must
    /// carry its hash – required before a build, not while the recipe is being written.
    pub fn problems(&self, pinned: bool) -> Vec<String> {
        let mut out = Vec::new();
        if !plain_name(&self.id) {
            out.push("id: lower-case letters, digits and - only".into());
        }
        if self.prefix != format!("/pkg/{}", self.id) {
            out.push(format!("prefix must be /pkg/{}", self.id));
        }
        let mut seen = BTreeSet::new();
        for part in &self.parts {
            part.problems(&mut out);
            if !seen.insert(&part.name) {
                out.push(format!("part {} appears twice", part.name));
            }
            let leader = self.parts.iter().find(|p| p.name == part.follows);
            if !part.follows.is_empty() && leader.is_none_or(|l| l.version != part.version) {
                out.push(format!(
                    "{}: follows {:?}, which must exist and have the same version",
                    part.name, part.follows
                ));
            }
            if pinned && part.sha256.is_empty() {
                out.push(format!("{}: no sha256 – run `fetch` and pin it", part.name));
            }
        }
        if self
            .remove
            .iter()
            .any(|r| r.starts_with('/') || r.contains(".."))
        {
            out.push("remove: paths below the prefix only".into());
        }
        for c in &self.checks {
            let plain = |d: &String| d.starts_with('/') && !d.contains(['\'', ' ', '"']);
            if !c.share.iter().all(plain) {
                out.push(format!(
                    "check {:?}: share lists absolute directories",
                    c.name
                ));
            }
            if !c.run.starts_with("{prefix}/") || c.expect.is_empty() {
                out.push(format!(
                    "check {:?}: runs a program of the package and expects output",
                    c.name
                ));
            }
        }
        if !self.parts.iter().any(|p| p.kind == Kind::Runtime) {
            out.push("no runtime part: the package would be empty".into());
        }
        out
    }

    /// Replaces the variables in a build command or environment value: `{prefix}` (package),
    /// `{tools}` (build tools), `{jobs}`, `{arch}`, `{version}` (of `part`), `{recipe}` (directory of the
    /// recipe, for patches and extra files), `{build}` (where the parts are unpacked: `{build}/<name>`).
    pub fn expand(&self, text: &str, part: &Part, dirs: &Dirs) -> String {
        self.expand_plain(text, dirs)
            .replace("{version}", &part.version)
    }

    /// [`Recipe::expand`] without a part (no `{version}`).
    pub fn expand_plain(&self, text: &str, dirs: &Dirs) -> String {
        text.replace("{prefix}", &self.prefix)
            .replace("{tools}", &dirs.tools)
            .replace("{recipe}", &dirs.recipe)
            .replace("{build}", &dirs.build)
            .replace("{jobs}", &dirs.jobs.to_string())
            .replace("{arch}", &self.arch)
    }
}

/// Where a build runs.
#[derive(Clone, Debug)]
pub struct Dirs {
    /// Directory the build tools are installed into.
    pub tools: String,
    /// Directory of the recipe.
    pub recipe: String,
    /// Directory the parts are unpacked and built in (`{build}/<part name>`): a later part
    /// may use the source of an earlier one.
    pub build: String,
    /// Parallel jobs.
    pub jobs: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"
id = "probe"
name = "Probe"
version = "1"
arch = "x86_64"
prefix = "/pkg/probe"
host_needs = ["gcc", "make"]
env = [["CFLAGS", "-O2 -I{prefix}/include"]]

[[part]]
name = "zlib"
kind = "runtime"
version = "1.3.1"
source = "https://example.org/zlib-{version}.tar.gz"
sha256 = "9a93b2b7dfdac77ceba5a558a580e74667dd6fede4585b91eefb60f03b72df23"
upstream = "https://github.com/madler/zlib"
tag = "v{version}"
license = "Zlib"
why = "compression"
build = ["./configure --prefix={prefix}", "make -j{jobs} install"]
"#;

    #[test]
    fn a_good_recipe_has_no_problems() {
        let r = Recipe::parse(GOOD).unwrap();
        assert_eq!(r.problems(true), Vec::<String>::new());
        assert_eq!(r.parts[0].url(), "https://example.org/zlib-1.3.1.tar.gz");
        assert_eq!(r.parts[0].archive(), "zlib-1.3.1.tar.gz");
        let mut tagged = r.parts[0].clone();
        tagged.name = "ninja".into();
        tagged.source = "https://example.org/archive/refs/tags/v{version}.tar.gz".into();
        assert_eq!(tagged.archive(), "ninja-1.3.1.tar.gz");
        let dirs = Dirs {
            tools: "/t".into(),
            recipe: "/r".into(),
            build: "/b".into(),
            jobs: 8,
        };
        assert_eq!(
            r.expand(
                "make -j{jobs} PREFIX={prefix} T={tools} V={version}",
                &r.parts[0],
                &dirs
            ),
            "make -j8 PREFIX=/pkg/probe T=/t V=1.3.1"
        );
    }

    #[test]
    fn problems_are_named() {
        let bad = GOOD
            .replace("prefix = \"/pkg/probe\"", "prefix = \"/usr\"")
            .replace("version = \"1.3.1\"", "version = \"1.3.1-rc1\"")
            .replace("https://example.org", "http://example.org")
            .replace("sha256 = \"9a93", "sha256 = \"zz93");
        let p = Recipe::parse(&bad).unwrap().problems(true);
        assert_eq!(p.len(), 4, "{p:?}");
        // an unpinned part is fine while writing, not for a build
        let open = GOOD.replace(
            "9a93b2b7dfdac77ceba5a558a580e74667dd6fede4585b91eefb60f03b72df23",
            "",
        );
        let r = Recipe::parse(&open).unwrap();
        assert!(r.problems(false).is_empty());
        assert_eq!(r.problems(true).len(), 1);
    }

    #[test]
    fn a_part_can_be_tied_to_another() {
        let tied = r#"
[[part]]
name = "zlib-data"
kind = "tool"
version = "1.3.1"
follows = "zlib"
source = "https://example.org/archive/0123abcd.tar.gz"
upstream = "https://example.org/data"
tag = ""
license = "MIT"
why = "the state zlib 1.3.1 asks for"
build = ["true"]
"#;
        let r = Recipe::parse(&format!("{GOOD}{tied}")).unwrap();
        assert_eq!(r.problems(false), Vec::<String>::new());
        assert_eq!(r.parts[1].archive(), "zlib-data-1.3.1.tar.gz");
        // the version must move together with the part it follows
        let stale = Recipe::parse(&format!(
            "{GOOD}{}",
            tied.replace("1.3.1\"\nfollows", "1.3.0\"\nfollows")
        ))
        .unwrap();
        assert_eq!(stale.problems(false).len(), 1);
    }

    #[test]
    fn unknown_keys_and_tool_only_recipes_are_refused() {
        assert!(Recipe::parse(&GOOD.replace("why =", "wy =")).is_err());
        let tools = GOOD.replace("kind = \"runtime\"", "kind = \"tool\"");
        assert_eq!(Recipe::parse(&tools).unwrap().problems(true).len(), 1);
    }
}
