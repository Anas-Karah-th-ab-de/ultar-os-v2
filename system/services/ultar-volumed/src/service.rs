//! The bus side of the removable-drive service.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use ultar_bus::messages::account_local::LocalUsers;
use ultar_bus::messages::session::SessionState;
use ultar_bus::messages::volumes::{
    Eject, EjectOutcome, EjectReply, Mounted, Space, Usage, UsageReply, VolumesState, INTERNAL,
    MAX_USAGE, SERVICE,
};
use ultar_bus::{Call, ErrorCode, IncomingRequest, RawEvent, Topic};
use ultar_hal::HalError;
use ultar_hal_linux::block::{Drives, Volume};
use ultar_service::log::{Level, Record};
use ultar_svckit::{Bus, Service};

/// How often the drives are looked at.
pub const TICK: Duration = Duration::from_secs(2);

/// One mounted drive.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Mount {
    volume: Volume,
    target: PathBuf,
    owner: String,
}

/// The removable-drive service.
pub struct VolumeService {
    drives: Drives,
    root: PathBuf,
    media: PathBuf,
    user: Option<String>,
    uids: BTreeMap<String, u32>,
    mounts: BTreeMap<String, Mount>,
    /// Drives that failed to mount (not tried again until they are pulled).
    failed: Vec<String>,
    last: Option<VolumesState>,
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

fn plain(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 32
        && !name.starts_with('-')
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

impl VolumeService {
    /// Drives below `root`, mounted below `media`.
    pub fn new(root: PathBuf, media: PathBuf) -> Self {
        Self {
            drives: Drives::new(root.clone()),
            root,
            media,
            user: None,
            uids: BTreeMap::new(),
            mounts: BTreeMap::new(),
            failed: Vec::new(),
            last: None,
        }
    }

    fn state(&self) -> VolumesState {
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
    fn internal(&self) -> PathBuf {
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

    fn publish(&mut self, bus: &Bus<'_, Self>) {
        let now = self.state();
        if self.last.as_ref() != Some(&now) {
            bus.retain(&now);
            self.last = Some(now);
        }
    }

    fn mount_new(&mut self, present: &[Volume], bus: &Bus<'_, Self>) {
        let Some(user) = self.user.clone() else {
            return;
        };
        let Some(uid) = self.uids.get(&user).copied() else {
            return;
        };
        for volume in present {
            if self.mounts.contains_key(&volume.device) || self.failed.contains(&volume.device) {
                continue;
            }
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
                    bus.log(
                        Record::new(Level::Info, "volumed.mounted").word("fs", volume.fs.name()),
                    );
                    let mount = Mount {
                        volume: volume.clone(),
                        target,
                        owner: user.clone(),
                    };
                    self.mounts.insert(volume.device.clone(), mount);
                }
                Err(_) => {
                    let _ = std::fs::remove_dir(&target);
                    bus.log(
                        Record::new(Level::Warn, "volumed.mount_failed")
                            .word("fs", volume.fs.name()),
                    );
                    self.failed.push(volume.device.clone());
                }
            }
        }
    }

    fn scan(&mut self, bus: &Bus<'_, Self>) {
        let present = self.drives.volumes();
        // Pulled out: detach what is left of them.
        let gone: Vec<String> = self
            .mounts
            .keys()
            .filter(|id| !present.iter().any(|v| &&v.device == id))
            .cloned()
            .collect();
        for id in gone {
            if let Some(m) = self.mounts.remove(&id) {
                let _ = rustix::mount::unmount(&m.target, rustix::mount::UnmountFlags::DETACH);
                let _ = std::fs::remove_dir(&m.target);
                bus.log(Record::new(Level::Info, "volumed.pulled"));
            }
        }
        self.failed
            .retain(|id| present.iter().any(|v| &v.device == id));
        self.mount_new(&present, bus);
        self.publish(bus);
    }

    fn eject(&mut self, id: &str) -> EjectOutcome {
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
        vec![SessionState::TOPIC, LocalUsers::TOPIC]
    }

    fn tick_every(&self) -> Option<Duration> {
        Some(TICK)
    }

    fn start(&mut self, bus: &Bus<'_, Self>) {
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
        if let Ok(state) = event.decode::<SessionState>() {
            self.user = state.user.filter(|u| plain(u));
        } else if let Ok(users) = event.decode::<LocalUsers>() {
            self.uids = users.users.into_iter().map(|u| (u.name, u.uid)).collect();
        }
        self.scan(bus);
    }

    fn tick(&mut self, bus: &Bus<'_, Self>) {
        self.scan(bus);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_become_safe_folder_names() {
        assert_eq!(folder("STICK", "sdb1"), "STICK");
        assert_eq!(folder("../../etc", "sdb1"), "etc");
        assert_eq!(folder("", "sdb1"), "sdb1");
        assert_eq!(folder("/ /", "sdc"), "sdc");
        assert_eq!(folder("صور", "sdb1"), "صور");
        assert!(plain("amal") && !plain("../x"));
    }

    #[test]
    fn eject_of_an_unknown_drive() {
        let mut s = VolumeService::new("/nonexistent".into(), "/nonexistent".into());
        assert_eq!(s.eject("sdb1"), EjectOutcome::Unknown);
        assert_eq!(s.state(), VolumesState::default());
    }

    #[test]
    fn usage_measures_the_own_storage_first() {
        let dir = std::env::temp_dir();
        let s = VolumeService::new(dir.clone(), "/nonexistent".into());
        let usage = s.usage();
        assert_eq!(usage.places.len(), 1, "no drives mounted");
        let own = &usage.places[0];
        assert_eq!((own.id.as_str(), own.name.as_str()), (INTERNAL, ""));
        assert!(own.total_bytes > 0 && own.free_bytes <= own.total_bytes);
        assert_eq!(
            space_of(&dir).map(|(total, _)| total),
            Some(own.total_bytes)
        );
        // A place that cannot be measured is left out.
        let gone = VolumeService::new("/nonexistent/ultar".into(), "/nonexistent".into());
        assert!(gone.usage().places.is_empty());
    }
}
