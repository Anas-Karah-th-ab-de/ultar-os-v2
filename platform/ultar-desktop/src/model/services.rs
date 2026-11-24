//! What the system services report (docs/integration/system-services.md): the retained topics
//! the shell follows, reduced to what its controls need, and the calls its controls make
//! ([`Call`], run by the app through the ports). A service that never reported stays `None`:
//! its controls are disabled with their reason.

use ultar_bus::messages::audio::AudioState;
use ultar_bus::messages::auth::Secret;
use ultar_bus::messages::bluetooth::BluetoothState;
use ultar_bus::messages::calendar::DayEvent;
use ultar_bus::messages::media::NowPlaying;
use ultar_bus::messages::net::NetState;
use ultar_bus::messages::power::PowerActions;
use ultar_bus::messages::session::SessionState;
use ultar_bus::messages::volumes::Mounted;
use ultar_bus::messages::vpn::VpnState;

/// Most drives shown on the bar.
pub const MAX_DRIVES: usize = 4;
/// Most appointments shown for a day.
pub const MAX_EVENTS: usize = 16;
/// Most Bluetooth devices kept.
pub const MAX_BT_DEVICES: usize = 32;
/// Most audio outputs offered.
pub const MAX_OUTPUTS: usize = 8;

/// A call of the shell to a system service (one click, one call).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    /// `net.set_wifi`.
    SetWifi(bool),
    /// `net.set_airplane`.
    SetAirplane(bool),
    /// `bluetooth.set`.
    SetBluetooth(bool),
    /// `audio.set_volume` (0–100).
    SetVolume(u8),
    /// `audio.set_mute`.
    SetMute(bool),
    /// `audio.outputs` (fills [`Services::outputs`]).
    Outputs,
    /// `audio.set_output`.
    SetOutput(String),
    /// `capture.screenshot` of every output.
    Screenshot,
    /// `power.suspend`.
    Suspend,
    /// `power.restart`.
    Restart,
    /// `power.off`.
    PowerOff,
    /// `session.sign_out`.
    SignOut,
    /// `notify.invoke {id, action}`.
    NoteAction(u64, String),
    /// `notify.dismiss {id}`.
    NoteDismiss(u64),
    /// `calendar.day` (year, month, day) – fills [`Services::day`].
    Day(u16, u8, u8),
    /// `volumes.eject {id}`.
    Eject(String),
    /// `bluetooth.devices` (fills [`Services::bt_devices`]).
    BtDevices,
    /// `bluetooth.scan {on}`.
    BtScan(bool),
    /// `bluetooth.pair {id}`.
    BtPair(String),
    /// `bluetooth.connect {id}` (true) / `bluetooth.disconnect {id}` (false).
    BtConnect(String, bool),
    /// `bluetooth.forget {id}`.
    BtForget(String),
    /// `net.networks` (fills [`Services::networks`]; up to 15 s: run off the loop).
    Networks,
    /// `net.known` (fills [`Services::known`]).
    Known,
    /// `net.join {ssid, secret, remember}` (up to 30 s: run off the loop). The secret is
    /// the typed password (wiped when dropped); `None` for open or saved networks.
    Join(String, Option<Secret>, bool),
    /// `net.disconnect`.
    Disconnect,
    /// `net.forget {ssid}`.
    Forget(String),
    /// `vpn.set {on, name}` (`None`: the last used one).
    SetVpn(bool, Option<String>),
}

impl Call {
    /// The call may take longer than the shell's turn may wait (scan, join, tunnel):
    /// it runs on a worker and its answer comes back later.
    pub fn is_slow(&self) -> bool {
        matches!(self, Call::Networks | Call::Join(..) | Call::SetVpn(..))
    }
}

/// The latest report of each service.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Services {
    /// `power.actions`.
    pub power: Option<PowerActions>,
    /// `session.state`.
    pub session: Option<SessionState>,
    /// `audio.state`.
    pub audio: Option<AudioState>,
    /// `net.state`.
    pub net: Option<NetState>,
    /// `bluetooth.state`.
    pub bluetooth: Option<BluetoothState>,
    /// `volumes.state`: mounted drives (at most [`MAX_DRIVES`]); `None`: no service.
    pub drives: Option<Vec<Mounted>>,
    /// `media.state` (`None`: nothing plays or no service).
    pub media: Option<NowPlaying>,
    /// The appointments of the day the calendar shows: ((year, month, day), events); `None`:
    /// the calendar service did not answer.
    pub day: Option<((u16, u8, u8), Vec<DayEvent>)>,
    /// Bluetooth devices from the last `bluetooth.devices` (at most [`MAX_BT_DEVICES`]).
    pub bt_devices: Vec<ultar_bus::messages::bluetooth::Device>,
    /// A Bluetooth scan ran since the desktop started (`bluetooth.state` said `scanning`).
    pub bt_searched: bool,
    /// Audio outputs (id, name, active) from the last `audio.outputs`.
    pub outputs: Vec<(String, String, bool)>,
    /// Settings id of the main display (`display.<id>.*`), once the settings service knows
    /// the displays.
    pub main_display: Option<String>,
    /// Wi-Fi: `net.wifi`, the networks, the saved ones and the join dialog.
    pub wifi: super::wifi::Wifi,
    /// `vpn.state`.
    pub vpn: Option<VpnState>,
}

impl Services {
    /// Keeps `drives`, at most [`MAX_DRIVES`].
    pub fn set_drives(&mut self, mut drives: Vec<Mounted>) {
        drives.truncate(MAX_DRIVES);
        self.drives = Some(drives);
    }

    /// Keeps the events of a day, at most [`MAX_EVENTS`], by start time.
    pub fn set_day(&mut self, date: (u16, u8, u8), mut events: Vec<DayEvent>) {
        events.sort_by_key(|e| e.start_minute);
        events.truncate(MAX_EVENTS);
        self.day = Some((date, events));
    }

    /// Keeps the Bluetooth devices, at most [`MAX_BT_DEVICES`].
    pub fn set_bt_devices(&mut self, mut devices: Vec<ultar_bus::messages::bluetooth::Device>) {
        devices.truncate(MAX_BT_DEVICES);
        self.bt_devices = devices;
    }

    /// Keeps the outputs, at most [`MAX_OUTPUTS`].
    pub fn set_outputs(&mut self, mut outputs: Vec<(String, String, bool)>) {
        outputs.truncate(MAX_OUTPUTS);
        self.outputs = outputs;
    }

    /// The power menu's entries that work now: (suspend, restart, power off, sign out).
    pub fn power_entries(&self) -> (bool, bool, bool, bool) {
        let p = self.power.as_ref();
        (
            p.is_some_and(|p| p.suspend),
            p.is_some_and(|p| p.restart),
            p.is_some_and(|p| p.power_off),
            self.session.is_some(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(id: u64, start: u16) -> DayEvent {
        DayEvent {
            id,
            start_minute: start,
            duration_minutes: 30,
            title: format!("e{id}"),
        }
    }

    #[test]
    fn reports_are_bounded_and_ordered() {
        let mut s = Services::default();
        assert_eq!(s.power_entries(), (false, false, false, false));
        s.set_day(
            (2026, 11, 24),
            (0..20).map(|i| event(i, 1000 - i as u16)).collect(),
        );
        let (_, events) = s.day.as_ref().unwrap();
        assert_eq!(events.len(), MAX_EVENTS);
        assert!(events
            .windows(2)
            .all(|w| w[0].start_minute <= w[1].start_minute));
        let drive = |i: u32| Mounted {
            id: i.to_string(),
            name: "USB".into(),
            uri: format!("file:///media/{i}"),
        };
        s.set_drives((0..9).map(drive).collect());
        assert_eq!(s.drives.as_ref().map(Vec::len), Some(MAX_DRIVES));
        s.power = Some(PowerActions {
            suspend: false,
            restart: true,
            power_off: true,
            needs_admin: false,
        });
        s.session = Some(SessionState {
            user: Some("anas".into()),
            signed_in: vec!["anas".into()],
        });
        assert_eq!(s.power_entries(), (false, true, true, true));
        let net = |i: u8| ultar_bus::messages::net::NetworkInfo {
            ssid: format!("n{i}"),
            bssid: String::new(),
            signal_percent: i,
            frequency_mhz: 2412,
            security: ultar_bus::messages::net::WifiSecurity::Open,
            known: false,
            joinable: true,
            connected: false,
        };
        s.wifi.set_networks((0..100).map(net).collect());
        assert_eq!(s.wifi.networks.as_ref().map(Vec::len), Some(48));
        s.wifi.set_known((0..100).map(|i| i.to_string()).collect());
        assert_eq!(s.wifi.known.len(), crate::model::wifi::MAX_KNOWN);
        assert!(Call::Join("x".into(), None, true).is_slow() && Call::Networks.is_slow());
        assert!(!Call::Known.is_slow() && !Call::Disconnect.is_slow());
    }
}
