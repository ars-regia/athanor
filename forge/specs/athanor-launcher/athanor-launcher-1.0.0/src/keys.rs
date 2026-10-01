//! What a key does while the launcher is shown (LA5). The entry keeps the focus, so these
//! are read before it in the capture phase; everything else is typed.

use crate::menu::Choice;

/// The keys the launcher reads; the UI maps GDK's key values onto them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Escape,
    Enter,
    Tab,
    Up,
    Down,
    PageUp,
    PageDown,
    C,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Hide,
    Run(Choice),
    Menu,
    Move(i32),
    /// The key goes on to the entry.
    Pass,
}

/// Rows a page key moves over.
pub const PAGE: i32 = 5;

pub fn command(key: Key, ctrl: bool, query_has_selection: bool) -> Command {
    match (key, ctrl) {
        (Key::Escape, _) => Command::Hide,
        (Key::Enter, false) => Command::Run(Choice::Open),
        (Key::Enter, true) => Command::Run(Choice::ShowInFolder),
        (Key::Tab, _) => Command::Menu,
        (Key::Down, _) => Command::Move(1),
        (Key::Up, _) => Command::Move(-1),
        (Key::PageDown, _) => Command::Move(PAGE),
        (Key::PageUp, _) => Command::Move(-PAGE),
        (Key::C, true) if !query_has_selection => Command::Run(Choice::Copy),
        _ => Command::Pass,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu::Choice;

    #[test]
    fn the_keys_of_la5() {
        let plain = |key| command(key, false, false);
        assert_eq!(plain(Key::Escape), Command::Hide);
        assert_eq!(plain(Key::Enter), Command::Run(Choice::Open));
        assert_eq!(command(Key::Enter, true, false), Command::Run(Choice::ShowInFolder));
        assert_eq!(plain(Key::Tab), Command::Menu);
        assert_eq!(plain(Key::Down), Command::Move(1));
        assert_eq!(plain(Key::Up), Command::Move(-1));
        assert_eq!(plain(Key::PageDown), Command::Move(PAGE));
        assert_eq!(plain(Key::PageUp), Command::Move(-PAGE));
        assert_eq!(plain(Key::Other), Command::Pass);
    }

    #[test]
    fn ctrl_c_copies_the_result_unless_the_query_has_a_selection() {
        assert_eq!(command(Key::C, true, false), Command::Run(Choice::Copy));
        assert_eq!(command(Key::C, true, true), Command::Pass, "the entry copies its own selection");
        assert_eq!(command(Key::C, false, false), Command::Pass, "a plain c is typed");
    }
}
