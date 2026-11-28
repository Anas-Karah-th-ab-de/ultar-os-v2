//! From an installed app to the plan of its cell: which namespaces, which
//! view, which limits, which user – each decided by the platform, the
//! profile and the grants, never by the app.

use crate::appid::AppId;
use crate::cell::{view, CellLimits, CellPlan, DevMode, ForeignBinds};
use crate::datadir;
use crate::error::{LaunchError, PackageError};
use crate::launcher::Core;
use crate::manifest::{Manifest, Restart};
use crate::perm::Permission;
use crate::platform::Feature;
use crate::store::Entry;
use std::collections::BTreeMap;
use ultar_bus::messages::launcher::Role;
use ultar_init::groups::GroupTree;
use ultar_init::spec::Slice;
use ultar_sys::cell::Namespaces;

/// Environment of the app (nothing of the launcher's own).
pub const PATH: &str = "/usr/bin:/bin";
/// The apps' variable naming the bus socket (only set for apps that declare bus access).
pub const BUS_ENV: &str = "ULTAR_BUS_SOCKET";
/// The apps' variable naming where the file areas are (only set for apps that have some).
pub const AREAS_ENV: &str = "ULTAR_AREAS";
/// The apps' variable naming their publish directory (only set for apps that have one).
pub const PUBLISH_ENV: &str = "ULTAR_PUBLISH";
/// File name of the apps' bus socket inside `app_bus`.
pub const BUS_SOCKET: &str = "bus.sock";
/// The compositor's variable for the activation token.
pub const ACTIVATION_ENV: &str = "ULTAR_ACTIVATION_TOKEN";

/// What a start needs.
pub(crate) struct Prepared {
    pub(crate) plan: CellPlan,
    pub(crate) env: Vec<(String, String)>,
    pub(crate) restart: Option<Restart>,
}

fn group(
    core: &Core,
    id: &AppId,
    role: Role,
    memory: u64,
    pids: u32,
) -> Result<Option<std::path::PathBuf>, LaunchError> {
    let wanted = core
        .platform
        .require(Feature::Groups, &core.config.accept_missing);
    if !wanted.map_err(LaunchError::Platform)? {
        return Ok(None);
    }
    let Some(root) = &core.config.paths.group_root else {
        return Err(LaunchError::Platform(Feature::Groups));
    };
    let limits = BTreeMap::from([
        ("memory.max".to_owned(), memory.to_string()),
        ("pids.max".to_owned(), pids.to_string()),
    ]);
    // a tree without the limit files (Linux without the controllers) is
    // refused by `prepare`: the app does not start unlimited
    // the limits hold for the app as a whole; each role gets a group of its
    // own below, so the hard end of one role never ends the other (R2-3)
    match GroupTree::detect(root).prepare(Slice::Apps, id.as_str(), &limits) {
        Ok(Some(prepared)) => {
            let dir = prepared.dir.join(role.word());
            match std::fs::create_dir(&dir) {
                Err(e) if e.kind() != std::io::ErrorKind::AlreadyExists => {
                    Err(LaunchError::Platform(Feature::Groups))
                }
                _ => Ok(Some(dir)),
            }
        }
        _ => Err(LaunchError::Platform(Feature::Groups)),
    }
}

fn program(
    manifest: &Manifest,
    role: Role,
    entry: &Entry,
) -> Result<(crate::manifest::Program, Option<Restart>), LaunchError> {
    match role {
        Role::Ui => Ok((manifest.ui.clone(), None)),
        Role::Service => {
            let (program, restart) = manifest.service.clone().ok_or(LaunchError::NotDeclared)?;
            if !entry.granted.has(Permission::Background) {
                return Err(LaunchError::NotGranted(Permission::Background));
            }
            Ok((program, Some(restart)))
        }
    }
}

/// The app's environment; `host_data`: its data directory when it runs
/// without a cell (else it sees `/data` and `/tmp`).
fn environment(
    id: &AppId,
    role: Role,
    host_data: Option<&std::path::PathBuf>,
    opening: &crate::open::Opening,
) -> Vec<(String, String)> {
    let (home, tmp) = match host_data {
        Some(data) => (
            data.to_string_lossy().into_owned(),
            std::env::temp_dir().to_string_lossy().into_owned(),
        ),
        None => (view::DATA.to_owned(), view::TMP.to_owned()),
    };
    let mut env = vec![
        ("PATH".to_owned(), PATH.to_owned()),
        ("HOME".to_owned(), home),
        ("TMPDIR".to_owned(), tmp),
        ("ULTAR_APP_ID".to_owned(), id.to_string()),
    ];
    if let Some(token) = opening.token.filter(|t| *t != 0 && role == Role::Ui) {
        env.push((ACTIVATION_ENV.to_owned(), token.to_string()));
    }
    // checked before (`open::check`): data for the app, nothing else
    if let Some(uri) = &opening.uri {
        env.push((crate::open::OPEN_ENV.to_owned(), uri.clone()));
    }
    env
}

/// Where the cell's device nodes come from.
fn dev_mode(cell: bool, platform: &crate::platform::Platform) -> DevMode {
    match (cell, platform.file_binds, platform.ultar) {
        (false, _, _) => DevMode::None,
        (true, true, _) => DevMode::Binds,
        (true, false, true) => DevMode::DevTmpfs,
        (true, false, false) => DevMode::None,
    }
}

/// The directory the app may publish to for the system: only a system app the operator
/// names in `publishers`, only in a cell; `None` when it cannot be made (nothing published).
fn publish_dir(
    core: &Core,
    id: &AppId,
    entry: &Entry,
    owner: Option<datadir::Owner>,
    cell: bool,
) -> Option<std::path::PathBuf> {
    let root = core.config.paths.publish_root.as_ref()?;
    let named = core.config.publishers.iter().any(|p| p == id.as_str());
    let system = entry.profile == crate::profile::ProfileName::System;
    if !(cell && named && system) {
        return None;
    }
    datadir::ensure_publish_dir(root, id.as_str(), owner).ok()
}

/// The network of a start: `(own network namespace, "internet only" barrier)`.
///
/// * Without the `network` permission: a namespace of its own – only its own loopback.
/// * With it, in the system profile (KM Code Craft's own apps, e.g. the browser that must open a router's
///   page): the host's network as before.
/// * With it, in any other profile: a namespace of its own with the barrier – the internet, but no host
///   loopback, no address of the device, no LAN, broadcast or multicast (`docs/integration/zell-netz.md`).
///   A kernel without the barrier stops the start unless the operator accepted `net-guard`.
fn network(
    platform: &crate::platform::Platform,
    accepted: &[Feature],
    entry: &Entry,
) -> Result<(bool, bool), LaunchError> {
    if !entry.granted.has(Permission::Network) {
        let own = platform
            .require(Feature::Net, accepted)
            .map_err(LaunchError::Platform)?;
        return Ok((own, false));
    }
    if entry.profile == crate::profile::ProfileName::System {
        return Ok((false, false));
    }
    let own = platform
        .require(Feature::Net, accepted)
        .map_err(LaunchError::Platform)?;
    let guard = platform
        .require(Feature::NetGuard, accepted)
        .map_err(LaunchError::Platform)?;
    Ok((own && guard, own && guard))
}

/// An app from another system (`foreign.rs`): every isolation feature is required –
/// `accept_missing` never counts. The network is a namespace of its own: empty without the
/// permission, with it behind the kernel's "internet only" barrier (`network` above, but
/// strictly). Returns `(own network namespace, barrier)`.
///
/// The barrier has no mode for the local network yet: an app the person allowed the local
/// network still gets the internet only (never more than granted; `docs/integration/ultar-foreign.md`).
fn foreign_isolation(
    platform: &crate::platform::Platform,
    entry: &Entry,
) -> Result<(bool, bool), LaunchError> {
    for feature in [
        Feature::Ids,
        Feature::Cell,
        Feature::Groups,
        Feature::Net,
        Feature::DeviceBinds,
    ] {
        platform
            .require_strictly(feature)
            .map_err(LaunchError::Platform)?;
    }
    if entry.granted.has(Permission::Network) {
        platform
            .require_strictly(Feature::NetGuard)
            .map_err(LaunchError::Platform)?;
        Ok((true, true))
    } else {
        Ok((true, false))
    }
}

/// What differs in the cell of an app from another system: the Linux area instead of
/// the system's view, the bridge, its environment, never the bus.
fn foreign_cell(
    core: &Core,
    id: &AppId,
    plan: &mut CellPlan,
    env: &mut Vec<(String, String)>,
) -> Result<(), LaunchError> {
    let runtime = core
        .config
        .paths
        .foreign
        .runtime
        .clone()
        .filter(|r| r.join("usr").is_dir())
        .ok_or(LaunchError::StartFailed)?;
    let bridge = core
        .config
        .paths
        .foreign
        .bridge
        .clone()
        .filter(|b| b.join(crate::foreign::BRIDGE_PROGRAM).is_file());
    plan.view.clear();
    plan.foreign.runtime = Some(runtime);
    plan.program = crate::foreign::bridged(&plan.program, bridge.is_some());
    plan.foreign.bridge = bridge;
    plan.foreign.setup = core
        .config
        .paths
        .foreign
        .setup
        .as_ref()
        .and_then(|root| crate::foreign::setup_dir(root, id));
    plan.publish = None;
    // only the compositor's directory of the shared ones; the apps' bus never
    if let Some(app_bus) = &core.config.paths.app_bus {
        plan.shares.retain(|dir| dir != app_bus);
    }
    let lang = core
        .look
        .env()
        .into_iter()
        .find(|(k, _)| k == "ULTAR_LANG")
        .map_or_else(|| "ar".to_owned(), |(_, v)| v);
    env.retain(|(k, _)| k != "PATH");
    env.extend(crate::foreign::environment(&lang));
    Ok(())
}

/// The plan for `role` of the installed app `id`.
pub(crate) fn prepare(
    core: &Core,
    id: &AppId,
    role: Role,
    opening: &crate::open::Opening,
) -> Result<Prepared, LaunchError> {
    let entry = core.installed(id)?.clone();
    let manifest = core.manifest(id)?;
    let (mut program, restart) = program(&manifest, role, &entry)?;
    // `[open] page_arg`: the page of the URI as the app's own argument (checked words only).
    if let (Role::Ui, Some(arg), Some(page)) = (
        role,
        manifest.open_page_arg.as_ref(),
        opening.uri.as_deref().and_then(crate::open::page),
    ) {
        program.args.push(arg.clone());
        program.args.push(page.to_owned());
    }
    let (platform, accepted) = (&core.platform, &core.config.accept_missing);
    let foreign = entry.profile == crate::profile::ProfileName::Foreign;
    if foreign && role != Role::Ui {
        return Err(LaunchError::NotDeclared);
    }
    let foreign_net = if foreign {
        Some(foreign_isolation(platform, &entry)?)
    } else {
        None
    };
    let cell = platform
        .require(Feature::Cell, accepted)
        .map_err(LaunchError::Platform)?;
    let (isolate_net, net_guard) = match foreign_net {
        Some(both) => both,
        None => network(platform, accepted, &entry)?,
    };
    let profile = entry.profile.profile();
    let limits = profile
        .limits(&manifest.limits)
        .map_err(|e| LaunchError::Package(PackageError::Profile(e)))?;
    let owner = core.owner(entry.uid)?;
    let data = datadir::ensure_app_dir(&core.config.paths.data_root, id.as_str(), owner)
        .map_err(|_| LaunchError::StartFailed)?;
    let root = core
        .config
        .paths
        .cell_root
        .join(format!("{id}-{}", role.word()));
    datadir::ensure_private_dir(&root).map_err(|_| LaunchError::StartFailed)?;
    let group_dir = group(core, id, role, limits.memory_bytes, limits.pids)?;
    let dev = dev_mode(cell, platform);
    let keep = manifest.kernel_rights & profile.kernel_rights;
    // the apps' bus socket only for an app whose signed manifest declares bus access (the broker
    // knows it by its user id from the launcher's app map; nobody else is let in there)
    let mut shares = core.config.shares.clone();
    let bus = core
        .declares_bus(&manifest)
        .then_some(())
        .and(core.config.paths.app_bus.clone());
    if let Some(dir) = &bus {
        shares.push(dir.clone());
    }
    // the user's file areas: system apps only, each one the manifest declares (areas.rs)
    let (mut areas, mut groups) = (Vec::new(), Vec::new());
    // system apps declare their areas; an app from another system gets the folders the
    // person chose for it in the system dialog (written into its manifest by the service)
    let may_have_areas = matches!(
        entry.profile,
        crate::profile::ProfileName::System | crate::profile::ProfileName::Foreign
    );
    if let (true, Some(root), Some(group)) = (
        may_have_areas && cell,
        &core.config.paths.areas_root,
        core.config.areas_group,
    ) {
        for (name, rw) in crate::areas::declared(&manifest.files) {
            areas.push((crate::areas::host(root, &name), name, rw));
        }
        if !areas.is_empty() {
            groups.push(group);
        }
    }
    let publish = publish_dir(core, id, &entry, owner, cell);
    let mut plan = CellPlan {
        identity: owner.map(|o| (o.uid, o.gid)),
        group_dir,
        namespaces: Namespaces {
            mount: cell,
            pid: cell,
            net: isolate_net,
            ipc: cell && platform.ipc,
        },
        net_guard,
        root,
        data: data.clone(),
        view: core.config.system_view.clone(),
        shares,
        areas: areas.clone(),
        publish: publish.clone(),
        groups,
        dev,
        limits: CellLimits {
            tmp_bytes: limits.tmp_bytes,
            drop_rights: platform.ultar.then_some(!keep),
            nproc: Some(u64::from(limits.pids)),
        },
        foreign: ForeignBinds::default(),
        program,
    };
    let mut env = environment(id, role, if cell { None } else { Some(&data) }, opening);
    // the person's language, theme, accent and text size
    env.extend(core.look.env());
    if let Some(dir) = bus {
        env.push((
            BUS_ENV.to_owned(),
            dir.join(BUS_SOCKET).to_string_lossy().into_owned(),
        ));
    }
    if !areas.is_empty() {
        env.push((AREAS_ENV.to_owned(), view::AREAS.to_owned()));
    }
    if publish.is_some() {
        env.push((PUBLISH_ENV.to_owned(), view::PUBLISH.to_owned()));
    }
    if foreign {
        foreign_cell(core, id, &mut plan, &mut env)?;
    }
    Ok(Prepared { plan, env, restart })
}
