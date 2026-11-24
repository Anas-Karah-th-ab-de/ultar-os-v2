//! The work directory of the run: the shipped definitions of logd, bus, settingsd and the
//! volume service with their paths moved below it, a socket for the Settings app's identity, a laptop-with-modem device
//! profile for settingsd, settingsd's catalogs and the init configuration.

use std::path::PathBuf;
use std::time::Duration;
use ultar_bus::{Client, ClientOptions};

/// The services of the run: name, shipped definition.
pub const SERVICES: [(&str, &str); 4] = [
    (
        "logd",
        include_str!("../../../../rootfs/etc/ultar/services/logd.toml"),
    ),
    (
        "bus",
        include_str!("../../../../rootfs/etc/ultar/services/bus.toml"),
    ),
    (
        "settingsd",
        include_str!("../../../../rootfs/etc/ultar/services/settingsd.toml"),
    ),
    (
        "volumed",
        include_str!("../../../../rootfs/etc/ultar/services/volumed.toml"),
    ),
];
const POLICY: &str = include_str!("../../../../rootfs/etc/ultar/bus-policy.toml");
const IDS: &str = include_str!("../../../../rootfs/etc/ultar/ids.toml");
/// settingsd's catalogs, installed where the app reads service catalogs.
pub const CATALOGS: [(&str, &str); 3] = [
    (
        "ar",
        include_str!("../../../../system/core/ultar-settingsd/locales/ar.ftl"),
    ),
    (
        "en",
        include_str!("../../../../system/core/ultar-settingsd/locales/en.ftl"),
    ),
    (
        "de",
        include_str!("../../../../system/core/ultar-settingsd/locales/de.ftl"),
    ),
];
/// Every panel of the app has its hardware: a laptop with touch screen, Wi-Fi, Bluetooth and a
/// modem.
const PROFILE: &str = "device_id = \"settingstest | ultar\"\nform_factor = \"laptop\"\n\
edition = \"desktop\"\n\n[capabilities]\nflags = [\"keyboard\", \"pointer\", \"touch\", \
\"battery\", \"modem\", \"wifi\", \"bluetooth\", \"audio_out\", \"audio_in\"]\n\
display_count = 1\nmemory_mb = 8192\n";

/// The identity of the Settings app: one socket, root only.
pub const APP: &str = "app.settings";

/// Where the run happens.
pub struct Run {
    pub bin: PathBuf,
    pub work: PathBuf,
}

impl Run {
    pub fn at(&self, path: &str) -> PathBuf {
        self.work.join(path)
    }

    fn moved(&self, text: &str) -> String {
        let work = self.work.display().to_string();
        text.replace("/run/ultar/", &format!("{work}/run/"))
            .replace("/data/ultar/", &format!("{work}/data/"))
            .replace("/etc/ultar/", &format!("{work}/etc/"))
            .replace("/usr/bin/", &format!("{}/", self.bin.display()))
    }

    fn bus(&self, text: &str) -> String {
        let extra = format!(
            "    \"--listen\", \"/run/ultar/bus/{APP}.sock={APP}\", \"--mode\", \"0600\", \"--max-connections\", \"4\",\n"
        );
        let anchor = "    \"--policy\"";
        self.moved(&text.replacen(anchor, &format!("{extra}{anchor}"), 1))
    }

    fn settingsd(&self, text: &str) -> String {
        let anchor = "    \"--policy\"";
        let profile = "    \"--profile\", \"/etc/ultar/profile.toml\",\n";
        self.moved(&text.replacen(anchor, &format!("{profile}{anchor}"), 1))
    }

    /// Writes everything; returns the init configuration.
    pub fn layout(&self, group_root: Option<&str>) -> std::io::Result<PathBuf> {
        let services = self.at("etc/services");
        std::fs::create_dir_all(&services)?;
        for (name, text) in SERVICES {
            let body = match name {
                "bus" => self.bus(text),
                "settingsd" => self.settingsd(text),
                _ => self.moved(text),
            };
            std::fs::write(services.join(format!("{name}.toml")), body)?;
        }
        std::fs::write(self.at("etc/bus-policy.toml"), POLICY)?;
        std::fs::write(self.at("etc/ids.toml"), IDS)?;
        std::fs::write(self.at("etc/profile.toml"), PROFILE)?;
        let catalogs = self.at("catalogs/settingsd");
        std::fs::create_dir_all(&catalogs)?;
        for (lang, text) in CATALOGS {
            std::fs::write(catalogs.join(format!("{lang}.ftl")), text)?;
        }
        let group_root = group_root
            .map(|g| format!("group_root = \"{g}\"\n"))
            .unwrap_or_default();
        let text = format!(
            "shutdown_grace_ms = 5000\nservice_dirs = [\"{}\"]\noutput_socket = \"{}\"\n\
             bus_socket = \"{}\"\nbus_policy = \"{}\"\ncontrol_socket = \"{}\"\n\
             launcher = \"{}\"\n{group_root}",
            services.display(),
            self.at("run/log/init.sock").display(),
            self.at("run/bus/init.sock").display(),
            self.at("etc/bus-policy.toml").display(),
            self.at("ctl.sock").display(),
            self.bin.join("ultar-init").display(),
        );
        let config = self.at("etc/init.toml");
        std::fs::write(&config, text)?;
        Ok(config)
    }

    /// A bus client on the socket of `identity` (`system.admin`: the administrator's socket).
    pub fn client(&self, identity: &str) -> Result<Client, String> {
        if identity == "system.admin" {
            self.client_at("system")
        } else {
            self.client_at(identity)
        }
    }

    /// A bus client on the broker's socket `name` (`displayd`: the display service's).
    pub fn client_at(&self, name: &str) -> Result<Client, String> {
        let identity = name;
        let socket = self.at(&format!("run/bus/{name}.sock"));
        let options = ClientOptions {
            connect_retry_for: Duration::from_secs(120),
            ..ClientOptions::named("settingstest")
        };
        Client::connect(&socket, options).map_err(|e| format!("bus {identity}: {e}"))
    }
}
