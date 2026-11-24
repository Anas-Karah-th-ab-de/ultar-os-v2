//! Rule `brand` on fixture repositories: literals, catalogs, os-release, manifests, mark.

mod common;
use common::{at, Fixture};
use ultar_check::rules::Rule;

const SOURCE: &str = "system/core/ultar-brand";

/// A fixture with the brand source: the crate, its three catalogs and `os-release`.
fn with_source() -> Fixture {
    let f = Fixture::new();
    f.write(
        &format!("{SOURCE}/src/lib.rs"),
        "pub const PRODUCT: &str = \"Ultar OS\";\npub const VENDOR: &str = \"KM Code Craft\";\n",
    );
    f.write(
        &format!("{SOURCE}/locales/ar.ftl"),
        "# c\nbrand.by = من KM Code Craft\nbrand.about.version = الإصدار\n",
    );
    f.write(
        &format!("{SOURCE}/locales/en.ftl"),
        "brand.by = by KM Code Craft\nbrand.about.version = Version\n",
    );
    f.write(
        &format!("{SOURCE}/locales/de.ftl"),
        "brand.by = von KM Code Craft\nbrand.about.version = Version\n",
    );
    f.write("rootfs/etc/os-release", ultar_brand::release::OS_RELEASE);
    f
}

#[test]
fn a_complete_source_passes() {
    let f = with_source();
    assert_eq!(f.lines(Rule::Brand), Vec::<String>::new());
}

#[test]
fn literals_outside_the_source_are_flagged() {
    let f = with_source();
    f.write(
        "apps/about/src/lib.rs",
        "pub fn a() -> &'static str {\n    \"Ultar OS\"\n}\nconst V: &str = \"made by km code craft\";\nconst W: &str = \"https://code-craft.tech/x\";\nconst M: &[u8] = b\"ak@code-craft.tech\";\nconst D: &str = \"Mohamad Anas Kara Mohamad\";\n",
    );
    assert_eq!(
        f.lines(Rule::Brand),
        [
            "apps/about/src/lib.rs:2: brand: brand term written in code: Ultar OS in \"Ultar OS\" (use ultar-brand)",
            "apps/about/src/lib.rs:4: brand: brand term written in code: KM Code Craft in \"made by km code craft\" (use ultar-brand)",
            "apps/about/src/lib.rs:5: brand: brand term written in code: https://code-craft.tech in \"https://code-craft.tech/x\" (use ultar-brand)",
            "apps/about/src/lib.rs:6: brand: brand term written in code: info@code-craft.tech in \"ak@code-craft.tech\" (use ultar-brand)",
            "apps/about/src/lib.rs:7: brand: brand term written in code: Mohamad Anas Kara Mohamad in \"Mohamad Anas Kara Mohamad\" (use ultar-brand)",
        ]
    );
}

#[test]
fn tests_comments_services_and_constants_are_not_flagged() {
    let f = with_source();
    f.write(
        "system/services/cloud/src/lib.rs",
        r#"//! Client of KM Code Craft's server for Ultar OS.
// "Ultar OS" in a comment
pub const BASE: &str = "https://ultar.code-craft.tech";
pub const NAME: &str = ultar_brand::PRODUCT;
pub const LINE: &str = concat!("Starting ", ultar_brand::product!());
pub const KERNEL: &str = "Ultar-Kern";

#[cfg(test)]
mod tests {
    const T: &str = "Ultar OS by KM Code Craft";
}
"#,
    );
    f.write(
        "system/services/cloud/tests/api.rs",
        "const T: &str = \"KM Code Craft\";\n",
    );
    assert!(f.check(&[Rule::Brand]).passed());
}

#[test]
fn every_language_must_cover_the_same_brand_keys() {
    let f = with_source();
    f.write(
        &format!("{SOURCE}/locales/en.ftl"),
        "brand.by = by KM Code Craft\nbrand.only.en = Only here\n",
    );
    let lines = f.lines(Rule::Brand);
    let expect = |text: &str| assert!(lines.iter().any(|l| l == text), "{text}\n{lines:#?}");
    expect("system/core/ultar-brand/locales/en.ftl:1: brand: brand key `brand.about.version` is missing in `en` (present in `ar`)");
    expect("system/core/ultar-brand/locales/ar.ftl:1: brand: brand key `brand.only.en` is missing in `ar` (present in `en`)");
    expect("system/core/ultar-brand/locales/de.ftl:1: brand: brand key `brand.only.en` is missing in `de` (present in `en`)");
    assert_eq!(lines.len(), 4, "{lines:#?}");
}

#[test]
fn a_missing_language_and_latin_only_arabic_are_flagged() {
    let f = with_source();
    std::fs::remove_file(f.root().join(format!("{SOURCE}/locales/de.ftl"))).unwrap();
    f.write(
        &format!("{SOURCE}/locales/ar.ftl"),
        "brand.by = by KM Code Craft\nbrand.about.version = الإصدار\n",
    );
    assert_eq!(
        f.lines(Rule::Brand),
        [
            "system/core/ultar-brand/locales/ar.ftl:1: brand: Arabic brand text `brand.by` has no Arabic letters",
            "system/core/ultar-brand/locales/de.ftl:1: brand: the brand catalog of language `de` does not exist",
        ]
    );
}

#[test]
fn catalogs_elsewhere_may_name_the_product_but_only_as_spelled() {
    let f = with_source();
    f.write(
        "apps/setup/locales/en.ftl",
        "setup.title = Install Ultar OS\nsetup.sub = ULTAR OS by Km Code Craft\nsetup.mail = write to ak@code-craft.tech\n",
    );
    f.write(
        "apps/setup/locales/ar.ftl",
        "setup.title = تثبيت Ultar OS\nbrand.vendor = KM Code Craft\nbrand.extra = نص\nsystem.vendor = KM CodeCraft\nsystem.name = Ultar OS\n",
    );
    assert_eq!(
        f.lines(Rule::Brand),
        [
            "apps/setup/locales/ar.ftl:2: brand: key `brand.vendor` is not translated: it comes from ultar-brand, not from a catalog",
            "apps/setup/locales/ar.ftl:3: brand: brand key `brand.extra` outside system/core/ultar-brand/locales",
            "apps/setup/locales/ar.ftl:4: brand: key `system.vendor` differs from the brand source; expected: KM Code Craft",
            "apps/setup/locales/en.ftl:2: brand: key `setup.sub` spells \"ULTAR OS\" instead of \"Ultar OS\"",
            "apps/setup/locales/en.ftl:2: brand: key `setup.sub` spells \"Km Code Craft\" instead of \"KM Code Craft\"",
            "apps/setup/locales/en.ftl:3: brand: key `setup.mail` differs from the brand source; expected: info@code-craft.tech",
        ]
    );
}

#[test]
fn os_release_must_be_the_text_of_the_source() {
    let f = with_source();
    let good = ultar_brand::release::OS_RELEASE;
    f.write(
        "rootfs/etc/os-release",
        &good.replace("KM Code Craft", "KM"),
    );
    assert_eq!(
        f.lines(Rule::Brand),
        ["rootfs/etc/os-release:5: brand: os-release differs from the brand source; expected here: VENDOR_NAME=\"KM Code Craft\""]
    );
    f.write("rootfs/etc/os-release", &format!("{good}VERSION_ID=1\n"));
    assert_eq!(f.places(Rule::Brand), [at("rootfs/etc/os-release", 7)]);
    f.write("rootfs/etc/os-release", good.trim_end());
    assert_eq!(f.places(Rule::Brand), [at("rootfs/etc/os-release", 7)]);
    f.write("rootfs/etc/os-release", "");
    assert_eq!(f.places(Rule::Brand), [at("rootfs/etc/os-release", 1)]);
    std::fs::remove_file(f.root().join("rootfs/etc/os-release")).unwrap();
    assert_eq!(
        f.lines(Rule::Brand),
        ["rootfs/etc/os-release:1: brand: os-release does not exist (source: ultar_brand::release::OS_RELEASE)"]
    );
    f.write("rootfs/etc/os-release", good);
    assert!(f.check(&[Rule::Brand]).passed());
}

#[test]
fn a_repository_without_the_source_needs_no_os_release() {
    let f = Fixture::new();
    f.write("apps/a/src/lib.rs", "pub const A: &str = \"plain\";\n");
    assert!(f.check(&[Rule::Brand]).passed());
}

#[test]
fn manifest_authors_must_be_the_one_of_the_source() {
    let f = with_source();
    f.write(
        "apps/a/Cargo.toml",
        "[package]\nname = \"a\"\nauthors = [\"KM Code Craft <https://code-craft.tech>\"]\n",
    );
    f.write(
        "apps/b/Cargo.toml",
        "[package]\nname = \"b\"\nauthors=[\"KM CodeCraft\"]\n",
    );
    f.write(
        "apps/c/Cargo.toml",
        "[package]\nname = \"c\"\nauthors.workspace = true\n",
    );
    assert_eq!(
        f.lines(Rule::Brand),
        ["apps/b/Cargo.toml:3: brand: authors differs from the brand source; expected: [\"KM Code Craft <https://code-craft.tech>\"]"]
    );
}

#[test]
fn scripts_and_documents_name_only_the_public_contact_address() {
    let f = with_source();
    f.write(
        "scripts/package.sh",
        "#!/bin/sh\necho 'Maintainer: KM Code Craft <ak@code-craft.tech>'\necho info@code-craft.tech\n",
    );
    f.write("docs/CONTACT.md", "Write to info@code-craft.tech.\n");
    f.write("docs/OLD.md", "\n\nWrite to support@code-craft.tech.\n");
    assert_eq!(
        f.lines(Rule::Brand),
        [
            "docs/OLD.md:3: brand: address support@code-craft.tech is not the public contact address; expected: info@code-craft.tech",
            "scripts/package.sh:2: brand: address ak@code-craft.tech is not the public contact address; expected: info@code-craft.tech",
        ]
    );
}

fn icon_table(entry: &str) -> String {
    format!("icons! {{\n    Home, \"home\", false, \"Home folder.\",\n        \"M4 11l8-7 8 7v9h-5v-6H9v6H4z\";\n{entry}}}\n")
}

#[test]
fn the_mark_of_the_icon_set_must_be_the_path_of_the_source() {
    let f = with_source();
    let table = "platform/ultar-ui/src/icon/mod.rs";
    // no mark in the set yet: nothing to compare
    f.write(table, &icon_table(""));
    assert!(f.check(&[Rule::Brand]).passed());
    let good = format!(
        "    Mark, \"mark\", false, \"Brand mark.\",\n        \"{}\";\n",
        ultar_brand::mark::PATH
    );
    f.write(table, &icon_table(&good));
    assert!(f.check(&[Rule::Brand]).passed());
    // split over two literals is the same path
    let split = "    Mark, \"mark\", false, \"Brand mark.\",\n        \"M12 3a9 9 0 1 0 9 9\"\n        \"M12 8a4 4 0 1 0 4 4\";\n";
    f.write(table, &icon_table(split));
    assert!(f.check(&[Rule::Brand]).passed());
    f.write(table, &icon_table(&good.replace("false", "true")));
    assert_eq!(f.places(Rule::Brand), [at(table, 4)]);
    f.write(table, &icon_table(&good.replace("9 9 0", "8 8 0")));
    assert_eq!(
        f.lines(Rule::Brand),
        [format!("{table}:4: brand: the brand mark of the icon set differs from the source; expected path: {} (never mirrored)", ultar_brand::mark::PATH)]
    );
}

#[test]
fn legacy_literals_are_held_by_the_baseline_and_may_not_grow() {
    let f = with_source();
    let one = "pub const A: &str = \"Ultar OS\";\n";
    f.write("platform/old/src/lib.rs", one);
    f.baseline(&[("brand", "platform/old/src/lib.rs", 1)]);
    let report = f.check(&[Rule::Brand]);
    assert!(report.passed());
    assert_eq!(report.baselined, 1);
    f.write(
        "platform/old/src/lib.rs",
        &format!("{one}pub const B: &str = \"KM Code Craft\";\n"),
    );
    assert!(!f.check(&[Rule::Brand]).passed());
}

#[test]
fn the_rule_is_part_of_all_and_has_a_name() {
    assert_eq!(Rule::from_name("brand"), Some(Rule::Brand));
    assert!(Rule::ALL.contains(&Rule::Brand));
    assert!(Rule::Brand.needs_sources());
}
