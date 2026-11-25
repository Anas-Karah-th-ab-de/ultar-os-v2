//! The bus side of the removable-drive service: the drives themselves (mount, eject,
//! pull, usage) and the `Service` loop. The Storage settings it follows live in
//! [`crate::follow`], the drive offers of automount-off in [`crate::offer`].

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use ultar_bus::messages::account_local::LocalUsers;
use ultar_bus::messages::notify::InvokedHint;
use ultar_bus::messages::session::SessionState;
use ultar_bus::messages::settings::Changed;
use ultar_bus::messages::volumes::{
    Eject, EjectOutcome, EjectReply, Mounted, Space, Usage, UsageReply, VolumesState, INTERNAL,
    MAX_USAGE, SERVICE,
};
use ultar_bus::{Call, ErrorCode, IncomingRequest, RawEvent, Topic};
use ultar_hal::HalError;
use ultar_hal_linux::block::{Drives, Volume};
use ultar_service::log::{Level, Record};
use ultar_svckit::{Bus, Service};

use crate::jobs::Jobs;
use crate::offer::Offer;
use crate::settings::StorageSettings;

/// How often the drives are looked at.
pub const TICK: Duration = Duration::from_secs(2);
/// Timeout of a call to another service (settings, notifications).
pub(crate) const CALL_TIME: Duration = Duration::from_secs(2);

/// One mounted drive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Mount {
    pub volume: Volume,
    pub target: PathBuf,
    pub owner: String,
}

/// The removable-drive service.
pub struct VolumeService {
    pub(crate) drives: Drives,
    pub(crate) root: PathBuf,
    pub(crate) media: PathBuf,
    pub(crate) downloads: PathBuf,
    pub(crate) user: Option<String>,
    pub(crate) uids: BTreeMap<String, u32>,
    pub(crate) mounts: BTreeMap<String, Mount>,
    /// Drives that failed to mount (not tried again until they are pulled).
    pub(crate) failed: Vec<String>,
    /// Drives offered with a "Mount" button while automount is off.
    pub(crate) offered: BTreeMap<String, Offer>,
    pub(crate) last: Option<VolumesState>,
    pub(crate) settings: StorageSettings,
    /// The settings service answered (until then the schema defaults are used).
    pub(crate) settings_known: bool,
    pub(crate) jobs: Jobs,
}

/// Size and free bytes (for a person without special rights) of the file system at `path`.
pub fn space_of(path: &Path) -> Option<(u64, u64)> {
    let stat = rustix::fs::statvfs(path).ok()?;
    let block = stat.f_frsize;
    Some((
        stat.f_blocks.saturating_mul(block),
        stat.f_bavail.saturating_mul(block),
    ))
}

/// A label as a folder name: letters, digits, `-` and `_` only, never empty.
pub fn folder(label: &str, device: &str) -> String {
    let clean: String = label
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        .take(32)
        .collect();
    if clean.is_empty() {
        device.to_owned()
    } else {
        clean
    }
}

pub(crate) fn plain(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 32
        && !name.starts_with('-')
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

pub(crate) fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl VolumeService {
    /// Drives below `root`, mounted below `media`, downloads cleaned in `downloads`.
    pub fn new(root: PathBuf, media: PathBuf, downloads: PathBuf) -> Self {
        Self {
            drives: Drives::new(root.clone()),
            root,
            media,
            downloads,
            user: None,
            uids: BTreeMap::new(),
            mounts: BTreeMap::new(),
            failed: Vec::new(),
            offered: BTreeMap::new(),
            last: None,
            settings: StorageSettings::default(),
            settings_known: false,
            jobs: Jobs::default(),
        }
    }

    pub(crate) fn state(&self) -> VolumesState {
        VolumesState {
            mounted: self
                .mounts
                .iter()
                .filter(|(_, m)| Some(&m.owner) == self.user.as_ref())
                .map(|(id, m)| Mounted {
                    id: id.clone(),
                    name: if m.volume.label.is_empty() {
                        id.clone()
                    } else {
                        m.volume.label.clone()
                    },
                    uri: format!("file://{}", m.target.display()),
                })
                .collect(),
        }
    }

    /// Where the person's own files of this device are kept: the data partition when the
    /// system has one, the root file system otherwise.
    pub(crate) fn internal(&self) -> PathBuf {
        let data = self.root.join("data");
        if data.is_dir() {
            data
        } else {
            self.root.clone()
        }
    }

    /// `volumes.usage`: the device's own storage, then the drives of the person in front.
    pub fn usage(&self) -> UsageReply {
        let place = |id: &str, name: &str, path: &Path| {
            space_of(path).map(|(total_bytes, free_bytes)| Space {
                id: id.to_owned(),
                name: name.to_owned(),
                total_bytes,
                free_bytes,
            })
        };
        let mut places: Vec<Space> = place(INTERNAL, "", &self.internal()).into_iter().collect();
        let mine = self
            .mounts
            .iter()
            .filter(|(_, m)| Some(&m.owner) == self.user.as_ref());
        places.extend(mine.filter_map(|(id, m)| {
            let name = if m.volume.label.is_empty() {
                id
            } else {
                &m.volume.label
            };
            place(id, name, &m.target)
        }));
        places.truncate(MAX_USAGE);
        UsageReply { places }
    }

    pub(crate) fn publish(&mut self, bus: &Bus<'_, Self>) {
        let now = self.state();
        if self.last.as_ref() != Some(&now) {
            bus.retain(&now);
            self.last = Some(now);
        }
    }

    /// Mounts one drive for the person in front; `false` (and a note in `failed`) when the
    /// kernel refuses it. Without a person or their id nothing is tried.
    pub(crate) fn mount_one(&mut self, volume: &Volume, bus: &Bus<'_, Self>) -> bool {
        let Some(user) = self.user.clone() else {
            return false;
        };
        let Some(uid) = self.uids.get(&user).copied() else {
            return false;
        };
        let target = self
            .media
            .join(&user)
            .join(folder(&volume.label, &volume.device));
        let made = std::fs::create_dir_all(&target);
        match made
            .map_err(|_| HalError::Io(0))
            .and_then(|()| self.drives.mount(volume, &target, uid))
        {
            Ok(()) => {
                bus.log(Record::new(Level::Info, "volumed.mounted").word("fs", volume.fs.name()));
                let mount = Mount {
                    volume: volume.clone(),
                    target,
                    owner: user,
                };
                self.mounts.insert(volume.device.clone(), mount);
                true
            }
            Err(_) => {
                let _ = std::fs::remove_dir(&target);
                bus.log(
                    Record::new(Level::Warn, "volumed.mount_failed").word("fs", volume.fs.name()),
                );
                self.failed.push(volume.device.clone());
                false
            }
        }
    }

    pub(crate) fn mount_new(&mut self, present: &[Volume], bus: &Bus<'_, Self>) {
        if self.user.is_none() {
            return;
        }
        for volume in present {
            let id = &volume.device;
            if self.mounts.contains_key(id)
                || self.failed.contains(id)
                || self.offered.contains_key(id)
            {
                continue;
            }
            if self.settings.automount {
                self.mount_one(volume, bus);
            } else {
                self.offer(volume, bus);
            }
        }
    }

    pub(crate) fn scan(&mut self, bus: &Bus<'_, Self>) {
        let present = self.drives.volumes();
        // Pulled out: detach what is left of them, withdraw their offers.
        let gone: Vec<String> = self
            .mounts
            .keys()
            .chain(self.offered.keys())
            .filter(|id| !present.iter().any(|v| &&v.device == id))
            .cloned()
            .collect();
        for id in gone {
            if let Some(m) = self.mounts.remove(&id) {
                let _ = rustix::mount::unmount(&m.target, rustix::mount::UnmountFlags::DETACH);
                let _ = std::fs::remove_dir(&m.target);
                bus.log(Record::new(Level::Info, "volumed.pulled"));
            }
            if let Some(o) = self.offered.remove(&id) {
                o.withdraw(bus);
            }
        }
        self.failed
            .retain(|id| present.iter().any(|v| &v.device == id));
        self.mount_new(&present, bus);
        self.publish(bus);
    }

    pub(crate) fn eject(&mut self, id: &str) -> EjectOutcome {
        let Some(m) = self.mounts.get(id) else {
            return EjectOutcome::Unknown;
        };
        if Some(&m.owner) != self.user.as_ref() {
            return EjectOutcome::Unknown;
        }
        match self.drives.unmount(&m.target) {
            Ok(()) => {
                let _ = std::fs::remove_dir(&m.target);
                // Not mounted again until it is pulled and put back.
                self.failed.push(id.to_owned());
                self.mounts.remove(id);
                EjectOutcome::Done
            }
            Err(HalError::Busy) => EjectOutcome::Busy,
            Err(_) => EjectOutcome::Failed,
        }
    }
}

impl Service for VolumeService {
    fn name(&self) -> &'static str {
        SERVICE
    }

    fn subscriptions(&self) -> Vec<&'static str> {
        vec![
            SessionState::TOPIC,
            LocalUsers::TOPIC,
            Changed::TOPIC,
            InvokedHint::TOPIC,
        ]
    }

    fn tick_every(&self) -> Option<Duration> {
        Some(TICK)
    }

    fn start(&mut self, bus: &Bus<'_, Self>) {
        self.read_settings(bus);
        self.scan(bus);
    }

    fn request(&mut self, request: IncomingRequest, bus: &Bus<'_, Self>) {
        if request.method() == Usage::METHOD {
            let _ = match request.decode::<Usage>() {
                Ok(_) => request.reply(&self.usage()),
                Err(_) => request.fail(ErrorCode::BadPayload),
            };
            return;
        }
        if request.method() != Eject::METHOD {
            let _ = request.fail(ErrorCode::UnknownMethod);
            return;
        }
        match request.decode::<Eject>() {
            Ok(e) => {
                let outcome = self.eject(&e.id);
                let _ = request.reply(&EjectReply { outcome });
                self.publish(bus);
            }
            Err(_) => {
                let _ = request.fail(ErrorCode::BadPayload);
            }
        }
    }

    fn event(&mut self, event: RawEvent, bus: &Bus<'_, Self>) {
        if event.topic == Changed::TOPIC {
            if let Ok(changed) = event.decode::<Changed>() {
                self.apply_setting(&changed.key, &changed.value, bus);
            }
            return;
        }
        if event.topic == InvokedHint::TOPIC {
            if let Ok(hint) = event.decode::<InvokedHint>() {
                if hint.sender == bus.client().identity() {
                    self.invoked(bus);
                }
            }
            return;
        }
        if let Ok(state) = event.decode::<SessionState>() {
            self.user = state.user.filter(|u| plain(u));
        } else if let Ok(users) = event.decode::<LocalUsers>() {
            self.uids = users.users.into_iter().map(|u| (u.name, u.uid)).collect();
        }
        self.scan(bus);
    }

    fn tick(&mut self, bus: &Bus<'_, Self>) {
        self.read_settings(bus);
        self.scan(bus);
        self.run_jobs(bus);
    }
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
