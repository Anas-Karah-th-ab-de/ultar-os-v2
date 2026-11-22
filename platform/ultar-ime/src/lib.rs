//! Ultar-Tastatur – Kern ohne Zeichnen.
//!
//! Enthält die verbindlichen Belegungen (Arabisch nach Gboard/„Arabisch 101“, Englisch QWERTY, Deutsch QWERTZ),
//! Sonderseiten (Zeichen, PIN, Telefon, Zahl), die Maße je Bildschirm, die Platzierung der Tasten, die
//! lückenlose Trefferprüfung und die Varianten für den langen Druck. Dieselben Daten nutzen Oberfläche,
//! Desktop und Browser – so sieht die Tastatur überall gleich aus.
//!
//! Ziffern: überall 0–9 (arabische Ziffern), auch im Arabischen. Indische Ziffern (٠–٩, ۰–۹) erzeugt die
//! Tastatur nie; eingefügte werden mit [`normalize_digits`] umgewandelt.

/// Sprache der Buchstabenseite.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    Ar,
    En,
    De,
}

impl Lang {
    pub const ALL: [Lang; 3] = [Lang::Ar, Lang::En, Lang::De];

    pub fn from_code(c: &str) -> Lang {
        match c {
            "en" => Lang::En,
            "de" => Lang::De,
            _ => Lang::Ar,
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Lang::Ar => "ar",
            Lang::En => "en",
            Lang::De => "de",
        }
    }

    /// Name in der eigenen Sprache (steht auf der Leertaste).
    pub fn native(self) -> &'static str {
        match self {
            Lang::Ar => "العربية",
            Lang::En => "English",
            Lang::De => "Deutsch",
        }
    }

    /// Nächste Sprache im Wechsel; die Systemsprache steht vorn.
    pub fn next(self, first: Lang) -> Lang {
        let mut order = vec![first];
        order.extend(Lang::ALL.iter().copied().filter(|l| *l != first));
        let i = order.iter().position(|l| *l == self).unwrap_or(0);
        order[(i + 1) % order.len()]
    }

    pub fn rtl(self) -> bool {
        self == Lang::Ar
    }
}

/// Art des Eingabefelds – bestimmt die Seite und die Aktionstaste.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Purpose {
    Text,
    /// Mehrzeilig: Aktionstaste = neue Zeile
    Multiline,
    Password,
    Pin,
    Phone,
    Number,
    Email,
    Url,
    Search,
    Send,
}

impl Purpose {
    /// Kein Lernen, keine Vorschläge, kein Verlauf der Zwischenablage.
    pub fn sensitive(self) -> bool {
        matches!(self, Purpose::Password | Purpose::Pin)
    }

    /// Ziffernblock statt Buchstaben.
    pub fn pad(self) -> bool {
        matches!(self, Purpose::Pin | Purpose::Phone | Purpose::Number)
    }
}

/// Feldart aus der Feld-Kennung der Oberfläche ableiten.
pub fn purpose_for(id: &str) -> Purpose {
    let id = id.to_ascii_lowercase();
    let has = |s: &str| id.contains(s);
    if has("pin") {
        Purpose::Pin
    } else if has("pw") || has("pass") || has("tok") || has("secret") {
        Purpose::Password
    } else if has("phone") || has("tel") || has("cnum") || id == "sms_to" {
        Purpose::Phone
    } else if has("num") || has("amount") || has("betrag") {
        Purpose::Number
    } else if has("mail") {
        Purpose::Email
    } else if has("url") || id == "host" || id == "ep" || has("addr") {
        Purpose::Url
    } else if id == "q" || id.ends_with("_q") || has("search") {
        Purpose::Search
    } else if id == "chat" || id == "buddy" || has("draft") || has("newtext") {
        Purpose::Send
    } else if has("note") || has("todo") {
        Purpose::Multiline
    } else {
        Purpose::Text
    }
}

/// Seite der Tastatur.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    Letters,
    Symbols,
    Symbols2,
}

/// Umschalt-Zustand (nur Latein).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shift {
    Off,
    Once,
    Lock,
}

/// Was eine Taste tut.
#[derive(Clone, PartialEq, Debug)]
pub enum Act {
    /// Zeichen(folge) einfügen
    Text(String),
    Shift,
    Backspace,
    ToSymbols,
    ToSymbols2,
    ToLetters,
    Lang,
    Space,
    Action,
}

/// Aussehen einer Taste.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Style {
    /// Buchstabe/Zeichen auf heller Taste
    Letter,
    /// Ziffer der Zahlenreihe (ohne Hintergrund)
    Digit,
    /// Funktionstaste (Umschalt, Löschen) – etwas dunkler
    Func,
    /// Seitenumschalter (?123 / ABC) – Pille, dezent im Akzent
    Toggle,
    /// Taste der unteren Reihe – Pille
    Pill,
    Space,
    /// runde Aktionstaste im Markengold
    Action,
    /// große Taste im Ziffernblock
    Pad,
}

#[derive(Clone, PartialEq, Debug)]
pub struct KeyDef {
    pub act: Act,
    pub label: String,
    /// Breite in Tasteneinheiten
    pub weight: f32,
    pub style: Style,
    /// kleines Nebenzeichen (langer Druck), z. B. Ziffer über der oberen Reihe
    pub hint: Option<String>,
}

impl KeyDef {
    fn ch(c: &str) -> KeyDef {
        KeyDef { act: Act::Text(c.into()), label: c.into(), weight: 1.0, style: Style::Letter, hint: None }
    }
    fn f(act: Act, label: &str, weight: f32, style: Style) -> KeyDef {
        KeyDef { act, label: label.into(), weight, style, hint: None }
    }
    /// Eingefügter Text dieser Taste (falls Zeichen-Taste).
    pub fn text(&self) -> Option<&str> {
        match &self.act {
            Act::Text(s) => Some(s),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RowKind {
    Numbers,
    Keys,
    Bottom,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Row {
    pub kind: RowKind,
    pub keys: Vec<KeyDef>,
}

/// Verbindliche Buchstabenreihen.
pub const AR_ROWS: [&str; 3] = ["ضصثقفغعهخحج", "شسيبلاتنمكط", "ذءؤرىةوزظد"];
pub const EN_ROWS: [&str; 3] = ["qwertyuiop", "asdfghjkl", "zxcvbnm"];
pub const DE_ROWS: [&str; 3] = ["qwertzuiop", "asdfghjkl", "yxcvbnm"];
pub const DIGITS: &str = "1234567890";

fn letters(lang: Lang) -> [&'static str; 3] {
    match lang {
        Lang::Ar => AR_ROWS,
        Lang::En => EN_ROWS,
        Lang::De => DE_ROWS,
    }
}

fn up(c: char, shift: Shift) -> String {
    if shift == Shift::Off {
        c.to_string()
    } else if c == 'ß' {
        "ß".into()
    } else {
        c.to_uppercase().collect()
    }
}

/// Untere Reihe (Buchstaben- und Zeichenseiten).
fn bottom(lang: Lang, page: Page, purpose: Purpose) -> Row {
    let toggle = if page == Page::Letters {
        KeyDef::f(Act::ToSymbols, if lang == Lang::Ar { "123؟" } else { "?123" }, 1.5, Style::Toggle)
    } else {
        KeyDef::f(Act::ToLetters, if lang == Lang::Ar { "أبج" } else { "ABC" }, 1.5, Style::Toggle)
    };
    let (left, right) = match purpose {
        Purpose::Email => ("@", "."),
        Purpose::Url => ("/", "."),
        _ => (if lang == Lang::Ar { "،" } else { "," }, "."),
    };
    let mut l = KeyDef::ch(left);
    l.style = Style::Pill;
    let mut r = KeyDef::ch(right);
    r.style = Style::Pill;
    r.hint = Some(if lang == Lang::Ar { "؟".into() } else { "?".into() });
    let globe = KeyDef::f(Act::Lang, "", 1.0, Style::Pill);
    let space = KeyDef::f(Act::Space, lang.native(), 4.0, Style::Space);
    let action = KeyDef::f(Act::Action, "", 1.4, Style::Action);
    Row { kind: RowKind::Bottom, keys: vec![toggle, l, globe, space, r, action] }
}

fn ar_punct(lang: Lang, latin: &'static str, arabic: &'static str) -> &'static str {
    if lang == Lang::Ar {
        arabic
    } else {
        latin
    }
}

/// Alle Reihen einer Seite, von oben nach unten.
pub fn rows(lang: Lang, page: Page, purpose: Purpose, shift: Shift, number_row: bool) -> Vec<Row> {
    if purpose.pad() {
        return pad_rows(purpose);
    }
    let mut out = Vec::new();
    match page {
        Page::Letters => {
            let l = letters(lang);
            if number_row {
                let keys = DIGITS
                    .chars()
                    .map(|c| {
                        let mut k = KeyDef::ch(&c.to_string());
                        k.style = Style::Digit;
                        k
                    })
                    .collect();
                out.push(Row { kind: RowKind::Numbers, keys });
            }
            for (ri, row) in l.iter().enumerate() {
                let mut keys: Vec<KeyDef> = row
                    .chars()
                    .enumerate()
                    .map(|(i, c)| {
                        let mut k = KeyDef::ch(&up(c, if lang == Lang::Ar { Shift::Off } else { shift }));
                        if ri == 0 && !number_row && i < 10 {
                            k.hint = Some(DIGITS[i..i + 1].into());
                        }
                        k
                    })
                    .collect();
                if ri == 2 {
                    if lang != Lang::Ar {
                        keys.insert(0, KeyDef::f(Act::Shift, "", 1.5, Style::Func));
                        keys.push(KeyDef::f(Act::Backspace, "", 1.5, Style::Func));
                    } else {
                        keys.push(KeyDef::f(Act::Backspace, "", 1.0, Style::Func));
                    }
                }
                out.push(Row { kind: RowKind::Keys, keys });
            }
        }
        Page::Symbols => {
            out.push(Row { kind: RowKind::Keys, keys: DIGITS.chars().map(|c| KeyDef::ch(&c.to_string())).collect() });
            out.push(Row { kind: RowKind::Keys, keys: ["@", "#", "€", "$", "%", "&", "-", "+", "(", ")"].iter().map(|c| KeyDef::ch(c)).collect() });
            let mut r3 = vec![KeyDef::f(Act::ToSymbols2, "=\\<", 1.5, Style::Func)];
            for c in ["*", "\"", "'", ":", ar_punct(lang, ";", "؛"), "!", ar_punct(lang, "?", "؟")] {
                r3.push(KeyDef::ch(c));
            }
            r3.push(KeyDef::f(Act::Backspace, "", 1.5, Style::Func));
            out.push(Row { kind: RowKind::Keys, keys: r3 });
        }
        Page::Symbols2 => {
            out.push(Row { kind: RowKind::Keys, keys: ["~", "`", "|", "•", "°", "^", "_", "{", "}", "\\"].iter().map(|c| KeyDef::ch(c)).collect() });
            out.push(Row { kind: RowKind::Keys, keys: ["£", "¥", "¢", "©", "®", "™", "§", "¶", "[", "]"].iter().map(|c| KeyDef::ch(c)).collect() });
            let mut r3 = vec![KeyDef::f(Act::ToSymbols, if lang == Lang::Ar { "123؟" } else { "?123" }, 1.5, Style::Func)];
            for c in ["<", ">", "«", "»", "×", "÷", "="] {
                r3.push(KeyDef::ch(c));
            }
            r3.push(KeyDef::f(Act::Backspace, "", 1.5, Style::Func));
            out.push(Row { kind: RowKind::Keys, keys: r3 });
        }
    }
    out.push(bottom(lang, page, purpose));
    out
}

/// Ziffernblöcke: PIN (3 Spalten), Telefon und Zahl (4 Spalten).
fn pad_rows(purpose: Purpose) -> Vec<Row> {
    let pk = |c: &str| {
        let mut k = KeyDef::ch(c);
        k.style = Style::Pad;
        k
    };
    let fk = |c: &str| {
        let mut k = KeyDef::ch(c);
        k.style = Style::Func;
        k
    };
    let bk = || KeyDef::f(Act::Backspace, "", 1.0, Style::Func);
    let ak = || KeyDef::f(Act::Action, "", 1.0, Style::Action);
    let row = |keys: Vec<KeyDef>| Row { kind: RowKind::Keys, keys };
    match purpose {
        Purpose::Pin => vec![
            row(vec![pk("1"), pk("2"), pk("3")]),
            row(vec![pk("4"), pk("5"), pk("6")]),
            row(vec![pk("7"), pk("8"), pk("9")]),
            row(vec![bk(), pk("0"), ak()]),
        ],
        Purpose::Phone => vec![
            row(vec![pk("1"), pk("2"), pk("3"), bk()]),
            row(vec![pk("4"), pk("5"), pk("6"), fk("+")]),
            row(vec![pk("7"), pk("8"), pk("9"), fk("*")]),
            row(vec![fk("#"), pk("0"), KeyDef::f(Act::Space, "", 1.0, Style::Func), ak()]),
        ],
        _ => vec![
            row(vec![pk("1"), pk("2"), pk("3"), bk()]),
            row(vec![pk("4"), pk("5"), pk("6"), fk("-")]),
            row(vec![pk("7"), pk("8"), pk("9"), fk(",")]),
            row(vec![fk("."), pk("0"), KeyDef::f(Act::Space, "", 1.0, Style::Func), ak()]),
        ],
    }
}

/// Arabische Vokalzeichen (Taschkil): Fatha, Damma, Kasra, Schadda, Sukun, Fathatan, Dammatan, Kasratan.
pub const TASHKIL: [char; 8] = ['\u{064E}', '\u{064F}', '\u{0650}', '\u{0651}', '\u{0652}', '\u{064B}', '\u{064C}', '\u{064D}'];

/// Varianten für den langen Druck (Reihenfolge = Anzeige, erste = Vorauswahl).
pub fn alts(lang: Lang, key: &str) -> Vec<String> {
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    let lower = key.to_lowercase();
    let upper = key != lower;
    let fix = |v: Vec<String>| -> Vec<String> {
        if upper {
            v.into_iter().map(|x| if x == "ß" { x } else { x.to_uppercase() }).collect()
        } else {
            v
        }
    };
    let mut c = key.chars();
    let (Some(first), None) = (c.next(), c.next()) else { return Vec::new() };
    if ('\u{0621}'..='\u{064A}').contains(&first) || "گڤچپ".contains(first) {
        let mut v = s(match key {
            "ا" => &["أ", "إ", "آ", "ٱ", "ء"],
            "ي" => &["ئ", "ى"],
            "و" => &["ؤ"],
            "ه" => &["ة"],
            "ل" => &["لا", "لأ", "لإ", "لآ"],
            "ك" => &["گ"],
            "ف" => &["ڤ"],
            "ج" => &["چ"],
            "ب" => &["پ"],
            "ء" => &["أ", "إ", "ؤ", "ئ"],
            _ => &[],
        });
        // Taschkil: Buchstabe mit Vokalzeichen
        for t in TASHKIL {
            v.push(format!("{key}{t}"));
        }
        return v;
    }
    let de = lang == Lang::De;
    fix(match lower.as_str() {
        "a" if de => s(&["ä", "à", "á", "â", "æ", "ã", "å"]),
        "a" => s(&["à", "á", "â", "ä", "æ", "ã", "å"]),
        "o" if de => s(&["ö", "ò", "ó", "ô", "õ", "ø", "œ"]),
        "o" => s(&["ò", "ó", "ô", "ö", "õ", "ø", "œ"]),
        "u" if de => s(&["ü", "ù", "ú", "û"]),
        "u" => s(&["ù", "ú", "û", "ü"]),
        "s" if de => s(&["ß", "ś", "š"]),
        "s" => s(&["ś", "š", "ß"]),
        "e" => s(&["é", "è", "ê", "ë", "€"]),
        "i" => s(&["í", "ì", "î", "ï"]),
        "c" => s(&["ç", "ć", "č"]),
        "n" => s(&["ñ", "ń"]),
        "y" => s(&["ý", "ÿ"]),
        "z" => s(&["ž", "ź", "ż"]),
        "." => {
            if lang == Lang::Ar {
                s(&["؟", "!", "؛", ":", "«", "»", "-"])
            } else {
                s(&["?", "!", ",", ";", ":", "'", "\"", "-"])
            }
        }
        "،" | "," => s(&[",", "،", "؛", ";"]),
        "-" => s(&["_", "–", "—"]),
        "$" => s(&["€", "£", "¥", "¢"]),
        "?" => s(&["¿", "؟"]),
        "!" => s(&["¡"]),
        "%" => s(&["‰"]),
        "/" => s(&["\\", "|"]),
        "@" => s(&[".com", ".de", ".org"]),
        _ => Vec::new(),
    })
}

/// Ist das eine indische/persische Ziffer (die Ultar nie anzeigt)?
pub fn is_foreign_digit(c: char) -> bool {
    ('\u{0660}'..='\u{0669}').contains(&c) || ('\u{06F0}'..='\u{06F9}').contains(&c)
}

/// Indische (٠–٩) und persische (۰–۹) Ziffern in 0–9 umwandeln.
pub fn normalize_digits(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\u{0660}'..='\u{0669}' => char::from(b'0' + (c as u32 - 0x0660) as u8),
            '\u{06F0}'..='\u{06F9}' => char::from(b'0' + (c as u32 - 0x06F0) as u8),
            _ => c,
        })
        .collect()
}

/// Maße der Tastatur in dp für einen Bildschirm.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Metrics {
    /// Breite der Tastatur (auf großen Bildschirmen schmaler, mittig)
    pub width: f32,
    pub toolbar: f32,
    pub number_h: f32,
    /// Zahlenreihe tatsächlich sichtbar (fällt weg, wenn zu wenig Höhe)
    pub number_row: bool,
    pub key_h: f32,
    pub gap_x: f32,
    pub gap_y: f32,
    /// Innenrand oben/unten und seitlich
    pub pad: f32,
    /// Gesamthöhe inklusive Werkzeugleiste
    pub total: f32,
}

/// Höchstanteil der Bildschirmhöhe für die Tastatur.
pub fn height_cap(w: f32, h: f32) -> f32 {
    let tablet = w.min(h) >= 600.0;
    if w > h {
        0.55
    } else if tablet {
        0.40
    } else {
        0.45
    }
}

/// size: 0 = klein, 1 = normal, 2 = groß.
pub fn metrics(w: f32, h: f32, number_row: bool, size: u8, pad_layout: bool) -> Metrics {
    let landscape = w > h;
    let tablet = w.min(h) >= 600.0;
    let f = [0.9f32, 1.0, 1.12][size.min(2) as usize];
    let (toolbar, number_h, key_base, key_min, gap_x, gap_y) = if landscape {
        (34.0, 30.0, 38.0, 30.0, 5.0, 5.0)
    } else if tablet {
        (44.0, 40.0, 54.0, 40.0, 7.0, 9.0)
    } else {
        (40.0, 36.0, 48.0, 38.0, 5.0, 7.0)
    };
    let key_base = key_base * f;
    let pad = 4.0;
    let width = if w > 760.0 { w.min(720.0).max(w * 0.6).min(w) } else { w };
    let cap = h * height_cap(w, h);
    let rows = 4.0;
    let fixed = |num: bool| pad * 2.0 + toolbar + if num { number_h + gap_y } else { 0.0 } + (rows - 1.0) * gap_y;
    let fit = |num: bool| (cap - fixed(num)) / rows;
    let mut num = number_row && !pad_layout;
    let mut key_h = key_base.min(fit(num));
    if key_h < key_min && num {
        num = false;
        key_h = key_base.min(fit(false));
    }
    let key_h = key_h.max(key_min * 0.85);
    let total = fixed(num) + rows * key_h;
    Metrics { width, toolbar, number_h, number_row: num, key_h, gap_x, gap_y, pad, total }
}

/// Rechteck in dp/px (gleiche Einheit wie die Eingabe).
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct R {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl R {
    pub fn cx(&self) -> f32 {
        self.x + self.w / 2.0
    }
    pub fn cy(&self) -> f32 {
        self.y + self.h / 2.0
    }
    pub fn right(&self) -> f32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct Placed {
    pub r: R,
    pub row: usize,
    pub def: KeyDef,
}

/// Grundeinheit: Anzahl Tasten, die eine volle Buchstabenreihe hat.
fn base_units(rows: &[Row]) -> f32 {
    rows.iter().filter(|r| r.kind == RowKind::Keys).map(|r| r.keys.len() as f32).fold(10.0, f32::max)
}

/// Tasten in die Fläche legen. `x, y, w` = Tastenfläche unter der Werkzeugleiste, alle Werte in derselben Einheit
/// (Pixel), `k` = Pixel je dp. Reihen mit weniger Gewicht werden mittig gesetzt (versetzte Reihe wie bei Gboard).
pub fn place(rows: &[Row], x: f32, y: f32, w: f32, m: &Metrics, k: f32) -> Vec<Placed> {
    let (gx, gy, pad) = (m.gap_x * k, m.gap_y * k, m.pad * k);
    let avail = w - 2.0 * pad;
    let units = base_units(rows);
    let unit = (avail + gx) / units; // Breite einer Taste inklusive Abstand
    let mut out = Vec::new();
    let mut cy = y + pad;
    for (ri, row) in rows.iter().enumerate() {
        let h = if row.kind == RowKind::Numbers { m.number_h * k } else { m.key_h * k };
        let weight: f32 = row.keys.iter().map(|k| k.weight).sum();
        // Untere Reihe und Ziffernblöcke füllen die Breite; Buchstabenreihen behalten die Grundeinheit
        let stretch = row.kind != RowKind::Keys || rows.iter().all(|r| r.keys.iter().all(|k| matches!(k.style, Style::Pad | Style::Func | Style::Action)));
        let u = if stretch || weight > units { (avail + gx) / weight } else { unit };
        let total = weight * u - gx;
        let mut cx = x + pad + (avail - total) / 2.0;
        for def in &row.keys {
            let kw = def.weight * u - gx;
            out.push(Placed { r: R { x: cx, y: cy, w: kw, h }, row: ri, def: def.clone() });
            cx += kw + gx;
        }
        cy += h + gy;
    }
    out
}

/// Taste unter dem Finger – lückenlos: jeder Punkt der Fläche gehört zur nächstgelegenen Taste.
/// `bias` verschiebt den Berührpunkt nach oben (Finger treffen meist etwas unter die Mitte).
pub fn hit(placed: &[Placed], x: f32, y: f32, bias: f32) -> Option<usize> {
    if placed.is_empty() {
        return None;
    }
    let y = y - bias;
    // Reihen-Bänder: Grenze in der Mitte zwischen zwei Reihen, ganz oben/unten offen
    let nrows = placed.iter().map(|p| p.row).max().unwrap_or(0) + 1;
    let span = |r: usize| {
        let ks = placed.iter().filter(|p| p.row == r);
        let top = ks.clone().map(|p| p.r.y).fold(f32::MAX, f32::min);
        let bot = ks.map(|p| p.r.bottom()).fold(f32::MIN, f32::max);
        (top, bot)
    };
    let mut row = nrows - 1;
    for r in 0..nrows - 1 {
        let (_, b) = span(r);
        let (t, _) = span(r + 1);
        if y < (b + t) / 2.0 {
            row = r;
            break;
        }
    }
    let mut best: Option<(usize, f32)> = None;
    for (i, p) in placed.iter().enumerate().filter(|(_, p)| p.row == row) {
        let d = if x < p.r.x {
            p.r.x - x
        } else if x >= p.r.right() {
            x - p.r.right() + 0.001
        } else {
            0.0
        };
        if best.map_or(true, |(_, bd)| d < bd) {
            best = Some((i, d));
        }
    }
    best.map(|b| b.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chars(rows: &[Row], kind: RowKind) -> Vec<String> {
        rows.iter().filter(|r| r.kind == kind).map(|r| r.keys.iter().filter_map(|k| k.text().map(String::from)).collect()).collect()
    }

    #[test]
    fn arabic_layout_is_gboard_standard() {
        let r = rows(Lang::Ar, Page::Letters, Purpose::Text, Shift::Off, true);
        assert_eq!(chars(&r, RowKind::Numbers), vec!["1234567890"]);
        assert_eq!(chars(&r, RowKind::Keys), vec!["ضصثقفغعهخحج", "شسيبلاتنمكط", "ذءؤرىةوزظد"]);
        // Löschen rechts in Reihe 3, kein Umschalter im Arabischen
        let r3 = &r[3].keys;
        assert_eq!(r3.last().unwrap().act, Act::Backspace);
        assert!(r3.iter().all(|k| k.act != Act::Shift));
        // untere Reihe: ؟123 · ، · Sprache · Leertaste · . · Aktion
        let b: Vec<_> = r.last().unwrap().keys.iter().map(|k| k.act.clone()).collect();
        assert_eq!(b, vec![Act::ToSymbols, Act::Text("،".into()), Act::Lang, Act::Space, Act::Text(".".into()), Act::Action]);
        assert_eq!(r.last().unwrap().keys[0].label, "123؟"); // wird rechts-nach-links als „؟123“ gezeigt
        assert_eq!(r.last().unwrap().keys[3].label, "العربية");
    }

    #[test]
    fn latin_layouts() {
        let en = rows(Lang::En, Page::Letters, Purpose::Text, Shift::Off, true);
        assert_eq!(chars(&en, RowKind::Keys), vec!["qwertyuiop", "asdfghjkl", "zxcvbnm"]);
        let de = rows(Lang::De, Page::Letters, Purpose::Text, Shift::Once, false);
        assert_eq!(chars(&de, RowKind::Keys), vec!["QWERTZUIOP", "ASDFGHJKL", "YXCVBNM"]);
        assert!(chars(&de, RowKind::Numbers).is_empty());
        // ohne Zahlenreihe: Ziffern als Nebenzeichen der oberen Reihe
        let hints: String = de[0].keys.iter().filter_map(|k| k.hint.clone()).collect();
        assert_eq!(hints, "1234567890");
        assert_eq!(de[2].keys.first().unwrap().act, Act::Shift);
        assert_eq!(de[2].keys.last().unwrap().act, Act::Backspace);
    }

    #[test]
    fn umlauts_by_long_press() {
        assert_eq!(alts(Lang::De, "u")[0], "ü");
        assert_eq!(alts(Lang::De, "a")[0], "ä");
        assert_eq!(alts(Lang::De, "o")[0], "ö");
        assert_eq!(alts(Lang::De, "s")[0], "ß");
        assert_eq!(alts(Lang::De, "U")[0], "Ü");
        assert_eq!(alts(Lang::Ar, "ا")[0], "أ");
        assert!(alts(Lang::Ar, "ب").contains(&"ب\u{064E}".to_string()));
        assert!(alts(Lang::Ar, "ل").contains(&"لا".to_string()));
    }

    #[test]
    fn never_indian_digits() {
        for lang in Lang::ALL {
            for page in [Page::Letters, Page::Symbols, Page::Symbols2] {
                for p in [Purpose::Text, Purpose::Pin, Purpose::Phone, Purpose::Number, Purpose::Email, Purpose::Url] {
                    for num in [true, false] {
                        for r in rows(lang, page, p, Shift::Off, num) {
                            for k in r.keys {
                                let all = format!("{}{}{}", k.label, k.text().unwrap_or(""), k.hint.clone().unwrap_or_default());
                                assert!(!all.chars().any(is_foreign_digit), "{lang:?} {page:?} {p:?}: {all}");
                            }
                        }
                    }
                }
            }
        }
        assert_eq!(normalize_digits("٠١٢٣٤٥٦٧٨٩ ۱۲"), "0123456789 12");
    }

    #[test]
    fn purposes() {
        assert_eq!(purpose_for("simpin"), Purpose::Pin);
        assert_eq!(purpose_for("sec_pw2"), Purpose::Password);
        assert_eq!(purpose_for("ghtok"), Purpose::Password);
        assert_eq!(purpose_for("tg_phone"), Purpose::Phone);
        assert_eq!(purpose_for("cnum"), Purpose::Phone);
        assert_eq!(purpose_for("sms_to"), Purpose::Phone);
        assert_eq!(purpose_for("q"), Purpose::Search);
        assert_eq!(purpose_for("help_q"), Purpose::Search);
        assert_eq!(purpose_for("host"), Purpose::Url);
        assert_eq!(purpose_for("chat"), Purpose::Send);
        assert_eq!(purpose_for("cname"), Purpose::Text);
    }

    const SCREENS: [(f32, f32); 8] = [(360.0, 780.0), (360.0, 640.0), (780.0, 360.0), (640.0, 360.0), (914.0, 571.0), (571.0, 914.0), (600.0, 960.0), (1280.0, 800.0)];

    fn all_layouts() -> Vec<Vec<Row>> {
        let mut v = Vec::new();
        for lang in Lang::ALL {
            for page in [Page::Letters, Page::Symbols, Page::Symbols2] {
                for num in [true, false] {
                    v.push(rows(lang, page, Purpose::Text, Shift::Off, num));
                }
            }
        }
        for p in [Purpose::Pin, Purpose::Phone, Purpose::Number] {
            v.push(rows(Lang::Ar, Page::Letters, p, Shift::Off, true));
        }
        v
    }

    #[test]
    fn metrics_fit_every_screen() {
        for (w, h) in SCREENS {
            for size in 0..3 {
                let m = metrics(w, h, true, size, false);
                assert!(m.total <= h * height_cap(w, h) + 0.5 || m.key_h >= 30.0 * 0.85, "{w}x{h}: {m:?}");
                assert!(m.total <= h * 0.6, "{w}x{h}: zu hoch {}", m.total);
                assert!(m.key_h >= 30.0 * 0.85, "{w}x{h}: Taste zu niedrig {}", m.key_h);
                assert!(m.width <= w);
            }
        }
        // A30 hochkant: Zahlenreihe bleibt, Tasten 48 dp
        let a30 = metrics(360.0, 780.0, true, 1, false);
        assert!(a30.number_row && (a30.key_h - 48.0).abs() < 0.01, "{a30:?}");
    }

    #[test]
    fn keys_inside_no_overlap_no_dead_zones() {
        for (w, h) in SCREENS {
            for rows_ in all_layouts() {
                let num = rows_.iter().any(|r| r.kind == RowKind::Numbers);
                let pad = rows_.iter().all(|r| r.kind != RowKind::Bottom);
                let m = metrics(w, h, num, 1, pad);
                if num && !m.number_row {
                    continue; // diese Kombination zeigt die Shell ohne Zahlenreihe
                }
                let k = 2.0;
                let (x0, y0, aw) = (((w - m.width) / 2.0) * k, 100.0, m.width * k);
                let p = place(&rows_, x0, y0, aw, &m, k);
                let ah = (m.total - m.toolbar) * k;
                for (i, a) in p.iter().enumerate() {
                    assert!(a.r.x >= x0 - 0.01 && a.r.right() <= x0 + aw + 0.01, "{w}x{h} Taste {} ragt raus", a.def.label);
                    assert!(a.r.y >= y0 - 0.01 && a.r.bottom() <= y0 + ah + 0.5, "{w}x{h} Taste {} unten raus: {} > {}", a.def.label, a.r.bottom(), y0 + ah);
                    assert!(a.r.w > 20.0 * k * 0.8, "{w}x{h} Taste {} zu schmal: {}", a.def.label, a.r.w / k);
                    for b in &p[i + 1..] {
                        let ov = a.r.x < b.r.right() - 0.01 && b.r.x < a.r.right() - 0.01 && a.r.y < b.r.bottom() - 0.01 && b.r.y < a.r.bottom() - 0.01;
                        assert!(!ov, "{w}x{h}: {} überlappt {}", a.def.label, b.def.label);
                    }
                    // Mitte trifft die eigene Taste
                    assert_eq!(hit(&p, a.r.cx(), a.r.cy(), 0.0), Some(i), "{w}x{h}: Mitte von {}", a.def.label);
                }
                // jeder Punkt (auch in den Lücken) trifft eine Taste, und zwar eine nahe
                let mut yy = y0;
                while yy < y0 + ah {
                    let mut xx = x0;
                    while xx < x0 + aw {
                        let i = hit(&p, xx, yy, 0.0).expect("tote Zone");
                        let r = p[i].r;
                        let dx = if xx < r.x { r.x - xx } else if xx > r.right() { xx - r.right() } else { 0.0 };
                        assert!(dx <= (m.gap_x * k).max(r.w), "{w}x{h}: Punkt {xx},{yy} -> ferne Taste {}", p[i].def.label);
                        xx += 6.0;
                    }
                    yy += 6.0;
                }
            }
        }
    }

    #[test]
    fn function_keys_stay_in_place() {
        // Löschen immer ganz rechts in der letzten Buchstabenreihe, Aktion ganz rechts unten, Umschalter links unten
        for lang in Lang::ALL {
            let r = rows(lang, Page::Letters, Purpose::Text, Shift::Off, true);
            let m = metrics(360.0, 780.0, true, 1, false);
            let p = place(&r, 0.0, 0.0, 360.0, &m, 1.0);
            let last_keys_row = r.len() - 2;
            let bk = p.iter().filter(|k| k.row == last_keys_row).last().unwrap();
            assert_eq!(bk.def.act, Act::Backspace);
            assert!(bk.r.right() > 350.0, "{lang:?}");
            let act = p.last().unwrap();
            assert_eq!(act.def.act, Act::Action);
            let tog = p.iter().find(|k| k.def.act == Act::ToSymbols).unwrap();
            assert!(tog.r.x < 10.0);
        }
    }

    #[test]
    fn language_cycle_starts_with_system_language() {
        assert_eq!(Lang::Ar.next(Lang::Ar), Lang::En);
        assert_eq!(Lang::En.next(Lang::Ar), Lang::De);
        assert_eq!(Lang::De.next(Lang::Ar), Lang::Ar);
        assert_eq!(Lang::De.next(Lang::De), Lang::Ar);
    }
}
