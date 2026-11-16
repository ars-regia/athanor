//! The calls the volume, brightness and screen-reader keys make to cosmic-settings-daemon
//! (doc_own_stack.md decision 29, plan D13): the methods cosmic-comp's session calls for them
//! (`system_actions`), so the same daemon and the same overlay serve both sessions. They read
//! and write nothing of the user's configuration. The compositor's third exception of
//! doc_shell.md SH2 by path; each key leaves this file when its replacement lands: volume and
//! brightness with athanor-osd (doc_osd.md OD15), the screen reader with athanor-a11y
//! (doc_accessibility.md AX3).

use crate::session_calls::Call;

/// A method of cosmic-settings-daemon. A step is never coalesced: each press is one step.
const fn daemon(method: &'static str, while_locked: bool, coalesce: bool) -> Call {
    Call {
        destination: "com.system76.CosmicSettingsDaemon",
        path: "/com/system76/CosmicSettingsDaemon",
        interface: "com.system76.CosmicSettingsDaemon",
        method,
        while_locked,
        coalesce,
    }
}

/// `XF86AudioRaiseVolume`; also while locked (doc_lock_and_prompts.md section 1).
pub(crate) const VOLUME_UP: Call = daemon("VolumeUp", true, false);
/// `XF86AudioLowerVolume`; also while locked.
pub(crate) const VOLUME_DOWN: Call = daemon("VolumeDown", true, false);
/// `XF86MonBrightnessUp`; also while locked.
pub(crate) const BRIGHTNESS_UP: Call = daemon("IncreaseDisplayBrightness", true, false);
/// `XF86MonBrightnessDown`; also while locked.
pub(crate) const BRIGHTNESS_DOWN: Call = daemon("DecreaseDisplayBrightness", true, false);
/// Super+Alt+S: the screen reader's setting flips (doc_accessibility.md AX3); a toggle, so a
/// press repeated while its call waits is not replayed.
pub(crate) const SCREEN_READER: Call = daemon("ScreenReader", false, true);
