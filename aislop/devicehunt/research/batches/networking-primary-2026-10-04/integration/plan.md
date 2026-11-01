# networking primary integration — staged, awaiting independent QA

Staging was authorized after source QA, frozen reviewed buying code and the
427-test full suite passed. Candidate and clean rebuild are complete; the manifest
has the accepted appended job. Live database and normal exports are unchanged.
Promotion remains unauthorized pending independent candidate QA. See
`review-ready.json`, `candidate-audit.json` and `candidate-practical-proof.json`.
The sequence below records the accepted staging plan; do not rerun mutation steps.
`preflight.py` is read-only and its `preflight.json` binds the currently proposed
inputs/captures to the unchanged live database and manifest.

## Accepted scope once released

Merge the two candidate payloads into
`data/networking-primary-facts-2026-10-04.json`. Preserve every device row/fact
verbatim. Change ONLY Oracle source snapshot to
`raw/networking-primary-oracle-2026-10-04/oracle-x7-2c-vpat-t2-6972.html` so it
resolves from `data/`; verify identical snapshot bytes. The owned source already
uses a data-relative snapshot. Validate both payloads and all captures before
calling curated import; no importer/schema changes are needed.

The unbranded AS7772-32X software-reference record adds an identity separate
from branded hardware; empty brand and affiliations remain empty. Oracle printed
`Oracle Server X7-2c` routes by existing casefold logic to the current Oracle
identity without creating an alias. ByteDance stays unimported.

Expected totals: 4,084 identities, 5,302 records, 283 sources, 44,763 facts.
Expected deltas: +1 identity, +2 records, +2 sources, +35 facts, +2 Oracle
brand/platform-vendor affiliations. Documents, pages, hyperscaler assertions,
complete reference bundle and all historical inputs remain unchanged.

## Sequential staging and verification

1. Freeze accepted input/capture/code hashes and full-suite log; recheck live
   SHA and manifest against `before.json`. Create new before-networking-primary
   live/manifest backups; never overwrite an existing path.
2. Append one curated manifest job and three hashes (merged JSON and two raw
   captures); expected 118 jobs / 2,181 inputs. Preserve every existing manifest
   entry/hash; update only accepted totals and two new source counts.
3. Copy live to `data/networking-primary-candidate-2026-10-04.sqlite`, import
   merged curated payload through existing `pipeline.import_source`. No live
   import. Verify deltas and all old rows including timestamps, raw JSON, source
   metadata, roles and facts with duplicate-sensitive counters, no exclusions.
4. Rebuild offline into new
   `data/rebuilt-with-networking-primary-2026-10-04.sqlite`. Run inspected
   `scripts/compare_rebuild.py` with explicit `--database` candidate and unique
   prefix. Supplement its set comparison with counters over all 43 logical
   tables, including FTS/coverage; only rebuild-generated observed/fetched/cache
   fields are omitted from candidate/rebuild equality. This omission never
   applies to old-live preservation. Validate SQLite integrity/foreign keys.
5. For old-live/candidate device FTS, permit updated content only for the
   existing Oracle identity; verify unrelated entries exactly and validate new
   artifact/Oracle FTS against underlying records. Document FTS stays identical.
6. Save actual unfiltered CLI buying reports for `Oracle Server X7-2C` and
   `AS7772-32X`; check variant separation, processor watts outside PSU facts,
   historical dependencies/scoped limits, no borrowed artifact brand/roles.
   Verify owned 10/10 with nine documents + one photo and ByteDance 0/2.
7. Send immutable candidate/log/audit paths to independent QA and stop. Promotion,
   normal export refresh and current-document edits need the coordinator's
   explicit followup. Historical stage/finish scripts are references only and
   must not run against this batch/live state.

Existing `devicehunt.rebuild.read_manifest`, `pipeline.import_source` and
`scripts/compare_rebuild.py` provide normal import/rebuild checks. The inspected
previous `integration/audit.py` illustrates counters, but its QCT source-metadata
exceptions and changed-device list must not be reused here.
