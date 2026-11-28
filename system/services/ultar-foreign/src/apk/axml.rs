//! The binary XML of `AndroidManifest.xml` (Android's `ResXMLTree`), read only as far as
//! the service needs: the package, its version, the permissions it asks for, the symbol
//! and name of the application and the activity the launcher starts.
//!
//! Strings come from the file's string pool; attributes are recognised by their
//! resource id (`android:label` = `0x01010001` …) so that stripped names still work.

/// Largest manifest the reader takes.
pub const MAX_MANIFEST: usize = 4 << 20;
/// Most strings of a pool.
const MAX_STRINGS: usize = 200_000;
/// Most permissions kept.
const MAX_PERMISSIONS: usize = 128;
/// Deepest element nesting.
const MAX_DEPTH: usize = 64;

const RES_STRING_POOL: u16 = 0x0001;
const RES_XML: u16 = 0x0003;
const RES_XML_START_ELEMENT: u16 = 0x0102;
const RES_XML_END_ELEMENT: u16 = 0x0103;
const RES_XML_RESOURCE_MAP: u16 = 0x0180;

const ATTR_LABEL: u32 = 0x0101_0001;
const ATTR_ICON: u32 = 0x0101_0002;
const ATTR_NAME: u32 = 0x0101_0003;
const ATTR_ENABLED: u32 = 0x0101_000e;
const ATTR_MIN_SDK: u32 = 0x0101_020c;
const ATTR_VERSION_CODE: u32 = 0x0101_021b;
const ATTR_VERSION_NAME: u32 = 0x0101_021c;
const ATTR_TARGET_ACTIVITY: u32 = 0x0101_0202;

/// A typed value of an attribute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// A string.
    Text(String),
    /// A reference to a resource (`@string/app_name`).
    Ref(u32),
    /// A number.
    Int(u32),
}

/// What the manifest says.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Manifest {
    /// `package`.
    pub package: String,
    /// `android:versionName` (a string or a reference).
    pub version_name: Option<Value>,
    /// `android:versionCode`.
    pub version_code: u32,
    /// `uses-sdk android:minSdkVersion`.
    pub min_sdk: u32,
    /// `uses-permission android:name`.
    pub permissions: Vec<String>,
    /// `application android:label`.
    pub label: Option<Value>,
    /// `application android:icon`.
    pub icon: Option<u32>,
    /// The activity with `MAIN` + `LAUNCHER` (full class name).
    pub launcher: Option<String>,
}

/// Why the manifest cannot be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AxmlError;

fn u16_at(b: &[u8], at: usize) -> Result<u16, AxmlError> {
    b.get(at..at + 2)
        .map(|s| u16::from_le_bytes([s[0], s[1]]))
        .ok_or(AxmlError)
}

fn u32_at(b: &[u8], at: usize) -> Result<u32, AxmlError> {
    b.get(at..at + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
        .ok_or(AxmlError)
}

/// A string pool (`ResStringPool`), UTF-8 or UTF-16.
#[derive(Debug, Default)]
pub struct Pool {
    strings: Vec<String>,
}

impl Pool {
    /// The string at `index`.
    pub fn get(&self, index: u32) -> Option<&str> {
        self.strings.get(index as usize).map(String::as_str)
    }

    /// Reads the pool chunk `chunk` (header included).
    pub fn parse(chunk: &[u8]) -> Result<Self, AxmlError> {
        let header = u16_at(chunk, 2)? as usize;
        let count = u32_at(chunk, 8)? as usize;
        let flags = u32_at(chunk, 16)?;
        let start = u32_at(chunk, 20)? as usize;
        if count > MAX_STRINGS || header < 28 || start > chunk.len() {
            return Err(AxmlError);
        }
        let utf8 = flags & (1 << 8) != 0;
        let mut strings = Vec::with_capacity(count);
        for i in 0..count {
            let offset = u32_at(chunk, header + 4 * i)? as usize;
            let at = start.checked_add(offset).ok_or(AxmlError)?;
            let s = if utf8 {
                utf8_string(chunk, at)
            } else {
                utf16_string(chunk, at)
            };
            // a damaged string is empty rather than fatal (apps obfuscate pools)
            strings.push(s.unwrap_or_default());
        }
        Ok(Self { strings })
    }
}

fn utf8_len(b: &[u8], at: usize) -> Result<(usize, usize), AxmlError> {
    let first = *b.get(at).ok_or(AxmlError)? as usize;
    if first & 0x80 != 0 {
        let second = *b.get(at + 1).ok_or(AxmlError)? as usize;
        Ok((((first & 0x7f) << 8) | second, 2))
    } else {
        Ok((first, 1))
    }
}

fn utf8_string(b: &[u8], at: usize) -> Result<String, AxmlError> {
    let (_, skip) = utf8_len(b, at)?; // length in UTF-16 units
    let (len, skip2) = utf8_len(b, at + skip)?;
    let from = at + skip + skip2;
    let bytes = b.get(from..from + len).ok_or(AxmlError)?;
    String::from_utf8(bytes.to_vec()).map_err(|_| AxmlError)
}

fn utf16_string(b: &[u8], at: usize) -> Result<String, AxmlError> {
    let first = u16_at(b, at)? as usize;
    let (len, from) = if first & 0x8000 != 0 {
        let second = u16_at(b, at + 2)? as usize;
        (((first & 0x7fff) << 16) | second, at + 4)
    } else {
        (first, at + 2)
    };
    let raw = b.get(from..from + 2 * len).ok_or(AxmlError)?;
    let units: Vec<u16> = raw
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16(&units).map_err(|_| AxmlError)
}

/// One attribute of an element.
struct Attr {
    id: Option<u32>,
    name: String,
    value: Value,
}

fn attr_value(pool: &Pool, raw: u32, kind: u8, data: u32) -> Value {
    match kind {
        0x03 => Value::Text(pool.get(data).unwrap_or_default().to_owned()),
        0x01 => Value::Ref(data),
        _ if raw != u32::MAX => Value::Text(pool.get(raw).unwrap_or_default().to_owned()),
        _ => Value::Int(data),
    }
}

fn element(chunk: &[u8], pool: &Pool, ids: &[u32]) -> Result<(String, Vec<Attr>), AxmlError> {
    let header = u16_at(chunk, 2)? as usize;
    let ext = header; // ResXMLTree_attrExt follows the node header
    let name = pool
        .get(u32_at(chunk, ext + 4)?)
        .unwrap_or_default()
        .to_owned();
    let attr_start = u16_at(chunk, ext + 8)? as usize;
    let attr_size = u16_at(chunk, ext + 10)? as usize;
    let count = u16_at(chunk, ext + 12)? as usize;
    if attr_size < 20 {
        return Err(AxmlError);
    }
    let mut attrs = Vec::with_capacity(count.min(64));
    for i in 0..count {
        let at = ext + attr_start + i * attr_size;
        let name_index = u32_at(chunk, at + 4)?;
        let raw = u32_at(chunk, at + 8)?;
        let kind = *chunk.get(at + 15).ok_or(AxmlError)?;
        let data = u32_at(chunk, at + 16)?;
        attrs.push(Attr {
            id: ids.get(name_index as usize).copied(),
            name: pool.get(name_index).unwrap_or_default().to_owned(),
            value: attr_value(pool, raw, kind, data),
        });
    }
    Ok((name, attrs))
}

fn find<'a>(attrs: &'a [Attr], id: u32, name: &str) -> Option<&'a Value> {
    attrs
        .iter()
        .find(|a| a.id == Some(id) || (a.id.is_none() && a.name == name))
        .map(|a| &a.value)
}

fn text(v: Option<&Value>) -> Option<String> {
    match v {
        Some(Value::Text(s)) => Some(s.clone()),
        _ => None,
    }
}

/// State of the walk through the elements.
#[derive(Default)]
struct Walk {
    manifest: Manifest,
    stack: Vec<String>,
    /// The activity (or alias target) being read and whether it is enabled.
    activity: Option<(String, bool)>,
    main: bool,
    launcher: bool,
}

impl Walk {
    fn start(&mut self, name: &str, attrs: &[Attr]) {
        let parent = self.stack.last().map(String::as_str).unwrap_or_default();
        let m = &mut self.manifest;
        match (parent, name) {
            ("", "manifest") => {
                m.package = text(find(attrs, 0, "package")).unwrap_or_default();
                m.version_name = find(attrs, ATTR_VERSION_NAME, "versionName").cloned();
                if let Some(Value::Int(c)) = find(attrs, ATTR_VERSION_CODE, "versionCode") {
                    m.version_code = *c;
                }
            }
            ("manifest", "uses-sdk") => {
                if let Some(Value::Int(v)) = find(attrs, ATTR_MIN_SDK, "minSdkVersion") {
                    m.min_sdk = *v;
                }
            }
            ("manifest", "uses-permission" | "uses-permission-sdk-23") => {
                if let Some(p) = text(find(attrs, ATTR_NAME, "name")) {
                    if m.permissions.len() < MAX_PERMISSIONS && !m.permissions.contains(&p) {
                        m.permissions.push(p);
                    }
                }
            }
            ("manifest", "application") => {
                m.label = find(attrs, ATTR_LABEL, "label").cloned();
                if let Some(Value::Ref(r)) = find(attrs, ATTR_ICON, "icon") {
                    m.icon = Some(*r);
                }
            }
            ("application", "activity" | "activity-alias") => {
                let target = if name == "activity-alias" {
                    text(find(attrs, ATTR_TARGET_ACTIVITY, "targetActivity"))
                } else {
                    text(find(attrs, ATTR_NAME, "name"))
                };
                let enabled = !matches!(find(attrs, ATTR_ENABLED, "enabled"), Some(Value::Int(0)));
                self.activity = target.map(|t| (t, enabled));
                self.main = false;
                self.launcher = false;
            }
            ("intent-filter", "action") => {
                self.main |= text(find(attrs, ATTR_NAME, "name")).as_deref()
                    == Some("android.intent.action.MAIN");
            }
            ("intent-filter", "category") => {
                self.launcher |= text(find(attrs, ATTR_NAME, "name")).as_deref()
                    == Some("android.intent.category.LAUNCHER");
            }
            _ => {}
        }
    }

    fn end(&mut self, name: &str) {
        if !matches!(name, "activity" | "activity-alias") {
            return;
        }
        if let Some((class, enabled)) = self.activity.take() {
            if enabled && self.main && self.launcher && self.manifest.launcher.is_none() {
                self.manifest.launcher = Some(class);
            }
        }
    }
}

/// Reads the binary manifest.
pub fn parse(bytes: &[u8]) -> Result<Manifest, AxmlError> {
    if bytes.len() > MAX_MANIFEST || u16_at(bytes, 0)? != RES_XML {
        return Err(AxmlError);
    }
    let mut at = u16_at(bytes, 2)? as usize;
    let mut pool = Pool::default();
    let mut ids: Vec<u32> = Vec::new();
    let mut walk = Walk::default();
    while at + 8 <= bytes.len() {
        let kind = u16_at(bytes, at)?;
        let size = u32_at(bytes, at + 4)? as usize;
        if size < 8 || at + size > bytes.len() {
            return Err(AxmlError);
        }
        let chunk = &bytes[at..at + size];
        match kind {
            RES_STRING_POOL => pool = Pool::parse(chunk)?,
            RES_XML_RESOURCE_MAP => {
                let header = u16_at(chunk, 2)? as usize;
                ids = chunk
                    .get(header..)
                    .ok_or(AxmlError)?
                    .chunks_exact(4)
                    .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                    .collect();
            }
            RES_XML_START_ELEMENT => {
                let (name, attrs) = element(chunk, &pool, &ids)?;
                if walk.stack.len() >= MAX_DEPTH {
                    return Err(AxmlError);
                }
                walk.start(&name, &attrs);
                walk.stack.push(name);
            }
            RES_XML_END_ELEMENT => {
                if let Some(name) = walk.stack.pop() {
                    walk.end(&name);
                }
            }
            _ => {}
        }
        at += size;
    }
    let mut m = walk.manifest;
    if m.package.is_empty() {
        return Err(AxmlError);
    }
    if let Some(class) = m.launcher.take() {
        m.launcher = Some(full_class(&m.package, &class));
    }
    Ok(m)
}

/// `.Main` and `Main` belong to the package; anything with a dot inside is already full.
fn full_class(package: &str, class: &str) -> String {
    if let Some(rest) = class.strip_prefix('.') {
        format!("{package}.{rest}")
    } else if !class.contains('.') {
        format!("{package}.{class}")
    } else {
        class.to_owned()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Builds a binary manifest (UTF-8 pool, resource map for the android attributes).
    pub struct Builder {
        strings: Vec<String>,
        ids: Vec<u32>,
        body: Vec<u8>,
    }

    impl Builder {
        pub fn new() -> Self {
            // the first strings are the attribute names with resource ids
            let named = [
                ("label", ATTR_LABEL),
                ("icon", ATTR_ICON),
                ("name", ATTR_NAME),
                ("minSdkVersion", ATTR_MIN_SDK),
                ("versionCode", ATTR_VERSION_CODE),
                ("versionName", ATTR_VERSION_NAME),
                ("targetActivity", ATTR_TARGET_ACTIVITY),
                ("enabled", ATTR_ENABLED),
            ];
            Self {
                strings: named.iter().map(|(n, _)| (*n).to_owned()).collect(),
                ids: named.iter().map(|(_, i)| *i).collect(),
                body: Vec::new(),
            }
        }

        fn string(&mut self, s: &str) -> u32 {
            if let Some(i) = self.strings.iter().position(|x| x == s) {
                return i as u32;
            }
            self.strings.push(s.to_owned());
            (self.strings.len() - 1) as u32
        }

        /// `attrs`: (name, value) where value is "@0x7f010000", "#12" (int) or text.
        pub fn start(&mut self, name: &str, attrs: &[(&str, &str)]) -> &mut Self {
            let name_i = self.string(name);
            let mut list = Vec::new();
            for (n, v) in attrs {
                let ni = self.string(n);
                let (raw, kind, data) = if let Some(h) = v.strip_prefix("@0x") {
                    (u32::MAX, 0x01u8, u32::from_str_radix(h, 16).unwrap())
                } else if let Some(d) = v.strip_prefix('#') {
                    (u32::MAX, 0x10, d.parse().unwrap())
                } else {
                    let si = self.string(v);
                    (si, 0x03, si)
                };
                list.push((ni, raw, kind, data));
            }
            let mut c = Vec::new();
            c.extend_from_slice(&RES_XML_START_ELEMENT.to_le_bytes());
            c.extend_from_slice(&16u16.to_le_bytes());
            let size = 16 + 20 + 20 * list.len();
            c.extend_from_slice(&(size as u32).to_le_bytes());
            c.extend_from_slice(&[0; 8]);
            c.extend_from_slice(&u32::MAX.to_le_bytes());
            c.extend_from_slice(&name_i.to_le_bytes());
            c.extend_from_slice(&20u16.to_le_bytes());
            c.extend_from_slice(&20u16.to_le_bytes());
            c.extend_from_slice(&(list.len() as u16).to_le_bytes());
            c.extend_from_slice(&[0; 6]);
            for (ni, raw, kind, data) in list {
                c.extend_from_slice(&u32::MAX.to_le_bytes());
                c.extend_from_slice(&ni.to_le_bytes());
                c.extend_from_slice(&raw.to_le_bytes());
                c.extend_from_slice(&8u16.to_le_bytes());
                c.push(0);
                c.push(kind);
                c.extend_from_slice(&data.to_le_bytes());
            }
            self.body.extend_from_slice(&c);
            self
        }

        pub fn end(&mut self, name: &str) -> &mut Self {
            let name_i = self.string(name);
            self.body
                .extend_from_slice(&RES_XML_END_ELEMENT.to_le_bytes());
            self.body.extend_from_slice(&16u16.to_le_bytes());
            self.body.extend_from_slice(&24u32.to_le_bytes());
            self.body.extend_from_slice(&[0; 8]);
            self.body.extend_from_slice(&u32::MAX.to_le_bytes());
            self.body.extend_from_slice(&name_i.to_le_bytes());
            self
        }

        pub fn build(&self) -> Vec<u8> {
            let pool = pool_chunk(&self.strings);
            let mut map = Vec::new();
            map.extend_from_slice(&RES_XML_RESOURCE_MAP.to_le_bytes());
            map.extend_from_slice(&8u16.to_le_bytes());
            map.extend_from_slice(&((8 + 4 * self.ids.len()) as u32).to_le_bytes());
            for i in &self.ids {
                map.extend_from_slice(&i.to_le_bytes());
            }
            let mut out = Vec::new();
            out.extend_from_slice(&RES_XML.to_le_bytes());
            out.extend_from_slice(&8u16.to_le_bytes());
            let total = 8 + pool.len() + map.len() + self.body.len();
            out.extend_from_slice(&(total as u32).to_le_bytes());
            out.extend_from_slice(&pool);
            out.extend_from_slice(&map);
            out.extend_from_slice(&self.body);
            out
        }
    }

    /// A UTF-8 string pool chunk.
    pub fn pool_chunk(strings: &[String]) -> Vec<u8> {
        let mut data = Vec::new();
        let mut offsets = Vec::new();
        for s in strings {
            offsets.push(data.len() as u32);
            let n = s.encode_utf16().count();
            assert!(n < 0x80 && s.len() < 0x80);
            data.push(n as u8);
            data.push(s.len() as u8);
            data.extend_from_slice(s.as_bytes());
            data.push(0);
        }
        while data.len() % 4 != 0 {
            data.push(0);
        }
        let header = 28usize;
        let start = header + 4 * strings.len();
        let mut c = Vec::new();
        c.extend_from_slice(&RES_STRING_POOL.to_le_bytes());
        c.extend_from_slice(&(header as u16).to_le_bytes());
        c.extend_from_slice(&((start + data.len()) as u32).to_le_bytes());
        c.extend_from_slice(&(strings.len() as u32).to_le_bytes());
        c.extend_from_slice(&0u32.to_le_bytes());
        c.extend_from_slice(&(1u32 << 8).to_le_bytes());
        c.extend_from_slice(&(start as u32).to_le_bytes());
        c.extend_from_slice(&0u32.to_le_bytes());
        for o in offsets {
            c.extend_from_slice(&o.to_le_bytes());
        }
        c.extend_from_slice(&data);
        c
    }

    pub fn sample() -> Vec<u8> {
        let mut b = Builder::new();
        b.start(
            "manifest",
            &[
                ("package", "org.example.calc"),
                ("versionCode", "#42"),
                ("versionName", "1.4.2"),
            ],
        )
        .start("uses-sdk", &[("minSdkVersion", "#21")])
        .end("uses-sdk")
        .start(
            "uses-permission",
            &[("name", "android.permission.INTERNET")],
        )
        .end("uses-permission")
        .start("uses-permission", &[("name", "android.permission.CAMERA")])
        .end("uses-permission")
        .start(
            "application",
            &[("label", "@0x7f010000"), ("icon", "@0x7f020000")],
        )
        .start("activity", &[("name", ".Settings")])
        .end("activity")
        .start("activity", &[("name", ".Main")])
        .start("intent-filter", &[])
        .start("action", &[("name", "android.intent.action.MAIN")])
        .end("action")
        .start("category", &[("name", "android.intent.category.LAUNCHER")])
        .end("category")
        .end("intent-filter")
        .end("activity")
        .end("application")
        .end("manifest");
        b.build()
    }

    #[test]
    fn the_manifest_says_package_version_permissions_and_launcher() {
        let m = parse(&sample()).unwrap();
        assert_eq!(m.package, "org.example.calc");
        assert_eq!(m.version_code, 42);
        assert_eq!(m.version_name, Some(Value::Text("1.4.2".into())));
        assert_eq!(m.min_sdk, 21);
        assert_eq!(
            m.permissions,
            vec!["android.permission.INTERNET", "android.permission.CAMERA"]
        );
        assert_eq!(m.label, Some(Value::Ref(0x7f01_0000)));
        assert_eq!(m.icon, Some(0x7f02_0000));
        assert_eq!(m.launcher.as_deref(), Some("org.example.calc.Main"));
    }

    #[test]
    fn class_names_are_completed() {
        assert_eq!(full_class("a.b", ".X"), "a.b.X");
        assert_eq!(full_class("a.b", "X"), "a.b.X");
        assert_eq!(full_class("a.b", "c.d.X"), "c.d.X");
    }

    #[test]
    fn damaged_manifests_never_panic() {
        let good = sample();
        assert!(parse(&good[..good.len() / 2]).is_err() || parse(&good[..good.len() / 2]).is_ok());
        let mut seed = 0x9e37_79b9_u64;
        for _ in 0..5000 {
            let mut copy = good.clone();
            for _ in 0..3 {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                let at = (seed as usize) % copy.len();
                copy[at] = (seed >> 24) as u8;
            }
            let _ = parse(&copy);
        }
        assert!(parse(b"").is_err());
        assert!(parse(&[3, 0, 8, 0, 255, 255, 255, 255]).is_err());
    }
}
