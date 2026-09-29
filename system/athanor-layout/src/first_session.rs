//! The default layout, picked once per user from the outputs of their first session
//! (doc_shell.md, SH10). The pick writes only the preset, nothing at all when the policy
//! layer names a preset, and a marker records that it ran.

use std::fs;
use std::io;
use std::path::Path;

use crate::document::Document;
use crate::loader::{Resolved, UserState};
use crate::placement::{Output, Shape};
use crate::preset::Preset;
use crate::user::save;

/// Below this logical height on the smallest output, the pick is the bar.
pub const MIN_HEIGHT_FOR_FLOAT: i32 = 800;

/// The preset for these outputs; `None` while no output has a size yet.
pub fn pick(outputs: &[Output]) -> Option<Preset> {
    let sized: Vec<&Output> = outputs.iter().filter(|output| output.is_sized()).collect();
    if sized.iter().any(|output| output.shape() == Shape::Portrait) {
        return Some(Preset::Float);
    }
    let smallest = sized.iter().map(|output| output.height).min()?;
    Some(if smallest < MIN_HEIGHT_FOR_FLOAT {
        Preset::Bar
    } else {
        Preset::Float
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// No output has a size yet; the next pass decides.
    NotYet,
    /// The marker exists.
    AlreadyRan,
    Wrote(Preset),
    /// The policy names a preset, or the user already has a document.
    LeftToPolicyOrUser,
}

pub fn run(
    resolved: &Resolved,
    user_file: &Path,
    marker: &Path,
    outputs: &[Output],
) -> io::Result<Outcome> {
    if marker.exists() {
        return Ok(Outcome::AlreadyRan);
    }
    let Some(preset) = pick(outputs) else {
        return Ok(Outcome::NotYet);
    };
    let outcome = if resolved.policy_names_preset || resolved.user != UserState::Absent {
        Outcome::LeftToPolicyOrUser
    } else {
        save(
            user_file,
            &Document {
                preset: Some(preset),
                ..Document::default()
            },
            None,
        )?;
        Outcome::Wrote(preset)
    };
    if let Some(dir) = marker.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(marker, format!("{}\n", preset.id()))?;
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loader::{resolve, Paths};
    use crate::testing::scratch;

    fn screen(width: i32, height: i32) -> Output {
        Output {
            connector: Some("eDP-1".into()),
            width,
            height,
        }
    }

    #[test]
    fn the_pick_follows_sh10() {
        assert_eq!(pick(&[screen(1920, 1080)]), Some(Preset::Float));
        assert_eq!(pick(&[screen(1280, 800)]), Some(Preset::Float));
        assert_eq!(pick(&[screen(1366, 768)]), Some(Preset::Bar));
        assert_eq!(
            pick(&[screen(2560, 1440), screen(1366, 768)]),
            Some(Preset::Bar)
        );
        assert_eq!(pick(&[screen(1080, 1920)]), Some(Preset::Float));
        assert_eq!(
            pick(&[screen(1366, 768), screen(768, 1366)]),
            Some(Preset::Float)
        );
        assert_eq!(
            pick(&[Output {
                connector: None,
                width: 0,
                height: 0
            }]),
            None
        );
        assert_eq!(pick(&[]), None);
    }

    struct Scene {
        paths: Paths,
        marker: std::path::PathBuf,
    }

    fn scene(name: &str) -> Scene {
        let base = scratch(name);
        Scene {
            paths: Paths {
                vendor_dir: base.join("vendor"),
                policy_dir: base.join("policy"),
                user_file: base.join("config/athanor/layout.toml"),
            },
            marker: base.join("state/athanor/layout-first-session"),
        }
    }

    fn run_on(scene: &Scene, outputs: &[Output]) -> Outcome {
        run(
            &resolve(&scene.paths),
            &scene.paths.user_file,
            &scene.marker,
            outputs,
        )
        .expect("run")
    }

    #[test]
    fn a_small_screen_gets_the_bar_once() {
        let s = scene("small");
        assert_eq!(
            run_on(&s, &[screen(1366, 768)]),
            Outcome::Wrote(Preset::Bar)
        );
        assert_eq!(
            fs::read_to_string(&s.paths.user_file).expect("doc"),
            "schema = 1\n\n[output.\"*\"]\npreset = \"bar\"\n"
        );
        assert!(s.marker.exists());
        fs::remove_file(&s.paths.user_file).expect("the user deletes the document");
        assert_eq!(run_on(&s, &[screen(1366, 768)]), Outcome::AlreadyRan);
        assert!(!s.paths.user_file.exists());
    }

    #[test]
    fn a_policy_preset_means_no_user_document_at_all() {
        let s = scene("policy");
        fs::create_dir_all(&s.paths.policy_dir).expect("mkdir");
        fs::write(
            s.paths.policy_dir.join("50-site.toml"),
            "schema = 1\n[output.\"*\"]\npreset = \"minimal\"\n",
        )
        .expect("policy");
        assert_eq!(
            run_on(&s, &[screen(1366, 768)]),
            Outcome::LeftToPolicyOrUser
        );
        assert!(!s.paths.user_file.exists());
        assert!(s.marker.exists());
    }

    #[test]
    fn an_existing_user_document_is_left_alone() {
        let s = scene("existing");
        fs::create_dir_all(s.paths.user_file.parent().expect("dir")).expect("mkdir");
        fs::write(
            &s.paths.user_file,
            "schema = 1\n[output.\"*\"]\npreset = \"minimal\"\n",
        )
        .expect("doc");
        assert_eq!(
            run_on(&s, &[screen(1366, 768)]),
            Outcome::LeftToPolicyOrUser
        );
        assert!(fs::read_to_string(&s.paths.user_file)
            .expect("doc")
            .contains("minimal"));
    }

    #[test]
    fn before_any_output_has_a_size_nothing_is_decided() {
        let s = scene("unsized");
        assert_eq!(
            run_on(
                &s,
                &[Output {
                    connector: None,
                    width: 0,
                    height: 0
                }]
            ),
            Outcome::NotYet
        );
        assert!(!s.paths.user_file.exists());
        assert!(!s.marker.exists());
    }
}
