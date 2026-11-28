//! `ultar-wayland [--compositor PATH] [--runtime-dir DIR] [--app-id ID] -- program [args…]`

use ultar_wayland::msg::{Lang, Msg};

fn main() {
    let lang = Lang::from_env();
    let args = ultar_wayland::args::parse(
        std::env::args_os().skip(1).collect(),
        std::env::var_os("ULTAR_COMPOSITOR_SOCKET"),
        std::env::var("ULTAR_APP_ID").ok(),
    );
    let Some(args) = args else {
        eprintln!("{}", Msg::Usage.text(lang));
        std::process::exit(64);
    };
    std::process::exit(ultar_wayland::run::run(args, lang));
}
