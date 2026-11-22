//! The `udrv` command line tool. All output is looked up through [`crate::msg`].

use crate::depot::{resolve, write_signed_index, Plan};
use crate::error::{Coded, Error};
use crate::hwlist::parse_hardware_list;
use crate::index::{DriverIndex, INDEX_SIGNATURE_NAME};
use crate::keys::{PrivateKey, TrustedKeys};
use crate::msg::{text, Lang};
use crate::package::{self, Limits};
use crate::payload::build_payload;
use std::fmt;
use std::io::Write;
use std::path::Path;

/// Kernel ABI assumed by `udrv match` when `--abi` is not given.
pub const DEFAULT_KERNEL_ABI: &str = "ultar-kern-abi-1";

/// Why a `udrv` invocation failed.
#[derive(Debug)]
pub enum CliError {
    /// Wrong number or form of arguments.
    Usage,
    /// The first argument is not a known command.
    UnknownCommand(String),
    /// A file could not be read or written.
    Io {
        /// The file involved.
        path: String,
        /// The kind of failure.
        kind: std::io::ErrorKind,
    },
    /// Writing the result to the output stream failed.
    Output,
    /// The library rejected the input.
    Rejected(Error),
}

impl Coded for CliError {
    fn code(&self) -> &'static str {
        match self {
            Self::Usage => "driverpkg.cli.usage",
            Self::UnknownCommand(_) => "driverpkg.cli.unknown-command",
            Self::Io { .. } => "driverpkg.cli.io",
            Self::Output => "driverpkg.cli.output",
            Self::Rejected(e) => e.code(),
        }
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage | Self::Output => f.write_str(self.code()),
            Self::UnknownCommand(name) => write!(f, "{}: {name}", self.code()),
            Self::Io { path, kind } => write!(f, "{}: {path}: {kind:?}", self.code()),
            Self::Rejected(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for CliError {}

impl<E: Into<Error>> From<E> for CliError {
    fn from(e: E) -> Self {
        Self::Rejected(e.into())
    }
}

/// Translated output: `<text>` or `<text>: <value>`.
struct Printer<'a> {
    lang: Lang,
    out: &'a mut dyn Write,
}

impl Printer<'_> {
    fn say(&mut self, key: &str, value: &str) -> Result<(), CliError> {
        let label = text(self.lang, key).unwrap_or(key);
        let result = if value.is_empty() {
            writeln!(self.out, "{label}")
        } else {
            writeln!(self.out, "{label}: {value}")
        };
        result.map_err(|_| CliError::Output)
    }

    fn raw(&mut self, line: &str) -> Result<(), CliError> {
        writeln!(self.out, "{line}").map_err(|_| CliError::Output)
    }
}

fn read(path: &str) -> Result<Vec<u8>, CliError> {
    std::fs::read(path).map_err(|e| CliError::Io {
        path: path.to_string(),
        kind: e.kind(),
    })
}

fn read_text(path: &str) -> Result<String, CliError> {
    String::from_utf8(read(path)?).map_err(|_| CliError::Io {
        path: path.to_string(),
        kind: std::io::ErrorKind::InvalidData,
    })
}

/// The private key argument is a file holding the hexadecimal key, or the hexadecimal key itself.
fn private_key(arg: &str) -> Result<PrivateKey, CliError> {
    let hex_text = if Path::new(arg).is_file() {
        read_text(arg)?
    } else {
        arg.to_string()
    };
    Ok(PrivateKey::from_hex(&hex_text)?)
}

fn trusted_keys(path: &str) -> Result<TrustedKeys, CliError> {
    Ok(TrustedKeys::parse(&read_text(path)?)?)
}

/// `--name value` options of an invocation.
type Options<'a> = Vec<(&'a str, &'a str)>;

/// Splits `--name value` options from positional arguments.
fn split_options<'a>(
    args: &'a [String],
    known: &[&str],
) -> Result<(Vec<&'a str>, Options<'a>), CliError> {
    let (mut positional, mut options) = (Vec::new(), Vec::new());
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.strip_prefix("--") {
            Some(name) if known.contains(&name) => {
                options.push((name, iter.next().ok_or(CliError::Usage)?.as_str()))
            }
            Some(_) => return Err(CliError::Usage),
            None => positional.push(arg.as_str()),
        }
    }
    Ok((positional, options))
}

fn option<'a>(options: &[(&str, &'a str)], name: &str) -> Option<&'a str> {
    options.iter().find(|(n, _)| *n == name).map(|(_, v)| *v)
}

fn keygen(p: &mut Printer<'_>) -> Result<(), CliError> {
    let key = PrivateKey::generate();
    p.say("driverpkg.cli.keygen.private", &key.to_hex())?;
    p.say("driverpkg.cli.keygen.public", &key.public().to_hex())?;
    p.say("driverpkg.cli.key-id", &key.public().key_id())
}

fn pack(p: &mut Printer<'_>, args: &[&str]) -> Result<(), CliError> {
    let [manifest, payload_dir, out, key] = args else {
        return Err(CliError::Usage);
    };
    let key = private_key(key)?;
    let payload = build_payload(Path::new(payload_dir))?;
    let bytes = package::pack(&read(manifest)?, &payload, &key, &Limits::default())?;
    std::fs::write(out, bytes).map_err(|e| CliError::Io {
        path: (*out).to_string(),
        kind: e.kind(),
    })?;
    p.say("driverpkg.cli.pack.done", out)?;
    p.say("driverpkg.cli.key-id", &key.public().key_id())
}

fn print_manifest(
    p: &mut Printer<'_>,
    manifest: &crate::manifest::Manifest,
) -> Result<(), CliError> {
    p.say("driverpkg.cli.info.id", &manifest.id)?;
    p.say("driverpkg.cli.info.version", &manifest.version.to_string())?;
    p.say("driverpkg.cli.info.class", manifest.class.as_str())?;
    p.say("driverpkg.cli.info.origin", manifest.origin.as_str())?;
    p.say("driverpkg.cli.info.rank", &manifest.rank.to_string())?;
    p.say(
        "driverpkg.cli.info.kernel-abi",
        &manifest.kernel_abi.join(", "),
    )?;
    p.say("driverpkg.cli.info.name-key", &manifest.name_key)
}

fn verify(p: &mut Printer<'_>, args: &[&str]) -> Result<(), CliError> {
    let [file, keys] = args else {
        return Err(CliError::Usage);
    };
    let verified = package::verify(&read(file)?, &trusted_keys(keys)?, &Limits::default())?;
    p.say("driverpkg.cli.verify.ok", file)?;
    p.say("driverpkg.cli.key-id", &verified.key_id)?;
    print_manifest(p, &verified.manifest)
}

fn info(p: &mut Printer<'_>, args: &[&str]) -> Result<(), CliError> {
    let [file] = args else {
        return Err(CliError::Usage);
    };
    let info = package::info(&read(file)?, &Limits::default())?;
    p.say("driverpkg.cli.info.not-verified", "")?;
    print_manifest(p, &info.manifest)?;
    match &info.key_id {
        Some(key_id) => p.say("driverpkg.cli.key-id", key_id)?,
        None => p.say("driverpkg.cli.info.unsigned", "")?,
    }
    p.say("driverpkg.cli.info.payload-sha256", &info.payload_sha256)?;
    for entry in info.entries.iter().filter(|e| !e.is_dir) {
        p.say(
            "driverpkg.cli.info.file",
            &format!("{} ({})", entry.path, entry.size),
        )?;
    }
    Ok(())
}

fn index(p: &mut Printer<'_>, args: &[&str], options: &[(&str, &str)]) -> Result<(), CliError> {
    let [dir, key] = args else {
        return Err(CliError::Usage);
    };
    let key = private_key(key)?;
    let mut trusted = match option(options, "trusted") {
        Some(path) => trusted_keys(path)?,
        None => TrustedKeys::new(),
    };
    trusted.insert(key.public());
    let built = write_signed_index(Path::new(dir), &key, &trusted, &Limits::default())?;
    p.say("driverpkg.cli.index.done", &built.len().to_string())?;
    p.say("driverpkg.cli.key-id", &key.public().key_id())
}

fn load_index(
    p: &mut Printer<'_>,
    path: &str,
    trusted: Option<&str>,
) -> Result<DriverIndex, CliError> {
    let bytes = read(path)?;
    let Some(keys) = trusted else {
        p.say("driverpkg.cli.match.index-not-verified", "")?;
        let text = std::str::from_utf8(&bytes).map_err(|_| crate::index::IndexError::NotUtf8)?;
        return Ok(DriverIndex::parse(text)?);
    };
    let signature_path = Path::new(path).with_file_name(INDEX_SIGNATURE_NAME);
    let signature = std::fs::read(signature_path).ok();
    Ok(DriverIndex::parse_verified(
        &bytes,
        signature.as_deref(),
        &trusted_keys(keys)?,
    )?)
}

fn print_plan(p: &mut Printer<'_>, plan: &Plan) -> Result<(), CliError> {
    for item in &plan.devices {
        let device = &item.device;
        let label = device.label.as_deref().unwrap_or_default();
        p.raw(&format!("{}  {}  {label}", device.address, device.primary))?;
        match &item.chosen {
            Some(driver) => {
                p.say(
                    "driverpkg.cli.match.driver",
                    &format!("{} {} ({})", driver.id, driver.version, driver.file),
                )?;
            }
            None => p.say("driverpkg.cli.match.no-driver", "")?,
        }
        if !item.alternatives.is_empty() {
            let names: Vec<String> = item
                .alternatives
                .iter()
                .map(|c| format!("{} {}", c.id, c.version))
                .collect();
            p.say("driverpkg.cli.match.alternatives", &names.join(", "))?;
        }
    }
    let missing = plan.driverless().len();
    p.say(
        "driverpkg.cli.match.with-driver",
        &(plan.devices.len() - missing).to_string(),
    )?;
    p.say("driverpkg.cli.match.without-driver", &missing.to_string())
}

fn match_hardware(
    p: &mut Printer<'_>,
    args: &[&str],
    options: &[(&str, &str)],
) -> Result<(), CliError> {
    let [index_path, list_path] = args else {
        return Err(CliError::Usage);
    };
    let index = load_index(p, index_path, option(options, "trusted"))?;
    let devices = parse_hardware_list(&read_text(list_path)?)?;
    let abi = option(options, "abi").unwrap_or(DEFAULT_KERNEL_ABI);
    print_plan(p, &resolve(&devices, &index, abi))
}

/// Runs `udrv` with the arguments after the program name, writing results to `out`.
pub fn run(args: &[String], lang: Lang, out: &mut dyn Write) -> Result<(), CliError> {
    let mut p = Printer { lang, out };
    let Some((command, rest)) = args.split_first() else {
        p.say("driverpkg.cli.usage", "")?;
        return Err(CliError::Usage);
    };
    let (positional, options) = split_options(rest, &["abi", "trusted"])?;
    match command.as_str() {
        "keygen" if positional.is_empty() => keygen(&mut p),
        "pack" => pack(&mut p, &positional),
        "verify" => verify(&mut p, &positional),
        "info" => info(&mut p, &positional),
        "index" => index(&mut p, &positional, &options),
        "match" => match_hardware(&mut p, &positional, &options),
        "help" | "--help" | "-h" => p.say("driverpkg.cli.usage", ""),
        "keygen" => Err(CliError::Usage),
        other => Err(CliError::UnknownCommand(other.to_string())),
    }
}

/// Renders an error for the user: translated text followed by the technical detail.
pub fn describe(error: &CliError, lang: Lang) -> String {
    let code = error.code();
    format!("{} [{error}]", text(lang, code).unwrap_or(code))
}
