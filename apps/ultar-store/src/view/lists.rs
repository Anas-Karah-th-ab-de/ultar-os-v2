//! The remaining areas: Categories, Search results, Installed, Updates,
//! Install-from-a-file, Offline sources, Account. The boards draw Discover,
//! the app page and the states; these follow their language: list rows,
//! badges, token spacing, every action bound.

use super::{app_name, chrome, loc_label, muted, rows, size_text, spacer, HitMap};
use crate::action::StoreAction;
use crate::state::{Area, Store};
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex, Sizing};
use ultar_ui::paint::TextSpec;
use ultar_ui::tokens::{ColorRole, Space, TypeSize};
use ultar_ui::ui::{NodeId, Ui, UiError};
use ultar_ui::widgets::{
    Button, ButtonVariant, EmptyState, InfoBar, Label, List, ListItem, Text, Tone,
};

pub(crate) fn categories_page(
    ui: &mut Ui,
    parent: NodeId,
    store: &Store,
    hits: &mut HitMap,
) -> Result<(), UiError> {
    if store.catalog.data.is_none() {
        return chrome::page_title_only(ui, parent);
    }
    match store.category.as_deref() {
        None => {
            // The chooser: every category as one clickable line.
            let list = ui.add_sized(
                parent,
                List::new().name(Label::key("store.section.categories")),
                Sizing::grow(1),
            )?;
            for category in store.categories() {
                let item = ui.add(list, ListItem::new(category_label(category)).chevron())?;
                hits.bind(item, StoreAction::Category(category.to_string()));
            }
        }
        Some(category) => {
            let list = ui.add_sized(
                parent,
                List::new().name(category_label(category)),
                Sizing::grow(1),
            )?;
            let apps: Vec<_> = store
                .apps()
                .iter()
                .filter(|a| a.category == category)
                .collect();
            for app in apps {
                rows::result_row(ui, hits, list, store, app)?;
            }
        }
    }
    Ok(())
}

/// The category name if the catalog carries it, otherwise the raw id (data).
fn category_label(category: &str) -> Label {
    let key = format!("store.category.{category}");
    Label::key(key)
}

pub(crate) fn search_page(
    ui: &mut Ui,
    parent: NodeId,
    store: &Store,
    hits: &mut HitMap,
) -> Result<(), UiError> {
    let results = crate::update::visible_results(store);
    if results.is_empty() {
        return super::states::no_results_panel(ui, parent, store, hits);
    }
    let head = ui.add(parent, Flex::row().gap(Space::S8).cross(CrossAlign::Center))?;
    ui.add(
        head,
        Text::new(Label::key_with(
            "store.search.results",
            &[("query", &store.query)],
        ))
        .spec(TextSpec::title(TypeSize::S16)),
    )?;
    spacer(ui, head)?;
    let clear = ui.add(head, Button::new(Label::key("store.search.clear")))?;
    hits.bind(clear, StoreAction::SearchClear);
    let list = ui.add_sized(
        parent,
        List::new().name(Label::key("store.search.results")),
        Sizing::grow(1),
    )?;
    for hit in results {
        rows::result_row(ui, hits, list, store, hit.app)?;
    }
    Ok(())
}

pub(crate) fn installed_page(
    ui: &mut Ui,
    parent: NodeId,
    store: &Store,
    hits: &mut HitMap,
) -> Result<(), UiError> {
    if store.installed.is_empty() {
        let empty = ui.add(
            parent,
            EmptyState::new(IconId::Installed, Label::key("store.installed.empty"))
                .message(Label::key("store.installed.empty-hint")),
        )?;
        let open = ui.add(
            empty,
            Button::new(Label::key("store.area.discover")).variant(ButtonVariant::Plain),
        )?;
        hits.bind(open, StoreAction::Area(Area::Discover));
        return Ok(());
    }
    let list = ui.add_sized(
        parent,
        List::new().name(Label::key("store.installed.on-device")),
        Sizing::grow(1),
    )?;
    for entry in &store.installed {
        let item = ui.add(
            list,
            ListItem::new(app_name(store, &entry.id, ui))
                .sublabel(Label::key_with(
                    "store.installed.size",
                    &[("size", &size_text(entry.size, ui))],
                ))
                .icon(rows::catalog_icon(store, &entry.id)),
        )?;
        hits.bind(item, StoreAction::Open(entry.id.clone()));
        let remove = ui.add(
            item,
            Button::new(Label::key("store.installed.remove")).variant(ButtonVariant::Danger),
        )?;
        hits.bind(remove, StoreAction::Remove(entry.id.clone()));
    }
    Ok(())
}

pub(crate) fn updates_page(
    ui: &mut Ui,
    parent: NodeId,
    store: &Store,
    hits: &mut HitMap,
) -> Result<(), UiError> {
    let body = ui.add_sized(parent, Flex::column().gap(Space::S16), Sizing::grow(1))?;
    // System and app updates are separated (DESKTOP-SPEC §7.3). The system
    // row is informational: the Updater owns it.
    ui.add(
        body,
        InfoBar::new(Tone::Info, Label::key("store.updates.system"))
            .message(Label::key("store.updates.system-desc")),
    )?;
    let updates: Vec<_> = store
        .apps()
        .iter()
        .filter(|a| store.update_available(&a.id))
        .collect();
    updates_head(ui, body, updates.len(), hits)?;
    if updates.is_empty() {
        ui.add(
            body,
            Text::new(Label::key("store.updates.none")).color(ColorRole::TextMuted),
        )?;
        return Ok(());
    }
    let list = ui.add_sized(
        body,
        List::new().name(Label::key("store.updates.apps")),
        Sizing::grow(1),
    )?;
    for app in updates {
        update_row(ui, list, store, app, hits)?;
    }
    Ok(())
}

/// Title · count · "update all" row.
fn updates_head(ui: &mut Ui, body: NodeId, count: usize, hits: &mut HitMap) -> Result<(), UiError> {
    let head = ui.add(body, Flex::row().gap(Space::S8).cross(CrossAlign::Center))?;
    ui.add(
        head,
        Text::new(Label::key("store.updates.apps")).spec(TextSpec::title(TypeSize::S16)),
    )?;
    spacer(ui, head)?;
    let count = i64::try_from(count).unwrap_or(i64::MAX);
    ui.add(
        head,
        Text::new(Label::key_count("store.updates.available", "count", count))
            .color(ColorRole::TextMuted),
    )?;
    if count > 1 {
        let all = ui.add(head, Button::new(Label::key("store.updates.update-all")))?;
        hits.bind(all, StoreAction::UpdateAll);
    }
    Ok(())
}

/// One updatable app: version change as sublabel, Update button.
fn update_row(
    ui: &mut Ui,
    list: NodeId,
    store: &Store,
    app: &crate::catalog::CatalogApp,
    hits: &mut HitMap,
) -> Result<(), UiError> {
    let Some(installed) = store.installed_app(&app.id) else {
        return Ok(());
    };
    let item = ui.add(
        list,
        ListItem::new(loc_label(&app.name, ui))
            .sublabel(Label::key_with(
                "store.updates.version",
                &[
                    ("from", &installed.version),
                    ("to", app.version.as_deref().unwrap_or_default()),
                ],
            ))
            .icon(rows::catalog_icon(store, &app.id)),
    )?;
    hits.bind(item, StoreAction::Open(app.id.clone()));
    let btn = ui.add(
        item,
        Button::new(Label::key("store.app.update")).variant(ButtonVariant::Plain),
    )?;
    hits.bind(btn, StoreAction::Install(app.id.clone()));
    Ok(())
}

pub(crate) fn install_file_page(
    ui: &mut Ui,
    parent: NodeId,
    store: &Store,
    hits: &mut HitMap,
) -> Result<(), UiError> {
    let body = ui.add_sized(parent, Flex::column().gap(Space::S12), Sizing::grow(1))?;
    ui.add(
        body,
        Text::new(Label::key("store.file.title")).spec(TextSpec::title(TypeSize::S16)),
    )?;
    muted(ui, body, Label::key("store.file.desc"), TypeSize::S14)?;
    let files = match store.catalog.dir.as_deref() {
        Some(dir) => crate::source::package_files(std::path::Path::new(dir)),
        None => Vec::new(),
    };
    if files.is_empty() {
        ui.add(
            body,
            Text::new(Label::key("store.file.empty")).color(ColorRole::TextMuted),
        )?;
        return Ok(());
    }
    let list = ui.add_sized(
        body,
        List::new().name(Label::key("store.file.pick")),
        Sizing::grow(1),
    )?;
    for file in files {
        let item = ui.add(
            list,
            ListItem::new(Label::data(&file)).icon(IconId::PackageFile),
        )?;
        let install = ui.add(
            item,
            Button::new(Label::key("store.app.install")).variant(ButtonVariant::Plain),
        )?;
        let dir = store.catalog.dir.clone().unwrap_or_default();
        hits.bind(install, StoreAction::InstallFile(format!("{dir}/{file}")));
    }
    Ok(())
}

pub(crate) fn offline_sources_page(
    ui: &mut Ui,
    parent: NodeId,
    store: &Store,
    hits: &mut HitMap,
) -> Result<(), UiError> {
    let body = ui.add_sized(parent, Flex::column().gap(Space::S12), Sizing::grow(1))?;
    ui.add(
        body,
        Text::new(Label::key("store.offline.title")).spec(TextSpec::title(TypeSize::S16)),
    )?;
    muted(ui, body, Label::key("store.offline.desc"), TypeSize::S14)?;
    if store.catalog.sources.is_empty() {
        ui.add(
            body,
            Text::new(Label::key("store.offline.none")).color(ColorRole::TextMuted),
        )?;
        return Ok(());
    }
    let list = ui.add_sized(
        body,
        List::new().name(Label::key("store.offline.title")),
        Sizing::grow(1),
    )?;
    for source in &store.catalog.sources {
        let item = ui.add(
            list,
            ListItem::new(Label::data(&source.name))
                .sublabel(Label::key(if source.has_catalog {
                    "store.offline.sign-ok"
                } else {
                    "store.offline.usb"
                }))
                .icon(IconId::Usb),
        )?;
        if source.has_catalog {
            let use_btn = ui.add(
                item,
                Button::new(Label::key("store.offline.use")).variant(ButtonVariant::Plain),
            )?;
            hits.bind(
                use_btn,
                StoreAction::UseSource(source.dir.display().to_string()),
            );
        }
    }
    Ok(())
}

pub(crate) fn account_page(
    ui: &mut Ui,
    parent: NodeId,
    _store: &Store,
    _hits: &mut HitMap,
) -> Result<(), UiError> {
    // Informational by design: signing in and the account services are owned
    // by Settings (DESKTOP-SPEC §10); the store adds no duplicate controls.
    let body = ui.add_sized(parent, Flex::column().gap(Space::S12), Sizing::grow(1))?;
    ui.add(
        body,
        Text::new(Label::key("store.account.title")).spec(TextSpec::title(TypeSize::S16)),
    )?;
    muted(ui, body, Label::key("store.account.desc"), TypeSize::S14)?;
    muted(
        ui,
        body,
        Label::key("store.account.sign-in-note"),
        TypeSize::S14,
    )?;
    Ok(())
}
