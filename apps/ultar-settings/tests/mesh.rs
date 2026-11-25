//! Settings › Mesh, driven by clicks: the switch, pairing in both directions, the page of
//! one device with its grants, and unpairing (docs/ui/mesh.md §2.1–2.3).

mod common;

use common::*;
use ultar_bus::messages::mesh::{DeviceKind, Grant, MeshState, Peer};
use ultar_i18n::Lang;
use ultar_settings::action::Act;
use ultar_settings::service::fake::Shared;
use ultar_settings::service::mesh::FAKE_CODE;
use ultar_settings::state::{DialogPurpose, SubPage};
use ultar_settings::App;
use ultar_ui::tokens::ThemeKind;

fn peer(id: &str, name: &str, kind: DeviceKind, paired: bool) -> Peer {
    let mut peer = Peer::new(id, name, kind);
    peer.paired = paired;
    peer.reachable = true;
    peer.set_grants(vec![Grant::Handoff, Grant::Files, Grant::Clipboard]);
    peer
}

fn mesh(enabled: bool) -> MeshState {
    MeshState {
        enabled,
        id: "abcd2345efgh6789ijkl".into(),
        name: "ultar-pc".into(),
        peers: vec![
            peer("phone-id", "KM One", DeviceKind::Phone, true),
            peer("board-id", "lab-board", DeviceKind::Board, false),
        ],
    }
}

fn open(lang: Lang, state: Option<MeshState>) -> (Shared, App) {
    let world = board_world(1, true);
    world.borrow_mut().system.mesh = state;
    let app = app(
        &world,
        lang,
        ThemeKind::Dark,
        content_size(1440.0, 900.0),
        "mesh",
    );
    (world, app)
}

fn shown(app: &App) -> String {
    let mut out = String::new();
    let mut stack = vec![app.ui.root()];
    stack.extend(app.view.dialog);
    while let Some(n) = stack.pop() {
        if let Some(w) = app.ui.widget(n) {
            out.push_str(&format!("{w:?}\n"));
        }
        stack.extend(app.ui.children(n).iter().rev());
    }
    out
}

fn press(app: &mut App, act: Act) {
    let node = app
        .view
        .node(act)
        .unwrap_or_else(|| panic!("{act:?} not shown"));
    click(app, node);
}

fn last(world: &Shared) -> String {
    log(world).last().cloned().unwrap_or_default()
}

#[test]
fn without_the_service_the_page_says_so_and_off_shows_no_devices() {
    let (_, app) = open(Lang::En, None);
    assert!(shown(&app).contains("app.settings.mesh.unavailable"));
    assert!(app.view.node(Act::MeshToggle).is_none());

    let (world, mut app) = open(Lang::Ar, Some(mesh(false)));
    assert!(shown(&app).contains("app.settings.mesh.off"));
    assert!(app.view.node(Act::MeshPair).is_none());
    press(&mut app, Act::MeshToggle);
    assert_eq!(last(&world), "mesh.set_enabled true");
    let text = shown(&app);
    assert!(
        text.contains("KM One") && text.contains("lab-board"),
        "{text}"
    );
    assert!(text.contains("abcd2345efgh6789ijkl"));
}

#[test]
fn the_device_id_is_copied_and_a_shown_code_ends_when_the_device_is_paired() {
    let (world, mut app) = open(Lang::En, Some(mesh(true)));
    press(&mut app, Act::MeshCopy);
    let copied = world.borrow().clipboard.clone();
    assert_eq!(copied.as_deref(), Some("abcd2345efgh6789ijkl"));

    press(&mut app, Act::MeshPair);
    assert_eq!(last(&world), "mesh.pair_begin");
    let purpose = app.state.dialog.as_ref().map(|d| d.purpose);
    assert_eq!(purpose, Some(DialogPurpose::MeshPairShow));
    assert!(shown(&app).contains("482 913"), "the code in two groups");
    // the other device typed the code: the dialog closes by itself
    if let Some(m) = world.borrow_mut().system.mesh.as_mut() {
        m.peers[1].paired = true;
    }
    app.poll().unwrap();
    assert!(app.state.dialog.is_none());
    assert!(
        app.view.node(Act::MeshDevice(1)).is_some(),
        "now in the paired list"
    );
}

#[test]
fn a_nearby_device_is_paired_with_its_code_and_a_wrong_code_is_said() {
    let (world, mut app) = open(Lang::En, Some(mesh(true)));
    press(&mut app, Act::MeshNearPair(0));
    assert_eq!(
        app.state.dialog.as_ref().map(|d| d.purpose),
        Some(DialogPurpose::MeshPairJoin)
    );
    let field = app.view.dialog_fields[0];
    click(&mut app, field);
    type_text(&mut app, "12ab");
    press(&mut app, Act::DialogOk);
    let error = app.state.dialog.as_ref().and_then(|d| d.error.clone());
    assert_eq!(
        error.map(|e| e.key),
        Some("app.settings.mesh.error.digits".into())
    );
    assert!(
        !last(&world).starts_with("mesh.pair_join"),
        "nothing was sent"
    );

    let field = app.view.dialog_fields[0];
    app.ui.set_text(field, "111111").unwrap();
    press(&mut app, Act::DialogOk);
    assert_eq!(last(&world), "mesh.pair_join board-id wrong");
    let error = app.state.dialog.as_ref().and_then(|d| d.error.clone());
    assert_eq!(
        error.map(|e| e.key),
        Some("app.settings.mesh.error.code".into())
    );

    let field = app.view.dialog_fields[0];
    app.ui.set_text(field, FAKE_CODE).unwrap();
    press(&mut app, Act::DialogOk);
    assert_eq!(last(&world), "mesh.pair_join board-id");
    assert!(app.state.dialog.is_none());
    assert!(app.view.node(Act::MeshNearPair(0)).is_none());
    assert!(app.view.node(Act::MeshDevice(1)).is_some());
}

#[test]
fn a_devices_rights_change_at_once_and_unpairing_asks_first() {
    let (world, mut app) = open(Lang::Ar, Some(mesh(true)));
    press(&mut app, Act::MeshDevice(0));
    assert_eq!(app.state.sub, SubPage::MeshDevice);
    for n in 0..8 {
        assert!(app.view.node(Act::MeshGrant(n)).is_some(), "grant {n}");
    }
    // the eighth switch lends the language model
    press(&mut app, Act::MeshGrant(7));
    assert_eq!(
        last(&world),
        "mesh.set_grants phone-id [Handoff, Files, Clipboard, Model]"
    );
    press(&mut app, Act::MeshGrant(1));
    assert_eq!(
        last(&world),
        "mesh.set_grants phone-id [Handoff, Clipboard, Model]"
    );

    press(&mut app, Act::MeshUnpair);
    assert_eq!(
        last(&world),
        "mesh.set_grants phone-id [Handoff, Clipboard, Model]"
    );
    press(&mut app, Act::DialogCancel);
    assert_eq!(
        app.state.sub,
        SubPage::MeshDevice,
        "cancel keeps the device"
    );
    press(&mut app, Act::MeshUnpair);
    press(&mut app, Act::DialogOk);
    assert_eq!(last(&world), "mesh.unpair phone-id");
    assert_eq!(app.state.sub, SubPage::None, "back on the mesh page");
    assert!(!shown(&app).contains("KM One"));

    press(&mut app, Act::MeshToggle);
    assert_eq!(last(&world), "mesh.set_enabled false");
}

#[test]
fn every_text_of_the_page_exists_in_all_three_languages() {
    let read = |lang: &str| {
        let file = format!("apps/ultar-settings/locales/{lang}.ftl");
        std::fs::read_to_string(repo().join(file)).unwrap()
    };
    let keys = |text: &str| -> Vec<String> {
        let lines = text.lines().filter_map(|l| l.split(" = ").next());
        let ours =
            |k: &&str| k.starts_with("app.settings.mesh.") || k.starts_with("app.settings.cloud.");
        let mesh = lines.filter(ours);
        mesh.map(str::to_owned).collect()
    };
    let english = keys(&read("en"));
    assert!(english.len() > 40);
    assert_eq!(keys(&read("ar")), english);
    assert_eq!(keys(&read("de")), english);
}

fn cloud() -> ultar_bus::messages::llm_cloud::CloudState {
    use ultar_bus::messages::llm_cloud::{CloudState, ProviderInfo};
    let provider = |id: &str, name: &str, needs_endpoint| ProviderInfo {
        id: id.into(),
        name: name.into(),
        needs_endpoint,
    };
    CloudState {
        provider: "claude".into(),
        providers: vec![
            provider("claude", "Claude", false),
            provider("compatible", "OpenAI-compatible", true),
        ],
        ..CloudState::default()
    }
}

/// Chooses the option `index` of the select behind `act`.
fn choose(app: &mut App, act: Act, index: usize) {
    let node = app
        .view
        .node(act)
        .unwrap_or_else(|| panic!("{act:?} not shown"));
    let kind = ultar_ui::event::ActionKind::Selected(index);
    app.actions(vec![ultar_ui::event::Action { node, kind }])
        .unwrap();
}

fn type_in_dialog(app: &mut App, text: &str) {
    let field = app.view.dialog_fields[0];
    app.ui.set_text(field, text).unwrap();
    press(app, Act::DialogOk);
}

fn dialog_error(app: &App) -> Option<String> {
    let error = app.state.dialog.as_ref().and_then(|d| d.error.clone());
    error.map(|e| e.key)
}

#[test]
fn the_key_of_the_cloud_model_goes_in_hidden_and_is_never_shown() {
    let world = board_world(1, true);
    // a device without the cloud-model service shows no such section
    let size = content_size(1440.0, 900.0);
    let plain = app(&world, Lang::En, ThemeKind::Dark, size, "assistant");
    assert!(plain.view.node(Act::CloudKey).is_none());
    world.borrow_mut().system.cloud = Some(cloud());
    let mut app = app(&world, Lang::Ar, ThemeKind::Dark, size, "assistant");
    let text = shown(&app);
    assert!(
        text.contains("app.settings.cloud.leaves"),
        "says what leaves the device"
    );
    assert!(text.contains("app.settings.cloud.cost"), "says who bills");
    // the order of the four ways, with the mark on the one in use: no key, so the fixed commands
    assert_eq!(text.matches("app.settings.cloud.order.now").count(), 1);
    assert_eq!(
        app.state.status.brain,
        Some(ultar_bus::messages::agent::Brain::Minimal)
    );
    assert!(
        app.view.node(Act::CloudClear).is_none(),
        "nothing to forget yet"
    );
    assert!(
        app.view.node(Act::CloudModel).is_none(),
        "no models before a key"
    );

    press(&mut app, Act::CloudKey);
    type_in_dialog(&mut app, "two words");
    let invalid = Some("app.settings.cloud.error.invalid".to_owned());
    assert_eq!(dialog_error(&app), invalid);
    // a key the provider refuses is said and not stored
    type_in_dialog(&mut app, ultar_settings::service::cloud::REFUSED_KEY);
    let refused = Some("app.settings.cloud.error.refused".to_owned());
    assert_eq!(dialog_error(&app), refused);
    assert!(!world.borrow().system.cloud.as_ref().unwrap().configured);
    type_in_dialog(&mut app, "sk-secret-123");
    assert!(app.state.dialog.is_none());
    assert_eq!(last(&world), "llmcloud.set_key (13 characters)");
    let text = shown(&app);
    assert!(
        text.contains("app.settings.cloud.on") && !text.contains("sk-secret"),
        "{text}"
    );
    let cloud = Some(ultar_bus::messages::agent::Brain::Cloud);
    assert_eq!(app.state.status.brain, cloud);

    press(&mut app, Act::CloudClear);
    press(&mut app, Act::DialogOk);
    assert_eq!(last(&world), "llmcloud.clear_key");
    assert!(shown(&app).contains("app.settings.cloud.off"));
}

#[test]
fn the_person_chooses_the_model_and_a_server_of_their_own() {
    let world = board_world(1, true);
    world.borrow_mut().system.cloud = Some(cloud());
    let size = content_size(1440.0, 900.0);
    let mut app = app(&world, Lang::En, ThemeKind::Dark, size, "assistant");
    press(&mut app, Act::CloudKey);
    type_in_dialog(&mut app, "sk-secret-123");
    assert!(
        app.view.node(Act::CloudModel).is_some(),
        "the provider's models are offered"
    );
    choose(&mut app, Act::CloudModel, 0);
    assert_eq!(
        last(&world),
        "llmcloud.configure (\"claude\", \"\", \"model-big\")"
    );
    assert!(
        world.borrow().system.cloud.as_ref().unwrap().configured,
        "the key stays"
    );

    // a server of one's own asks for its address; only https, and the key is forgotten
    assert!(app.view.node(Act::CloudAddress).is_none());
    choose(&mut app, Act::CloudProvider, 1);
    assert_eq!(
        app.state.dialog.as_ref().map(|d| d.purpose),
        Some(DialogPurpose::CloudAddress)
    );
    type_in_dialog(&mut app, "http://example.org");
    let invalid = Some("app.settings.cloud.error.invalid".to_owned());
    assert_eq!(dialog_error(&app), invalid);
    type_in_dialog(&mut app, "https://llm.example.org");
    assert!(app.state.dialog.is_none());
    let now = world.borrow().system.cloud.clone().unwrap();
    assert_eq!(
        (now.provider.as_str(), now.configured),
        ("compatible", false)
    );
    assert!(shown(&app).contains("https://llm.example.org"));
    assert!(app.view.node(Act::CloudAddress).is_some());
    // back to the provider with a fixed address: at once, without a dialog
    choose(&mut app, Act::CloudProvider, 0);
    assert!(app.state.dialog.is_none());
    assert_eq!(
        world.borrow().system.cloud.as_ref().unwrap().provider,
        "claude"
    );
}
