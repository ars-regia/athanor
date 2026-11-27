//! The identity to authenticate as (doc_lock_and_prompts.md, LP11): one of the `unix-user`
//! identities polkit offers, the session's user preselected when offered. Each uid is resolved
//! to its name through NSS, since the name is the first line the agent writes to polkit's
//! helper (plan, section 1, item 1).

use nix::unistd::{Uid, User};

/// An offered identity whose name NSS gave.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    pub uid: u32,
    pub name: String,
}

/// The identities the dialog offers, in polkit's order, and the one preselected.
#[derive(Debug, PartialEq, Eq)]
pub struct Choice {
    pub identities: Vec<Identity>,
    pub selected: usize,
}

/// The identities of `offered` that `name_of` resolves, the session's `user` preselected when
/// among them and the first one otherwise. An identity NSS cannot resolve is dropped alone;
/// `None` when none is left, and the request then fails (maintainer decision, 2026-11-26).
pub fn choose(
    offered: &[u32],
    user: u32,
    name_of: impl Fn(u32) -> Option<String>,
) -> Option<Choice> {
    let identities: Vec<Identity> = offered
        .iter()
        .filter_map(|&uid| name_of(uid).map(|name| Identity { uid, name }))
        .collect();
    if identities.is_empty() {
        return None;
    }
    let selected = identities.iter().position(|i| i.uid == user).unwrap_or(0);
    Some(Choice {
        identities,
        selected,
    })
}

/// The user name of `uid` from NSS: `None` when NSS fails or knows no such user.
#[must_use]
pub fn nss_name(uid: u32) -> Option<String> {
    User::from_uid(Uid::from_raw(uid))
        .ok()
        .flatten()
        .map(|user| user.name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(uid: u32) -> Option<String> {
        match uid {
            0 => Some("root".to_owned()),
            1000 => Some("ada".to_owned()),
            1001 => Some("grace".to_owned()),
            _ => None,
        }
    }

    fn uids(choice: &Choice) -> Vec<u32> {
        choice.identities.iter().map(|i| i.uid).collect()
    }

    #[test]
    fn the_session_user_is_preselected_when_offered() {
        let choice = choose(&[1001, 1000, 0], 1000, names);
        let choice = choice.as_ref();
        assert_eq!(choice.map(uids), Some(vec![1001, 1000, 0]));
        assert_eq!(choice.map(|c| c.selected), Some(1));
        assert_eq!(
            choice
                .and_then(|c| c.identities.get(1))
                .map(|i| i.name.as_str()),
            Some("ada")
        );
    }

    #[test]
    fn the_first_identity_is_preselected_when_the_user_is_not_offered() {
        let choice = choose(&[1001, 0], 1000, names);
        assert_eq!(choice.as_ref().map(uids), Some(vec![1001, 0]));
        assert_eq!(choice.map(|c| c.selected), Some(0));
    }

    #[test]
    fn an_identity_nss_cannot_resolve_is_dropped_alone() {
        let choice = choose(&[1001, 4242, 1000], 1000, names);
        assert_eq!(choice.as_ref().map(uids), Some(vec![1001, 1000]));
        assert_eq!(choice.map(|c| c.selected), Some(1));
    }

    #[test]
    fn the_user_is_not_preselected_when_nss_cannot_resolve_them() {
        let choice = choose(&[4242, 1001], 4242, names);
        assert_eq!(choice.as_ref().map(uids), Some(vec![1001]));
        assert_eq!(choice.map(|c| c.selected), Some(0));
    }

    #[test]
    fn with_no_identity_left_the_request_fails() {
        assert_eq!(choose(&[4242, 4343], 4242, names), None);
        assert_eq!(choose(&[], 1000, names), None);
    }

    #[test]
    fn nss_resolves_root_and_not_an_unused_uid() {
        assert_eq!(nss_name(0).as_deref(), Some("root"));
        assert_eq!(nss_name(4_000_000_000), None);
    }
}
