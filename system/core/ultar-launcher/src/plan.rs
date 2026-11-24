//! From an installed app to the plan of its cell: which namespaces, which
//! view, which limits, which user – each decided by the platform, the
//! profile and the grants, never by the app.

use crate::appid::AppId;
use crate::cell::{view, CellPlan, DevMode};
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
    match GroupTree::detect(root).prepare(Slice::Apps, id.as_str(), &limits) {
        Ok(Some(prepared)) => Ok(Some(prepared.dir)),
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

/// The plan for `role` of the installed app `id`.
pub(crate) fn prepare(
    core: &Core,
    id: &AppId,
    role: Role,
    opening: &crate::open::Opening,
) -> Result<Prepared, LaunchError> {
    let entry = core.installed(id)?.clone();
    let manifest = core.manifest(id)?;
    let (program, restart) = program(&manifest, role, &entry)?;
    let (platform, accepted) = (&core.platform, &core.config.accept_missing);
    let cell = platform
        .require(Feature::Cell, accepted)
        .map_err(LaunchError::Platform)?;
    let isolate_net = if entry.granted.has(Permission::Network) {
        false
    } else {
        platform
            .require(Feature::Net, accepted)
            .map_err(LaunchError::Platform)?
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
    let group_dir = group(core, id, limits.memory_bytes, limits.pids)?;
    let dev = dev_mode(cell, platform);
    let keep = manifest.kernel_rights & profile.kernel_rights;
    let plan = CellPlan {
        identity: owner.map(|o| (o.uid, o.gid)),
        group_dir,
        namespaces: Namespaces {
            mount: cell,
            pid: cell,
            net: isolate_net,
            ipc: cell && platform.ipc,
        },
        root,
        data: data.clone(),
        view: core.config.system_view.clone(),
        shares: core.config.shares.clone(),
        dev,
        tmp_bytes: limits.tmp_bytes,
        drop_rights: platform.ultar.then_some(!keep),
        nproc: Some(u64::from(limits.pids)),
        program,
    };
    let env = environment(id, role, if cell { None } else { Some(&data) }, opening);
    Ok(Prepared { plan, env, restart })
}
