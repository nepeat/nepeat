# Reviewed Celestica legal names in Korean approvals

Current capture: 151 raw approvals, 105 eligible manufacturer rows and 149
model assertions. The [main-model list parser](rra-main-model-lists.md) adds 28
assertions after the historical-exhibit expansion described below. Counts in
the individual expansion sections are historical.
The separate [reviewed manufacturer-list extraction](rra-manufacturer-lists.md)
adds 37 further assertions from composite cells in this same capture; it does
not change the default whole-cell mapping or supply independent corroboration.

## Initial company-name expansion

The Celestica RRA capture originally retained 151 approval rows but mapped only
48 manufacturer cells to the target company. Reviewing legal-name spellings now
makes 41 additional rows eligible, yielding 42 additional model assertions and
39 new catalog identities. The same capture now supplies 100 model assertions
from 89 eligible manufacturer rows; 62 rows remain raw searchable evidence.

## Evidence and interpretations

Celestica's [2025 filing, Exhibit 21.1](https://celestica.gcs-web.com/node/17171/html)
lists Celestica LLC, Celestica (Thailand) Limited and Celestica Holdings Pte
Limited among its subsidiaries. The reporting date is December 31, 2025.
The list is not a full subsidiary register or corporate ownership history.

The public web-tool text was successfully read and reviewed on September 26,
2026. A subsequent direct HTML download returned HTTP 403; it was not retried
or accessed through another route. Only the already retrieved factual names are
cached in `data/raw/rra-celestica-official/celestica-subsidiaries-2025.json`.
This is explicitly a **reviewed text extract, not original HTML**. The original
RRA detail pages remain independently cached with their hashes.

| Additional company spelling after punctuation normalization | Interpretation |
|---|---|
| Celestica LLC | Exact legal name from the filing |
| Celestica Thailand Ltd | Reviewed abbreviation of Celestica (Thailand) Limited |
| Celestica Holdings Pte Ltd | Reviewed abbreviation of Celestica Holdings Pte Limited |
| Celestica Holdings Pte Limited | Exact legal name from the filing |

Case, spacing and punctuation normalization cover the observed whole-cell
spellings, including `Celestica,Thailand Ltd`. The Ltd/Limited correspondence is
a documented interpretation, not a claim that the filing prints every spelling.
These aliases are scoped to an RRA bundle with the checked optional company
evidence dependency. Global company aliases and other importers are unchanged.

The raw manufacturer/applicant values are preserved. New record facts include
the canonical legal name, mapping basis, evidence URL, extract hash, capture
method and reporting date. An invalid/missing dependency cannot replace existing
records. Removing the optional evidence and reimporting removes its added claims.
The original query bundle is retained as `bundle-original-query.json`.

Composite companies are not split. Korean text cannot disappear into an English
target match, and an unfamiliar company is not mapped merely because it contains
"Celestica". The later filing does not establish parent ownership at every
historical approval date. Regulatory manufacturer claims do not establish a
physical factory, retail brand, operator deployment or auction-unit provenance.

## Useful examples

- DS4100 and DS5000 now have explicit RRA manufacturer claims for Celestica LLC.
- SPARC Enterprise T1000 and T2000 appear as separately listed derived models,
  with Celestica Holdings as manufacturer and Oracle America as applicant.
  Their main-model lists now also supply literal entries; no aliases are inferred.
- Cisco, Polycom and Spectralink-associated equipment appears under the
  Celestica manufacturer namespace with original applicant names retained.

```sh
python3 -m devicehunt vendor-inventory Celestica --limit -1
python3 -m devicehunt lookup 'SPARC Enterprise T1000'
python3 -m devicehunt search DS4100 --vendor Celestica --role registry_manufacturer
python3 -m devicehunt sql "SELECT d.model,f.name,f.value FROM records r JOIN devices d ON d.id=r.device_id JOIN facts f ON f.record_id=r.id WHERE f.name LIKE 'manufacturer_company_%' AND d.model='DS4100' ORDER BY f.name"
```

The detailed audit is `data/exports/rra-celestica-company-review.json`.
The compact `data/exports/celestica-manufacturer-additions.csv` (also JSON)
contains all 42 added assertions across 41 identities with original approval
links, company names and mapping interpretations.
Celestica manufacturer scope now has 106 identities, backed by 150 source
records across seven sources. Across all five targets there are 1,690
manufacturer-claim identities. The complete catalog has 3,295 identities,
4,305 assertions and 172 sources. Counts include revisions, components and
regulatory/commercial codes that may represent overlapping physical hardware.

The exact owned-hardware baseline remains 10/10 searchable and 8/10 corroborated.
No baseline label or operator claim was changed. A separate audit of every
mapped affiliation found no remaining company cell containing non-ASCII letters
or digits; that check is recorded in `data/exports/company-name-unicode-audit.json`
and is not a full semantic audit of corporate relationships.

All 273 tests pass. A fresh rebuild from 71 import jobs and 1,807 hashed files
matches the live database across 31 tables/search indexes and the baseline.
Tests verify that company mappings require their optional checked evidence,
remain scoped to the bundle, preserve raw names and provenance, reject composite
cells, fail before replacement on corrupt evidence, and can be withdrawn by
reimporting without the optional dependency.
All 36 saved lookup reports, vendor inventories and the offline catalog are
refreshed. Read-only export generation preserved the live database byte hash.

## Historical legal-name expansion — September 27, 2026

Two additional reviewed exhibits support five more whole legal names:

| Legal entity | Exhibit |
|---|---|
| Celestica (Dongguan-SSL) Technology Limited | SEC Exhibit 8.1 |
| Celestica Hong Kong Limited | SEC Exhibit 8.1 |
| Celestica (Suzhou) Technology Co. Ltd | SEC Exhibit 8.1 |
| Dongguan Celestica Electronics, Ltd. | Archived Exhibit 21.1 |
| Celestica Kladno, s.r.o. | Archived Exhibit 21.1 |

The [SEC exhibit](https://www.sec.gov/Archives/edgar/data/1030894/000162828018003056/exhibit81.htm)
was reviewed through the web tool. The
[archived corporate exhibit](https://corporate.celestica.com/static-files/aae92c39-4634-4e66-805e-6fb4adca1376)
was available as factual search text; opening the original PDF returned 403 and
was not retried. Both captures explicitly distinguish extracted facts from an
original document. Reporting periods were not established from these excerpts.
They support historical legal-name associations, not current ownership or
ownership at an approval's date.

The compound company-evidence bundle hashes these two extracts and retains the
existing 2025 dependency. Only reviewed whole-cell spellings map, including
explicit Ltd/Limited abbreviations. Raw names, legal names, source URLs, hashes,
capture methods, exhibit numbers and unverified-period limitations accompany
each affected record. Composite cells and unreviewed geographic qualifiers stay
unmapped. Global aliases and model matching rules are unchanged.

Sixteen more manufacturer rows are eligible, adding 21 model assertions and
21 identities. Examples include Communicator C100S, MAX V2, Jabra STONE and
WFM-E2.0. The later main-list parser also imports the literal entries `15938`
and `15939` from the Kladno main-model cell. `DS 4100` remains distinct
from `DS4100`. Forty-six manufacturer cells remain unmapped in this capture.
These are manufacturer-register associations, not proof of hyperscaler use.

At that historical-exhibit checkpoint, Celestica manufacturer inventory had
127 identities, 171 source records,
seven sources. The full catalog has 3,400 identities and 4,482 assertions across
186 sources. The audit is `data/exports/celestica-historical-import-audit.json`.
All 310 tests pass, including proof withdrawal, source replacement protection,
whole-cell boundaries and historical provenance. Baseline remains 10/10
searchable and 8/10 corroborated, including all four Oracle labels.
