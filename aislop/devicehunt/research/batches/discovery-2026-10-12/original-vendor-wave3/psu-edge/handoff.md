# original-vendor wave 3: SD6200 + Wedge 100-32X

owner `/root/psu`; **finite discovery complete, not imported**. six actual HTTP
exchanges of seven allowed, including two robots requests and a counted 302
redirect. zero searches, remote screenshots, denied-source retries, additional
sources, installs or delegation. only this task directory was written.

immutable raw capture ledger `requests.json` records each exact input, UTC start/
finish, status, headers, final response URL, byte count and SHA-256. automatic
redirects were disabled; `fetch.py` made one exchange per invocation with no
retry. both robots files are saved; `policy-checks.json` records allowance for
the declared `devicehunt-public-research/1.0` user agent. Celestica has separate
disallows for named crawlers including GPTBot; this collector used the named
research user agent and applicable wildcard policy. Edge-Core's published link
and its permitted same-origin redirect are separately bound in
`published-link.json` / `redirect-check.json`. no login was needed.

| request | exact source / outcome |
| --- | --- |
| 1 | `https://www.celestica.com/robots.txt`, 200 |
| 2 | `https://www.edge-core.com/robots.txt`, 200 |
| 3 | `https://www.celestica.com/uploadedFiles/Site/our-expertise/hardware-platform-solutions/storage/storage-list/cls_datasheet_SD6200.pdf`, 200 |
| 4 | `https://www.edge-core.com/download/wedge-100-32x-r04/`, 200 |
| 5 | landing's sole published `?wpdmdl=3223&refresh=6acda754b06ef1791862612` link, 302; zero-byte response saved |
| 6 | response Location `https://www.edge-core.com/wp-content/uploads/2024/01/Wedge-100-32X-R04.pdf`, 200 |

## raw PDFs and complete local extraction

- `sd6200-datasheet.pdf`: 642,770 bytes, two pages;
  SHA-256 `3a92bffdaafff4b4428c5ac1d4b6514c347154ca828b65639217837c4efa2f54`.
- `wedge100-32x-r04.pdf`: 281,885 bytes, three pages;
  SHA-256 `66368b4e16367b335f27d53c6819d884e15e53390dbcc8e6528c39024fda4269`.

reviewed `scripts/extract_pdf_pages.py` created **new** `sd6200-pages.json` and
`wedge100-32x-r04-pages.json`. all five pages, exact consecutive numbering,
PDF/sidecar hashes and strict metadata were checked with `read_capture`.
method is `Poppler pdftotext -layout embedded text; no OCR`; both tools
26.06.0, with explicit per-page `-layout -enc UTF-8 -eol unix
-remove-hyphens none -nopgbrk`. complete command/version/pdfinfo/raw warning
provenance is inside each sidecar. no pypdf label, OCR or code edit.
tools are installed outside PATH at
`/nix/store/wb8486c5l68f1shsbc217b7m4hbgrw3s-poppler-utils-26.06.0/bin/`.

four pages rendered in two local invocations, all visually reviewed:
`sd6200-1.png`, `sd6200-2.png`, `wedge100-2.png`, `wedge100-3.png`.
`render-provenance.json` binds tool/version, argv, source and image hashes.
`findings.json` preserves **19 finite factual observations plus six ordering
rows**, page references, absent fields and conflicts; it is discovery structure,
not an importer candidate. `checks.json` records completion checks.

## useful source-scoped findings

**SD6200**, p. 1: 4U standard 19-inch enclosure, up to 90 SAS/SATA drives;
13 W maximum drive power at 35°C. dual redundant hot-swappable ESMs and
single-ESM configuration supported; four 24G SAS4-capable mini-SAS HD ports
per ESM, RJ45 management and UART via 3.5 mm jack.

p. 2: 1300 W 80 plus Platinum PSU, **2+2 (200–240 V) or 3+1 (100–240 V)**
redundancy. separate input line reads **100–127, 200–240 V AC**, auto ranging,
47–63 Hz. retain the two voltage bands and the differently worded redundancy
range; do not silently rewrite as continuous universal input. five 80 mm
dual-rotor fans are hot-pluggable from the top. height175.3 / width448 /
depth1070 mm **with CMA**; operating5–35°C, altitude0–950 m then temperature
derating1°C per300 m. sound **power** <7.6 Bel at23±2°C, not sound pressure.
no chassis ordering table, PSU manufacturer/model, weight or airflow direction
is supplied. copyright2024 and literal `091024` preserved; the trademark footer
says **DS6200** while both product headings say **SD6200**. no alias inferred.

**Wedge 100-32X**, p. 2: two redundant/load-sharing/hot-swappable **AC or
-54 VDC** PSUs; input120–240 VAC50–60 Hz /40–72 VDC. a separate line supplies
the **12 VDC power input option**. p. 1/previous saved HTML describe AC or
12 VDC PSUs; preserve these option statements, not a wiring or revision fix.
typical power436 W has no load/test condition in that section and is not a
maximum/PSU output. dimensions **W×D×H44×50.7×4.4 cm**, weight8.8 kg;
4+1 fans, front(port)→back(power); operating0–45°C. OpenBMC/Diagnostics
and named NOS compatibility are historical vendor statements, not current
support, download entitlement or an auction unit's installed software.

p. 3 exact ordering rows, visually checked:

| printed code | printed description |
| --- | --- |
| `Wedge100-32X-O-12V-F` | 12V power supply |
| `Wedge100-32X-O-AC-F-US` | US SKU power cord of Wedge |
| `Wedge100-32X-O-AC-F-EU` | EU SKU power cord of Wedge |
| `Wedge100-32X-O-AC-F-UK` | UK SKU power cord of Wedge |
| `Wedge100-32X-O-AC-F-JP` | JP SKU power cord of Wedge |
| `ORSA-1U` | Open Rack Switch Adapter for Open Rack installation |

retain these whole codes and their minimal printed descriptions. no general
`-O-`/`-F-` decoder, bare AC code, exact PSU FRU/model or per-code input/airflow
allocation is supplied. footer `EC-DS-0617-04`, copyright2016; uploaded path
2024/01 is not proof of publication/revision date.
the full `Wedge100-32X-O-12V-F` ordering code's short description does not
establish that it is a standalone PSU FRU; no component-kind assignment follows.

p. 2's about-company text makes a generic Accton-developed/manufactured
Edgecore Data Center switch claim and subsidiary statement; it is not
independent physical-factory evidence or an automatic new per-model
manufacturer affiliation. keep it contextual pending separate role review.
p. 1 Facebook/Wedge relationship text
is captured/extracted but not visually reviewed in this four-page render pass;
no deployment assertion is proposed from it. no hyperscaler/operator,
auction-unit history, current certification/stock/support, safe wiring or
standalone operation follows from this discovery handoff.

stop condition reached: both finite PDFs captured, complete local sidecars,
four visual pages, useful exact findings and absences. one unused request
remains unused. no catalog, manifest, archive, export or shared-code writes.
coordinator may independently review originals/pages before selecting any
candidate and complete-PDF-bundle integration.
