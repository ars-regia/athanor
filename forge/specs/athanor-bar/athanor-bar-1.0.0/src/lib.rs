//! The logic of athanor-bar, with no GTK type (doc_bar.md, section 2, "Shared code"): what
//! each preset holds, what logind offers, the time zone, and the state of NetworkManager,
//! BlueZ and UPower, and the trust shield. The binary draws it.

pub mod clock;
pub mod control_center;
pub mod dbusmenu;
pub mod keyboard;
pub mod notices;
pub mod order;
pub mod popups;
pub mod power;
pub mod props;
pub mod shield;
pub mod tiling;
pub mod tray;

/// Until the other models move (control-center plan, Task 7), the modules that name
/// `athanor_bar::battery` keep their path.
pub use athanor_services::battery;

#[cfg(test)]
mod tests {
    use std::path::Path;

    /// The theme directories main.rs grants are ones the unit makes writable: under
    /// ProtectHome=read-only, only its configuration directories are.
    #[test]
    fn the_unit_declares_every_theme_directory_as_a_configuration_directory() {
        let unit = include_str!("../data/athanor-bar.service");
        let declared: Vec<&str> = unit
            .lines()
            .filter_map(|line| line.strip_prefix("ConfigurationDirectory="))
            .flat_map(str::split_whitespace)
            .collect();
        for dir in athanor_compositor_client::theme::theme_dirs_in(Path::new("cosmic")) {
            assert!(
                declared.iter().any(|name| Path::new(name) == dir),
                "{declared:?} lacks {dir:?}"
            );
        }
    }
}
