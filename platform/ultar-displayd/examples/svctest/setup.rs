//! The work directory of the run: the shipped service files with their
//! paths moved below it, a headless compositor, extra bus sockets for the
//! test's own identities, and the init configuration.

use std::path::PathBuf;
use std::time::Duration;
use ultar_bus::{Client, ClientOptions};

/// The services of the run: name, definition (None: written here), uid.
pub const SERVICES: [(&str, Option<&str>, u32); 6] = [
    (
        "logd",
        Some(include_str!(
            "../../../../rootfs/etc/ultar/services/logd.toml"
        )),
        802,
    ),
    (
        "bus",
        Some(include_str!(
            "../../../../rootfs/etc/ultar/services/bus.toml"
        )),
        801,
    ),
    (
        "settingsd",
        Some(include_str!(
            "../../../../rootfs/etc/ultar/services/settingsd.toml"
        )),
        803,
    ),
    ("compositor", None, 811),
    (
        "displayd",
        Some(include_str!(
            "../../../../rootfs/etc/ultar/services/displayd.toml"
        )),
        817,
    ),
    (
        "accountd",
        Some(include_str!(
            "../../../../rootfs/etc/ultar/services/accountd.toml"
        )),
        818,
    ),
];
const POLICY: &str = include_str!("../../../../rootfs/etc/ultar/bus-policy.toml");
const IDS: &str = include_str!("../../../../rootfs/etc/ultar/ids.toml");
const RIGHTS: &str = include_str!("../../../../rootfs/etc/ultar/compositor-rights.toml");

/// Rules for the stand-ins of the authentication service and the dialog.
const TEST_RULES: &str = r#"
[[rule]]
identity = "system.authd"
register = ["auth"]

[[rule]]
identity = "system.sysdialog"
register = ["sysdialog"]
"#;

/// The test's own identities: a socket each, root only.
pub const TEST_IDENTITIES: [&str; 4] = [
    "system.authd",
    "system.sysdialog",
    "app.settings",
    "system.installer",
];

/// Where the run happens.
pub struct Run {
    pub bin: PathBuf,
    pub work: PathBuf,
    /// The kernel enforces rights and accountd keeps `file_admin` (the Ultar
    /// kernel); on Linux a service without root has no capabilities.
    pub file_admin: bool,
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
            .replace("\"/home\"", &format!("\"{work}/home\""))
            .replace("/usr/bin/", &format!("{}/", self.bin.display()))
    }

    /// The bus with one more socket per test identity.
    fn bus(&self, text: &str) -> String {
        let extra: String = TEST_IDENTITIES
            .iter()
            .map(|id| {
                format!(
                    "    \"--listen\", \"/run/ultar/bus/{id}.sock={id}\", \"--mode\", \"0600\", \"--max-connections\", \"4\",\n"
                )
            })
            .collect();
        let anchor = "    \"--policy\"";
        self.moved(&text.replacen(anchor, &format!("{extra}{anchor}"), 1))
    }

    fn compositor(&self) -> String {
        format!(
            "exec = \"{bin}/ultar-compositor\"\nargs = [\"--socket\", \"{w}/run/ui/compositor.sock\", \"--rights\", \"{w}/etc/compositor-rights.toml\", \"--headless\", \"1280x800\", \"--headless\", \"1024x768\", \"--lang\", \"en\", \"--no-cursor\"]\nuser = 811\npublic_dirs = [\"{w}/run/ui\"]\nnotify = true\nrestart = \"on-failure\"\n\n[cgroup]\n\"memory.max\" = \"128M\"\n\"pids.max\" = \"16\"\n",
            bin = self.bin.display(),
            w = self.work.display()
        )
    }

    /// Writes everything; returns the init configuration.
    pub fn layout(&self, group_root: Option<&str>) -> std::io::Result<PathBuf> {
        let services = self.at("etc/services");
        std::fs::create_dir_all(&services)?;
        std::fs::create_dir_all(self.at("home"))?;
        for (name, text, _) in SERVICES {
            let body = match (name, text) {
                ("bus", Some(t)) => self.bus(t),
                (_, Some(t)) => self.moved(t),
                (_, None) => self.compositor(),
            };
            std::fs::write(services.join(format!("{name}.toml")), body)?;
        }
        std::fs::write(
            self.at("etc/bus-policy.toml"),
            format!("{POLICY}\n{TEST_RULES}"),
        )?;
        std::fs::write(self.at("etc/ids.toml"), IDS)?;
        std::fs::write(self.at("etc/compositor-rights.toml"), RIGHTS)?;
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

    /// A bus client on the socket of `identity`.
    pub fn client(&self, identity: &str) -> Result<Client, String> {
        let socket = if identity == "system.admin" {
            self.at("run/bus/system.sock")
        } else {
            self.at(&format!("run/bus/{identity}.sock"))
        };
        let options = ClientOptions {
            connect_retry_for: Duration::from_secs(120),
            ..ClientOptions::named("svctest")
        };
        Client::connect(&socket, options).map_err(|e| format!("bus {identity}: {e}"))
    }
}
