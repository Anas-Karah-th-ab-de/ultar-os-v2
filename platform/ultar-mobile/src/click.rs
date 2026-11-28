//! What each operable node of the phone shell means.

use crate::model::quick::Toggle;

/// A click (tap) on the phone shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Click {
    /// Lock screen: unlock (the platform asks for the password, PIN or fingerprint).
    Unlock,
    /// Lock screen: emergency call.
    Emergency,
    /// Lock screen: torch.
    Torch,
    /// Lock screen: camera (works without unlocking).
    Camera,
    /// Status bar: open the quick settings.
    Status,
    /// Home: the search field.
    Search,
    /// Home: voice search (the assistant).
    Voice,
    /// An app of the grid, the dock, the search or the recents.
    App(String),
    /// Home: a page dot.
    Page(usize),
    /// Navigation bar: recent apps.
    Recents,
    /// Navigation bar: home screen.
    Home,
    /// Navigation bar: back.
    Back,
    /// Quick settings: a switch.
    Toggle(Toggle),
    /// Quick settings: the "more" part of a large tile (its settings page).
    ToggleMore(Toggle),
    /// Quick settings: open Settings.
    Settings,
    /// Quick settings: power menu.
    Power,
    /// Quick settings: dismiss one notification.
    Dismiss(u64),
    /// Quick settings: clear all notifications.
    ClearAll,
    /// Recents: close one app.
    CloseRecent(String),
    /// Recents: close all.
    ClearRecents,
    /// Power menu: restart.
    PowerRestart,
    /// Power menu: power off.
    PowerOff,
    /// Power menu: lock the screen now.
    LockNow,
    /// Power menu: close it.
    PowerCancel,
    /// Sign-in: check the typed password or PIN.
    Submit,
    /// Sign-in: password ↔ PIN.
    Method,
    /// Sign-in: back to the cover.
    SignInBack,
    /// Continue this app on another device (recent apps; the quick tile's arrow: the app
    /// in front).
    MeshSend(String),
    /// The choice: this device.
    MeshChoose(usize),
    /// The choice: send to the chosen device.
    MeshConfirm,
    /// The choice: cancel.
    MeshCancel,
    /// The choice: back to the page before (the arrow of the header).
    MeshBack,
    /// A notice: take the waiting offer back.
    MeshOfferCancel,
    /// A notice: send again.
    MeshRetry,
    /// A notice: open the file that arrived.
    MeshOpen,
    /// A notice: close it.
    MeshNoticeClose,
}
