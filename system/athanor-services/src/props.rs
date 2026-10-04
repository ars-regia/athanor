//! The objects and properties of a D-Bus service, mirrored from `GetManagedObjects`, `GetAll`
//! and their signals. Everything a peer sends is type-checked here before anything reads it:
//! a reply or a signal of the wrong type is `None`, and a property of the wrong type reads as
//! absent, never as a panic. The getters match the value's exact D-Bus type and convert
//! nothing: a `u` is not read as an `i`, and an `o` is not read as an `s`.

use std::collections::{BTreeMap, HashMap};

use zbus::zvariant::{OwnedObjectPath, OwnedValue, Signature, Value};
use zbus::Message;

pub type Props = BTreeMap<String, OwnedValue>;
pub type Interfaces = BTreeMap<String, Props>;
pub type Objects = BTreeMap<String, Interfaces>;

pub const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
pub const OBJECT_MANAGER: &str = "org.freedesktop.DBus.ObjectManager";

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

/// The reply of `GetManagedObjects`, `a{oa{sa{sv}}}`.
pub fn managed_objects(reply: &Message) -> Option<Objects> {
    let objects: HashMap<OwnedObjectPath, Interfaces> = reply.body().deserialize().ok()?;
    Some(
        objects
            .into_iter()
            .map(|(path, interfaces)| (path.as_str().to_owned(), interfaces))
            .collect(),
    )
}

/// The reply of `GetAll`, `a{sv}`.
pub fn get_all(reply: &Message) -> Option<Props> {
    reply.body().deserialize().ok()
}

/// A signal as a change of the mirror; `None` for any other member or a wrong type.
pub fn change(signal: &Message) -> Option<Change> {
    let header = signal.header();
    let path = header.path()?.as_str().to_owned();
    let body = signal.body();
    match (header.interface()?.as_str(), header.member()?.as_str()) {
        (PROPERTIES, "PropertiesChanged") => {
            let (interface, changed, invalidated) = body.deserialize().ok()?;
            Some(Change::Properties {
                path,
                interface,
                changed,
                invalidated,
            })
        }
        (OBJECT_MANAGER, "InterfacesAdded") => {
            let (path, interfaces): (OwnedObjectPath, Interfaces) = body.deserialize().ok()?;
            Some(Change::Added {
                path: path.as_str().to_owned(),
                interfaces,
            })
        }
        (OBJECT_MANAGER, "InterfacesRemoved") => {
            let (path, interfaces): (OwnedObjectPath, Vec<String>) = body.deserialize().ok()?;
            Some(Change::Removed {
                path: path.as_str().to_owned(),
                interfaces,
            })
        }
        _ => None,
    }
}

/// Applies `change`. A property change of an object or interface the mirror does not hold is
/// ignored: the mirror learns of objects from its load and from `InterfacesAdded` only.
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

fn value<'a>(props: &'a Props, name: &str) -> Option<&'a Value<'static>> {
    props.get(name).map(|value| &**value)
}

/// The elements of an array property whose elements are of type `element`.
fn array<'a>(props: &'a Props, name: &str, element: &Signature) -> Option<&'a [Value<'static>]> {
    match value(props, name)? {
        Value::Array(array) if array.element_signature() == element => Some(array),
        _ => None,
    }
}

pub fn get_str<'a>(props: &'a Props, name: &str) -> Option<&'a str> {
    match value(props, name)? {
        Value::Str(text) => Some(text.as_str()),
        _ => None,
    }
}

pub fn get_u32(props: &Props, name: &str) -> Option<u32> {
    match value(props, name)? {
        Value::U32(number) => Some(*number),
        _ => None,
    }
}

pub fn get_i32(props: &Props, name: &str) -> Option<i32> {
    match value(props, name)? {
        Value::I32(number) => Some(*number),
        _ => None,
    }
}

pub fn get_i64(props: &Props, name: &str) -> Option<i64> {
    match value(props, name)? {
        Value::I64(number) => Some(*number),
        _ => None,
    }
}

pub fn get_u64(props: &Props, name: &str) -> Option<u64> {
    match value(props, name)? {
        Value::U64(number) => Some(*number),
        _ => None,
    }
}

pub fn get_f64(props: &Props, name: &str) -> Option<f64> {
    match value(props, name)? {
        Value::F64(number) => Some(*number),
        _ => None,
    }
}

pub fn get_bool(props: &Props, name: &str) -> Option<bool> {
    match value(props, name)? {
        Value::Bool(flag) => Some(*flag),
        _ => None,
    }
}

/// An `ay` property.
pub fn get_bytes(props: &Props, name: &str) -> Option<Vec<u8>> {
    array(props, name, &Signature::U8)?
        .iter()
        .map(|byte| match byte {
            Value::U8(byte) => Some(*byte),
            _ => None,
        })
        .collect()
}

/// An `o` property; `None` for `/`, which D-Bus services use for "none".
pub fn get_path(props: &Props, name: &str) -> Option<String> {
    match value(props, name)? {
        Value::ObjectPath(path) if path.as_str() != "/" => Some(path.as_str().to_owned()),
        _ => None,
    }
}

/// An `ao` property.
pub fn get_paths(props: &Props, name: &str) -> Option<Vec<String>> {
    array(props, name, &Signature::ObjectPath)?
        .iter()
        .map(|path| match path {
            Value::ObjectPath(path) => Some(path.as_str().to_owned()),
            _ => None,
        })
        .collect()
}

/// An `as` property.
pub fn get_strs(props: &Props, name: &str) -> Option<Vec<String>> {
    array(props, name, &Signature::Str)?
        .iter()
        .map(|text| match text {
            Value::Str(text) => Some(text.as_str().to_owned()),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use zbus::zvariant::{ObjectPath, OwnedValue, Value};
    use zbus::Message;

    use super::*;

    fn owned(value: Value<'_>) -> OwnedValue {
        value.try_to_owned().expect("owned")
    }

    fn props(entries: &[(&str, Value<'_>)]) -> Props {
        entries
            .iter()
            .map(|(name, value)| ((*name).to_owned(), owned(value.clone())))
            .collect()
    }

    /// Every type a getter reads, once, under the name of its type.
    fn typed() -> Props {
        props(&[
            ("s", Value::from("text")),
            ("u", Value::from(7u32)),
            ("i", Value::from(-7i32)),
            ("t", Value::from(7u64)),
            ("d", Value::from(0.5f64)),
            ("b", Value::from(true)),
            ("ay", Value::from(vec![1u8, 2])),
            (
                "o",
                Value::from(ObjectPath::from_static_str_unchecked("/x")),
            ),
            (
                "root",
                Value::from(ObjectPath::from_static_str_unchecked("/")),
            ),
            (
                "ao",
                Value::from(vec![
                    ObjectPath::from_static_str_unchecked("/x"),
                    ObjectPath::from_static_str_unchecked("/y"),
                ]),
            ),
            ("as", Value::from(vec!["a", "b"])),
            ("v", Value::Value(Box::new(Value::from(7u32)))),
        ])
    }

    #[test]
    fn get_str_reads_a_string_only() {
        let p = typed();
        assert_eq!(get_str(&p, "s"), Some("text"));
        assert_eq!(get_str(&p, "o"), None);
        assert_eq!(get_str(&p, "u"), None);
        assert_eq!(get_str(&p, "absent"), None);
    }

    #[test]
    fn get_u32_reads_a_u32_only() {
        let p = typed();
        assert_eq!(get_u32(&p, "u"), Some(7));
        assert_eq!(get_u32(&p, "i"), None);
        assert_eq!(get_u32(&p, "s"), None);
    }

    #[test]
    fn get_i32_reads_an_i32_only() {
        let p = typed();
        assert_eq!(get_i32(&p, "i"), Some(-7));
        assert_eq!(get_i32(&p, "u"), None);
    }

    #[test]
    fn get_u64_reads_a_u64_only() {
        let p = typed();
        assert_eq!(get_u64(&p, "t"), Some(7));
        assert_eq!(get_u64(&p, "u"), None);
    }

    #[test]
    fn get_f64_reads_a_double_only() {
        let p = typed();
        assert_eq!(get_f64(&p, "d"), Some(0.5));
        assert_eq!(get_f64(&p, "u"), None);
    }

    #[test]
    fn get_bool_reads_a_boolean_only() {
        let p = typed();
        assert_eq!(get_bool(&p, "b"), Some(true));
        assert_eq!(get_bool(&p, "u"), None);
    }

    #[test]
    fn get_bytes_reads_a_byte_array_only() {
        let p = typed();
        assert_eq!(get_bytes(&p, "ay"), Some(vec![1, 2]));
        assert_eq!(get_bytes(&p, "as"), None);
        assert_eq!(get_bytes(&p, "s"), None);
    }

    #[test]
    fn get_path_reads_an_object_path_and_root_is_none() {
        let p = typed();
        assert_eq!(get_path(&p, "o").as_deref(), Some("/x"));
        assert_eq!(get_path(&p, "root"), None);
        assert_eq!(get_path(&p, "s"), None);
    }

    #[test]
    fn get_paths_reads_an_object_path_array_only() {
        let p = typed();
        assert_eq!(get_paths(&p, "ao"), Some(vec!["/x".into(), "/y".into()]));
        assert_eq!(get_paths(&p, "as"), None);
        assert_eq!(get_paths(&p, "o"), None);
    }

    #[test]
    fn get_strs_reads_a_string_array_only() {
        let p = typed();
        assert_eq!(get_strs(&p, "as"), Some(vec!["a".into(), "b".into()]));
        assert_eq!(get_strs(&p, "ao"), None);
        assert_eq!(get_strs(&p, "s"), None);
    }

    #[test]
    fn a_value_nested_in_a_variant_is_another_type() {
        assert_eq!(get_u32(&typed(), "v"), None);
    }

    fn objects() -> Objects {
        let x_y = BTreeMap::from([(
            "x.Y",
            BTreeMap::from([("On", Value::from(true)), ("Name", Value::from("a"))]),
        )]);
        let reply = Message::method_call("/", "GetManagedObjects")
            .expect("header")
            .build(&BTreeMap::from([(
                ObjectPath::from_static_str_unchecked("/a"),
                x_y,
            )]))
            .expect("body");
        managed_objects(&reply).expect("managed objects")
    }

    fn signal(interface: &str, member: &str) -> zbus::message::Builder<'static> {
        Message::signal("/a", interface.to_owned(), member.to_owned()).expect("header")
    }

    #[test]
    fn managed_objects_are_read_and_a_wrong_type_is_none() {
        let objects = objects();
        assert_eq!(
            get_bool(lookup(&objects, "/a", "x.Y").expect("x.Y"), "On"),
            Some(true)
        );
        let wrong = Message::method_call("/", "GetManagedObjects")
            .expect("header")
            .build(&BTreeMap::from([("On", Value::from(true))]))
            .expect("body");
        assert!(managed_objects(&wrong).is_none());
    }

    #[test]
    fn get_all_is_read_and_a_wrong_type_is_none() {
        let reply = Message::method_call("/", "GetAll")
            .expect("header")
            .build(&BTreeMap::from([("Level", Value::from(3u32))]))
            .expect("body");
        assert_eq!(get_u32(&get_all(&reply).expect("props"), "Level"), Some(3));
        let wrong = Message::method_call("/", "GetAll")
            .expect("header")
            .build(&("x",))
            .expect("body");
        assert!(get_all(&wrong).is_none());
    }

    #[test]
    fn a_property_change_updates_and_invalidates() {
        let mut objects = objects();
        let message = signal(PROPERTIES, "PropertiesChanged")
            .build(&(
                "x.Y",
                BTreeMap::from([("On", Value::from(false))]),
                vec!["Name"],
            ))
            .expect("body");
        apply(&mut objects, change(&message).expect("change"));
        let props = lookup(&objects, "/a", "x.Y").expect("x.Y");
        assert_eq!(get_bool(props, "On"), Some(false));
        assert!(!props.contains_key("Name"));
    }

    #[test]
    fn a_change_of_an_unknown_object_is_ignored() {
        let mut objects = objects();
        let message = Message::signal("/b", PROPERTIES, "PropertiesChanged")
            .expect("header")
            .build(&(
                "x.Y",
                BTreeMap::from([("On", Value::from(false))]),
                Vec::<String>::new(),
            ))
            .expect("body");
        apply(&mut objects, change(&message).expect("change"));
        assert_eq!(objects.len(), 1);
        assert!(!objects.contains_key("/b"));
    }

    #[test]
    fn objects_come_and_go_with_their_interfaces() {
        let mut objects = objects();
        let added = signal(OBJECT_MANAGER, "InterfacesAdded")
            .build(&(
                ObjectPath::from_static_str_unchecked("/b"),
                BTreeMap::from([("x.Z", BTreeMap::<String, Value>::new())]),
            ))
            .expect("body");
        apply(&mut objects, change(&added).expect("added"));
        assert!(lookup(&objects, "/b", "x.Z").is_some());
        let removed = signal(OBJECT_MANAGER, "InterfacesRemoved")
            .build(&(ObjectPath::from_static_str_unchecked("/b"), vec!["x.Z"]))
            .expect("body");
        apply(&mut objects, change(&removed).expect("removed"));
        assert!(!objects.contains_key("/b"));
    }

    #[test]
    fn a_signal_of_the_wrong_type_is_none() {
        for (interface, member) in [
            (PROPERTIES, "PropertiesChanged"),
            (OBJECT_MANAGER, "InterfacesAdded"),
            (OBJECT_MANAGER, "InterfacesRemoved"),
            ("x.Y", "Other"),
        ] {
            let wrong = signal(interface, member).build(&("x",)).expect("body");
            assert!(change(&wrong).is_none(), "{member}");
        }
    }
}
