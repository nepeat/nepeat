# Accton report-cover model evidence

Thirty selected report covers in the cached FCCID.co filing excerpts supply
38 model assertions. Each accepted cover has an explicit FCC ID, report number,
MODEL NO field and Accton APPLICANT field within the first 1,800 characters.
The capture records the exact fields, excerpt ordinal, excerpt hash and original
HTML hash. Import validates those values against the cached page before writing.

This is **regulatory-applicant evidence**, not physical-manufacturer evidence.
Accton is used as the identity namespace because it is the named applicant; it
is not an inferred retail brand. Rebadged-looking names stay verbatim. Models in
comma-separated fields remain separate identities, never aliases. A repeated
filing/report cover is counted once per model; the same model in different
reports retains separate source assertions.

The source format is cached HTML mirror text, often a truncated excerpt. Eleven
original PDF links were attempted and returned HTTP 403. An independently linked
FCC.report copy of Oracle report 4621145 also returned 403. Those attempts are
recorded under `data/raw/fcc-identity-reports/`; no PDF bytes were acquired or
claimed. Manufacturer/factory statements seen in those excerpts remain discovery
leads. They are not promoted into manufacturer affiliations by this importer.

## Accepted scope

The accepted layout is a narrow report-cover sequence: FCC TEST REPORT,
REPORT NO, MODEL NO, RECEIVED, with APPLICANT and ADDRESS fields. The FCC IDs
in the inspected prefix must match the filing. Model tokens contain letters,
digits, hyphens or periods and must contain a digit. Only commas split models.
This avoids treating nearby test-instrument tables as product identities.

Examples retained include `3CRWE820096A` (not the shorter FCC product code),
`SMC2552W-G`, `WA4001A-32`, `RBT3K-AG-G` and case-preserved `LR11b`.
`J8130A` appears in three distinct reports and is not merged with a different
model merely because the FCC IDs share a prefix.

Examples left for original-document review include `WN3302A 1`, `WN3306A 2`,
`SMC2632W V.2`, `HiveAP 20 ag`, fields with OEM subheadings, and a mixed field
containing `WG4005A 1-17`. Spaces, footnotes or OCR fragments are not silently
removed. `data/exports/fcc-cover-exclusions.json` lists unsupported excerpts;
that list is a review queue, not a declaration that those reports lack models.

## Queries and outputs

```sh
python3 -m devicehunt search 3CRWE820096A --vendor Accton --role regulatory_applicant
python3 -m devicehunt sql "SELECT d.model,a.company,a.role,r.source_url FROM devices d JOIN records r ON r.device_id=d.id JOIN affiliations a ON a.record_id=r.id WHERE r.source_id='fcc-accton-report-covers' ORDER BY d.model"
python3 -m devicehunt import fcc-cover-models data/raw/fcc-report-cover-models.json
```

`data/exports/fcc-report-cover-models.csv` contains model, applicant role, filing
reference and source URL. `show` also exposes the original report/model fields,
mirror excerpt ordinal, truncation status and format limitation. These records
never satisfy a manufacturer-only filter through this source.

Tests cover exact grouping, applicant-only roles, malformed or ambiguous model
fields, wrong FCC IDs/applicants, peripheral-table rejection, duplicate report
captures, checksum/source association, traversal protection and failed-refresh
preservation. The offline manifest hashes the capture and all referenced HTML
files already included by the filing-detail import.
