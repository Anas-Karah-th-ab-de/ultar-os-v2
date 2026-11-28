//! The whole browser window – tabs, address field, protection button, the page – over the Servo
//! engine, without a screen: the browser of `apps/ultar-web` with `RemoteEngine`, a page typed into
//! the address field, then the window as PNG in Arabic (dark) and English (light). For builders
//! and for looking at the real thing next to the boards.
//!
//! `window <ultar-web-engine> <proxy> <address> <out-prefix> [seconds]`

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use ultar_ui::paint::PaintEnv;
use ultar_ui::tokens::{InputMode, ThemeKind};
use ultar_ui::{Direction, Lang, Size};
use ultar_web::app::{Browser, Handoff, HandoffError};
use ultar_web::engine::remote::{HostConfig, RemoteEngine};

type Failure = Box<dyn std::error::Error>;

/// No Files app here.
struct NoFiles;

impl Handoff for NoFiles {
    fn show_in_folder(&mut self, _file: &Path) -> Result<(), HandoffError> {
        Err(HandoffError::Missing)
    }
}

fn main() -> Result<(), Failure> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [program, proxy, address, prefix, rest @ ..] = args.as_slice() else {
        return Err("usage: window <engine> <proxy> <address> <out-prefix> [seconds]".into());
    };
    let seconds: u64 = rest.first().map_or(Ok(30), |s| s.parse())?;
    for (lang, theme, name) in [
        (Lang::Ar, ThemeKind::Dark, "ar-dark"),
        (Lang::En, ThemeKind::Light, "en-light"),
    ] {
        let config = HostConfig {
            program: PathBuf::from(program),
            cell: None,
            profile: std::env::temp_dir().join(format!("ultar-window-{}", std::process::id())),
            downloads: std::env::temp_dir(),
            user_agent: "UltarWeb/0.8".into(),
            filter: None,
            proxy: Some(proxy.clone()),
        };
        std::fs::create_dir_all(&config.profile)?;
        let mut browser = Browser::new(
            RemoteEngine::start(&config)?,
            window_ui(lang, theme)?,
            None,
            Box::new(NoFiles),
        )?;
        let field = browser.chrome().address;
        browser.open_typed(address, field)?;
        let until = Instant::now() + Duration::from_secs(seconds);
        while Instant::now() < until {
            browser.pump(Some(Duration::from_millis(200)))?;
        }
        let canvas = ultar_ui::snapshot::render(browser.ui_mut())?;
        let png = ultar_ui::snapshot::Image::from_canvas(&canvas).encode_png()?;
        let out = format!("{prefix}-{name}.png");
        std::fs::write(&out, png)?;
        println!("saved {out}");
    }
    Ok(())
}

/// The window's interface at 1280 × 800 in `lang` and `theme`.
fn window_ui(lang: Lang, theme: ThemeKind) -> Result<ultar_ui::ui::Ui, Failure> {
    let tr = ultar_web::catalog::translator(lang)?;
    let size = Size::new(1280.0, 800.0);
    let mut ui = ultar_ui::ui::Ui::new(PaintEnv::new(lang, theme), InputMode::Pointer, size, tr)?;
    ui.set_direction(if lang == Lang::Ar {
        Direction::Rtl
    } else {
        Direction::Ltr
    });
    Ok(ui)
}
