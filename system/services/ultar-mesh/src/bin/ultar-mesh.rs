//! `ultar-mesh` – the device mesh (see the library documentation).
//!
//! ```text
//! ultar-mesh --bus PATH --state DIR --inbox DIR [--name TEXT] [--kind WORD] [--port N]
//!            [--announce N|off] [--peer ADDR,ADDR] [--opener APP] [--share DIR,DIR]
//!            [--jobs DIR] [--relay URL --relay-token-file PATH]
//!            [--log-socket PATH] [--token-file PATH]
//! ```

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::net::{Ipv4Addr, SocketAddr};
use std::process::ExitCode;
use ultar_mesh::device::{default_capabilities, kind_from};
use ultar_mesh::net::{Announcing, PORT};
use ultar_mesh::node::Config;
use ultar_mesh::relay::Login;
use ultar_mesh::service::{list, Listen};
use ultar_mesh::MeshService;
use ultar_svckit::args::CommonArgs;
use ultar_svckit::msg::Msg;

const OPTIONS: [&str; 12] = [
    "state",
    "inbox",
    "name",
    "kind",
    "port",
    "announce",
    "peer",
    "opener",
    "share",
    "jobs",
    "relay",
    "relay-token-file",
];

/// The name other devices see: the one given, else the machine's own name when the
/// service manager passes it on (`HOSTNAME`).
fn name(args: &CommonArgs) -> String {
    let own = || std::env::var("HOSTNAME").ok();
    let name = args.option("name").map(str::to_owned).or_else(own);
    match name.as_deref().map(str::trim) {
        Some(name) if !name.is_empty() => name.to_owned(),
        _ => "device".to_owned(),
    }
}

fn listen(args: &CommonArgs) -> Result<Listen, Msg> {
    let port = match args.option("port") {
        Some(text) => text.parse().map_err(|_| Msg::BadArguments)?,
        None => PORT,
    };
    let announce = match args.option("announce") {
        Some("off") => None,
        Some(text) => Some(Announcing::broadcast(
            text.parse().map_err(|_| Msg::BadArguments)?,
        )),
        None => Some(Announcing::broadcast(PORT)),
    };
    let mut peers = Vec::new();
    for peer in list(args.option("peer").unwrap_or("")) {
        peers.push(peer.parse().map_err(|_| Msg::BadArguments)?);
    }
    // The relay needs the account's device token; without both there is no relay.
    let token = match args
        .path("relay-token-file")
        .map_err(|_| Msg::BadArguments)?
    {
        Some(file) => std::fs::read_to_string(file).ok(),
        None => None,
    };
    let relay = args.option("relay").zip(token).map(|(url, token)| Login {
        url: url.to_owned(),
        token: token.trim().to_owned(),
        name: name(args),
        kind: args.option("kind").unwrap_or("desktop").to_owned(),
    });
    Ok(Listen {
        bind: SocketAddr::from((Ipv4Addr::UNSPECIFIED, port)),
        announce,
        peers,
        relay,
        jobs: args.path("jobs").map_err(|_| Msg::BadArguments)?,
        opener: args.option("opener").unwrap_or("files").to_owned(),
    })
}

fn main() -> ExitCode {
    // No kernel right: sockets above port 1024 and its own two folders.
    ultar_svckit::main("ultar-mesh", &OPTIONS, None, |args, _| {
        let state = args.required_path("state").map_err(|_| Msg::BadArguments)?;
        let inbox = args.required_path("inbox").map_err(|_| Msg::BadArguments)?;
        let kind = kind_from(args.option("kind").unwrap_or("desktop"));
        let config = Config {
            name: name(args),
            kind,
            capabilities: default_capabilities(kind),
            apps: Vec::new(),
            inbox,
            share: list(args.option("share").unwrap_or(""))
                .into_iter()
                .map(Into::into)
                .collect(),
            remote_view: false,
        };
        MeshService::new(config, &state, listen(args)?).ok_or(Msg::State)
    })
}
