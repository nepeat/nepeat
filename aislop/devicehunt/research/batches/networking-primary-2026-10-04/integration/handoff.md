# staged handoff — 2026-10-04

Stage complete; awaiting independent candidate QA. No promotion performed.
`review-ready.json` binds final database/input/manifest/proof hashes.
Candidate: `data/networking-primary-candidate-2026-10-04.sqlite`.
Clean rebuild: `data/rebuilt-with-networking-primary-2026-10-04.sqlite`.
Merged input: `data/networking-primary-facts-2026-10-04.json`.
Rollback inputs: `data/devices-before-networking-primary-2026-10-04.sqlite` and
`data/import-manifest-before-networking-primary-2026-10-04.json`.

Verified 4,084 identities / 5,302 records / 283 sources / 44,763 facts.
Exactly +1 unbranded software-reference identity, +2 records, +35 facts, +2
sources and +2 source-scoped Oracle roles. Oracle routes to the existing identity;
AS7772 software reference stays separate from branded hardware. Every old row,
timestamp, source metadata, raw record, role and fact survives unchanged.

All 43 logical tables match clean rebuild with duplicate-sensitive counters.
Existing FTS content survives except the expected Oracle text addition; both new
and rebuilt FTS completely reconstruct from catalog records/document pages.
SQLite integrity and foreign keys pass. Owned validation is 10/10 (9 documents
+ 1 photo), ByteDance 0/2. Additional Oracle evidence and community leads do not
change coverage; all old baseline evidence/leads/search hits survive.

`oracle-cli-report.json` and `as7772-cli-report.json` are actual unfiltered CLI
outputs for the full model names. Practical proof validates all 35 context facts,
separate SM 1U/SM 2U sections, processor watts outside power/PSU categories,
dated software limits, and no artifact brand/role borrowing (including Accton
filter exclusion). Reports and all audits leave database bytes unchanged.

Manifest has 118 jobs / 2,181 hashed inputs. All original 2,178 inputs, all 56
archived PDF hashes, complete reference bundle, HTML and current export audit
hashes verified unchanged. Only the accepted appended curated manifest job and
its three hashes/totals changed shared existing data. Source/device payloads are
verbatim except the reviewed Oracle source snapshot path rebasing.

Accepted 427-test full-suite and 10-test focused logs were reused with frozen
11 input/proof files and 162 code/test hashes rechecked after staging. No new
acquisition, packages, delegation or code edits. All launched processes exited.

Next: independent QA accepts these candidates, then coordinator authorizes
promotion/export refresh. Coordinator owns current-document updates and later
bounded Ingrasys grouped-source / Inspur / original-vendor gaps. Do not rerun
`stage.py`; it is a one-shot historical artifact after this completed stage.
