//! Where the helper thinks. A device that cannot run a model gets the smallest help – unless
//! a paired device lends its model; and a device with a small model prefers a paired
//! device with a better one.

use ultar_bus::messages::agent::Brain;
use ultar_bus::messages::llm::LlmState;
use ultar_bus::messages::llm_cloud::CloudState;
use ultar_bus::messages::mesh_model::Lender;

/// The brain for now and, for a lent model, the lending device's name.
/// `lenders`: paired devices that lend a model, best first.
pub fn choose(local: &LlmState, lenders: &[Lender]) -> (Brain, String) {
    let best = lenders.first();
    match best {
        Some(lender) if !local.ready || lender.class > local.class => {
            (Brain::Mesh(lender.device.clone()), lender.name.clone())
        }
        _ if local.ready => (Brain::Local, String::new()),
        _ => (Brain::Minimal, String::new()),
    }
}

/// As [`choose`], with the last way: when neither this device nor a paired one has a
/// model and the person entered a key, a provider's model in the internet. Never
/// preferred over a model on the person's own devices, however small that one is.
pub fn choose_with_cloud(
    local: &LlmState,
    lenders: &[Lender],
    cloud: &CloudState,
) -> (Brain, String) {
    match choose(local, lenders) {
        (Brain::Minimal, _) if cloud.configured => (Brain::Cloud, cloud.provider.clone()),
        own => own,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultar_bus::messages::llm::ModelClass;

    fn local(ready: bool, class: ModelClass) -> LlmState {
        LlmState {
            ready,
            model: "m".into(),
            class,
        }
    }

    fn lender(class: ModelClass) -> Lender {
        Lender {
            device: "desk".into(),
            name: "Desk".into(),
            class,
        }
    }

    #[test]
    fn a_device_without_any_model_gets_the_smallest_help() {
        let none = local(false, ModelClass::Small);
        assert_eq!(choose(&none, &[]), (Brain::Minimal, String::new()));
    }

    #[test]
    fn a_weak_device_thinks_with_a_paired_device_that_lends_its_model() {
        let none = local(false, ModelClass::Small);
        let lent = choose(&none, &[lender(ModelClass::Small)]);
        assert_eq!(lent, (Brain::Mesh("desk".into()), "Desk".into()));
        // A small own model, a better one in the mesh: the better one.
        let small = local(true, ModelClass::Small);
        assert_eq!(
            choose(&small, &[lender(ModelClass::Large)]).0,
            Brain::Mesh("desk".into())
        );
    }

    #[test]
    fn an_own_model_that_is_as_good_stays_at_home() {
        let medium = local(true, ModelClass::Medium);
        assert_eq!(
            choose(&medium, &[lender(ModelClass::Medium)]).0,
            Brain::Local
        );
        assert_eq!(
            choose(&medium, &[lender(ModelClass::Small)]).0,
            Brain::Local
        );
        assert_eq!(choose(&medium, &[]).0, Brain::Local);
    }

    #[test]
    fn the_cloud_is_the_last_way_and_needs_the_persons_key() {
        let cloud = |configured| CloudState {
            configured,
            provider: "Claude".into(),
            model: "m".into(),
        };
        let none = local(false, ModelClass::Small);
        let small = local(true, ModelClass::Small);
        // no key: the smallest help, as before
        assert_eq!(
            choose_with_cloud(&none, &[], &cloud(false)).0,
            Brain::Minimal
        );
        assert_eq!(
            choose_with_cloud(&none, &[], &cloud(true)),
            (Brain::Cloud, "Claude".into())
        );
        // any model on the person's own devices comes first, even a small one
        assert_eq!(choose_with_cloud(&small, &[], &cloud(true)).0, Brain::Local);
        let lent = choose_with_cloud(&none, &[lender(ModelClass::Small)], &cloud(true));
        assert_eq!(lent.0, Brain::Mesh("desk".into()));
    }
}
