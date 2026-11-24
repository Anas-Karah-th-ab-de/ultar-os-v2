//! Building the package (`ultar-pkg` packs and signs; the result is verified against the
//! developer key before it is kept) and installing it for testing through the launcher.

use crate::devkey::DevKey;
use crate::error::AppError;
use crate::project::Checked;
use std::path::{Path, PathBuf};

/// Output folder inside the project.
pub const OUT: &str = "out";

/// Where the package of `checked` is written.
pub fn package_path(checked: &Checked) -> PathBuf {
    checked
        .dir
        .join(OUT)
        .join(format!("{}-{}.uap", checked.id, checked.version))
}

/// A throw-away trusted-keys folder holding only the developer's key, named after the
/// publisher (the launcher binds store ids to the key file's name the same way).
fn trust_dir(out: &Path, checked: &Checked, key: &DevKey) -> Result<PathBuf, AppError> {
    use std::os::unix::fs::PermissionsExt;
    let dir = out.join(format!(".trust-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    let file = dir.join(format!("{}.ed25519", checked.publisher));
    std::fs::write(&file, format!("{}\n", key.public_hex))?;
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644))?;
    Ok(dir)
}

/// Packs `payload/` with the manifest, signs it with `key`, verifies it and keeps it at
/// [`package_path`]. On any failure nothing is left behind.
pub fn build(checked: &Checked, key: &DevKey) -> Result<PathBuf, AppError> {
    let target = package_path(checked);
    let out = target.parent().ok_or(AppError::Io)?.to_path_buf();
    std::fs::create_dir_all(&out)?;
    let part = out.join(format!(".{}-{}.uap.part", checked.id, checked.version));
    let result = (|| {
        ultar_pkg::pack_and_sign(
            &checked.manifest(),
            &checked.payload(),
            &part,
            key.private_hex(),
        )
        .map_err(|_| AppError::Pack)?;
        let trust = trust_dir(&out, checked, key)?;
        let verified = ultar_pkg::verify(&part, &trust);
        let _ = std::fs::remove_dir_all(&trust);
        match verified {
            Ok(m) if m.id == checked.id && m.version == checked.version => {}
            _ => return Err(AppError::Verify),
        }
        std::fs::File::open(&part)?.sync_all()?;
        std::fs::rename(&part, &target)?;
        Ok(target.clone())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    result
}

/// Asks the launcher to install `package` (it verifies the signature against the device's
/// trusted keys). Answers the installed app id.
#[cfg(feature = "bus")]
pub fn install(bus: &ultar_bus::Client, package: &Path) -> Result<String, AppError> {
    use ultar_bus::messages::launcher;
    use ultar_bus::{BusError, ErrorCode};
    let call = launcher::Install {
        package: package.display().to_string(),
    };
    match bus.call(&call, std::time::Duration::from_secs(60)) {
        Ok(reply) => Ok(reply.app),
        Err(
            BusError::Timeout
            | BusError::NotConnected
            | BusError::Closed
            | BusError::Remote(
                ErrorCode::UnknownService | ErrorCode::UnknownMethod | ErrorCode::ServiceGone,
            ),
        ) => Err(AppError::NoLauncher),
        Err(_) => Err(AppError::Refused),
    }
}
