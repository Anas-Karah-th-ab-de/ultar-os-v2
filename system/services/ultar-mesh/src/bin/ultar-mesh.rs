//! `ultar-mesh` – the device mesh (see the library documentation).
//!
//! ```text
//! ultar-mesh --bus PATH --state DIR --inbox DIR [--name TEXT] [--kind WORD] [--port N]
//!            [--announce N|off] [--peer ADDR,ADDR] [--opener APP]
//!            [--log-socket PATH] [--token-file PATH]
//! ```

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::net::{Ipv4Addr, SocketAddr};
use std::process::ExitCode;
use ultar_mesh::device::{default_capabilities, kind_from};
use ultar_mesh::net::PORT;
use ultar_mesh::node::Config;
use ultar_mesh::service::{list, Listen};
use ultar_mesh::MeshService;
use ultar_svckit::args::CommonArgs;
use ultar_svckit::msg::Msg;

const OPTIONS: [&str; 8] = [
    "state", "inbox", "name", "kind", "port", "announce", "peer", "opener",
];

fn listen(args: &CommonArgs) -> Result<Listen, Msg> {
    let port = match args.option("port") {
        Some(text) => text.parse().map_err(|_| Msg::BadArguments)?,
        None => PORT,
    };
    let announce = match args.option("announce") {
        Some("off") => None,
        Some(text) => Some(text.parse().map_err(|_| Msg::BadArguments)?),
        None => Some(PORT),
    };
    let mut peers = Vec::new();
    for peer in list(args.option("peer").unwrap_or("")) {
        peers.push(peer.parse().map_err(|_| Msg::BadArguments)?);
    }
    Ok(Listen {
        bind: SocketAddr::from((Ipv4Addr::UNSPECIFIED, port)),
        announce,
        peers,
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
            name: args.option("name").unwrap_or("device").to_owned(),
            kind,
            capabilities: default_capabilities(kind),
            apps: Vec::new(),
            inbox,
            remote_view: false,
        };
        MeshService::new(config, &state, listen(args)?).ok_or(Msg::State)
    })
}
