# finite vendor-discovery integration plan

status: preparation only. no source acceptance, import, rebuild, live-state write,
or export refresh has been authorized to this worker yet. coordinator owns docs.

## required accepted handoff

- review report plus coordinator acceptance identifying exact curated rows, source
  IDs, reviewed hashes, immutable snapshots and permitted PDF bundle additions.
- QCT D52BQ-2U official HTML and D51B-2U vendor-authored public-mirror PDF are
  provisional candidates. T41S reseller-modified material is excluded pending
  explicit reviewed disposition. ByteDance missing evidence remains unimported.
- frozen curated input with all sources and device rows; reference entry and
  PDF/text hashes for each accepted archive addition. preserve capture URL and
  distinguish vendor authorship from mirror hosting in source coverage.

## sequential staging after explicit accepted-input followup

1. recheck accepted hashes and starting live/manifest hashes in `../before.json`.
   validate every curated field, source reference, tuple shape and local snapshot
   before any import. reject source IDs already in live; no hyperscaler or physical
   manufacturer inference. freeze acceptance/checkpoint JSON beside this plan.
2. preserve live and manifest at fresh `data/devices-before-vendor-discovery-2026-10-04.sqlite`
   and `data/import-manifest-before-vendor-discovery-2026-10-04.json`. copy curated
   data to `data/vendor-discovery-facts-2026-10-04.json`, rebasing snapshots with
   verified byte-identical captures. retain researcher originals unchanged.
3. build `data/raw/reference-documents-with-vendor-discovery-2026-10-04.json`
   from all 44 existing reference entries unchanged plus accepted unique entries.
   copy new PDF/text dependencies byte-identically beside bundle; use sibling
   filenames only. complete archive starts at 55 PDFs, not 44. run
   `reference_documents.prepare` before reference import.
4. copy live to fresh `data/vendor-discovery-candidate-2026-10-04.sqlite`.
   import only accepted new curated sources and the complete reference bundle.
   discard a failed candidate; curated importer is not globally transactional.
5. derive identities, records, facts, sources, archive pages and source-count deltas
   from candidate. inspect exact practical facts, source URLs, role boundaries,
   document page/search hits and `buying-report` for D51B-2U and D52BQ-2U.
   audit readonly hashes around CLI calls. inspect old rows and metadata changes
   with `audit.py ... --mode preserve --old-bundle <old-complete-bundle> --new-bundle <new-complete-bundle>`; inspect both FTS content/coverage directly.
6. update manifest by appending accepted curated job, replacing only complete
   reference job path, retaining every old job/hash and registering all new inputs
   and snapshot/PDF/text dependencies. derive expected counts, never add unsupported
   facts key. verify `read_manifest`. run existing `test_reference_documents.py`
   and `test_rebuild.py`; run relevant existing catalog/buying tests as needed.
7. offline rebuild into fresh `data/rebuilt-with-vendor-discovery-2026-10-04.sqlite`.
   run `scripts/compare_rebuild.py` with explicit candidate and unique batch prefix;
   supplement with `audit.py ... --mode equal` for duplicate-sensitive all-43-table
   content equality. owned validation remains nine documents + one label; ByteDance
   remains 0/2. provide candidate hashes, exact deltas, source counts, test logs,
   manifest validation, preservation and CLI/SQL/FTS proof to independent reviewer.
8. stop before promotion/export writes. require coordinator followup accepting
   independent candidate review and assigning promotion. then promote frozen
   candidate bytes, sequentially refresh existing normal/collection/lookup exports,
   add fresh dated buying snapshots, verify export and readonly database hashes,
   candidate/live equality and unchanged manifest/rebuild proof. coordinator edits
   and independently reviews current documentation.

`audit.py` only reads databases and writes a fresh report beside itself. it does
not import, rebuild or promote. equality ignores three reproducibility timestamps/cache
fields; preservation retains all ordinary row fields exactly, including record
observed_at/raw_json. only the named 44 old reference sources may change bundle
hash/cache/fetched metadata, verified against explicit old/new complete bundles.
document FTS content preserves old multiplicities and matches page/key/text rows;
only the two accepted existing device search rows may change. equality requires
all logical FTS contents and duplicate multiplicities equal. `compare_rebuild.py` separately checks deterministic FTS rowids.
use small inspected helpers, never rerun historical owned-buying mutation phases.

runtime: `/Users/nep/.cache/codex-runtimes/codex-primary-runtime/dependencies/python/bin/python3`.
run helper with `PYTHONPATH=.` from project root. no new packages or network tests.

prepared derivatives (not accepted or registered): `d51b-embedded-text-wrapper.json`
and `proposed-d51b-reference-entry.json`. original bare pages list is preserved;
wrapper passes existing read_pages validation and associates reviewed PDF hash.
