//! What reaches `org.athanor.desktop.idle` from cosmic-idle's store (SD3, SD16): the one-time
//! migration, and the fail-closed follow of cosmic-settings' Power page. Reading the store is
//! `athanor-sulphur-client`'s (SH2); this is what is copied from what it read.
//!
//! A read finds each key valid (`None`, or `Some(n)` with n positive), absent, or invalid (an
//! unreadable or unparsable file, or any other value). An explicit `None` becomes 0, never;
//! `Some(n)` milliseconds becomes the ceiling of n / 1000 seconds, at least 1, clamped to the
//! key's range, so a copy never makes 0 out of a time.

use athanor_sulphur_client::idle::{Idle as Store, Key};

/// The schema's largest `blank-after`, in seconds (SD3).
pub const BLANK_MAX: u32 = 7200;
/// The schema's largest suspend times, in seconds (SD3).
pub const SUSPEND_MAX: u32 = 14400;

/// A key of the schema and the key of the store it follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    BlankAfter,
    SuspendAfterBattery,
    SuspendAfterAc,
}

impl Field {
    pub const ALL: [Field; 3] = [
        Field::BlankAfter,
        Field::SuspendAfterBattery,
        Field::SuspendAfterAc,
    ];

    /// The key's name in `org.athanor.desktop.idle`.
    pub fn key(self) -> &'static str {
        match self {
            Field::BlankAfter => "blank-after",
            Field::SuspendAfterBattery => "suspend-after-battery",
            Field::SuspendAfterAc => "suspend-after-ac",
        }
    }

    /// The name of the store's key it follows, for the journal.
    pub fn store_key(self) -> &'static str {
        match self {
            Field::BlankAfter => "screen_off_time",
            Field::SuspendAfterBattery => "suspend_on_battery_time",
            Field::SuspendAfterAc => "suspend_on_ac_time",
        }
    }

    fn max(self) -> u32 {
        match self {
            Field::BlankAfter => BLANK_MAX,
            Field::SuspendAfterBattery | Field::SuspendAfterAc => SUSPEND_MAX,
        }
    }

    fn read(self, store: &Store) -> Key {
        match self {
            Field::BlankAfter => store.screen_off_time,
            Field::SuspendAfterBattery => store.suspend_on_battery_time,
            Field::SuspendAfterAc => store.suspend_on_ac_time,
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// A valid value of the store: `None` (never) or a positive number of milliseconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Valid {
    Never,
    Millis(u64),
}

/// What a read found for one key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Found {
    Valid(Valid),
    Absent,
    Invalid,
}

fn found(key: Key) -> Found {
    match key {
        Key::Never => Found::Valid(Valid::Never),
        Key::Millis(n) if n > 0 => Found::Valid(Valid::Millis(n)),
        Key::Absent => Found::Absent,
        Key::Millis(_) | Key::Unreadable | Key::Unparsable => Found::Invalid,
    }
}

/// The seconds a valid value becomes in `field`'s range.
fn seconds(field: Field, value: Valid) -> u32 {
    match value {
        Valid::Never => 0,
        Valid::Millis(n) => {
            let s = n.div_ceil(1000).clamp(1, u64::from(field.max()));
            u32::try_from(s).unwrap_or(field.max())
        }
    }
}

/// One journal line at warning for a key that is skipped (SD3, "What is skipped").
fn skipped(field: Field, what: Found) {
    let why = match what {
        Found::Absent => "absent",
        _ => "invalid",
    };
    tracing::warn!(
        key = field.store_key(),
        "the Power page's {} is {why}; {} keeps its value",
        field.store_key(),
        field.key()
    );
}

/// SD16's migration, made once while `migrated` is false: the value to write for each valid
/// key. An absent or invalid key is skipped, and the schema keeps its default.
pub fn migrate(store: &Store) -> Vec<(Field, u32)> {
    Field::ALL
        .into_iter()
        .filter_map(|field| match found(field.read(store)) {
            Found::Valid(value) => Some((field, seconds(field, value))),
            other => {
                skipped(field, other);
                None
            }
        })
        .collect()
}

/// The last valid value of a key, or what the start read found in its place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Last {
    Valid(Valid),
    /// Absent at start: the page never wrote it, so its first valid value is a change.
    AbsentAtStart,
    /// Invalid at start: its first valid value only becomes the last one, uncopied.
    InvalidAtStart,
}

/// The follow of the Power page (SD3): only a change seen while running is copied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Follow {
    last: [Last; 3],
    /// What the last read found for a key it skipped, `None` after a valid read: a key skipped
    /// for the same reason at every event of the directory is one journal line, not one each.
    skipping: [Option<Found>; 3],
}

impl Follow {
    /// From the read made at start, which only learns the last valid value of each key.
    pub fn start(store: &Store) -> Self {
        let start = Field::ALL.map(|field| found(field.read(store)));
        Follow {
            last: start.map(|what| match what {
                Found::Valid(value) => Last::Valid(value),
                Found::Absent => Last::AbsentAtStart,
                Found::Invalid => Last::InvalidAtStart,
            }),
            skipping: start.map(|what| match what {
                Found::Valid(_) => None,
                skipped => Some(skipped),
            }),
        }
    }

    /// Records what a read found for `field`; true when it is a skip the journal has not been
    /// told of since the key's last change of state.
    fn note(&mut self, field: Field, what: Found) -> bool {
        let now = match what {
            Found::Valid(_) => None,
            skipped => Some(skipped),
        };
        let before = std::mem::replace(&mut self.skipping[field.index()], now);
        now.is_some() && now != before
    }

    /// After a change of the store: the value to write for each key that changed. An absent
    /// or invalid read changes nothing and keeps the last valid value.
    pub fn changed(&mut self, store: &Store) -> Vec<(Field, u32)> {
        let mut out = Vec::new();
        for field in Field::ALL {
            let what = found(field.read(store));
            let tell = self.note(field, what);
            let last = &mut self.last[field.index()];
            match what {
                Found::Valid(value) => {
                    let copy = match *last {
                        Last::Valid(old) => old != value,
                        Last::AbsentAtStart => true,
                        Last::InvalidAtStart => false,
                    };
                    *last = Last::Valid(value);
                    if copy {
                        out.push((field, seconds(field, value)));
                    }
                }
                other if tell => skipped(field, other),
                _ => {}
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(blank: Key, battery: Key, ac: Key) -> Store {
        Store {
            screen_off_time: blank,
            suspend_on_battery_time: battery,
            suspend_on_ac_time: ac,
        }
    }

    fn blank(key: Key) -> Store {
        store(key, Key::Absent, Key::Absent)
    }

    #[test]
    fn rounding_never_makes_zero_of_a_time() {
        assert_eq!(seconds(Field::BlankAfter, Valid::Millis(500)), 1);
        assert_eq!(seconds(Field::BlankAfter, Valid::Millis(1000)), 1);
        assert_eq!(seconds(Field::BlankAfter, Valid::Millis(1001)), 2);
        assert_eq!(seconds(Field::BlankAfter, Valid::Millis(900_000)), 900);
        assert_eq!(seconds(Field::BlankAfter, Valid::Millis(u64::MAX)), 7200);
        assert_eq!(
            seconds(Field::SuspendAfterAc, Valid::Millis(u64::MAX)),
            14400
        );
        assert_eq!(seconds(Field::BlankAfter, Valid::Never), 0);
    }

    #[test]
    fn the_migration_copies_never_and_times_and_skips_the_rest() {
        // The maintainer's desktop: three `None`, so the lock stays off as today.
        let none = store(Key::Never, Key::Never, Key::Never);
        assert_eq!(
            migrate(&none),
            [
                (Field::BlankAfter, 0),
                (Field::SuspendAfterBattery, 0),
                (Field::SuspendAfterAc, 0)
            ]
        );
        // No file: nothing is copied, the defaults stay (300 for blank-after).
        assert_eq!(migrate(&store(Key::Absent, Key::Absent, Key::Absent)), []);
        // `Some(0)`, an unreadable and an unparsable key are skipped.
        assert_eq!(
            migrate(&store(Key::Millis(0), Key::Unreadable, Key::Unparsable)),
            []
        );
        assert_eq!(
            migrate(&store(
                Key::Millis(500),
                Key::Absent,
                Key::Millis(1_800_000)
            )),
            [(Field::BlankAfter, 1), (Field::SuspendAfterAc, 1800)]
        );
    }

    #[test]
    fn only_a_change_seen_while_running_is_copied() {
        let mut follow = Follow::start(&blank(Key::Never));
        // The same value again, as at every rewrite of another key: nothing.
        assert_eq!(follow.changed(&blank(Key::Never)), []);
        assert_eq!(
            follow.changed(&blank(Key::Millis(600_000))),
            [(Field::BlankAfter, 600)]
        );
        // The page chose Never while running: 0.
        assert_eq!(follow.changed(&blank(Key::Never)), [(Field::BlankAfter, 0)]);
    }

    #[test]
    fn an_invalid_read_changes_nothing() {
        let mut follow = Follow::start(&blank(Key::Never));
        for key in [
            Key::Unreadable,
            Key::Unparsable,
            Key::Millis(0),
            Key::Absent,
        ] {
            assert_eq!(follow.changed(&blank(key)), [], "{key:?}");
        }
        // An invalid read followed by the same valid `None` copies nothing: a transient read
        // error cannot bring a stale `None` over a `gsettings set`.
        assert_eq!(follow.changed(&blank(Key::Never)), []);
    }

    #[test]
    fn a_key_invalid_at_start_takes_its_first_value_uncopied() {
        let mut follow = Follow::start(&blank(Key::Unreadable));
        assert_eq!(follow.changed(&blank(Key::Never)), []);
        assert_eq!(
            follow.changed(&blank(Key::Millis(60_000))),
            [(Field::BlankAfter, 60)]
        );
    }

    #[test]
    fn a_key_absent_at_start_is_copied_at_its_first_value() {
        let mut follow = Follow::start(&blank(Key::Absent));
        assert_eq!(follow.changed(&blank(Key::Unparsable)), []);
        assert_eq!(follow.changed(&blank(Key::Never)), [(Field::BlankAfter, 0)]);
        assert_eq!(follow.changed(&blank(Key::Never)), []);
    }

    const SCHEMA: &str = include_str!(
        "../../../forge/specs/athanor-system-config/SOURCES/usr/share/glib-2.0/schemas/org.athanor.desktop.idle.gschema.xml"
    );

    /// The shipped keys have the defaults of `Config::default()` and the ranges the follow
    /// clamps to.
    #[test]
    fn the_shipped_schema_has_the_defaults_and_ranges() {
        assert!(SCHEMA.contains(r#"<schema id="org.athanor.desktop.idle""#));
        let d = crate::Config::default();
        let keys = [
            (Field::BlankAfter, d.blank_after),
            (Field::SuspendAfterBattery, d.suspend_after_battery),
            (Field::SuspendAfterAc, d.suspend_after_ac),
        ];
        for (field, default) in keys {
            let name = field.key();
            let block = SCHEMA
                .split(&format!(r#"<key name="{name}" type="u">"#))
                .nth(1)
                .and_then(|rest| rest.split("</key>").next())
                .expect(name);
            let range = format!(r#"<range min="0" max="{}"/>"#, field.max());
            assert!(block.contains(&range), "{name}");
            assert!(
                block.contains(&format!("<default>{default}</default>")),
                "{name}"
            );
        }
        let migrated = SCHEMA
            .split(r#"<key name="migrated" type="b">"#)
            .nth(1)
            .and_then(|rest| rest.split("</key>").next())
            .expect("migrated");
        assert!(migrated.contains("<default>false</default>"));
        assert_eq!(SCHEMA.matches("<key ").count(), 4);
    }

    #[test]
    fn a_skip_is_told_once_per_change_of_the_key_state() {
        let mut follow = Follow::start(&blank(Key::Never));
        let told = |follow: &mut Follow, key| {
            let what = found(key);
            follow.note(Field::BlankAfter, what)
        };
        assert!(told(&mut follow, Key::Unreadable));
        assert!(!told(&mut follow, Key::Unparsable), "still invalid");
        assert!(told(&mut follow, Key::Absent), "absent is another state");
        assert!(!told(&mut follow, Key::Absent));
        assert!(!told(&mut follow, Key::Never), "a valid read is no skip");
        assert!(told(&mut follow, Key::Absent), "skipped again after a valid read");
        // A key skipped at start is not told again while it stays so.
        let mut follow = Follow::start(&blank(Key::Absent));
        assert!(!told(&mut follow, Key::Absent));
    }

    #[test]
    fn keys_are_followed_each_on_its_own() {
        let mut follow = Follow::start(&store(Key::Millis(1), Key::Never, Key::Absent));
        assert_eq!(
            follow.changed(&store(Key::Millis(1), Key::Millis(120_000), Key::Never)),
            [
                (Field::SuspendAfterBattery, 120),
                (Field::SuspendAfterAc, 0)
            ]
        );
    }
}
