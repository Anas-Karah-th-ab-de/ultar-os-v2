//! The forms the identity takes outside the screen: the `os-release` file, the user agent of
//! network clients and the device strings of USB.

use core::fmt::{self, Write};

/// The brand part of `/etc/os-release` (`rootfs/etc/os-release` is exactly this text;
/// `ultar-check brand` compares them). The image build appends the fields of one release
/// with [`write_release_fields`].
pub const OS_RELEASE: &str = concat!(
    "NAME=\"",
    crate::product!(),
    "\"\nID=",
    crate::product_id!(),
    "\nHOME_URL=\"",
    crate::website!(),
    "\"\nBUG_REPORT_URL=\"mailto:",
    crate::contact_email!(),
    "\"\nVENDOR_NAME=\"",
    crate::vendor!(),
    "\"\nVENDOR_URL=\"",
    crate::website!(),
    "\"\n"
);

/// The `Maintainer` of packages built for other package systems (`.deb`): name and public
/// contact address.
pub const PACKAGE_MAINTAINER: &str = concat!(crate::vendor!(), " <", crate::contact_email!(), ">");

/// Manufacturer string of USB device descriptors.
pub const USB_MANUFACTURER: &str = crate::VENDOR;
/// Product string of USB device descriptors.
pub const USB_PRODUCT: &str = crate::PRODUCT;

/// Longest accepted release field (version, build, variant, component).
pub const FIELD_MAX: usize = 64;

/// Why release data was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseError {
    /// A field is empty, longer than [`FIELD_MAX`] or has a character outside
    /// `A–Z a–z 0–9 . _ + -` (it would break the file or the header line).
    Field,
    /// The output refused the text.
    Output,
}

impl fmt::Display for ReleaseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReleaseError::Field => f.write_str("brand.error.field"),
            ReleaseError::Output => f.write_str("brand.error.output"),
        }
    }
}

impl core::error::Error for ReleaseError {}

impl From<fmt::Error> for ReleaseError {
    fn from(_: fmt::Error) -> Self {
        ReleaseError::Output
    }
}

/// The fields of one release of the system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Release<'a> {
    /// Version of the system image (`0.7.0`).
    pub version: &'a str,
    /// Build identifier, if the build has one.
    pub build: Option<&'a str>,
    /// Edition (`desktop`, `phone`, `server` …), if the image has one.
    pub variant: Option<&'a str>,
}

fn check_field(field: &str) -> Result<&str, ReleaseError> {
    let allowed = |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'+' | b'-');
    if field.is_empty() || field.len() > FIELD_MAX || !field.bytes().all(allowed) {
        return Err(ReleaseError::Field);
    }
    Ok(field)
}

/// Writes the release lines that follow [`OS_RELEASE`] in `/etc/os-release`:
/// `PRETTY_NAME`, `VERSION_ID` and, when present, `BUILD_ID` and `VARIANT_ID`.
pub fn write_release_fields(
    out: &mut impl Write,
    release: &Release<'_>,
) -> Result<(), ReleaseError> {
    let version = check_field(release.version)?;
    let build = release.build.map(check_field).transpose()?;
    let variant = release.variant.map(check_field).transpose()?;
    writeln!(out, "PRETTY_NAME=\"{} {version}\"", crate::PRODUCT)?;
    writeln!(out, "VERSION_ID={version}")?;
    if let Some(build) = build {
        writeln!(out, "BUILD_ID={build}")?;
    }
    if let Some(variant) = variant {
        writeln!(out, "VARIANT_ID={variant}")?;
    }
    Ok(())
}

/// Writes the complete `/etc/os-release` of one release.
pub fn write_os_release(out: &mut impl Write, release: &Release<'_>) -> Result<(), ReleaseError> {
    // validate before the first byte is written
    check_field(release.version)?;
    release.build.map(check_field).transpose()?;
    release.variant.map(check_field).transpose()?;
    out.write_str(OS_RELEASE)?;
    write_release_fields(out, release)
}

/// Writes the user agent of a network client of the system:
/// `<component>/<version> (Ultar OS; KM Code Craft; +https://code-craft.tech)`.
///
/// `component` is the program (`ultar-update`), `version` its version. Nothing about the
/// device or the user is part of it.
pub fn write_user_agent(
    out: &mut impl Write,
    component: &str,
    version: &str,
) -> Result<(), ReleaseError> {
    let component = check_field(component)?;
    let version = check_field(version)?;
    write!(
        out,
        "{component}/{version} ({}; {}; +{})",
        crate::PRODUCT,
        crate::VENDOR,
        crate::WEBSITE
    )?;
    Ok(())
}

/// A fixed text buffer for [`write_user_agent`] and [`write_os_release`] where no allocator
/// exists. Text that does not fit is refused, never cut.
#[derive(Debug, Clone, Copy)]
pub struct Line<const N: usize> {
    bytes: [u8; N],
    len: usize,
}

impl<const N: usize> Default for Line<N> {
    fn default() -> Self {
        Line {
            bytes: [0; N],
            len: 0,
        }
    }
}

impl<const N: usize> Line<N> {
    /// An empty buffer.
    pub fn new() -> Self {
        Self::default()
    }

    /// The text written so far.
    pub fn as_str(&self) -> &str {
        // Only whole `&str` values are appended, so the bytes are valid UTF-8.
        core::str::from_utf8(self.bytes.get(..self.len).unwrap_or(&[])).unwrap_or("")
    }
}

impl<const N: usize> Write for Line<N> {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        let end = self.len.checked_add(text.len()).ok_or(fmt::Error)?;
        let slot = self.bytes.get_mut(self.len..end).ok_or(fmt::Error)?;
        slot.copy_from_slice(text.as_bytes());
        self.len = end;
        Ok(())
    }
}

/// The user agent of `component` in a fixed buffer of 160 bytes.
pub fn user_agent(component: &str, version: &str) -> Result<Line<160>, ReleaseError> {
    let mut line = Line::new();
    write_user_agent(&mut line, component, version)?;
    Ok(line)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_release_brand_part_is_exact() {
        assert_eq!(
            OS_RELEASE,
            "NAME=\"Ultar OS\"\nID=ultar\nHOME_URL=\"https://code-craft.tech\"\n\
             BUG_REPORT_URL=\"mailto:info@code-craft.tech\"\nVENDOR_NAME=\"KM Code Craft\"\n\
             VENDOR_URL=\"https://code-craft.tech\"\n"
        );
    }

    #[test]
    fn os_release_of_a_release() {
        let mut out = Line::<512>::new();
        let release = Release {
            version: "0.7.0",
            build: Some("20261124.1"),
            variant: Some("desktop"),
        };
        write_os_release(&mut out, &release).unwrap();
        let text = out.as_str();
        assert!(text.starts_with(OS_RELEASE));
        assert!(text.ends_with(
            "PRETTY_NAME=\"Ultar OS 0.7.0\"\nVERSION_ID=0.7.0\nBUILD_ID=20261124.1\nVARIANT_ID=desktop\n"
        ));
        // every line is KEY=value, keys are unique
        let mut keys = [""; 16];
        for (i, line) in text.lines().enumerate() {
            let (key, value) = line.split_once('=').unwrap();
            assert!(!value.is_empty());
            assert!(!keys[..i].contains(&key), "{key} twice");
            keys[i] = key;
        }
    }

    #[test]
    fn optional_fields_are_left_out() {
        let mut out = Line::<256>::new();
        let release = Release {
            version: "1.0",
            build: None,
            variant: None,
        };
        write_release_fields(&mut out, &release).unwrap();
        assert_eq!(
            out.as_str(),
            "PRETTY_NAME=\"Ultar OS 1.0\"\nVERSION_ID=1.0\n"
        );
    }

    #[test]
    fn hostile_fields_are_refused_before_anything_is_written() {
        let long = [b'a'; FIELD_MAX + 1];
        let long = core::str::from_utf8(&long).unwrap();
        for bad in ["", "1.0\nID=evil", "1 0", "\"", "١٫٠", "a/b", long] {
            let mut out = Line::<512>::new();
            let release = Release {
                version: bad,
                build: None,
                variant: None,
            };
            assert_eq!(
                write_os_release(&mut out, &release),
                Err(ReleaseError::Field)
            );
            assert_eq!(out.as_str(), "", "{bad:?}");
            let release = Release {
                version: "1.0",
                build: Some(bad),
                variant: None,
            };
            assert_eq!(
                write_os_release(&mut out, &release),
                Err(ReleaseError::Field)
            );
            let release = Release {
                version: "1.0",
                build: None,
                variant: Some(bad),
            };
            assert_eq!(
                write_os_release(&mut out, &release),
                Err(ReleaseError::Field)
            );
            assert_eq!(out.as_str(), "", "{bad:?}");
            assert_eq!(user_agent(bad, "1.0").map(|_| ()), Err(ReleaseError::Field));
            assert_eq!(
                user_agent("ultar-update", bad).map(|_| ()),
                Err(ReleaseError::Field)
            );
        }
        let exact = &long[..FIELD_MAX];
        assert!(user_agent(exact, "1").is_ok());
    }

    #[test]
    fn user_agent_form() {
        let agent = user_agent("ultar-update", "0.7.0").unwrap();
        assert_eq!(
            agent.as_str(),
            "ultar-update/0.7.0 (Ultar OS; KM Code Craft; +https://code-craft.tech)"
        );
        assert!(agent.as_str().is_ascii());
    }

    #[test]
    fn a_full_buffer_refuses_instead_of_cutting() {
        let mut small = Line::<8>::new();
        assert_eq!(
            write_user_agent(&mut small, "ultar-update", "0.7.0"),
            Err(ReleaseError::Output)
        );
        let mut exact = Line::<4>::new();
        assert!(exact.write_str("abcd").is_ok());
        assert!(exact.write_str("e").is_err());
        assert_eq!(exact.as_str(), "abcd");
        assert!(exact.write_str("").is_ok());
    }

    #[test]
    fn package_maintainer_is_name_and_contact() {
        assert_eq!(PACKAGE_MAINTAINER, "KM Code Craft <info@code-craft.tech>");
    }

    #[test]
    fn usb_strings_are_the_facts() {
        assert_eq!(USB_MANUFACTURER, crate::VENDOR);
        assert_eq!(USB_PRODUCT, crate::PRODUCT);
    }

    #[test]
    fn errors_show_their_message_key() {
        let mut out = Line::<64>::new();
        write!(out, "{} {}", ReleaseError::Field, ReleaseError::Output).unwrap();
        assert_eq!(out.as_str(), "brand.error.field brand.error.output");
    }
}
