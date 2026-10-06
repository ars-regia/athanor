# Operations

| Field | Value |
| --- | --- |
| Purpose | Index of the runbooks and the team operating model: how to run, release, rotate and rebuild Athanor |
| Owner | the maintainer (`@hr-mes`) |
| Status | revision 1, 2026-10-06 (A2-34) |

A runbook says how to act. What a component must do lives in its spec under
`docs/architecture/`, and why in the decision records under `docs/decisions/`. A runbook links to both and repeats neither.

| Document | What it covers |
| --- | --- |
| [contributing.md](contributing.md) | First day, build, tests, the unit of work, review, red CI, Claude Code (CT1 to CT8) |
| [branching.md](branching.md) | The branches today, the trunk-based model, the `iso-v0` name (BRN1 to BRN6) |
| [repository-layout.md](repository-layout.md) | The top-level layout and the paths that moved |
| [ownership.md](ownership.md) | Areas, their paths and owners, the proposed CODEOWNERS map (OWN1 to OWN4) |
| [secrets.md](secrets.md) | Every GitHub secret, variable and environment the pipeline uses; how each key is generated, where its public half lives, rotation, custody, recovery after a loss (SEC, VAR, ENV, KC, RL) |
| [transfer-to-organisation.md](transfer-to-organisation.md) | Moving the repository to the `athanor-os` organisation, and what a later rename costs (TO1 to TO8) |

`rebuild.md` (rebuild from zero on a new organisation and new machines) is planned in the same phase.
