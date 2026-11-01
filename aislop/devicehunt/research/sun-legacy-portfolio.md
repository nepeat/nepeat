# Legacy Sun approvals and Oracle portfolio association

The official RRA manufacturer query `Sun Microsystems`, with dates
`19900101`–`20260926` and all other filters blank, returned 40 approvals.
Original detail pages, both CSV exports and their hashes are stored in
`data/raw/rra-sun-official/`. The query was collected on 2026-09-27 UTC.
All rows are retained in `rra_equipment`, including unfamiliar legal names and
model fields left unparsed. This is query-level coverage, not a complete Sun or
Korean equipment inventory.

## Exact fields despite changing row order

The initial capture was rejected because two same-date approvals exchanged
positions between CSV exports. Certificates `E-C012-01-3045` (TYPE 6 USB) and
`E-C011-01-3046` (Crossbow USB) swapped generated row numbers 17 and 18.
Every other field was unchanged. Both original files and the failed candidate
are preserved; no network retry was needed.

This bundle explicitly opts into `csv_comparison: exact_rows_ignore_order`.
Validation checks consecutive row numbers and unique certificates separately
in each export, then requires every field after the generated row number to
agree exactly for each certificate. Missing/extra certificates, changed cells,
duplicate certificates and malformed numbering are rejected. The original
before-export ordering supplies raw registry row numbers and detail-page order.
No intermediate unquoted CSV cells are interpreted or repaired.

Byte equality remains the default for existing bundles. Future captures can
explicitly request this mode with `--allow-row-reordering`; it never permits
changing model, applicant, country, date, status or certificate fields.

## Corporate lineage and manufacturer are separate

Oracle's [January 2010 Form 8-K, item 2.01](https://www.sec.gov/Archives/edgar/data/1341439/000119312510015241/d8k.htm)
states that Sun Microsystems, Inc. became its wholly owned subsidiary on
January 26, 2010. Item 8.01 dates the public announcement January 27. The dates
are stored separately. The public web-tool text was successfully reviewed;
direct HTML retrieval subsequently returned HTTP 403 and was not retried.
The hashed company evidence is a selected factual extract, not original HTML.

The importer accepts whole manufacturer names Sun Microsystems, Inc. and the
reviewed short form Sun Microsystems. It does not map Scotland entities,
Computer Corp., Company, Ltd. or composite cells merely because they contain
the same text. Ten approval rows retain those unresolved manufacturer names.

Thirty matched manufacturer rows yield 29 model assertions across 26 distinct
catalog identities. Sun Microsystems is their brand/identity namespace and
the original manufacturer cell remains a `registry_manufacturer` affiliation
with no Oracle target. Oracle Corporation has the separate
`acquired_company_portfolio` role. These records appear under Oracle with
`--scope all`, not Oracle manufacturer scope. The relationship does not imply
Oracle manufactured a pre-acquisition unit or owned Sun when an older approval
was issued. It also establishes no AWS deployment, current certification,
software support, or auction-unit provenance.

Original model names and dates remain unchanged. For example, `ULTRA60` and
`ULTRA 60` remain distinct source spellings; neither is silently merged with
the other or with Oracle's differently named catalog records. Numeric-only
models and components are not automatically classified as servers.

## Local use

```sh
python3 -m devicehunt import rra-sun-portfolio data/raw/rra-sun-official/bundle.json
python3 -m devicehunt search --vendor Oracle --role acquired_company_portfolio --limit -1
python3 -m devicehunt lookup 'Netra ct 1600'
python3 -m devicehunt vendor-inventory Oracle --scope all --limit -1
```

```sql
SELECT d.brand, d.model, a.company, a.role, r.source_url
FROM records r JOIN devices d ON d.id=r.device_id
JOIN affiliations a ON a.record_id=r.id
WHERE r.source_id LIKE 'rra-sun-portfolio-%'
ORDER BY d.model, a.role;
```

The dedicated importer validates both acquisition evidence and the complete
query before replacing either source. It preserves a separate raw-query source
and portfolio source. Tests verify row-order comparison, rejection of changed
data, whole-company selection, historical manufacturer/portfolio role isolation,
refresh withdrawal and invalid lineage rejection before mutation. Both source
snapshots replay offline from the manifest.

Exports: `data/exports/sun-legacy-portfolio.csv` and `.json`,
`rra-sun-audit.json`, `lookup-sun-netra.json` and `lookup-sun-blade.json`.
Your ten owned labels are unchanged and both exact-label gaps remain open.
