# FCC collection coverage and local discovery gaps

`fcc-coverage` compares the FCC identifiers we already know about with the
records collected locally. It requires no network access and does not change
the database. This creates an actionable collection list without treating a
mirror's reported total as the complete FCC universe.

```sh
python3 -m devicehunt fcc-coverage --limit 0
python3 -m devicehunt fcc-coverage --vendor Accton --missing index --limit -1
python3 -m devicehunt fcc-coverage --vendor Celestica --missing detail --format json
python3 -m devicehunt fcc-coverage --missing pdf --limit -1 --format csv
```

The report separates four collection stages: index entries, captured detail
pages, detailed application records and archived PDF metadata. JSON retains
the original rows, dates, source references, capture failures and catalog
affiliation roles. CSV gives counts and collection states for each identifier.
`--limit 0` retains totals without rows; `--limit -1` returns everything.
The summary always covers all known identifiers for the chosen vendor, while
`matches` counts the chosen missing-stage filter before the output limit.

Identifiers enter the comparison through explicit `fcc_id` values with target
affiliations on the same catalog record, stored reviewed grantee codes in
indexes/detail pages, or target-associated detailed application records.
Arbitrary model text, FCC-looking facts and `related_fcc_id` values do not seed
it. A target association on a different record for the same model cannot lend
its company to an identifier. Seller claims retain their source tier and role.
An Accton-associated product may have another company's FCC grantee; association
is not a statement that Accton owns that grantee code or made that unit.

Comparison trims outer whitespace and ignores case but preserves all punctuation
and suffixes. Consequently these totals count distinct identifier strings,
not an independently deduplicated set of legal authorizations. Hyphen variants
remain separate pending evidence. Multiple companies can be associated with one
ID, so per-company totals must not be summed as distinct IDs.

## Snapshot result, 2026-09-27

Across all five target companies there are 271 known identifier strings:

| Local collection stage | Strings with collected records | Strings with no matching collected record |
|---|---:|---:|
| Discovery index | 210 | 61 |
| Captured detail page | 171 | 100 |
| Detailed application record | 2 | 269 |
| Archived FCC-linked PDF metadata | 1 | 270 |

Two [Amazon company rows](fcc-amazon-company.md) add discovery-index coverage
for CubScout, without changing detail/application/PDF counts.

The one PDF-linked ID has eleven archived documents. The separate reference
PDF collection is not automatically assigned FCC IDs merely because its text
mentions them. None of these columns measures whether the full filing or all
exhibits have been captured.

The new [Accton company index](fcc-accton-company.md) adds index coverage for
HEDML16035, HEDML2560 and HEDML60LW; HEDMR3202A remains missing from the index.
Earlier detail probes for HEDML2560, HEDML60LW and HEDMR3202A returned not-found pages in
FCCID.co. Their original HTML and failed-attempt rows are now retained;
see [probe results](fcc-gap-probes.md). The table now holds 174 detail attempts,
of which 171 are captured pages, leaving the collected-stage totals unchanged.
Their catalog source records are included in the JSON report for follow-up.
Other missing strings may represent spelling variants or weaker source claims;
the report does not promote them into authorization facts.

Seven saved JSON/CSV report pairs under `data/exports/fcc-coverage-*` cover all
companies, missing-index entries, and each target separately. The read-only
audit records database SHA-256. The database contents and owned baseline are
unchanged by this feature.

## Limits and checks

`not_collected` means no matching local row at a stage whose table is loaded.
If a table is absent, the state is `not_loaded`; unknown coverage is not counted
as a known gap by `--missing`. Unavailable detail attempts retain their reason
and count as attempts, not captured pages. Duplicate mirrors remain separate
source rows. An empty optional dataset does not prove a company has no filings.

This is a metadata audit, not a filesystem integrity check or download command.
It does not fetch URLs, retry denied requests, verify grants, infer models or
add manufacturer claims. It leaves the caller's existing database transaction
intact and uses a consistent read transaction when it owns one.

Tests cover stage distinctions, failed downloads, duplicate mirrors, source
roles, same-record vendor association, identifiers from application-only records,
literal suffixes and punctuation, missing tables, limits, transactions and
read-only CLI JSON/CSV output. Live totals are independently checked against SQL.

A new attempt to download the public
[Electric Garden Accton index](https://electric.garden/accton-hed) returned HTTP
403. It was stopped without retry or alternate download. No records from that
index were imported; the new report uses the existing local evidence only.
