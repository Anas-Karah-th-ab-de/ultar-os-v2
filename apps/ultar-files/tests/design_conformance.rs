//! Design conformance of the Files window and the desktop icons against the boards
//! (design/boards/project/Main.dc.html; DesktopTop has the same parts): sizes, gaps and
//! paddings read from the board's styles, accessible names against the board's labels.
mod common;

use common::*;
use ultar_files::action::Target;
use ultar_files::Mode;
use ultar_i18n::Lang;
use ultar_ui::ui::NodeId;

const BOARDS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../design/boards/project/");

fn board(name: &str) -> String {
    std::fs::read_to_string(format!("{BOARDS}{name}.dc.html")).unwrap()
}

/// The Files section of a board.
fn files_section(html: &str) -> String {
    let start = html
        .find("<section aria-label=\"الملفات\"")
        .expect("Files window on the board");
    let end = html[start..].find("</section>").unwrap() + start;
    html[start..end].to_owned()
}

/// The number before `px` after `prop: ` in the style of the tag containing `anchor`.
fn px(html: &str, anchor: &str, prop: &str) -> f32 {
    let at = html
        .find(anchor)
        .unwrap_or_else(|| panic!("{anchor} not on the board"));
    let tag_start = html[..at + 1].rfind('<').unwrap_or(0);
    let tag_end = html[at..].find('>').map_or(html.len(), |e| e + at);
    let tag = &html[tag_start..tag_end];
    let key = format!("{prop}: ");
    let i = tag
        .find(&format!(" {key}"))
        .or_else(|| tag.find(&format!("\"{key}")))
        .map(|i| i + 1)
        .unwrap_or_else(|| panic!("{prop} missing near {anchor}: {tag}"))
        + key.len();
    let n: String = tag[i..]
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    n.parse().unwrap()
}

/// Accessible name of a node, without direction isolates.
fn name(app: &ultar_files::App, id: NodeId) -> String {
    let n = app.ui.a11y_node(id).unwrap();
    let label = n.name.unwrap();
    label
        .resolve(app.ui.translator())
        .chars()
        .filter(|c| !matches!(c, '\u{2066}'..='\u{2069}'))
        .collect()
}

fn board_root() -> Root {
    let r = Root::new();
    for d in ["مشاريع", "عقود", "فواتير"] {
        std::fs::create_dir_all(r.path(&format!("documents/{d}"))).unwrap();
    }
    for f in ["تقرير.docx", "ميزانية.xlsx", "مخطط.png"] {
        r.write(&format!("documents/{f}"), b"x");
    }
    r
}

#[test]
fn the_window_matches_the_board() {
    for b in ["Main", "DesktopTop"] {
        let html = files_section(&board(b));
        let r = board_root();
        let (mut app, _) = app(&r, Mode::Window, Lang::Ar, Some("files://documents/"));
        app.ui.layout();
        let width = px(&html, "aria-label=\"الملفات\"", "width");
        let height = px(&html, "aria-label=\"الملفات\"", "height");
        let titlebar = px(&html, "<header", "height");
        assert_eq!(
            (app.ui.size().w, app.ui.size().h),
            (width, height - titlebar),
            "{b}: content size"
        );
        // sidebar: width, padding, rows, gap (Arabic: at the right edge)
        let nav_w = px(&html, "aria-label=\"الأماكن\"", "width");
        let pad = px(&html, "aria-label=\"الأماكن\"", "padding");
        let gap = px(&html, "aria-label=\"الأماكن\"", "gap");
        let row_h = px(&html, "data-act=\"files.place\"", "height");
        let p0 = app.ui.rect(node(&app, &Target::Place(0))).unwrap();
        let p1 = app.ui.rect(node(&app, &Target::Place(1))).unwrap();
        assert_eq!(
            (p0.x + p0.w, p0.y, p0.w, p0.h),
            // border-box: the 1-point border-inline-end is inside the 208 as well
            (width - pad, pad, nav_w - 2.0 * pad - 1.0, row_h),
            "{b}: first place"
        );
        assert_eq!(p1.y - p0.y, row_h + gap, "{b}: place gap");
        let eject = app.ui.rect(node(&app, &Target::Eject(0))).unwrap();
        let eject_edge = px(&html, "data-act=\"files.eject\"", "width");
        assert_eq!(
            (eject.w, eject.h),
            (eject_edge, eject_edge),
            "{b}: eject button"
        );
        // toolbar, search, new folder
        let bar_h = px(&html, "height: 48px", "height");
        let search = app.ui.rect(node(&app, &Target::Search)).unwrap();
        let search_w = px(&html, "gap: 4px; width:", "width");
        assert_eq!(search.w, search_w, "{b}: search width");
        assert!(
            (search.y + search.h / 2.0 - bar_h / 2.0).abs() < 0.5,
            "{b}: search centred in the toolbar"
        );
        let new = app.ui.rect(node(&app, &Target::NewFolder)).unwrap();
        assert_eq!(
            (new.w, new.h),
            (32.0, px(&html, "data-act=\"files.newfolder\"", "height")),
            "{b}: new folder"
        );
        check_tiles(&app, &html, b, (width, nav_w, bar_h));
    }
}

#[test]
fn every_board_label_is_the_accessible_name() {
    let html = files_section(&board("Main"));
    let r = board_root();
    let (app, _) = app(&r, Mode::Window, Lang::Ar, Some("files://documents/"));
    let mut names: Vec<String> = app
        .built()
        .targets
        .keys()
        .filter_map(|id| {
            app.ui
                .a11y_node(*id)
                .and_then(|n| n.name)
                .map(|_| name(&app, *id))
        })
        .collect();
    names.sort();
    for label in [
        "رجوع",
        "إلى الأمام",
        "المجلد الرئيسي",
        "المستندات",
        "التنزيلات",
        "الصور",
        "الموسيقى",
        "ذاكرة USB",
        "إخراج ذاكرة USB",
        "سلة المحذوفات",
        "ابحث في المستندات",
        "مجلد جديد",
        "مشاريع",
        "عقود",
        "فواتير",
        "تقرير.docx",
        "ميزانية.xlsx",
        "مخطط.png",
    ] {
        assert!(html.contains(label), "board label {label}");
        assert!(
            names.iter().any(|n| n == label),
            "no element named {label}: {names:?}"
        );
    }
}

#[test]
fn the_desktop_icons_match_the_board() {
    for b in ["Main", "DesktopTop"] {
        let html = board(b);
        let at = html.find("data-act=\"desk.icon\"").unwrap();
        let column = &html[html[..at].rfind("<div").unwrap()..at];
        let top = px(column, "top:", "top");
        let start = px(column, "inset-inline-start", "inset-inline-start");
        let gap = px(column, "gap:", "gap");
        let r = board_root();
        let (mut app, _) = app(&r, Mode::Desk, Lang::Ar, None);
        app.ui.layout();
        let home = app.ui.rect(node(&app, &Target::Place(0))).unwrap();
        let usb = app.ui.rect(node(&app, &Target::Drive(0))).unwrap();
        let w = app.ui.size().w;
        assert_eq!(
            (w - home.x - home.w, if b == "Main" { home.y } else { top }),
            (start, top),
            "{b}: first icon"
        );
        assert_eq!(
            home.w,
            px(&html, "data-act=\"desk.icon\"", "width"),
            "{b}: icon width"
        );
        assert_eq!(usb.y - (home.y + home.h), gap, "{b}: icon gap");
        assert_eq!(name(&app, node(&app, &Target::Place(0))), "المجلد الرئيسي");
        assert_eq!(name(&app, node(&app, &Target::Drive(0))), "ذاكرة USB");
    }
}

fn check_tiles(
    app: &ultar_files::App,
    html: &str,
    b: &str,
    (width, nav_w, bar_h): (f32, f32, f32),
) {
    // tiles: 104 wide, 8 apart, 12 from the edges, below the toolbar line
    let tile_w = px(html, "data-act=\"files.item\"", "width");
    let first = app.built().item_nodes[0].0;
    let second = app.built().item_nodes[1].0;
    let (t0, t1) = (app.ui.rect(first).unwrap(), app.ui.rect(second).unwrap());
    assert_eq!(t0.w, tile_w, "{b}: tile width");
    assert_eq!(
        t0.x + t0.w,
        width - nav_w - 12.0,
        "{b}: first tile at the start edge"
    );
    assert_eq!(
        t0.y,
        bar_h + 1.0 + 12.0,
        "{b}: first tile below the toolbar"
    );
    assert_eq!(t0.x - (t1.x + t1.w), 8.0, "{b}: tile gap");
    assert_eq!(
        app.built().item_nodes.len(),
        html.matches("data-act=\"files.item\"").count(),
        "{b}: items"
    );
}
