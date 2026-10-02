//! Where an application comes from (doc_launcher.md, LA6): the system image, a Flatpak, or
//! the user. The entry's path decides, never its `X-Flatpak` key alone: any process of the
//! user can write a desktop entry that claims to be a Flatpak. For a Flatpak, the remote and
//! version are read from `flatpak list --app --columns=application,origin,version`, whose
//! output is tab-separated and never translated.

use std::path::{Component, Path};

/// The image's data directory: read-only on a booted system.
const IMAGE_APPLICATIONS: &str = "/usr/share";
/// Where the system installation exports its applications' entries.
const SYSTEM_FLATPAK_APPLICATIONS: &str = "/var/lib/flatpak/exports/share/applications";
/// Below the user's data directory: where the user installation exports them.
const USER_FLATPAK_APPLICATIONS: &str = "flatpak/exports/share/applications";

#[derive(Debug, PartialEq, Eq)]
pub enum Provenance {
    /// The entry is under the image's data directory.
    Image,
    /// The entry is exported by a Flatpak installation; the app ID is the file name's stem.
    Flatpak(String),
    /// Anywhere else: an entry the user, or a process of the user, installed.
    User,
}

/// `path` is the desktop entry's file, `x_flatpak` its `X-Flatpak` key, `data_home` the
/// user's data directory.
pub fn provenance(path: &Path, x_flatpak: Option<&str>, data_home: &Path) -> Provenance {
    // `..` or `.` would let a path name one directory and lie in another.
    if !path.components().all(|part| matches!(part, Component::RootDir | Component::Normal(_))) {
        return Provenance::User;
    }
    if path.starts_with(IMAGE_APPLICATIONS) {
        return Provenance::Image;
    }
    let exported = path
        .parent()
        .is_some_and(|dir| dir == Path::new(SYSTEM_FLATPAK_APPLICATIONS) || dir == data_home.join(USER_FLATPAK_APPLICATIONS));
    let app_id = path
        .file_name()
        .and_then(|name| name.to_str()?.strip_suffix(".desktop"))
        .filter(|app_id| !app_id.is_empty());
    match app_id {
        Some(app_id) if exported && x_flatpak.is_none_or(|claimed| claimed == app_id) => Provenance::Flatpak(app_id.to_owned()),
        _ => Provenance::User,
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Origin {
    pub remote: String,
    pub version: Option<String>,
}

pub fn flatpak_origin(listing: &str, app_id: &str) -> Option<Origin> {
    listing.lines().find_map(|line| {
        let mut fields = line.split('\t');
        (fields.next()? == app_id).then(|| Origin {
            remote: fields.next().unwrap_or_default().to_owned(),
            version: fields.next().filter(|version| !version.is_empty()).map(str::to_owned),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME: &str = "/var/home/u/.local/share";

    fn of(path: &str, x_flatpak: Option<&str>) -> Provenance {
        provenance(Path::new(path), x_flatpak, Path::new(HOME))
    }

    #[test]
    fn an_entry_of_the_image_is_the_system_image() {
        assert_eq!(of("/usr/share/applications/org.gnome.Nautilus.desktop", None), Provenance::Image);
        // The key does not make an image entry a Flatpak, nor a Flatpak the image.
        assert_eq!(of("/usr/share/applications/firefox.desktop", Some("org.mozilla.firefox")), Provenance::Image);
    }

    #[test]
    fn an_exported_entry_is_a_flatpak_named_by_its_file() {
        let system = "/var/lib/flatpak/exports/share/applications/org.mozilla.firefox.desktop";
        assert_eq!(of(system, Some("org.mozilla.firefox")), Provenance::Flatpak("org.mozilla.firefox".into()));
        assert_eq!(of(system, None), Provenance::Flatpak("org.mozilla.firefox".into()), "the stem alone names it");
        let user = format!("{HOME}/flatpak/exports/share/applications/org.gnome.Calculator.desktop");
        assert_eq!(of(&user, Some("org.gnome.Calculator")), Provenance::Flatpak("org.gnome.Calculator".into()));
        // An export whose key names another application is not believed.
        assert_eq!(of(system, Some("org.example.Other")), Provenance::User);
        assert_eq!(of("/var/lib/flatpak/exports/share/applications/sub/org.mozilla.firefox.desktop", None), Provenance::User);
        assert_eq!(of("/var/lib/flatpak/exports/share/applications/.desktop", None), Provenance::User);
    }

    #[test]
    fn anything_else_was_installed_by_the_user() {
        // A user entry that claims to be Firefox's Flatpak.
        assert_eq!(of(&format!("{HOME}/applications/org.mozilla.firefox.desktop"), Some("org.mozilla.firefox")), Provenance::User);
        assert_eq!(of("/usr/local/share/applications/x.desktop", None), Provenance::User);
        assert_eq!(of("/usr/share/../../var/home/u/x.desktop", None), Provenance::User, "no dot-dot out of the image");
        assert_eq!(of("/usr/shared/applications/x.desktop", None), Provenance::User, "a prefix of a name is not the directory");
    }

    const LISTING: &str = "org.mozilla.firefox\tflathub\t143.0\norg.gnome.Calculator\tfedora\t\n";

    #[test]
    fn the_remote_and_version_come_from_the_listing() {
        assert_eq!(
            flatpak_origin(LISTING, "org.mozilla.firefox"),
            Some(Origin { remote: "flathub".into(), version: Some("143.0".into()) })
        );
        assert_eq!(
            flatpak_origin(LISTING, "org.gnome.Calculator"),
            Some(Origin { remote: "fedora".into(), version: None })
        );
        assert_eq!(flatpak_origin(LISTING, "org.mozilla"), None, "a prefix is not a match");
    }
}
