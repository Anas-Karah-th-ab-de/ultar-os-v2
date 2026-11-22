use ultar_ui::text::{isolate, FontId, Overflow, TextEngine, TextLayout, TextStyle};
use ultar_ui::Lang;

/// Visual string: the characters of each glyph's cluster, glyphs left to right.
fn visual(text: &str, l: &TextLayout) -> String {
    let mut starts: Vec<usize> = l.glyphs.iter().map(|g| g.cluster as usize).collect();
    starts.sort_unstable();
    starts.dedup();
    let end_of = |c: usize| {
        starts
            .iter()
            .copied()
            .find(|s| *s > c)
            .unwrap_or(l.text.len())
    };
    let mut out = String::new();
    let mut last = usize::MAX;
    for g in &l.glyphs {
        let c = g.cluster as usize;
        if c == last {
            continue;
        }
        last = c;
        let piece = &l.text[c..end_of(c)];
        let piece: String = piece
            .chars()
            .filter(|ch| !ultar_ui::text::is_bidi_control(*ch))
            .collect();
        out.push_str(&piece);
        out.push('|');
    }
    let _ = text;
    out
}

fn show(e: &mut TextEngine, name: &str, text: &str, lang: Lang) -> std::rc::Rc<TextLayout> {
    let st = TextStyle::new(14.0, lang);
    let l = e.layout(text, &st, None);
    println!(
        "A {name} [{lang:?}] {text:?}\n   visual L→R: {}",
        visual(text, &l)
    );
    l
}

#[test]
fn arabic_hard_list() {
    let mut e = TextEngine::new().unwrap();
    let ar = Lang::Ar;
    // shaping
    for t in [
        "لا",
        "لأ",
        "لإ",
        "لآ",
        "بلا",
        "كلام",
        "لَا",
        "الله",
        "لله",
        "اللّٰه",
        "ﷲ",
        "ســـلام",
        "سلام",
        "بَّ",
        "بًّ",
        "بِّ",
        "يُّ",
        "مُحَمَّدٌ",
        "ـــ",
        "ه",
        "هه",
        "ههه",
        "ك",
        "كك",
        "ي",
        "ئ",
        "لل",
        "لم",
        "في",
        "تج",
        "٪",
        "؟",
        "،",
        "؛",
        "٠١٢٣",
        "ﻻ",
        "گ پ چ ژ",
        "ٱ",
    ] {
        let st = TextStyle::new(14.0, ar);
        let l = e.layout(t, &st, None);
        let g: Vec<String> = l
            .glyphs
            .iter()
            .map(|g| {
                format!(
                    "{:?}:{}@{:.1}+{:.1}(dx{:.1},dy{:.1})c{}",
                    g.font, g.id, g.x, g.advance, g.dx, g.dy, g.cluster
                )
            })
            .map(|s| s.replace("Arabic", "A").replace("Latin", "L"))
            .collect();
        let nominal: Vec<Option<u16>> = t
            .chars()
            .map(|c| e.nominal_glyph(FontId::Arabic, c))
            .collect();
        println!(
            "S {t:?} chars {} glyphs {} w {:.2}: {} | nominal {:?}",
            t.chars().count(),
            l.glyphs.len(),
            l.width,
            g.join(" "),
            nominal
        );
    }
    // bidi
    let cases = [
        ("paren", "سلام (عالم) مرحبا"),
        ("percent", "النسبة 50%"),
        ("percent-ar-first", "50% من القرص"),
        ("phone", "هاتف: +963 11 123 4567"),
        ("url", "الموقع https://code-craft.tech/ar/page?id=5 متاح"),
        ("url-trailing-slash", "افتح https://example.com/ الآن"),
        ("email", "البريد info@code-craft.tech."),
        ("latin-word", "تطبيق Telegram مثبت"),
        ("latin-num", "قال John 25 years مرحبا"),
        ("mixed-paren", "Ultar OS (نسخة 2)"),
        ("paren-latin-in-ar", "الشبكة (Wi-Fi) متصلة"),
        ("paren-num", "ملفات (12) جديدة"),
        ("resolution", "الدقة 3840 × 2160"),
        ("hz", "معدل 60 Hz"),
        ("range", "من 10-20 عنصر"),
        ("range-spaced", "من 10 - 20 عنصر"),
        ("date", "التاريخ 2026/11/23"),
        ("time", "الساعة 10:30"),
        ("neg", "الحرارة -5°"),
        ("neutral-end", "مرحبا!"),
        ("neutral-start-latin", "Hello!"),
        ("latin-trailing-dot", "KM Code Craft."),
        ("version", "الإصدار 0.7.0"),
        ("ip", "العنوان 192.168.100.16"),
        ("num-ar", "5 ملفات"),
        ("digits", "0123456789"),
        ("brackets", "[أ] {ب} <ج> «د»"),
        ("slash", "نعم/لا"),
        ("ltr-in-rtl-in-ltr", "A ب C د E"),
        ("quote", "قال \"hello world\" ثم"),
    ];
    for (name, t) in cases {
        show(&mut e, name, t, ar);
    }
    for (name, t) in [
        ("en-with-ar", "Name: محمد (Admin)"),
        ("en-ar-num", "File ملف 12 saved"),
        ("en-ar-end", "Hello مرحبا!"),
    ] {
        show(&mut e, name, t, Lang::En);
    }
    // isolates
    let iso = |v: &str| isolate(v);
    for (name, t) in [
        ("iso-resolution", format!("الدقة {}", iso("3840 × 2160"))),
        ("iso-phone", format!("هاتف: {}", iso("+963 11 123 4567"))),
        ("iso-hz", format!("معدل {}", iso("60 Hz"))),
        (
            "iso-recommended",
            format!("{} (موصى بها)", iso("3840 × 2160")),
        ),
        ("iso-neg", format!("الحرارة {}", iso("-5°"))),
        (
            "iso-url",
            format!("افتح {} الآن", iso("https://example.com/")),
        ),
        (
            "iso-arabic-in-en",
            format!("{} (recommended)", iso("شاشة 1")),
        ),
        (
            "iso-nested",
            format!("أ {} ب", iso(&format!("x {} y", iso("ج 1")))),
        ),
        ("iso-empty", format!("أ {} ب", iso(""))),
        ("iso-unclosed", "أ \u{2068}abc 12".to_string()),
    ] {
        show(&mut e, name, &t, ar);
    }
    // mirroring: glyph ids of parens
    let st = TextStyle::new(14.0, ar);
    let l = e.layout("(ب)", &st, None);
    let open = e.nominal_glyph(FontId::Latin, '(');
    let close = e.nominal_glyph(FontId::Latin, ')');
    let open_a = e.nominal_glyph(FontId::Arabic, '(');
    let close_a = e.nominal_glyph(FontId::Arabic, ')');
    println!(
        "M nominal ( L{open:?} A{open_a:?}  ) L{close:?} A{close_a:?}; visual glyphs {:?}",
        l.glyphs
            .iter()
            .map(|g| (g.font, g.id, g.cluster, g.rtl))
            .collect::<Vec<_>>()
    );
    let l = e.layout("(a)", &st, None);
    println!(
        "M latin in rtl para: {:?}",
        l.glyphs
            .iter()
            .map(|g| (g.font, g.id, g.cluster, g.rtl))
            .collect::<Vec<_>>()
    );
    for t in ["«ب»", "<ب>", "[ب]", "{ب}", "ب ← ج", "١ < ٢"] {
        let l = e.layout(t, &st, None);
        let nom: Vec<_> = t
            .chars()
            .map(|c| {
                (
                    c,
                    e.nominal_glyph(FontId::Latin, c),
                    e.nominal_glyph(FontId::Arabic, c),
                )
            })
            .collect();
        println!(
            "M {t:?}: {:?} nominal {:?}",
            l.glyphs
                .iter()
                .map(|g| (g.font, g.id, g.cluster))
                .collect::<Vec<_>>(),
            nom
        );
    }
    // ellipsis
    for (name, t, lang) in [
        ("ell-ar", "هذا نص عربي طويل جدا يجب أن يقطع".to_string(), ar),
        (
            "ell-latin-in-rtl",
            "Hello wonderful long world".to_string(),
            ar,
        ),
        (
            "ell-latin-iso-in-rtl",
            iso("Hello wonderful long world"),
            ar,
        ),
        (
            "ell-ar-then-latin",
            "الشبكة اللاسلكية Wonderful-Network-5G".to_string(),
            ar,
        ),
        (
            "ell-ar-then-iso",
            format!("الدقة الموصى بها {}", iso("3840 × 2160 (16:9)")),
            ar,
        ),
        (
            "ell-ar-in-ltr",
            "هذا نص عربي طويل جدا يجب أن يقطع".to_string(),
            Lang::En,
        ),
        ("ell-num", "الرقم 1234567890123456789".to_string(), ar),
        ("ell-tashkeel", "مُحَمَّدٌ رَسُولُ اللَّهِ صَلَّى".to_string(), ar),
        ("ell-lamalef", "بلا بلا بلا بلا بلا بلا".to_string(), ar),
        ("ell-newline", "سطر أول\nسطر ثان".to_string(), ar),
        ("ell-newline-short", "أ\nب".to_string(), ar),
    ] {
        let st = TextStyle::new(14.0, lang).with_overflow(Overflow::Ellipsis);
        let full = e.layout(&t, &TextStyle::new(14.0, lang), None);
        for max in [70.0f32, 110.0] {
            let l = e.layout(&t, &st, Some(max));
            let ell = l
                .glyphs
                .iter()
                .find(|g| l.text[g.cluster as usize..].starts_with('…'))
                .map(|g| g.x);
            println!("E {name} [{lang:?}] max {max} full {:.1} -> w {:.1} trunc {} text {:?}\n   visual: {} ellipsis x {:?}", full.width, l.width, l.truncated, l.text, visual(&t, &l), ell);
        }
    }
    // wrap
    let st = TextStyle::new(14.0, ar).with_overflow(Overflow::Wrap);
    let t = "اسحب الشاشات لتطابق أماكنها على مكتبك (3840 × 2160) اختر شاشة لتغيير إعداداتها Supercalifragilisticexpialidocious";
    let l = e.layout(t, &st, Some(120.0));
    for line in &l.lines {
        println!(
            "W line {:?} w {:.1}: {:?}",
            line.bytes,
            line.width,
            &l.text[line.bytes.clone()]
        );
    }
}
