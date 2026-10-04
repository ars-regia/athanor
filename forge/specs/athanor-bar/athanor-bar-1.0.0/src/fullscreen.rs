//! Whether a fullscreen window has the focus: what the bar tells athanor-shelld so that
//! do not disturb can follow it (NC6).

use athanor_compositor_client::WindowState;

/// Any window is both fullscreen and activated.
pub fn fullscreen_active(windows: impl IntoIterator<Item = WindowState>) -> bool {
    windows
        .into_iter()
        .any(|state| state.fullscreen && state.activated)
}

/// The `ReportFullscreen(available, active)` arguments: a compositor that withholds the
/// toplevel list is unobservable, and says nothing about the windows it does not describe.
pub fn report(available: bool, windows: impl IntoIterator<Item = WindowState>) -> (bool, bool) {
    if available {
        (true, fullscreen_active(windows))
    } else {
        (false, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(activated: bool, fullscreen: bool) -> WindowState {
        WindowState {
            activated,
            fullscreen,
            ..WindowState::default()
        }
    }

    #[test]
    fn only_a_window_both_fullscreen_and_activated_counts() {
        assert!(!fullscreen_active([]));
        assert!(!fullscreen_active([state(false, true)]));
        assert!(!fullscreen_active([state(true, false)]));
        assert!(fullscreen_active([state(true, true)]));
        assert!(fullscreen_active([state(true, false), state(true, true)]));
    }

    #[test]
    fn an_unobservable_compositor_is_reported_as_such() {
        assert_eq!(report(false, [state(true, true)]), (false, false));
        assert_eq!(report(true, []), (true, false));
        assert_eq!(report(true, [state(true, true)]), (true, true));
    }
}
