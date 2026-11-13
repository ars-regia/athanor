//! The user's reduced-motion setting (doc_accessibility.md AX9): GNOME's `enable-animations`,
//! read through `athanor-style`'s reader, which the bar sends to the own compositor with
//! `athanor_a11y_manager_v1` version 2, at its start and at each change. The compositor reads
//! no setting itself (doc_own_stack.md OS5), so without a bar the value stays false.

use athanor_style::appearance::{self, Appearance};

/// Whether `appearance` asks for reduced motion: `enable-animations` is false.
pub fn reduced(appearance: &Appearance) -> bool {
    !appearance.animations
}

/// Calls `send` with the setting now, and with its value at each change of it.
pub fn follow(send: impl Fn(bool) + 'static) {
    follow_with(appearance::watch, Appearance::read, send);
}

/// `follow` on the given watch and reader. The watch is connected first and the value read
/// after it, so a change between the two is not lost: it reaches `send` from the watch, after
/// this returns.
fn follow_with(
    watch: impl FnOnce(Box<dyn Fn(Appearance)>),
    read: impl FnOnce() -> Appearance,
    send: impl Fn(bool) + 'static,
) {
    let send = std::rc::Rc::new(send);
    let on_change = send.clone();
    watch(Box::new(move |current| on_change(reduced(&current))));
    send(reduced(&read()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn animations_off_is_reduced_motion() {
        let mut appearance = Appearance::default();
        assert!(!reduced(&appearance), "the factory settings animate");
        appearance.animations = false;
        assert!(reduced(&appearance));
    }

    /// The watch is connected before the value is read, the value is sent first, and a later
    /// change reaches `send` through the watch: no GSettings involved.
    #[test]
    fn the_watch_comes_before_the_read_and_changes_follow_the_first_value() {
        use std::cell::RefCell;
        use std::rc::Rc;
        let log = Rc::new(RefCell::new(Vec::<String>::new()));
        let callback: Rc<RefCell<Option<Box<dyn Fn(Appearance)>>>> = Rc::default();
        let (watched, read, sent) = (log.clone(), log.clone(), log.clone());
        let held = callback.clone();
        follow_with(
            move |on_change| {
                watched.borrow_mut().push("watch".into());
                *held.borrow_mut() = Some(on_change);
            },
            move || {
                read.borrow_mut().push("read".into());
                Appearance {
                    animations: false,
                    ..Appearance::default()
                }
            },
            move |reduced| sent.borrow_mut().push(format!("send {reduced}")),
        );
        assert_eq!(*log.borrow(), ["watch", "read", "send true"]);
        let change = callback.borrow();
        let on_change = change.as_ref().expect("the watch's callback");
        on_change(Appearance::default());
        assert_eq!(log.borrow().last().map(String::as_str), Some("send false"));
    }
}
