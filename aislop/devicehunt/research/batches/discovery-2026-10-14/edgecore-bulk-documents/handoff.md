# edge-core bulk datasheet corpus

outcome: **complete bounded corpus; independent review pending**. owner/editor
`/root/vendor_bulk_full`. all work is confined to this new directory. previous
vendor-source-expansion artifacts are immutable and all39 recorded hashes still
match. production code, catalog, manifest, complete61-entry reference bundle,
exports, archive and `sources/` were not changed.

**nine new unique PDFs, 34,104,605 bytes, 40 completely extracted pages** across
nine distinct printed family buckets. every page contains some embedded text;
this does not establish that every diagram/table/image is represented. no OCR,
visual-page review, manual specification curation, inferred identities/aliases,
manufacturer/compatibility/deployment claims or structured device facts.

| literal vendor document heading | complete PDF pages |
| --- | ---: |
| AGR400 (AS7946-30XB) DS R05 | 4 |
| AGS8200 Datasheet R01 | 3 |
| AIS800-32D/32O Datasheet R03 | 5 |
| COR550 (AS7926-40XKFB) DS R07 | 4 |
| CSR440 (AS7535-28XB) DS R05 | 4 |
| DCS511 (AS9737-32DB) Datasheet R02 | 5 |
| ECIS4500 PoE Series DS R04 | 7 |
| EXP400-32X Datasheet R01 | 4 |
| SAF4100I DS R03 | 4 |

headings are vendor landing labels, not normalized model identities. grouped
names remain grouped. selection spans printed AGR/AGS/AIS/COR/CSR/DCS/ECIS/EXP/SAF
buckets; no family linkage or per-unit configuration is inferred from the names.

## selection and acquisition

[selection-plan.json](selection-plan.json) was saved before transport and pins the
previous889-row discovery graph, original source-page URLs/hashes, exact link
ordinals/lines, full printed headings/metadata, document IDs and URL novelty.
all selected entries were metadata-labelled Datasheet, had no graph quarantine
or revision-label/URL conflict, and their landing endpoints were absent from
catalog source/record URLs and audited prior request ledgers. nine resources fit
the cap even if each route requires its normal three exchanges; no spare crawl
or individual-device study was added.

[known-failure-audit.json](known-failure-audit.json) pins source-policy/failure
notes and records **54 historical ledgers, eight prior Edge-Core failure rows**.
[prior-url-inventory.json](prior-url-inventory.json) retains51 historical
Edge-Core URL rows and ledger hashes. [prior-pdf-inventory.json](prior-pdf-inventory.json)
contains **165 local PDF paths/hashes** plus catalog reference-document hashes.
AS7710/AS7760/AS7761 denied-document families, DCS203 R09, DCS510/AS9716,
AS7312 and AS4630-54NPE missing-route families, all graph revision disagreements,
known prior routes and prior PDF hashes were excluded. the negative audit is
bounded to the listed local paths and ledger-name patterns, not an assertion
that no other external or historical capture exists.

**28/28 actual HTTP exchanges**, no transport-only failures, no automatic
redirects/retries and no searches:

- one fresh `https://www.edge-core.com/robots.txt`200, original bytes/receipt;
- nine canonical query-free download landing pages200;
- nine exact freshly published `wpdmdl`/refresh URLs302;
- nine individually validated same-origin Location PDF targets200.

fresh policy allows the exact paths for `devicehunt-public-research/1.0`, with
no published crawl delay/request-rate constraint. policy bytes SHA-256
`055ba6f6e008ba3c9bf807f8e461882a5a37076fb31ce03c749dc0801c1b43e2`.
[policy-check.json](policy-check.json) records selection-path allowance and pacing;
every later receipt binds the policy hash/exact-path decision. no stale graph
refresh token, guessed PDF filename, alternate denied route/copy, login/form,
contact, install, delegation or publication occurred.

[requests.jsonl](requests.jsonl) is the append-only reservation ledger. all28
response bodies and individual `.receipt.json` files retain URL/method, UTC,
status, headers without cookies, bytes/hash, request basis and redirect policy.
`01-published-link.json` through `09-published-link.json` bind the fresh anchor
and unchanged full heading to its saved landing hash. zero-byte302 bodies are
retained and counted. [collection-results.json](collection-results.json) records
all nine outcomes as `new_pdf`; there were **zero HTML-only, failed, known-URL
or PDF-hash duplicate outcomes**. collection stopped at its selected set and cap.

## extraction and integration boundary

[prepare_corpus.py](prepare_corpus.py) invokes existing reviewed
`scripts/extract_pdf_pages.py` read-only, using already installed Poppler26.06.0
`pdfinfo`/`pdftotext`. no package/system changes. method is **Poppler pdftotext
-layout embedded text; no OCR**, with all explicit reviewed per-page options.
all nine `*-pages.json` sidecars preserve complete consecutive pages, PDF hash,
tool versions/options, pdfinfo, commands and stderr. originals remain unchanged.
[extraction-report.json](extraction-report.json) binds extractor/tool hashes and
per-document page/text totals. no successful extraction warnings occurred.

[candidate-reference-entries.json](candidate-reference-entries.json) contains
**nine standard-shaped entry additions only**. it deliberately is not an
importable complete reference collection. each entry binds exact original PDF
URL, vendor tier, printed landing title, PDF/text sibling filenames/hashes,
page count and archive-only scope. `reference_documents.prepare` validated all
nine entry shapes together in temporary byte-identical sibling files; that
partial validation bundle was ephemeral and never imported or persisted.

root must independently review selection/provenance and corpus, then preserve
all existing61 complete-reference entries, append only accepted additions using
byte-identical sibling inputs, and validate/rebuild the complete combined
collection under the normal integration workflow. no `import_bundle` call,
manifest edit or candidate/live catalog replacement occurred here. text-search
hits remain document evidence, not interpreted device/manufacturer assertions.

## checks and continuation

[checks.json](checks.json), [tests.log](tests.log) and `SHA256SUMS` bind the actual
results. four offline collector fixtures pass: source denial saved once and
subsequent transport blocked; host/cap/prior-URL/robots guards before transport;
fresh-link + exactly one published redirect using no stale token; existing
reservation ledger cannot trigger an automatic resume/retry. all28 receipt body
hashes and fresh href/Location chains were independently checked. all9 original
PDF copies and sidecars validate, complete40-page list and prior/batch hash
novelty checks pass. prior discovery39 hashes remain unchanged.

live catalog remains
`0879a77f5d2cdf4edfdce3a88ab03034504c02f99b4645537f43af5db5c32853`:
5,157 devices / 7,302 records / 2,121 sources / 74,837 facts /72 documents.
manifest remains
`86f6179ff8dd490739936d5ab416e39e606380acf547ac009b1dc5b269dca81b`.
no full production suite or new imports are claimed for this research corpus.

repeat offline verification without changing artifacts, from project root:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 research/batches/discovery-2026-10-14/edgecore-bulk-documents/verify.py --check-only
```

next bounded step: root's independent review, followed by a separately owned
complete-bundle integration if accepted. `collect.py` refuses rerunning a started
batch. no further acquisition is authorized by unused resource slots; the28-HTTP
budget is exhausted. there are no live processes or pending request handles.
