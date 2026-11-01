# NIC / Wiwynn / Hyve integration plan

Phase 1 complete: read-only preparation, except this plan. No live database,
manifest, bundle, captures, code or exports changed. Phase 2 requires coordinator
acceptance of independent QA, stable code and both curated fact files.

## Inputs and fixed gates

- Live SHA-256: `935d8774acac42b1906f1e30d77b127a7d40534123d72cb1cd916a3a84be8dcb` (rechecked).
- Schema 28; 4,064 identities / 5,273 records / 263 sources / 50 PDFs / 3,641 pages.
- Complete existing reference bundle: `data/raw/reference-documents-with-inspur-2018-inventec-2026-10-04-v2.json` (39 references; other import jobs supply 11 PDFs).
- Facts: `data/curated-inspur-nics-2026-10-04.json` (eight identities, ten rows, one source), and pending `data/curated-wiwynn-hyve-2026-10-04.json`.
- Capture report: `data/raw/nics-wiwynn-hyve-2026-10-04/fetch-report.json`; five PDFs / eight pages. Four product HTML pages may substantiate curated facts; sitemap remains discovery. Ambient HTTP 403 is excluded, with no acquisition retry.
- Keep 10/10 owned checks with 9 document + 1 label evidence, ByteDance 0/2, every manufacturer/factory inventory count, and 22 hyperscaler rows unchanged.
- Engineering handoff/logs in `engineering/`: 411 Python + 6 Node tests. Preserve logs and record tested code hashes before promotion. Current `listings.py` SHA `1717e4ca63cdc7096e0b411a81d1aab0bb7d2db10075d31f4f9f3d42185c34d5`; listing-test SHA `6b22266f0336946d5aed04eeb86f594fcf6c25187dcf7722b716d318ea47a9b4`. Record all relevant code/test/template hashes in phase 2; coordinator must confirm these are the tested bytes. Repeat full tests only if code changes or evidence is insufficient.

## Sequential phase 2

1. Recheck live/input hashes and coordinator QA acceptance. Create batch-specific helpers; historical stage/promote/verify/build/CLI/UI helpers are references only and must never be rerun. Assert every output path is new before writing. Save an exclusive-copy manifest backup.
2. Produce `data/raw/reference-documents-with-nics-wiwynn-hyve-2026-10-04.json` by preserving the entire old JSON document array and appending five uniquely named references. Copy PDF bytes with `/bin/cp -cp` to unique sibling filenames beside the bundle; verify equal SHA-256 against immutable originals/report. Extract each with bundled pypdf into sibling text JSON containing `pdf_sha256`, `extraction_method: pypdf embedded text; no OCR`, and ordered `{page,text}` pages. Validate all 44 references with `devicehunt.reference_documents.prepare`; expected global archive is 55 PDFs / 3,649 pages. Do not rewrite original captures or old bundles.
3. Create `data/nics-wiwynn-hyve-candidate-2026-10-04.sqlite` using a byte-preserving APFS copy of live. Import the COMPLETE bundle via `import_source(c,'reference-documents',bundle)`, then each accepted curated file via `import_source(c,'curated',facts)` sequentially. Importers validate/transactionally replace only their source. Close writers.
4. Attach old live read-only to candidate. Audit old→candidate row preservation for all relevant tables using EXCEPT plus old-row multiplicity checks where appropriate. Audit old source metadata separately: reference sources legitimately get new bundle hash/cache path/fetched timestamp; all other old source values must persist. Exclude FTS from strict old-row equality because reindex can move rowids; verify indexed content/completeness separately. Check exact suffix identities, chassis/node distinctions, rows/conflicts, expected affiliation roles, no invented aliases/manufacture/deployment, source-scoped NIC duplicates and blank remarks. Check integrity, foreign keys, schema, both validation sets, unchanged manufacturer/factory/hyperscaler inventories and raw discovery tables. Save preservation/count audit and candidate hash; assert live hash unchanged.
5. Update manifest only after candidate acceptance: replace the single reference-documents job path; append exactly TWO curated jobs. Add hashes for accepted facts, complete bundle, five sibling PDFs/text JSONs and supporting captures/report, retaining every existing file entry. Compute expected counts and per-source records_seen from candidate; never guess identity/source totals. Expected imports: 116. File total is derived, not hard-coded. Validate through read_manifest.
6. Offline clean rebuild into NEW `data/rebuilt-with-nics-wiwynn-hyve-2026-10-04.sqlite`. From workspace root use bundled runtime (`PYTHONPATH=.`):

   `/Users/nep/.cache/codex-runtimes/codex-primary-runtime/dependencies/python/bin/python3 -m devicehunt --db data/rebuilt-with-nics-wiwynn-hyve-2026-10-04.sqlite rebuild --manifest data/import-manifest.json`

   `PYTHONPATH=. /Users/nep/.cache/codex-runtimes/codex-primary-runtime/dependencies/python/bin/python3 scripts/compare_rebuild.py data/rebuilt-with-nics-wiwynn-hyve-2026-10-04.sqlite --database data/nics-wiwynn-hyve-candidate-2026-10-04.sqlite --output-prefix nics-wiwynn-hyve-rebuild-2026-10-04`

   Require all 43 tables/indexes and baseline equal. This is distinct from old→candidate preservation. Compare source counts, integrity/FK/FTS and validation evidence on rebuild too.
7. Exercise actual CLI with `--db candidate` and read-only hashing: exact NIC/spec/document searches, Wiwynn V versus V+ matches with no borrowed specs for variant leads, Hyve board versus system, all relevant inventories and both validation sets (ByteDance gap CLI exit 2 is expected). Generate candidate HTML into a batch-specific preview and exercise its payload/script through `tests/report.test.cjs` mount harness. Record DOM proof, not browser screenshot. All proof hashes must identify candidate bytes.
8. Promotion gate: accepted QA, code-hash-bound 411+6 test evidence, preservation audit, 43-table rebuild match, read-only CLI/UI and immutable live hash. Create NEW `data/devices-before-nics-wiwynn-hyve-2026-10-04.sqlite` and staging file with `/bin/cp -cp`; verify both hashes; atomically replace live via `os.replace`. Keep candidate, rebuild, backups and captures.
9. Refresh existing exports sequentially using current generic scripts (PYTHONPATH and bundled runtime): `scripts/refresh_exports.py`, then `scripts/refresh_collection_reports.py`. First refreshes validation, all inventory scopes, targets CSV/JSON, HTML and 98 existing lookups; second refreshes current FCC coverage/discovery and hyperscaler CSV/JSON/audits. Do NOT run historical hyperscaler helper with stale 11/15 assertions and 9/10 baseline. Repeat the new UI/CLI proof against final live exports and record matching live hash.
10. Final new batch audit: validate manifest hashes and counts, five originals/siblings and all 55 document files, exact fact rows, old preservation, unchanged raw feed/discovery, 43-table clean rebuild equality, 98 lookup refreshes and all current export-audit hashes. Report actual totals and limits to coordinator for sole documentation editing. Integrator never edits root handoff/source research or authored facts/code.

## Stop / recovery

Stop on failed hashes, QA findings, code drift, preservation/rebuild/count mismatch or ambiguous accepted facts. Preserve staged outputs and report the failing gate. Do not promote partial results, delete historical artifacts, weaken sibling validation, retry denied sources, install packages, or perform network/publish/push actions.
