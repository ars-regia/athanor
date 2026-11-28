//! The one module that reads and writes GSettings (doc_settings.md SE7): through a handle of
//! athanor-settings-core's table alone, so a page writes the keys the table gives it and no
//! others, and each value is checked against the table before it is written. A schema or a key
//! that is not installed is refused here, before GLib, which aborts the process on either.

use std::fmt;

use athanor_settings_core::writes::{Handle, Invalid, Value};
use gio::prelude::*;
use glib::variant::{FromVariant, StaticVariantType, ToVariant};

/// Why a key was not read or written.
#[derive(Debug, PartialEq)]
pub enum Error {
    /// The schema is not installed, or has no such key of the handle's type.
    Absent {
        schema: &'static str,
        key: &'static str,
    },
    /// The value is outside the table's range.
    Invalid(Invalid),
    /// The backend refused the write: the key is locked.
    Refused {
        schema: &'static str,
        key: &'static str,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Absent { schema, key } => write!(f, "{schema} {key} is not installed"),
            Error::Invalid(invalid) => invalid.fmt(f),
            Error::Refused { schema, key } => write!(f, "{schema} {key} is not writable"),
        }
    }
}

impl std::error::Error for Error {}

/// The schemas and the backend keys are read from and written to.
pub struct Store {
    source: Option<gio::SettingsSchemaSource>,
    /// `None` for the default backend, dconf's.
    backend: Option<gio::SettingsBackend>,
}

impl Default for Store {
    /// The installed schemas, on the default backend.
    fn default() -> Self {
        Store {
            source: gio::SettingsSchemaSource::default(),
            backend: None,
        }
    }
}

impl Store {
    /// The key of `handle`, when its schema is installed and holds it with the handle's type.
    fn open<T: StaticVariantType>(&self, handle: &Handle<T>) -> Result<gio::Settings, Error> {
        let absent = Error::Absent {
            schema: handle.schema(),
            key: handle.key(),
        };
        let Some(schema) = self
            .source
            .as_ref()
            .and_then(|source| source.lookup(handle.schema(), true))
        else {
            return Err(absent);
        };
        if schema.path().is_none()
            || !schema.has_key(handle.key())
            || schema.key(handle.key()).value_type() != *T::static_variant_type()
        {
            return Err(absent);
        }
        Ok(gio::Settings::new_full(
            &schema,
            self.backend.as_ref(),
            None::<&str>,
        ))
    }

    /// The value of the key of `handle`.
    pub fn read<T: FromVariant + StaticVariantType>(&self, handle: &Handle<T>) -> Result<T, Error> {
        let settings = self.open(handle)?;
        settings
            .value(handle.key())
            .get::<T>()
            .ok_or(Error::Absent {
                schema: handle.schema(),
                key: handle.key(),
            })
    }

    /// Writes `value` to the key of `handle`, once the table accepts it.
    pub fn write<T: Value + ToVariant + StaticVariantType>(
        &self,
        handle: &Handle<T>,
        value: T,
    ) -> Result<(), Error> {
        let value = handle.check(value).map_err(Error::Invalid)?;
        let settings = self.open(handle)?;
        settings
            .set_value(handle.key(), &value.to_variant())
            .map_err(|_| Error::Refused {
                schema: handle.schema(),
                key: handle.key(),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use athanor_settings_core::writes::handle;
    use std::path::{Path, PathBuf};

    const IDLE: &str = "org.athanor.desktop.idle";

    /// A store on a private keyfile (SE25), with Athanor's idle schema alone compiled into a
    /// directory of this test: it reads and writes nothing of the session's dconf.
    fn store(name: &str) -> (Store, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "athanor-settings-gsettings-{name}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let schema = Path::new(env!("CARGO_MANIFEST_DIR")).join(
            "../../athanor-system-config/SOURCES/usr/share/glib-2.0/schemas/org.athanor.desktop.idle.gschema.xml",
        );
        std::fs::copy(&schema, dir.join("org.athanor.desktop.idle.gschema.xml"))
            .expect("the idle schema");
        let status = std::process::Command::new("glib-compile-schemas")
            .arg("--strict")
            .arg(&dir)
            .status()
            .expect("glib-compile-schemas");
        assert!(status.success(), "the schema compiles: {status}");
        let keyfile = dir.join("keyfile");
        let _absent = std::fs::remove_file(&keyfile);
        let store = Store {
            source: Some(
                gio::SettingsSchemaSource::from_directory(&dir, None, false).expect("source"),
            ),
            backend: Some(gio::keyfile_settings_backend_new(
                keyfile.to_str().expect("a UTF-8 path"),
                "/",
                None,
            )),
        };
        (store, keyfile)
    }

    #[test]
    fn a_value_the_table_accepts_is_written_and_read_back() {
        let (store, keyfile) = store("write");
        let blank = handle::<u32>("power", IDLE, "blank-after").expect("in the table");
        assert_eq!(store.read(&blank), Ok(300), "the schema's default");
        store.write(&blank, 600).expect("written");
        assert_eq!(store.read(&blank), Ok(600));
        let written = std::fs::read_to_string(&keyfile).expect("the keyfile");
        assert!(
            written.contains("blank-after=600") || written.contains("blank-after=uint32 600"),
            "{written}"
        );
    }

    #[test]
    fn a_value_outside_the_table_is_refused_before_it_is_written() {
        let (store, _) = store("refused");
        let blank = handle::<u32>("power", IDLE, "blank-after").expect("in the table");
        assert!(matches!(store.write(&blank, 7201), Err(Error::Invalid(_))));
        assert_eq!(store.read(&blank), Ok(300), "nothing was written");
    }

    #[test]
    fn a_schema_that_is_not_installed_is_refused_not_aborted_on() {
        let (store, _) = store("absent");
        let picture = handle::<String>("wallpaper", "org.athanor.desktop.background", "picture")
            .expect("in the table");
        let absent = Error::Absent {
            schema: "org.athanor.desktop.background",
            key: "picture",
        };
        assert_eq!(store.read(&picture), Err(absent));
        assert!(matches!(
            store.write(&picture, "/a.png".into()),
            Err(Error::Absent { .. })
        ));
    }
}
