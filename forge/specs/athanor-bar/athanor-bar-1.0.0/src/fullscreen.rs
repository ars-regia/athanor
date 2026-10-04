//! Whether a fullscreen window has the focus: what the bar tells athanor-shelld so that
//! do not disturb can follow it (NC6).

use athanor_compositor_client::Window;

/// Any window is both fullscreen and activated.
pub fn fullscreen_active(windows: &[Window]) -> bool {
    windows
        .iter()
        .any(|window| window.state.fullscreen && window.state.activated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use athanor_compositor_client::WindowState;

    fn window(activated: bool, fullscreen: bool) -> Window {
        Window {
            state: WindowState {
                activated,
                fullscreen,
                ..WindowState::default()
            },
            ..Window::default()
        }
    }

    #[test]
    fn only_a_window_both_fullscreen_and_activated_counts() {
        assert!(!fullscreen_active(&[]));
        assert!(!fullscreen_active(&[window(false, true)]));
        assert!(!fullscreen_active(&[window(true, false)]));
        assert!(fullscreen_active(&[window(true, true)]));
        assert!(fullscreen_active(&[
            window(true, false),
            window(true, true)
        ]));
    }
}
