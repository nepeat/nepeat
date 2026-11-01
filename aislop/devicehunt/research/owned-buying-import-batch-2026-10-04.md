# owned buying facts import — 2026-10-04

Status: milestone complete; final documentation review passed. The overall goal
remains active. The previous goal turn made verified progress: a read-only
buying command, ten saved reports, reviewed source facts and a simplified handoff.
This batch brings the reviewed label and BIOS details into the SQL catalog.
The coordinator owns this note, PROGRESS and shared documentation.

## ownership

All workers use GPT-6.1 Sol with fresh briefs. Original captures, live database,
manifest and exports remain read-only until the exclusive integration assignment.

| Worker | Bounded deliverable | Exclusive writable paths |
| --- | --- | --- |
| label curator | New version of complete one-label input, preserving every prior assertion and adding photographed input marking | `data/label-evidence-with-input-marking-2026-10-04.json`; `research/batches/owned-buying-import-2026-10-04/label/` |
| BIOS engineer | Validated source-scoped BIOS extraction, buying classification, meaningful tests | `devicehunt/broadcom.py`, `devicehunt/buying.py`, `tests/test_broadcom.py`, `tests/test_buying.py`; `research/batches/owned-buying-import-2026-10-04/engineering/` |
| reviewer | Independent source/code review, then docs review | New assigned review files in this batch directory only |
| integrator | Accepted input manifest, candidate/rebuild, preservation audit, promotion and exports | Assigned batch driver plus exclusive shared data/output ownership after acceptance |

## scope and checks

No new acquisition or source URL recovery is required to import these already
captured facts. Do not import provenance-incomplete X7 VPAT or seller claims in
this batch. Preserve all BIOS certification rows, duplicates and raw field values;
never invent an OS-release-to-BIOS pairing or current firmware recommendation.
Keep the 7760 voltage/frequency/current marking literal and photographed-unit only.

Prefer existing schema/fact storage if it preserves row identity and supports
useful SQL. Validate all source inputs before replacing a source in a transaction.
Keep prior records, roles, identifiers and facts; permit only reviewed source
metadata/raw-record updates. Test affected behavior, run the full suite, rebuild
offline to a new path, compare all tables/indexes, and preserve the ten owned
checks plus separate ByteDance gaps. Refresh affected buying and normal catalog
exports after acceptance. Complete a concise independently reviewed handoff;
the larger thirteen-vendor collection goal remains active.

## accepted result

The source/code review and subsequent independent candidate review accepted the
new label input and all 206 BIOS rows across 24 source records. One exclusive
integrator promoted the candidate after the clean rebuild matched 43 logical
tables, including duplicate-sensitive comparisons of both FTS contents.
Exactly 26 facts were added; no identity, record, role or source was removed.
Source metadata changed only by appending the reviewed label coverage. The 25
reimported records have new import timestamps; all other timestamps are preserved.
Only the label's raw row changed, exactly matching the accepted versioned input.

The 18 affected and 424 full Python tests passed. Real CLI/SQL queries confirm
the 206 literal rows, three rows each for E4/E5/X10, X7's remaining BIOS gap and
the photographed input marking. Generated HTML passed the existing DOM harness.
All 98 saved lookups, normal exports and a new ten-device buying snapshot were
refreshed; the earlier snapshots and live-database backup remain intact.

## proof and reproducibility

- [Input/code review](batches/owned-buying-import-2026-10-04/review.md) and
  [candidate/rebuild review](batches/owned-buying-import-2026-10-04/candidate-review.md).
- [Final documentation/export review](batches/owned-buying-import-2026-10-04/docs-review.md)
  passed with no findings; the coordinator updated only completion status and this link afterward.
- [Coordinator phase acceptance](batches/owned-buying-import-2026-10-04/acceptance.json)
  freezes source, code, test-log and review hashes; original stage acceptance is
  retained separately. Engineering logs and label preservation proof are siblings.
- [Final audit](batches/owned-buying-import-2026-10-04/integration/final-verification.json)
  and `integration/final-cli-dom-proof.json` bind results to the promoted hash.
- [New buying snapshot](../data/exports/owned-buying-import-2026-10-04-buying-reports.json);
  [SQL usage](../docs/buying-reports.md) and exact `documented-bios-query.sql` in
  the batch directory reproduce the practical queries.
- `integration/driver.py` records gated staging, preservation, rebuild and
  promotion. It is historical batch evidence, not a command to rerun blindly.
  The early proof-helper logs retain corrected report-field and X7 expectations;
  the final proof verifies actual requirements. Logical FTS contents are compared,
  not physical SQLite segment layout.

No network acquisition, PDF-bundle change, seller import or VPAT import occurred.
The owned baseline remains 9 documents + 1 photo and ByteDance examples 0/2.
Current counts, recovery paths and the next finite work live in PROGRESS.
