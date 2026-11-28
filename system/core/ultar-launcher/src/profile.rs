//! Profiles (PLAN.md §3.7): what an app may get at most, decided by who
//! signed it – never by the app.
//!
//! | | `app.standard` (store) | `app.system` (KM Code Craft) |
//! |---|---|---|
//! | grantable | `network`, `background` | all app permissions (never the system UI ones, S-7) |
//! | granted at installation | nothing (the user decides) | everything it declares |
//! | memory default / most | 256 / 1024 MiB | 256 / 4096 MiB |
//! | processes default / most | 64 / 256 | 128 / 1024 |
//! | private `/tmp` | 64 MiB | 128 MiB |
//! | kernel rights | none | none |
//!
//! A manifest that declares more than its profile may grant, or asks for
//! more than the profile's most, is refused at installation.

use crate::manifest::{Manifest, Requested};
use crate::perm::{Permission, PermissionSet};

/// Which profile an app runs with.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProfileName {
    /// Apps of other publishers.
    Standard,
    /// System apps of KM Code Craft.
    System,
    /// Programs of other systems (Linux, later Windows and Android).
    Foreign,
}

impl ProfileName {
    /// Name in the state file and on the bus.
    pub const fn word(self) -> &'static str {
        match self {
            Self::Standard => "app.standard",
            Self::System => "app.system",
            Self::Foreign => "app.foreign",
        }
    }

    /// The profile called `word`.
    pub fn from_word(word: &str) -> Option<Self> {
        [Self::Standard, Self::System, Self::Foreign]
            .into_iter()
            .find(|p| p.word() == word)
    }

    /// The profile's rules.
    pub const fn profile(self) -> Profile {
        match self {
            Self::Standard => STANDARD,
            Self::System => SYSTEM,
            Self::Foreign => FOREIGN,
        }
    }
}

/// The rules of a profile.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Profile {
    /// Its name.
    pub name: ProfileName,
    /// Permissions an app of the profile may be granted.
    pub grantable: PermissionSet,
    /// Whether what the app declares is granted at installation.
    pub granted_at_install: bool,
    /// Memory when the manifest asks for none, in MiB.
    pub memory_default_mb: u32,
    /// Most memory, in MiB.
    pub memory_max_mb: u32,
    /// Processes and threads when the manifest asks for none.
    pub pids_default: u32,
    /// Most processes and threads.
    pub pids_max: u32,
    /// Size of the private `/tmp` in MiB.
    pub tmp_mb: u32,
    /// Kernel rights an app may keep (bit mask).
    pub kernel_rights: u64,
}

const STANDARD: Profile = Profile {
    name: ProfileName::Standard,
    grantable: PermissionSet::EMPTY
        .with(Permission::Network)
        .with(Permission::Background),
    granted_at_install: false,
    memory_default_mb: 256,
    memory_max_mb: 1024,
    pids_default: 64,
    pids_max: 256,
    tmp_mb: 64,
    kernel_rights: 0,
};

const SYSTEM: Profile = Profile {
    name: ProfileName::System,
    // the shell, keyboard, lock screen and system dialogs are services with
    // fixed rights, never apps (review S-7)
    grantable: PermissionSet::for_apps(),
    granted_at_install: true,
    memory_default_mb: 256,
    memory_max_mb: 4096,
    pids_default: 128,
    pids_max: 1024,
    tmp_mb: 128,
    // Apps of the system may play sound themselves (the browser's videos, the phone): the
    // sound card's streams, nothing else. A sound service that mixes several apps and keeps
    // recording apart will take this over.
    kernel_rights: crate::perm::KERNEL_SOUND,
};

const FOREIGN: Profile = Profile {
    name: ProfileName::Foreign,
    // ui.client-header: programs that draw their own title bar (GTK) appear without a
    // second one from the compositor; system dialogs carry the compositor's own mark,
    // which an app's title bar cannot imitate (docs/integration/ultar-foreign.md)
    grantable: PermissionSet::EMPTY
        .with(Permission::Network)
        .with(Permission::UiClientHeader),
    // the service for such apps installs only after the person confirmed with an
    // administrator's password what the app gets (`foreign.install`)
    granted_at_install: true,
    memory_default_mb: 1024,
    memory_max_mb: 4096,
    pids_default: 256,
    pids_max: 1024,
    tmp_mb: 256,
    kernel_rights: 0,
};

/// The limits an app's cell gets.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Limits {
    /// `memory.max` in bytes.
    pub memory_bytes: u64,
    /// `pids.max` (also the per-user process limit on Linux).
    pub pids: u32,
    /// Size of the private `/tmp` in bytes.
    pub tmp_bytes: u64,
}

/// Why a manifest does not fit its profile.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProfileError {
    /// It declares a permission the profile cannot grant.
    Permission(Permission),
    /// It asks for kernel rights the profile does not allow.
    KernelRights,
    /// It asks for more memory or processes than the profile allows.
    Limit,
}

impl Profile {
    /// Checks `manifest` against the profile.
    pub fn admit(&self, manifest: &Manifest) -> Result<(), ProfileError> {
        if let Some(p) = manifest.declared.iter().find(|p| !self.grantable.has(*p)) {
            return Err(ProfileError::Permission(p));
        }
        if manifest.kernel_rights & !self.kernel_rights != 0 {
            return Err(ProfileError::KernelRights);
        }
        self.limits(&manifest.limits).map(|_| ())
    }

    /// The limits for `requested`, filled with the profile's defaults.
    pub fn limits(&self, requested: &Requested) -> Result<Limits, ProfileError> {
        let memory = requested.memory_mb.unwrap_or(self.memory_default_mb);
        let pids = requested.pids.unwrap_or(self.pids_default);
        if memory > self.memory_max_mb || pids > self.pids_max {
            return Err(ProfileError::Limit);
        }
        Ok(Limits {
            memory_bytes: u64::from(memory) << 20,
            pids,
            tmp_bytes: u64::from(self.tmp_mb) << 20,
        })
    }

    /// What is granted at installation of `manifest`.
    pub fn initial_grants(&self, manifest: &Manifest) -> PermissionSet {
        if self.granted_at_install {
            manifest.declared.and(self.grantable)
        } else {
            PermissionSet::EMPTY
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(extra: &str) -> Manifest {
        let text =
            format!("id = \"a\"\nversion = \"1\"\nkind = \"native\"\nexec = \"/bin/a\"\n{extra}");
        Manifest::parse(text.as_bytes()).unwrap()
    }

    #[test]
    fn standard_apps_get_nothing_without_the_user() {
        let p = ProfileName::Standard.profile();
        let m = manifest("[permissions]\nnetwork = \"allowed\"\nbackground = true\n");
        assert_eq!(p.admit(&m), Ok(()));
        assert!(p.initial_grants(&m).is_empty());
        let ui = manifest("[permissions]\nui = [\"ui.overlay\"]\n");
        assert_eq!(
            p.admit(&ui),
            Err(ProfileError::Permission(Permission::UiOverlay))
        );
        let kernel = manifest("[permissions]\nkernel = [\"raw_device\"]\n");
        assert_eq!(p.admit(&kernel), Err(ProfileError::KernelRights));
        assert_eq!(
            ProfileName::System.profile().admit(&kernel),
            Err(ProfileError::KernelRights)
        );
    }

    #[test]
    fn system_apps_get_what_they_declare() {
        let p = ProfileName::System.profile();
        let m =
            manifest("[permissions]\nnetwork = \"ask\"\nui = [\"ui.overlay\", \"ui.capture\"]\n");
        assert_eq!(p.admit(&m), Ok(()));
        assert_eq!(
            p.initial_grants(&m).names(),
            "network,ui.capture,ui.overlay"
        );
    }

    #[test]
    fn not_even_a_system_app_gets_the_system_ui() {
        let p = ProfileName::System.profile();
        for (name, permission) in [
            ("ui.shell", Permission::UiShell),
            ("ui.input-method", Permission::UiInputMethod),
            ("ui.lock-screen", Permission::UiLockScreen),
            ("ui.system-dialog", Permission::UiSystemDialog),
        ] {
            let m = manifest(&format!("[permissions]\nui = [\"{name}\"]\n"));
            assert_eq!(p.admit(&m), Err(ProfileError::Permission(permission)));
            assert!(!p.grantable.has(permission));
        }
    }

    #[test]
    fn foreign_apps_get_the_network_and_their_own_title_bar_at_most() {
        let p = ProfileName::Foreign.profile();
        let m = manifest("[permissions]\nnetwork = \"allowed\"\nui = [\"ui.client-header\"]\n");
        assert_eq!(p.admit(&m), Ok(()));
        assert_eq!(p.initial_grants(&m).names(), "network,ui.client-header");
        for extra in [
            "[permissions]\nbackground = true\n",
            "[permissions]\nui = [\"ui.overlay\"]\n",
            "[permissions]\nkernel = [\"sound\"]\n",
            "[limits]\nmemory_mb = 8192\n",
        ] {
            assert!(p.admit(&manifest(extra)).is_err(), "{extra}");
        }
    }

    #[test]
    fn limits_are_filled_and_capped() {
        let p = ProfileName::Standard.profile();
        let l = p.limits(&Requested::default()).unwrap();
        assert_eq!(
            (l.memory_bytes, l.pids, l.tmp_bytes),
            (256 << 20, 64, 64 << 20)
        );
        let asked = Requested {
            memory_mb: Some(120),
            pids: Some(10),
        };
        assert_eq!(p.limits(&asked).unwrap().memory_bytes, 120 << 20);
        let greedy = manifest("[limits]\nmemory_mb = 2048\n");
        assert_eq!(p.admit(&greedy), Err(ProfileError::Limit));
        assert_eq!(ProfileName::System.profile().admit(&greedy), Ok(()));
        let forks = Requested {
            memory_mb: None,
            pids: Some(257),
        };
        assert_eq!(p.limits(&forks), Err(ProfileError::Limit));
    }

    #[test]
    fn names_round_trip() {
        for name in [
            ProfileName::Standard,
            ProfileName::System,
            ProfileName::Foreign,
        ] {
            assert_eq!(ProfileName::from_word(name.word()), Some(name));
            assert_eq!(name.profile().name, name);
        }
        assert_eq!(ProfileName::from_word("app.root"), None);
    }
}
