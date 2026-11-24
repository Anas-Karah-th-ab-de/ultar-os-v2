//! Network interfaces as the kernel lists them: wired links with their speed, Wi-Fi
//! interfaces, whether a link is up.

use crate::sysfs::{entries, number, text};
use std::path::PathBuf;

/// One interface.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Interface {
    /// Name (`eth0`, `wlan0`).
    pub name: String,
    /// A Wi-Fi interface.
    pub wireless: bool,
    /// The link is up and carries traffic.
    pub up: bool,
    /// Speed in Mbit/s (wired; `None` when the driver does not say).
    pub speed_mbit: Option<u32>,
}

/// The interfaces below a root directory.
#[derive(Clone, Debug)]
pub struct Interfaces {
    root: PathBuf,
}

/// `ARPHRD_ETHER`: Ethernet and Wi-Fi.
const ETHER: i64 = 1;

impl Interfaces {
    /// Interfaces below `root` (`/` on a device).
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Every Ethernet or Wi-Fi interface (loopback, tunnels and bridges left out).
    pub fn list(&self) -> Vec<Interface> {
        entries(&self.root.join("sys/class/net"), 128)
            .into_iter()
            .filter(|p| number(&p.join("type")) == Some(ETHER) && !p.join("bridge").exists())
            .filter_map(|p| {
                let name = p.file_name()?.to_str()?.to_owned();
                let wireless = p.join("wireless").exists() || p.join("phy80211").exists();
                let up = text(&p.join("operstate")).as_deref() == Some("up")
                    && number(&p.join("carrier")) == Some(1);
                let speed_mbit = number(&p.join("speed"))
                    .filter(|s| *s > 0)
                    .and_then(|s| u32::try_from(s).ok());
                Some(Interface {
                    name,
                    wireless,
                    up,
                    speed_mbit,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(root: &std::path::Path, path: &str, value: &str) {
        let p = root.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, value).unwrap();
    }

    #[test]
    fn wired_and_wireless_are_told_apart() {
        let root = std::env::temp_dir().join(format!("hal-net-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (n, t) in [("lo", "772"), ("eth0", "1"), ("wlan0", "1"), ("br0", "1")] {
            put(&root, &format!("sys/class/net/{n}/type"), t);
        }
        put(&root, "sys/class/net/eth0/operstate", "up\n");
        put(&root, "sys/class/net/eth0/carrier", "1\n");
        put(&root, "sys/class/net/eth0/speed", "1000\n");
        put(&root, "sys/class/net/wlan0/operstate", "down\n");
        put(&root, "sys/class/net/wlan0/speed", "-1\n");
        std::fs::create_dir_all(root.join("sys/class/net/wlan0/wireless")).unwrap();
        std::fs::create_dir_all(root.join("sys/class/net/br0/bridge")).unwrap();
        let list = Interfaces::new(&root).list();
        assert_eq!(list.len(), 2);
        assert_eq!(
            list[0],
            Interface {
                name: "eth0".into(),
                wireless: false,
                up: true,
                speed_mbit: Some(1000)
            }
        );
        assert!(list[1].wireless && !list[1].up && list[1].speed_mbit.is_none());
        assert!(Interfaces::new(root.join("none")).list().is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }
}
