//! Listing: the panels and items a caller should see on this device.

use super::{slot_of, Core};
use crate::access::Caller;
use crate::error::SettingsError;
use crate::schema::{
    InstanceKind, KeyTemplate, Panel, Resolved, Risk, Scope, SettingDef, ValueType,
};
use ultar_bus::messages::settings::{
    ChoiceInfo, ItemInfo, ItemKind, PanelInfo, RiskInfo, ScopeInfo,
};
use ultar_bus::Text;
use ultar_profile::{visible_panels, Capabilities};

/// Most apps listed per app-keyed setting, so that an answer always fits
/// into one bus frame. Apps beyond that are still reachable by key.
pub const MAX_LISTED_APPS: usize = 128;

/// Whether a panel is shown on a device with these capabilities: the rule
/// of `ultar-profile` where it knows the panel, plus "needs a display" for
/// the shell panels it does not know.
pub fn panel_visible(panel: Panel, capabilities: &Capabilities) -> bool {
    let by_profile = panel
        .profile_panel()
        .is_none_or(|id| visible_panels(capabilities).contains(&id));
    by_profile && (!panel.needs_display() || capabilities.has_display())
}

fn item_kind(def: &SettingDef) -> ItemKind {
    match def.value_type {
        ValueType::Bool => ItemKind::Toggle,
        ValueType::Int { min, max, step } => ItemKind::Integer { min, max, step },
        ValueType::Float { min, max, step } => ItemKind::Decimal { min, max, step },
        ValueType::Enum(choices) => ItemKind::Choice {
            choices: choices
                .iter()
                .map(|choice| ChoiceInfo {
                    id: (*choice).to_owned(),
                    label: Text::key(&def.choice_key(choice)),
                })
                .collect(),
        },
        ValueType::Text(format) => ItemKind::Text {
            format: format.name().to_owned(),
        },
        ValueType::List { item, max_len } => ItemKind::List {
            format: item.name().to_owned(),
            max_len: u32::try_from(max_len).unwrap_or(u32::MAX),
        },
    }
}

fn risk_info(risk: Risk) -> RiskInfo {
    match risk {
        Risk::Safe => RiskInfo::Safe,
        Risk::ConfirmRevert { seconds } => RiskInfo::ConfirmRevert { seconds },
        Risk::AdminOnly => RiskInfo::AdminOnly,
    }
}

fn scope_info(scope: Scope) -> ScopeInfo {
    match scope {
        Scope::System => ScopeInfo::System,
        Scope::User => ScopeInfo::User,
        Scope::DisplayCombination => ScopeInfo::DisplayCombination,
    }
}

impl Core {
    fn item(&self, caller: &Caller, found: &Resolved<'_>, template: bool) -> ItemInfo {
        let def = found.def;
        let instance = found.instance.as_deref();
        ItemInfo {
            key: found.key.clone(),
            label: Text::key(&def.label_key()),
            description: Text::key(&def.description_key()),
            kind: item_kind(def),
            value: self.effective(found).into(),
            default: self.default_value(def, instance).into(),
            risk: risk_info(def.risk),
            scope: scope_info(def.scope),
            writable: def.risk != Risk::AdminOnly || caller.admin,
            pending: self.pending.contains_key(&found.key),
            template,
        }
    }

    /// Instances of an app-keyed setting that have a stored value.
    fn stored_instances(&self, def: &SettingDef, template: &KeyTemplate) -> Vec<String> {
        self.store
            .keys(slot_of(def.scope, &self.combo))
            .filter_map(|key| template.matches(key).flatten().map(str::to_owned))
            .take(MAX_LISTED_APPS)
            .collect()
    }

    fn items_of(&self, caller: &Caller, def: &SettingDef, items: &mut Vec<ItemInfo>) {
        let Some(template) = KeyTemplate::parse(def.key) else {
            return;
        };
        let concrete = |instance: Option<&str>| Resolved {
            def,
            key: template.instantiate(instance),
            instance: instance.map(str::to_owned),
        };
        match def.instance {
            InstanceKind::None => items.push(self.item(caller, &concrete(None), false)),
            InstanceKind::Display => {
                for id in self.display_ids() {
                    items.push(self.item(caller, &concrete(Some(&id)), false));
                }
            }
            InstanceKind::App => {
                // The family itself, so the app can build keys for apps the
                // service has no stored value for yet.
                let family = Resolved {
                    def,
                    key: def.key.to_owned(),
                    instance: None,
                };
                items.push(self.item(caller, &family, true));
                for id in self.stored_instances(def, &template) {
                    items.push(self.item(caller, &concrete(Some(&id)), false));
                }
            }
        }
    }

    /// The panels and items the caller should see: panels whose rule the
    /// device meets, items whose capabilities the device has. Each item
    /// says whether this caller may change it. `panel` restricts the answer
    /// to one panel.
    pub fn list(
        &self,
        caller: &Caller,
        panel: Option<&str>,
    ) -> Result<Vec<PanelInfo>, SettingsError> {
        let only = match panel {
            Some(id) => Some(Panel::parse(id).ok_or(SettingsError::BadRequest)?),
            None => None,
        };
        let capabilities = self.capabilities();
        let mut panels = Vec::new();
        for panel in Panel::ALL {
            if only.is_some_and(|only| only != panel) || !panel_visible(panel, &capabilities) {
                continue;
            }
            let mut items = Vec::new();
            for def in self.registry.panel_defs(panel) {
                if capabilities.flags.contains_all(def.requires) {
                    self.items_of(caller, def, &mut items);
                }
            }
            if !items.is_empty() {
                panels.push(PanelInfo {
                    id: panel.id().to_owned(),
                    title: Text::key(&panel.title_key()),
                    items,
                });
            }
        }
        Ok(panels)
    }
}
