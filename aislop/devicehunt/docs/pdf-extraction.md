# capture complete PDF page text offline

Use an already installed Poppler tool pair to create a new, source-bound text
capture without installing packages or changing the catalog:

```sh
python3 scripts/extract_pdf_pages.py saved.pdf --output saved-text-new.json --tool-directory /path/to/poppler/bin
```

The output parent directory must exist and the output filename must be new.
The tool directory must contain executable `pdfinfo` and `pdftotext` from the
same version. On this host, Poppler 26.06.0 was verified at
`/nix/store/wb8486c5l68f1shsbc217b7m4hbgrw3s-poppler-utils-26.06.0/bin`.
That path is host-specific, not a project dependency to download.

The utility checks the PDF header and page count, extracts every page separately
including blank pages, and checks the PDF hash again before publishing. Calls
have a finite timeout. Existing outputs are never overwritten. Failed extraction
does not publish a successful sidecar. Encrypted inputs are outside this utility's
scope. No HTTP, OCR, import or catalog writes are performed.

Captures declare `Poppler pdftotext -layout embedded text; no OCR`, actual tool
versions, exact extraction parameters, PDF SHA-256 and consecutive numbered page
text. Layout, UTF-8 and Unix line endings are explicit; `-remove-hyphens none`
preserves line-end hyphens that newer Poppler versions otherwise remove by
default. Blank text and whitespace are retained. Provenance includes invocation
arguments, timestamps, pdfinfo output and tool warnings.

Import/rebuild validates saved metadata and hashes offline; it does not invoke
Poppler. Existing `pypdf embedded text; no OCR` captures remain valid with their
original label. Never relabel a web extract or Poppler output as pypdf, and never
overwrite original captures to standardize extraction methods.

Text extraction cannot establish table column relationships, curve values or
image-only content. Review the actual rendered fact-bearing pages before
interpreting practical ratings or identities. Page text search alone establishes
neither manufacturer roles nor component fitment.

For reference-PDF integration, retain the **complete** current collection and add
reviewed new entries. PDF/text dependencies must be sibling filenames beside its
bundle, with matching hashes and page counts. Follow the
[agent workflow](agent-workflow.md) for candidate review, offline rebuild,
preservation checks and promotion; extracting a sidecar does not perform or
authorize those steps by itself.

Implementation passed independent review and 63 guarded focused tests in the
2026-10-12 discovery wave. All five staged PSU PDFs were then extracted locally
with the reviewed utility. See [current acceptance status](../PROGRESS.md) for
full-suite and catalog-integration results.
