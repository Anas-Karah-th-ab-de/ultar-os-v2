//! `ultar-signd` command line: the service (`serve`) and the client commands of the publish
//! scripts. See the crate documentation and `deploy/SIGNING.md`.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::process::ExitCode;
use ultar_signd::audit::AuditLog;
use ultar_signd::keys::{self, Role};
use ultar_signd::msg::{text, Lang, Msg};
use ultar_signd::ratelimit::Limit;
use ultar_signd::server::{bind, run, Signer};
use ultar_signd::{client, sshsig, SignError};

fn role(word: &str) -> Result<Role, SignError> {
    Role::parse(word).ok_or(SignError::BadRequest)
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn dispatch(args: &[String]) -> Result<Option<String>, SignError> {
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    match a.as_slice() {
        ["serve", socket, key_dir, audit] => {
            if rustix::process::geteuid().is_root() {
                return Err(SignError::RunningAsRoot);
            }
            let signer = Signer::load(
                Path::new(key_dir),
                AuditLog::new(*audit),
                Role::ALL.map(Limit::default_for),
                now(),
            )?;
            run(bind(Path::new(socket))?, signer)?;
            Ok(None)
        }
        ["keygen", key_dir, r] => {
            let public = keys::generate(Path::new(key_dir), role(r)?)?;
            Ok(Some(hex::encode(public.as_bytes())))
        }
        ["sign-uap", socket, r, manifest, payload, out] => client::sign_uap(
            Path::new(socket),
            role(r)?,
            Path::new(manifest),
            Path::new(payload),
            Path::new(out),
        )
        .map(Some),
        ["sign-catalog", socket, file] => client::sign_catalog(Path::new(socket), Path::new(file))
            .map(|p| Some(p.display().to_string())),
        ["sign-release", socket, file] => client::sign_release(Path::new(socket), Path::new(file))
            .map(|p| Some(p.display().to_string())),
        ["pubkey", socket, r] => client::public_key(Path::new(socket), role(r)?).map(Some),
        ["ssh-signers", socket] => {
            let hex_key = client::public_key(Path::new(socket), Role::Os)?;
            let bytes: [u8; 32] = hex::decode(hex_key)
                .map_err(|_| SignError::Protocol)?
                .try_into()
                .map_err(|_| SignError::Protocol)?;
            let key =
                ed25519_dalek::VerifyingKey::from_bytes(&bytes).map_err(|_| SignError::Protocol)?;
            Ok(Some(sshsig::allowed_signers_line(
                &key,
                ultar_signd::request::RELEASE_NAMESPACE,
            )))
        }
        _ => Err(SignError::BadRequest),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let lang = Lang::from_env();
    match dispatch(&args) {
        Ok(Some(out)) => {
            println!("{out}");
            ExitCode::SUCCESS
        }
        Ok(None) => ExitCode::SUCCESS,
        Err(SignError::BadRequest) => {
            eprintln!("{}", text(Msg::Usage, lang));
            ExitCode::from(2)
        }
        Err(e) => {
            eprintln!("{} {e}", text(Msg::Failed, lang));
            ExitCode::FAILURE
        }
    }
}
