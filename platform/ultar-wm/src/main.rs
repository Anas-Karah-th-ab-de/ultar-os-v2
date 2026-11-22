static POSSIBLE_BACKENDS: &[&str] = &[
    #[cfg(feature = "winit")]
    "--winit : als Fenster in einer bestehenden Sitzung (Entwicklung)",
    #[cfg(feature = "udev")]
    "--drm (--tty-udev) : direkt auf dem Bildschirm, GPU über DRM/KMS (als root mit LIBSEAT_BACKEND=builtin)",
    #[cfg(feature = "fb")]
    "--fb : ohne DRM direkt in den Bildspeicher (fbdev, Samsung DECON), Darstellung mit Pixman",
    #[cfg(feature = "x11")]
    "--x11 : als X11-Fenster",
];

#[cfg(feature = "profile-with-tracy-mem")]
#[global_allocator]
static GLOBAL: profiling::tracy_client::ProfiledAllocator<std::alloc::System> =
    profiling::tracy_client::ProfiledAllocator::new(std::alloc::System, 10);

fn main() {
    if let Ok(env_filter) = tracing_subscriber::EnvFilter::try_from_default_env() {
        tracing_subscriber::fmt()
            .compact()
            .with_env_filter(env_filter)
            .init();
    } else {
        tracing_subscriber::fmt().compact().init();
    }

    #[cfg(feature = "profile-with-tracy")]
    profiling::tracy_client::Client::start();

    profiling::register_thread!("Main Thread");

    #[cfg(feature = "profile-with-puffin")]
    let _server = puffin_http::Server::new(&format!("0.0.0.0:{}", puffin_http::DEFAULT_PORT)).unwrap();
    #[cfg(feature = "profile-with-puffin")]
    profiling::puffin::set_scopes_on(true);

    let arg = ::std::env::args().nth(1);
    // Ohne Angabe: in einer bestehenden Sitzung als Fenster, sonst direkt auf dem Bildschirm (DRM/KMS)
    let arg = arg.or_else(|| {
        if std::env::var_os("WAYLAND_DISPLAY").is_some() || std::env::var_os("DISPLAY").is_some() {
            Some("--winit".into())
        } else if std::fs::read_dir("/dev/dri").map(|mut d| d.any(|e| e.map(|e| e.file_name().to_string_lossy().starts_with("card")).unwrap_or(false))).unwrap_or(false) {
            Some("--tty-udev".into())
        } else {
            // kein DRM-Treiber (z. B. Galaxy A30): Bildspeicher-Weg
            Some("--fb".into())
        }
    });
    match arg.as_ref().map(|s| &s[..]) {
        #[cfg(feature = "winit")]
        Some("--winit") => {
            tracing::info!("Ultar-Fenstersystem startet im Fenster (winit)");
            ultar_wm::winit::run_winit();
        }
        #[cfg(feature = "udev")]
        Some("--tty-udev") | Some("--drm") => {
            tracing::info!("Ultar-Fenstersystem startet auf dem Bildschirm (DRM/KMS, udev, libinput)");
            ultar_wm::udev::run_udev();
        }
        #[cfg(feature = "fb")]
        Some("--fb") => {
            tracing::info!("Ultar-Fenstersystem startet auf dem Bildspeicher (fbdev/DECON, Pixman)");
            ultar_wm::fb::run_fb();
        }
        #[cfg(feature = "x11")]
        Some("--x11") => {
            tracing::info!("Ultar-Fenstersystem startet als X11-Fenster");
            ultar_wm::x11::run_x11();
        }
        Some("--version") => {
            #[allow(clippy::disallowed_macros)]
            {
                println!("ultar-wm {} – KM Code Craft", env!("CARGO_PKG_VERSION"));
            }
        }
        _ => {
            #[allow(clippy::disallowed_macros)]
            {
                println!("ultar-wm {} – Ultar-Fenstersystem (Wayland-Compositor) von KM Code Craft", env!("CARGO_PKG_VERSION"));
                println!();
                println!("Aufruf: ultar-wm [--drm | --fb | --winit | --x11 | --version]");
                for b in POSSIBLE_BACKENDS {
                    println!("\t{}", b);
                }
                println!();
                println!("Tasten: Super+Leertaste Startmenü · Alt+Tab Fenster wechseln · Super+Pfeile andocken/maximieren/minimieren");
                println!("        Super+Enter Terminal · Alt+F4 / Super+Q schließen · Strg+Alt+Rücktaste Sitzung beenden");
            }
        }
    }
}
