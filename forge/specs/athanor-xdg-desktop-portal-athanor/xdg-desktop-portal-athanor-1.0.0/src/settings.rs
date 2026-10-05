//! `org.freedesktop.impl.portal.Settings` (doc_portal.md, PT5).
//!
//! The GLib thread reads every served GSettings schema whole into the [`Store`], derives
//! `org.freedesktop.appearance` from it, and follows each schema's `changed` signal; the
//! D-Bus thread answers `ReadAll` and `Read` from the store and emits `SettingChanged` for
//! every [`Change`] it is sent. A value is sent only when it differs from the stored one.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;
use std::sync::{Arc, PoisonError, RwLock};

use athanor_portal::settings::{self as values, Fallback, APPEARANCE};
use gio::prelude::*;
use glib::{Variant, VariantClass};
use tokio::sync::mpsc::UnboundedSender;
use zbus::message::Header;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{Array, Dict, ObjectPath, OwnedValue, Signature, StructureBuilder, Value};
use zbus::Connection;

use crate::{caller, Error};

/// The GNOME schemas served whole, key by key, as xdg-desktop-portal-gtk 1.15.3 does: every
/// key GTK 4.20 and libadwaita 1.8 read through the portal (PT5). A schema that is not
/// installed is skipped.
const GNOME_SCHEMAS: [&str; 10] = [
    "org.gnome.desktop.interface",
    "org.gnome.desktop.a11y",
    "org.gnome.desktop.a11y.interface",
    "org.gnome.desktop.calendar",
    "org.gnome.desktop.input-sources",
    "org.gnome.desktop.peripherals.mouse",
    "org.gnome.desktop.privacy",
    "org.gnome.desktop.sound",
    "org.gnome.desktop.wm.preferences",
    "org.gnome.settings-daemon.plugins.xsettings",
];
const INTERFACE: &str = "org.gnome.desktop.interface";
const A11Y_INTERFACE: &str = "org.gnome.desktop.a11y.interface";
/// Athanor's own keys: only the accent mode and the computed accent (decision A2-20).
/// Read, not served.
const ATHANOR: &str = "org.athanor.desktop.appearance";

/// Namespace, then key, then value: what `ReadAll` and `Read` answer.
pub type Store = BTreeMap<String, BTreeMap<String, Variant>>;

/// A served value that changed, for `SettingChanged`.
#[derive(Debug)]
pub struct Change {
    pub namespace: String,
    pub key: String,
    pub value: Variant,
}

/// Opens the served schemas and Athanor's on the calling thread's main context, fills the
/// store and follows every change. The returned objects must live as long as the watch.
/// `backend` is where the values live; `None` is the default one, dconf in a session.
pub fn watch(
    changes: UnboundedSender<Change>,
    backend: Option<&gio::SettingsBackend>,
) -> (Arc<RwLock<Store>>, Vec<gio::Settings>) {
    let watcher = Rc::new(Watcher {
        store: Arc::new(RwLock::new(Store::new())),
        athanor: RefCell::new(BTreeMap::new()),
        changes,
    });
    let mut opened = Vec::new();
    for id in GNOME_SCHEMAS.into_iter().chain([ATHANOR]) {
        let Some(settings) = open(id, backend) else {
            tracing::info!(schema = id, "not installed; not served");
            continue;
        };
        // GSettings emits `changed` only for keys read after a handler is connected: connect
        // first, read second.
        let watching = Rc::clone(&watcher);
        settings.connect_changed(None, move |settings, key| {
            watching.changed(id, key, settings.value(key));
        });
        let keys = read_all(&settings);
        if id == ATHANOR {
            *watcher.athanor.borrow_mut() = keys;
        } else {
            write(&watcher.store).insert(id.to_owned(), keys);
        }
        opened.push(settings);
    }
    watcher.refresh_appearance();
    (Arc::clone(&watcher.store), opened)
}

fn open(id: &str, backend: Option<&gio::SettingsBackend>) -> Option<gio::Settings> {
    let schema = gio::SettingsSchemaSource::default()?.lookup(id, true)?;
    Some(gio::Settings::new_full(&schema, backend, None))
}

fn read_all(settings: &gio::Settings) -> BTreeMap<String, Variant> {
    let Some(schema) = settings.settings_schema() else {
        return BTreeMap::new();
    };
    schema
        .list_keys()
        .into_iter()
        .map(|key| (key.to_string(), settings.value(&key)))
        .collect()
}

fn write(store: &RwLock<Store>) -> std::sync::RwLockWriteGuard<'_, Store> {
    store.write().unwrap_or_else(PoisonError::into_inner)
}

struct Watcher {
    store: Arc<RwLock<Store>>,
    /// The keys of `org.athanor.desktop.appearance`, empty while the schema is not installed.
    athanor: RefCell<BTreeMap<String, Variant>>,
    changes: UnboundedSender<Change>,
}

impl Watcher {
    fn changed(&self, namespace: &str, key: &str, value: Variant) {
        if namespace == ATHANOR {
            self.athanor.borrow_mut().insert(key.to_owned(), value);
        } else {
            self.set(namespace, key, value);
        }
        self.refresh_appearance();
    }

    /// Stores the value and reports it, unless it is the one already stored.
    fn set(&self, namespace: &str, key: &str, value: Variant) {
        {
            let mut store = write(&self.store);
            let keys = store.entry(namespace.to_owned()).or_default();
            if keys.get(key) == Some(&value) {
                return;
            }
            keys.insert(key.to_owned(), value.clone());
        }
        // The receiver lives as long as the process: a failed send means it is exiting.
        let _ = self.changes.send(Change {
            namespace: namespace.to_owned(),
            key: key.to_owned(),
            value,
        });
    }

    /// Derives `org.freedesktop.appearance` from the stored keys (VL5, A2-20).
    fn refresh_appearance(&self) {
        let derived = {
            let store = self.store.read().unwrap_or_else(PoisonError::into_inner);
            let gnome = |namespace: &str, key: &str| store.get(namespace)?.get(key).cloned();
            let athanor = self.athanor.borrow();
            let text = |value: Option<Variant>| value.and_then(|v| v.str().map(str::to_owned));
            let scheme = text(gnome(INTERFACE, "color-scheme")).unwrap_or_default();
            let high = gnome(A11Y_INTERFACE, "high-contrast")
                .and_then(|v| v.get::<bool>())
                .unwrap_or(false);
            let preset = text(gnome(INTERFACE, "accent-color")).unwrap_or_default();
            let mode = text(athanor.get("accent-mode").cloned());
            let computed = text(athanor.get("accent-computed").cloned()).unwrap_or_default();
            let (accent, fallback) = values::accent_color(mode.as_deref(), &preset, &computed);
            match fallback {
                Some(Fallback::UnknownPreset) => {
                    tracing::warn!(preset, "unknown accent preset; serving blue")
                }
                Some(Fallback::MalformedComputed) => {
                    tracing::warn!(
                        computed,
                        "malformed accent-computed; serving the fixed accent"
                    )
                }
                None => {}
            }
            [
                ("color-scheme", values::color_scheme(&scheme).to_variant()),
                ("accent-color", accent.to_variant()),
                ("contrast", values::contrast(high).to_variant()),
            ]
        };
        for (key, value) in derived {
            self.set(APPEARANCE, key, value);
        }
    }
}

/// A GLib value as a D-Bus one. `None` for what D-Bus cannot carry: maybe types and handles.
pub fn to_value(variant: &Variant) -> Option<OwnedValue> {
    OwnedValue::try_from(convert(variant)?).ok()
}

fn convert(variant: &Variant) -> Option<Value<'static>> {
    Some(match variant.classify() {
        VariantClass::Boolean => Value::Bool(variant.get()?),
        VariantClass::Byte => Value::U8(variant.get()?),
        VariantClass::Int16 => Value::I16(variant.get()?),
        VariantClass::Uint16 => Value::U16(variant.get()?),
        VariantClass::Int32 => Value::I32(variant.get()?),
        VariantClass::Uint32 => Value::U32(variant.get()?),
        VariantClass::Int64 => Value::I64(variant.get()?),
        VariantClass::Uint64 => Value::U64(variant.get()?),
        VariantClass::Double => Value::F64(variant.get()?),
        VariantClass::String => Value::from(variant.str()?.to_owned()),
        VariantClass::ObjectPath => {
            Value::ObjectPath(ObjectPath::try_from(variant.str()?.to_owned()).ok()?)
        }
        VariantClass::Signature => Value::Signature(Signature::try_from(variant.str()?).ok()?),
        VariantClass::Variant => Value::Value(Box::new(convert(&variant.as_variant()?)?)),
        VariantClass::Array => {
            let element = variant.type_().element();
            let children = (0..variant.n_children()).map(|i| variant.child_value(i));
            if element.is_dict_entry() {
                let mut dict = Dict::new(
                    &Signature::try_from(element.key().as_str()).ok()?,
                    &Signature::try_from(element.value().as_str()).ok()?,
                );
                for entry in children {
                    dict.append(
                        convert(&entry.child_value(0))?,
                        convert(&entry.child_value(1))?,
                    )
                    .ok()?;
                }
                Value::Dict(dict)
            } else {
                let mut array = Array::new(&Signature::try_from(element.as_str()).ok()?);
                for child in children {
                    array.append(convert(&child)?).ok()?;
                }
                Value::Array(array)
            }
        }
        VariantClass::Tuple => {
            let mut fields = StructureBuilder::new();
            for i in 0..variant.n_children() {
                fields = fields.append_field(convert(&variant.child_value(i))?);
            }
            Value::Structure(fields.build().ok()?)
        }
        _ => return None,
    })
}

/// `SettingChanged` for one change. A value D-Bus cannot carry is logged and skipped.
pub async fn emit(emitter: &SignalEmitter<'_>, change: &Change) {
    let Some(value) = to_value(&change.value) else {
        tracing::warn!(
            namespace = change.namespace,
            key = change.key,
            "a value D-Bus cannot carry; not announced"
        );
        return;
    };
    if let Err(err) = Portal::setting_changed(emitter, &change.namespace, &change.key, &value).await
    {
        tracing::warn!(error = %err, namespace = change.namespace, key = change.key, "cannot announce a changed setting");
    }
}

pub struct Portal {
    store: Arc<RwLock<Store>>,
}

impl Portal {
    pub fn new(store: Arc<RwLock<Store>>) -> Portal {
        Portal { store }
    }
}

#[zbus::interface(name = "org.freedesktop.impl.portal.Settings")]
impl Portal {
    async fn read_all(
        &self,
        namespaces: Vec<String>,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
    ) -> Result<HashMap<String, HashMap<String, OwnedValue>>, Error> {
        caller::authorise(&header, conn).await?;
        let store = self.store.read().unwrap_or_else(PoisonError::into_inner);
        Ok(store
            .iter()
            .filter(|(namespace, _)| values::namespace_matches(namespace, &namespaces))
            .map(|(namespace, keys)| {
                let keys = keys
                    .iter()
                    .filter_map(|(key, value)| Some((key.clone(), to_value(value)?)))
                    .collect();
                (namespace.clone(), keys)
            })
            .collect())
    }

    async fn read(
        &self,
        namespace: String,
        key: String,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
    ) -> Result<OwnedValue, Error> {
        caller::authorise(&header, conn).await?;
        let store = self.store.read().unwrap_or_else(PoisonError::into_inner);
        store
            .get(&namespace)
            .and_then(|keys| keys.get(&key))
            .and_then(to_value)
            .ok_or_else(|| Error::NotFound(format!("{namespace} {key} is not served")))
    }

    #[zbus(signal)]
    async fn setting_changed(
        emitter: &SignalEmitter<'_>,
        namespace: &str,
        key: &str,
        value: &Value<'_>,
    ) -> zbus::Result<()>;

    // The specification names it in lower case; zbus would export "Version".
    #[zbus(property, name = "version")]
    fn version(&self) -> u32 {
        1
    }
}

#[cfg(test)]
mod tests {
    use tokio::sync::mpsc::error::TryRecvError;

    use super::*;

    #[test]
    fn glib_values_cross_to_dbus_with_their_signature() {
        let cases: [(Variant, &str); 8] = [
            (true.to_variant(), "b"),
            (7u32.to_variant(), "u"),
            (1.25f64.to_variant(), "d"),
            ("Adwaita Sans 11".to_variant(), "s"),
            (vec!["a", "b"].to_variant(), "as"),
            (
                Variant::array_from_iter_with_type(
                    glib::VariantTy::new("(ss)").unwrap(),
                    [("xkb", "us").to_variant()],
                ),
                "a(ss)",
            ),
            ((0.5f64, 0.25f64, 1.0f64).to_variant(), "(ddd)"),
            (
                HashMap::from([("k".to_owned(), 1i32.to_variant())]).to_variant(),
                "a{sv}",
            ),
        ];
        for (variant, signature) in cases {
            let value = to_value(&variant).unwrap_or_else(|| panic!("{signature} converts"));
            assert_eq!(value.value_signature().to_string(), signature);
        }
        let accent = to_value(&(0.5f64, 0.25f64, 1.0f64).to_variant()).expect("(ddd)");
        assert_eq!(
            <(f64, f64, f64)>::try_from(accent).expect("a tuple"),
            (0.5, 0.25, 1.0)
        );
    }

    #[test]
    fn a_maybe_value_does_not_cross() {
        assert!(to_value(&Some(1u32).to_variant()).is_none());
    }

    /// The interface is written by hand (PT2 asks for zbus-xmlgen output); this holds it to
    /// xdg-desktop-portal's own definition, vendored unmodified from tag 1.20.4: every method,
    /// signal and property, with the type and direction of each argument.
    #[test]
    fn the_interface_matches_the_upstream_definition() {
        let upstream = members(include_str!(
            "../interfaces/org.freedesktop.impl.portal.Settings.xml"
        ));
        let mut ours = String::new();
        zbus::object_server::Interface::introspect_to_writer(
            &Portal::new(Arc::default()),
            &mut ours,
            0,
        );
        assert_eq!(
            upstream.len(),
            4,
            "ReadAll, Read, SettingChanged, version: {upstream:?}"
        );
        assert_eq!(members(&ours), upstream, "{ours}");
    }

    type Member = (String, String, Vec<(String, String)>);

    /// `(kind, name, [(type, direction or access)])` of each member in introspection XML.
    /// Signal arguments are compared without a direction: upstream writes "out", zbus nothing.
    fn members(xml: &str) -> std::collections::BTreeSet<Member> {
        let mut text = xml.to_owned();
        while let Some(start) = text.find("<!--") {
            let end = text[start..]
                .find("-->")
                .map_or(text.len(), |at| start + at + 3);
            text.replace_range(start..end, "");
        }
        let attr = |tag: &str, key: &str| {
            let needle = format!(" {key}=\"");
            tag.find(&needle)
                .map(|at| &tag[at + needle.len()..])
                .and_then(|rest| rest.split('"').next())
                .unwrap_or_default()
                .to_owned()
        };
        let mut found = std::collections::BTreeSet::new();
        let mut open: Option<Member> = None;
        for tag in text.split('<').skip(1).filter_map(|t| t.split('>').next()) {
            match tag.split_whitespace().next().unwrap_or_default() {
                kind @ ("method" | "signal") => {
                    open = Some((kind.to_owned(), attr(tag, "name"), Vec::new()));
                    if tag.ends_with('/') {
                        found.extend(open.take());
                    }
                }
                "/method" | "/signal" => found.extend(open.take()),
                "arg" => {
                    if let Some((kind, _, args)) = open.as_mut() {
                        let direction = match kind.as_str() {
                            "signal" => String::new(),
                            _ => attr(tag, "direction"),
                        };
                        args.push((attr(tag, "type"), direction));
                    }
                }
                "property" => {
                    found.insert((
                        "property".to_owned(),
                        attr(tag, "name"),
                        vec![(attr(tag, "type"), attr(tag, "access"))],
                    ));
                }
                _ => {}
            }
        }
        found
    }

    /// The whole GLib side, on GSettings' memory backend and the GNOME schemas the build
    /// machine has installed (gsettings-desktop-schemas).
    #[test]
    fn a_gsettings_write_reaches_the_store_and_the_derived_keys_once() {
        // A backend of the test's own, passed in: setting GSETTINGS_BACKEND would write
        // the environment while other tests' threads read it.
        let backend = gio::memory_settings_backend_new();
        let schema = |id: &str| open(id, Some(&backend)).expect(id);
        let context = glib::MainContext::new();
        context
            .with_thread_default(|| {
                let (sender, mut received) = tokio::sync::mpsc::unbounded_channel();
                let (store, _watched) = watch(sender, Some(&backend));
                let appearance = |key: &str| store.read().unwrap()[APPEARANCE][key].clone();
                assert_eq!(appearance("color-scheme"), 0u32.to_variant());
                assert_eq!(appearance("contrast"), 0u32.to_variant());
                // The start fills the store; it announces nothing.
                while received.try_recv().is_ok() {}

                let writer = schema(INTERFACE);
                writer.set_string("color-scheme", "prefer-dark").unwrap();
                writer.set_string("accent-color", "teal").unwrap();
                schema(A11Y_INTERFACE)
                    .set_boolean("high-contrast", true)
                    .unwrap();
                while context.iteration(false) {}

                let mut announced = Vec::new();
                loop {
                    match received.try_recv() {
                        Ok(change) => announced.push((change.namespace, change.key, change.value)),
                        Err(TryRecvError::Empty) => break,
                        Err(err) => panic!("{err}"),
                    }
                }
                let teal = values::accent_color(None, "teal", "").0.to_variant();
                for expected in [
                    (INTERFACE, "color-scheme", "prefer-dark".to_variant()),
                    (APPEARANCE, "color-scheme", 1u32.to_variant()),
                    (INTERFACE, "accent-color", "teal".to_variant()),
                    (APPEARANCE, "accent-color", teal.clone()),
                    (A11Y_INTERFACE, "high-contrast", true.to_variant()),
                    (APPEARANCE, "contrast", 1u32.to_variant()),
                ] {
                    let count = announced
                        .iter()
                        .filter(|(ns, key, value)| {
                            (ns.as_str(), key.as_str(), value)
                                == (expected.0, expected.1, &expected.2)
                        })
                        .count();
                    assert_eq!(count, 1, "{expected:?} announced once in {announced:?}");
                }
                assert_eq!(announced.len(), 6, "{announced:?}");
                assert_eq!(appearance("accent-color"), teal);

                // Writing the value already stored announces nothing.
                writer.set_string("color-scheme", "prefer-dark").unwrap();
                while context.iteration(false) {}
                assert!(matches!(received.try_recv(), Err(TryRecvError::Empty)));
            })
            .expect("own the test's main context");
    }
}
