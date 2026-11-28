//! The base apps' own manifests (`apps/*/ultar-app.toml`) are ones the launcher accepts, with
//! the `arch` and `exec` the release adds (tools/release/build-apps.sh). (Files still carries
//! the shell id `org.ultar.files`; its image manifest is written by tools/vm/make-vmware.sh.)

use ultar_launcher::manifest::Manifest;

#[test]
fn base_app_manifests_parse() {
    let apps = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../apps");
    for app in [
        "ultar-settings",
        "ultar-store",
        "ultar-editor",
        "ultar-doc",
        "ultar-terminal",
        "ultar-web",
    ] {
        let file = apps.join(app).join("ultar-app.toml");
        let text = std::fs::read_to_string(&file).unwrap();
        let first = text.find("\n[").map_or(text.len(), |i| i + 1);
        let with = format!(
            "{}arch = \"x86_64\"\nexec = \"/usr/bin/app\"\n\n{}",
            &text[..first],
            &text[first..]
        );
        if let Err(e) = Manifest::parse(with.as_bytes()) {
            panic!("{}: {e:?}", file.display());
        }
    }
}
