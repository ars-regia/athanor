//! The time zone the clock shows (doc_bar.md, BR3). `$TZ` wins, as for every program.
//! Otherwise the zone is the target of /etc/localtime, which timedatectl relinks. GLib's
//! local zone is cached for the life of the process, so the bar names the zone itself.

use std::path::{Component, Path};

/// The zone identifier, or `None` for GLib's local zone.
pub fn zone(tz: Option<&str>, localtime: Option<&Path>) -> Option<String> {
    let tz = tz
        .map(|tz| tz.strip_prefix(':').unwrap_or(tz))
        .filter(|tz| !tz.is_empty());
    match tz {
        Some(tz) if Path::new(tz).is_absolute() => below_zoneinfo(Path::new(tz)),
        Some(tz) => Some(tz.to_string()),
        None => below_zoneinfo(localtime?),
    }
}

/// `Europe/Rome` from `…/zoneinfo/Europe/Rome`. Only plain names may follow `zoneinfo`.
fn below_zoneinfo(path: &Path) -> Option<String> {
    let mut components = path.components();
    components
        .by_ref()
        .find(|component| component.as_os_str() == "zoneinfo")?;
    let names = components
        .map(|component| match component {
            Component::Normal(name) => name.to_str(),
            _ => None,
        })
        .collect::<Option<Vec<&str>>>()?;
    (!names.is_empty()).then(|| names.join("/"))
}

/// The zone to show. An identifier GLib does not know falls back to local time; the flag
/// says so, for the caller to log.
pub fn time_zone(id: Option<&str>) -> (glib::TimeZone, bool) {
    match id {
        Some(id) => match glib::TimeZone::from_identifier(Some(id)) {
            Some(zone) => (zone, false),
            None => (glib::TimeZone::local(), true),
        },
        None => (glib::TimeZone::local(), false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_absolute_or_relative_link_gives_the_zone() {
        assert_eq!(
            zone(None, Some(Path::new("/usr/share/zoneinfo/Europe/Rome"))).as_deref(),
            Some("Europe/Rome")
        );
        assert_eq!(
            zone(
                None,
                Some(Path::new(
                    "../usr/share/zoneinfo/America/Argentina/Buenos_Aires"
                ))
            )
            .as_deref(),
            Some("America/Argentina/Buenos_Aires")
        );
    }

    #[test]
    fn tz_wins_without_its_colon_and_an_empty_tz_counts_as_unset() {
        let rome = Some(Path::new("/usr/share/zoneinfo/Europe/Rome"));
        assert_eq!(
            zone(Some(":Asia/Tokyo"), rome).as_deref(),
            Some("Asia/Tokyo")
        );
        assert_eq!(
            zone(Some("/usr/share/zoneinfo/UTC"), rome).as_deref(),
            Some("UTC")
        );
        assert_eq!(
            zone(Some("CET-1CEST,M3.5.0,M10.5.0/3"), rome).as_deref(),
            Some("CET-1CEST,M3.5.0,M10.5.0/3")
        );
        assert_eq!(zone(Some(""), rome).as_deref(), Some("Europe/Rome"));
    }

    #[test]
    fn a_link_that_is_absent_or_outside_zoneinfo_means_local_time() {
        assert_eq!(zone(None, None), None);
        assert_eq!(zone(None, Some(Path::new("/etc/athanor/localtime"))), None);
        assert_eq!(zone(None, Some(Path::new("/usr/share/zoneinfo"))), None);
        assert_eq!(
            zone(
                None,
                Some(Path::new("/usr/share/zoneinfo/../../../etc/passwd"))
            ),
            None
        );
    }

    #[test]
    fn a_zone_glib_knows_is_used() {
        let (utc, unknown) = time_zone(Some("UTC"));
        assert!(!unknown);
        assert_eq!(utc.identifier(), "UTC");
        // A POSIX rule needs no tzdata, so the test does not depend on the build image.
        let rule = "CET-1CEST,M3.5.0,M10.5.0/3";
        let (posix, unknown) = time_zone(Some(rule));
        assert!(!unknown);
        assert_eq!(posix.identifier(), rule);
    }

    #[test]
    fn an_unknown_zone_falls_back_to_local_time_and_says_so() {
        let (zone, unknown) = time_zone(Some("Mars/Olympus_Mons"));
        assert!(unknown);
        assert_eq!(zone.identifier(), glib::TimeZone::local().identifier());
        let (zone, unknown) = time_zone(None);
        assert!(!unknown);
        assert_eq!(zone.identifier(), glib::TimeZone::local().identifier());
    }
}
