# owned buying import integration plan

Phase 1: read-only driver preparation. Exclusive integrator writes only this
directory until coordinator supplies accepted input hashes, frozen tested code,
test-log hashes and explicit phase 2 assignment. No network, packages or delegation.

Baseline: live SHA `ea7d121849c34cac801c178d48be80ea5f3af09958b526228b6b6f536d0662d8`,
schema 28; 4,083 devices / 5,298 records / 278 sources / 44,664 facts;
55 PDFs / 3,649 pages; manifest 116 jobs / 2,171 hashed inputs.

## sequential stage and acceptance

1. Freeze reviewed new label input, relevant modules/tests/templates, full-suite
   log and review artifact hashes in a new checkpoint here. Recheck before each
   mutation and promotion. Preserve old label input and complete PDF bundle.
2. New paths: `data/owned-buying-import-candidate-2026-10-04.sqlite`,
   `data/rebuilt-with-owned-buying-import-2026-10-04.sqlite`,
   `data/devices-before-owned-buying-import-2026-10-04.sqlite` and
   `data/import-manifest-before-owned-buying-import-2026-10-04.json`.
   Refuse existing outputs. Copy live byte-identically before importing.
3. Use current `Catalog` and `pipeline.import_source`, sequentially importing
   `broadcom` from unchanged `data/raw/broadcom-servers-bundle.json` and `curated`
   from reviewed `data/label-evidence-with-input-marking-2026-10-04.json`.
   No identities, records, sources, schema or PDF changes are expected.
4. Duplicate-sensitive Counter preservation audit of all ordinary tables;
   separately audit source metadata and FTS content. Require all old facts,
   affiliations and identifiers intact. Broadcom raw_json stays byte-identical.
   Only the reviewed existing label raw_json may change, equal to the reviewed
   input row. Source coverage/hash/cache/timestamp changes need exact allowlist.
   All other raw and source values remain unchanged.
5. Expect 24 new `compatibility_bios_certification_rows` facts containing 206
   literal BIOS rows overall. `json_each(facts.value)` must preserve every row,
   order, duplicate, nested metadata and certId from each capture; the three owned
   Oracle platforms each have three rows. No release-to-BIOS join. Add reviewed
   label fact count to 44,664 + 24; derive the label delta from accepted input.
   Require total records and the same 24 Broadcom source records unchanged.
6. Check integrity/FK, FTS coverage, 10/10 owned (9 documents + 1 label), separate
   ByteDance 0/2, unchanged manufacturer/factory counts and 22 hyperscaler rows.
   Verify complete literal record proof and label input power CLI. All CLI/report
   operations explicitly select candidate and prove its SHA unchanged.
7. Backup manifest exclusively. Replace exactly the old curated label job path;
   add new label hash while retaining every previous file entry. No job addition,
   no bundle edit: expected 116 jobs and 2,172 inputs. Keep manifest expected
   counts unchanged; facts are not a supported manifest expected key. Assert
   44,690 facts in separate audits and rebuild comparison; validate manifest via
   current `read_manifest`.
8. Offline clean rebuild via bundled Python `-m devicehunt --db <new-rebuild>
   rebuild --manifest data/import-manifest.json`. Run current
   `scripts/compare_rebuild.py <new-rebuild> --database <candidate>
   --output-prefix owned-buying-import-rebuild-2026-10-04`. Require all 43 tables
   and baseline equal; supplement EXCEPT with Counter duplicate equality.
9. Generate ten buying reports using `driver.buying_snapshot(candidate,new-path)`
   (the previous generator selects live only). Render new candidate HTML using
   current report API; exercise payload and inline script with the existing Node
   DOM harness, preserving scoped identifiers/roles and ByteDance gaps. No browser
   screenshot claim. Save proofs with exact candidate hash.
10. Promotion requires accepted independent QA, frozen tested bytes/full suite,
    preservation, clean equality and proof. Create fresh byte-identical live backup
    and staging copy; atomic replace only after final checks. Sequentially refresh
    `scripts/refresh_exports.py` and `scripts/refresh_collection_reports.py`;
    assert 98 existing lookups refreshed and audit hashes equal final live.
    Save ten buying reports to a new dated snapshot, retaining prior snapshot.
    Recheck manifest/all 55 PDF hashes, final live/rebuild equality and proof;
    return exact verified state to coordinator, sole documentation editor.

Stop on any hash/code drift, review finding, preservation/count mismatch or failed
check. Retain staged artifacts; never promote partially accepted evidence.

`driver.py` has reusable SHA/read-only/Counter/freeze/report helpers and a harmless
read-only default preflight. Phase 2 orchestration is intentionally not enabled
until review and code freeze; historical batch/session drivers are patterns only.
