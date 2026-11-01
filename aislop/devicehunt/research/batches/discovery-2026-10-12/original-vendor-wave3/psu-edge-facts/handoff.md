# two original-vendor family records — staged candidate

author `/root/handoff_audit`; outcome: **finite candidate ready for independent review**, not imported. only `psu-edge-facts/` written. inputs are the already independently reviewed SD6200 / Wedge PDFs, complete sidecars, discovery findings and visual notes. no network, imports, new identities, shared-state writes or scope expansion.

`candidate.json` SHA-256 **`0fbf35eb2c9e35364ca915cdba6c4622ae6cd7ae037e9955376576d38d7d03ab`**; reproduce with `PYTHONPATH=. python3 -B research/batches/discovery-2026-10-12/original-vendor-wave3/psu-edge-facts/build_candidate.py`. exact source copies and text sidecars are under `snapshots/`; bindings are in `provenance.json`, read-only checks in `checks.json`, current catalog observations in `catalog-context.json`.

## exact proposed rows

| new source id | existing namespace/model/id | facts | new source affiliation |
| --- | --- | --- | --- |
| celestica-sd6200-datasheet-2026-10-12 | Celestica / SD6200 / d1daa8e8c3ad705b1118 | 11 | Celestica Inc., documentation_vendor |
| edgecore-wedge100-32x-r04-2026-10-12 | Edge-Core / Wedge 100-32X / 9287d01bdc56adda4517 | 16 | Edgecore Networks Corporation, documentation_vendor |

totals: **two sources / two records / two existing identities / 27 facts**. zero identifiers/aliases, manufacturer/factory/hyperscaler affiliations, additional SKU/ORSA identities or proposed target aliases. kinds retain existing family identity classifications; new record titles are neutral datasheet descriptions, without borrowing another source's manufacturer wording.

the Wedge PDF heading prints **Wedge 100-32X**, exactly matching the existing spaced Edge-Core identity. the joined **WEDGE100-32X** product-index identity, Accton-spelled identities, BF/S variants and order-code identities remain separate. this candidate does not canonicalize case/spacing across different identities, transfer manufacturer roles from the existing Meta record, or modify that record.

## selected practical facts and exclusions

all 19 accepted discovery observations are retained with PDF page citations, plus six ordering-table literal facts and two explicit source-context/conflict facts. electrical/cooling/physical statements use the existing categorized names; ESM interfaces/configuration, revision/conflict and order rows remain distinct literal fact names.

- SD6200: up to 90 drives and 13 W maximum drive power at 35°C; dual redundant hot-swappable ESMs with single-ESM support; four 24G SAS4-capable mini-SAS HD ports per ESM. PSU 1300 W/80 plus Platinum with 2+2(200–240V) or3+1(100–240V) redundancy remains distinct from separately printed input 100–127/200–240V, 47–63 Hz. five 80 mm dual-rotor fans; H 175.3 / W 448 / D 1070 mm **with CMA**. operating/altitude derating, sound **power**<7.6 Bel at 23±2°C and SD6200/DS6200 footer conflict are preserved. no weight, PSU-model, airflow direction or installed-drive conclusion.
- Wedge: two AC or **-54 VDC** PSUs, 120–240 VAC/50–60Hz and 40–72 VDC input, separate 12 VDC option, typical 436 W remain source-scoped. page 1 AC/12 VDC PSU wording versus page 2 AC/-54 VDC plus 12 VDC option is recorded without resolving wiring/rating. W×D×H 44×50.7×4.4 cm, 8.8 kg, 4+1 fans/front-port→back-power airflow and environment conditions are family facts, not assigned to each ordering SKU.
- historical OpenBMC/Diagnostics-loaded and FBOSS/ONL/FlexSwitch/Cumulus Linux 3.4-and-later compatibility statements retain document identifier `EC-DS-0617-04` and ©2016 context, with publication date not independently verified. no claim of current support, downloadable software, license entitlement or software installed on a liquidated unit. no new unreviewed Facebook/deployment statement is selected from page 1.
- six full ordering codes and minimal descriptions are serialized as `ordering_row_as_printed` facts on the family record: `Wedge100-32X-O-12V-F`, full US/EU/UK/JP AC codes, and `ORSA-1U`. every row explicitly says classification unresolved. none is an alias/part-number identifier/new device class; the 12V short description does not establish a standalone PSU FRU, nor do regional cord descriptions settle complete-switch/component scope.
- Accton/Edgecore about-company text is `company_context_as_printed` only; no manufacturer/factory affiliation is proposed from generic class-wide prose.

## snapshots and checks

| copied original artifact | sha256 |
| --- | --- |
| sd6200-datasheet.pdf | 3a92bffdaafff4b4428c5ac1d4b6514c347154ca828b65639217837c4efa2f54 |
| sd6200-pages.json | 167e926937d22b6aa6ba4028773124000c73b76b21e104c7acf299d14a68b38f |
| wedge100-32x-r04.pdf | 66368b4e16367b335f27d53c6819d884e15e53390dbcc8e6528c39024fda4269 |
| wedge100-32x-r04-pages.json | aaf215bb1ee5bb9a67c881ca9cabea40d50d89a89b034ec518fb5d454af0609c |

checks passed: exact two existing anchor ids/namespaces/models and absent new source ids in the observed read-only transaction; source headings contain the exact chosen model spellings; four snapshot byte copies/hashes match originals; `documents.read_capture` validates all five consecutive pages with honest Poppler 26.06.0 metadata and hyphen-preserving options; one record per source; all facts cite PDF pages; all six exact ordering rows are present on PDF 3; identifiers empty; every affiliation is documentation_vendor. no importer execution or tests that write databases occurred.

catalog hash/read timestamps are deliberately observations, not a frozen acceptance baseline: coordinator may promote another batch concurrently. `catalog-context.json` records query time, hash start/finish, file-stat stability during hashing and anchor/source context. the sole integrator must recheck against its current accepted catalog/manifest before staging.

stop condition met. independent candidate review is required before the coordinator's ordinary complete-PDF-bundle/rebuild/old-data-preservation checks. source-scoped specs do not identify an auction unit or authorize brand merging, wiring recommendations, generic manufacturer-role transfer or additional order-code identities.
