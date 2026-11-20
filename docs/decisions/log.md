# Decision log

One dated line per maintainer decision, newest first, with a link to where the decision is
recorded: a decision record, an issue, a pull request or the document that cites it. A second
maintainer can find a decision here by its date.

The log is an index, not a record. It does not replace a decision record
([README.md](README.md)): a decision that fixes how Athanor is built, shipped, secured or run
still gets one, and the log line then links to it.

A line has the form `YYYY-MM-DD: what was decided. Where.` Documents cite a decision as
"maintainer decision, <date>", and the date of that citation is the date of the line.

## Backfill

The lines below are the decisions that `CLAUDE.md`, `.claude/rules/` and the specifications
cite as "maintainer decision, <date>" on 2026-11-21. Where a specification cites a date for
several decisions, one line names the date and the specifications that carry them, and the
specification holds the text of each.

- 2026-11-20: the pre-1.0 audit decisions: the confined class, one build policy, the kernel and alerting gates, the firewall acceptance and the recovery console. [ADR-0143](0143-pre-1-0-audit-decisions.md); cited in [doc_lock_and_prompts](../architecture/doc_lock_and_prompts.md).
- 2026-11-19: pull requests of Dependabot and of the athanor-bots App are followed as an App pull request is, with a card and an approval. [.claude/rules/workflow.md](../../.claude/rules/workflow.md).
- 2026-11-19: the decisions cited in [doc_launcher](../architecture/doc_launcher.md), [doc_lock_and_prompts](../architecture/doc_lock_and_prompts.md).
- 2026-11-18: a small amendment to an approved specification is approved in the pull request of its code. [ADR-0145](0145-small-spec-amendment-in-the-code-pull-request.md), [#1282](https://github.com/ars-regia/athanor/issues/1282).
- 2026-11-18: after a CLEAN audit, a later head is audited on the delta. [audits.md](../operations/audits.md), [#1283](https://github.com/ars-regia/athanor/issues/1283).
- 2026-11-18: this decision log. [#1284](https://github.com/ars-regia/athanor/issues/1284).
- 2026-11-18: specifications hold contracts only, no crate internals and no line numbers. [contributing.md](../operations/contributing.md) CT5, [#1287](https://github.com/ars-regia/athanor/issues/1287).
- 2026-11-18: in exploratory areas the specification follows the spike. [contributing.md](../operations/contributing.md) CT5, [#1290](https://github.com/ars-regia/athanor/issues/1290).
- 2026-11-18: the decisions cited in [doc_files](../architecture/doc_files.md), [doc_settings](../architecture/doc_settings.md), [doc_visual_language](../architecture/doc_visual_language.md).
- 2026-11-17: the decisions cited in [doc_portal](../architecture/doc_portal.md), [doc_own_stack](../architecture/doc_own_stack.md).
- 2026-11-16: the decisions cited in [doc_own_stack](../architecture/doc_own_stack.md), [doc_session_daemons](../architecture/doc_session_daemons.md).
- 2026-11-16: a pull request the maintainer decides will not merge is closed with its branch deleted, or its branch is archived. [.claude/rules/workflow.md](../../.claude/rules/workflow.md).
- 2026-11-15: a merged pull request's worktree is removed, and a cargo target outside it is reported with the command that removes it. [.claude/rules/workflow.md](../../.claude/rules/workflow.md).
- 2026-11-12: the decisions cited in [doc_bar](../architecture/doc_bar.md), [doc_kernel_build](../architecture/doc_kernel_build.md).
- 2026-11-11: the decisions cited in [doc_ci](../architecture/doc_ci.md); documents-only pull requests stay outside the open pull request limit. [ADR-0139](0139-docs-pull-requests-outside-the-limit.md).
- 2026-11-07: the decisions cited in [doc_ci](../architecture/doc_ci.md), [doc_pipeline](../architecture/doc_pipeline.md), [doc_bar](../architecture/doc_bar.md), [doc_osd](../architecture/doc_osd.md), [doc_own_stack](../architecture/doc_own_stack.md), [doc_session](../architecture/doc_session.md), [doc_session_daemons](../architecture/doc_session_daemons.md).
- 2026-11-06: the decisions cited in [doc_control_center](../architecture/doc_control_center.md), [doc_launcher](../architecture/doc_launcher.md), [doc_lock_and_prompts](../architecture/doc_lock_and_prompts.md), [doc_osd](../architecture/doc_osd.md), [doc_session](../architecture/doc_session.md), [doc_overview](../architecture/doc_overview.md), [doc_session_daemons](../architecture/doc_session_daemons.md); the own components placed in 1.0, [ADR-0138](0138-own-components-in-1-0.md); pull requests land on `iso-v0` through a merge queue, [ADR-0137](0137-merge-queue-on-iso-v0.md).
- 2026-11-05: on a base with no merge queue, a pull request's checks must have run on the current base before its card. [.claude/rules/workflow.md](../../.claude/rules/workflow.md); the faster road to 1.0, [ADR-0135](0135-faster-road-to-1-0.md).
- 2026-10-30: the decisions cited in [doc_launcher](../architecture/doc_launcher.md), [doc_accessibility](../architecture/doc_accessibility.md), [doc_own_stack](../architecture/doc_own_stack.md); [ADR-0133](0133-magnifier-filters-and-keyboard-aids-in-the-own-compositor.md).
- 2026-10-22: a sync pull request of a rebase integration branch is merged with `--merge`. [.claude/rules/workflow.md](../../.claude/rules/workflow.md); the decisions cited in [doc_kernel_build](../architecture/doc_kernel_build.md), [doc_pipeline](../architecture/doc_pipeline.md).
- 2026-10-19: the decisions cited in [doc_osd](../architecture/doc_osd.md), [doc_visual_language](../architecture/doc_visual_language.md); [ADR-0124](0124-visual-language-revision-3.md).
- 2026-10-17: the decisions cited in [doc_own_stack](../architecture/doc_own_stack.md).
- 2026-10-15: the decisions cited in [doc_portal](../architecture/doc_portal.md), [doc_osd](../architecture/doc_osd.md).
- 2026-10-14: what waits on the maintainer is reported with the paste-ready command. [.claude/rules/workflow.md](../../.claude/rules/workflow.md); with 2026-10-08 and 2026-10-09, every merge goes through a real approval, [ADR-0103](0103-audit-5-decisions.md) D2, cited in [CLAUDE.md](../../CLAUDE.md).
- 2026-10-09: the audit calendar and audit types, [ADR-0102](0102-audit-calendar.md); the decisions cited in [doc_update_delivery](../architecture/doc_update_delivery.md).
- 2026-10-08: the decisions cited in [doc_compositor](../architecture/doc_compositor.md), [doc_accessibility](../architecture/doc_accessibility.md), [doc_languages](../architecture/doc_languages.md), [doc_kernel_profile](../architecture/doc_kernel_profile.md), [doc_platform_experience](../architecture/doc_platform_experience.md), [doc_overview](../architecture/doc_overview.md), [doc_software](../architecture/doc_software.md), [doc_visual_language](../architecture/doc_visual_language.md), [doc_threat_model](../architecture/doc_threat_model.md); [ADR-0099](0099-desktop-specs-batch-4.md).
- 2026-10-06: the decisions cited in [doc_session](../architecture/doc_session.md), [doc_session_daemons](../architecture/doc_session_daemons.md), [doc_visual_language](../architecture/doc_visual_language.md).
- 2026-10-05: the decisions cited in [doc_kernel_profile](../architecture/doc_kernel_profile.md).
- 2026-09-13: the decisions cited in [doc_kernel_profile](../architecture/doc_kernel_profile.md).
