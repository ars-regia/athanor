//! Where an application comes from (doc_launcher.md, LA6): the Flatpak remote and version,
//! read from `flatpak list --app --columns=application,origin,version`, whose output is
//! tab-separated and never translated; or the system image.

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
