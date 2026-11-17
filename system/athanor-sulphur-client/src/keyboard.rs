//! The keyboard layouts of LN9's store, cosmic-comp's `xkb_config` (doc_languages.md LN9).
//! Under cosmic-comp the store names the layouts: a `wl_keyboard.keymap` also brings every
//! virtual keyboard's keymap, such as wtype's, to an unfocused client, and that keymap names
//! no layout. Each name is the English `<description>` of `evdev.xml`, which is the
//! `name[Group]` line of the layout's symbols and athanor-sulphur's `layouts` event.
//! LN9's `write()` and `available()`, and the model, options and repeat of `read()`, are not
//! here yet: nothing calls them.

use std::fs;
use std::path::PathBuf;

use gio::prelude::*;

use crate::connection::State;
use crate::cosmic_config;

const COMPONENT: &str = "com.system76.CosmicComp";
const KEY: &str = "xkb_config";
const EVDEV: &str = "/usr/share/X11/xkb/rules/evdev.xml";

/// The store's ordered layouts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct XkbLayouts {
    pub layouts: Vec<Layout>,
}

/// One layout and its variant, empty for the layout's own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    pub layout: String,
    pub variant: String,
}

/// A store that is not cosmic-config's record of `XkbConfig`.
#[derive(Debug, thiserror::Error)]
#[error("xkb_config is not a record of XkbConfig")]
pub struct Malformed;

/// The user's layouts: the user's store, else the system's default; none without either.
pub fn read() -> Result<XkbLayouts, Malformed> {
    Store::session().read().map(Option::unwrap_or_default)
}

/// Where the store and the names are read.
pub(crate) struct Store {
    /// cosmic-config's directories, the one whose keys change while a session runs first.
    dirs: Vec<PathBuf>,
    evdev: PathBuf,
}

impl Store {
    pub(crate) fn session() -> Self {
        Self {
            dirs: cosmic_config::user_dir()
                .into_iter()
                .chain(cosmic_config::system_dirs())
                .collect(),
            evdev: PathBuf::from(EVDEV),
        }
    }

    /// `None` without a store: cosmic-comp's keymap is then the one `XKB_DEFAULT_*` gives
    /// (doc_own_stack.md decision 27).
    fn read(&self) -> Result<Option<XkbLayouts>, Malformed> {
        cosmic_config::key(&self.dirs, COMPONENT, KEY)
            .map(|text| parse(&text))
            .transpose()
    }

    /// The layouts' names in group order; `None`, logged, without a store or for a malformed
    /// one, where the names the client holds stay.
    pub(crate) fn names(&self) -> Option<Vec<String>> {
        let layouts = match self.read() {
            Ok(Some(layouts)) => layouts,
            Ok(None) => {
                tracing::info!("no xkb_config: the keymap names the keyboard layouts");
                return None;
            }
            Err(err) => {
                tracing::warn!("the keyboard layouts are kept: {err}");
                return None;
            }
        };
        if layouts.layouts.is_empty() {
            return Some(Vec::new());
        }
        let evdev = fs::read_to_string(&self.evdev).unwrap_or_else(|err| {
            tracing::warn!(path = %self.evdev.display(), "the layouts are named by their codes: {err}");
            String::new()
        });
        Some(
            layouts
                .layouts
                .iter()
                .map(|layout| layout.name(&evdev))
                .collect(),
        )
    }

    /// The store of the first directory, the one that changes.
    fn changing(&self) -> PathBuf {
        self.dirs.first().map_or_else(PathBuf::new, |dir| {
            cosmic_config::component(dir, COMPONENT).join(KEY)
        })
    }

    /// Calls `changed` with the names after each change of the first directory's store.
    pub(crate) fn follow(
        self,
        changed: impl Fn(Vec<String>) + 'static,
    ) -> Result<gio::FileMonitor, glib::Error> {
        let monitor = gio::File::for_path(self.changing())
            .monitor_file(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE)?;
        monitor.connect_changed(move |_, _, _, event| {
            use gio::FileMonitorEvent::{ChangesDoneHint, Created, Deleted, MovedIn, Renamed};
            if matches!(
                event,
                ChangesDoneHint | Created | Deleted | MovedIn | Renamed
            ) {
                if let Some(names) = self.names() {
                    changed(names);
                }
            }
        });
        Ok(monitor)
    }
}

impl Layout {
    /// `evdev.xml`'s description, else the codes, as `us(intl)`.
    fn name(&self, evdev: &str) -> String {
        description(evdev, &self.layout, &self.variant).unwrap_or_else(|| {
            match self.variant.as_str() {
                "" => self.layout.clone(),
                variant => format!("{}({variant})", self.layout),
            }
        })
    }
}

impl State {
    /// Under cosmic-comp the store names the layouts from now on, and a keymap no longer does.
    pub(crate) fn store_layouts(&mut self, names: Vec<String>) {
        self.layouts_from_store = true;
        self.set_layouts(names);
    }
}

/// The `layout` and `variant` fields of cosmic-config's record (`ron::ser::to_string_pretty`
/// of cosmic-comp-config's `XkbConfig`, or the rig's one-line form), as athanor-sulphur's
/// `xkb_config.rs` reads them; the other fields are skipped. A variant list shorter than the
/// layouts leaves the rest at the layout's own.
fn parse(text: &str) -> Result<XkbLayouts, Malformed> {
    let mut rest = text.trim().strip_prefix('(').ok_or(Malformed)?;
    let (mut layout, mut variant) = (None, None);
    loop {
        rest = rest.trim_start();
        if let Some(after) = rest.strip_prefix(')') {
            if !after.trim().is_empty() {
                return Err(Malformed);
            }
            break;
        }
        let (name, after) = rest.split_once(':').ok_or(Malformed)?;
        let after = after.trim_start();
        let (value, after) = match after.strip_prefix('"') {
            Some(quoted) => string(quoted).map(|(value, after)| (Some(value), after))?,
            None => (None, skip(after)),
        };
        match name.trim() {
            "layout" => layout = value,
            "variant" => variant = value,
            _ => {}
        }
        let after = after.trim_start();
        rest = after.strip_prefix(',').unwrap_or(after);
        if rest.len() == after.len() && !after.starts_with(')') {
            return Err(Malformed);
        }
    }
    let (layout, variant) = (layout.ok_or(Malformed)?, variant.unwrap_or_default());
    let mut variants = variant.split(',');
    let layouts = layout
        .split(',')
        .filter(|_| !layout.is_empty())
        .map(|layout| Layout {
            layout: layout.trim().to_owned(),
            variant: variants.next().unwrap_or("").trim().to_owned(),
        })
        .collect();
    Ok(XkbLayouts { layouts })
}

/// A quoted string after its opening quote, whose only escapes are `\"` and `\\`, and the
/// text after its closing quote.
fn string(text: &str) -> Result<(String, &str), Malformed> {
    let mut value = String::new();
    let mut chars = text.char_indices();
    while let Some((at, c)) = chars.next() {
        match c {
            '"' => return Ok((value, &text[at + 1..])),
            '\\' => match chars.next() {
                Some((_, escaped @ ('"' | '\\'))) => value.push(escaped),
                _ => return Err(Malformed),
            },
            c => value.push(c),
        }
    }
    Err(Malformed)
}

/// The text after an unquoted value (`None`, `Some("...")`, an integer): up to the next
/// comma or closing parenthesis of the record outside a string.
fn skip(text: &str) -> &str {
    let (mut depth, mut quoted, mut escaped) = (0usize, false, false);
    for (at, c) in text.char_indices() {
        match c {
            _ if escaped => escaped = false,
            '\\' if quoted => escaped = true,
            '"' => quoted = !quoted,
            _ if quoted => {}
            '(' => depth += 1,
            ')' if depth == 0 => return &text[at..],
            ')' => depth -= 1,
            ',' if depth == 0 => return &text[at..],
            _ => {}
        }
    }
    ""
}

/// The English description `evdev.xml` gives a layout, or one of its variants.
fn description(evdev: &str, layout: &str, variant: &str) -> Option<String> {
    let list = evdev
        .split_once("<layoutList>")?
        .1
        .split_once("</layoutList>")?
        .0;
    let (item, variants) = list
        .split("<layout>")
        .skip(1)
        .map(|block| block.split_once("<variantList>").unwrap_or((block, "")))
        .find(|(item, _)| element(item, "name") == Some(layout))?;
    let item = match variant {
        "" => item,
        variant => variants
            .split("<variant>")
            .skip(1)
            .find(|item| element(item, "name") == Some(variant))?,
    };
    element(item, "description").map(unescape)
}

/// The text of the first `<tag>` element.
fn element<'a>(text: &'a str, tag: &str) -> Option<&'a str> {
    let start = text.split_once(&format!("<{tag}>"))?.1;
    Some(start.split_once(&format!("</{tag}>"))?.0.trim())
}

fn unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// The store and `evdev.xml` of a test, in a directory of its own.
#[cfg(test)]
pub(crate) fn test_store(name: &str, record: Option<&str>) -> Store {
    let dir = std::env::temp_dir().join(format!("athanor-xkb-{name}-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("test directory");
    let evdev = dir.join("evdev.xml");
    fs::write(&evdev, TEST_EVDEV).expect("evdev.xml");
    let store = Store {
        dirs: vec![dir.join("user"), dir.join("system")],
        evdev,
    };
    if let Some(record) = record {
        replace(&store.changing(), record);
    }
    store
}

/// Replaces a test's store as cosmic-config and the rig do: a swap file renamed over it.
#[cfg(test)]
fn replace(path: &std::path::Path, record: &str) {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).expect("store directory");
    }
    let swap = path.with_extension("swap");
    fs::write(&swap, record).expect("swap");
    fs::rename(&swap, path).expect("rename");
}

/// The shape of xkeyboard-config's `evdev.xml`, with its names.
#[cfg(test)]
const TEST_EVDEV: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<xkbConfigRegistry version="1.1">
  <modelList><model><configItem><name>pc105</name><description>Generic 105-key PC</description></configItem></model></modelList>
  <layoutList>
    <layout>
      <configItem>
        <name>us</name>
        <shortDescription>en</shortDescription>
        <description>English (US)</description>
        <languageList><iso639Id>eng</iso639Id></languageList>
      </configItem>
      <variantList>
        <variant>
          <configItem>
            <name>intl</name>
            <shortDescription>en</shortDescription>
            <description>English (US, intl., with dead keys)</description>
          </configItem>
        </variant>
      </variantList>
    </layout>
    <layout>
      <configItem popularity="exotic">
        <name>it</name>
        <shortDescription>it</shortDescription>
        <description>Italian</description>
      </configItem>
    </layout>
  </layoutList>
  <optionList><group allowMultipleSelection="true"><configItem><name>grp</name><description>Switching &amp; more</description></configItem></group></optionList>
</xkbConfigRegistry>
"#;

#[cfg(test)]
mod tests {
    use super::*;

    const PRETTY: &str = "(\n    rules: \"\",\n    model: \"pc105\",\n    layout: \"us,it\",\n    variant: \"intl,\",\n    options: Some(\"grp:a,b\"),\n    repeat_delay: 600,\n    repeat_rate: 25,\n)";

    fn layout(layout: &str, variant: &str) -> Layout {
        Layout {
            layout: layout.into(),
            variant: variant.into(),
        }
    }

    #[test]
    fn the_record_gives_its_layouts_and_variants() {
        assert_eq!(
            parse(PRETTY).expect("pretty").layouts,
            [layout("us", "intl"), layout("it", "")]
        );
        let line = "(rules: \"\", model: \"pc105\", layout: \"us,it\", variant: \",\", options: None, repeat_delay: 600, repeat_rate: 25)";
        assert_eq!(
            parse(line).expect("line").layouts,
            [layout("us", ""), layout("it", "")]
        );
        let short = "(rules: \"\", model: \"\", layout: \"us,it\", variant: \"\")";
        assert_eq!(
            parse(short).expect("short").layouts,
            [layout("us", ""), layout("it", "")]
        );
        assert_eq!(
            parse("(layout: \"\", variant: \"\")")
                .expect("empty")
                .layouts,
            Vec::<Layout>::new()
        );
    }

    #[test]
    fn a_malformed_record_is_refused() {
        for text in [
            "",
            "layout: \"us\"",
            "(variant: \"\")",
            "(layout: \"us\" variant: \"\")",
            "(layout: \"us)",
            "(layout: \"us\") trailing",
        ] {
            assert!(parse(text).is_err(), "{text:?}");
        }
    }

    #[test]
    fn evdev_names_layouts_and_variants_as_their_symbols_do() {
        assert_eq!(
            description(TEST_EVDEV, "us", "").as_deref(),
            Some("English (US)")
        );
        assert_eq!(
            description(TEST_EVDEV, "us", "intl").as_deref(),
            Some("English (US, intl., with dead keys)")
        );
        assert_eq!(
            description(TEST_EVDEV, "it", "").as_deref(),
            Some("Italian")
        );
        assert_eq!(description(TEST_EVDEV, "it", "intl"), None);
        assert_eq!(description(TEST_EVDEV, "grp", ""), None);
        assert_eq!(layout("xx", "yy").name(TEST_EVDEV), "xx(yy)");
        assert_eq!(
            unescape("The &quot;&lt; &gt;&quot; key &amp;"),
            "The \"< >\" key &"
        );
    }

    /// The names the watch of `store` reports after `change` and up to `wait`.
    fn followed(store: Store, change: impl FnOnce(&std::path::Path), wait: u64) -> Vec<Vec<String>> {
        let path = store.changing();
        let context = glib::MainContext::new();
        let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let _monitor = context
            .with_thread_default(|| {
                let seen = seen.clone();
                store.follow(move |names| seen.borrow_mut().push(names))
            })
            .expect("thread default")
            .expect("monitor");
        change(&path);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(wait);
        while seen.borrow().is_empty() && std::time::Instant::now() < deadline {
            context.iteration(false);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        seen.take()
    }

    /// A store replaced as cosmic-config replaces it reaches the watch, as does the first one,
    /// written where no store was (athanor-shelld's seed, LN9), its directory included.
    #[test]
    fn a_store_change_is_followed_live() {
        let two = vec!["English (US)".to_owned(), "Italian".to_owned()];
        let record = "(layout: \"us,it\", variant: \",\")";
        let replaced = test_store("live", Some("(layout: \"us\", variant: \"\")"));
        assert_eq!(followed(replaced, |path| replace(path, record), 5).last(), Some(&two));
        let first = test_store("first", None);
        fs::create_dir_all(first.changing().parent().expect("parent")).expect("directory");
        assert_eq!(followed(first, |path| replace(path, record), 5).last(), Some(&two));
        // GLib looks for a missing directory every few seconds.
        let seeded = test_store("seeded", None);
        assert_eq!(followed(seeded, |path| replace(path, record), 15).last(), Some(&two));
    }

    #[test]
    fn without_a_store_the_keymap_names_the_layouts() {
        let store = test_store("none", None);
        assert_eq!(store.names(), None);
        let mut state = State::default();
        if let Some(names) = store.names() {
            state.store_layouts(names);
        }
        state.keymap_layouts(vec!["English (US)".to_owned(), "Italian".to_owned()]);
        assert_eq!(state.keyboard_layouts, ["English (US)", "Italian"]);
        let empty = test_store("empty", Some("(layout: \"\", variant: \"\")"));
        assert_eq!(empty.names(), Some(Vec::new()));
    }

    #[test]
    fn the_store_names_its_layouts_and_a_malformed_one_keeps_them() {
        let store = test_store("named", Some("(layout: \"us,it\", variant: \"intl,\")"));
        assert_eq!(
            store.names(),
            Some(vec![
                "English (US, intl., with dead keys)".to_owned(),
                "Italian".to_owned()
            ])
        );
        replace(&store.changing(), "(layout: \"us\"");
        assert_eq!(store.names(), None, "a malformed store keeps the layouts");
    }
}
