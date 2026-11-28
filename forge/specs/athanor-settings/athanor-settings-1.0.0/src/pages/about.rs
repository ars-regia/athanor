//! About (doc_settings.md SE22), read-only: what the system says of itself with no privilege.

use std::time::{SystemTime, UNIX_EPOCH};

use athanor_sal::{Kind, Ui, WidgetId, Wrap};
use athanor_trust_state::{Badge, ReadError, Reason, State};

use super::{Ctx, Page};
use crate::i18n::tr;
use crate::info::{self, Hardware};

/// A row of a read-only page: its label, and its value when one was read.
type Line = (String, Option<String>);

pub(crate) fn new() -> Box<dyn Page> {
    Box::new(About::default())
}

#[derive(Default)]
pub(crate) struct About {
    /// Read when About is first drawn: pci.ids alone is over a megabyte.
    pub(crate) hardware: Option<Hardware>,
}

impl Page for About {
    fn draw(&mut self, ctx: &Ctx, ui: &mut Ui, column: WidgetId) {
        let hardware = self.hardware.get_or_insert_with(info::hardware);
        let info = ctx.info.borrow();
        let title = ctx
            .registry
            .page("about")
            .map_or_else(|| tr("About"), |page| page.title.clone());
        let mut rows: Vec<(String, Vec<Line>)> = vec![
            (
                tr("Device"),
                vec![
                    (tr("Device name"), info.device_name().map(str::to_owned)),
                    (tr("Manufacturer"), info.vendor.clone()),
                    (tr("Model"), info.model.clone()),
                ],
            ),
            (
                tr("System"),
                vec![
                    (tr("System"), ctx.system.map(str::to_owned)),
                    (tr("System integrity"), Some(trust_text(info.trust.as_ref(), now()))),
                ],
            ),
        ];
        let mut parts = vec![
            (tr("Processor"), hardware.processor.clone()),
            (tr("Memory"), hardware.memory.clone()),
        ];
        parts.extend(
            hardware
                .graphics
                .iter()
                .map(|name| (tr("Graphics"), Some(name.clone()))),
        );
        parts.push((tr("Disk capacity"), hardware.disk.clone()));
        rows.push((tr("Hardware"), parts));
        drop(info);

        let title = ui.add(column, Kind::Label(title));
        ctx.look.title(ui, title);
        for (name, lines) in rows {
            let group = ui.add(column, Kind::Group(name.clone()));
            let heading = ui.add(group, Kind::Label(name));
            ctx.look.heading(ui, heading);
            // ponytail: two labels side by side until Sal's preference row (SL4) reads them as
            // one control named by its title.
            for (label, value) in lines {
                let Some(value) = value else { continue };
                let row = ui.add(group, Kind::Row);
                let label = ui.add(row, Kind::Label(label));
                ctx.look.note(ui, label);
                let value = ui.add(row, Kind::Label(value));
                ui.set_wrap(value, Some(Wrap::default()));
            }
        }
    }
}

/// The trust state as About words it (SE22): the shield's own classification
/// (`athanor_trust_state::badge`, at `now`), so "Verified" only where the shield shows the
/// system as verified. A refused update is the shield's to say, not About's, so that badge
/// reads plain "Not verified", as every state that was not read or not trusted does.
fn trust_text(read: Option<&Result<State, ReadError>>, now: i64) -> String {
    let Some(Ok(state)) = read else {
        return tr("Not verified");
    };
    match athanor_trust_state::badge(state, now) {
        Badge::Check => tr("Verified"),
        Badge::Cross => tr("Not verified"),
        Badge::Attention if state.verified.value => tr("Not verified yet"),
        Badge::Attention => match state.verified.reason {
            Reason::Media => tr("Not verified: installed from media"),
            Reason::NoSignature => tr("Not verified: no signature"),
            Reason::KeyNotInPolicy => tr("Not verified: its key is not in the policy"),
            Reason::PolicyNotInForce => tr("Not verified: fetched under a permissive policy"),
            Reason::ReferenceOutOfScope => {
                tr("Not verified: the image is outside the policy's scope")
            }
            Reason::LocalChanges => tr("Not verified: the system was changed on this machine"),
            Reason::ChannelAbsent => {
                tr("Not verified: waiting for the stable channel to be published")
            }
            Reason::OriginNotEnforcing => {
                tr("Not verified: this machine does not verify its updates")
            }
            Reason::PinnedBuild => tr("Signed, but pinned to one build: it receives no updates"),
            Reason::Signature => tr("Not verified"),
        },
    }
}

/// Seconds since the epoch, as the shield counts the age of the last check.
fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;
    use athanor_trust_state::{
        state_reply, Deployment, ErrorCode, Policy, SecureBoot, UpdateState, Verified, SCHEMA,
    };

    const NOW: i64 = 1_800_000_000;

    /// A state the shield shows as verified, with the update service at rest.
    fn state(verified: Verified) -> State {
        let booted = Deployment {
            image: "registry.example/athanor-system:stable".into(),
            digest: format!("sha256:{}", "1".repeat(64)),
            version: "43.20260915.2".into(),
            build_time: NOW - 86_400,
        };
        State {
            schema: SCHEMA,
            booted,
            downloaded: None,
            previous: None,
            verified,
            update: UpdateState::None,
            policy: Policy {
                path: "/etc/containers/policy.json".into(),
                sha256: "ab".repeat(32),
                shipped: true,
            },
            secure_boot: SecureBoot {
                secure_boot: Some(1),
                setup_mode: Some(0),
                mok_sb_state: None,
                lockdown: Some("integrity".into()),
            },
            newest_booted_build_time: NOW - 86_400,
            last_successful_check: Some(NOW - 3_600),
            last_error: ErrorCode::None,
            last_error_host: None,
            automatic_security_updates: true,
            postpone: None,
        }
    }

    #[test]
    fn only_a_verified_state_from_root_reads_verified() {
        let json = serde_json::to_string(&state(Reason::Signature.into())).expect("serialises");
        let not_verified = tr("Not verified");
        let read = state_reply(Ok(&json), Some(0));
        assert_eq!(trust_text(Some(&read), NOW), tr("Verified"));
        for read in [
            state_reply(Ok(&json), Some(1000)),
            state_reply(Ok(&json), None),
            state_reply(Err(None), Some(0)),
            state_reply(Err(Some("os.athanor.Update1.Error.NoState")), Some(0)),
            state_reply(Err(Some("os.athanor.Update1.Error.Untrusted")), Some(0)),
            state_reply(Err(Some("os.athanor.Update1.Error.Unreadable")), Some(0)),
            state_reply(Ok("{"), Some(0)),
        ] {
            assert_eq!(trust_text(Some(&read), NOW), not_verified, "{read:?}");
        }
        assert_eq!(trust_text(None, NOW), not_verified, "not yet read");
    }

    #[test]
    fn verified_reads_verified_only_where_the_shield_shows_it() {
        let words = |state: State| trust_text(Some(&Ok(state)), NOW);
        let mut stale = state(Reason::Signature.into());
        stale.last_successful_check = Some(NOW - 15 * 86_400);
        assert_eq!(words(stale), tr("Not verified yet"));
        let mut older = state(Reason::Signature.into());
        older.newest_booted_build_time = NOW;
        assert_eq!(words(older), tr("Not verified yet"));
        // Through the parser, as a real read goes: it refuses a pair that disagrees.
        let json = serde_json::to_string(&state(Reason::PinnedBuild.into())).expect("serialises");
        assert_eq!(
            trust_text(Some(&state_reply(Ok(&json), Some(0))), NOW),
            tr("Signed, but pinned to one build: it receives no updates")
        );
        // The update verdict is the shield's, not About's.
        let mut refused = state(Reason::Signature.into());
        refused.update = UpdateState::Refused;
        assert_eq!(words(refused), tr("Not verified"));
    }

    #[test]
    fn a_state_that_is_not_verified_gives_its_reason() {
        let read = Ok(state(Reason::Media.into()));
        assert_eq!(trust_text(Some(&read), NOW), tr("Not verified: installed from media"));
    }
}
