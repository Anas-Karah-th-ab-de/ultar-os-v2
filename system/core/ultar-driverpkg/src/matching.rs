//! The matching engine: which drivers of an index fit a device, best first.

use crate::class::{DriverClass, Origin};
use crate::hardware::DeviceDescriptor;
use crate::index::{DriverIndex, IndexEntry};
use crate::pattern::glob_match;
use crate::version::Version;
use std::cmp::Reverse;
use std::collections::BTreeMap;

/// How a driver rule matched a device; later variants are more specific.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MatchKind {
    /// Matched by device class (PCI/USB class prefix).
    Class,
    /// Matched by vendor with a wildcard device (`8086:*`).
    Wildcard,
    /// Matched by the exact id (PCI/USB id, device-tree, ACPI or platform name).
    Exact,
}

/// Specificity of a match: kind first, then the class prefix length (longer is more specific).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Specificity {
    /// How the rule matched.
    pub kind: MatchKind,
    /// Number of class digits for [`MatchKind::Class`], otherwise `0`.
    pub detail: u8,
}

impl Specificity {
    /// An exact id match.
    pub const EXACT: Self = Self {
        kind: MatchKind::Exact,
        detail: 0,
    };
    /// A vendor wildcard match.
    pub const WILDCARD: Self = Self {
        kind: MatchKind::Wildcard,
        detail: 0,
    };

    /// A class match with the given prefix length.
    pub fn class(digits: u8) -> Self {
        Self {
            kind: MatchKind::Class,
            detail: digits,
        }
    }
}

/// A driver that fits a device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// Driver identifier.
    pub id: String,
    /// Driver version.
    pub version: Version,
    /// Functional class.
    pub class: DriverClass,
    /// Where the driver comes from.
    pub origin: Origin,
    /// Rank from the manifest.
    pub rank: i32,
    /// Package file name inside the depot.
    pub file: String,
    /// i18n key of the display name.
    pub name_key: String,
    /// How specifically the driver matched.
    pub specificity: Specificity,
    /// `true` when the driver is tuned for this machine model (`dmi` rule matched).
    pub machine_tuned: bool,
}

impl Candidate {
    fn from_entry(entry: &IndexEntry, specificity: Specificity, machine_tuned: bool) -> Self {
        let manifest = &entry.manifest;
        Self {
            id: manifest.id.clone(),
            version: manifest.version.clone(),
            class: manifest.class,
            origin: manifest.origin,
            rank: manifest.rank,
            file: entry.file.clone(),
            name_key: manifest.name_key.clone(),
            specificity,
            machine_tuned,
        }
    }
}

/// `None` = excluded by the `dmi` rule, `Some(tuned)` otherwise.
fn machine_fit(entry: &IndexEntry, device: &DeviceDescriptor) -> Option<bool> {
    let patterns = &entry.manifest.rules.dmi;
    if patterns.is_empty() {
        return Some(false);
    }
    let machine = device.machine.as_deref()?;
    patterns
        .iter()
        .any(|p| glob_match(p, machine))
        .then_some(true)
}

/// Best specificity per index entry over all identifiers of the device.
fn best_hits(device: &DeviceDescriptor, index: &DriverIndex) -> BTreeMap<usize, Specificity> {
    let mut best = BTreeMap::new();
    for id in device.ids() {
        for (entry, specificity) in index.hits(id) {
            let slot = best.entry(entry).or_insert(specificity);
            *slot = (*slot).max(specificity);
        }
    }
    best
}

/// Lists the drivers that fit `device` on the given kernel ABI.
///
/// Order: specificity (exact id > wildcard > class, longer class prefix first), then
/// machine-tuned drivers, then higher rank, then newer version; remaining ties are
/// broken by driver id and file name so the result is fully deterministic.
pub fn candidates(
    device: &DeviceDescriptor,
    index: &DriverIndex,
    kernel_abi: &str,
) -> Vec<Candidate> {
    let mut found = Vec::new();
    for (position, specificity) in best_hits(device, index) {
        let Some(entry) = index.get(position) else {
            continue;
        };
        if !entry.manifest.supports_kernel(kernel_abi) {
            continue;
        }
        if let Some(tuned) = machine_fit(entry, device) {
            found.push(Candidate::from_entry(entry, specificity, tuned));
        }
    }
    found.sort_by(|a, b| {
        let key = |c: &Candidate| {
            (
                Reverse(c.specificity),
                Reverse(c.machine_tuned),
                Reverse(c.rank),
            )
        };
        key(a)
            .cmp(&key(b))
            .then_with(|| b.version.cmp(&a.version))
            .then_with(|| a.id.cmp(&b.id))
            .then_with(|| a.file.cmp(&b.file))
    });
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn specificity_order() {
        assert!(Specificity::EXACT > Specificity::WILDCARD);
        assert!(Specificity::WILDCARD > Specificity::class(6));
        assert!(Specificity::class(6) > Specificity::class(4));
        assert!(Specificity::class(4) > Specificity::class(2));
        assert!(MatchKind::Exact > MatchKind::Class);
    }
}
