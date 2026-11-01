# exact qct t42s-2u handoff

bounded research complete: 3/3 queries, 1/1 new primary capture. two proposed
records for the existing exact `QCT QuantaPlex T42S-2U` identity, 14 practical
facts plus 8 provenance/scope facts. coordinator owns acceptance/integration;
no database, manifests, shared exports, code, sources or current docs changed.

## primary capture and scope

`data/raw/smartnic-qct-2026-10-04/t42s/qct-product-portfolio-v14-2023.pdf`
is the complete 49-page original QCT portfolio, HTTP 200, SHA-256
`f6506bba15899678bac2b676a3599fb3fd81391d6b80b57ded12c78695d7910a`.
Original/final URL, UTC capture timestamp, bytes and method are in the sibling
`fetch-report.json`. The full embedded-text envelope binds the PDF hash and
pypdf/no-OCR method with all 49 sequential pages; metadata creation/modification
is 2023-03-31. Filename/URL/document dates are not a selected release date.

Selected exact table is **PDF page 43 / printed page 42**, bottom right under
QuantaPlex T42S-2U. The earlier search snippet's page 42 is printed numbering.
Visually checked `portfolio-p43.png`; adjacent T43Z and S24P tables excluded.
Seven practical facts: two Xeon Scalable CPUs/node up to 165W; C621/C624;
16 2666 MHz DDR4 RDIMM/LRDIMM/node; chassis 24 SFF or 12 LFF/SFF SATA/SAS
alternatives; OCP 2.0/PHY and dedicated GbE management/node; four specified
PCIe/M.2 expansion choices/node; 2U/four nodes. Storage counts are not labelled
per-node and remain chassis choices. No T42D/T42SP facts or inferred aliases.

The second record is additional extraction from **existing** QxVDI PDF
`data/raw/qct-qxvdi-2020-2026-10-04.pdf`, SHA-256
`32fcf545dc4fc67b8182bd0ae54138ce48cb4c7130e40d3315b914718ea4baed`,
not independent corroboration or a new capture. PDF 9-10 / printed 5-6 were
rendered and visually inspected. Its dated Version 2.0/copyright 2020-2021
solution describes two shared power supplies and literally “consumes 1600 watt
for each power supply”; this is retained as a marketing consumption statement,
not inferred rating, measured draw or a platform-wide PSU option. Solution-scoped
facts cover two CPUs/node, six SATA/SAS disks/node, up to 512GB RDIMM/node,
2666 MHz, SATA/PCIe M.2 boot support and 2U/four-node form factor. No new Quanta
parent-portfolio record is proposed. Brand/platform_vendor only on both records.

## review and integration contract

- `data/raw/smartnic-qct-2026-10-04/t42s/candidate-curated.json`: unique source
  IDs, exact prefixed model, literal per-field quotes, PDF/printed pages, table
  locations, visual review and hash provenance. All source paths resolve **relative
  to this candidate file**; independently verified in `verification.json`.
- If copying candidate to `data/`, rebase new snapshot to
  `raw/smartnic-qct-2026-10-04/t42s/qct-product-portfolio-v14-2023.pdf` and existing
  snapshot to `raw/qct-qxvdi-2020-2026-10-04.pdf`. Current candidate's existing
  source snapshot is `../../qct-qxvdi-2020-2026-10-04.pdf`, intentionally relative
  to its assigned raw directory. Do not copy without rebasing.
- `reference-pdf-proposal.json` is **one entry proposal, not a full collection**.
  Integrator must append to the COMPLETE current collection, preserving every
  prior entry. Copy this new PDF and complete text envelope byte-identically as
  sibling filenames beside the complete collection bundle. Existing QxVDI already
  belongs to the archive; no duplicate reference entry is proposed.
- `fact-proof.json`, `hash-manifest.json`, `search-ledger.json`, `verification.json`
  bind exact captures, selected fields and scope. Full primary source captured;
  only the assigned model table extracted for curated records.
- No imports or tests run by this researcher. Local checks verified full page
  envelope, PDF hash, reused PDF hash, and both candidate snapshot paths.

## remaining gaps and exclusions

No primary dimensions, fan counts/airflow, input voltage, derating, PSU alternatives,
current firmware access, software entitlement or installed-unit condition found
within this pass. Secondary search leads (Hyperscalers -HS datasheet, ServerStore
combined T42S/T42SP guide, Exalit portfolio mirror) were not captured or promoted.
Known Broadwell index/European-office press 403 resources were not requested;
T41S reseller source remained excluded. No outreach, paid access or gated download.
Query/capture budget is exhausted; stop here. Any further pass needs a distinct
primary lead, not more WSSD copies or family inference.
