# Athanor local AI: direction

Status: **revision 3, 2026-09-30, awaiting the maintainer's approval.** It records two decisions the maintainer took on 2026-09-30, a light local AI and the consent to the microphone, and proposes the rest. Nothing in it is implemented, and it changes no file outside itself: section 7 lists the changes other files would take.

Revision 1 was checked the same day against an audit of the shell, the forge and the kernel build on `iso-v0`. Revision 2 corrects what the audit showed wrong (the microphone path, the shipping of models, the fuzz setup, the tier of a stub) and adds package A0, the prerequisites the audit found missing. The direction did not change.

Revision 3 records the maintainer's decision on microphone consent, the first of four options put that day: consent is a switch in the bar, it covers only our own voice control, and every capture stream is made visible. It rewrites AI6, A0 (a) and (b), the acceptance items and the open doubt 9.

The decision, in the maintainer's words translated: a light local AI of this kind is wanted: a tool-call model (Needle), wake word and voice activity detection, speech recognition, embeddings, speech synthesis, and possibly Laya.

The document does not change the objective of the GitHub milestone `iso-v0` (an ISO that boots and shows the greeter) or the order of `doc_shell.md`. Its spikes need no shell. Its packages render inside surfaces that stage 2 and later stages deliver, and wait for them.

## 1. Context

**What the repository says today.** `doc_platform_experience.md`, section 4, says the AI translation daemons were removed from the OS and calls the documentation portal "Zero-AI". That sentence is about a static portal built at image time and stays true. It is not a position on a local model in the session, which this document takes.

**What exists, and what it is worth.**

- `athanor-ai-daemon` is outside the workspace and is built by nothing. Its weights loader allocates zero-filled quantized tensors and reports the model as loaded; its DRM lease returns "unimplemented"; its answer to a query is a formatted string. It carries `.expect` calls and direct dependency versions (`candle-core`, `vulkano`, `openvino`). It is not a base to build on. ADR-0073 deleted `athanor-ai-daemon` from the tree.
- `athanor-ui-agent`, deleted on 2026-10-05, was a Python daemon that asked Ollama and `llama3.2:1b` for widgets and wrote `widgets.json` for `athanor-shell-rs`, which is frozen (`doc_shell.md`, SH4).
- `athanor-semantic-db` is a stub: its spec installs a script that prints one line. It is listed in `custom_packages` and `custom_tier3` of `forge/config/packages.json`, so it ships. That is a facade in the image (`doc_shell.md`, SH1).
- `athanor-ai-daemon` is in no tier list, so it does not ship; neither did `athanor-ui-agent`. ADR-0073 deleted `athanor-ai-daemon` from the tree.

**What the audit of 2026-09-30 found that this document depends on.** Each item is a prerequisite in package A0 (section 3).

- **The microphone path was not trustworthy, and the portal cannot carry it anyway.** The portal implemented `org.freedesktop.impl.portal.Microphone` and treated the exit status 0 of `athanor-shell-rs --privacy-prompt` as "granted". A closed prompt exited 0, and so did a second identical request, forwarded to the first prompt by the single-instance application (measured on 2026-09-30 with a session bus: the second prompt exited 0 at once). `xdg-desktop-portal` 1.18.4 defines no backend interface for the microphone, the camera or the location (checked against its development package), so it never called those three interfaces and a grant through them controlled nothing. They are removed from the portal on the branch of this revision.
- **PipeWire does not tell us who is recording.** Measured on 2026-09-30 with PipeWire 1.0.5 and WirePlumber 0.4.17 (Ubuntu 24.04; the versions of the image are newer and must be measured again): `pipewire.access` is `unrestricted` for every client, and for a recorder that speaks the PulseAudio protocol `pipewire.sec.pid` is the pid of `pipewire-pulse`, not of the application. The application's own pid and name reach PipeWire only as properties the application declares. Upstream has no audio portal (flatpak/xdg-desktop-portal#1142; as of January 2025 no implementation had begun), and a Flatpak application gets audio, microphone included, from `--socket=pulseaudio`, all or nothing.
- **The bar builds 9 of the 16 modules of BR3.** Audio, network, Bluetooth, battery, tray, notifications and the shield are absent, and so is the notification popup. AI6's indicator and A1's offers need a bar that has them.
- **`athanor-shelld` does not bind a notification to its sender.** `Notify` and `CloseNotification` record and check no sender, and `replaces_id` lets any process of the session replace or close another's notification. `app_name` is self-declared. An extraction offer drawn from such a notification would carry a claim of origin the shell cannot back.
- **Landlock and the memory controller were not asserted for the main kernel; they are now (#110).** `kernel-local` states `CONFIG_SECURITY_LANDLOCK=y` and `CONFIG_MEMCG=y`, which `build.sh` enforces against the generated config, and the boot matrix checks that `landlock` is in the active LSM list and `memory` among the controllers of the root cgroup (2026-09-30; not yet run on a built kernel). There is no swap and no zram today (D15 is not implemented) and KSM is on; `zswap.enabled=1` was removed from the command line.
- **The image has no mechanism for data-only packages.** Nothing ships weights today. The tier repositories are consumed by a mutable tag without a digest, and the kernel is the only artifact pinned by digest and verified. Sources fetched at build time are pinned by URL and a `sources.sha256` file, which suits a data package.

**What was learned on 2026-09-30, from secondary sources.** The environment that produced this document could not reach Hugging Face, `cactuscompute.com` or arXiv. Section 5 lists each claim and its status. None comes from a model card.

**What a light local AI is for.** Three things, none of which needs a generative model: turning a sentence into a typed call on a closed list of actions, extracting typed data from messy text, and hearing a spoken command. The rest of this document is about doing them without weakening the trust the shell exists to show.

## 2. Decisions

**AI1. A light local AI is in the product, opt-in, and nothing listens or loads by default.** Six capabilities, each switchable on its own:

| Id | Capability | Candidate | Runs in |
|---|---|---|---|
| C1 | Tool calls and structured extraction | Needle 3 | `athanor-inference` |
| C2 | Wake word and voice activity detection | openWakeWord, Silero VAD | `athanor-voice` |
| C3 | Speech recognition | to be chosen by spike N2 | `athanor-voice` |
| C4 | Embeddings | Needle's own head, or a dedicated model (N1) | `athanor-inference` |
| C5 | Speech synthesis | to be chosen by spike N3 | speech-dispatcher module |
| C6 | Decision classifier | Laya, only through AI9 | `athanor-inference` |

Out of scope: a generative chat or summarising model (section 6 records the memory it would need), any cloud or remote model, any GPU or NPU offload, speaker identification, on-device training.

**AI2. Two programs, one crate each, split by what they may touch.** The names are proposals; the plan of the first package checks that no crate already has them (`doc_shell.md`, SH4).

- `athanor-voice` opens the microphone and holds C2, C3 and the capture side of C5. It hears audio and emits text. It has no tool registry and no access to the session's actions.
- `athanor-inference` holds C1, C4 and C6. It takes text and emits typed data. It has no audio access and no network.

A compromise of either does not give the other's reach. Neither holds a D-Bus proxy to any service that changes system state.

**AI3. Confinement follows the shell's programs.** Each is a user unit hardened like `athanor-shelld.service`: `ProtectSystem=strict`, `NoNewPrivileges`, `SystemCallFilter=@system-service`, restricted namespaces and realtime, a memory budget with `MemoryHigh` and `MemoryMax` taken from a measurement, and Landlock applied at start (`doc_bar.md`, BR1). Landlock is a fail-closed requirement, so package A0 first makes the kernel assert it (section 3). `athanor-inference` adds `RestrictAddressFamilies=AF_UNIX` and `IPAddressDeny=any`; `athanor-voice` has the same and reads the microphone only through PipeWire's socket. Whether a runtime needs `MemoryDenyWriteExecute` relaxed is a spike finding, not an assumption. Whether the inference service should also launch inside a Gatekeeper compartment is a question for the maintainer, because the Gatekeeper is not edited without asking (`CLAUDE.md`); this document does not depend on it.

**AI4. Models are signed data with a manifest.**

- **Weights ship inside the image, as data packages.** Each is a `noarch` spec in `forge/specs/` whose source is fetched at build time from a pinned revision and checked against `sources.sha256`, the mechanism `fetch_sources.sh` already provides. It installs read-only under `/usr/share/athanor/models/` and travels through the tier flow like any package, so the image's own signature and update path cover it (`doc_update_trust.md`). Separate cosign-signed OCI artifacts are not in version 1: the signing and policy wiring covers image digests only, and would need new work.
- Weights are never downloaded at run time and never read from a user-writable path. A user-supplied model is not in version 1.
- **The service verifies what it loads, and does not trust the path.** The tier repositories reach the image build by a mutable tag, so the digest check in the manifest is the control, not the package manager. The manifest is itself a file of the image.
- A manifest lists, per model, the digest, the licence, the source repository and revision, the languages verified and the runtime it needs. A service refuses a model whose digest is not in its manifest.
- Image size grows with the weights (tens of MB for Needle, a recogniser and a voice; hundreds of MB for a dedicated embedding model). The plan of each package states its size and whether it is optional, and only the capabilities a user enables are loaded.
- Only models under Apache-2.0, MIT or an equivalent permissive licence ship. A new check in `scripts/verify.py` fails when a manifest names another licence.

**AI5. The tool registry is the capability boundary, and there are two paths.**

- **Path A, the user's own utterance** (typed in the launcher, or transcribed from the microphone). C1 may propose a tool call.
- **Path B, third-party text** (notifications, clipboard, files). C1 may only extract typed data, which the interface offers as a button the user presses ("Copy code", "Open link"). No tool with an effect runs from path B, whatever the text says. An offer names its source, and a notification offers one only when `athanor-shelld` has bound the notification to its sender (package A0); until then the offer is not shown.

The registry is a closed list in the repository, generated into Needle's schemas at build time. Each tool declares its typed arguments and one effect class:

| Class | Meaning | Examples |
|---|---|---|
| `read` | changes nothing | open a settings page, show the layout |
| `reversible` | changes the session and can be undone at once | layout preset and knobs, do-not-disturb, volume, tiling |
| `confirm` | needs an on-screen confirmation drawn by the bar | power actions, closing windows in bulk |
| `forbidden` | never registered | anything that maps to an `os.athanor.*` polkit action, updates and rollback, execution control (IPE), attestation, credentials, mesh keys |

Rules that hold for every tool:

- The model's output is untrusted input. It is validated against the schema, executed by the shell's own client, and reaches services through D-Bus and polkit like any other client. The model holds no bus access of its own.
- Below the confidence threshold, or on an empty call list, nothing runs. The interface asks or does nothing.
- A `confirm` needs a keystroke or a pointer press. **Voice never confirms**, because a played recording can say yes.
- A check in `scripts/verify.py` fails when the registry names a `forbidden` action.

**AI6. Consent is a switch in the bar, for our own voice control only, and every capture is visible.** *Decided by the maintainer on 2026-09-30, from the options put that day.*

- **What is consented to.** Only `athanor-voice`. Turning voice control on is the consent. The switch is a module of the bar, in the audio group of `doc_bar.md`, BR3, and it is off by default (AI1). The first time it is turned on, its popover says what listens and what does not: the wake word and voice activity stage, on this machine, with the audio never stored. Turning it off stops `athanor-voice`. How the switch starts and stops the unit, by a call to the user manager or by a flag the unit is conditioned on, is decided in the plan of A3; the state is per user and survives the session. The wake word and voice activity stage is the only stage that runs while the user is not speaking to the machine; it is small, runs on the CPU, and exists only while the user has enabled voice control.
- **What is not covered.** Applications other than `athanor-voice` are not gated: they record without asking, as on the other Linux desktops, and the interface says so wherever it lists them. A refusal per application cannot be made to hold today, because the identity of a PulseAudio-protocol client reaches PipeWire only as properties the client declares (section 1), so a false name would evade it. That stays out of scope until the identity can be trusted on the versions the image ships, or until the audio portal of `xdg-desktop-portal` exists.
- **Everything that records is shown.** The bar shows a microphone indicator whenever any capture stream exists, and lists the applications behind it by the name each declares, marked as declared. It reads PipeWire's streams through the PulseAudio protocol that the audio module already speaks (BR3), never a program's own report, so `athanor-voice` cannot hide its own capture from it.
- **A mute.** The same module mutes the default source, shows a hardware mute when there is one, and respects it. With the source muted, voice control hears silence, and the bar says "muted".
- **The audio.** Audio lives in memory, is never written to disk and is discarded once transcribed. Only text leaves `athanor-voice`.

**AI7. Speech synthesis is an accessibility feature first.** It ships as a speech-dispatcher module, so Orca and notifications use it and nothing else speaks in a voice of its own. A voice that does not cover both shipped locales, Italian and English, is not shipped (`doc_shell.md`, SH13).

**AI8. Embeddings feed a real semantic index, which replaces the stub.**

- The index covers application names, settings pages and folders the user chooses, and nothing else by default. It never indexes keyrings, browser profiles or anything under a secrets path.
- It lives under `$XDG_STATE_HOME/athanor/`, inside the encrypted home, and the launcher (stage 3) and Settings (stage 6) query it.
- Whether Needle's own embedding head suffices, or a dedicated model is needed, is decided by spike N1. The Rust runtime the maintainer approves may not carry the head at all (section 5).

**AI9. Laya is admitted only through a gate.** A calibrated score is a hint to the interface, never an input to a security decision.

- It stays out until spike N4 shows, on a labelled sample of real Athanor notifications, that it beats a rule baseline by a margin the plan of the package states.
- If admitted, it runs as an ONNX export inside `athanor-inference`, not as a Python process, with the multilingual checkpoint for Italian.
- It decides nothing about execution control (IPE), polkit, attestation, updates or the trust state (`doc_shell.md`, SH12).

**AI10. Budgets and hardware.**

- All six capabilities together stay within a resident budget the first measurement fixes, proposed at 1 GB. Models load on use and unload after an idle period the plan states; the wake word and voice activity models stay loaded while voice control is on.
- The recommended hardware stays 8 GiB. On a machine with less, the capabilities are off by default and Settings says why.
- The budget must hold with no swap, which is the state today. If D15 (zram) lands, the budget may relax; it is never planned on it.
- Version 1 runs on the CPU and needs no GPU or NPU. Offload is a later specification, and this document retires the "0 % CPU" claim of `athanor-ai-daemon`.

**AI11. The residues go.** The proposal, for the maintainer to approve because it retires code: `athanor-ai-daemon` is deleted when the first package of this document ships (`athanor-ui-agent` was deleted on 2026-10-05); `athanor-semantic-db` leaves `forge/config/packages.json` now, and returns as a real package with AI8.

## 3. Stages

Each spike produces an answer, not code we keep. Each package has its own plan.

### Spikes

| # | Spike | Settles |
|---|---|---|
| N1 | Needle 3 on the runtime candidates: the official C engine and the third-party Rust runtime. About 30 commands and 30 notifications in Italian and English, with grammars generated from a draft registry | Whether Italian works; accuracy, latency and resident memory on the dev VM at 4, 8 and 16 GiB; whether embeddings are available and on which runtime; which runtime, given `deny.toml` and the rule of the workspace dependency table |
| N2 | Voice front end: voice activity, wake word and two recognisers, on PipeWire, in Italian and English | Which recogniser; word error rate on the same command list; false accepts and rejects of the wake word; CPU and memory while listening |
| N3 | Speech synthesis through speech-dispatcher with Orca | Which voice covers both locales; latency to first audio |
| N4 | Laya against a rule baseline on a labelled notification sample | AI9's gate |

N1 to N3 run on the maintainer's own machine, the environment that reaches Hugging Face; the container that produced this document does not. Each reports its answer in a note under `docs/superpowers/`, with the machine's CPU and memory. N4 waits for real notifications from `athanor-shelld`.

### Packages

| Package | Delivers | Gated by |
|---|---|---|
| **A0. Prerequisites** | four independent fixes, each useful without this document: **(a)** the portal's privacy grant: done on the branch of this revision. The grant was fixed in `xdg-desktop-portal-athanor` and `athanor-shell-rs` (only the Allow button's own status granted; a closed prompt, Escape or status 0 denied), and then the camera, microphone and location interfaces were removed, because `xdg-desktop-portal` never calls them (section 1). The portal offers only FileChooser, behind a check that the caller is `xdg-desktop-portal`; the prompt module is in the history of `db3a221b`, for a future ScreenCast of our own; **(b)** the microphone module of the bar: the indicator of every capture stream, the mute of the default source and the voice-control switch of AI6, in place of the unimplemented `SetPrivacyIndicator`; **(c)** sender binding in `athanor-shelld`, so a notification can be replaced or closed only by the process that sent it; **(d)** the kernel asserts `CONFIG_SECURITY_LANDLOCK`, the `lsm=` list and the memory controller in `kernel-local`, and `athanor-profile-check` verifies them at boot | none; (b) needs the audio module of BR3 |
| **A1. Inference service and extraction** | `athanor-inference`, the model manifest and its check, the model data packages, the registry generator, extraction offers in the notification popups of the bar | N1; A0 (c) and (d); stage 2b |
| **A2. Commands** | typed commands in the launcher, and settings search | A1; stages 3 and 6 |
| **A3. Voice** | `athanor-voice` and the service side of the voice-control switch | N2; A0 (b) and (d); A1 |
| **A4. Speech** | the speech-dispatcher module | N3 |
| **A5. Semantic index** | the index of AI8 | A1; stage 3 |

## 4. Risks

- **Italian.** Needle's language coverage is unknown to us, and small recognisers are often strongest in English. A capability that fails in Italian does not ship, and the spikes are where this is found.
- **Text as an attack.** A notification can carry instructions. AI5's path B makes them data. The plan of A1 includes an injection corpus in its tests.
- **Weights as a supply chain.** Weights are data that can carry behaviour, whoever trained them. Provenance by digest and the closed registry bound the damage: even a model that misbehaves can only call what the registry lists.
- **A misheard command.** Recognition errors are certain to happen. The threshold, the effect classes and the rule that voice never confirms are what keep a mishearing harmless.
- **The runtime.** A C or foreign-function runtime is `unsafe` code beside `panic = "abort"`. It goes in one small crate; its input path gets a `fuzz/` directory in the crate, the layout `forge/scripts/run_fuzz.sh` runs (the `tests/fuzz` that `fuzzing.yml` and the workspace `exclude` name does not exist in the tree, and the plan of A1 fixes or drops that reference); a crash costs a restart of one unit, not the session.
- **The shell that would host the offers is unfinished.** The audit found the bar building 9 of 16 modules, and no notification popup. A0 and A1 do not run ahead of stage 2b, and a surface that would be a facade does not ship (`doc_shell.md`, SH1).
- **Memory on 8 GiB.** The budget of AI10 is a proposal. The first measurement may lower it or turn a capability off on small machines.
- **Accessibility.** Voice control must not displace Orca or the keyboard. Every new surface exposes a role and a name (`doc_shell.md`, SH13).

## 5. Facts learned on 2026-09-30, and how far to trust them

| Claim | Source | Status |
|---|---|---|
| Needle 3: 8 to 29 MB, 29M to 121M parameters, 2 to 20 layers, about 2.1 bits per weight, Apache-2.0, text in and a JSON tool call out, grammar-constrained, with a calibrated confidence | the project README on GitHub and a web search | secondary; model card not read |
| Needle 3 also produces embeddings | the project's own description | not verified; a third-party runtime says its v3 weights carry only a confidence head |
| A third-party Rust runtime (`needle-rs`, MIT) runs Needle 3 and claims token parity with the JAX reference | its GitHub page | third-party claim, not verified |
| Needle 3 was released on 2026-09-17 | a web search | secondary |
| Laya 0.3.22, 2026-09-29, Apache-2.0; 322M and 421M parameter encoders; 0.766 accuracy and 32.8 ms per question on a Tesla T4 against a named competitor | the project README and PyPI | vendor benchmark, GPU only; no CPU or Italian data |
| Openly licensed speech recognisers, wake word and voice activity models, synthesis voices and embedding models exist at sizes from a few tens of MB to 600M parameters | vendor and aggregator articles | candidates only; each one's licence and languages must be read on its model card |
| Models of 2B to 9B parameters need roughly 3 to 10 GB at 4 to 8 bits | aggregator articles | ordinary range; not measured here |

## 6. Open doubts

1. **Names.** `athanor-voice` and `athanor-inference` are proposals.
2. **Which embedding model,** and whether Needle's head is enough (N1).
3. **The wake word.** A custom phrase needs a trained model; who trains it, on what data, and whether the user can change it.
4. **Voice confirmation.** AI5 forbids it outright. A user who cannot press a key needs another answer, for example a spoken passphrase that is not a recording, and that is not designed here.
5. **Confinement beyond AI3** for `athanor-inference`: whether the hardened user unit and Landlock of AI3 suffice, or a stronger compartment is needed.
6. **A user-supplied model,** for people who want their own language. Excluded from version 1 because the manifest would then hold unsigned weights.
7. **A generative model.** If a use case appears, it needs its own specification. For sizing: a 2B model adds about 3 to 5 GB, a 4B about 6 to 8 GB, a 9B about 8 to 12 GB, so it lifts the recommended memory to 16 GiB from the 4B class up. These figures are estimates.
8. **A remote model on an attested mesh host** (`doc_kernel_profile.md`, D38), for thin machines. Not designed here.
9. **Microphone consent: settled on 2026-09-30** (AI6). Still open: the mechanism of the switch (the plan of A3), and whether a consent per application returns once the identity of a PulseAudio client is trustworthy on the PipeWire and WirePlumber of the image (to be measured on Fedora 43) or the audio portal of `xdg-desktop-portal` exists.

## 7. Changes to other files, if this document is approved

- `doc_platform_experience.md`, section 4: a sentence that "Zero-AI" describes the static portal, and a pointer here.
- `doc_shell.md`: a pointer in the list of later stages; nothing else.
- `doc_bar.md`: the microphone module (the indicator of every capture, the mute, the voice-control switch) joins the audio group of BR3, and the extraction offers join BR4. Both are written in the plan of A1 and A3, not before, and not while the bar's own plans are moving.
- `forge/config/packages.json`: `athanor-semantic-db` leaves `custom_packages` and `custom_tier0` (AI11).
- `forge/specs/athanor-ai-daemon`: deleted at the first package (AI11), by the maintainer's decision. `forge/specs/athanor-ui-agent` was deleted on 2026-10-05.
- `scripts/verify.py`: two checks, the licence of every manifest entry (AI4) and the absence of `forbidden` actions in the registry (AI5).
- `forge/specs/athanor-xdg-desktop-portal-athanor`, `forge/specs/athanor-shelld`, `forge/specs/azoth/kernel-local` and `athanor-kernel-profile`: the changes of A0. Each is a fix of a defect the audit found, and none waits for the rest of this document.
- The audit's other findings (the DAG that rebuilds every node, the tier repositories consumed by tag, the `kernel-build.yml` identity that accepts any branch, the shipped command line that contradicts D15 and D16) are outside this document and are reported to the maintainer separately.
- The milestone `iso-v0`: unchanged.

## 8. Acceptance

For each package, on the dev VM and on the maintainer's desktop upgraded in place. The plan of the package states every number.

0. **A0:** a notification can be replaced or closed only by its sender; the boot check fails when Landlock or the memory controller is missing; the portal offers no interface for the microphone, the camera or the location.
1. On a fresh install no model process is resident and nothing holds the microphone.
2. The switch of the bar starts and stops `athanor-voice`. With it on, the indicator shows while PipeWire holds a capture stream from `athanor-voice` and goes away after the stream closes. A capture by any other client is shown too, listed by its declared name and marked as declared. The mute silences the default source and the bar says so. Checked in the rig with a null source.
3. A tool call from an utterance is validated against the schema, runs through D-Bus and polkit, and a request for a `forbidden` action is refused before any bus call.
4. A notification carrying instructions produces at most an offer button, and no tool runs from it. An injection corpus passes.
5. Below the confidence threshold, and on an empty call list, nothing runs.
6. A `confirm` tool runs only after a keystroke or a pointer press; a spoken "yes" does not run it.
7. A model whose digest is not in the manifest is refused, and a manifest entry with a non-permissive licence fails `scripts/verify.py`.
8. Resident memory of each program stays within its measured budget, and a model unloads after the idle period.
9. Italian and English each meet the accuracy the plan of the package states on its command and notification sets.
10. Orca reads through the speech module in both locales.
11. The AT-SPI tree of every new surface exposes a role and a name for each control.
