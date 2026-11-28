//! The runtime bundle of the engine: `/usr/lib/ultar/web/{libexec,lib,share/fonts,etc/fonts}`
//! (built by `scripts/bundle-servo-engine.sh`, libraries from `tools/ultar-paketbau/pakete/
//! servo-laufzeit`). The host stands in `libexec/`; it finds its libraries through its own
//! run path (`$ORIGIN/../lib`, set when the bundle is made), so nothing has to be exported from
//! outside. What a library cannot be told through the run path, this module sets in the
//! environment before Servo starts: the font configuration (only the fonts of the bundle), the
//! surfaceless EGL platform with software rendering, no caches on disk.
//!
//! Outside a bundle (a development build on Linux, `cargo run`) nothing is changed.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// A bundle found around the running program.
#[derive(Debug, PartialEq, Eq)]
pub struct Bundle {
    root: PathBuf,
}

/// One variable the host sets.
#[derive(Debug, PartialEq, Eq)]
pub struct Var {
    pub name: &'static str,
    pub value: OsString,
}

impl Bundle {
    /// The bundle `exe` belongs to: `<root>/libexec/<program>` with `lib/` and a font
    /// configuration next to it. `is_file`/`is_dir` ask the file system (a test passes its own).
    pub fn around(
        exe: &Path,
        is_dir: &dyn Fn(&Path) -> bool,
        is_file: &dyn Fn(&Path) -> bool,
    ) -> Option<Bundle> {
        let libexec = exe.parent()?;
        if libexec.file_name()? != "libexec" {
            return None;
        }
        let root = libexec.parent()?;
        (is_dir(&root.join("lib")) && is_file(&root.join("etc/fonts/fonts.conf"))).then(|| Bundle {
            root: root.to_path_buf(),
        })
    }

    /// The variables to set. `already` tells whether the environment has a variable (a value
    /// given from outside wins for the choices of the operator; the ones that keep the page
    /// engine closed – fonts, caches – are always the bundle's). `is_dir` asks the file system.
    pub fn vars(&self, already: &dyn Fn(&str) -> bool, is_dir: &dyn Fn(&Path) -> bool) -> Vec<Var> {
        let at = |rel: &str| self.root.join(rel).into_os_string();
        let mut out = vec![
            // fonts: only those of the bundle, never those of the machine
            Var {
                name: "FONTCONFIG_FILE",
                value: at("etc/fonts/fonts.conf"),
            },
            Var {
                name: "FONTCONFIG_PATH",
                value: at("etc/fonts"),
            },
            // Mesa: the driver library (libgallium) is found through the run path of libEGL; nothing
            // is cached on disk (nothing is kept)
            Var {
                name: "MESA_SHADER_CACHE_DISABLE",
                value: "true".into(),
            },
            Var {
                name: "MESA_GLSL_CACHE_DISABLE",
                value: "true".into(),
            },
        ];
        // choices: EGL without a window system, rendering by the processor (llvmpipe); the
        // Ultar kernel does not hand a program the wide vector registers yet, so llvmpipe
        // generates 128-bit code only
        let defaults: [(&'static str, &str); 3] = [
            ("EGL_PLATFORM", "surfaceless"),
            ("LIBGL_ALWAYS_SOFTWARE", "1"),
            ("LP_NATIVE_VECTOR_WIDTH", "128"),
        ];
        for (name, value) in defaults {
            if !already(name) {
                out.push(Var {
                    name,
                    value: value.into(),
                });
            }
        }
        // certificates: the platform verifier of Servo's network stack panics when the machine has
        // no CA store, so the bundle brings one (Mozilla's list, `share/ssl/cacert.pem`)
        if !already("SSL_CERT_FILE") {
            out.push(Var {
                name: "SSL_CERT_FILE",
                value: at("share/ssl/cacert.pem"),
            });
        }
        out.extend(self.media_vars(is_dir));
        out
    }

    /// Sound and video: the plug-ins of the bundle and a registry that is not kept; nothing when
    /// the bundle has no plug-ins.
    fn media_vars(&self, is_dir: &dyn Fn(&Path) -> bool) -> Vec<Var> {
        let at = |rel: &str| self.root.join(rel).into_os_string();
        if !is_dir(&self.root.join("lib/gstreamer-1.0")) {
            return Vec::new();
        }
        vec![
            Var {
                name: "GST_PLUGIN_SYSTEM_PATH_1_0",
                value: at("lib/gstreamer-1.0"),
            },
            Var {
                name: "GST_PLUGIN_PATH_1_0",
                value: at("lib/gstreamer-1.0"),
            },
            Var {
                name: "GST_PLUGIN_SCANNER",
                value: at("libexec/gst-plugin-scanner"),
            },
            Var {
                name: "GST_REGISTRY_1_0",
                value: "/tmp/ultar-web-gst-registry.bin".into(),
            },
        ]
    }
}

/// Finds the bundle around the running program and sets its environment. Call first thing in
/// `main`, before any thread exists (Servo starts its threads later). Returns the bundle, or
/// `None` when the program does not run from one.
pub fn setup() -> Option<Bundle> {
    let exe = std::env::current_exe().ok()?;
    let bundle = Bundle::around(&exe, &|p| p.is_dir(), &|p| p.is_file())?;
    let vars = bundle.vars(&|n| std::env::var_os(n).is_some(), &|p| p.is_dir());
    for v in vars {
        std::env::set_var(v.name, v.value);
    }
    Some(bundle)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUNDLE_ROOT: &str = "/usr/lib/ultar/web";

    fn bundle() -> Bundle {
        Bundle {
            root: PathBuf::from(BUNDLE_ROOT),
        }
    }

    fn value<'a>(vars: &'a [Var], name: &str) -> Option<&'a OsString> {
        vars.iter().find(|v| v.name == name).map(|v| &v.value)
    }

    #[test]
    fn a_program_in_libexec_with_lib_and_fonts_belongs_to_the_bundle() {
        let exe = Path::new("/usr/lib/ultar/web/libexec/ultar-web-engine");
        let b = Bundle::around(exe, &|p| p == Path::new("/usr/lib/ultar/web/lib"), &|p| {
            p == Path::new("/usr/lib/ultar/web/etc/fonts/fonts.conf")
        });
        assert_eq!(b, Some(bundle()));
    }

    #[test]
    fn a_program_elsewhere_belongs_to_no_bundle() {
        let all = |_: &Path| true;
        // a development build
        let dev = Path::new("/root/target/release/ultar-web-engine");
        assert_eq!(Bundle::around(dev, &all, &all), None);
        // in libexec but without the libraries or the font configuration
        let exe = Path::new("/opt/x/libexec/ultar-web-engine");
        assert_eq!(Bundle::around(exe, &|_| false, &all), None);
        assert_eq!(Bundle::around(exe, &all, &|_| false), None);
    }

    #[test]
    fn fonts_are_the_bundles_and_nothing_is_cached() {
        let vars = bundle().vars(&|_| false, &|_| false);
        assert_eq!(
            value(&vars, "FONTCONFIG_FILE"),
            Some(&OsString::from("/usr/lib/ultar/web/etc/fonts/fonts.conf"))
        );
        assert_eq!(
            value(&vars, "FONTCONFIG_PATH"),
            Some(&OsString::from("/usr/lib/ultar/web/etc/fonts"))
        );
        assert_eq!(
            value(&vars, "MESA_SHADER_CACHE_DISABLE"),
            Some(&OsString::from("true"))
        );
    }

    #[test]
    fn the_defaults_are_surfaceless_software_and_narrow_vectors() {
        let vars = bundle().vars(&|_| false, &|_| false);
        assert_eq!(
            value(&vars, "EGL_PLATFORM"),
            Some(&OsString::from("surfaceless"))
        );
        assert_eq!(
            value(&vars, "LIBGL_ALWAYS_SOFTWARE"),
            Some(&OsString::from("1"))
        );
        assert_eq!(
            value(&vars, "LP_NATIVE_VECTOR_WIDTH"),
            Some(&OsString::from("128"))
        );
    }

    #[test]
    fn a_choice_given_from_outside_is_kept_but_the_fonts_are_not_negotiable() {
        // the operator set all three choices and a font file of the machine
        let given = |n: &str| {
            matches!(
                n,
                "EGL_PLATFORM"
                    | "LIBGL_ALWAYS_SOFTWARE"
                    | "LP_NATIVE_VECTOR_WIDTH"
                    | "FONTCONFIG_FILE"
            )
        };
        let vars = bundle().vars(&given, &|_| false);
        assert_eq!(value(&vars, "EGL_PLATFORM"), None);
        assert_eq!(value(&vars, "LIBGL_ALWAYS_SOFTWARE"), None);
        assert_eq!(value(&vars, "LP_NATIVE_VECTOR_WIDTH"), None);
        assert!(value(&vars, "FONTCONFIG_FILE").is_some());
    }

    #[test]
    fn the_sound_plugins_are_named_only_when_the_bundle_has_them() {
        let none = bundle().vars(&|_| false, &|_| false);
        assert_eq!(value(&none, "GST_PLUGIN_SYSTEM_PATH_1_0"), None);
        let with = bundle().vars(&|_| false, &|p| p.ends_with("lib/gstreamer-1.0"));
        assert_eq!(
            value(&with, "GST_PLUGIN_SYSTEM_PATH_1_0"),
            Some(&OsString::from("/usr/lib/ultar/web/lib/gstreamer-1.0"))
        );
        assert_eq!(
            value(&with, "GST_PLUGIN_SCANNER"),
            Some(&OsString::from(
                "/usr/lib/ultar/web/libexec/gst-plugin-scanner"
            ))
        );
    }

    #[test]
    fn the_bundle_brings_its_own_certificate_store() {
        let vars = bundle().vars(&|_| false, &|_| false);
        assert_eq!(
            value(&vars, "SSL_CERT_FILE"),
            Some(&OsString::from("/usr/lib/ultar/web/share/ssl/cacert.pem"))
        );
        let given = bundle().vars(&|n| n == "SSL_CERT_FILE", &|_| false);
        assert_eq!(value(&given, "SSL_CERT_FILE"), None);
    }

    #[test]
    fn this_test_binary_is_not_a_bundle() {
        assert_eq!(setup(), None);
    }
}
