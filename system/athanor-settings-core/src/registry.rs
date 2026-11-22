//! The page registry (doc_settings.md SE2, SE3, SE4): every page in its group, the sections
//! each offers, the translated keywords the search matches, and what a page or a section needs
//! to be listed. The ids are a stable interface: callers open them through `Show` (SE4).

use crate::i18n::tr;

/// The sidebar's groups, in their order (SE3). Notifications and Accessibility are groups of
/// one page and show no heading of their own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Connections,
    Device,
    Personalisation,
    Notifications,
    Accessibility,
    System,
}

impl Group {
    /// Every group, in the sidebar's order.
    pub const ALL: [Group; 6] = [
        Group::Connections,
        Group::Device,
        Group::Personalisation,
        Group::Notifications,
        Group::Accessibility,
        Group::System,
    ];

    /// The heading the sidebar shows above the group's pages, if any.
    pub fn heading(self) -> Option<String> {
        match self {
            Group::Connections => Some(tr("Connections")),
            Group::Device => Some(tr("Device")),
            Group::Personalisation => Some(tr("Personalisation")),
            Group::Notifications | Group::Accessibility => None,
            Group::System => Some(tr("System")),
        }
    }
}

/// What must be present for a page or a section to be listed (SE3: "a page whose service or
/// hardware is absent is not listed"). The program answers each; the registry only names them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Need {
    /// A Bluetooth adapter, through BlueZ.
    BluetoothAdapter,
    /// fwupd on the system bus (SE22).
    Fwupd,
    /// A fingerprint reader, through fprintd's `GetDefaultDevice` (SE20).
    FingerprintReader,
}

/// A section of a page: a target of its own (`page/section`, SE4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    pub id: &'static str,
    pub title: String,
    pub keywords: Vec<String>,
    pub needs: Option<Need>,
}

/// One page of the sidebar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Page {
    pub id: &'static str,
    pub group: Group,
    pub title: String,
    pub keywords: Vec<String>,
    pub sections: Vec<Section>,
    pub needs: Option<Need>,
}

impl Page {
    /// The section `id` of this page.
    pub fn section(&self, id: &str) -> Option<&Section> {
        self.sections.iter().find(|section| section.id == id)
    }
}

/// A search result: a page, or a section of it, which the window opens (SE3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit<'a> {
    pub page: &'a Page,
    pub section: Option<&'a Section>,
}

/// Every page of SE3 and SE4, translated by the catalog set at the time it is built.
#[derive(Clone, Debug)]
pub struct Registry {
    pages: Vec<Page>,
}

/// Keywords as translators write them in a desktop entry: separated by `;`.
fn keywords(list: &str) -> Vec<String> {
    list.split(';')
        .map(str::trim)
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect()
}

fn page(id: &'static str, group: Group, title: String, words: String) -> Page {
    Page {
        id,
        group,
        title,
        keywords: keywords(&words),
        sections: Vec::new(),
        needs: None,
    }
}

fn section(id: &'static str, title: String, words: String) -> Section {
    Section {
        id,
        title,
        keywords: keywords(&words),
        needs: None,
    }
}

impl Registry {
    /// The registry, its strings translated now.
    pub fn new() -> Self {
        use Group::*;
        // TRANSLATORS: the keywords of each page are search terms separated by ";". Keep the
        // English ones a user might still type, and add the words of your language.
        let mut pages = vec![
            page("network", Connections, tr("Network"), tr("network;wi-fi;wifi;wireless;ethernet;wired;internet;vpn;wireguard;hotspot;proxy;dns;mobile broadband;")),
            page("bluetooth", Connections, tr("Bluetooth"), tr("bluetooth;pairing;headphones;wireless;")),
            page("displays", Device, tr("Displays"), tr("display;monitor;screen;resolution;refresh rate;scale;rotation;night light;brightness;")),
            page("sound", Device, tr("Sound"), tr("sound;audio;volume;speakers;microphone;headset;output;input;alert;")),
            page("power", Device, tr("Power"), tr("power;battery;charge;energy;profile;suspend;sleep;blank;")),
            page("keyboard", Device, tr("Keyboard"), tr("keyboard;layout;shortcuts;keys;repeat;input method;input source;num lock;")),
            page("mouse", Device, tr("Mouse and Touchpad"), tr("mouse;touchpad;trackpad;pointer;scrolling;tap;click;tablet;touchscreen;")),
            page("appearance", Personalisation, tr("Appearance"), tr("appearance;theme;dark;light;accent;colour;color;contrast;")),
            page("wallpaper", Personalisation, tr("Wallpaper"), tr("wallpaper;background;picture;")),
            page("desktop", Personalisation, tr("Desktop"), tr("desktop;layout;panel;bar;dock;workspaces;windows;tiling;hot corner;")),
            page("notifications", Notifications, tr("Notifications"), tr("notifications;alerts;do not disturb;popups;")),
            page("accessibility", Accessibility, tr("Accessibility"), tr("accessibility;screen reader;magnifier;zoom;contrast;large text;cursor;motion;animations;sticky keys;on-screen keyboard;mono audio;")),
            page("region", System, tr("Language and Region"), tr("language;region;locale;formats;translation;")),
            page("datetime", System, tr("Date and Time"), tr("date;time;clock;time zone;calendar;week;")),
            page("users", System, tr("Users"), tr("users;account;name;picture;avatar;password;fingerprint;")),
            page("privacy", System, tr("Privacy and Security"), tr("privacy;security;recent files;permissions;encryption;developer;usb;")),
            page("search", System, tr("Search"), tr("search;index;files;folders;")),
            page("default-apps", System, tr("Default Applications"), tr("default applications;browser;mail;music;video;photos;terminal;startup;autostart;")),
            page("firmware", System, tr("Firmware"), tr("firmware;bios;uefi;fwupd;device updates;")),
            page("about", System, tr("About"), tr("about;system;version;device name;hostname;hardware;processor;memory;trust;")),
        ];
        for page in &mut pages {
            match page.id {
                "bluetooth" => page.needs = Some(Need::BluetoothAdapter),
                "firmware" => page.needs = Some(Need::Fwupd),
                "notifications" => page.sections.push(section(
                    "schedule",
                    tr("Do Not Disturb"),
                    tr("do not disturb;schedule;quiet;"),
                )),
                "privacy" => {
                    page.sections.push(section(
                        "developer-mode",
                        tr("Developer mode"),
                        tr("developer;development;"),
                    ));
                    page.sections.push(section(
                        "usb",
                        tr("USB devices"),
                        tr("usb;usbguard;blocked devices;"),
                    ));
                }
                "users" => {
                    let mut finger = section(
                        "fingerprint",
                        tr("Fingerprint"),
                        tr("fingerprint;finger;reader;"),
                    );
                    finger.needs = Some(Need::FingerprintReader);
                    page.sections.push(finger);
                }
                "displays" => page.sections.push(section(
                    "mode",
                    tr("Display mode"),
                    tr("display mode;mirror;extend;projector;"),
                )),
                "power" => page.sections.push(section(
                    "idle",
                    tr("Blank and suspend"),
                    tr("blank screen;screen off;suspend;sleep;idle;"),
                )),
                _ => {}
            }
        }
        Self { pages }
    }

    /// Every page, in the sidebar's order.
    pub fn pages(&self) -> &[Page] {
        &self.pages
    }

    /// The page `id`.
    pub fn page(&self, id: &str) -> Option<&Page> {
        self.pages.iter().find(|page| page.id == id)
    }

    /// The pages and sections whose title or keywords hold every word of `query`, ignoring
    /// case: pages whose title matches first, then the other pages, then sections, each in the
    /// sidebar's order. An empty query matches nothing.
    pub fn search(&self, query: &str) -> Vec<Hit<'_>> {
        let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        if words.is_empty() {
            return Vec::new();
        }
        let in_title = |title: &str| {
            let title = title.to_lowercase();
            words.iter().all(|word| title.contains(word.as_str()))
        };
        let anywhere = |title: &str, keywords: &[String]| {
            let title = title.to_lowercase();
            words.iter().all(|word| {
                title.contains(word.as_str())
                    || keywords.iter().any(|kw| kw.contains(word.as_str()))
            })
        };
        let mut hits: Vec<Hit<'_>> = Vec::new();
        let titled = self.pages.iter().filter(|page| in_title(&page.title));
        let keyed = self
            .pages
            .iter()
            .filter(|page| !in_title(&page.title) && anywhere(&page.title, &page.keywords));
        hits.extend(titled.chain(keyed).map(|page| Hit {
            page,
            section: None,
        }));
        for page in &self.pages {
            for section in &page.sections {
                if anywhere(&section.title, &section.keywords) {
                    hits.push(Hit {
                        page,
                        section: Some(section),
                    });
                }
            }
        }
        hits
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SE4's page ids, in SE3's order.
    const IDS: [&str; 20] = [
        "network",
        "bluetooth",
        "displays",
        "sound",
        "power",
        "keyboard",
        "mouse",
        "appearance",
        "wallpaper",
        "desktop",
        "notifications",
        "accessibility",
        "region",
        "datetime",
        "users",
        "privacy",
        "search",
        "default-apps",
        "firmware",
        "about",
    ];

    #[test]
    fn the_registry_holds_the_twenty_pages_of_se4_in_the_order_of_se3() {
        let registry = Registry::new();
        let ids: Vec<&str> = registry.pages().iter().map(|page| page.id).collect();
        assert_eq!(ids, IDS);
        let groups: Vec<Group> = registry.pages().iter().map(|page| page.group).collect();
        let mut seen: Vec<Group> = groups.clone();
        seen.dedup();
        assert_eq!(
            seen,
            Group::ALL,
            "each group's pages are together, in order"
        );
        assert_eq!(
            registry.page("mouse").map(|p| p.title.as_str()),
            Some("Mouse and Touchpad")
        );
    }

    #[test]
    fn the_sections_the_document_names_are_registered() {
        let registry = Registry::new();
        for target in [
            "notifications/schedule",
            "privacy/developer-mode",
            "privacy/usb",
            "users/fingerprint",
            "displays/mode",
            "power/idle",
        ] {
            let (page, section) = target.split_once('/').expect("a section target");
            assert!(
                registry
                    .page(page)
                    .and_then(|p| p.section(section))
                    .is_some(),
                "{target} is not registered"
            );
        }
        let sections: usize = registry.pages().iter().map(|p| p.sections.len()).sum();
        assert_eq!(sections, 6, "no section the document does not name");
    }

    #[test]
    fn a_page_that_needs_a_service_or_hardware_says_which() {
        let registry = Registry::new();
        let need = |id: &str| registry.page(id).and_then(|p| p.needs);
        assert_eq!(need("bluetooth"), Some(Need::BluetoothAdapter));
        assert_eq!(need("firmware"), Some(Need::Fwupd));
        assert_eq!(need("about"), None);
        let finger = registry
            .page("users")
            .and_then(|p| p.section("fingerprint"));
        assert_eq!(finger.and_then(|s| s.needs), Some(Need::FingerprintReader));
        assert_eq!(Group::Notifications.heading(), None);
        assert_eq!(Group::System.heading().as_deref(), Some("System"));
    }

    fn found(registry: &Registry, query: &str) -> Vec<String> {
        registry
            .search(query)
            .iter()
            .map(|hit| match hit.section {
                Some(section) => format!("{}/{}", hit.page.id, section.id),
                None => hit.page.id.to_owned(),
            })
            .collect()
    }

    #[test]
    fn search_matches_titles_before_keywords_and_ignores_case() {
        let registry = Registry::new();
        assert_eq!(found(&registry, "SOUND")[0], "sound");
        assert!(found(&registry, "wi-fi").contains(&"network".to_owned()));
        // "contrast" is a keyword of two pages; neither title holds it.
        assert_eq!(
            found(&registry, "contrast"),
            ["appearance", "accessibility"]
        );
        assert!(found(&registry, "  ").is_empty());
        assert!(found(&registry, "no such thing").is_empty());
    }

    #[test]
    fn search_opens_a_section_and_needs_every_word() {
        let registry = Registry::new();
        let hits = found(&registry, "do not disturb");
        assert!(hits.contains(&"notifications/schedule".to_owned()));
        assert!(found(&registry, "developer mode").contains(&"privacy/developer-mode".to_owned()));
        assert_eq!(found(&registry, "blank suspend"), ["power", "power/idle"]);
        assert!(!found(&registry, "blank wallpaper").contains(&"power".to_owned()));
    }
}
