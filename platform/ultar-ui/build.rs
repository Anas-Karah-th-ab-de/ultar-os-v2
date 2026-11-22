//! Generates the Rust design tokens of `ultar-ui` from `tokens.toml` (the one source of every
//! visual value, design spec §3). The output is included by `src/tokens/mod.rs`.

use std::fmt::{self, Write as _};
use std::path::PathBuf;
use toml::{Table, Value};

/// Why token generation failed.
#[derive(Debug)]
enum GenError {
    Io(std::io::Error),
    Parse(toml::de::Error),
    Missing(String),
    Invalid(String),
    Env(std::env::VarError),
}

impl fmt::Display for GenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GenError::Io(e) => write!(f, "tokens.toml: {e}"),
            GenError::Parse(e) => write!(f, "tokens.toml: {e}"),
            GenError::Missing(k) => write!(f, "tokens.toml: missing `{k}`"),
            GenError::Invalid(k) => write!(f, "tokens.toml: invalid value for `{k}`"),
            GenError::Env(e) => write!(f, "environment: {e}"),
        }
    }
}

impl std::error::Error for GenError {}

impl From<fmt::Error> for GenError {
    fn from(_: fmt::Error) -> Self {
        GenError::Invalid("formatting".into())
    }
}

type Res<T> = Result<T, GenError>;

fn main() -> Res<()> {
    println!("cargo:rerun-if-changed=tokens.toml");
    println!("cargo:rerun-if-changed=build.rs");
    let source = std::fs::read_to_string("tokens.toml").map_err(GenError::Io)?;
    let root: Table = source.parse().map_err(GenError::Parse)?;
    let mut out = String::new();
    gen_scale(&mut out, &root, "space", "values", "Space", "Spacing step")?;
    gen_scale(&mut out, &root, "type", "sizes", "TypeSize", "Type size")?;
    gen_radius(&mut out, &root)?;
    gen_numbers(
        &mut out,
        &root,
        "size",
        "f32",
        "Fixed sizes in logical points.",
    )?;
    gen_numbers(
        &mut out,
        &root,
        "motion",
        "u32",
        "Durations in milliseconds.",
    )?;
    gen_numbers(
        &mut out,
        &root,
        "layout",
        "f32",
        "Size-class limits (logical width).",
    )?;
    gen_numbers(&mut out, &root, "shadow", "f32", "The one elevation level.")?;
    gen_typography(&mut out, &root)?;
    gen_themes(&mut out, &root)?;
    let dir = std::env::var("OUT_DIR").map_err(GenError::Env)?;
    std::fs::write(PathBuf::from(dir).join("tokens_gen.rs"), out).map_err(GenError::Io)
}

fn table<'a>(root: &'a Table, path: &str) -> Res<&'a Table> {
    let mut current = root;
    for part in path.split('.') {
        current = current
            .get(part)
            .and_then(Value::as_table)
            .ok_or_else(|| GenError::Missing(path.to_string()))?;
    }
    Ok(current)
}

fn number(value: &Value, key: &str) -> Res<f64> {
    match value {
        Value::Integer(i) => Ok(*i as f64),
        Value::Float(f) => Ok(*f),
        _ => Err(GenError::Invalid(key.to_string())),
    }
}

fn numbers(root: &Table, section: &str, key: &str) -> Res<Vec<f64>> {
    let path = format!("{section}.{key}");
    let list = table(root, section)?
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| GenError::Missing(path.clone()))?;
    if list.is_empty() {
        return Err(GenError::Invalid(path));
    }
    list.iter().map(|v| number(v, &path)).collect()
}

fn float(value: f64) -> String {
    format!("{value:?}")
}

fn camel(name: &str) -> String {
    name.split('_')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

/// An enum with one variant per scale value (`S4`, `S8`, …) so that no other value can be named.
fn gen_scale(
    out: &mut String,
    root: &Table,
    section: &str,
    key: &str,
    name: &str,
    doc: &str,
) -> Res<()> {
    let values = numbers(root, section, key)?;
    writeln!(
        out,
        "/// {doc}s from `[{section}] {key}`; no other value exists."
    )?;
    writeln!(
        out,
        "#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]"
    )?;
    writeln!(out, "pub enum {name} {{")?;
    for v in &values {
        writeln!(out, "    /// {doc} of {v} logical points.\n    S{v},")?;
    }
    writeln!(out, "}}\nimpl {name} {{")?;
    writeln!(out, "    /// All steps, smallest first.")?;
    write!(out, "    pub const ALL: [{name}; {}] = [", values.len())?;
    for v in &values {
        write!(out, "{name}::S{v}, ")?;
    }
    writeln!(out, "];\n    /// The value in logical points.")?;
    writeln!(
        out,
        "    pub const fn points(self) -> f32 {{\n        match self {{"
    )?;
    for v in &values {
        writeln!(out, "            {name}::S{v} => {},", float(*v))?;
    }
    writeln!(out, "        }}\n    }}\n}}")?;
    Ok(())
}

fn gen_radius(out: &mut String, root: &Table) -> Res<()> {
    let radii = table(root, "radius")?;
    writeln!(out, "/// Corner radii from `[radius]`.")?;
    writeln!(
        out,
        "#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]\npub enum Radius {{"
    )?;
    for key in radii.keys() {
        writeln!(out, "    /// Radius `{key}`.\n    {},", camel(key))?;
    }
    writeln!(
        out,
        "}}\nimpl Radius {{\n    /// The radius in logical points."
    )?;
    writeln!(
        out,
        "    pub const fn points(self) -> f32 {{\n        match self {{"
    )?;
    for (key, value) in radii {
        let v = float(number(value, key)?);
        writeln!(out, "            Radius::{} => {v},", camel(key))?;
    }
    writeln!(out, "        }}\n    }}\n}}")?;
    Ok(())
}

/// A module of numeric constants, one per key of a flat section.
fn gen_numbers(out: &mut String, root: &Table, section: &str, ty: &str, doc: &str) -> Res<()> {
    writeln!(out, "/// {doc}\npub mod {section} {{")?;
    for (key, value) in table(root, section)? {
        let n = number(value, key)?;
        let literal = if ty == "u32" {
            if n < 0.0 || n.fract() != 0.0 {
                return Err(GenError::Invalid(format!("{section}.{key}")));
            }
            format!("{}", n as u32)
        } else {
            float(n)
        };
        writeln!(out, "    /// Token `{section}.{key}`.")?;
        writeln!(
            out,
            "    pub const {}: {ty} = {literal};",
            key.to_ascii_uppercase()
        )?;
    }
    writeln!(out, "}}")?;
    Ok(())
}

fn gen_typography(out: &mut String, root: &Table) -> Res<()> {
    let ty = table(root, "type")?;
    writeln!(
        out,
        "/// Typography tokens from `[type]`.\npub mod typography {{"
    )?;
    for (key, value) in ty {
        let name = key.to_ascii_uppercase();
        match value {
            Value::String(s) => {
                writeln!(out, "    /// Token `type.{key}`.\n    pub const {name}: &str = {s:?};")?
            }
            Value::Float(f) => writeln!(
                out,
                "    /// Token `type.{key}`.\n    pub const {name}: f32 = {};",
                float(*f)
            )?,
            Value::Integer(i) if key == "body" => writeln!(
                out,
                "    /// Token `type.{key}`.\n    pub const {name}: super::TypeSize = super::TypeSize::S{i};"
            )?,
            _ => {}
        }
    }
    let weights = numbers(root, "type", "weights")?;
    write!(
        out,
        "    /// Token `type.weights`.\n    pub const WEIGHTS: [u16; {}] = [",
        weights.len()
    )?;
    for w in weights {
        write!(out, "{}, ", w as u16)?;
    }
    writeln!(out, "];\n}}")?;
    Ok(())
}

fn colour(text: &str, key: &str) -> Res<String> {
    let hex = text.strip_prefix('#').unwrap_or("");
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(GenError::Invalid(key.to_string()));
    }
    Ok(format!(
        "Color::rgb(0x{}, 0x{}, 0x{})",
        &hex[0..2],
        &hex[2..4],
        &hex[4..6]
    ))
}

fn gen_themes(out: &mut String, root: &Table) -> Res<()> {
    let dark = table(root, "theme.dark")?;
    let light = table(root, "theme.light")?;
    if !dark.keys().eq(light.keys()) {
        return Err(GenError::Invalid(
            "theme.light (roles differ from theme.dark)".into(),
        ));
    }
    writeln!(
        out,
        "/// Colour roles from `[theme.*]`; code names roles, never values."
    )?;
    writeln!(
        out,
        "#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]\npub enum ColorRole {{"
    )?;
    for key in dark.keys() {
        writeln!(out, "    /// Role `{key}`.\n    {},", camel(key))?;
    }
    writeln!(
        out,
        "}}\nimpl ColorRole {{\n    /// All roles in index order."
    )?;
    write!(out, "    pub const ALL: [ColorRole; {}] = [", dark.len())?;
    for key in dark.keys() {
        write!(out, "ColorRole::{}, ", camel(key))?;
    }
    writeln!(
        out,
        "];\n    /// The role name as written in `tokens.toml`."
    )?;
    writeln!(
        out,
        "    pub const fn name(self) -> &'static str {{\n        match self {{"
    )?;
    for key in dark.keys() {
        writeln!(out, "            ColorRole::{} => {key:?},", camel(key))?;
    }
    writeln!(out, "        }}\n    }}\n}}")?;
    for (name, theme) in [("DARK", dark), ("LIGHT", light)] {
        writeln!(
            out,
            "/// Palette of `[theme.{}]`.",
            name.to_ascii_lowercase()
        )?;
        write!(out, "pub const {name}: Palette = Palette::from_roles([")?;
        for (key, value) in theme {
            let text = value
                .as_str()
                .ok_or_else(|| GenError::Invalid(key.clone()))?;
            write!(out, "{}, ", colour(text, key)?)?;
        }
        writeln!(out, "]);")?;
    }
    Ok(())
}
