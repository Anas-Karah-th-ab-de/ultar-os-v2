//! Walks through the public API the way other crates of Ultar OS use it.

use std::cmp::Ordering;

use ultar_i18n::calendar::{Date, WeekStart, Weekday};
use ultar_i18n::datefmt::{self, ClockStyle, MonthNames};
use ultar_i18n::{
    collation_key, compare, contains_rtl, direction, first_strong_direction, normalize_for_search,
    numbers, units, Arg, Catalog, Direction, Lang, ParseErrorKind, PhysicalSide, Side, Translator,
};

#[test]
fn installer_texts_in_three_languages() {
    let mut tr = Translator::new(Lang::Ar);
    assert_eq!(tr.t("installer.welcome"), "مرحباً بك في Ultar OS");
    assert_eq!(
        tr.t_args(
            "installer.step_of",
            &[("current", Arg::Int(2)), ("total", Arg::Int(5))]
        ),
        "الخطوة 2 من 5"
    );
    tr.set_lang(Lang::En);
    assert_eq!(tr.t("installer.confirm.button"), "Erase and install");
    tr.set_lang(Lang::De);
    assert_eq!(tr.t("installer.disk.title"), "Wohin soll Ultar OS?");
    assert_eq!(
        tr.t_args(
            "installer.step_of",
            &[("current", 2.into()), ("total", 5.into())]
        ),
        "Schritt 2 von 5"
    );
}

#[test]
fn arabic_plurals_use_all_six_forms() {
    let tr = Translator::new(Lang::Ar);
    let files = |n: i64| tr.t_args("count.files", &[("count", Arg::Int(n))]);
    assert_eq!(files(0), "لا ملفات");
    assert_eq!(files(1), "ملف واحد");
    assert_eq!(files(2), "ملفان");
    assert_eq!(files(3), "3 ملفات");
    assert_eq!(files(11), "11 ملفاً");
    assert_eq!(files(100), "100 ملف");
    assert_eq!(files(103), "103 ملفات");
    assert_eq!(
        tr.t_args("time.minutes_ago", &[("count", Arg::Int(2))]),
        "قبل دقيقتين"
    );
    let en = Translator::new(Lang::En);
    assert_eq!(
        en.t_args("count.files", &[("count", Arg::Int(1))]),
        "1 file"
    );
    assert_eq!(
        en.t_args("count.files", &[("count", Arg::Grouped(2500))]),
        "2,500 files"
    );
    let de = Translator::new(Lang::De);
    assert_eq!(de.t_args("count.days", &[("count", Arg::Int(0))]), "0 Tage");
}

#[test]
fn app_brings_its_own_catalog() {
    let mut tr = Translator::new(Lang::Ar);
    tr.add_namespaced_source(Lang::Ar, "settings.display", "night = الوضع الليلي\n")
        .unwrap();
    tr.add_namespaced_source(
        Lang::En,
        "settings.display",
        "night = Night mode\nextra = Extra\n",
    )
    .unwrap();
    assert_eq!(tr.t("settings.display.night"), "الوضع الليلي");
    assert_eq!(tr.t("settings.display.title"), "الشاشة");
    assert_eq!(tr.t("settings.display.extra"), "settings.display.extra");
    let error = tr
        .add_namespaced_source(Lang::De, "settings.display", "night = {oops")
        .unwrap_err();
    assert_eq!(error.kind, ParseErrorKind::UnclosedBrace);
    assert_eq!(tr.t(error.kind.message_key()), "قوس مفتوح لم يُغلق");
}

#[test]
fn error_and_hardware_messages() {
    let tr = Translator::new(Lang::Ar);
    assert_eq!(
        tr.t_args("error.file_not_found", &[("name", Arg::Str("تقرير.pdf"))]),
        "الملف «تقرير.pdf» غير موجود."
    );
    assert_eq!(tr.t("hw.class.wifi"), "شبكة لاسلكية");
    assert_eq!(tr.t("hw.state.driver_missing"), "لا يوجد تعريف بعد");
    assert_eq!(tr.t("edition.server"), "إصدار الخادم");
    assert_eq!(tr.t("form.laptop"), "حاسوب محمول");
    assert_eq!(units::format_size(3 * 1024 * 1024, &tr), "3.0 ميغابايت");
    assert_eq!(units::format_percent(40, &tr), "40%");
}

#[test]
fn layout_follows_the_language() {
    assert_eq!(direction(Lang::Ar), Direction::Rtl);
    assert_eq!(
        Side::Start.resolve(direction(Lang::Ar)),
        PhysicalSide::Right
    );
    assert_eq!(Side::End.resolve(direction(Lang::En)), PhysicalSide::Right);
    assert!(contains_rtl("Ultar – نظام"));
    assert_eq!(first_strong_direction("Ultar – نظام"), Some(Direction::Ltr));
    assert_eq!(first_strong_direction("2026"), None);
}

#[test]
fn numbers_dates_and_week() {
    assert_eq!(numbers::to_western_digits("٢٠٢٦/١١/٢٢"), "2026/11/22");
    assert_eq!(numbers::parse_int("۱۲۳"), Ok(123));
    assert_eq!(numbers::format_int(1_500_000, Lang::Ar), "1,500,000");
    let ar = Translator::new(Lang::Ar);
    let date = Date::new(2026, 11, 22).unwrap();
    assert_eq!(
        datefmt::format_date_long(date, MonthNames::Levantine, &ar),
        "22 تشرين الثاني 2026"
    );
    assert_eq!(
        datefmt::format_hijri_long(date.to_hijri().unwrap(), &ar),
        "11 جمادى الآخرة 1448 هـ"
    );
    assert_eq!(datefmt::format_time(21, 15, ClockStyle::H12, &ar), "9:15 م");
    assert_eq!(
        WeekStart::default_for(Lang::Ar).days()[0],
        Weekday::Saturday
    );
    let header: Vec<String> = WeekStart::Saturday
        .days()
        .iter()
        .map(|day| datefmt::weekday_short_name(*day, &ar))
        .collect();
    assert_eq!(
        header,
        ["سبت", "أحد", "اثنين", "ثلاثاء", "أربعاء", "خميس", "جمعة"]
    );
}

#[test]
fn search_and_sort_arabic_names() {
    assert_eq!(
        normalize_for_search("الإِعْدَادَات"),
        normalize_for_search("الاعدادات")
    );
    assert_eq!(normalize_for_search("KM Code Craft"), "km code craft");
    let mut contacts = vec!["يوسف", "Zaid", "أنس", "محمد", "Adam", "إيمان", "خالد"];
    contacts.sort_by(|a, b| compare(a, b));
    assert_eq!(
        contacts,
        ["أنس", "إيمان", "خالد", "محمد", "يوسف", "Adam", "Zaid"]
    );
    contacts.sort_by_key(|name| collation_key(name));
    assert_eq!(contacts[0], "أنس");
    assert_eq!(compare("سلام", "سَلام"), Ordering::Less);
}

#[test]
fn catalog_format_features() {
    let source = "# comment\ngreeting = مرحباً {name}\n    سطر ثانٍ\nbrace = {\"{\"}x{\"}\"}\n";
    let catalog = Catalog::parse(source).unwrap();
    let greeting = catalog.get("greeting").unwrap();
    assert_eq!(
        greeting.format(Lang::Ar, &[("name", Arg::Str("أنس"))]),
        "مرحباً أنس\nسطر ثانٍ"
    );
    assert_eq!(catalog.get("brace").unwrap().as_static_text(), Some("{x}"));
    assert_eq!(
        Catalog::parse("a b = c").unwrap_err().kind,
        ParseErrorKind::InvalidKey
    );
}

#[test]
fn language_detection_values() {
    assert_eq!(Lang::parse("ar_SY.UTF-8"), Some(Lang::Ar));
    assert_eq!(
        Lang::from_candidates(["", "C", "de_DE.UTF-8"]),
        Some(Lang::De)
    );
    assert_eq!("en-GB".parse::<Lang>(), Ok(Lang::En));
    assert!("tr".parse::<Lang>().is_err());
}
