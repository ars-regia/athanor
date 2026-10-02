//! What each preset holds, and the order it is drawn in (doc_bar.md, BR7 and BR6). The
//! status modules of later plans keep their slots here; the UI builds none of them yet.

use athanor_layout::preset::Preset;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Module {
    Launcher,
    AppLibrary,
    Workspaces,
    RunningApps,
    Clock,
    InputSource,
    Accessibility,
    Tray,
    Tiling,
    Audio,
    Bluetooth,
    Network,
    Battery,
    Notifications,
    Power,
    Shield,
}

impl Module {
    pub const ALL: [Module; 16] = [
        Module::Launcher,
        Module::AppLibrary,
        Module::Workspaces,
        Module::RunningApps,
        Module::Clock,
        Module::InputSource,
        Module::Accessibility,
        Module::Tray,
        Module::Tiling,
        Module::Audio,
        Module::Bluetooth,
        Module::Network,
        Module::Battery,
        Module::Notifications,
        Module::Power,
        Module::Shield,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Module::Launcher => "launcher",
            Module::AppLibrary => "app-library",
            Module::Workspaces => "workspaces",
            Module::RunningApps => "running-apps",
            Module::Clock => "clock",
            Module::InputSource => "input-source",
            Module::Accessibility => "accessibility",
            Module::Tray => "tray",
            Module::Tiling => "tiling",
            Module::Audio => "audio",
            Module::Bluetooth => "bluetooth",
            Module::Network => "network",
            Module::Battery => "battery",
            Module::Notifications => "notifications",
            Module::Power => "power",
            Module::Shield => "shield",
        }
    }

    pub fn from_id(id: &str) -> Option<Module> {
        Module::ALL.into_iter().find(|module| module.id() == id)
    }
}

/// The status modules, in the order of BR7, before the clock and the shield.
pub const STATUS: [Module; 10] = [
    Module::InputSource,
    Module::Accessibility,
    Module::Tray,
    Module::Tiling,
    Module::Audio,
    Module::Bluetooth,
    Module::Network,
    Module::Battery,
    Module::Notifications,
    Module::Power,
];

/// The groups in text order: `start` is at the left under left-to-right text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Groups {
    pub start: Vec<Module>,
    pub centre: Vec<Module>,
    pub end: Vec<Module>,
}

/// The table of BR7. The shield is last at the end in every preset (SH9.1).
pub fn groups(preset: Preset) -> Groups {
    match preset {
        Preset::Float | Preset::Minimal => Groups {
            start: vec![Module::Workspaces, Module::AppLibrary],
            centre: vec![Module::Clock],
            end: [&STATUS[..], &[Module::Shield]].concat(),
        },
        Preset::Bar => Groups {
            start: vec![Module::Launcher, Module::AppLibrary, Module::RunningApps],
            centre: Vec::new(),
            end: [&STATUS[..], &[Module::Clock, Module::Shield]].concat(),
        },
    }
}

/// The groups as drawn, left to right, whatever the text direction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub left: Vec<Module>,
    pub centre: Vec<Module>,
    pub right: Vec<Module>,
}

/// Under right-to-left text the whole row mirrors: the end group is drawn at the left, in
/// reverse, so the shield sits at the left end (BR6).
pub fn visual(preset: Preset, rtl: bool) -> Row {
    let Groups { start, centre, end } = groups(preset);
    if !rtl {
        return Row {
            left: start,
            centre,
            right: end,
        };
    }
    let reversed = |mut modules: Vec<Module>| {
        modules.reverse();
        modules
    };
    Row {
        left: reversed(end),
        centre: reversed(centre),
        right: reversed(start),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Module::*;

    #[test]
    fn float_and_minimal_follow_the_table_of_br7() {
        for preset in [Preset::Float, Preset::Minimal] {
            let groups = groups(preset);
            assert_eq!(groups.start, [Workspaces, AppLibrary]);
            assert_eq!(groups.centre, [Clock]);
            assert_eq!(
                groups.end,
                [
                    InputSource,
                    Accessibility,
                    Tray,
                    Tiling,
                    Audio,
                    Bluetooth,
                    Network,
                    Battery,
                    Notifications,
                    Power,
                    Shield
                ]
            );
        }
    }

    #[test]
    fn bar_follows_the_table_of_br7() {
        let groups = groups(Preset::Bar);
        assert_eq!(groups.start, [Launcher, AppLibrary, RunningApps]);
        assert!(groups.centre.is_empty());
        assert_eq!(&groups.end[..10], &STATUS[..]);
        assert_eq!(&groups.end[10..], [Clock, Shield]);
    }

    #[test]
    fn the_shield_ends_every_preset_and_nothing_repeats() {
        for preset in Preset::ALL {
            let groups = groups(preset);
            assert_eq!(groups.end.last(), Some(&Shield), "{preset:?}");
            let all: Vec<Module> = [groups.start, groups.centre, groups.end].concat();
            let unique: std::collections::HashSet<Module> = all.iter().copied().collect();
            assert_eq!(unique.len(), all.len(), "{preset:?}");
        }
    }

    #[test]
    fn left_to_right_the_row_is_the_groups() {
        let row = visual(Preset::Bar, false);
        let groups = groups(Preset::Bar);
        assert_eq!(
            (row.left, row.centre, row.right),
            (groups.start, groups.centre, groups.end)
        );
    }

    #[test]
    fn right_to_left_the_row_mirrors() {
        let row = visual(Preset::Float, true);
        assert_eq!(row.left.first(), Some(&Shield));
        assert_eq!(row.left.last(), Some(&InputSource));
        assert_eq!(row.centre, [Clock]);
        assert_eq!(row.right, [AppLibrary, Workspaces]);
        let bar = visual(Preset::Bar, true);
        assert_eq!(&bar.left[..2], [Shield, Clock]);
        assert_eq!(bar.right, [RunningApps, AppLibrary, Launcher]);
    }

    #[test]
    fn module_ids_round_trip() {
        for module in Module::ALL {
            assert_eq!(Module::from_id(module.id()), Some(module));
        }
        assert_eq!(Module::from_id("nothing"), None);
    }
}
