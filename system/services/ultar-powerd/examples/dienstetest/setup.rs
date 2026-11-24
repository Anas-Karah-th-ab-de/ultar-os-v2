//! The work directory of the run: the shipped service files with their paths moved
//! below it, extra bus sockets for the test's own identities, the init configuration.

use std::path::PathBuf;
use std::time::Duration;
use ultar_bus::{Client, ClientOptions};

macro_rules! shipped {
    ($name:literal) => {
        include_str!(concat!(
            "../../../../../rootfs/etc/ultar/services/",
            $name,
            ".toml"
        ))
    };
}

/// The services of the run (shipped definitions).
pub const SERVICES: [(&str, &str); 12] = [
    ("logd", shipped!("logd")),
    ("bus", shipped!("bus")),
    ("authd", shipped!("authd")),
    ("sessiond", shipped!("sessiond")),
    ("powerd", shipped!("powerd")),
    ("notifyd", shipped!("notifyd")),
    ("calendard", shipped!("calendard")),
    ("audiod", shipped!("audiod")),
    ("mediad", shipped!("mediad")),
    ("netd", shipped!("netd")),
    ("btd", shipped!("btd")),
    ("volumed", shipped!("volumed")),
];
const POLICY: &str = include_str!("../../../../../rootfs/etc/ultar/bus-policy.toml");
const IDS: &str = include_str!("../../../../../rootfs/etc/ultar/ids.toml");

/// What only the test may do: create the first account in the credential store.
const TEST_RULES: &str = r#"
[[rule]]
identity = "system.installer"
call = ["auth.set_secret", "auth.admin", "auth.users"]
"#;

/// The test's own identities: a socket each, root only.
pub const TEST_IDENTITIES: [&str; 6] = [
    "system.shell",
    "system.lock",
    "system.sysdialog",
    "system.installer",
    "app.chat",
    "app.calendar",
];

/// Where the run happens.
pub struct Run {
    pub bin: PathBuf,
    pub work: PathBuf,
    /// Kernel attribute files below this root instead of `/` (Linux hosts: never
    /// suspend the machine that runs the test).
    pub fake_root: bool,
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

    fn with_root(&self, text: &str) -> String {
        if !self.fake_root {
            return self.moved(text);
        }
        let root = self.at("root").display().to_string();
        self.moved(&text.replacen(
            "    \"--log-socket\"",
            &format!("    \"--root\", \"{root}\",\n    \"--log-socket\""),
            1,
        ))
    }

    /// Writes everything; returns the init configuration.
    pub fn layout(&self, group_root: Option<&str>) -> std::io::Result<PathBuf> {
        let services = self.at("etc/services");
        std::fs::create_dir_all(&services)?;
        std::fs::create_dir_all(self.at("root"))?;
        for (name, text) in SERVICES {
            let body = match name {
                "bus" => self.bus(text),
                "powerd" | "audiod" | "netd" | "btd" | "volumed" => self.with_root(text),
                _ => self.moved(text),
            };
            std::fs::write(services.join(format!("{name}.toml")), body)?;
        }
        std::fs::write(
            self.at("etc/bus-policy.toml"),
            format!("{POLICY}\n{TEST_RULES}"),
        )?;
        std::fs::write(self.at("etc/ids.toml"), IDS)?;
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
            ..ClientOptions::named("dienstetest")
        };
        Client::connect(&socket, options).map_err(|e| format!("bus {identity}: {e}"))
    }
}
