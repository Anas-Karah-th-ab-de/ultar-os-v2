//! /data-Verschlüsselung über LUKS2 (cryptsetup). aes-xts-plain64/512, Argon2id.
//!
//! WICHTIG: `enable()` formatiert die Datenpartition neu → alle Daten weg. Der Aufrufer muss
//! vorher warnen/sichern. `unlock()` öffnet nur; das Mounten macht ultar-init.

use crate::{data_partition, CRYPTSETUP, DATA_MAP};
use std::io::Write;
use std::process::{Command, Stdio};

/// Passwort an stdin von cryptsetup geben und auf Ende warten.
fn run_with_pw(args: &[&str], pw: &str) -> Result<(), String> {
    let mut child = Command::new(CRYPTSETUP)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("{CRYPTSETUP} start: {e}"))?;
    if let Some(si) = child.stdin.as_mut() {
        si.write_all(pw.as_bytes()).map_err(|e| e.to_string())?;
        si.write_all(b"\n").ok();
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// Ist die Datenpartition bereits LUKS-verschlüsselt?
pub fn is_encrypted() -> bool {
    Command::new(CRYPTSETUP)
        .args(["isLuks", &data_partition()])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Ist das entschlüsselte Mapper-Gerät offen?
pub fn is_open() -> bool {
    std::path::Path::new(&format!("/dev/mapper/{DATA_MAP}")).exists()
}

/// Verschlüsselung einschalten: LUKS2 formatieren + öffnen + ext4 anlegen. LÖSCHT die Partition.
pub fn enable(password: &str) -> Result<(), String> {
    let part = data_partition();
    run_with_pw(
        &[
            "luksFormat",
            "--type",
            "luks2",
            "--cipher",
            "aes-xts-plain64",
            "--key-size",
            "512",
            "--pbkdf",
            "argon2id",
            "--batch-mode",
            &part,
        ],
        password,
    )?;
    run_with_pw(&["luksOpen", &part, DATA_MAP], password)?;
    // frisches ext4 im Mapper-Gerät
    let st = Command::new("/usr/lib/ultar/mke2fs")
        .args(["-t", "ext4", "-F", &format!("/dev/mapper/{DATA_MAP}")])
        .status()
        .map_err(|e| format!("mke2fs: {e}"))?;
    if !st.success() {
        return Err("ext4 anlegen fehlgeschlagen".into());
    }
    Ok(())
}

/// Verschlüsselung öffnen (nicht mounten). Für den Start nach Passworteingabe.
pub fn unlock(password: &str) -> Result<(), String> {
    if is_open() {
        return Ok(());
    }
    run_with_pw(&["luksOpen", &data_partition(), DATA_MAP], password)
}

/// Passwort ändern (altes + neues).
pub fn change_password(old: &str, new: &str) -> Result<(), String> {
    let part = data_partition();
    // cryptsetup erwartet altes dann neues über stdin.
    let mut child = Command::new(CRYPTSETUP)
        .args(["luksChangeKey", "--pbkdf", "argon2id", &part])
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    if let Some(si) = child.stdin.as_mut() {
        let _ = si.write_all(format!("{old}\n{new}\n").as_bytes());
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// Mapper-Gerät schließen (nach unmount).
pub fn close() -> Result<(), String> {
    let st = Command::new(CRYPTSETUP)
        .args(["luksClose", DATA_MAP])
        .status()
        .map_err(|e| e.to_string())?;
    if st.success() {
        Ok(())
    } else {
        Err("luksClose".into())
    }
}

/// Entschlüsseln (in-place) — hebt die Verschlüsselung auf. Schwer/langwierig.
pub fn disable(password: &str) -> Result<(), String> {
    run_with_pw(
        &["reencrypt", "--decrypt", "--batch-mode", &data_partition()],
        password,
    )
}
