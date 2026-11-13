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
    let send = std::rc::Rc::new(send);
    send(reduced(&Appearance::read()));
    appearance::watch(move |current| send(reduced(&current)));
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
}
