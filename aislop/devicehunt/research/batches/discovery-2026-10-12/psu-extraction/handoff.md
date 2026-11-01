# complete staged PSU extraction — 2026-10-12

outcome: complete, **5/5 PDFs, 63/63 consecutive page records**. all source
hashes match saved captures before/after; all commands returned zero, with
no page stderr warnings or failed/malformed extractions.

used only the already realized Poppler 26.06.0 absolute executable paths from
the adjacent pdf-runtime handoff. no network, installation, system changes,
production edits, original-capture edits, preparation or imports.

batch helper: `extract.py`, run once with project python. do not rerun over
its finalized capture outputs; use a new directory for a future capture version.
`extraction-report.json` preserves exact per-page argv, return codes,
raw stdout/stderr filenames/hashes, raw pdfinfo for each input, raw extractor
version output and source/output hashes.

| staged envelope | pages | SHA-256 |
| --- | --- | --- |
| `bel-pfe1100-series-text.json` | 25 | see extraction-report input entry |
| `bel-pfe1500-series-text.json` | 27 | see extraction-report input entry |
| `bel-pfe1500-nemko-text.json` | 2 | see extraction-report input entry |
| `bel-pfe1500-csa-text.json` | 7 | see extraction-report input entry |
| `fsp550-29fm-text.json` | 2 | see extraction-report input entry |

for every numbered page, ran
`pdftotext -f n -l n -layout -enc UTF-8 PDF -`.
strict UTF-8 decoding; removed at most one final form-feed character and
rejected any remaining form-feed. no stripping, OCR, page omission or
blank-page filtering. all pages happen to contain nonblank embedded text.
saved envelopes bind PDF SHA-256, the exact method
`poppler pdftotext -layout embedded text; no OCR`,
extractor version `26.06.0`, declared page count and ordered
`{page, text}` records.

final offline checks: all reported artifact sizes/hashes verified, all five
envelopes parse, every page sequence equals 1..N, no text has a form-feed,
method/version strings match, source hashes remain unchanged, and all
per-input command statuses are zero.

these are **staged Poppler envelopes**, unsupported by the current pypdf-only
importer contract. no existing prepare/import entrypoint was called; no pypdf
label was spoofed. independent extraction/method review and separately
authorized importer support are required before integration.

next bounded step: review source-bound extraction and the already rendered
CSA/FSP/PFE1100 pages, then decide whether reviewed Poppler method support
belongs in a separate code change. full collection preservation still applies.
