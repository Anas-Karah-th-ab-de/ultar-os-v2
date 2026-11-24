//! The local accounts: who may change what, the checks every change must
//! pass, and the change itself (credential store, home folder, account
//! file – each step undone when a later one fails).
//!
//! Rules (DESKTOP-SPEC §8 "Benutzer"):
//! * the signed-in user may change their own display name and picture, and
//!   their own password with the current one;
//! * everything else – another user, administrator or standard, automatic
//!   sign-in, creating, removing, a password without the current one –
//!   needs an administrator's password in the system dialog;
//! * there is always at least one administrator; the signed-in user cannot
//!   be removed; system names and ids are never given to a person.

use crate::names::{valid_display, valid_name, SystemIds};
use crate::ports::{Check, CredError, Credentials, Homes};
use crate::refusal::{refuse, refuse_wait, Key};
use crate::store::{self, StoreError, Stored, UserRecord};
use crate::throttle::Failures;
use std::path::PathBuf;
use ultar_bus::messages::account_local::{
    LocalUser, LocalUsers, Picture, PictureInfo, RemoveData, UserKind, BUILTIN_PICTURES,
    MAX_LOCAL_USERS, MAX_PICTURE_BYTES,
};
use ultar_bus::messages::account_service::Refusal;
use ultar_bus::messages::auth::{Secret, MAX_SECRET};

/// Where and with which ids.
#[derive(Clone, Debug)]
pub struct Config {
    /// The service's own directory (account file, pictures).
    pub dir: PathBuf,
    /// First and last id given to people.
    pub uids: (u32, u32),
    /// Names and ids of the system.
    pub system: SystemIds,
}

/// A change of local users.
#[derive(Clone, Debug)]
pub enum Op {
    /// A new user.
    Create(ultar_bus::messages::account_local::CreateUser),
    /// Display name.
    Rename(ultar_bus::messages::account_local::RenameUser),
    /// Picture.
    Picture(ultar_bus::messages::account_local::SetPicture),
    /// Password.
    Password(ultar_bus::messages::account_local::SetPassword),
    /// Administrator or standard.
    Kind(ultar_bus::messages::account_local::SetKind),
    /// Automatic sign-in.
    AutoLogin(ultar_bus::messages::account_local::SetAutoLogin),
    /// Removal.
    Remove(ultar_bus::messages::account_local::RemoveUser),
}

impl Op {
    /// The account the change is about (`None`: the device's setting).
    pub fn target(&self) -> Option<&str> {
        match self {
            Op::Create(c) => Some(&c.name),
            Op::Rename(c) => Some(&c.user),
            Op::Picture(c) => Some(&c.user),
            Op::Password(c) => Some(&c.user),
            Op::Kind(c) => Some(&c.user),
            Op::AutoLogin(c) => c.user.as_deref(),
            Op::Remove(c) => Some(&c.user),
        }
    }

    /// Word of the change (i18n key part, log word).
    pub fn word(&self) -> &'static str {
        match self {
            Op::Create(_) => "create",
            Op::Rename(_) => "rename",
            Op::Picture(_) => "picture",
            Op::Password(_) => "password",
            Op::Kind(_) => "kind",
            Op::AutoLogin(_) => "auto_login",
            Op::Remove(_) => "remove",
        }
    }
}

/// The ports one change uses.
pub struct Ports<'a> {
    /// Credential store.
    pub credentials: &'a dyn Credentials,
    /// Home folders.
    pub homes: &'a dyn Homes,
}

/// The local accounts.
#[derive(Debug)]
pub struct Accounts {
    config: Config,
    stored: Result<Stored, StoreError>,
    session: Option<String>,
    failures: Failures,
    warnings: Vec<&'static str>,
}

fn same_secret(a: &Secret, b: &Secret) -> bool {
    ultar_service::secret::constant_time_eq(a.expose().as_bytes(), b.expose().as_bytes())
}

fn check_new_password(new: &Secret, confirm: &Secret) -> Result<(), Refusal> {
    if new.is_empty() || new.len() > MAX_SECRET {
        return Err(refuse(Key::PasswordRejected));
    }
    if !same_secret(new, confirm) {
        return Err(refuse(Key::PasswordsDiffer));
    }
    Ok(())
}

fn check_picture(p: &Picture) -> Result<(), Refusal> {
    let ok = match p {
        Picture::Initials => true,
        Picture::Builtin(n) => *n < BUILTIN_PICTURES,
        Picture::Image(bytes) => bytes.len() <= MAX_PICTURE_BYTES && crate::picture::valid(bytes),
    };
    ok.then_some(()).ok_or_else(|| refuse(Key::PictureInvalid))
}

fn cred(e: CredError) -> Refusal {
    match e {
        CredError::Rejected => refuse(Key::PasswordRejected),
        CredError::Unavailable => refuse(Key::CredentialsUnavailable),
    }
}

impl Accounts {
    /// Loads the account file (a damaged file makes every change refused).
    pub fn open(config: Config) -> Accounts {
        let stored = store::load(&config.dir);
        Accounts {
            config,
            stored,
            session: None,
            failures: Failures::default(),
            warnings: Vec::new(),
        }
    }

    /// The service's directory.
    pub fn dir(&self) -> &std::path::Path {
        &self.config.dir
    }

    /// The account file could be read.
    pub fn usable(&self) -> bool {
        self.stored.is_ok()
    }

    fn stored(&self) -> Result<&Stored, Refusal> {
        self.stored.as_ref().map_err(|_| refuse(Key::StoreDamaged))
    }

    fn user(&self, name: &str) -> Result<&UserRecord, Refusal> {
        self.stored()?
            .users
            .iter()
            .find(|u| u.name == name)
            .ok_or_else(|| refuse(Key::UnknownUser))
    }

    /// The signed-in user.
    pub fn session(&self) -> Option<&str> {
        self.session.as_deref()
    }

    /// There is no account at all (first start: the first one is created
    /// without a dialog by the set-up identity).
    pub fn empty(&self) -> bool {
        self.stored.as_ref().is_ok_and(|s| s.users.is_empty())
    }

    /// What the bus sees.
    pub fn view(&self, credentials_available: bool) -> LocalUsers {
        let users = self
            .stored
            .as_ref()
            .map(|s| s.users.clone())
            .unwrap_or_default();
        LocalUsers {
            users: users
                .into_iter()
                .map(|u| LocalUser {
                    current: self.session.as_deref() == Some(&u.name),
                    name: u.name,
                    uid: u.uid,
                    display_name: u.display,
                    kind: u.kind,
                    picture: u.picture,
                    auto_login: u.auto_login,
                })
                .collect(),
            credentials_available,
        }
    }

    /// `account.session` from the sign-in screen.
    pub fn set_session(&mut self, user: Option<String>) -> Result<(), Refusal> {
        if let Some(name) = &user {
            self.user(name)?;
        }
        self.session = user;
        Ok(())
    }

    /// True when the change needs an administrator's password in the
    /// system dialog (the own display name, picture and password with the
    /// current one do not).
    pub fn needs_admin(&self, op: &Op) -> bool {
        let own = self.session.is_some() && op.target() == self.session.as_deref();
        match op {
            Op::Rename(_) | Op::Picture(_) => !own,
            Op::Password(p) => !(own && p.current.is_some()),
            _ => true,
        }
    }

    /// Checks a change before anybody is asked (no dialog for a change that
    /// would be refused anyway).
    pub fn precheck(&self, op: &Op) -> Result<(), Refusal> {
        let s = self.stored()?;
        match op {
            Op::Create(c) => {
                if !valid_name(&c.name) {
                    return Err(refuse(Key::InvalidName));
                }
                if self.config.system.names.contains(&c.name) {
                    return Err(refuse(Key::ReservedName));
                }
                if s.users.iter().any(|u| u.name == c.name) {
                    return Err(refuse(Key::NameTaken));
                }
                if s.users.len() >= MAX_LOCAL_USERS {
                    return Err(refuse(Key::TooManyUsers));
                }
                if !valid_display(&c.display_name) {
                    return Err(refuse(Key::InvalidDisplayName));
                }
                check_new_password(&c.password, &c.confirm)
            }
            Op::Rename(r) => {
                self.user(&r.user)?;
                valid_display(&r.display_name)
                    .then_some(())
                    .ok_or_else(|| refuse(Key::InvalidDisplayName))
            }
            Op::Picture(p) => self.user(&p.user).and_then(|_| check_picture(&p.picture)),
            Op::Password(p) => self
                .user(&p.user)
                .and_then(|_| check_new_password(&p.new, &p.confirm)),
            Op::Kind(k) => self.precheck_kind(&k.user, k.kind),
            Op::AutoLogin(a) => a
                .user
                .as_deref()
                .map_or(Ok(()), |u| self.user(u).map(|_| ())),
            Op::Remove(r) => self.precheck_remove(&r.user),
        }
    }

    fn admins(&self) -> usize {
        self.stored.as_ref().map_or(0, |s| {
            s.users
                .iter()
                .filter(|u| u.kind == UserKind::Administrator)
                .count()
        })
    }

    fn precheck_kind(&self, user: &str, kind: UserKind) -> Result<(), Refusal> {
        let u = self.user(user)?;
        if u.kind == UserKind::Administrator && kind == UserKind::Standard && self.admins() <= 1 {
            return Err(refuse(Key::LastAdmin));
        }
        Ok(())
    }

    fn precheck_remove(&self, user: &str) -> Result<(), Refusal> {
        let u = self.user(user)?;
        if self.session.as_deref() == Some(user) {
            return Err(refuse(Key::CurrentUser));
        }
        if u.kind == UserKind::Administrator && self.admins() <= 1 {
            return Err(refuse(Key::LastAdmin));
        }
        Ok(())
    }

    /// Carries out a checked and authorised change.
    pub fn perform(&mut self, op: Op, ports: &Ports<'_>, now: u64) -> Result<(), Refusal> {
        self.precheck(&op)?;
        match op {
            Op::Create(c) => self.create(c, ports),
            Op::Rename(r) => self.rename(&r.user, &r.display_name, ports),
            Op::Picture(p) => self.picture(&p.user, p.picture),
            Op::Password(p) => self.password(p, ports, now),
            Op::Kind(k) => self.kind(&k.user, k.kind, ports),
            Op::AutoLogin(a) => self.edit(|s| {
                for u in &mut s.users {
                    u.auto_login = a.user.as_deref() == Some(u.name.as_str());
                }
            }),
            Op::Remove(r) => self.remove(&r.user, r.data, ports),
        }
    }

    /// Applies an edit to a copy, stores it, and only then takes it.
    fn edit(&mut self, f: impl FnOnce(&mut Stored)) -> Result<(), Refusal> {
        let mut next = self.stored()?.clone();
        f(&mut next);
        store::save(&self.config.dir, &next).map_err(|_| refuse(Key::StoreFailed))?;
        self.stored = Ok(next);
        Ok(())
    }

    fn free_uid(&self) -> Option<u32> {
        let s = self.stored.as_ref().ok()?;
        (self.config.uids.0..=self.config.uids.1).find(|id| {
            !s.users.iter().any(|u| u.uid == *id)
                && !s.retired.contains(id)
                && !self.config.system.ids.contains(id)
        })
    }

    fn create(
        &mut self,
        c: ultar_bus::messages::account_local::CreateUser,
        ports: &Ports<'_>,
    ) -> Result<(), Refusal> {
        let uid = self.free_uid().ok_or_else(|| refuse(Key::NoFreeId))?;
        ports
            .homes
            .create(&c.name, uid)
            .map_err(|_| refuse(Key::HomeFailed))?;
        let admin = c.kind == UserKind::Administrator;
        let made = ports
            .credentials
            .set_password(&c.name, &c.display_name, &c.password)
            .and_then(|()| ports.credentials.update(&c.name, None, Some(admin)));
        if let Err(e) = made {
            let _ = ports.credentials.remove(&c.name);
            let _ = ports.homes.delete(&c.name);
            return Err(cred(e));
        }
        let record = UserRecord {
            name: c.name.clone(),
            uid,
            display: c.display_name.clone(),
            kind: c.kind,
            picture: PictureInfo::Initials,
            auto_login: false,
        };
        let stored = self.edit(|s| s.users.push(record));
        if stored.is_err() {
            let _ = ports.credentials.remove(&c.name);
            let _ = ports.homes.delete(&c.name);
        }
        stored
    }

    fn rename(&mut self, user: &str, display: &str, ports: &Ports<'_>) -> Result<(), Refusal> {
        let old = self.user(user)?.display.clone();
        ports
            .credentials
            .update(user, Some(display), None)
            .map_err(cred)?;
        let result = self.edit(|s| {
            if let Some(u) = s.users.iter_mut().find(|u| u.name == user) {
                u.display = display.to_owned();
            }
        });
        if result.is_err() {
            let _ = ports.credentials.update(user, Some(&old), None);
        }
        result
    }

    fn picture(&mut self, user: &str, picture: Picture) -> Result<(), Refusal> {
        let dir = self.config.dir.clone();
        let counter = self.stored()?.pictures.wrapping_add(1);
        let info = match &picture {
            Picture::Initials => PictureInfo::Initials,
            Picture::Builtin(n) => PictureInfo::Builtin(*n),
            Picture::Image(bytes) => {
                store::save_picture(&dir, user, bytes).map_err(|_| refuse(Key::StoreFailed))?;
                PictureInfo::Custom(counter)
            }
        };
        let custom = matches!(info, PictureInfo::Custom(_));
        self.edit(|s| {
            if custom {
                s.pictures = counter;
            }
            if let Some(u) = s.users.iter_mut().find(|u| u.name == user) {
                u.picture = info;
            }
        })?;
        if !custom {
            store::remove_picture(&dir, user);
        }
        Ok(())
    }

    fn password(
        &mut self,
        p: ultar_bus::messages::account_local::SetPassword,
        ports: &Ports<'_>,
        now: u64,
    ) -> Result<(), Refusal> {
        let display = self.user(&p.user)?.display.clone();
        let own = self.session.as_deref() == Some(p.user.as_str());
        if let (true, Some(current)) = (own, &p.current) {
            let wait = self.failures.remaining(&p.user, now);
            if wait > 0 {
                return Err(refuse_wait(Key::Throttled, wait));
            }
            match ports.credentials.verify(&p.user, current) {
                Check::Granted => self.failures.succeed(&p.user),
                Check::Denied(ms) => {
                    let wait = self.failures.fail(&p.user, now).max(ms);
                    return Err(refuse_wait(Key::WrongPassword, wait));
                }
                Check::Throttled(ms) => return Err(refuse_wait(Key::Throttled, ms)),
                Check::Unavailable => return Err(refuse(Key::CredentialsUnavailable)),
            }
        }
        ports
            .credentials
            .set_password(&p.user, &display, &p.new)
            .map_err(cred)
    }

    fn kind(&mut self, user: &str, kind: UserKind, ports: &Ports<'_>) -> Result<(), Refusal> {
        let old = self.user(user)?.kind;
        let admin = kind == UserKind::Administrator;
        ports
            .credentials
            .update(user, None, Some(admin))
            .map_err(cred)?;
        let result = self.edit(|s| {
            if let Some(u) = s.users.iter_mut().find(|u| u.name == user) {
                u.kind = kind;
            }
        });
        if result.is_err() {
            let _ = ports
                .credentials
                .update(user, None, Some(old == UserKind::Administrator));
        }
        result
    }

    fn remove(&mut self, user: &str, data: RemoveData, ports: &Ports<'_>) -> Result<(), Refusal> {
        let uid = self.user(user)?.uid;
        ports.credentials.remove(user).map_err(cred)?;
        let home = match data {
            RemoveData::Keep => ports.homes.retire(user, uid),
            RemoveData::Delete => ports.homes.delete(user),
        };
        self.edit(|s| {
            s.users.retain(|u| u.name != user);
            // The id is never given again while files could still carry it.
            s.retired.insert(uid);
        })?;
        store::remove_picture(&self.config.dir, user);
        self.failures.forget(user);
        if home.is_err() {
            // The account is gone; its folder stays where it was.
            self.warnings.push("home_not_removed");
        }
        Ok(())
    }

    /// Things worth a log record since the last call (fixed words).
    pub fn take_warnings(&mut self) -> Vec<&'static str> {
        std::mem::take(&mut self.warnings)
    }
}

#[cfg(test)]
#[path = "core_tests.rs"]
mod tests;
