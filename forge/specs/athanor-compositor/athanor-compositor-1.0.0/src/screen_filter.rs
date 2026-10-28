//! Whether cosmic-comp's colour inversion or colour filter is on (doc_own_stack.md decision 28):
//! `athanor-compositor --screen-filter-on`, asked by athanor-compositor-session before it
//! starts the compositor, since the own compositor draws neither and a shell script does not
//! parse RON. This file is an exception to the COSMIC boundary (`verify.py boundary`,
//! doc_shell.md SH2) until cosmic-comp's session leaves the greeter: it reads the record
//! cosmic-comp persists and writes nothing.

use crate::xkb_config::{once, read, Reader, Refused};
use std::path::PathBuf;
use std::process::ExitCode;

/// The file, under the state root (cosmic-comp's `place_state_file`).
const RELATIVE: &str = "cosmic-comp/a11y_screen_filter.ron";

/// The file: under `$XDG_STATE_HOME` when it is absolute, else `.local/state` in the home
/// directory, which is `$HOME` when set and not empty, else the password database's, as the
/// xdg crate resolves it for cosmic-comp.
fn path() -> Option<PathBuf> {
    let root = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|root| root.is_absolute())
        .or_else(|| {
            // xdg's own call, which skips an empty $HOME; deprecated on older toolchains.
            #[allow(deprecated)]
            let home = std::env::home_dir()?;
            Some(home.join(".local/state"))
        })?;
    Some(root.join(RELATIVE))
}

/// Reads the record cosmic-comp writes (`ron::ser::to_string_pretty` of its `ScreenFilter`):
/// `inverted: bool` and an optional `color_filter: Some(<name>)` or `None`. True when either
/// is on; any filter name counts, so one a later cosmic-comp adds is on too.
fn on(text: &str) -> Result<bool, Refused> {
    let mut reader = Reader::new(text);
    reader.eat(b'(')?;
    let (mut inverted, mut filter) = (None, None);
    loop {
        if reader.peek() == Some(b')') {
            reader.eat(b')')?;
            break;
        }
        let name = reader.ident()?;
        reader.eat(b':')?;
        let start = reader.at;
        match name {
            "inverted" => match reader.ident()? {
                "true" => once(&mut inverted, true, name)?,
                "false" => once(&mut inverted, false, name)?,
                _ => return Err(Refused::Syntax(start)),
            },
            "color_filter" => match reader.ident()? {
                "None" => once(&mut filter, false, name)?,
                "Some" => {
                    reader.eat(b'(')?;
                    reader.ident()?;
                    reader.eat(b')')?;
                    once(&mut filter, true, name)?
                }
                _ => return Err(Refused::Syntax(start)),
            },
            _ => return Err(Refused::Unknown(name.into())),
        }
        if reader.peek() == Some(b',') {
            reader.eat(b',')?;
        } else if reader.peek() != Some(b')') {
            return Err(Refused::Syntax(reader.at));
        }
    }
    if reader.peek().is_some() {
        return Err(Refused::Syntax(reader.at));
    }
    Ok(inverted.ok_or(Refused::Missing("inverted"))? || filter.unwrap_or(false))
}

/// The answer as an exit status: 0 when no file exists or it holds neither, 10 when it holds
/// either, 1 when it cannot be told, which the script counts as on.
pub fn answer() -> ExitCode {
    let Some(path) = path() else {
        eprintln!("athanor-compositor: no state root and no home directory");
        return ExitCode::FAILURE;
    };
    match read(&path).and_then(|text| text.as_deref().map_or(Ok(false), on)) {
        Ok(false) => ExitCode::SUCCESS,
        Ok(true) => ExitCode::from(10),
        Err(refused) => {
            eprintln!("athanor-compositor: {}: {refused}", path.display());
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::on;

    #[test]
    fn the_written_forms_are_read() {
        assert!(!on("(\n    inverted: false,\n)").expect("off"));
        assert!(on("(\n    inverted: true,\n)").expect("inverted"));
        assert!(
            on("(\n    inverted: false,\n    color_filter: Some(Greyscale),\n)").expect("filter")
        );
        assert!(!on("(inverted: false, color_filter: None)").expect("none"));
        assert!(on("(color_filter: Some(Tritanopia), inverted: false)").expect("any order"));
    }

    #[test]
    fn what_cannot_be_told_is_refused() {
        for text in [
            "",
            "(",
            "()",
            "(color_filter: None)",
            "(inverted: maybe)",
            "(inverted: false, inverted: true)",
            "(inverted: false, colour: true)",
            "(inverted: false, color_filter: Some(Greyscale)",
            "(inverted: false) trailing",
            "ScreenFilter(inverted: false)",
        ] {
            assert!(on(text).is_err(), "{text:?}");
        }
    }
}
