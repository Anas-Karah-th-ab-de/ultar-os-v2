//! Where the daemon's administrator rights and bus token come from, and
//! how the rights are read again after `SIGHUP` (review round 3, R3-P1,
//! R3-S2).

use crate::access::{AdminAccess, AllowAll, PolicyAccess, Reloadable};
use crate::runner::DaemonArgs;
use std::path::PathBuf;
use ultar_bus::Policy;
use ultar_service::log::{Level, Logger};
use ultar_service::{Lifecycle, Runtime, Secret, SecretError, SecretSource};

/// Largest bus token read from a file or descriptor.
const MAX_TOKEN_LEN: usize = 4096;

/// The rights from the policy file, or everybody with `--allow-all`.
pub fn load_access(policy: Option<&PathBuf>) -> Result<Box<dyn AdminAccess>, String> {
    match policy {
        None => Ok(Box::new(AllowAll)),
        Some(path) => Policy::load(path)
            .map(|policy| Box::new(PolicyAccess(policy)) as Box<dyn AdminAccess>)
            .map_err(|error| error.to_string()),
    }
}

/// Rights that `SIGHUP` reads again; a faulty file keeps the old ones.
pub fn reloadable_access(
    args: &DaemonArgs,
    lifecycle: &Lifecycle,
    logger: &Logger,
) -> Result<Reloadable, String> {
    let access = Reloadable::new(load_access(args.policy.as_ref())?);
    if let Some(path) = &args.policy {
        let (path, shared, logger) = (path.clone(), access.clone(), logger.clone());
        lifecycle.on_reload(move || match load_access(Some(&path)) {
            Ok(policy) => {
                shared.replace(policy);
                logger.log(Level::Notice, "devd.policy_reloaded");
            }
            Err(_) => logger.log(Level::Error, "devd.policy_reload_failed"),
        });
    }
    Ok(access)
}

/// Where the bus token comes from, if one is named.
pub fn token_source(args: &DaemonArgs) -> Option<SecretSource> {
    match (&args.token_file, args.token_fd) {
        (Some(path), _) => Some(SecretSource::File(path.clone())),
        (None, Some(fd)) => Some(SecretSource::Fd(fd)),
        (None, None) => None,
    }
}

/// Reads the bus token from a checked file or a declared inherited
/// descriptor.
pub fn load_token(args: &DaemonArgs, runtime: &Runtime) -> Result<Option<Secret>, SecretError> {
    token_source(args)
        .map(|source| runtime.read_secret(&source, MAX_TOKEN_LEN))
        .transpose()
}
