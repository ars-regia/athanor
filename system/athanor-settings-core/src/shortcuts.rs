//! The shortcut-conflict check of SE15 (doc_settings.md): a new binding that collides with an
//! existing one is refused with the name of the holder, and a binding another document owns
//! (Super+Space, Super+C, the OSD keys) can be reassigned but not removed. The bindings come
//! from `athanor_sulphur_client::shortcuts`; this module only compares them.

use std::collections::BTreeSet;

/// One key combination, compared without regard to case or to the order of its modifiers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Combo {
    modifiers: BTreeSet<String>,
    key: String,
}

const MODIFIERS: [&str; 5] = ["super", "ctrl", "alt", "shift", "altgr"];

impl Combo {
    /// The combination of `text`, such as `Super+Shift+Space`. Refused when it has no key, a
    /// modifier twice, or a part that is empty.
    pub fn parse(text: &str) -> Option<Self> {
        let mut parts: Vec<String> = text
            .split('+')
            .map(|part| part.trim().to_lowercase())
            .collect();
        let key = parts
            .pop()
            .filter(|key| !key.is_empty() && !MODIFIERS.contains(&key.as_str()))?;
        let mut modifiers = BTreeSet::new();
        for part in parts {
            let canonical = if part == "control" {
                "ctrl".to_owned()
            } else {
                part
            };
            if !MODIFIERS.contains(&canonical.as_str()) || !modifiers.insert(canonical) {
                return None;
            }
        }
        Some(Self { modifiers, key })
    }
}

/// An existing binding: its combination, the action it runs and its holder's name, as the
/// refusal shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    pub combo: Combo,
    pub action: String,
    pub holder: String,
    /// Owned by another document: it may be reassigned, never removed.
    pub protected: bool,
}

/// Why a change to the bindings is refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The combination is held by this holder.
    Taken(String),
    /// The action's binding is owned elsewhere and must keep one.
    Protected(String),
}

/// Whether `action` may be bound to `combo`. A binding of the same action does not collide
/// with itself, so reassigning one to the combination it has is allowed.
pub fn check_new(bindings: &[Binding], action: &str, combo: &Combo) -> Result<(), Refusal> {
    match bindings
        .iter()
        .find(|binding| binding.combo == *combo && binding.action != action)
    {
        Some(binding) => Err(Refusal::Taken(binding.holder.clone())),
        None => Ok(()),
    }
}

/// Whether the binding of `action` may be removed.
pub fn check_remove(bindings: &[Binding], action: &str) -> Result<(), Refusal> {
    match bindings
        .iter()
        .find(|binding| binding.action == action && binding.protected)
    {
        Some(binding) => Err(Refusal::Protected(binding.holder.clone())),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding(combo: &str, action: &str, holder: &str, protected: bool) -> Binding {
        Binding {
            combo: Combo::parse(combo).expect("a valid combination"),
            action: action.into(),
            holder: holder.into(),
            protected,
        }
    }

    fn bindings() -> Vec<Binding> {
        vec![
            binding("Super+Space", "input-source", "Switch input source", true),
            binding("Super+C", "control-center", "Control center", true),
            binding("Super+T", "custom:terminal", "Terminal", false),
        ]
    }

    #[test]
    fn combinations_compare_without_case_or_modifier_order() {
        assert_eq!(Combo::parse("Shift+Super+a"), Combo::parse("super+SHIFT+A"));
        assert_eq!(Combo::parse("Control+Q"), Combo::parse("Ctrl+q"));
        assert_ne!(Combo::parse("Super+A"), Combo::parse("Super+Shift+A"));
        for bad in ["", "Super+", "Super", "Hyper+A", "Ctrl+Ctrl+A", "Ctrl++A"] {
            assert_eq!(Combo::parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn a_colliding_binding_is_refused_with_the_holder() {
        let list = bindings();
        let space = Combo::parse("super+space").expect("valid");
        assert_eq!(
            check_new(&list, "custom:files", &space),
            Err(Refusal::Taken("Switch input source".into()))
        );
        assert_eq!(
            check_new(&list, "input-source", &space),
            Ok(()),
            "rebinding to itself"
        );
        let free = Combo::parse("Super+E").expect("valid");
        assert_eq!(check_new(&list, "custom:files", &free), Ok(()));
    }

    #[test]
    fn a_binding_owned_elsewhere_is_never_removed() {
        let list = bindings();
        assert_eq!(
            check_remove(&list, "control-center"),
            Err(Refusal::Protected("Control center".into()))
        );
        assert_eq!(check_remove(&list, "custom:terminal"), Ok(()));
        let moved = Combo::parse("Super+Shift+C").expect("valid");
        assert_eq!(
            check_new(&list, "control-center", &moved),
            Ok(()),
            "reassigned, not removed"
        );
    }
}
