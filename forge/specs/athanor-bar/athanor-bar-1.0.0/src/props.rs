//! The objects and properties of a D-Bus service, mirrored from `GetManagedObjects`, `GetAll`
//! and their signals. Everything a peer sends is type-checked here before anything reads it:
//! a reply or a signal of the wrong type is `None`, never a panic.

use std::collections::BTreeMap;

use glib::prelude::*;
use glib::variant::{FromVariant, ObjectPath};
use glib::{Variant, VariantTy};

pub type Props = BTreeMap<String, Variant>;
pub type Interfaces = BTreeMap<String, Props>;
pub type Objects = BTreeMap<String, Interfaces>;

/// One signal of `org.freedesktop.DBus.Properties` or `org.freedesktop.DBus.ObjectManager`.
#[derive(Debug)]
pub enum Change {
    Properties {
        path: String,
        interface: String,
        changed: Props,
        invalidated: Vec<String>,
    },
    Added {
        path: String,
        interfaces: Interfaces,
    },
    Removed {
        path: String,
        interfaces: Vec<String>,
    },
}

pub fn has_type(variant: &Variant, signature: &str) -> bool {
    VariantTy::new(signature).is_ok_and(|ty| variant.is_type(ty))
}

/// The pair `(o, a{sa{sv}})` of a managed object; `get` cannot read it, since `String` is `s`.
fn object_entry(entry: &Variant) -> Option<(String, Interfaces)> {
    let path = entry.try_child_value(0)?.str()?.to_owned();
    let interfaces = entry.try_child_value(1)?.get::<Interfaces>()?;
    Some((path, interfaces))
}

/// The reply of `GetManagedObjects`, `(a{oa{sa{sv}}})`.
pub fn managed_objects(reply: &Variant) -> Option<Objects> {
    if !has_type(reply, "(a{oa{sa{sv}}})") {
        return None;
    }
    reply
        .try_child_value(0)?
        .iter()
        .map(|entry| object_entry(&entry))
        .collect()
}

/// The reply of `GetAll`, `(a{sv})`.
pub fn get_all(reply: &Variant) -> Option<Props> {
    reply.get::<(Props,)>().map(|(props,)| props)
}

/// A signal as a change of the mirror; `None` for any other member or a wrong type.
pub fn change(path: &str, interface: &str, member: &str, params: &Variant) -> Option<Change> {
    match (interface, member) {
        ("org.freedesktop.DBus.Properties", "PropertiesChanged") => {
            let (interface, changed, invalidated) = params.get::<(String, Props, Vec<String>)>()?;
            Some(Change::Properties {
                path: path.to_owned(),
                interface,
                changed,
                invalidated,
            })
        }
        ("org.freedesktop.DBus.ObjectManager", "InterfacesAdded") => {
            if !has_type(params, "(oa{sa{sv}})") {
                return None;
            }
            let (path, interfaces) = object_entry(params)?;
            Some(Change::Added { path, interfaces })
        }
        ("org.freedesktop.DBus.ObjectManager", "InterfacesRemoved") => {
            if !has_type(params, "(oas)") {
                return None;
            }
            let path = params.try_child_value(0)?.str()?.to_owned();
            let interfaces = params.try_child_value(1)?.get::<Vec<String>>()?;
            Some(Change::Removed { path, interfaces })
        }
        _ => None,
    }
}

/// Applies `change`. A property change of an object or interface the mirror does not hold is
/// ignored: the mirror learns of objects from `GetManagedObjects` and `InterfacesAdded` only.
pub fn apply(objects: &mut Objects, change: Change) {
    match change {
        Change::Properties {
            path,
            interface,
            changed,
            invalidated,
        } => {
            if let Some(props) = objects
                .get_mut(&path)
                .and_then(|interfaces| interfaces.get_mut(&interface))
            {
                props.extend(changed);
                for name in invalidated {
                    props.remove(&name);
                }
            }
        }
        Change::Added { path, interfaces } => {
            objects.entry(path).or_default().extend(interfaces);
        }
        Change::Removed { path, interfaces } => {
            if let Some(held) = objects.get_mut(&path) {
                for name in interfaces {
                    held.remove(&name);
                }
                if held.is_empty() {
                    objects.remove(&path);
                }
            }
        }
    }
}

pub fn lookup<'a>(objects: &'a Objects, path: &str, interface: &str) -> Option<&'a Props> {
    objects.get(path)?.get(interface)
}

/// A property of type `T`; `None` when it is absent or of another type.
pub fn value<T: FromVariant>(props: &Props, name: &str) -> Option<T> {
    props.get(name)?.get::<T>()
}

/// An object-path property; `None` for `/`, which D-Bus services use for "none".
pub fn path(props: &Props, name: &str) -> Option<String> {
    let value = props.get(name)?;
    if !value.is_type(VariantTy::OBJECT_PATH) {
        return None;
    }
    value.str().filter(|path| *path != "/").map(str::to_owned)
}

/// An `ao` property; empty when it is absent or of another type.
pub fn paths(props: &Props, name: &str) -> Vec<String> {
    match props.get(name) {
        Some(value) if value.is_type(VariantTy::OBJECT_PATH_ARRAY) => value
            .iter()
            .filter_map(|path| path.str().map(str::to_owned))
            .collect(),
        _ => Vec::new(),
    }
}

/// `path` as an `o` value; `None` when it is not a valid object path.
pub fn object_path(path: &str) -> Option<Variant> {
    ObjectPath::try_from(path)
        .ok()
        .map(|path| path.to_variant())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(signature: &str, text: &str) -> Variant {
        Variant::parse(Some(VariantTy::new(signature).unwrap()), text).unwrap()
    }

    fn objects() -> Objects {
        managed_objects(&parse(
            "(a{oa{sa{sv}}})",
            "({objectpath '/a': {'x.Y': {'On': <true>, 'Name': <'a'>}}},)",
        ))
        .unwrap()
    }

    #[test]
    fn managed_objects_are_read_and_a_wrong_type_is_none() {
        let objects = objects();
        assert_eq!(
            value::<bool>(lookup(&objects, "/a", "x.Y").unwrap(), "On"),
            Some(true)
        );
        assert!(managed_objects(&parse("(a{sv})", "({'On': <true>},)")).is_none());
    }

    #[test]
    fn a_property_change_updates_and_invalidates() {
        let mut objects = objects();
        let params = parse("(sa{sv}as)", "('x.Y', {'On': <false>}, ['Name'])");
        apply(
            &mut objects,
            change(
                "/a",
                "org.freedesktop.DBus.Properties",
                "PropertiesChanged",
                &params,
            )
            .unwrap(),
        );
        let props = lookup(&objects, "/a", "x.Y").unwrap();
        assert_eq!(value::<bool>(props, "On"), Some(false));
        assert!(props.get("Name").is_none());
    }

    #[test]
    fn a_change_of_an_unknown_object_is_ignored() {
        let mut objects = objects();
        let params = parse("(sa{sv}as)", "('x.Y', {'On': <false>}, @as [])");
        apply(
            &mut objects,
            change(
                "/b",
                "org.freedesktop.DBus.Properties",
                "PropertiesChanged",
                &params,
            )
            .unwrap(),
        );
        assert_eq!(objects.len(), 1);
        assert!(!objects.contains_key("/b"));
    }

    #[test]
    fn objects_come_and_go_with_their_interfaces() {
        let mut objects = objects();
        let added = parse("(oa{sa{sv}})", "(objectpath '/b', {'x.Z': @a{sv} {}})");
        apply(
            &mut objects,
            change(
                "/",
                "org.freedesktop.DBus.ObjectManager",
                "InterfacesAdded",
                &added,
            )
            .unwrap(),
        );
        assert!(lookup(&objects, "/b", "x.Z").is_some());
        let removed = parse("(oas)", "(objectpath '/b', ['x.Z'])");
        apply(
            &mut objects,
            change(
                "/",
                "org.freedesktop.DBus.ObjectManager",
                "InterfacesRemoved",
                &removed,
            )
            .unwrap(),
        );
        assert!(!objects.contains_key("/b"));
    }

    #[test]
    fn a_signal_of_the_wrong_type_is_none() {
        let wrong = parse("(s)", "('x',)");
        for (interface, member) in [
            ("org.freedesktop.DBus.Properties", "PropertiesChanged"),
            ("org.freedesktop.DBus.ObjectManager", "InterfacesAdded"),
            ("org.freedesktop.DBus.ObjectManager", "InterfacesRemoved"),
            ("x.Y", "Other"),
        ] {
            assert!(
                change("/a", interface, member, &wrong).is_none(),
                "{member}"
            );
        }
    }

    #[test]
    fn paths_are_type_checked_and_root_is_none() {
        let props = get_all(&parse(
            "(a{sv})",
            "({'One': <objectpath '/x'>, 'Root': <objectpath '/'>, 'Text': <'/x'>, 'Many': <@ao ['/x', '/y']>},)",
        ))
        .unwrap();
        assert_eq!(path(&props, "One").as_deref(), Some("/x"));
        assert_eq!(path(&props, "Root"), None);
        assert_eq!(path(&props, "Text"), None);
        assert_eq!(paths(&props, "Many"), ["/x", "/y"]);
        assert!(paths(&props, "Text").is_empty());
        assert!(object_path("not a path").is_none());
    }
}
