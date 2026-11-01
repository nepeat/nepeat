# AS8000 R02 + SC6100 finite discovery

owner `/root/psu`; **complete discovery, no candidate/import**. four actual
HTTP exchanges within seven, zero searches/remote screenshots/retries.
both named original PDFs captured; fourteen PDF pages completely extracted;
six targeted pages rendered and visually checked. only this directory written.

## source route, originals and policy

`source-basis.json` binds exact saved product-page hrefs/hashes, failure-note
inputs and the reused robots bytes/previous request metadata. no product/index
page recapture. `requests.json` records every new GET input, UTC start/finish,
response/final URL/status/headers/body size/SHA-256. redirects were disabled;
the single302 and its zero-byte body are explicitly counted and retained.

| exchange | outcome |
| --- | --- |
| 1 | `https://www.edge-core.com/download/as8000-quick-start-guide-r02/`, 200; one published download anchor |
| 2 | exact saved `https://www.celestica.com/uploadedFiles/Site/our-expertise/hardware-platform-solutions/storage/storage-list/cls_datasheet_SC6100.pdf`, 200 |
| 3 | landing's fresh `?wpdmdl=632&refresh=6acdaed8154b81791864536` link, 302 |
| 4 | Location `https://www.edge-core.com/wp-content/uploads/2023/08/AS8000-Quick-Start-Guide-R02.pdf`, 200 |

`published-link.json` binds the sole anchor to the landing hash; request4 exactly
matches request3 Location. no stale token, guessed filename, alternate route,
login or denial/throttle encountered. source helper `fetch.py` permits one
exchange per invocation, disables redirects/retries and enforces seven-cap,
repeated-URL and denied/throttled-origin stops.

reused policies, byte-identical to wave3 originals, are saved locally as
`robots-edgecore.txt` / `robots-celestica.txt`; fetched during the same session
at03:36UTC, provenance in `source-basis.json`. **each new exact request URL** is
checked against saved policy, with path allowance and policy SHA recorded in
its request entry. declared agent `devicehunt-public-research/1.0` is permitted
by applicable policy and has no crawl delay. Celestica's separate named-crawler
disallows, including GPTBot, remain preserved; this collector used its declared
research agent and wildcard policy, not a universal allowance claim.

| original PDF | bytes / pages | SHA-256 |
| --- | --- | --- |
| as8000-r02-qsg.pdf | 2,637,838 / 12 | `0cc5631b82ffd5e57c2750baea33f4203a1714d633bcccb73f6ebd4ff473fdcd` |
| sc6100-datasheet.pdf | 618,920 / 2 | `5dc67c8d2976c3dfe36ef813b919d0000f0b5f6bf1eb034d04040cae3254ddba` |

## complete extraction and finite findings

reviewed `scripts/extract_pdf_pages.py` created fresh `as8000-pages.json` and
`sc6100-pages.json`, using existing Poppler26.06.0 at
`/nix/store/wb8486c5l68f1shsbc217b7m4hbgrw3s-poppler-utils-26.06.0/bin/`.
method **Poppler pdftotext -layout embedded text; no OCR**; per-page explicit
`-layout -enc UTF-8 -eol unix -remove-hyphens none -nopgbrk`.
all12+2 consecutive pages and strict PDF-hash/method/options metadata validated
with `read_capture`; original hashes unchanged. full subprocess/version/pdfinfo/
warning provenance retained inside sidecars; invocation/script hashes in
`checks.json`. no package install, OCR, production code or old capture change.

six selected PNGs, all viewed: **`as8000-01.png` through `as8000-04.png`**,
**`sc6100-1.png`, `sc6100-2.png`**. two render invocations, six pages; complete
source/tool/version/argv/image hash binding in `render-provenance.json`.
`findings.json` has **26 observations** (14AS8000,12SC6100), page pointers,
source scopes, absences and unresolved wording. no importable candidate built.

**Minipack-AS8000**, English physicalPDFp1–4:

- p1 exact heading128-Port100G Chassis Switch; literal footer
  `E082019-CS-R02`, `150200002142A`. 4RU, 19-inch rack, separately ordered
  L-shaped rails; screw/tool-less options and29-inch-deep-rack adapters.
- p2 whole chassis up to four AC PSUs; source says two permit operation,
  three/four provide redundancy. at least two separate AC circuits for
  redundancy. chassis mounting handles/screws and rack air baffles described.
  SCM management port and separate PIM named; no standalone-module identity
  or bootability inferred from complete-system statements.
- p3 module/backplane release/latching steps retain network-disconnect/ESD
  conditions. eight fan trays as documented shipping configuration; trays
  hot-swappable, no empty fan-tray slot, prompt replacement. PSU service text
  describes continuing with three installed supplies and **replacement within
  ten minutes**; not silently combined with general two-PSU operation wording.
- p4 W×D×H440.4×755.5×176.2mm,4RU; **55kg with1SCM,8PIMs,8fan trays,4PSUs**,
  not empty-chassis weight. maximum power consumption3000W is not PSU output.
  rating lines remain separate:100–120VAC50/60Hz10AperPS×4;
  200–240VAC50/60Hz7AperPS×4;200–277VAC50/60Hz9AperPS×4 **except Bel Fuse
  and Murata power supply**. no universal277V/PSU-model compatibility inferred.
  operating0–45°C, altitude≤1829m **at0–35°C**; operating humidity5–85%
  non-condensing; storage-40–70°C retained as storage scope.

no full chassis/module/PSU/fan/rail ordering-code table, exact PSU rated output/
manufacturer-model pairing, airflow direction or software-access procedure
found in selected English operating pages. chassis, SCM, PIM, tray and PSU
remain separate scopes; no current-unit operation/fitment or safe-wiring claim.

**SC6100**, physicalPDFp1–2:

- 2U chassis/19-inch rack, two hot-swap computing nodes, up to24 dual-portU.2
  PCIeGen5 SSDs. platform and per-node processor/DIMM/interface declarations
  remain separately named, not installed inventory or a new node-model identity.
  BBU availability for node AC-loss power is optional, no runtime promise.
- p1 **two2400W Titanium80+ CRPS units,200–240VAC47–63Hz,1+1 redundant**;
  no4800W summed-capacity claim. fourteen40×56mm fans **hot-swapped with compute
  nodes**, seven per node; no independent live fan-removal procedure inferred.
  H87/W446/D874mm; no weight or airflow direction supplied.
- p2 operating5–35°C,8–80%RH, altitude0–950m then1°C derating per300m above950m.
  separate non-operating-40–60°C,8–95%RH, altitude0–12,000m. sound **power**
  ≤8.6Bels at23±2°C, not sound pressure.737.28TB topology statement explicitly
  uses30.72TBSSD×24; not installed/formatted usable capacity. copyright2024 and
  literal `092724` preserved, without inferring unit manufacture date.

top-level FEATURES slot text says low-profilex16 slots per node plusx8 slots,
while NODE FEATURES/topology use standardHHHL and2x16+2x8 wording. originals
preserved; **no slot-configuration fact chosen to reconcile scopes**.
no chassis/node/drive ordering code, exact PSU manufacturer/model, manufacturer/
factory/deployment/current-support claim or auction-unit configuration supplied.

## extraction-quality and language boundaries

AS8000p5–8 parallel Chinese section andp9–12 safety/compliance material are
retained in complete text but not visually reviewed in this six-render pass;
no new claims selected from them. p12 restricted-substance table has visibly
garbled font mappings/control characters in embedded text; **preserved verbatim**,
not silently repaired/OCRed or used as fact evidence. p5 warning's embedded
text `Minipack-AS80000` differs from `Minipack-AS8000` headings; do not create
an alias from that unreviewed warning spelling. no language-wide agreement
or whole-document visual review is asserted.

stop condition met: both named document sets, raw request/policy provenance,
complete honest sidecars, six visual pages and finite useful findings. three
HTTP allowances remain unused. no network expansion, import, catalog/manifest/
archive/export/shared-doc/code writes. independent discovery review precedes
any later candidate or identity routing; ordinary integration remains separate.
