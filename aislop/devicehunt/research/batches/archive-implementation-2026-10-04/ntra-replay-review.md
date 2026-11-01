# independent NTRA saved replay review

reviewer: storage_engine, 2026-10-04. outcome: **accepted**. Inspected
`devicehunt/ntra.py` saved importer, full-register parser, `tests/test_ntra.py`,
pipeline `ntra-saved-targets` routing, saved bundle and staged manifest switch.
No original databases/captures/manifests were modified. Temporary fixture catalog
was cleaned after checks; no network or optional parser installation was used.

`python3 -m unittest tests.test_ntra -v`: **7 passed**. Tests verify original PDF
source metadata rather than JSON bundle metadata, no optional PDF parser import,
malformed noncandidate-page rejection, all dependency hash failures, missing file,
symlink/traversal refusal, missing text pages, wrong PDF association, text screen
omitting target rows and prior-source preservation on validation failure.

Independent replay of real saved bytes into a new temporary catalog produced
760 screened pages, 12 candidate pages and **169 target rows**. Compared records'
IDs/device IDs/source keys/source URLs/raw JSON with the read-only live source;
all matched. Duplicate-sensitive comparison also matched **704 facts, 169
registry_brand affiliations, zero identifiers and zero hyperscaler rows**. Stable
source metadata (URL, tier, license, coverage, PDF SHA, revision and count) matched.

The two relocation fields differ from historical live metadata: old cache path
under `/Users/nep/...` versus this workspace under `/home/erin/...`; fetched_at
uses copied original PDF mtime at whole-second precision instead of the historical
fractional `.339349` mtime. This follows existing original-PDF importer behavior,
not a change to source evidence or coverage; rebuild comparison already excludes
cache_path/fetched_at. Source SHA remains
`72f63661db38c56d1d78fdf97dfe5f8268d5ada85a538c7d6911abe8965bb54a`.

Saved replay uses the reviewed PDF, full 760-page positional extraction and
complete ordered saved text; it does not read catalog rows as evidence. Strict
sibling filenames, symlink checks and dependency SHA validation precede parsing.
Full table rows/header/page order and contiguous 47,302-row register sequence
validate before screening. Text envelope binds the original PDF SHA and extraction
method, all 760 headers/text pages validate, and screened target rows must equal
full-table target selection. Only then `save_rows()` revalidates selected rows and
replaces its source. Linked model cells remain facts rather than aliases; brand
classification does not create manufacturer/operator assertions.

Staged manifest comparison verified exactly one importer route changed from
`ntra` PDF extraction to `ntra-saved-targets` saved bundle; job count and all
expected counts are unchanged. PDF, full tables, text and new bundle are all
hash-pinned. Full candidate/clean-rebuild equality remains the coordinator's
integration acceptance check.
