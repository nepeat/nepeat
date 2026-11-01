# owned hardware buying-evidence milestone — 2026-10-04

Status: complete for this bounded code/review milestone; the overall goal remains active.
The coordinator owns this note, PROGRESS, README and final
acceptance. Previous goal turn made verified progress: 19 identities were imported,
the suffix matcher was fixed and shared exports were refreshed. The app goal is
now active with the current enterprise/networking/server scope.

## assignments

All workers use GPT-6.1 Sol with fresh briefs. Existing captures and the live
database are read-only throughout research and engineering. No network acquisition
or imports are assigned in this milestone; source gaps become finite next leads.

| Worker | Scope | Exclusive writable paths |
| --- | --- | --- |
| networking researcher | Six owned networking identifiers; saved evidence for power, cooling, chassis and software | `research/batches/owned-buying-2026-10-04/networking/`, `research/owned-networking-buying-2026-10-04.md` |
| Oracle researcher | E4-2C, E5-2C, X7-2C, X10-2C; same questions | `research/batches/owned-buying-2026-10-04/oracle/`, `research/owned-oracle-buying-2026-10-04.md` |
| engineer | Read-only exact-identity buying report, source-scoped facts and unknowns | `devicehunt/buying.py`, `devicehunt/__main__.py`, `tests/test_buying.py`, `docs/buying-reports.md`, `research/batches/owned-buying-2026-10-04/engineering/` |
| independent reviewer | Rotate a free slot into evidence/code/documentation review | Assigned review report under `research/batches/owned-buying-2026-10-04/` only |

The coordinator exclusively writes the new buying export and its reproducible
`research/batches/owned-buying-2026-10-04/generate_reports.py`. No integrator is
needed for the catalog because this milestone does not change database inputs.

## acceptance and stop condition

Review each of the ten identifiers against saved source material. Preserve complete
suffixes, record-local aliases/part numbers, literal statements, dates and roles.
Explicit unknowns describe missing collected evidence, not proof of inability to
operate. Distinguish photographed-unit firmware, source-code availability and
dated compatibility testing from bootability, download access or entitlement.

The report must work for any exact catalog identity, keep references and conflicts
on their own records, avoid lending specifications across variants or vendors,
and leave the database unchanged. Meaningful offline tests cover those boundaries
and missing-database behavior. Generate ten owned reports after review and keep
the baseline and ByteDance expectations unchanged. Update the small handoff with
verified findings and exact next work. A report's empty category is a research gap,
not a completed buying assessment.

## accepted results

- [Networking review](owned-networking-buying-2026-10-04.md): six identifiers,
  22 exact records. The 7760 input marking is newly transcribed, not imported.
  Three historical seller statements remain candidate marketplace-only fields.
- [Oracle review](owned-oracle-buying-2026-10-04.md): four identifiers, source
  archive and dated compatibility contexts. BIOS rows/NIST detail are reviewed
  but not imported; X7 VPAT has an unresolved original-URL provenance gap.
- [Buying command](../docs/buying-reports.md): exact models and record-local
  aliases/parts; category gaps, source context, conflicts and seller distinctions.
- [Independent review](batches/owned-buying-2026-10-04/review.md) accepted both
  evidence groups and code after the NIST single-user-mode qualifier and literal
  SQL model-name example were corrected. A reviewer reran six new tests; the
  engineer's 19 affected tests and coordinator's 417-test full suite passed.
- [Generated ten reports](../data/exports/owned-buying-2026-10-04.json) include all
  exact-record company roles. Expected vendors are validation query context;
  they do not hide useful OEM-role records. An initial vendor-filtered snapshot
  is preserved under the batch directory, separate from the current export.
- Owned validation remains 10/10 (9 documents + 1 photo); ByteDance examples
  remain 0/2. The database is byte-identical to the prior accepted checkpoint.
  No schema, import manifest, source capture or old catalog export changed.

Proof lives in `research/batches/owned-buying-2026-10-04/`: `before.json`,
`full-python-tests.log`, `owned-validation.json`, `bytedance-validation.json`,
worker findings/verification and the independent review. `generate_reports.py`
uses the current Python executable and actual CLI, verifies unchanged database
hash and refuses an existing output path. All source acquisition gaps remain
explicit. Next work is in PROGRESS, not another copied backlog here.

[Final documentation review](batches/owned-buying-2026-10-04/docs-review.md)
accepted the handoff, all-role report scope and preserved history. The coordinator's
`final-verification.json` records unchanged database bytes, exact report references,
current artifact hashes and checks; no new data-import acceptance is implied.
