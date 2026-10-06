# Operations runbooks

Purpose: index of the runbooks that say how to run, release, rotate and rebuild Athanor.
Owner: maintainer. Status: index, revision 1 (2026-10-06). Defined by decision A2-34.

A runbook says how to act. What a component must do lives in its spec under
`docs/architecture/`, and why in the decision records. A runbook links to both and repeats neither.

| Runbook | Covers | Item ids |
| --- | --- | --- |
| [secrets.md](secrets.md) | Every GitHub secret, variable and environment the pipeline uses; how each key is generated, where its public half lives, rotation, custody, recovery after a loss | SEC, VAR, ENV, KC, RL |

`rebuild.md` (rebuild from zero on a new organisation and new machines) is planned in the same phase.
