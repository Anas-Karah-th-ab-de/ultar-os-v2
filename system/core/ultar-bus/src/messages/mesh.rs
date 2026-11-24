//! Contract of the device mesh (`ultar-mesh`): the person's own devices find each other,
//! are paired, and hand work over ("continue on the other device").
//!
//! A device is described by what it *can do* ([`Capability`]), not by what it is: a TV is a
//! device with a screen and sound but without files, a speaker one with sound only. What may
//! be handed to a device follows from its capabilities, so a new kind of device needs no
//! change here.

use crate::message::{Call, Message, Topic};
use serde::{Deserialize, Serialize};

/// Name of the service on the bus.
pub const SERVICE: &str = "mesh";

/// What a device is (for its icon and its name only – never for a decision).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum DeviceKind {
    /// A phone.
    Phone,
    /// A tablet.
    Tablet,
    /// A desktop or laptop computer.
    #[default]
    Desktop,
    /// A server without a screen.
    Server,
    /// A development board.
    Board,
    /// A television.
    Tv,
    /// A speaker.
    Speaker,
    /// A kind this version does not know.
    Other,
}

/// One thing a device can do.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Capability {
    /// It shows pictures (has a screen).
    Screen,
    /// It takes pointer, touch or key input.
    Input,
    /// It plays sound.
    AudioOut,
    /// It records sound.
    AudioIn,
    /// It keeps files of the person.
    Files,
    /// It runs apps (can continue an activity in an app).
    Apps,
    /// It has a clipboard.
    Clipboard,
    /// It lends computing power.
    Compute,
    /// It has a language model small enough for short answers and simple commands.
    ModelSmall,
    /// Its language model manages several steps in a row.
    ModelMedium,
    /// Its language model manages long tasks.
    ModelLarge,
}

/// One thing a paired device is allowed to do on this device.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Right {
    /// Offer an activity to continue here (always asked on this device).
    Handoff,
    /// Send files here (always asked on this device).
    Files,
    /// Share the clipboard with this device.
    Clipboard,
    /// See this device's screen.
    View,
    /// Control this device.
    Control,
    /// Play and record sound here.
    Audio,
    /// Run computing jobs here.
    Compute,
    /// Let this device's language model think for it.
    Model,
}

/// One device of the mesh as this device sees it.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Peer {
    /// Its id (derived from its key).
    pub id: String,
    /// Its name (data).
    pub name: String,
    /// What it is.
    pub kind: DeviceKind,
    /// What it can do.
    pub capabilities: Vec<Capability>,
    /// Paired with this device.
    pub paired: bool,
    /// Connected right now.
    pub reachable: bool,
    /// What it may do here (empty when not paired).
    pub rights: Vec<Right>,
}

/// `mesh.state` – the mesh as this device sees it (retained).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct MeshState {
    /// The mesh is switched on.
    pub enabled: bool,
    /// This device's id.
    pub id: String,
    /// This device's name (data).
    pub name: String,
    /// The devices seen or paired.
    pub peers: Vec<Peer>,
}

/// `mesh.set_enabled` – switch the mesh on or off (off: no socket is open).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct SetEnabled {
    /// On.
    pub enabled: bool,
}

/// A plain "done or not".
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Done {
    /// It was done.
    pub ok: bool,
}

/// `mesh.pair_begin` – show a pairing code on this device.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct PairBegin;

/// Answer to [`PairBegin`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct PairCode {
    /// Six digits, valid for [`PairCode::seconds`].
    pub code: String,
    /// How long the code is valid.
    pub seconds: u32,
}

/// `mesh.pair_join` – pair with the device that shows `code`.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct PairJoin {
    /// The other device's id.
    pub device: String,
    /// The code it shows.
    pub code: String,
}

/// `mesh.unpair` – forget a paired device.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Unpair {
    /// Its id.
    pub device: String,
}

/// `mesh.set_rights` – what a paired device may do here.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct SetRights {
    /// Its id.
    pub device: String,
    /// The rights it has from now on.
    pub rights: Vec<Right>,
}

/// What the person is doing in an app – enough to continue it elsewhere.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct Activity {
    /// App id.
    pub app: String,
    /// Title shown in the offer (data).
    pub title: String,
    /// What is open, as a URI of a scheme the app declares (position included).
    pub uri: Option<String>,
    /// A file that must travel with it (absolute path on this device).
    pub file: Option<String>,
}

/// `mesh.set_activity` – an app (or the shell for it) reports the activity in front.
/// `None` clears it.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct SetActivity {
    /// The activity.
    pub activity: Option<Activity>,
}

/// How an activity arrives on the other device.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    /// The same app opens at the same place.
    State,
    /// The file travels and is opened there.
    File,
    /// The other device shows this device's window.
    RemoteView,
}

/// One device an activity can be handed to.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Target {
    /// Its id.
    pub device: String,
    /// Its name (data).
    pub name: String,
    /// What it is.
    pub kind: DeviceKind,
    /// How the activity would arrive.
    pub stage: Stage,
}

/// `mesh.handoff_send` – hand the activity in front to another device. Without `target`
/// the mesh picks the only possible device, or asks which one.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct HandoffSend {
    /// The chosen device, once the person chose.
    pub target: Option<String>,
}

/// Answer to [`HandoffSend`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub enum SendOutcome {
    /// The offer is on its way; the other device asks its person.
    Offered {
        /// Id of the offer (see [`HandoffProgress`]).
        offer: u64,
        /// The device it went to.
        target: Target,
    },
    /// Several devices are possible: the sender chooses, then sends again with `target`.
    Choose(Vec<Target>),
    /// No reachable paired device can take it.
    NoTarget,
    /// Nothing is in front that can be handed over.
    NoActivity,
}

/// `mesh.handoff_incoming` – another device offers an activity; this device asks its person.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct HandoffIncoming {
    /// Id of the offer on this device.
    pub offer: u64,
    /// Who offers.
    pub from: String,
    /// Its name (data).
    pub from_name: String,
    /// Title of the activity (data).
    pub title: String,
    /// App id.
    pub app: String,
    /// How it would arrive.
    pub stage: Stage,
    /// Kind of the offering device.
    pub from_kind: DeviceKind,
    /// Size of the file that would arrive, in bytes.
    pub size: Option<u64>,
}

/// `mesh.handoff_answer` – the person's answer to a [`HandoffIncoming`].
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct HandoffAnswer {
    /// The offer.
    pub offer: u64,
    /// Accepted.
    pub accept: bool,
}

/// Where an offer stands.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum OfferState {
    /// Waiting for the person on the receiving device.
    Asked,
    /// Accepted; data is on its way.
    Accepted,
    /// The activity is open on the receiving device.
    Opened,
    /// The person declined.
    Declined,
    /// Nobody answered in time.
    Expired,
    /// It failed (device gone, file unreadable, app refused).
    Failed,
}

/// `mesh.handoff_progress` – an offer changed (on the sending and the receiving device).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct HandoffProgress {
    /// The offer.
    pub offer: u64,
    /// The other device.
    pub device: String,
    /// This device sent the offer.
    pub outgoing: bool,
    /// Where it stands.
    pub state: OfferState,
}

/// `mesh.handoff_targets` – the devices the activity in front can go to.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct HandoffTargets;

/// Answer to [`HandoffTargets`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct TargetList {
    /// The devices (empty: none, or nothing in front).
    pub targets: Vec<Target>,
}

/// `mesh.send_file` – send a file to a device (asked there like a handoff).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct SendFile {
    /// The device.
    pub device: String,
    /// Absolute path of the file here.
    pub path: String,
}

/// `mesh.clipboard_set` – the clipboard of this device changed.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct ClipboardSet {
    /// The text.
    pub text: String,
    /// Marked confidential (a password): never leaves this device.
    pub confidential: bool,
}

/// `mesh.clipboard` – text another device copied (the clipboard owner takes it over).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct ClipboardRemote {
    /// Where it was copied.
    pub from: String,
    /// The text.
    pub text: String,
}

macro_rules! version_one {
    ($($t:ty),*) => { $( impl Message for $t { const VERSION: u16 = 1; } )* };
}
version_one!(
    Peer,
    MeshState,
    SetEnabled,
    Done,
    PairBegin,
    PairCode,
    PairJoin,
    Unpair,
    SetRights,
    Activity,
    SetActivity,
    Target,
    HandoffSend,
    SendOutcome,
    HandoffIncoming,
    HandoffAnswer,
    HandoffProgress,
    HandoffTargets,
    TargetList,
    SendFile,
    ClipboardSet,
    ClipboardRemote
);

impl Topic for MeshState {
    const TOPIC: &'static str = "mesh.state";
    const RETAINED: bool = true;
}

impl Topic for HandoffIncoming {
    const TOPIC: &'static str = "mesh.handoff_incoming";
    const RETAINED: bool = false;
}

impl Topic for HandoffProgress {
    const TOPIC: &'static str = "mesh.handoff_progress";
    const RETAINED: bool = false;
}

impl Topic for ClipboardRemote {
    const TOPIC: &'static str = "mesh.clipboard";
    const RETAINED: bool = false;
}

macro_rules! call {
    ($t:ty, $m:literal, $r:ty) => {
        impl Call for $t {
            const SERVICE: &'static str = SERVICE;
            const METHOD: &'static str = $m;
            type Reply = $r;
        }
    };
}
call!(SetEnabled, "set_enabled", Done);
call!(PairBegin, "pair_begin", PairCode);
call!(PairJoin, "pair_join", Done);
call!(Unpair, "unpair", Done);
call!(SetRights, "set_rights", Done);
call!(SetActivity, "set_activity", Done);
call!(HandoffSend, "handoff_send", SendOutcome);
call!(HandoffAnswer, "handoff_answer", Done);
call!(HandoffTargets, "handoff_targets", TargetList);
call!(SendFile, "send_file", SendOutcome);
call!(ClipboardSet, "clipboard_set", Done);
