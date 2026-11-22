//! Kiri als Docker-Ersatz: schlanke Container-Laufzeit in Rust (Apache-2.0, github.com/libracoder/kiri),
//! spricht die Docker-Engine-API, läuft unter Linux direkt auf dem Kernel (ohne VM, ohne containerd/runc).
//!
//! * `zelle kiri einrichten`: neueste Version (download.runkiri.com/latest.json), Paket `kiri-lite_<v>_<arch>.deb`
//!   aus der GitHub-Veröffentlichung, SHA-256 gegen `SHA256SUMS-linux` geprüft, nach /usr/lib/ultar/kiri entpackt,
//!   Dienst `kiri` für ultar-init. Docker-Werkzeuge (docker, compose) sprechen danach mit /run/kiri/docker.sock.
//! * Verknüpfen von Containern: gemeinsames Netz (sie rufen sich beim Namen) und gemeinsames Volume.
//! * Für Empfehlungen: laufende Container und ihre Abbilder.

use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const SOCKET: &str = "/run/kiri/docker.sock";
pub const ZIEL: &str = "/usr/lib/ultar/kiri";

pub struct Docker {
    pub socket: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Container {
    pub id: String,
    pub name: String,
    pub abbild: String,
    pub zustand: String,
}

impl Docker {
    /// Kiri-Socket; sonst DOCKER_HOST=unix://… oder /run/docker.sock
    pub fn standard() -> Docker {
        let s = std::env::var("ULTAR_KIRI_SOCKET")
            .ok()
            .or_else(|| std::env::var("DOCKER_HOST").ok().and_then(|h| h.strip_prefix("unix://").map(String::from)))
            .unwrap_or_else(|| if Path::new(SOCKET).exists() { SOCKET.into() } else { "/run/docker.sock".into() });
        Docker { socket: s.into() }
    }

    /// eine Anfrage (HTTP/1.1 über den Unix-Socket); liefert Status und Rumpf
    pub fn anfrage(&self, methode: &str, pfad: &str, rumpf: Option<&serde_json::Value>) -> Result<(u16, Vec<u8>), String> {
        let mut s = UnixStream::connect(&self.socket).map_err(|e| format!("Container-Dienst ({}): {e}", self.socket.display()))?;
        s.set_read_timeout(Some(Duration::from_secs(60))).ok();
        let b = rumpf.map(|v| v.to_string()).unwrap_or_default();
        let kopf = format!("{methode} {pfad} HTTP/1.1\r\nHost: docker\r\nUser-Agent: ultar-zellen\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n", b.len());
        s.write_all(kopf.as_bytes()).and_then(|_| s.write_all(b.as_bytes())).map_err(|e| e.to_string())?;
        let mut roh = Vec::new();
        s.read_to_end(&mut roh).map_err(|e| e.to_string())?;
        let trenn = roh.windows(4).position(|w| w == b"\r\n\r\n").ok_or("kaputte Antwort")?;
        let k = String::from_utf8_lossy(&roh[..trenn]).into_owned();
        let status: u16 = k.split_whitespace().nth(1).and_then(|x| x.parse().ok()).unwrap_or(0);
        let mut body = roh[trenn + 4..].to_vec();
        if k.to_lowercase().contains("transfer-encoding: chunked") {
            body = entstuecken(&body);
        }
        Ok((status, body))
    }

    fn json(&self, methode: &str, pfad: &str, rumpf: Option<&serde_json::Value>) -> Result<serde_json::Value, String> {
        let (st, b) = self.anfrage(methode, pfad, rumpf)?;
        if !(200..300).contains(&st) {
            return Err(format!("{methode} {pfad}: {st} {}", String::from_utf8_lossy(&b).trim()));
        }
        if b.is_empty() {
            return Ok(serde_json::Value::Null);
        }
        serde_json::from_slice(&b).map_err(|e| e.to_string())
    }

    pub fn version(&self) -> Result<String, String> {
        let v = self.json("GET", "/version", None)?;
        Ok(format!("{} {}", v["Platform"]["Name"].as_str().or(v["Components"][0]["Name"].as_str()).unwrap_or("Engine"), v["Version"].as_str().unwrap_or("?")))
    }

    pub fn container(&self) -> Result<Vec<Container>, String> {
        let v = self.json("GET", "/containers/json?all=1", None)?;
        Ok(v.as_array()
            .map(|a| {
                a.iter()
                    .map(|c| Container {
                        id: c["Id"].as_str().unwrap_or_default().into(),
                        name: c["Names"][0].as_str().unwrap_or_default().trim_start_matches('/').into(),
                        abbild: c["Image"].as_str().unwrap_or_default().into(),
                        zustand: c["State"].as_str().unwrap_or_default().into(),
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    /// zwei Container verknüpfen: gemeinsames Netz (Namen auflösbar) + gemeinsames Volume für Neuanlagen
    pub fn verknuepfen(&self, a: &str, b: &str) -> Result<String, String> {
        let netz = format!("ultar-{}-{}", a.min(b), a.max(b));
        let vol = format!("ultar-gemeinsam-{}-{}", a.min(b), a.max(b));
        let vorhanden = self.json("GET", "/networks", None)?.as_array().map(|n| n.iter().any(|x| x["Name"] == netz.as_str())).unwrap_or(false);
        if !vorhanden {
            self.json("POST", "/networks/create", Some(&serde_json::json!({"Name": netz, "Driver": "bridge", "Labels": {"ultar.verknuepft": format!("{a},{b}")}})))?;
        }
        for c in [a, b] {
            let r = self.anfrage("POST", &format!("/networks/{netz}/connect"), Some(&serde_json::json!({"Container": c})))?;
            if !(200..300).contains(&r.0) && !String::from_utf8_lossy(&r.1).contains("already") {
                return Err(format!("{c} ins Netz {netz}: {} {}", r.0, String::from_utf8_lossy(&r.1).trim()));
            }
        }
        self.json("POST", "/volumes/create", Some(&serde_json::json!({"Name": vol, "Labels": {"ultar.verknuepft": format!("{a},{b}")}})))?;
        Ok(format!("{a} und {b} im Netz „{netz}“ (rufen sich beim Namen); gemeinsames Volume „{vol}“ – beim nächsten Anlegen mit -v {vol}:/gemeinsam einbinden"))
    }
}

fn entstuecken(b: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut p = 0;
    while p < b.len() {
        let Some(e) = b[p..].windows(2).position(|w| w == b"\r\n") else { break };
        let n = usize::from_str_radix(String::from_utf8_lossy(&b[p..p + e]).trim(), 16).unwrap_or(0);
        p += e + 2;
        if n == 0 || p + n > b.len() {
            break;
        }
        out.extend_from_slice(&b[p..p + n]);
        p += n + 2;
    }
    out
}

pub fn installiert() -> bool {
    Path::new(ZIEL).join("usr/bin/kiri").exists()
}

/// Kiri einrichten (Version aus dem offiziellen Feed, Paket von GitHub, SHA-256 geprüft)
pub fn einrichten() -> Result<String, String> {
    let arch = if cfg!(target_arch = "aarch64") { "arm64" } else { "amd64" };
    let feed: serde_json::Value = ureq::get("https://download.runkiri.com/latest.json").timeout(Duration::from_secs(20)).call().map_err(|e| format!("Kiri-Version: {e}"))?.into_json().map_err(|e| e.to_string())?;
    let v = feed["version"].as_str().ok_or("keine Version im Feed")?.trim_start_matches('v').to_string();
    let basis = format!("https://github.com/libracoder/kiri/releases/download/v{v}");
    let name = format!("kiri-lite_{v}_{arch}.deb");
    let summen = ureq::get(&format!("{basis}/SHA256SUMS-linux")).timeout(Duration::from_secs(20)).call().map_err(|e| format!("Prüfsummen: {e}"))?.into_string().map_err(|e| e.to_string())?;
    let soll = summen.lines().find(|l| l.ends_with(&name)).and_then(|l| l.split_whitespace().next()).ok_or(format!("{name} nicht in SHA256SUMS-linux"))?.to_lowercase();
    let mut deb = Vec::new();
    ureq::get(&format!("{basis}/{name}")).timeout(Duration::from_secs(600)).call().map_err(|e| format!("{name}: {e}"))?.into_reader().take(256 << 20).read_to_end(&mut deb).map_err(|e| e.to_string())?;
    use sha2::Digest;
    let ist: String = sha2::Sha256::digest(&deb).iter().map(|b| format!("{b:02x}")).collect();
    if ist != soll {
        return Err(format!("Prüfsumme falsch – nicht eingerichtet ({ist} ≠ {soll})"));
    }
    let tmp = std::env::temp_dir().join(&name);
    std::fs::write(&tmp, &deb).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(ZIEL).map_err(|e| e.to_string())?;
    // dpkg-deb, sonst ar + tar
    let ok = std::process::Command::new("dpkg-deb").args(["-x"]).arg(&tmp).arg(ZIEL).status().map(|s| s.success()).unwrap_or(false);
    if !ok {
        deb_entpacken(&tmp, Path::new(ZIEL))?;
    }
    let _ = std::fs::remove_file(&tmp);
    if !installiert() {
        return Err("Paket entpackt, aber kiri fehlt".into());
    }
    let _ = std::fs::write("/etc/ultar/services/kiri.toml", DIENST);
    Ok(format!("Kiri {v} eingerichtet (SHA-256 geprüft) – Dienst: ultar-ctl start kiri, Docker-Werkzeuge: DOCKER_HOST=unix://{SOCKET}"))
}

fn deb_entpacken(deb: &Path, ziel: &Path) -> Result<(), String> {
    let t = std::env::temp_dir().join("ultar-kiri-ar");
    let _ = std::fs::remove_dir_all(&t);
    std::fs::create_dir_all(&t).map_err(|e| e.to_string())?;
    crate::befehl("ar", &["x", &deb.to_string_lossy(), "--output", &t.to_string_lossy()])?;
    let daten = std::fs::read_dir(&t).map_err(|e| e.to_string())?.flatten().map(|e| e.path()).find(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("data.tar"))).ok_or("data.tar fehlt")?;
    crate::befehl("tar", &["-xf", &daten.to_string_lossy(), "-C", &ziel.to_string_lossy()])?;
    Ok(())
}

/// ultar-init-Dienst für Kiri
pub const DIENST: &str = "# Kiri – Container-Laufzeit (Docker-kompatibel), eingerichtet mit `zelle kiri einrichten`
exec = \"/usr/lib/ultar/kiri/usr/bin/kiri\"
args = [\"daemon\"]
restart = \"on-failure\"
manual = true

[env]
KIRI_SOCKET = \"/run/kiri/docker.sock\"
HOME = \"/var/lib/kiri-daemon\"
PATH = \"/usr/lib/ultar/kiri/usr/bin:/usr/bin:/bin:/usr/sbin:/sbin\"
";

#[cfg(test)]
mod tests {
    #[test]
    fn stuecke() {
        assert_eq!(super::entstuecken(b"4\r\nWiki\r\n5\r\npedia\r\n0\r\n\r\n"), b"Wikipedia");
    }
}
