//! GNOME search providers (LA2): each installed application that ships an
//! `org.gnome.Shell.SearchProvider2` `.ini` answers the query itself, on the session bus.
//! The provider of Nautilus is skipped: it queries localsearch too, and every file would
//! appear twice.
//!
//! Anything an installed application ships is untrusted: the `.ini` files are bounded and
//! validated, every call carries a timeout, and every string a
//! provider returns is bounded and cleaned before it reaches a row.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use athanor_unit::text;
use gio::glib::{self, Variant, VariantTy};
use gio::prelude::*;

use crate::files::within;
use crate::item::{Action, Group, Hit};
use crate::rank::Tier;

const GROUP: &str = "Shell Search Provider";
const IFACE: &str = "org.gnome.Shell.SearchProvider2";
pub const SKIPPED: &[&str] = &["org.gnome.Nautilus.desktop"];
/// The deadline of LA4, also the bus timeout of every call.
const TIMEOUT_MS: i32 = 1000;
/// A larger `.ini` is not a search provider description.
const MAX_INI_BYTES: u64 = 64 * 1024;
/// A result id longer than this is dropped, never cut: a cut id would activate another result.
const MAX_ID_BYTES: usize = 512;
/// Bytes read from a provider's name, description or icon string before cleaning.
const MAX_FIELD_CHARS: usize = 1024;
/// A larger GetResultMetas reply is refused whole.
const MAX_METAS_BYTES: usize = 64 * 1024;
const MAX_DESKTOP_ID_BYTES: usize = 255;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Provider {
    pub desktop_id: String,
    pub bus_name: String,
    pub object_path: String,
    /// The application's name, the title of its group.
    pub name: String,
}

/// `$XDG_DATA_HOME`, then every `XDG_DATA_DIRS` entry, each with the providers' suffix.
pub fn dirs() -> Vec<PathBuf> {
    suffixed(glib::user_data_dir(), glib::system_data_dirs())
}

fn suffixed(home: PathBuf, system: Vec<PathBuf>) -> Vec<PathBuf> {
    std::iter::once(home)
        .chain(system)
        .map(|dir| dir.join("gnome-shell/search-providers"))
        .collect()
}

/// The providers of installed applications; the first file of a desktop id wins.
pub fn discover(dirs: &[PathBuf]) -> Vec<Provider> {
    discover_with(dirs, &|id| gio_unix::DesktopAppInfo::new(id).map(|app| app.name().to_string()))
}

/// `installed` returns the name of an installed application by desktop id: a provider of an
/// application that is not installed is stale.
fn discover_with(dirs: &[PathBuf], installed: &dyn Fn(&str) -> Option<String>) -> Vec<Provider> {
    let mut seen = HashSet::new();
    let mut buses = HashSet::new();
    let mut providers = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        let mut files: Vec<_> = entries.filter_map(|entry| Some(entry.ok()?.path())).collect();
        files.sort();
        for file in files.into_iter().filter(|f| f.extension().is_some_and(|e| e == "ini")) {
            match read(&file, installed) {
                // Every .ini that parses claims its desktop id, disabled or not, so an
                // override in a user directory switches a system provider off, as on GNOME.
                Ok((id, provider)) => {
                    if seen.insert(id) {
                        if let Some(provider) = provider.filter(|p| buses.insert((p.bus_name.clone(), p.object_path.clone()))) {
                            providers.push(provider);
                        }
                    }
                }
                Err(err) => tracing::warn!(
                    "{} is not a search provider: {}",
                    text::line(&file.display().to_string(), text::SUMMARY_CHARS),
                    text::line(&err.to_string(), text::SUMMARY_CHARS)
                ),
            }
        }
    }
    providers
}

fn invalid(why: &str) -> glib::Error {
    glib::Error::new(gio::IOErrorEnum::InvalidData, why)
}

fn read(file: &Path, installed: &dyn Fn(&str) -> Option<String>) -> Result<(String, Option<Provider>), glib::Error> {
    let meta = std::fs::metadata(file).map_err(|err| invalid(&err.to_string()))?;
    if !meta.is_file() || meta.len() > MAX_INI_BYTES {
        return Err(invalid("not a regular file, or larger than 64 KiB"));
    }
    let keys = glib::KeyFile::new();
    keys.load_from_file(file, glib::KeyFileFlags::NONE)?;
    let desktop_id = keys.string(GROUP, "DesktopId")?.to_string();
    if keys.integer(GROUP, "Version")? != 2
        || keys.boolean(GROUP, "DefaultDisabled").unwrap_or(false)
        || SKIPPED.contains(&desktop_id.as_str())
    {
        return Ok((desktop_id, None));
    }
    let bus_name = keys.string(GROUP, "BusName")?.to_string();
    let object_path = keys.string(GROUP, "ObjectPath")?.to_string();
    if desktop_id.len() > MAX_DESKTOP_ID_BYTES
        || !gio::dbus_is_name(&bus_name)
        || gio::dbus_is_unique_name(&bus_name)
        || !Variant::is_object_path(&object_path)
    {
        return Err(invalid("DesktopId, BusName or ObjectPath is not valid"));
    }
    let Some(name) = installed(&desktop_id) else { return Ok((desktop_id, None)) };
    let name = text::line(&name, text::NAME_CHARS);
    Ok((desktop_id.clone(), Some(Provider { bus_name, object_path, name, desktop_id })))
}

fn terms(query: &str) -> Vec<String> {
    query.split_whitespace().map(str::to_owned).collect()
}

/// A call to a provider, started on demand by bus activation; it ends at the deadline.
async fn call(
    bus: &gio::DBusConnection,
    bus_name: &str,
    object_path: &str,
    method: &str,
    args: Variant,
    reply: &str,
) -> Result<Variant, glib::Error> {
    bus.call_future(
        Some(bus_name),
        object_path,
        IFACE,
        method,
        Some(&args),
        VariantTy::new(reply).ok(),
        // ponytail: a keystroke can start a provider's process, as on GNOME; the deadline
        // bounds the wait, a slow starter misses this query and answers the next one.
        gio::DBusCallFlags::NONE,
        TIMEOUT_MS,
    )
    .await
}

/// A string of the reply, at most `Group::PROVIDER_ROWS` of them.
fn strings(reply: &Variant) -> Option<Vec<String>> {
    let list = reply.try_child_value(0).filter(|list| list.type_().as_str() == "as")?;
    Some(
        list.iter()
            .take(Group::PROVIDER_ROWS)
            .filter_map(|id| id.str().filter(|id| !id.is_empty() && id.len() <= MAX_ID_BYTES).map(str::to_owned))
            .collect(),
    )
}

type Meta = HashMap<String, Variant>;

/// The metas of the reply; `None` when its type is not `(aa{sv})`.
fn metas(reply: &Variant) -> Option<Vec<Meta>> {
    if reply.size() > MAX_METAS_BYTES {
        return None;
    }
    let list = reply.try_child_value(0).filter(|list| list.type_().as_str() == "aa{sv}")?;
    list.iter().take(Group::PROVIDER_ROWS).map(|meta| meta.get::<Meta>()).collect()
}

/// At most `Group::PROVIDER_ROWS` results; `None` when the provider is unavailable, failed,
/// answered the wrong types or missed the deadline, which drops its group for this query (LA10).
pub async fn search(provider: &Provider, query: &str) -> Option<Vec<Hit>> {
    let terms = terms(query);
    if terms.is_empty() {
        return None;
    }
    let work = async {
        let bus = gio::bus_get_future(gio::BusType::Session).await?;
        let (name, path) = (provider.bus_name.as_str(), provider.object_path.as_str());
        let reply = call(&bus, name, path, "GetInitialResultSet", (terms.clone(),).to_variant(), "(as)").await?;
        let ids = strings(&reply).ok_or_else(|| invalid("GetInitialResultSet: not (as)"))?;
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let reply = call(&bus, name, path, "GetResultMetas", (ids,).to_variant(), "(aa{sv})").await?;
        let metas = metas(&reply).ok_or_else(|| invalid("GetResultMetas: not (aa{sv})"))?;
        Ok::<_, glib::Error>(metas_to_hits(provider, metas, &terms))
    };
    match within(Duration::from_millis(TIMEOUT_MS as u64), work).await {
        Some(Ok(hits)) => Some(hits),
        // No such service, and no .service file to start one.
        Some(Err(err)) if err.matches(gio::DBusError::ServiceUnknown) => {
            tracing::debug!(provider = %provider.bus_name, "the search provider is not available");
            None
        }
        Some(Err(err)) => {
            tracing::warn!(provider = %provider.bus_name, "the search provider failed: {}", text::line(&err.to_string(), text::SUMMARY_CHARS));
            None
        }
        None => {
            tracing::warn!(provider = %provider.bus_name, "the search provider did not answer in time");
            None
        }
    }
}

/// A bounded string of a meta; `None` when the key is absent or not a string.
fn field(meta: &Meta, key: &str) -> Option<String> {
    Some(meta.get(key)?.str()?.chars().take(MAX_FIELD_CHARS).collect())
}

/// A themed icon whose every name is a plain icon name: never a path.
fn plain_themed(icon: &gio::Icon) -> bool {
    icon.downcast_ref::<gio::ThemedIcon>().is_some_and(|themed| {
        themed.names().iter().all(|name| {
            !name.is_empty()
                && name.len() <= 255
                && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        })
    })
}

pub fn metas_to_hits(provider: &Provider, metas: Vec<Meta>, terms: &[String]) -> Vec<Hit> {
    metas
        .into_iter()
        .filter_map(|meta| {
            let id = meta.get("id")?.str().filter(|id| !id.is_empty() && id.len() <= MAX_ID_BYTES)?.to_owned();
            let title = text::line(&field(&meta, "name")?, text::NAME_CHARS);
            if title.is_empty() {
                return None;
            }
            let subtitle = text::line(&field(&meta, "description").unwrap_or_default(), text::SUMMARY_CHARS);
            let icon = meta
                .get("icon")
                .and_then(gio::Icon::deserialize)
                .or_else(|| field(&meta, "gicon").and_then(|s| gio::Icon::for_string(&s).ok()))
                .filter(plain_themed);
            Some(Hit {
                group: Group::Providers,
                key: String::new(),
                title,
                subtitle,
                icon,
                tier: Tier::Scattered,
                score: 0,
                learned: false,
                action: Action::Provider {
                    bus_name: provider.bus_name.clone(),
                    object_path: provider.object_path.clone(),
                    result_id: id,
                    terms: terms.to_vec(),
                },
            })
        })
        .take(Group::PROVIDER_ROWS)
        .collect()
}

/// The provider opens its own result (LA9, a declared limit): `ActivateResult(id, terms,
/// timestamp)`. Like a search it may start the provider, and it ends at the deadline.
pub async fn activate(
    bus_name: &str,
    object_path: &str,
    id: &str,
    terms: &[String],
    timestamp: u32,
) -> Result<(), glib::Error> {
    let bus = gio::bus_get_future(gio::BusType::Session).await?;
    call(&bus, bus_name, object_path, "ActivateResult", (id, terms.to_vec(), timestamp).to_variant(), "()")
        .await
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
    }

    /// The installed check against the applications fixtures, not the process environment.
    fn fixture_apps(id: &str) -> Option<String> {
        let app = gio_unix::DesktopAppInfo::from_filename(base().join("applications").join(id))?;
        Some(app.name().to_string())
    }

    fn provider() -> Provider {
        Provider {
            desktop_id: "org.mozilla.firefox.desktop".into(),
            bus_name: "org.example.P".into(),
            object_path: "/org/example/P".into(),
            name: "Example".into(),
        }
    }

    #[test]
    fn version_2_enabled_installed_providers_are_read_once_and_nautilus_is_skipped() {
        let dir = base().join("search-providers");
        let providers = discover_with(&[dir.clone(), dir], &fixture_apps);
        let buses: Vec<_> = providers.iter().map(|p| p.bus_name.as_str()).collect();
        assert_eq!(buses, ["org.gnome.Calculator.SearchProvider"]);
        assert_eq!(providers[0].object_path, "/org/gnome/Calculator/SearchProvider");
        assert_eq!(providers[0].name, "Firefox");
    }

    fn write_ini(dir: &Path, name: &str, id: &str, bus: &str, extra: &str) {
        std::fs::create_dir_all(dir).expect("dir");
        let body = format!("[Shell Search Provider]\nDesktopId={id}\nBusName={bus}\nObjectPath=/a/b\nVersion=2\n{extra}");
        std::fs::write(dir.join(name), body).expect("write");
    }

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("athanor-providers-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn a_disabled_ini_in_an_earlier_directory_switches_the_provider_off() {
        let root = scratch("override");
        write_ini(&root.join("a"), "x.ini", "org.mozilla.firefox.desktop", "org.example.A", "DefaultDisabled=true\n");
        write_ini(&root.join("b"), "x.ini", "org.mozilla.firefox.desktop", "org.example.A", "");
        assert!(discover_with(&[root.join("a"), root.join("b")], &fixture_apps).is_empty());
        assert_eq!(discover_with(&[root.join("b")], &fixture_apps).len(), 1);
        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn providers_are_keyed_on_bus_name_and_path() {
        let root = scratch("bus");
        write_ini(&root, "a.ini", "org.mozilla.firefox.desktop", "org.example.Same", "");
        write_ini(&root, "b.ini", "bidi.desktop", "org.example.Same", "");
        let got = discover_with(std::slice::from_ref(&root), &|_| Some("X".into()));
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].desktop_id, "org.mozilla.firefox.desktop");
        let body = |id: &str, path: &str| format!("[Shell Search Provider]\nDesktopId={id}\nBusName=org.example.Same\nObjectPath={path}\nVersion=2\n");
        std::fs::write(root.join("b.ini"), body("bidi.desktop", "/other")).expect("write");
        assert_eq!(discover_with(std::slice::from_ref(&root), &|_| Some("X".into())).len(), 2);
        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn only_themed_icons_survive() {
        let themed = gio::ThemedIcon::new("folder").serialize().expect("themed");
        let bytes = gio::BytesIcon::new(&glib::Bytes::from_static(b"x")).serialize().expect("bytes");
        let file = gio::FileIcon::new(&gio::File::for_path("/etc/passwd")).serialize().expect("file");
        let icon_of = |icon: Variant| {
            let mut meta = Meta::new();
            meta.insert("id".to_owned(), "r".to_variant());
            meta.insert("name".to_owned(), "n".to_variant());
            meta.insert("icon".to_owned(), icon);
            let hits = metas_to_hits(&provider(), vec![meta], &[]);
            assert_eq!(hits.len(), 1);
            hits[0].icon.clone()
        };
        assert!(icon_of(themed).is_some());
        assert!(icon_of(bytes).is_none());
        assert!(icon_of(file).is_none());
        let named = |name: &str| icon_of(gio::ThemedIcon::new(name).serialize().expect("themed"));
        assert!(named("../../etc/x").is_none());
        assert!(named("/tmp/x").is_none());
        assert!(named("org.gnome.Calculator-symbolic").is_some());
    }

    #[test]
    fn a_large_metas_reply_is_refused() {
        let big: Vec<Meta> = (0..3)
            .map(|_| Meta::from([("name".to_owned(), "x".repeat(40_000).to_variant())]))
            .collect();
        assert!(metas(&(big,).to_variant()).is_none());
    }

    #[test]
    fn an_application_that_is_not_installed_leaves_a_stale_provider_out() {
        let dir = base().join("search-providers");
        assert!(discover_with(&[dir], &|_| None).is_empty());
    }

    #[test]
    fn dirs_list_the_data_home_then_every_system_dir_with_the_suffix() {
        let got = suffixed("/h".into(), vec!["/a".into(), "/b".into()]);
        let want: Vec<PathBuf> = ["/h", "/a", "/b"].iter().map(|d| Path::new(d).join("gnome-shell/search-providers")).collect();
        assert_eq!(got, want);
        assert!(dirs().iter().all(|d| d.ends_with("gnome-shell/search-providers")));
    }

    #[test]
    fn an_oversized_or_invalid_ini_is_refused() {
        let dir = std::env::temp_dir().join(format!("athanor-providers-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let big = dir.join("big.ini");
        std::fs::write(&big, format!("[Shell Search Provider]\nDesktopId=x.desktop\nBusName=a.b\nObjectPath=/a\nVersion=2\n#{}\n", "x".repeat(70_000))).expect("write");
        assert!(read(&big, &|_| Some("X".into())).is_err());
        let bad = dir.join("bad.ini");
        std::fs::write(&bad, "[Shell Search Provider]\nDesktopId=x.desktop\nBusName=not a name\nObjectPath=/a\nVersion=2\n").expect("write");
        assert!(read(&bad, &|_| Some("X".into())).is_err());
        let path = dir.join("path.ini");
        std::fs::write(&path, "[Shell Search Provider]\nDesktopId=x.desktop\nBusName=a.b\nObjectPath=a//b\nVersion=2\n").expect("write");
        assert!(read(&path, &|_| Some("X".into())).is_err());
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn result_metas_become_plain_text_hits() {
        let mut meta = Meta::new();
        meta.insert("id".to_owned(), "r1".to_variant());
        meta.insert("name".to_owned(), "Evil\u{202e}txt".to_variant());
        meta.insert("description".to_owned(), "line one\nline two".to_variant());
        let hits = metas_to_hits(&provider(), vec![meta], &["evil".to_owned()]);
        assert_eq!(hits.len(), 1);
        assert!(!hits[0].title.contains('\u{202e}'));
        assert!(!hits[0].subtitle.contains('\n'));
        assert_eq!(
            hits[0].action,
            Action::Provider {
                bus_name: "org.example.P".into(),
                object_path: "/org/example/P".into(),
                result_id: "r1".into(),
                terms: vec!["evil".into()],
            }
        );
    }

    #[test]
    fn a_meta_without_an_id_or_a_usable_name_is_dropped() {
        let mut no_id = Meta::new();
        no_id.insert("name".to_owned(), "x".to_variant());
        let mut blank = Meta::new();
        blank.insert("id".to_owned(), "r".to_variant());
        blank.insert("name".to_owned(), "\u{202e}\n".to_variant());
        let mut wrong_type = Meta::new();
        wrong_type.insert("id".to_owned(), 7u32.to_variant());
        wrong_type.insert("name".to_owned(), "x".to_variant());
        let mut long_id = Meta::new();
        long_id.insert("id".to_owned(), "i".repeat(MAX_ID_BYTES + 1).to_variant());
        long_id.insert("name".to_owned(), "x".to_variant());
        assert!(metas_to_hits(&provider(), vec![no_id, blank, wrong_type, long_id], &[]).is_empty());
    }

    #[test]
    fn replies_are_capped_and_a_wrong_type_is_none() {
        let many: Vec<String> = (0..50).map(|i| format!("r{i}")).collect();
        assert_eq!(strings(&(many,).to_variant()).map(|ids| ids.len()), Some(Group::PROVIDER_ROWS));
        assert!(strings(&(7u32,).to_variant()).is_none());
        assert!(strings(&(vec!["x".repeat(MAX_ID_BYTES + 1)],).to_variant()).is_some_and(|ids| ids.is_empty()));
        let rows: Vec<Meta> = (0..50).map(|_| Meta::new()).collect();
        assert_eq!(metas(&(rows,).to_variant()).map(|m| m.len()), Some(Group::PROVIDER_ROWS));
        assert!(metas(&(vec!["x"],).to_variant()).is_none());
    }
}
