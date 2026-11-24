//! Where the helper thinks. A device that cannot run a model gets the smallest help – unless
//! a paired device lends its model; and a device with a small model prefers a paired
//! device with a better one.

use ultar_bus::messages::agent::Brain;
use ultar_bus::messages::llm::LlmState;
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
}
