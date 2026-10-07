# Reporting runbook (Art. 14)

This runbook is followed when the maintainer becomes aware of (1) an actively exploited
vulnerability in Athanor, or (2) a severe incident having an impact on the security of
Athanor. Art. 14 of Regulation (EU) 2024/2847 applies from 2026-09-11 to products in
scope; whether Athanor is in scope is a legal question **[LAWYER]**, and the procedure is
kept ready regardless (ADR-0081).

It is a working procedure, not legal advice. The definitions (what is "actively
exploited", what is "severe") are those of the Regulation and of the Commission guidance;
read them when deciding.

## Contacts and registration

| Item | Value |
| ---- | ----- |
| Reporting platform | ENISA single reporting platform, https://portal.cra-srp.enisa.europa.eu/ |
| Coordinating CSIRT | CSIRT Italia (confirmed by the maintainer; legal confirmation of the Member State of main establishment still **[LAWYER]**) |
| Registration of the manufacturer on the platform | `<MAINTAINER: not done; check first that the platform accepts a natural person>` |
| Reporting e-mail address | esenese@proton.me (as in `.github/SECURITY.md`) |
| Manufacturer identity and postal address | `<MAINTAINER: undecided; not published here>` |

The Art. 13 contact obligation needs a postal address or an electronic address that
users and authorities can use; the maintainer has to decide whether to publish a postal
address or choose an alternative **[LAWYER]**.

## Clocks

The clock starts when the maintainer becomes aware, not when the report arrived or the
vulnerability was fixed. Write the date and time of awareness in the decision log (below)
first.

| Case | Early warning | Notification | Final report |
| ---- | ------------- | ------------ | ------------ |
| Actively exploited vulnerability (Art. 14(1)-(2)) | within 24 hours | within 72 hours | within 14 days after a corrective or mitigating measure is available |
| Severe incident (Art. 14(3)-(4)) | within 24 hours | within 72 hours | within one month of the notification |

## Procedure

1. **Record awareness.** Open a private GitHub security advisory (draft) for the issue.
   Note the date and time of awareness, the source and the affected variants and versions.
   Do not put details in any public place; this repository is public.
2. **Decide.** Is exploitation known (actively exploited), or is there a severe incident?
   Write the decision and the reason, including a decision not to report, in the decision log.
3. **Early warning (24 h).** Submit on the platform that the vulnerability is actively
   exploited or that an incident occurred and, where known, the Member States in which the
   product is available. Nothing else is required at this stage.
4. **Notification (72 h).** Update the report: general information on the product and the
   nature of the exploit or incident, the initial assessment of severity and impact, and any
   corrective or mitigating measure taken. Say what users can do now.
5. **Mitigate.** Prepare the fix as a security update (separate from feature updates, free of
   charge; ADR-0082, UT13 in [doc_update_trust.md](../architecture/doc_update_trust.md)). If
   no fix exists, give users a mitigation they can apply.
6. **Inform users (Art. 14(8)).** When the fix or mitigation is available, publish the
   advisory: the GitHub security advisory and, once the CSAF provider exists
   ([doc_pipeline.md](../architecture/doc_pipeline.md) PL39), a CSAF 2.0 document. For a
   vulnerability still unfixed, tell users the risk and the mitigation they can take. Use the
   notice in the shield for the affected digest when the update path carries one.
7. **Final report.** Within the period in the table: description and severity, the type of
   threat or root cause, the corrective and mitigating measures applied and, where relevant,
   the cross-border impact.
8. **Close.** Add the evidence to the release's evidence bundle (doc_pipeline.md PL42),
   publish the advisory under the coordinated disclosure terms of
   [`.github/SECURITY.md`](../../.github/SECURITY.md), report upstream where the flaw is in a
   component Athanor integrates (Art. 13(6)), and note any change needed in the risk
   assessment ([technical-documentation.md](technical-documentation.md), item 3).

## Decision log

Keep one entry per report or decision, in the private advisory and, once public, in the
evidence bundle of the release.

| Field | Content |
| ----- | ------- |
| Reference | advisory identifier |
| Aware at (UTC) | date and time |
| Decision and reason | report, or not report |
| Early warning sent (UTC) | |
| Notification sent (UTC) | |
| Final report sent (UTC) | |
| Users informed (UTC) and how | |
