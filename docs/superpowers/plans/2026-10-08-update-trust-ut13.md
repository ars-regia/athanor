# Update Control (UT13, ADR-0082): What Is Undefined Before It Can Be Planned

> **For agentic workers:** this is not yet an implementation plan. UT13 and ADR-0082 are not
> defined enough to write tasks with exact interfaces and tests. This document lists what is
> undefined, the proposed default for each point, and what each point blocks. Once the
> maintainer settles them, the implementation plan is written with superpowers:writing-plans
> under the name `2026-10-08-update-trust-ut13-plan.md` (or the date of that day).

**Scope:** `docs/architecture/doc_update_trust.md` UT13 (with the A2-26 mechanism),
`docs/decisions/0082-update-control.md` (postpone and opt-out), `docs/architecture/doc_pipeline.md`
PQ4, PQ6, PL31 and block PB10. Checked against `origin/iso-v0` at 42dc49db.

**Gate of PB10 (doc_pipeline):** "dev-VM harness: a security update applies at the next
shutdown by default; postpone holds it until its limit, then it applies; the opt-out stops
automatic application, shows the warning, and still notifies". Depends on PB5; effort 3
(estimate).

## Global Constraints (for the plan that follows)

- English for code, comments, commits and documentation; no AI attribution.
- Build and test jobs at most 4 CPU threads (`cargo -j 4`, podman `--cpus 4`); one VM at a time.
- Signing and keys stay with the maintainer: the plan never uses a key; it stops at the
  maintainer's signing step.
- No `|| true`, no `continue-on-error`; workflow YAML only calls scripts under the repo.
- Changes to PAM, `system/athanor-bus-api/src/polkit.rs`, the Gatekeeper, attestation or
  crypto/LUKS need the maintainer's approval before editing. The opt-out needs a polkit action
  (point 1), so at least one task of the plan is of this kind.

## 1. What is already in place

| Piece | Where | State |
| --- | --- | --- |
| Download, staged deployment, `will-apply-at-next-shutdown` | `forge/specs/athanor-update/athanor-update-1.0.0/src/check.rs` | the state is derived from a staged deployment that is not download-only; reserved today for UT6's instant between unlock and reboot and for a failed re-lock |
| Tool interface | `src/tools.rs`, trait `Tools` | `status`, `candidate`, `download`, `apply_downloaded`, `relock`, `switch`, `fetch_signature`, `metered`; the unlock UT13 needs exists as `apply_downloaded` |
| Held and refused digests | `src/store.rs` | covers "a digest the user went back from stays held, whatever its class" |
| Signature verification | `src/sigobj.rs` | ECDSA cosign signature of the image; no attestation is fetched or verified |
| Published state | `athanor-trust-state` (schema 1, `deny_unknown_fields`, enum `UpdateState`) | no class, postpone or opt-out field |
| Requests | `src/requests.rs`, `src/serve.rs` | UT6's two requests, `Apply()` and `GoBack()` |

Nothing in the tree reads a security class, a postpone or an opt-out (grep for `class`,
`postpone`, `opt-out` in `athanor-update` finds only the error classifier of `tools.rs`).
The A2-26 mechanism itself (the unattended unlock) is therefore not implemented either: it
cannot be, before the client can tell the classes apart.

## 2. Undefined points

Each point names the texts involved, the proposed default and what it blocks.

### Point 1: where the opt-out switch lives, and its authorisation

- **Texts:** ADR-0082 point 3, "Settings offers a switch that turns automatic application
  off". `doc_settings.md`: "It is not the place for updates: system updates stay in the shield
  and in Software (SW16), and Settings has no updates page" (also F-settings-28 and the About
  page, "The image's update state is not shown: it lives in the shield (SW16)").
- **Undefined:** the host of the switch (Settings, Software, or the shield's update panel of
  BR6); the D-Bus method that sets it (UT6 says "two requests, no arguments"); the polkit
  action name and its defaults.
- **Proposed default:** the switch lives in the shield's sheet (`doc_bar.md` BR6), where the
  security state already is; Settings keeps no updates page, and ADR-0082 is amended by one
  line. Software is not the host: SW16 says "System image updates are not here" and shows only
  a read-only version row (`doc_software.md:11,202`), so placing it there would need an SW16
  amendment, listed in point 6 only if the maintainer chooses Software. A third request on `os.athanor.Update1`,
  `SetAutomaticSecurityUpdates(b enabled)`, under a new polkit action
  `os.athanor.update.set-automatic` with `auth_admin` for every subject (no `_keep`), and UT6
  amended from "two requests" to "three requests".
- **Who decides:** the maintainer (ADR amendment; polkit change needs approval before
  editing).
- **Blocks:** the opt-out half of the PB10 gate; the Software and shield UI tasks.

### Point 2: where the opt-out is stored and how the shield shows it

- **Undefined:** per machine or per user (ADR-0082 implies per machine, since it needs an
  administrator); the file and its owner; whether `athanor-trust-state` gains a field by
  `#[serde(default)]` (readers with `deny_unknown_fields` at schema 1 would refuse it) or by a
  schema bump to 2; the text of the warning shown before it takes effect and of the persistent
  warning in the shield.
- **Proposed default:** per machine, a root-owned file under `/var/lib/athanor-update/`
  written only by the update service; `athanor-trust-state` schema 2 with
  `automatic_security_updates: bool`, every reader updated in the same change (schema bump,
  not a silent default, because the shield's persistent warning must not vanish on an old
  reader); the warning texts written in `doc_update_trust.md` by the maintainer.
- **Who decides:** the maintainer for the texts; the schema choice can follow the default.
- **Blocks:** the shield task and the trust-state schema task.

### Point 3: the postpone

- **Texts:** ADR-0082 point 2 and PQ6, "One postpone per update, up to seven days, then the
  update applies at the next shutdown."
- **Undefined:**
  1. Who may postpone: any session user, the active session only, or an administrator. A
     postpone delays a security fix for every user of the machine.
  2. The request: another D-Bus method (amends UT6 again, see point 1) and its polkit action.
  3. From when the seven days count: the download, the first notice, or the postpone request.
  4. What "up to" means: does the user pick a length, or is it always seven days.
  5. The state of the staged deployment during the postpone: re-locked
     (`ostree admin lock-finalization`, so a shutdown does not apply it) and unlocked again at
     expiry; and the state name the shield shows (a new `UpdateState` variant).
  6. Survival across reboots and across a newer security digest arriving during the postpone
     (does the newer digest inherit the postpone, start its own, or apply at once).
  7. The clock: wall time can be moved by the user; monotonic time does not survive a reboot.
- **Proposed default:** an active local session user may postpone without an administrator
  (`allow_active=yes`), because the update still applies within seven days; always seven days,
  counted from the first notice; the deployment is re-locked and the state reads
  `postponed-until <time>`; the expiry is stored as wall time and also bounded by a count of
  boots (seven boots), whichever comes first; a newer security digest replaces the staged one
  and keeps the remaining postpone of the one it replaces, never extends it.
- **Who decides:** the maintainer (who may postpone, the clock rule, the newer-digest rule).
- **Blocks:** the postpone half of the PB10 gate; the notifier change of UT11 (the notice
  gains a "Postpone" action that UT13 today forbids).

### Point 4: the producer of the security class

- **Texts:** PQ4 and PL31: the class, with its advisory ids, is set when the release is
  signed and signed in `sign-images` (environment `signing-images`) in the key-signed release
  attestation; UD6 and decision 3 of `doc_update_delivery.md` (promotion by `promote.sh` with
  a signing approval) are superseded but not rewritten.
- **Undefined:** no PB block owns the producer of the release attestation (PB4 is provenance,
  PB5 the VSA and policy gate, PB5b the key rotation); the predicate type and its fields;
  where the class and advisory ids come from on a release that runs on push and is not
  dispatched (a file in the repo, a label of the merged pull request, the PB8 advisory
  output); how the client fetches it (OCI referrer, cosign attestation tag), where it stores
  it, and which public key verifies it (`system/keys/` image key 2 after PB5b).
- **Proposed default:** a new block, "PB4b release attestation", before PB10: an in-toto
  statement with a release predicate type (its URI chosen by the maintainer) and fields `build_time`,
  `class` (`security` or `feature`), `advisories` (array of CSAF ids, empty for feature),
  produced by a script under `scripts/ci/`, signed in `sign-images` by the maintainer's key
  job and attached as a cosign attestation; the class comes from a `release-class.toml` file
  committed with the change that motivates it, checked by `policy_check.py` against the PB8
  advisories.
- **Who decides:** the maintainer (new block, predicate, source of the class).
- **Blocks:** everything in UT13 that acts without a confirmation; the PB10 gate cannot be
  met without a signed security-class image in the dev-VM harness.

### Point 5: an attestation that verifies but carries no class field

- **Texts:** UT13, "Open after A2-26, awaiting the maintainer": the proposal reads it as the
  feature class. ADR-0082 does not touch it.
- **Proposed default:** keep the proposal (feature class, the side that asks for a
  confirmation), and add it to UT13 as decided.
- **Who decides:** the maintainer.
- **Blocks:** the class-reading task (one branch of its tests).

### Point 6: the UT13 text is not amended for ADR-0082, PQ4 and PQ6

- **Texts:** UT13 still says "there is no 'Later' for the security class", UT11 that the
  notice "has no 'Later' action", acceptance item 17 "the notice offers no 'Later' action",
  and item 18 speaks of the "promotion attestation" PQ4 replaced with the release attestation.
- **Undefined:** none of the content, but the plan cannot cite a specification that
  contradicts the decision it implements.
- **Proposed default:** one documentation change that amends UT11, UT13 and acceptance items
  16 to 18 to ADR-0082, PQ4 and PQ6, with points 1 to 5 settled, before the implementation
  plan; the matching amendments of `doc_update_delivery.md` UD6 and decision 3 that
  `doc_pipeline.md` already lists; BR6 gains the switch of point 1, or SW16 is amended if the
  maintainer places it in Software.
- **Who decides:** the maintainer approves the amended text.
- **Blocks:** the implementation plan itself.

### Point 7: interaction with soft reboot and metered connections

- **Texts:** UD37 (`apply_kind`, soft reboot for an update that does not change the kernel)
  and UT12 (no download on a metered connection).
- **Undefined:** whether a security update whose `apply_kind` allows a soft reboot applies at
  the next soft reboot or only at a full shutdown; whether a security update downloads on a
  metered connection (CRA Annex I asks for automatic security updates, UT12 holds every
  download).
- **Proposed default:** "next shutdown or reboot" includes a soft reboot the user starts; a
  security update downloads on a metered connection as well (NetworkManager `Metered` 1 "yes"
  or 3 "guessed yes"), because CRA Annex I asks for automatic security updates, while a
  feature update keeps UT12; with the opt-out on, UT12 holds for every class.
- **Who decides:** the maintainer.
- **Blocks:** two branches of the check-service task.

## 3. Requirements that no document states yet

- **Expiry of the release attestation (PL31):** the attestation orders releases but has no
  expiry, so a machine kept offline cannot tell a frozen feed from a quiet one. PL31 defers
  freeze protection; a UT requirement would have to say what the client does with an old
  attestation. The `stable` manifest expiry of 180 days was decided (#160) and is not
  implemented.
- **Audit trail of the opt-out:** ADR-0082 asks for a warning; it does not say whether
  turning automatic updates off and on is logged (journal with a message id, or the trust
  state history). Proposed default: a journal entry with its own `MESSAGE_ID` at each change.

## 4. The plan once the points are settled (outline, not tasks)

1. Documentation amendment (point 6), approved by the maintainer.
2. Release attestation producer and its client verification (point 4, point 5), in the new
   block before PB10; the maintainer runs the signing step.
3. `athanor-trust-state` schema 2 (points 2 and 3).
4. Check service: classify the downloaded digest, unlock a security digest unless opted out or
   postponed, re-lock on postpone, unlock at expiry (A2-26, points 3 and 7).
5. Requests and polkit actions for the postpone and the opt-out (points 1 and 3); **needs the
   maintainer's approval before editing polkit**.
6. Notifier: the notice of UT13 with the "Postpone" action (point 3).
7. Shield and Software: the switch, its warning and the persistent warning (points 1 and 2).
8. Dev-VM harness for the PB10 gate, one VM at a time: a signed security-class test image
   applies at the next shutdown; a postponed one does not, then applies after the limit with
   the clock moved; with the opt-out the image stays downloaded, the warning shows, the notice
   still arrives.

Points 1, 3, 4 and 5 are maintainer decisions; points 2, 6 and 7 have defaults that can be
taken as written if the maintainer agrees with them.
