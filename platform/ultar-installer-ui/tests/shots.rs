//! Writes the installer's renders of the boards (with `ULTAR_CONFORMANCE_OUT=<dir>`).
mod common;
use common::*;
use ultar_i18n::Lang;
use ultar_installer_ui::wizard::{Encryption, Step};
use ultar_ui::tokens::ThemeKind;
use ultar_ui::Size;

#[test]
fn renders_the_boards() {
    let Some(out) = std::env::var_os("ULTAR_CONFORMANCE_OUT") else {
        return;
    };
    let out = std::path::PathBuf::from(out);
    let dir = scratch("shots");
    let src = medium(&dir);
    let tgt = target(&dir, 512 * MIB);
    for (board, step) in [
        ("Installer", Step::Components),
        ("InstallerDisk", Step::Disk),
        ("InstallerDrivers", Step::Drivers),
        ("InstallerSummary", Step::Summary),
    ] {
        let world = world();
        let mut w = wizard_at(&src, vec![disk_info(&tgt, "Samsung SSD 980")], step);
        w.encryption = Encryption::None;
        let mut app = app_with(
            &world,
            w,
            Lang::Ar,
            ThemeKind::Dark,
            Size::new(1280.0, 800.0),
        );
        if step == Step::Drivers {
            app.load_devices();
            app.rebuild().unwrap();
        }
        write_png(&out.join(format!("{board}-app.png")), &mut app.ui);
    }
    std::fs::remove_dir_all(dir).unwrap();
}
