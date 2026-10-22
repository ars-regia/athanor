//! The lineage of security updates (docs/architecture/doc_update_trust.md, UT13): the run of
//! security digests since the last one applied on this machine, and its budget of one
//! postpone, bounded by seven days from the first notice and by seven boots, whichever comes
//! first. Per machine, not per digest: a newer security digest inherits it. Kept in
//! `/var/lib/athanor-update/lineage.json` (`Store::lineage`) and published in the state's
//! `postpone` member while it is open.
use athanor_trust_state::{Postpone, POSTPONE_BOOTS, POSTPONE_SECS};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lineage {
    /// The digest of the last security update applied here; `None` before the first. Kept
    /// whether or not a lineage is open. Both members are required in the file, as `null`
    /// at least: an absent one is no lineage the service wrote.
    #[serde(deserialize_with = "Option::deserialize")]
    pub anchor: Option<String>,
    /// `None` until a security digest reaches `will-apply-at-next-shutdown` after the anchor.
    #[serde(deserialize_with = "Option::deserialize")]
    pub open: Option<Open>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Open {
    /// Wall time of the first notice, seconds since the epoch. Set once: a clock that moves
    /// back never moves it, and so never extends the budget.
    pub first_notice_at: i64,
    pub boots_remaining: u32,
    pub postponed: bool,
    /// The boot id the lineage last counted (`/proc/sys/kernel/random/boot_id`): a soft
    /// reboot keeps it and is not counted.
    pub last_boot_id: String,
}

/// Why `Postpone()` has no budget to spend.
#[cfg_attr(not(test), expect(dead_code, reason = "the security-class check and Postpone() of UT13 call it; until then only the tests do"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Budget {
    /// The lineage's one postpone is spent.
    Used,
    /// Seven days from the first notice have passed, or seven boots.
    Expired,
    /// No security digest is waiting: there is nothing to postpone.
    NotOpen,
}

#[cfg_attr(not(test), expect(dead_code, reason = "the security-class check and Postpone() of UT13 call it; until then only the tests do"))]
impl Lineage {
    /// A security digest reached `will-apply-at-next-shutdown` at `now`, during `boot_id`:
    /// opens the lineage, unless one is open, which the digest then inherits unchanged.
    pub fn notice(&mut self, now: i64, boot_id: &str) {
        if self.open.is_none() {
            self.open = Some(Open { first_notice_at: now, boots_remaining: POSTPONE_BOOTS, postponed: false, last_boot_id: boot_id.to_owned() });
        }
    }

    /// The machine runs boot `boot_id`: one boot fewer remains when it is not the one last counted.
    pub fn boot(&mut self, boot_id: &str) {
        if let Some(open) = self.open.as_mut().filter(|open| open.last_boot_id != boot_id) {
            open.boots_remaining = open.boots_remaining.saturating_sub(1);
            boot_id.clone_into(&mut open.last_boot_id);
        }
    }

    /// The security update `digest` booted (ADR-0127 point 10): the anchor moves to it and
    /// the next security digest opens a new lineage with a new budget.
    pub fn applied(&mut self, digest: &str) {
        self.anchor = Some(digest.to_owned());
        self.open = None;
    }

    /// True when an open lineage's budget is spent by time or by boots. A clock set ahead
    /// expires it, which errs toward applying the fix.
    #[must_use]
    pub fn expired(&self, now: i64) -> bool {
        self.open.as_ref().is_some_and(|open| open.boots_remaining == 0 || now >= open.first_notice_at.saturating_add(POSTPONE_SECS))
    }

    /// Spends the lineage's one postpone and returns its expiry, the first notice plus seven days.
    ///
    /// # Errors
    /// See [`Budget`]; the lineage is unchanged then.
    pub fn postpone(&mut self, now: i64) -> Result<i64, Budget> {
        let expired = self.expired(now);
        let open = self.open.as_mut().ok_or(Budget::NotOpen)?;
        if open.postponed {
            return Err(Budget::Used);
        }
        if expired {
            return Err(Budget::Expired);
        }
        open.postponed = true;
        Ok(open.first_notice_at.saturating_add(POSTPONE_SECS))
    }

    /// The state's `postpone` member: `None` while no lineage is open.
    #[must_use]
    pub fn published(&self) -> Option<Postpone> {
        self.open.as_ref().map(|open| Postpone { anchor: self.anchor.clone(), first_notice_at: open.first_notice_at, boots_remaining: open.boots_remaining, postponed: open.postponed })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use athanor_trust_state::{POSTPONE_BOOTS, POSTPONE_SECS};

    const FIRST: i64 = 1_790_000_000;

    fn opened() -> Lineage {
        let mut lineage = Lineage::default();
        lineage.notice(FIRST, "boot-0");
        lineage
    }

    fn first_notice_at(lineage: &Lineage) -> Option<i64> {
        lineage.open.as_ref().map(|open| open.first_notice_at)
    }

    #[test]
    fn notice_opens_once_and_never_moves_first_notice_at() {
        let mut lineage = Lineage::default();
        assert_eq!(lineage.published(), None, "no lineage is open before the first notice");
        lineage.notice(FIRST, "boot-0");
        let published = lineage.published().expect("open");
        assert_eq!((published.first_notice_at, published.boots_remaining, published.postponed, published.anchor.clone()), (FIRST, POSTPONE_BOOTS, false, None));
        lineage.notice(FIRST + 3 * 86_400, "boot-1");
        assert_eq!(lineage.published(), Some(published));
    }

    #[test]
    fn postpone_once_then_used() {
        let mut lineage = opened();
        assert_eq!(lineage.postpone(FIRST + 60), Ok(FIRST + 604_800));
        assert_eq!(lineage.postpone(FIRST + 120), Err(Budget::Used));
        assert!(lineage.published().expect("open").postponed);
        assert_eq!(Lineage::default().postpone(FIRST), Err(Budget::NotOpen));
    }

    #[test]
    fn a_newer_digest_inherits_the_budget() {
        let mut lineage = opened();
        assert!(lineage.postpone(FIRST + 60).is_ok());
        let before = lineage.clone();
        // The next security digest reaches will-apply-at-next-shutdown: the lineage is per
        // machine, not per digest, so its notice changes nothing.
        lineage.notice(FIRST + 2 * 86_400, "boot-0");
        assert_eq!(lineage, before);
        assert_eq!(lineage.postpone(FIRST + 2 * 86_400), Err(Budget::Used));
    }

    #[test]
    fn seven_boots_expire() {
        let mut lineage = opened();
        for boot in 1..=7 {
            assert!(!lineage.expired(FIRST), "expired after {} boots", boot - 1);
            lineage.boot(&format!("boot-{boot}"));
        }
        assert_eq!(lineage.published().expect("open").boots_remaining, 0);
        assert!(lineage.expired(FIRST));
        assert_eq!(lineage.postpone(FIRST), Err(Budget::Expired));
        lineage.boot("boot-8");
        assert_eq!(lineage.published().expect("open").boots_remaining, 0, "never below zero");
    }

    #[test]
    fn the_same_boot_id_counts_once() {
        let mut lineage = opened();
        lineage.boot("boot-0");
        assert_eq!(lineage.published().expect("open").boots_remaining, POSTPONE_BOOTS, "the boot of the notice is not counted");
        lineage.boot("boot-1");
        lineage.boot("boot-1");
        assert_eq!(lineage.published().expect("open").boots_remaining, POSTPONE_BOOTS - 1);
        let mut closed = Lineage::default();
        closed.boot("boot-1");
        assert_eq!(closed, Lineage::default(), "no lineage, nothing to count");
    }

    #[test]
    fn applied_resets_and_records_the_anchor() {
        let mut lineage = opened();
        assert!(lineage.postpone(FIRST).is_ok());
        lineage.applied("sha256:aa");
        assert_eq!(lineage, Lineage { anchor: Some("sha256:aa".into()), open: None });
        assert_eq!(lineage.published(), None);
        lineage.notice(FIRST + 30 * 86_400, "boot-9");
        let published = lineage.published().expect("a new lineage");
        assert_eq!((published.anchor.as_deref(), published.first_notice_at, published.postponed), (Some("sha256:aa"), FIRST + 30 * 86_400, false));
        assert_eq!(lineage.postpone(FIRST + 30 * 86_400), Ok(FIRST + 37 * 86_400), "the budget is new");
    }

    #[test]
    fn a_clock_set_back_neither_moves_nor_extends() {
        let mut lineage = opened();
        assert!(!lineage.expired(1_000_000_000));
        lineage.notice(1_000_000_000, "boot-0");
        assert_eq!(first_notice_at(&lineage), Some(FIRST));
        assert_eq!(lineage.postpone(1_000_000_000), Ok(FIRST + POSTPONE_SECS), "the expiry stays the first notice's");
        for boot in 1..=7 {
            lineage.boot(&format!("boot-{boot}"));
        }
        assert!(lineage.expired(1_000_000_000), "seven boots expire it whatever the clock says");
        assert_eq!(first_notice_at(&lineage), Some(FIRST));
    }

    #[test]
    fn a_clock_set_ahead_expires() {
        let lineage = opened();
        assert!(!lineage.expired(FIRST + POSTPONE_SECS - 1));
        assert!(lineage.expired(FIRST + POSTPONE_SECS));
        assert!(lineage.expired(i64::MAX));
        let mut lineage = lineage;
        assert_eq!(lineage.postpone(FIRST + 10 * 365 * 86_400), Err(Budget::Expired));
        assert!(!Lineage::default().expired(i64::MAX), "no lineage, nothing to expire");
    }
}
