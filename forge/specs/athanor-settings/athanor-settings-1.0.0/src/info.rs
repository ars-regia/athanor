//! What the landing page and About show (doc_settings.md SE3, SE22), all of it read without
//! privilege: the user's full name and picture from AccountsService, the device's names from
//! hostnamed, the trust state from the root-owned state file (SE22), and the system's own files. The bus
//! replies arrive on GLib's main context and land in [`Info`]; the window draws them in its
//! next turn.

use std::cell::RefCell;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use athanor_settings_core::about;
use athanor_trust_state::{Badge, ReadError, State};
use gio::prelude::*;

/// How long each service gets to answer, as the bar gives the update service.
const ANSWER_WITHIN_MS: i32 = 5_000;
const OS_RELEASE: &str = "/usr/lib/os-release";
const PCI_IDS: &str = "/usr/share/hwdata/pci.ids";
const DRM: &str = "/sys/class/drm";

/// The answers that came so far; `None` until one comes, and for good when none does.
#[derive(Debug, Default)]
pub struct Info {
    pub real_name: Option<String>,
    pub picture: Option<PathBuf>,
    pub pretty_hostname: Option<String>,
    pub hostname: Option<String>,
    pub vendor: Option<String>,
    pub model: Option<String>,
    /// The trust state file as read, or why it backs nothing.
    pub trust: Option<Result<State, ReadError>>,
    /// Set by each answer, cleared by the window once it has drawn it.
    pub changed: bool,
}

impl Info {
    /// The device's name as the cards show it: the pretty name, else the hostname.
    pub fn device_name(&self) -> Option<&str> {
        self.pretty_hostname
            .as_deref()
            .filter(|name| !name.is_empty())
            .or(self.hostname.as_deref())
    }

    /// The trust badge of SH12: a state not yet read, or not trusted, backs nothing.
    pub fn badge(&self) -> Option<Badge> {
        let read = self.trust.as_ref()?;
        Some(read.as_ref().map_or(Badge::Attention, |state| {
            athanor_trust_state::badge(state, now())
        }))
    }
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX)
        })
}

/// A string property of an `a{sv}`, when it is one and not empty.
fn text(dict: &glib::VariantDict, key: &str) -> Option<String> {
    dict.lookup::<String>(key)
        .ok()
        .flatten()
        .filter(|value| !value.is_empty())
}

/// `GetAll` of `interface` on `object` of `name`, handed to `done` as a dictionary.
fn get_all(
    bus: &gio::DBusConnection,
    name: &str,
    object: &str,
    interface: &'static str,
    done: impl FnOnce(glib::VariantDict) + 'static,
) {
    bus.call(
        Some(name),
        object,
        "org.freedesktop.DBus.Properties",
        "GetAll",
        Some(&(interface,).to_variant()),
        Some(glib::VariantTy::new("(a{sv})").unwrap_or(glib::VariantTy::ANY)),
        gio::DBusCallFlags::NONE,
        ANSWER_WITHIN_MS,
        gio::Cancellable::NONE,
        move |reply| match reply {
            Ok(reply) => done(glib::VariantDict::new(Some(&reply.child_value(0)))),
            Err(error) => tracing::warn!(%error, interface, "no answer; the page shows less"),
        },
    );
}

/// Reads the trust state file, then asks hostnamed and AccountsService on the system bus.
///
/// The file is the shield's (BR6), and Settings' unit has no user namespace, so its owner reads
/// as root and `athanor_trust_state::read` checks it itself. A missing or refused file shows
/// "Not verified"; the update service is not asked, since the image's update state lives in
/// the shield (SE22, SW16).
pub fn fetch(info: &Rc<RefCell<Info>>) {
    let read = athanor_trust_state::read();
    match &read {
        Ok(_) | Err(ReadError::Missing) => {}
        Err(error) => tracing::warn!(?error, "the trust state file is not trusted"),
    }
    {
        let mut info = info.borrow_mut();
        info.trust = Some(read);
        info.changed = true;
    }
    let info = info.clone();
    gio::bus_get(gio::BusType::System, gio::Cancellable::NONE, move |bus| {
        let bus = match bus {
            Ok(bus) => bus,
            Err(error) => {
                tracing::warn!(%error, "no system bus; the pages show what the files say");
                return;
            }
        };
        let names = info.clone();
        get_all(
            &bus,
            "org.freedesktop.hostname1",
            "/org/freedesktop/hostname1",
            "org.freedesktop.hostname1",
            move |dict| {
                let mut info = names.borrow_mut();
                info.pretty_hostname = text(&dict, "PrettyHostname");
                info.hostname = text(&dict, "Hostname");
                info.vendor = text(&dict, "HardwareVendor");
                info.model = text(&dict, "HardwareModel");
                info.changed = true;
            },
        );
        user(&bus, &info);
    });
}

fn user(bus: &gio::DBusConnection, info: &Rc<RefCell<Info>>) {
    let uid = match fs::metadata("/proc/self") {
        Ok(meta) => u64::from(meta.uid()),
        Err(error) => {
            tracing::warn!(%error, "cannot tell this process's user");
            return;
        }
    };
    let (info, account) = (info.clone(), bus.clone());
    bus.call(
        Some("org.freedesktop.Accounts"),
        "/org/freedesktop/Accounts",
        "org.freedesktop.Accounts",
        "FindUserById",
        Some(&(uid as i64,).to_variant()),
        Some(glib::VariantTy::new("(o)").unwrap_or(glib::VariantTy::ANY)),
        gio::DBusCallFlags::NONE,
        ANSWER_WITHIN_MS,
        gio::Cancellable::NONE,
        move |reply| {
            let path = match reply {
                Ok(reply) => reply.child_value(0).str().map(str::to_owned),
                Err(error) => {
                    tracing::warn!(%error, "AccountsService does not know this user");
                    None
                }
            };
            let Some(path) = path else { return };
            get_all(
                &account,
                "org.freedesktop.Accounts",
                &path,
                "org.freedesktop.Accounts.User",
                move |dict| {
                    let mut info = info.borrow_mut();
                    info.real_name = text(&dict, "RealName").or_else(|| text(&dict, "UserName"));
                    info.picture = text(&dict, "IconFile").map(PathBuf::from);
                    info.changed = true;
                },
            );
        },
    );
}

/// The system's name and version, from os-release.
pub fn system() -> Option<String> {
    let text = fs::read_to_string(OS_RELEASE)
        .map_err(|error| tracing::warn!(%error, "cannot read {OS_RELEASE}"))
        .ok()?;
    let name = about::os_release(&text, "NAME").unwrap_or("Athanor");
    Some(match about::os_release(&text, "VERSION") {
        Some(version) => format!("{name} {version}"),
        None => name.to_owned(),
    })
}

/// The About page's hardware rows, read from `/proc`, `/sys` and the filesystem.
pub struct Hardware {
    pub processor: Option<String>,
    pub memory: Option<String>,
    pub graphics: Vec<String>,
    pub disk: Option<String>,
}

pub fn hardware() -> Hardware {
    let processor = fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|text| about::processor(&text))
        .map(|(model, count)| format!("{model} \u{d7} {count}"));
    let memory = fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|text| about::memory_bytes(&text))
        .map(about::size);
    let disk = nix::sys::statvfs::statvfs("/var")
        .ok()
        .and_then(|fs| fs.fragment_size().checked_mul(fs.blocks()))
        .map(about::size);
    Hardware {
        processor,
        memory,
        graphics: graphics(),
        disk,
    }
}

/// Every DRM card's adapter, named from pci.ids.
fn graphics() -> Vec<String> {
    let Ok(cards) = fs::read_dir(DRM) else {
        return Vec::new();
    };
    let ids = fs::read_to_string(PCI_IDS).unwrap_or_default();
    let mut names: Vec<String> = cards
        .filter_map(Result::ok)
        .map(|card| card.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("card") && !name.contains('-'))
        })
        .filter_map(|card| {
            let read =
                |file: &str| fs::read_to_string(Path::new(&card).join("device").join(file)).ok();
            about::pci_name(&ids, &read("vendor")?, &read("device")?)
        })
        .collect();
    names.sort();
    names.dedup();
    names
}
