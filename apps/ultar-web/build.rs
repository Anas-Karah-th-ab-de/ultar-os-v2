//! Mit Web-Engine wird nur gebaut, wenn das Laufzeit-Bündel (Buildroot-Sysroot) angegeben ist:
//!   ULTAR_WEB_SYSROOT=<buildroot>/output/staging  (enthält usr/lib/libWPEWebKit-2.0.so …)
//! Ohne Angabe entsteht ein Platzhalter, damit `cargo build --workspace` überall durchläuft.
fn main() {
    println!("cargo:rerun-if-env-changed=ULTAR_WEB_SYSROOT");
    println!("cargo::rustc-check-cfg=cfg(ultar_engine)");
    let Ok(sys) = std::env::var("ULTAR_WEB_SYSROOT") else {
        return;
    };
    println!("cargo:rustc-cfg=ultar_engine");
    if std::env::var("ULTAR_WEB_CHECK_ONLY").is_ok() {
        return;
    }
    for d in ["usr/lib", "lib"] {
        println!("cargo:rustc-link-search=native={sys}/{d}");
    }
    for l in ["WPEWebKit-2.0", "gio-2.0", "gobject-2.0", "glib-2.0"] {
        println!("cargo:rustc-link-lib=dylib={l}");
    }
}
