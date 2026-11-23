//! The update manifest: what a release is, for whom, and what its payloads must hash to.
//!
//! The manifest is a small text with one canonical byte form (see `docs/SYSTEM-UPDATE.md` §2.1).
//! [`Manifest::parse`] accepts only that form: it parses, encodes again and compares.

use crate::error::Malformed;
use crate::version::Version;
use std::fmt::Write as _;

/// Largest manifest in bytes.
pub const MAX_MANIFEST: usize = 16 * 1024;
/// Largest single payload (64 GiB).
pub const MAX_PAYLOAD: u64 = 64 * 1024 * 1024 * 1024;
/// Most entries in the device list.
pub const MAX_DEVICES: usize = 64;
/// Longest identifier.
pub const MAX_IDENT: usize = 32;
/// Longest release-notes key.
pub const MAX_NOTES_KEY: usize = 96;
const HEADER: &str = "ultar-update-manifest 1";

/// What a payload is written to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Role {
    /// The kernel image.
    Kernel,
    /// The system image (verity protected).
    System,
    /// Boot code / start partition content.
    Boot,
}

impl Role {
    /// All roles in manifest order.
    pub const ALL: [Role; 3] = [Role::Kernel, Role::System, Role::Boot];

    /// The lower-case name used in manifests and file names.
    pub fn name(self) -> &'static str {
        match self {
            Role::Kernel => "kernel",
            Role::System => "system",
            Role::Boot => "boot",
        }
    }

    /// The role with this name.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.name() == name)
    }
}

/// One payload of a release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Payload {
    /// Where it goes.
    pub role: Role,
    /// Exact size in bytes.
    pub size: u64,
    /// SHA-256 of the payload.
    pub sha256: [u8; 32],
    /// dm-verity root hash of the image, when it has one.
    pub verity_root: Option<[u8; 32]>,
}

/// A release description. See the module documentation for the text form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    /// Release version.
    pub version: Version,
    /// Anti-rollback index; a device never accepts a smaller one than it has stored.
    pub rollback_index: u64,
    /// Update channel.
    pub channel: String,
    /// Edition the release is built for.
    pub edition: String,
    /// Processor architecture.
    pub arch: String,
    /// Devices that may install it; the single entry `*` means every device.
    pub devices: Vec<String>,
    /// Oldest running version that may update directly to this release.
    pub min_version: Version,
    /// i18n key of the release notes.
    pub notes_key: String,
    /// When the manifest was signed (seconds since 1970).
    pub created: u64,
    /// After this time the manifest is no longer accepted.
    pub expires: u64,
    /// The release closes a serious hole; the UI urges the user.
    pub critical: bool,
    /// The payloads in role order.
    pub payloads: Vec<Payload>,
}

/// True for identifiers as the server defines them: 1–32 of `a-z A-Z 0-9 . _ - +`, first a
/// letter or digit, no `..`.
pub fn valid_ident(text: &str) -> bool {
    let first_ok = text
        .bytes()
        .next()
        .is_some_and(|b| b.is_ascii_alphanumeric());
    first_ok
        && text.len() <= MAX_IDENT
        && !text.contains("..")
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'+'))
}

fn valid_notes_key(text: &str) -> bool {
    let first_ok = text.bytes().next().is_some_and(|b| b.is_ascii_alphabetic());
    first_ok
        && text.len() <= MAX_NOTES_KEY
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

fn hash_from_hex(text: &str) -> Option<[u8; 32]> {
    let lower = text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    if !lower {
        return None;
    }
    let mut out = [0u8; 32];
    hex::decode_to_slice(text, &mut out).ok()?;
    Some(out)
}

fn field<'a>(lines: &mut std::str::Lines<'a>, name: &str) -> Result<&'a str, Malformed> {
    let line = lines.next().ok_or(Malformed::Text)?;
    line.strip_prefix(name)
        .and_then(|rest| rest.strip_prefix('='))
        .ok_or(Malformed::Text)
}

fn parse_payload(text: &str) -> Result<Payload, Malformed> {
    let mut parts = text.split(' ');
    let mut next = || parts.next().ok_or(Malformed::Field);
    let role = Role::from_name(next()?).ok_or(Malformed::Field)?;
    let size: u64 = next()?.parse().map_err(|_| Malformed::Field)?;
    let sha256 = hash_from_hex(next()?).ok_or(Malformed::Field)?;
    let verity_root = match next()? {
        "-" => None,
        hex => Some(hash_from_hex(hex).ok_or(Malformed::Field)?),
    };
    Ok(Payload {
        role,
        size,
        sha256,
        verity_root,
    })
}

impl Manifest {
    /// Checks the rules that the text form cannot express by itself.
    pub fn validate(&self) -> Result<(), Malformed> {
        let idents = [&self.channel, &self.edition, &self.arch];
        if !idents.iter().all(|i| valid_ident(i)) || !valid_notes_key(&self.notes_key) {
            return Err(Malformed::Field);
        }
        let any_device = self.devices.len() == 1 && self.devices[0] == "*";
        let listed = !self.devices.is_empty()
            && self.devices.len() <= MAX_DEVICES
            && self.devices.iter().all(|d| valid_ident(d))
            && self.devices.windows(2).all(|w| w[0] < w[1]);
        if !any_device && !listed {
            return Err(Malformed::Field);
        }
        if self.expires < self.created || self.min_version > self.version {
            return Err(Malformed::Field);
        }
        let ordered = self.payloads.windows(2).all(|w| w[0].role < w[1].role);
        let sized = self
            .payloads
            .iter()
            .all(|p| p.size > 0 && p.size <= MAX_PAYLOAD);
        if self.payloads.is_empty() || !ordered || !sized {
            return Err(Malformed::Field);
        }
        Ok(())
    }

    /// The canonical bytes (what is signed). Fails when [`Manifest::validate`] fails or the
    /// result would exceed [`MAX_MANIFEST`].
    pub fn encode(&self) -> Result<Vec<u8>, Malformed> {
        self.validate()?;
        let mut out = String::new();
        let _ = writeln!(out, "{HEADER}");
        let _ = writeln!(out, "version={}", self.version);
        let _ = writeln!(out, "rollback_index={}", self.rollback_index);
        let _ = writeln!(out, "channel={}", self.channel);
        let _ = writeln!(out, "edition={}", self.edition);
        let _ = writeln!(out, "arch={}", self.arch);
        let _ = writeln!(out, "devices={}", self.devices.join(","));
        let _ = writeln!(out, "min_version={}", self.min_version);
        let _ = writeln!(out, "notes_key={}", self.notes_key);
        let _ = writeln!(out, "created={}", self.created);
        let _ = writeln!(out, "expires={}", self.expires);
        let _ = writeln!(out, "critical={}", u8::from(self.critical));
        for p in &self.payloads {
            let verity = p.verity_root.map_or_else(|| "-".to_string(), hex::encode);
            let hash = hex::encode(p.sha256);
            let _ = writeln!(out, "payload={} {} {hash} {verity}", p.role.name(), p.size);
        }
        if out.len() > MAX_MANIFEST {
            return Err(Malformed::Length);
        }
        Ok(out.into_bytes())
    }

    fn parse_lines(text: &str) -> Result<Self, Malformed> {
        let mut lines = text.lines();
        if lines.next() != Some(HEADER) {
            return Err(Malformed::Magic);
        }
        let number = |t: &str| t.parse::<u64>().map_err(|_| Malformed::Field);
        let version = Version::parse(field(&mut lines, "version")?).ok_or(Malformed::Field)?;
        let rollback_index = number(field(&mut lines, "rollback_index")?)?;
        let channel = field(&mut lines, "channel")?.to_string();
        let edition = field(&mut lines, "edition")?.to_string();
        let arch = field(&mut lines, "arch")?.to_string();
        let devices = field(&mut lines, "devices")?;
        let devices = devices.split(',').map(str::to_string).collect();
        let min_version =
            Version::parse(field(&mut lines, "min_version")?).ok_or(Malformed::Field)?;
        let notes_key = field(&mut lines, "notes_key")?.to_string();
        let created = number(field(&mut lines, "created")?)?;
        let expires = number(field(&mut lines, "expires")?)?;
        let critical = match field(&mut lines, "critical")? {
            "0" => false,
            "1" => true,
            _ => return Err(Malformed::Field),
        };
        let mut payloads = Vec::new();
        for line in lines {
            if payloads.len() == Role::ALL.len() {
                return Err(Malformed::Text);
            }
            let rest = line.strip_prefix("payload=").ok_or(Malformed::Text)?;
            payloads.push(parse_payload(rest)?);
        }
        Ok(Self {
            version,
            rollback_index,
            channel,
            edition,
            arch,
            devices,
            min_version,
            notes_key,
            created,
            expires,
            critical,
            payloads,
        })
    }

    /// Parses canonical manifest bytes. Anything that would not be produced by
    /// [`Manifest::encode`] byte for byte is refused.
    pub fn parse(bytes: &[u8]) -> Result<Self, Malformed> {
        if bytes.len() > MAX_MANIFEST {
            return Err(Malformed::Length);
        }
        let text = std::str::from_utf8(bytes).map_err(|_| Malformed::Text)?;
        let manifest = Self::parse_lines(text)?;
        let canonical = manifest.encode()?;
        if canonical != bytes {
            return Err(Malformed::Text);
        }
        Ok(manifest)
    }

    /// The payload for `role`, if the release has one.
    pub fn payload(&self, role: Role) -> Option<&Payload> {
        self.payloads.iter().find(|p| p.role == role)
    }

    /// Sum of all payload sizes.
    pub fn payload_bytes(&self) -> u64 {
        // At most three payloads of at most 64 GiB each: cannot overflow.
        self.payloads.iter().map(|p| p.size).sum()
    }

    /// True when `device` may install the release.
    pub fn allows_device(&self, device: &str) -> bool {
        self.devices.iter().any(|d| d == "*" || d == device)
    }
}

#[cfg(test)]
#[path = "manifest_tests.rs"]
pub(crate) mod tests;
