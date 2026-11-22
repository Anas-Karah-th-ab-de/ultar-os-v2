//! Where the raw facts come from: the [`ProfileSource`] trait and the detection built on it.

use crate::capabilities::Capabilities;
use crate::capability::Capability;
use crate::edition::resolve_edition;
use crate::form_factor::{detect_form_factor, edition_from_kernel_info, DetectInputs, FormFactor};
use crate::profile::{device_id, DeviceProfile};

/// Supplier of the raw facts a profile is built from.
///
/// The real implementation reads kernel and firmware interfaces and lives in the HAL;
/// this crate never touches hardware paths itself.
pub trait ProfileSource {
    /// Kernel command line.
    fn kernel_cmdline(&self) -> Option<String>;
    /// Edition requested through the process environment.
    fn edition_env(&self) -> Option<String>;
    /// Content of the edition file written by the image build.
    fn edition_file(&self) -> Option<String>;
    /// Device description of the Ultar kernel (`key=value` lines), if that kernel runs.
    fn kernel_device_info(&self) -> Option<String>;
    /// SMBIOS chassis type as decimal text.
    fn chassis_type(&self) -> Option<String>;
    /// True if the platform is described by a device tree.
    fn has_device_tree(&self) -> bool;
    /// Model name of the device.
    fn model(&self) -> Option<String>;
    /// Kernel id (release string).
    fn kernel_id(&self) -> Option<String>;
    /// Capabilities found on the device.
    fn capabilities(&self) -> Capabilities;
}

/// Builds the device profile from a source.
///
/// The form factor is detected from the kernel description, chassis type, device tree,
/// battery and display size. The edition follows the configured sources (command line,
/// environment, file) before the detected one. If nothing could be detected, the form
/// factor follows the resolved edition.
pub fn detect(source: &dyn ProfileSource) -> DeviceProfile {
    let capabilities = source.capabilities();
    let kernel_info = source.kernel_device_info();
    let chassis = source.chassis_type();
    let inputs = DetectInputs {
        kernel_info: kernel_info.as_deref(),
        chassis: chassis.as_deref(),
        device_tree: source.has_device_tree(),
        battery: capabilities.has(Capability::Battery),
        display_diagonal_mm: capabilities
            .primary_display
            .and_then(|geometry| geometry.diagonal_mm()),
    };
    let detected_form_factor = detect_form_factor(&inputs);
    let detected_edition = kernel_info
        .as_deref()
        .and_then(edition_from_kernel_info)
        .or_else(|| detected_form_factor.and_then(FormFactor::edition_hint));
    let edition = resolve_edition(
        source.kernel_cmdline().as_deref().unwrap_or_default(),
        source.edition_env().as_deref(),
        source.edition_file().as_deref(),
        detected_edition,
    );
    DeviceProfile {
        device_id: device_id(
            source.model().as_deref().unwrap_or_default(),
            source.kernel_id().as_deref().unwrap_or_default(),
        ),
        form_factor: detected_form_factor.unwrap_or(FormFactor::from_edition(edition)),
        edition,
        capabilities,
    }
}

/// In-memory [`ProfileSource`] for tests, tools and simulated devices.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemorySource {
    /// Kernel command line.
    pub kernel_cmdline: Option<String>,
    /// Edition from the environment.
    pub edition_env: Option<String>,
    /// Content of the edition file.
    pub edition_file: Option<String>,
    /// Device description of the Ultar kernel.
    pub kernel_device_info: Option<String>,
    /// SMBIOS chassis type as decimal text.
    pub chassis_type: Option<String>,
    /// Device tree present.
    pub device_tree: bool,
    /// Model name.
    pub model: Option<String>,
    /// Kernel id.
    pub kernel_id: Option<String>,
    /// Capabilities of the simulated device.
    pub capabilities: Capabilities,
}

impl MemorySource {
    /// Empty source: nothing known, no capabilities.
    pub fn new() -> Self {
        MemorySource::default()
    }

    /// Sets model name and kernel id.
    #[must_use]
    pub fn with_identity(mut self, model: &str, kernel_id: &str) -> Self {
        self.model = Some(model.to_string());
        self.kernel_id = Some(kernel_id.to_string());
        self
    }

    /// Sets the SMBIOS chassis type.
    #[must_use]
    pub fn with_chassis(mut self, chassis_type: u8) -> Self {
        self.chassis_type = Some(chassis_type.to_string());
        self
    }

    /// Marks the platform as described by a device tree.
    #[must_use]
    pub fn with_device_tree(mut self) -> Self {
        self.device_tree = true;
        self
    }

    /// Sets the kernel command line.
    #[must_use]
    pub fn with_cmdline(mut self, cmdline: &str) -> Self {
        self.kernel_cmdline = Some(cmdline.to_string());
        self
    }

    /// Sets the capabilities.
    #[must_use]
    pub fn with_capabilities(mut self, capabilities: Capabilities) -> Self {
        self.capabilities = capabilities;
        self
    }
}

impl ProfileSource for MemorySource {
    fn kernel_cmdline(&self) -> Option<String> {
        self.kernel_cmdline.clone()
    }

    fn edition_env(&self) -> Option<String> {
        self.edition_env.clone()
    }

    fn edition_file(&self) -> Option<String> {
        self.edition_file.clone()
    }

    fn kernel_device_info(&self) -> Option<String> {
        self.kernel_device_info.clone()
    }

    fn chassis_type(&self) -> Option<String> {
        self.chassis_type.clone()
    }

    fn has_device_tree(&self) -> bool {
        self.device_tree
    }

    fn model(&self) -> Option<String> {
        self.model.clone()
    }

    fn kernel_id(&self) -> Option<String> {
        self.kernel_id.clone()
    }

    fn capabilities(&self) -> Capabilities {
        self.capabilities
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edition::Edition;

    #[test]
    fn empty_source_falls_back_to_phone() {
        let profile = detect(&MemorySource::new());
        assert_eq!(profile.edition, Edition::Phone);
        assert_eq!(profile.form_factor, FormFactor::Phone);
        assert_eq!(profile.device_id, "unknown | ");
        assert_eq!(profile.capabilities, Capabilities::default());
    }

    #[test]
    fn chassis_decides_form_factor_and_edition() {
        let profile = detect(
            &MemorySource::new()
                .with_chassis(10)
                .with_identity("Vendor Book", "6.8.0"),
        );
        assert_eq!(profile.form_factor, FormFactor::Laptop);
        assert_eq!(profile.edition, Edition::Desktop);
        assert_eq!(profile.device_id, "Vendor Book | 6.8.0");
    }

    #[test]
    fn configured_edition_beats_detection_but_keeps_form_factor() {
        let source = MemorySource::new()
            .with_chassis(35)
            .with_cmdline("quiet ultar.edition=server");
        let profile = detect(&source);
        assert_eq!(profile.form_factor, FormFactor::Desktop);
        assert_eq!(profile.edition, Edition::Server);
    }

    #[test]
    fn unknown_hardware_follows_configured_edition() {
        let mut source = MemorySource::new();
        source.edition_file = Some("# image\nserver\n".into());
        assert_eq!(detect(&source).form_factor, FormFactor::Server);
        source.edition_env = Some("desktop".into());
        let profile = detect(&source);
        assert_eq!(profile.edition, Edition::Desktop);
        assert_eq!(profile.form_factor, FormFactor::Desktop);
    }

    #[test]
    fn kernel_description_wins() {
        let mut source = MemorySource::new().with_chassis(35);
        source.kernel_device_info = Some("art=server\nausgabe=server\n".into());
        let profile = detect(&source);
        assert_eq!(profile.form_factor, FormFactor::Server);
        assert_eq!(profile.edition, Edition::Server);
    }

    #[test]
    fn board_keeps_default_edition() {
        let profile = detect(&MemorySource::new().with_device_tree());
        assert_eq!(profile.form_factor, FormFactor::Board);
        assert_eq!(profile.edition, Edition::Phone);
    }
}
