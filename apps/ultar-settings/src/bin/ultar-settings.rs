//! `ultar-settings` – the Settings app as a compositor client.
//!
//! ```text
//! ultar-settings [--compositor PATH] [--bus PATH] [--lang ar|en|de] [--light]
//!                [--area ID] [--catalogs DIR] [--documents DIR]
//! ```
//! `--compositor` defaults to `$ULTAR_COMPOSITOR`, `--bus` to `$ULTAR_BUS_SOCKET`,
//! `--catalogs` (service catalogs `<dir>/<service>/<lang>.ftl`) to `$ULTAR_CATALOGS`,
//! `--documents` to `$ULTAR_DOCUMENTS`.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;
use ultar_bus::{Client as Bus, ClientOptions};
use ultar_i18n::Lang;
use ultar_settings::areas::{index_of, AREAS};
use ultar_settings::service::bus::{
    BusAccount, BusDevices, BusDisplay, BusSettings, BusStatus, BusUpdate, ClipboardQueue,
    DocumentsFiles,
};
use ultar_settings::service::Ports;
use ultar_settings::{App, AppError};
use ultar_surface::client::{Client, WindowConfig};
use ultar_surface::ui::UiWindow;
use ultar_ui::paint::PaintEnv;
use ultar_ui::tokens::{InputMode, ThemeKind};
use ultar_ui::ui::Ui;
use ultar_ui::Size;

/// App id towards compositor and bus: the desktop shell's name of Settings, so the window belongs
/// to the bar button of Settings (found in VMware: a second, nameless button appeared).
const APP_ID: &str = "org.ultar.settings";
/// How often services are followed while idle.
const POLL: Duration = Duration::from_millis(500);

struct Options {
    compositor: Option<PathBuf>,
    bus: Option<PathBuf>,
    lang: Lang,
    light: bool,
    area: usize,
    catalogs: Option<PathBuf>,
    documents: Option<PathBuf>,
}

/// The page a `settings://<area>[/…]` URI names (an unknown area: none).
fn area_of_uri(uri: &str) -> Option<usize> {
    let rest = uri.strip_prefix("settings://")?;
    index_of(rest.split(['/', '?', '#']).next()?)
}

fn parse(args: impl Iterator<Item = String>) -> Result<Options, AppError> {
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    let mut o = Options {
        compositor: env("ULTAR_COMPOSITOR"),
        bus: env("ULTAR_BUS_SOCKET"),
        lang: Lang::from_env(),
        light: false,
        // `settings://<area>[/…]` from the launcher (`ULTAR_OPEN_URI`), else Displays
        area: std::env::var("ULTAR_OPEN_URI")
            .ok()
            .and_then(|uri| area_of_uri(&uri))
            .or_else(|| index_of("displays"))
            .unwrap_or(0),
        catalogs: env("ULTAR_CATALOGS"),
        documents: env("ULTAR_DOCUMENTS"),
    };
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or(AppError::Usage);
        match arg.as_str() {
            "--compositor" => o.compositor = Some(value()?.into()),
            "--bus" => o.bus = Some(value()?.into()),
            "--lang" => o.lang = Lang::parse(&value()?).ok_or(AppError::Usage)?,
            "--light" => o.light = true,
            "--area" => o.area = index_of(&value()?).ok_or(AppError::Usage)?,
            "--catalogs" => o.catalogs = Some(value()?.into()),
            "--documents" => o.documents = Some(value()?.into()),
            _ => return Err(AppError::Usage),
        }
    }
    Ok(o)
}

fn ports(bus: &Bus, documents: PathBuf, clipboard: ClipboardQueue) -> Ports {
    Ports {
        // /etc/os-release, else /usr/lib/os-release (os-release(5)): an app's cell sees /usr only
        os_release: ["/etc/os-release", "/usr/lib/os-release"]
            .iter()
            .map(PathBuf::from)
            .find(|p| p.is_file()),
        settings: Box::new(BusSettings::new(bus.clone())),
        display: Box::new(BusDisplay::new(bus.clone())),
        devices: Box::new(BusDevices(bus.clone())),
        update: Box::new(BusUpdate::new(bus.clone())),
        account: Box::new(BusAccount(bus.clone())),
        mesh: Box::new(ultar_settings::service::mesh::BusMesh::new(bus)),
        foreign: Box::new(ultar_settings::service::foreign::BusForeign::new(bus)),
        cloud: Box::new(ultar_settings::service::cloud::BusCloud::new(bus)),
        status: Box::new(BusStatus::new(bus)),
        system: Box::new(ultar_settings::service::system::BusSystem::new(bus.clone())),
        files: Box::new(DocumentsFiles { dir: documents }),
        clipboard: Box::new(clipboard),
    }
}

fn run(o: Options) -> Result<(), AppError> {
    let surface_err = |e: ultar_surface::client::ClientError| AppError::Surface(e.to_string());
    let socket = o.compositor.ok_or(AppError::Usage)?;
    let bus_path = o.bus.ok_or(AppError::Usage)?;
    let bus = Bus::connect(&bus_path, ClientOptions::named(APP_ID))
        .map_err(|e| AppError::Surface(e.to_string()))?;
    let documents = o.documents.unwrap_or_else(|| PathBuf::from("documents"));
    let clipboard = ClipboardQueue::default();
    let tr = ultar_settings::i18n::translator(o.lang, o.catalogs.as_deref());
    let theme = if o.light {
        ThemeKind::Light
    } else {
        ThemeKind::Dark
    };
    let ui = Ui::new(
        PaintEnv::new(o.lang, theme).with_session(o.light),
        InputMode::Pointer,
        Size::new(1.0, 1.0),
        tr,
    )?;
    let client = Client::connect(&socket, APP_ID).map_err(surface_err)?;
    let title = ui.translator().t("app.settings.name").into_owned();
    let mut window =
        UiWindow::new(client, &WindowConfig::toplevel(&title), ui).map_err(surface_err)?;
    let placeholder = Ui::new(
        PaintEnv::new(o.lang, theme).with_session(o.light),
        InputMode::Pointer,
        Size::new(1.0, 1.0),
        ultar_settings::i18n::translator(o.lang, None),
    )?;
    let ui = std::mem::replace(window.ui_mut(), placeholder);
    let mut app = App::new(
        ui,
        ports(&bus, documents, clipboard.clone()),
        o.area.min(AREAS.len() - 1),
    )?;
    if !o.light {
        app.apply_stored_theme();
    }
    std::mem::swap(window.ui_mut(), &mut app.ui);
    loop {
        let size = window.ui_mut().size();
        let actions = window.dispatch(Some(POLL)).map_err(surface_err)?;
        if window.window().close_requested() {
            return Ok(());
        }
        std::mem::swap(window.ui_mut(), &mut app.ui);
        let result = (|| {
            if app.ui.size() != size {
                app.resized()?;
            }
            app.actions(actions)?;
            app.poll()
        })();
        std::mem::swap(window.ui_mut(), &mut app.ui);
        result?;
        if let Some(text) = clipboard.0.borrow_mut().take() {
            window.offer_text(text).map_err(surface_err)?;
        }
        window.draw().map_err(surface_err)?;
    }
}

fn main() -> ExitCode {
    match parse(std::env::args().skip(1)).and_then(run) {
        Ok(()) => ExitCode::SUCCESS,
        Err(AppError::Usage) => ExitCode::from(2),
        Err(_) => ExitCode::FAILURE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_settings_uri_names_a_page() {
        assert_eq!(area_of_uri("settings://apps"), index_of("apps"));
        assert_eq!(area_of_uri("settings://apps/phone"), index_of("apps"));
        assert_eq!(area_of_uri("settings://nope"), None);
        assert_eq!(area_of_uri("files://apps"), None);
    }
}
