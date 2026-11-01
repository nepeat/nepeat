# Poppler extraction support — 2026-10-12

outcome: implemented, **ready for independent review**. no live imports,
archive registration/migration, manifest edits, schema changes, installs,
network or old capture rewrites. eight owned code/test files are stable.
originals are preserved under `before/`; `original-files.json` binds their
paths/hashes and records the two initially absent new files.
`code-manifest.json` binds the final code/tests; diffs cover existing files.

## final behavior

- `devicehunt.documents.read_capture` validates and returns pages plus
  extraction metadata. `read_pages` retains its page-list API. legacy
  `pypdf embedded text; no OCR` captures remain accepted unchanged.
  malformed root/page objects, boolean/nonconsecutive ordinals, bad hashes
  and unknown/OCR methods are rejected.
- exactly one new method:
  **`Poppler pdftotext -layout embedded text; no OCR`**.
  Poppler envelopes require agreeing numeric extractor/pdfinfo versions,
  positive integer `pdf_page_count` matching every ordinal, and strictly
  typed exact `extraction_options`: layout true, UTF-8, unix EOL,
  remove_hyphens none, individual page selection, page_breaks false.
- both document importers persist the sidecar-validated method. reference
  `prepare` keeps its public `(path, [(entry, pages), ...])` shape and
  returns copied entries enriched with the validated method. an untrusted
  bundle-entry method cannot override the sidecar.
- sibling path/hash restrictions, complete bundle/index checks, reference
  HTTPS/tier/uniqueness checks and validation-before-replacement remain.

## utility and preservation

`scripts/extract_pdf_pages.py PDF --output NEW_PATH --tool-directory LOCAL_DIR`
uses explicit local executable paths, argument lists, no shell and 30-second
timeouts under locale C. both tools' versions must agree; pdfinfo must report
a positive page count and unencrypted input. PDF header and before/after
hashes are checked.

each ordinal uses:
`pdftotext -f n -l n -layout -enc UTF-8 -eol unix -remove-hyphens none -nopgbrk PDF -`.
hyphen removal is explicitly disabled because the installed version defaults
to all. blank pages, whitespace and identifiers are preserved without
stripping/OCR/normalization. unexpected form-feed delimiters are rejected.

the output contains the complete source-bound page list, declared count,
stable method/options/versions, UTC timestamp/source filename, raw pdfinfo
and version outputs, exact command provenance and stderr/warnings.
after every call and final validation succeeds, a same-directory temporary
file is published using an exclusive hard link; existing/racing output paths
are never overwritten. failed, timed-out or source-changed extraction
publishes no sidecar.

## archive compatibility correction

the first compatibility run exposed one archive-release failure: enriched
prepared entries differed from legacy pinned raw entries.
the coordinator explicitly extended ownership to `devicehunt/archive.py`
and `tests/test_archive_release.py`, whose originals were saved first.

`register_reference` now compares the complete **raw bundle entries** against
inventory entries, checks count equality with validated preparation, and keeps
those raw entries in reference-document receipt payloads. the saved-derivation
method comes from validated prepared metadata. legacy raw receipt payloads
and pypdf saved-derivation identity construction stay unchanged; Poppler
derivations receive their honest method. no existing recipe or live receipt
is rewritten. a bounded caller search found no other prepared-entry/inventory
equality comparison needing a change.

## checks and remaining acceptance

- `focused-tests.json` / stdout/stderr: **63 tests passed**, covering validator,
  mocked utility commands/failures/blank pages/no-overwrite, both importers,
  mixed-method collections, raw inventory preservation, legacy derivation
  payload/ID equality, lineage and existing PDF source parsers.
- `archive-final-tests.json` / stdout/stderr records the final archive-only
  check after the complete-collection length guard.
- extractor subprocess calls are mocked in tests; no real Poppler execution
  occurred in this engineering batch. non-failing ResourceWarnings for
  unclosed test database connections appeared in the focused run.
- no full acceptance suite or data promotion was performed by this worker.

independent code review is required next. the coordinator will then run the
reviewed utility to a fresh `psu-extraction-r2/`, compare it with the untouched
first-pass staged extraction, and handle complete collection/candidate review
and required acceptance checks. r1 remains unsupported and may have joined
hyphens; do not relabel or overwrite it.
