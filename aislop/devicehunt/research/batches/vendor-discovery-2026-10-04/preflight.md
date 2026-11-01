# vendor discovery integration preflight — 2026-10-04

read-only independent review; researcher files are still in progress and are not
accepted by this report. no imports, tests, acquisition or shared-state writes ran.

## verified starting point

live sha256 `1e7a877f46b0140c62993de0ebc48b0378773db30c0e889cdfb67244371d7074`;
4,083 identities, 5,298 records, 278 sources, 44,690 facts, 55 archived PDFs and
3,649 pages. manifest: 116 jobs / 2,172 hashed inputs. its sole reference-documents
job uses `raw/reference-documents-with-nics-wiwynn-hyve-2026-10-04.json`.
that complete *reference* bundle has 44 entries; 55 is the entire archive including
other document collections. do not confuse these counts when deriving deltas.

## minimum sound data-only recipe

1. freeze accepted researcher inputs and capture hashes, original URLs/outcomes,
   embedded-text page associations, reviewed exact claims and exclusions. recheck
   hashes before integration. ByteDance missing results remain explicit gaps.
   verify model prefix/suffix, system/node scope, QCT brand and separately supported
   Quanta association; do not infer manufacturer or deployment.
2. appoint one integrator. preserve live and manifest at fresh paths. create a new
   candidate by copying live; only new unique curated source IDs are appropriate
   for this additive batch. `importers.curated` does not perform complete upfront
   validation or wrap the whole input in a replacement transaction: independently
   validate all rows/source references/snapshots first, and discard any failed
   candidate. never run first against live.
3. create a new complete reference bundle by retaining all 44 existing entries
   byte-for-byte as JSON values and appending only accepted unique PDFs. place
   byte-identical PDF/text copies beside it, using sibling filenames only.
   `reference_documents.prepare` validates the entire bundle before import;
   reference import replaces the complete `reference-pdf-*` collection, so a
   partial bundle would delete earlier document evidence. verify prepare and all
   reviewed hashes before `pipeline.import_source` on candidate.
4. audit old-live to candidate preservation using duplicate-sensitive Counters
   over logical ordinary rows, retaining all old records, affiliations, identifiers,
   facts, document/pages and raw datasets. allow only explained archive source
   cache/hash/timestamp metadata refreshes. FTS physical internals and document
   FTS rowids can change when the full document collection is reindexed; compare
   document/page/text content plus multiplicities and coverage separately.
5. derive exact candidate deltas and source counts from accepted entries, rather
   than model-name or document-count guesses. check schema unchanged, integrity,
   foreign keys, search/document FTS coverage, 10/10 owned (nine documents + one
   label), separate ByteDance examples, exact new source/model buying reports and
   source/page search. hash database before/after read-only CLI checks.
6. append accepted curated job(s), replace only the complete reference job path,
   register every new bundle/PDF/text/curated/snapshot dependency hash and preserve
   all old file-hash entries/jobs. update manifest expected identities/records,
   source counts and document/pages to candidate; facts are checked separately,
   not added as an unsupported expected key. run `read_manifest` validation.
7. rebuild offline to another new database; compare explicitly against candidate.
   supplement `compare_rebuild.py`'s EXCEPT/count checks with duplicate-sensitive
   logical equality. its 43-table comparison excludes observed_at/fetched_at/
   cache_path; audit relevant metadata changes separately. require integrity,
   baseline, source counts and FTS checks before promotion.
8. run affected existing tests for reference bundle/rebuild/curated routing and
   candidate exact-claim acceptance checks. unchanged-code data-only work does
   not require a new full-suite run under current rules; code/schema/importer
   changes would require full suite and renewed review. independent review must
   accept the candidate/proof and current docs before milestone completion.
9. promote only accepted frozen bytes, retaining prior live; sequentially refresh
   current exports using existing scripts, then verify export hashes match live,
   CLI leaves live unchanged and manifest/rebuild equality still holds. new buying
   reports belong to a fresh dated snapshot; prior snapshots remain historical.

## commands for the assigned integrator (not executed here)

```sh
PY=/Users/nep/.cache/codex-runtimes/codex-primary-runtime/dependencies/python/bin/python3
"$PY" -m unittest discover -s tests -p 'test_reference_documents.py'
"$PY" -m unittest discover -s tests -p 'test_rebuild.py'
"$PY" -m devicehunt --db <new-clean-rebuild> rebuild --manifest data/import-manifest.json
PYTHONPATH=. "$PY" scripts/compare_rebuild.py <new-clean-rebuild> --database <candidate> --output-prefix vendor-discovery-2026-10-04
# only after accepted promotion:
PYTHONPATH=. "$PY" scripts/refresh_exports.py
PYTHONPATH=. "$PY" scripts/refresh_collection_reports.py
```

placeholders require concrete fresh paths. compare script writes proof under
`data/exports/`; it is an integrator action, not a read-only reviewer action.
prior owned-buying driver is historical evidence only; reuse inspected helpers
if useful, never execute its mutation phases for this batch. no need to repeat
old exhaustive capture crawling, acquisition or unrelated verification.

awaiting completed source paths for capture/claim review, then candidate and final
documentation assignment. this report establishes no source acceptance.
