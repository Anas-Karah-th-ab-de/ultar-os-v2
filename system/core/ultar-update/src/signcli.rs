//! Command line of `uupdate-sign` (the logic is in [`crate::signtool`]).

use crate::args::Args;
use crate::error::UpdateError;
use crate::fsutil::{read_bounded, write_atomic};
use crate::keys::{load_burned, read_secret_file};
use crate::manifest::Manifest;
use crate::sign::{key_id, public_of};
use crate::signtool::{
    build_manifest, keygen, make_bundle, make_rotation, parse_payload_arg, read_public_keys,
    sign_manifest, verify_bundle, write_key_file, PayloadFile,
};
use crate::version::Version;
use std::io::Write;
use std::path::Path;
use ultar_i18n::{Arg, Translator};

const MAX_INPUT: u64 = 128 * 1024;

struct Tool<'a> {
    tr: &'a Translator,
    out: &'a mut dyn Write,
}

fn read_file(path: &str) -> Result<Vec<u8>, UpdateError> {
    read_bounded(Path::new(path), MAX_INPUT)?
        .ok_or_else(|| UpdateError::Io(std::io::ErrorKind::NotFound.into()))
}

fn number(args: &Args, name: &str) -> Result<u64, UpdateError> {
    args.required(name)?
        .parse()
        .map_err(|_| UpdateError::InvalidArgument)
}

fn payloads(args: &Args) -> Result<Vec<PayloadFile>, UpdateError> {
    args.all("--payload")
        .into_iter()
        .map(parse_payload_arg)
        .collect()
}

impl Tool<'_> {
    fn wrote(&mut self, path: &str) {
        let line = self
            .tr
            .t_args("update.sign.wrote", &[("path", Arg::Str(path))]);
        let _ = writeln!(self.out, "{line}");
    }

    fn key_line(&mut self, public: &[u8; 32]) {
        let id = hex::encode(key_id(public));
        let line = self
            .tr
            .t_args("update.sign.key_id", &[("id", Arg::Str(&id))]);
        let _ = writeln!(self.out, "{line}");
    }

    fn keygen(&mut self, args: &Args) -> Result<(), UpdateError> {
        args.only(&["--secret", "--public", "--lang"])?;
        let (secret_path, public_path) = (args.required("--secret")?, args.required("--public")?);
        let (secret, public) = keygen()?;
        write_key_file(Path::new(secret_path), &secret, true)?;
        write_key_file(Path::new(public_path), &public, false)?;
        self.wrote(secret_path);
        self.wrote(public_path);
        self.key_line(&public);
        Ok(())
    }

    fn manifest(&mut self, args: &Args) -> Result<(), UpdateError> {
        args.only(&[
            "--out",
            "--version",
            "--rollback-index",
            "--channel",
            "--edition",
            "--arch",
            "--devices",
            "--min-version",
            "--notes-key",
            "--created",
            "--expires",
            "--payload",
            "--lang",
        ])?;
        let version =
            |name| Version::parse(args.required(name)?).ok_or(UpdateError::InvalidArgument);
        let mut devices: Vec<String> = args
            .required("--devices")?
            .split(',')
            .map(str::to_string)
            .collect();
        devices.sort();
        devices.dedup();
        let spec = Manifest {
            version: version("--version")?,
            rollback_index: number(args, "--rollback-index")?,
            channel: args.required("--channel")?.to_string(),
            edition: args.required("--edition")?.to_string(),
            arch: args.required("--arch")?.to_string(),
            devices,
            min_version: version("--min-version")?,
            notes_key: args.required("--notes-key")?.to_string(),
            created: number(args, "--created")?,
            expires: number(args, "--expires")?,
            critical: args.switch("--critical"),
            payloads: Vec::new(),
        };
        let bytes = build_manifest(spec, &payloads(args)?)?;
        let out = args.required("--out")?;
        write_atomic(Path::new(out), &bytes)?;
        self.wrote(out);
        Ok(())
    }

    fn sign(&mut self, args: &Args) -> Result<(), UpdateError> {
        args.only(&["--manifest", "--key", "--out", "--lang"])?;
        let secret = read_secret_file(Path::new(args.required("--key")?))?;
        let signed = sign_manifest(&read_file(args.required("--manifest")?)?, &secret)?;
        let out = args.required("--out")?;
        write_atomic(Path::new(out), &signed)?;
        self.wrote(out);
        self.key_line(&public_of(&secret));
        Ok(())
    }

    fn rotate(&mut self, args: &Args) -> Result<(), UpdateError> {
        args.only(&[
            "--serial",
            "--created",
            "--key",
            "--out",
            "--public",
            "--lang",
        ])?;
        let secret = read_secret_file(Path::new(args.required("--key")?))?;
        let keys = read_public_keys(&args.all("--public"))?;
        let signed = make_rotation(
            number(args, "--serial")?,
            number(args, "--created")?,
            &keys,
            &secret,
        )?;
        let out = args.required("--out")?;
        write_atomic(Path::new(out), &signed)?;
        self.wrote(out);
        Ok(())
    }

    fn bundle(&mut self, args: &Args) -> Result<(), UpdateError> {
        args.only(&["--signed", "--out", "--rotation", "--payload", "--lang"])?;
        let signed = read_file(args.required("--signed")?)?;
        let mut rotations = Vec::new();
        for path in args.all("--rotation") {
            rotations.push(read_file(path)?);
        }
        let out = args.required("--out")?;
        make_bundle(&signed, &rotations, &payloads(args)?, Path::new(out))?;
        self.wrote(out);
        Ok(())
    }

    fn verify(&mut self, args: &Args) -> Result<(), UpdateError> {
        args.only(&["--bundle", "--keys", "--lang"])?;
        let keys = load_burned(Path::new(args.required("--keys")?))?.keys;
        let authentic = verify_bundle(Path::new(args.required("--bundle")?), &keys)?;
        let m = authentic.manifest();
        let version = m.version.to_string();
        let index = i64::try_from(m.rollback_index).unwrap_or(i64::MAX);
        let args = [("version", Arg::Str(&version)), ("index", Arg::Int(index))];
        let _ = writeln!(
            self.out,
            "{}",
            self.tr.t_args("update.sign.verified", &args)
        );
        Ok(())
    }
}

/// Runs `uupdate-sign` with `words`; returns the exit code (0 ok, 1 failed, 2 usage).
pub fn run(words: &[String], tr: &Translator, out: &mut dyn Write) -> u8 {
    let parsed = Args::parse(words, &["--critical"]);
    let mut tool = Tool { tr, out };
    let result = parsed.and_then(|args| {
        let command = args.positional.first().map(String::as_str);
        if args.positional.len() != 1 {
            return Err(UpdateError::InvalidArgument);
        }
        match command {
            Some("keygen") => tool.keygen(&args),
            Some("manifest") => tool.manifest(&args),
            Some("sign") => tool.sign(&args),
            Some("rotate") => tool.rotate(&args),
            Some("bundle") => tool.bundle(&args),
            Some("verify") => tool.verify(&args),
            _ => Err(UpdateError::InvalidArgument),
        }
    });
    match result {
        Ok(()) => 0,
        Err(UpdateError::InvalidArgument) => {
            let _ = writeln!(tool.out, "{}", tr.t("update.sign.usage"));
            2
        }
        Err(error) => {
            let detail = tr.t(error.key()).into_owned();
            let line = tr.t_args("update.sign.failed", &[("detail", Arg::Str(&detail))]);
            let _ = writeln!(tool.out, "{line}");
            1
        }
    }
}
