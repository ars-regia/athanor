//! The four Secure Boot readings of UT8. They are published; they never move the badge.
use athanor_trust_state::SecureBoot;
use std::path::Path;

const EFI_GLOBAL: &str = "8be4df61-93ca-11d2-aa0d-00e098032b8c";
const SHIM_LOCK: &str = "605dab50-e046-4300-abb6-3dd810dd8b23";

/// An efivarfs file is four attribute bytes and then the value.
fn efi_byte(efivars: &Path, name: &str, guid: &str) -> Option<u8> {
    std::fs::read(efivars.join(format!("{name}-{guid}"))).ok()?.get(4).copied()
}

/// `none [integrity] confidentiality` -> `integrity`.
fn lockdown_mode(text: &str) -> Option<String> {
    text.split_whitespace().find_map(|word| word.strip_prefix('[')?.strip_suffix(']')).map(str::to_owned)
}

#[must_use]
pub fn read(efivars: &Path, lockdown: &Path) -> SecureBoot {
    SecureBoot {
        secure_boot: efi_byte(efivars, "SecureBoot", EFI_GLOBAL),
        setup_mode: efi_byte(efivars, "SetupMode", EFI_GLOBAL),
        mok_sb_state: efi_byte(efivars, "MokSBStateRT", SHIM_LOCK),
        lockdown: std::fs::read_to_string(lockdown).ok().as_deref().and_then(lockdown_mode),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_four_readings_are_read_separately() {
        let dir = std::env::temp_dir().join(format!("athanor-update-sb-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::write(dir.join(format!("SecureBoot-{EFI_GLOBAL}")), [6, 0, 0, 0, 1]).expect("write");
        std::fs::write(dir.join(format!("SetupMode-{EFI_GLOBAL}")), [6, 0, 0, 0, 0]).expect("write");
        std::fs::write(dir.join("lockdown"), "none [integrity] confidentiality\n").expect("write");
        let on = read(&dir, &dir.join("lockdown"));
        assert_eq!((on.secure_boot, on.setup_mode, on.mok_sb_state, on.lockdown.as_deref()), (Some(1), Some(0), None, Some("integrity")));
        assert!(on.on());

        std::fs::write(dir.join(format!("MokSBStateRT-{SHIM_LOCK}")), [6, 0, 0, 0, 1]).expect("write");
        assert!(!read(&dir, &dir.join("lockdown")).on(), "shim validation disabled");
        let bios = read(&dir.join("absent"), &dir.join("absent"));
        assert_eq!((bios.secure_boot, bios.lockdown), (None, None));
        std::fs::remove_dir_all(dir).expect("cleanup");
    }
}
