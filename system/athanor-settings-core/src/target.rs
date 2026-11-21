//! The deep-link grammar of `Show(target)` and `--show=<target>` (doc_settings.md SE4): `""`
//! for the landing page, a page id, or a page id and a section id joined by `/`. An unknown
//! target opens the landing page; the caller writes one journal line with [`Unknown`].

use std::fmt;

use crate::registry::Registry;

/// Where the window opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Landing,
    Page(&'static str),
    Section(&'static str, &'static str),
}

impl Target {
    /// The page the target is on, if any.
    pub fn page(self) -> Option<&'static str> {
        match self {
            Target::Landing => None,
            Target::Page(page) | Target::Section(page, _) => Some(page),
        }
    }
}

impl fmt::Display for Target {
    /// The target as `Show` takes it back.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Target::Landing => Ok(()),
            Target::Page(page) => f.write_str(page),
            Target::Section(page, section) => write!(f, "{page}/{section}"),
        }
    }
}

/// A target the registry does not know: the text as it came, for the journal line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unknown(pub String);

impl fmt::Display for Unknown {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown target {:?}; the landing page opens", self.0)
    }
}

/// The target `text` names in `registry`.
pub fn parse(registry: &Registry, text: &str) -> Result<Target, Unknown> {
    if text.is_empty() {
        return Ok(Target::Landing);
    }
    let unknown = || Unknown(text.to_owned());
    let (page_id, section_id) = match text.split_once('/') {
        Some((page, section)) => (page, Some(section)),
        None => (text, None),
    };
    let page = registry.page(page_id).ok_or_else(unknown)?;
    match section_id {
        None => Ok(Target::Page(page.id)),
        Some(section_id) => {
            let section = page.section(section_id).ok_or_else(unknown)?;
            Ok(Target::Section(page.id, section.id))
        }
    }
}

/// The target of the command line, `--show=<target>`, `""` without one. Any other argument is
/// refused with its text.
pub fn from_args<I: IntoIterator<Item = String>>(args: I) -> Result<String, String> {
    let mut target = String::new();
    for arg in args {
        match arg.strip_prefix("--show=") {
            Some(text) => text.clone_into(&mut target),
            None => return Err(arg),
        }
    }
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grammar_names_the_landing_page_a_page_or_a_section() {
        let registry = Registry::new();
        assert_eq!(parse(&registry, ""), Ok(Target::Landing));
        assert_eq!(parse(&registry, "sound"), Ok(Target::Page("sound")));
        assert_eq!(
            parse(&registry, "notifications/schedule"),
            Ok(Target::Section("notifications", "schedule"))
        );
        for target in ["", "about", "privacy/developer-mode"] {
            let parsed = parse(&registry, target).expect("a known target");
            assert_eq!(
                parsed.to_string(),
                target,
                "a target prints as it is parsed"
            );
        }
    }

    #[test]
    fn an_unknown_target_is_refused_with_its_text() {
        let registry = Registry::new();
        for text in [
            "printers",
            "sound/",
            "sound/balance",
            "/",
            "a/b/c",
            "notifications/schedule/x",
            "Sound",
        ] {
            assert_eq!(
                parse(&registry, text),
                Err(Unknown(text.to_owned())),
                "{text}"
            );
        }
        assert!(Unknown("printers".into())
            .to_string()
            .contains("\"printers\""));
    }

    #[test]
    fn the_command_line_takes_one_show_argument() {
        let args = |list: &[&str]| from_args(list.iter().map(|s| (*s).to_owned()));
        assert_eq!(args(&[]), Ok(String::new()));
        assert_eq!(
            args(&["--show=displays/mode"]),
            Ok("displays/mode".to_owned())
        );
        assert_eq!(args(&["--show="]), Ok(String::new()));
        assert_eq!(args(&["--verbose"]), Err("--verbose".to_owned()));
    }
}
