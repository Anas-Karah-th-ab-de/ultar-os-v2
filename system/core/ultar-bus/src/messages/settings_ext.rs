//! Extended contracts of the settings service (`settingsd`), added with the
//! first real implementation of the service: reset, listing, confirm or
//! revert, display combinations and store reports.

use super::settings::SettingValue;
use crate::message::{Call, Message, Text, Topic};
use serde::{Deserialize, Serialize};

/// Service-defined failure codes of `settingsd`, carried as
/// [`crate::ErrorCode::Service`].
pub mod code {
    /// No setting is declared under the key.
    pub const UNKNOWN_KEY: u32 = 1;
    /// The value has another type than the declared one.
    pub const TYPE_MISMATCH: u32 = 2;
    /// A number lies outside the declared range or off the declared step.
    pub const OUT_OF_RANGE: u32 = 3;
    /// A text, choice or list entry is not acceptable for the setting.
    pub const INVALID_VALUE: u32 = 4;
    /// The device lacks a capability the setting needs.
    pub const UNAVAILABLE: u32 = 5;
    /// The key names an instance (display, app) that is not known.
    pub const UNKNOWN_INSTANCE: u32 = 6;
    /// Nothing is waiting for confirmation under the key.
    pub const NOT_PENDING: u32 = 7;
    /// A limit of the service (pending changes, instances) was reached.
    pub const LIMIT: u32 = 8;
    /// The component that puts the setting into effect refused the value.
    pub const APPLY_FAILED: u32 = 9;
    /// The value could not be written to disk.
    pub const STORE_FAILED: u32 = 10;
    /// A request field (panel name, display description) is not acceptable.
    pub const BAD_REQUEST: u32 = 11;
}

macro_rules! message {
    ($($name:ty),+) => {$(
        impl Message for $name {
            const VERSION: u16 = 1;
        }
    )+};
}

macro_rules! call {
    ($name:ty, $method:literal, $reply:ty) => {
        impl Call for $name {
            const SERVICE: &'static str = "settings";
            const METHOD: &'static str = $method;
            type Reply = $reply;
        }
    };
}

/// `settings.reset` – return one setting to its default.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Reset {
    /// Hierarchical setting key.
    pub key: String,
}

/// Answer to [`Reset`].
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct ResetReply {
    /// `false` when the setting already had its default.
    pub changed: bool,
}

/// `settings.list` – the panels and items the caller should see.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct ListRequest {
    /// Restrict the answer to one panel (its identifier), or all panels.
    pub panel: Option<String>,
}

/// How dangerous a change of the setting is.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum RiskInfo {
    /// Takes effect at once and stays.
    Safe,
    /// Takes effect provisionally and is taken back unless confirmed in time.
    ConfirmRevert {
        /// Time the user has to confirm.
        seconds: u32,
    },
    /// Only administrators may change it.
    AdminOnly,
}

/// Where the value of a setting is remembered.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScopeInfo {
    /// One value for the whole device.
    System,
    /// One value per user.
    User,
    /// One value per set of connected displays.
    DisplayCombination,
}

/// One choice of a [`ItemKind::Choice`] setting.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct ChoiceInfo {
    /// Stable identifier, the value to send in `settings.set`.
    pub id: String,
    /// Label for the user.
    pub label: Text,
}

/// Type of a setting with everything a generic editor needs.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub enum ItemKind {
    /// On/off switch.
    Toggle,
    /// Whole number.
    Integer {
        /// Smallest accepted value.
        min: i64,
        /// Largest accepted value.
        max: i64,
        /// Distance between accepted values, counted from `min`.
        step: i64,
    },
    /// Fractional number.
    Decimal {
        /// Smallest accepted value.
        min: f64,
        /// Largest accepted value.
        max: f64,
        /// Distance between accepted values (`0` = any value in range).
        step: f64,
    },
    /// One of a fixed set of choices.
    Choice {
        /// The choices in display order.
        choices: Vec<ChoiceInfo>,
    },
    /// Text checked against a named format (`time`, `resolution`, …).
    Text {
        /// Name of the format.
        format: String,
    },
    /// Ordered list of texts.
    List {
        /// Name of the format of each entry.
        format: String,
        /// Largest accepted number of entries.
        max_len: u32,
    },
}

/// One setting as shown in a panel.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct ItemInfo {
    /// Concrete key; for [`ItemInfo::template`] items it contains `<id>`.
    pub key: String,
    /// Label for the user.
    pub label: Text,
    /// Longer explanation for the user.
    pub description: Text,
    /// Type and limits.
    pub kind: ItemKind,
    /// Value in effect right now (provisional values included).
    pub value: SettingValue,
    /// Value a reset would bring back.
    pub default: SettingValue,
    /// Risk class.
    pub risk: RiskInfo,
    /// Where the value is remembered.
    pub scope: ScopeInfo,
    /// Whether the caller is allowed to change it.
    pub writable: bool,
    /// Whether a provisional value is waiting for confirmation.
    pub pending: bool,
    /// The item describes a family of keys (one per app); replace `<id>`.
    pub template: bool,
}

/// One panel of the settings app.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct PanelInfo {
    /// Stable panel identifier.
    pub id: String,
    /// Title for the user.
    pub title: Text,
    /// The items of the panel in display order.
    pub items: Vec<ItemInfo>,
}

/// Answer to [`ListRequest`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
pub struct ListReply {
    /// Visible panels in display order.
    pub panels: Vec<PanelInfo>,
}

/// `settings.confirm` – keep provisional values.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct Confirm {
    /// The key to keep, or every pending key.
    pub key: Option<String>,
}

/// `settings.revert` – take provisional values back now.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct Revert {
    /// The key to take back, or every pending key.
    pub key: Option<String>,
}

/// Answer to [`Confirm`] and [`Revert`].
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct PendingReply {
    /// Number of pending changes that were resolved.
    pub resolved: u32,
}

/// One connected display as the display service sees it.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct DisplayDescriptor {
    /// Manufacturer code from the display's identification data, if any.
    pub manufacturer: String,
    /// Product code or model name, if any.
    pub product: String,
    /// Serial number, if any.
    pub serial: String,
    /// Connector name (`edp-1`, `hdmi-a-1`), used when nothing else is known.
    pub connector: String,
    /// Built into the device (laptop panel, phone screen).
    pub internal: bool,
    /// Preferred width in pixels (`0` = unknown).
    pub width_px: u32,
    /// Preferred height in pixels (`0` = unknown).
    pub height_px: u32,
    /// Pixel density (`0` = unknown).
    pub dpi: u16,
    /// Preferred refresh rate in millihertz (`0` = unknown).
    pub refresh_mhz: u32,
}

/// `settings.set_displays` – the set of connected displays changed.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct SetDisplays {
    /// Every display connected now.
    pub displays: Vec<DisplayDescriptor>,
}

/// Answer to [`SetDisplays`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct SetDisplaysReply {
    /// Stable identifier of this combination of displays.
    pub combination: String,
    /// Stable identifier of each display, in request order; the `<id>` of
    /// `display.<id>.*` keys.
    pub ids: Vec<String>,
    /// `true` when the combination was never seen before.
    pub first_time: bool,
}

/// `settings.shutdown` – write everything to disk and end the service.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Shutdown;

/// `settings.pending` – a provisional value is in effect and needs
/// confirmation.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct PendingConfirmation {
    /// Hierarchical setting key.
    pub key: String,
    /// The provisional value now in effect.
    pub value: SettingValue,
    /// The value that returns without confirmation.
    pub previous: SettingValue,
    /// Seconds from now until the value is taken back.
    pub seconds: u32,
}

/// How a pending change ended.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum PendingOutcome {
    /// Confirmed; the provisional value stays.
    Kept,
    /// Taken back on request.
    Reverted,
    /// Taken back because nobody confirmed in time.
    TimedOut,
    /// Taken back because the service restarted while it was pending.
    RevertedAtStart,
    /// Taken back because the connected displays changed.
    DisplaysChanged,
}

/// `settings.pending_resolved` – a pending change ended.
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct PendingResolved {
    /// Hierarchical setting key.
    pub key: String,
    /// How it ended.
    pub outcome: PendingOutcome,
    /// The value in effect afterwards.
    pub value: SettingValue,
}

/// What happened to a settings file.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum StoreNoticeKind {
    /// The file was damaged; a copy was put aside and defaults are in use.
    Quarantined,
    /// The file was written by a newer system; a copy was put aside.
    NewerSchema,
    /// The file was converted from an older layout.
    Migrated,
    /// A stored key is not declared any more and was dropped.
    UnknownKeyDropped,
    /// A stored value is not acceptable any more and was dropped.
    InvalidValueDropped,
    /// The file could not be read; defaults are in use.
    Unreadable,
}

/// One entry of a [`StoreReport`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct StoreNotice {
    /// File name inside the settings directory.
    pub file: String,
    /// What happened.
    pub kind: StoreNoticeKind,
    /// The affected key, or the name of the copy that was put aside.
    pub detail: String,
    /// Message for the user.
    pub text: Text,
}

/// `settings.store_report` – problems found while loading the settings
/// files (retained, so a shell that starts later still learns of them).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct StoreReport {
    /// The findings, at most a bounded number.
    pub notices: Vec<StoreNotice>,
}

message!(
    Reset,
    ResetReply,
    ListRequest,
    ListReply,
    Confirm,
    Revert,
    PendingReply,
    SetDisplays,
    SetDisplaysReply,
    Shutdown,
    PendingConfirmation,
    PendingResolved,
    StoreReport
);

call!(Reset, "reset", ResetReply);
call!(ListRequest, "list", ListReply);
call!(Confirm, "confirm", PendingReply);
call!(Revert, "revert", PendingReply);
call!(SetDisplays, "set_displays", SetDisplaysReply);
call!(Shutdown, "shutdown", ());

impl Topic for PendingConfirmation {
    const TOPIC: &'static str = "settings.pending";
}

impl Topic for PendingResolved {
    const TOPIC: &'static str = "settings.pending_resolved";
}

impl Topic for StoreReport {
    const TOPIC: &'static str = "settings.store_report";
    const RETAINED: bool = true;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{decode, encode};

    fn item() -> ItemInfo {
        ItemInfo {
            key: "display.edp-1.scale".into(),
            label: Text::key("settings.display.scale.label"),
            description: Text::key("settings.display.scale.desc"),
            kind: ItemKind::Integer {
                min: 100,
                max: 300,
                step: 25,
            },
            value: SettingValue::Integer(125),
            default: SettingValue::Integer(100),
            risk: RiskInfo::ConfirmRevert { seconds: 15 },
            scope: ScopeInfo::DisplayCombination,
            writable: true,
            pending: false,
            template: false,
        }
    }

    #[test]
    fn list_reply_round_trips() {
        let reply = ListReply {
            panels: vec![PanelInfo {
                id: "display".into(),
                title: Text::key("settings.panel.display"),
                items: vec![item()],
            }],
        };
        assert_eq!(
            decode::<ListReply>(&encode(&reply).unwrap()).unwrap(),
            reply
        );
    }

    #[test]
    fn calls_and_events_round_trip() {
        let displays = SetDisplays {
            displays: vec![DisplayDescriptor {
                connector: "edp-1".into(),
                internal: true,
                ..DisplayDescriptor::default()
            }],
        };
        assert_eq!(
            decode::<SetDisplays>(&encode(&displays).unwrap()).unwrap(),
            displays
        );
        let resolved = PendingResolved {
            key: "display.mode".into(),
            outcome: PendingOutcome::TimedOut,
            value: SettingValue::Text("extend".into()),
        };
        assert_eq!(
            decode::<PendingResolved>(&encode(&resolved).unwrap()).unwrap(),
            resolved
        );
        let report = StoreReport {
            notices: vec![StoreNotice {
                file: "user-default.toml".into(),
                kind: StoreNoticeKind::Quarantined,
                detail: "user-default.toml.0.corrupt".into(),
                text: Text::key("settingsd.store.quarantined"),
            }],
        };
        assert_eq!(
            decode::<StoreReport>(&encode(&report).unwrap()).unwrap(),
            report
        );
        assert!(encode(&Shutdown).unwrap().is_empty());
    }
}
