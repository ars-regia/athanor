# Athanor local AI: direction

Status: **revision 1, 2026-09-30, awaiting the maintainer's approval.** It records one decision the maintainer took on 2026-09-30 and proposes the rest. Nothing in it is implemented, and it changes no file outside itself: section 7 lists the changes other files would take.

The decision, in the maintainer's words translated: a light local AI of this kind is wanted: a tool-call model (Needle), wake word and voice activity detection, speech recognition, embeddings, speech synthesis, and possibly Laya.

The document does not change the objective of `NEXT.md` (an ISO that boots and shows the greeter) or the order of `doc_shell.md`. Its spikes need no shell. Its packages render inside surfaces that stage 2 and later stages deliver, and wait for them.

## 1. Context

**What the repository says today.** `doc_platform_experience.md`, section 4, says the AI translation daemons were removed from the OS and calls the documentation portal "Zero-AI". That sentence is about a static portal built at image time and stays true. It is not a position on a local model in the session, which this document takes.

**What exists, and what it is worth.**

- `athanor-ai-daemon` is outside the workspace and is built by nothing. Its weights loader allocates zero-filled quantized tensors and reports the model as loaded; its DRM lease returns "unimplemented"; its answer to a query is a formatted string. It carries `.expect` calls and direct dependency versions (`candle-core`, `vulkano`, `openvino`). It is not a base to build on.
- `athanor-ui-agent` is a Python daemon that asks Ollama and `llama3.2:1b` for widgets and writes `widgets.json` for `athanor-shell-rs`, which is frozen (`doc_shell.md`, SH4).
- `athanor-semantic-db` is a stub: its spec installs a script that prints one line. It is listed in `custom_packages` and `custom_tier0` of `forge/config/packages.json`, so it ships. That is a facade in the image (`doc_shell.md`, SH1).
- The portal already implements `org.freedesktop.impl.portal.Microphone` and asks the user before granting it. Its prompt still runs through the frozen shell.

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

**AI3. Confinement follows the shell's programs.** Each is a user unit hardened like `athanor-shelld.service`: `ProtectSystem=strict`, `NoNewPrivileges`, `SystemCallFilter=@system-service`, restricted namespaces and realtime, a memory budget with `MemoryHigh` and `MemoryMax` taken from a measurement, and Landlock applied at start (`doc_bar.md`, BR1). `athanor-inference` adds `RestrictAddressFamilies=AF_UNIX` and `IPAddressDeny=any`; `athanor-voice` has the same and reads the microphone only through PipeWire's socket. Whether a runtime needs `MemoryDenyWriteExecute` relaxed is a spike finding, not an assumption. Whether the inference service should also launch inside a Gatekeeper compartment is a question for the maintainer, because the Gatekeeper is not edited without asking (`CLAUDE.md`); this document does not depend on it.

**AI4. Models are signed data with a manifest.**

- Weights ship read-only under `/usr/share/athanor/models/`, in the image, or as separate OCI artifacts signed with the project's cosign key (`doc_update_trust.md`). They are never downloaded at run time and never read from a user-writable path. A user-supplied model is not in version 1.
- A manifest lists, per model, the digest, the licence, the source repository and revision, the languages verified and the runtime it needs. A service refuses a model whose digest is not in its manifest.
- Only models under Apache-2.0, MIT or an equivalent permissive licence ship. A new check in `scripts/verify.py` fails when a manifest names another licence.

**AI5. The tool registry is the capability boundary, and there are two paths.**

- **Path A, the user's own utterance** (typed in the launcher, or transcribed from the microphone). C1 may propose a tool call.
- **Path B, third-party text** (notifications, clipboard, files). C1 may only extract typed data, which the interface offers as a button the user presses ("Copy code", "Open link"). No tool with an effect runs from path B, whatever the text says.

The registry is a closed list in the repository, generated into Needle's schemas at build time. Each tool declares its typed arguments and one effect class:

| Class | Meaning | Examples |
|---|---|---|
| `read` | changes nothing | open a settings page, show the layout |
| `reversible` | changes the session and can be undone at once | layout preset and knobs, do-not-disturb, volume, tiling |
| `confirm` | needs an on-screen confirmation drawn by the bar | power actions, closing windows in bulk |
| `forbidden` | never registered | anything that maps to an `os.athanor.*` polkit action, updates and rollback, the Gatekeeper, attestation, credentials, mesh keys |

Rules that hold for every tool:

- The model's output is untrusted input. It is validated against the schema, executed by the shell's own client, and reaches services through D-Bus and polkit like any other client. The model holds no bus access of its own.
- Below the confidence threshold, or on an empty call list, nothing runs. The interface asks or does nothing.
- A `confirm` needs a keystroke or a pointer press. **Voice never confirms**, because a played recording can say yes.
- A check in `scripts/verify.py` fails when the registry names a `forbidden` action.

**AI6. The microphone has one path and one indicator.**

- Capture goes through PipeWire and through the portal's Microphone interface. The wake word and voice activity stage is the only stage that runs while the user is not speaking to the machine; it is small, runs on the CPU, and exists only while the user has enabled voice control.
- Audio lives in memory, is never written to disk and is discarded once transcribed. Only text leaves `athanor-voice`.
- The bar shows a microphone indicator whenever a capture stream is open. It reads the state of PipeWire's stream, not the voice program's own report, so a program that lies cannot hide it. A toggle in the bar closes the stream at once and respects a hardware mute.

**AI7. Speech synthesis is an accessibility feature first.** It ships as a speech-dispatcher module, so Orca and notifications use it and nothing else speaks in a voice of its own. A voice that does not cover both shipped locales, Italian and English, is not shipped (`doc_shell.md`, SH13).

**AI8. Embeddings feed a real semantic index, which replaces the stub.**

- The index covers application names, settings pages and folders the user chooses, and nothing else by default. It never indexes keyrings, browser profiles or anything under a secrets path.
- It lives under `$XDG_STATE_HOME/athanor/`, inside the encrypted home, and the launcher (stage 3) and Settings (stage 6) query it.
- Whether Needle's own embedding head suffices, or a dedicated model is needed, is decided by spike N1. The Rust runtime the maintainer approves may not carry the head at all (section 5).

**AI9. Laya is admitted only through a gate.** A calibrated score is a hint to the interface, never an input to a security decision.

- It stays out until spike N4 shows, on a labelled sample of real Athanor notifications, that it beats a rule baseline by a margin the plan of the package states.
- If admitted, it runs as an ONNX export inside `athanor-inference`, not as a Python process, with the multilingual checkpoint for Italian.
- It decides nothing about the Gatekeeper, polkit, attestation, updates or the trust state (`doc_shell.md`, SH12).

**AI10. Budgets and hardware.**

- All six capabilities together stay within a resident budget the first measurement fixes, proposed at 1 GB. Models load on use and unload after an idle period the plan states; the wake word and voice activity models stay loaded while voice control is on.
- The recommended hardware stays 8 GiB. On a machine with less, the capabilities are off by default and Settings says why.
- Version 1 runs on the CPU and needs no GPU or NPU. Offload is a later specification, and this document retires the "0 % CPU" claim of `athanor-ai-daemon`.

**AI11. The residues go.** The proposal, for the maintainer to approve because it retires code: `athanor-ai-daemon` and `athanor-ui-agent` are deleted when the first package of this document ships; `athanor-semantic-db` leaves `forge/config/packages.json` now, and returns as a real package with AI8.

## 3. Stages

Each spike produces an answer, not code we keep. Each package has its own plan.

### Spikes

| # | Spike | Settles |
|---|---|---|
| N1 | Needle 3 on the runtime candidates: the official C engine and the third-party Rust runtime. About 30 commands and 30 notifications in Italian and English, with grammars generated from a draft registry | Whether Italian works; accuracy, latency and resident memory on the dev VM at 4, 8 and 16 GiB; whether embeddings are available and on which runtime; which runtime, given `deny.toml` and the rule of the workspace dependency table |
| N2 | Voice front end: voice activity, wake word and two recognisers, on PipeWire, in Italian and English | Which recogniser; word error rate on the same command list; false accepts and rejects of the wake word; CPU and memory while listening |
| N3 | Speech synthesis through speech-dispatcher with Orca | Which voice covers both locales; latency to first audio |
| N4 | Laya against a rule baseline on a labelled notification sample | AI9's gate |

N1 to N3 run as soon as the environment can reach the weights. N4 waits for real notifications from `athanor-shelld`.

### Packages

| Package | Delivers | Gated by |
|---|---|---|
| **A1. Inference service and extraction** | `athanor-inference`, the model manifest and its check, the registry generator, extraction offers in the notification popups of the bar | N1; stage 2b |
| **A2. Commands** | typed commands in the launcher, and settings search | A1; stages 3 and 6 |
| **A3. Voice** | `athanor-voice`, the microphone indicator module of the bar, the voice-control switch | N2; A1 |
| **A4. Speech** | the speech-dispatcher module | N3 |
| **A5. Semantic index** | the index of AI8 | A1; stage 3 |

## 4. Risks

- **Italian.** Needle's language coverage is unknown to us, and small recognisers are often strongest in English. A capability that fails in Italian does not ship, and the spikes are where this is found.
- **Text as an attack.** A notification can carry instructions. AI5's path B makes them data. The plan of A1 includes an injection corpus in its tests.
- **Weights as a supply chain.** Weights are data that can carry behaviour, whoever trained them. Provenance by digest and the closed registry bound the damage: even a model that misbehaves can only call what the registry lists.
- **A misheard command.** Recognition errors are certain to happen. The threshold, the effect classes and the rule that voice never confirms are what keep a mishearing harmless.
- **The runtime.** A C or foreign-function runtime is `unsafe` code beside `panic = "abort"`. It goes in one small crate; its input path is fuzzed with the existing `tests/fuzz` setup; a crash costs a restart of one unit, not the session.
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
5. **A Gatekeeper compartment** for `athanor-inference` (AI3).
6. **A user-supplied model,** for people who want their own language. Excluded from version 1 because the manifest would then hold unsigned weights.
7. **A generative model.** If a use case appears, it needs its own specification. For sizing: a 2B model adds about 3 to 5 GB, a 4B about 6 to 8 GB, a 9B about 8 to 12 GB, so it lifts the recommended memory to 16 GiB from the 4B class up. These figures are estimates.
8. **A remote model on an attested mesh host** (`doc_kernel_profile.md`, D38), for thin machines. Not designed here.

## 7. Changes to other files, if this document is approved

- `doc_platform_experience.md`, section 4: a sentence that "Zero-AI" describes the static portal, and a pointer here.
- `doc_shell.md`: a pointer in the list of later stages; nothing else.
- `doc_bar.md`: the microphone indicator becomes a module of BR3, and the extraction offers join BR4. Both are written in the plan of A1 and A3, not before.
- `forge/config/packages.json`: `athanor-semantic-db` leaves `custom_packages` and `custom_tier0` (AI11).
- `forge/specs/athanor-ai-daemon` and `forge/specs/athanor-ui-agent`: deleted at the first package (AI11), by the maintainer's decision.
- `scripts/verify.py`: two checks, the licence of every manifest entry (AI4) and the absence of `forbidden` actions in the registry (AI5).
- `NEXT.md`: unchanged.

## 8. Acceptance

For each package, on the dev VM and on the maintainer's desktop upgraded in place. The plan of the package states every number.

1. On a fresh install no model process is resident and nothing holds the microphone.
2. With voice control on, the bar shows the microphone indicator while PipeWire holds an open capture stream from `athanor-voice`, and removes it after the stream closes; the toggle closes the stream. Checked in the rig with a null source.
3. A tool call from an utterance is validated against the schema, runs through D-Bus and polkit, and a request for a `forbidden` action is refused before any bus call.
4. A notification carrying instructions produces at most an offer button, and no tool runs from it. An injection corpus passes.
5. Below the confidence threshold, and on an empty call list, nothing runs.
6. A `confirm` tool runs only after a keystroke or a pointer press; a spoken "yes" does not run it.
7. A model whose digest is not in the manifest is refused, and a manifest entry with a non-permissive licence fails `scripts/verify.py`.
8. Resident memory of each program stays within its measured budget, and a model unloads after the idle period.
9. Italian and English each meet the accuracy the plan of the package states on its command and notification sets.
10. Orca reads through the speech module in both locales.
11. The AT-SPI tree of every new surface exposes a role and a name for each control.
