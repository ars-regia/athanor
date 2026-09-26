//! The directories the bar uses, from the environment its unit gives it. A relative or
//! empty value counts as unset (XDG Base Directory Specification).

use std::ffi::OsString;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dirs {
    /// The crash-loop record (doc_shell.md, SH8), in the unit's runtime directory.
    pub failures: PathBuf,
    /// `$XDG_CONFIG_HOME`: the layout document and the favourites.
    pub config: PathBuf,
    /// `$XDG_CACHE_HOME`: GTK's and fontconfig's caches.
    pub cache: PathBuf,
    /// `$XDG_RUNTIME_DIR`: the socket directories of launched applications (BR2) and dconf.
    pub runtime: PathBuf,
}

impl Dirs {
    pub fn from_vars(var: impl Fn(&str) -> Option<OsString>) -> Option<Dirs> {
        let absolute = |name: &str| var(name).map(PathBuf::from).filter(|dir| dir.is_absolute());
        let home = absolute("HOME");
        let config = absolute("XDG_CONFIG_HOME")
            .or_else(|| home.as_ref().map(|home| home.join(".config")))?;
        let cache =
            absolute("XDG_CACHE_HOME").or_else(|| home.as_ref().map(|home| home.join(".cache")))?;
        let runtime = absolute("XDG_RUNTIME_DIR")?;
        let unit_runtime =
            absolute("RUNTIME_DIRECTORY").unwrap_or_else(|| runtime.join("athanor-bar"));
        Some(Dirs {
            failures: unit_runtime.join("failures"),
            config,
            cache,
            runtime,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
        move |name| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| OsString::from(value))
        }
    }

    #[test]
    fn the_unit_directories_come_first() {
        let dirs = Dirs::from_vars(vars(&[
            ("HOME", "/home/u"),
            ("XDG_CONFIG_HOME", "/c"),
            ("XDG_CACHE_HOME", "/k"),
            ("XDG_RUNTIME_DIR", "/run/user/1"),
            ("RUNTIME_DIRECTORY", "/run/user/1/athanor-bar"),
        ]));
        assert_eq!(
            dirs,
            Some(Dirs {
                failures: PathBuf::from("/run/user/1/athanor-bar/failures"),
                config: PathBuf::from("/c"),
                cache: PathBuf::from("/k"),
                runtime: PathBuf::from("/run/user/1"),
            })
        );
    }

    #[test]
    fn home_fills_in_and_relative_or_empty_values_count_as_unset() {
        let dirs = Dirs::from_vars(vars(&[
            ("HOME", "/home/u"),
            ("XDG_CONFIG_HOME", "relative"),
            ("XDG_CACHE_HOME", ""),
            ("XDG_RUNTIME_DIR", "/run/user/1"),
            ("RUNTIME_DIRECTORY", "also/relative"),
        ]))
        .expect("dirs");
        assert_eq!(dirs.config, PathBuf::from("/home/u/.config"));
        assert_eq!(dirs.cache, PathBuf::from("/home/u/.cache"));
        assert_eq!(
            dirs.failures,
            PathBuf::from("/run/user/1/athanor-bar/failures")
        );
    }

    #[test]
    fn without_a_runtime_directory_or_a_home_there_are_none() {
        assert_eq!(Dirs::from_vars(vars(&[("HOME", "/home/u")])), None);
        assert_eq!(
            Dirs::from_vars(vars(&[("XDG_RUNTIME_DIR", "/run/user/1")])),
            None
        );
        assert_eq!(
            Dirs::from_vars(vars(&[
                ("HOME", "home"),
                ("XDG_RUNTIME_DIR", "/run/user/1")
            ])),
            None
        );
    }
}
