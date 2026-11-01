# offline rebuild runtime investigation and resolution

outcome: resolved through existing saved source derivations; no installation,
network, environment/system change or substitution of database rows as evidence.
initial assignment was read-only investigation; coordinator subsequently assigned
exclusive `devicehunt/ntra.py` and `tests/test_ntra.py` implementation. only those
files and this note were written by this worker. coordinator owns routing, saved
bundle and manifest changes.

host runtime is `/etc/profiles/per-user/erin/bin/python3`, Python 3.14.7 from Nix.
README's Mac bundled path is absent. no usable installed pypdf was found in the
realized Nix store packages; pdfplumber/pdfminer/pillow/cryptography exist as
separate packages outside active sys.path. unrelated Python3.13/3.14 environments
exist, but selecting an unproven package mixture is unnecessary. no packages were
imported through ad hoc path changes or installed.

AST inventory of non-stdlib imports across `devicehunt/*.py` finds only:

| module | optional packages | rebuild use |
| --- | --- | --- |
| ntra.py | pypdf, pdfplumber | old `ntra` job directly extracted PDF |
| ntra_register.py | pdfplumber | extract helper only; import already saved bundle |
| ntra_historical.py | pdfplumber | extract helper only; import already saved bundle |
| icasa.py | pdfplumber | extract helper only; import already saved bundle |

all other importer code uses standard-library modules. reference/FCC document
imports consume saved hashed page-text envelopes, not new pypdf extraction.
thus the old target NTRA job was the only rebuild extraction dependency.

existing inputs were independently hashed against current manifest:

- original PDF `ntra-2026-01.pdf`:
  `72f63661db38c56d1d78fdf97dfe5f8268d5ada85a538c7d6911abe8965bb54a`.
- complete text `ntra-2026-01-text.json`:
  `22e891b7891d1259b73f77490d794fe427bb859e981fa19849937fbd64b2df89`.
- positional tables `ntra-all-tables.json`:
  `07bff41523ce7a47532207a28c6bc80b6ec533d64d230751236d0e3d3cfc443c`.
- original register bundle:
  `4118f55510171a9385c1c025e80234fcfefd0374982faaad5aa882e2efd7c486`.

an older `ntra-2026-01.pages.json` is not manifest-pinned and was excluded. no data
was taken from live SQL for import: immutable read-only SQL supplied only expected
comparison output after deriving rows from original PDF-associated saved captures.
current live NTRA source has **169** rows, not the historical note's 119 after
expanded targets. complete text screens 760 pages into 12 candidate pages; saved
positional `target_rows` returns 169 exact dictionaries. screening all complete
saved tables independently also returns the same169 rows. each row equals current
live `records.raw_json` exactly; no missing/new IDs or cell changes.

implemented `ntra.import_saved(c, bundle)` retains existing `import_pdf` unchanged.
new bundle extends the complete register bundle with explicit hashed `text` input.
validates sibling filenames/no symlinks, all dependency hashes, exact reviewed
PDF hash/signature, complete760 page table shape/header, expected47302 row summary
and contiguous sequence, complete PDF-associated text pages and every page header.
then repeats the exact `PAGE_TARGET` screen and proves it omits no target table
rows before calling `save_rows` with the **original PDF path**. source byte hash,
mtime-based historical source metadata and original projection semantics remain
those of the PDF. no optional packages run; changed derivation inputs require
fresh review/hash acceptance rather than a new guessed extraction.

coordinator-produced bundle
`data/raw/ntra-targets-saved-bundle-2026-10-04.json`, SHA-256
`35c77a188f2fac4bfbe1a8479efa7a2912ed19d589e8668257851c0bf89e1b26`,
was imported into a separate TemporaryDirectory Catalog: 760 screened,12 candidate
pages,169 imports. raw row dictionaries exactly equal immutable read-only live
source; imported source hash remains original PDF hash. temporary DB removed
through TemporaryDirectory cleanup; shared catalog stayed unchanged.

checks: seven `test_ntra.py` and four `test_ntra_register.py` tests pass. new tests
forbid optional package imports, verify original PDF source metadata, malformed
noncandidate page table rejection before replacement, wronghash/missing/symlink/
traversal dependencies, incomplete text, mismatched PDF association and screen
omissions while preserving preexisting source rows. one initial test used incorrect
SQL metadata column names and failed; corrected to actual `sha256/cache_path` and
rerun passed. no rebuild/full-suite was run by this worker; coordinator owns those
checks and new `ntra-saved-targets` routing/manifest selection.
