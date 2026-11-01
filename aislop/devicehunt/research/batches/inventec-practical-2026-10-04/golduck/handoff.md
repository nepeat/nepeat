# golduck handoff — 2026-10-04

status: bounded research complete; pending coordinator acceptance and independent review. no imports, shared manifest, live database, exports or shared docs changed.

one search query and one new original vendor PDF capture used. two queries remain unspent; no spare capture requested. saved three Inventec download-index pages and the original Dongguan article were inspected first; their fetch reports contain no Golduck denial. indexes have no exact Golduck match. this is a reviewed-corpus gap, not worldwide absence.

## accepted-input proposal

- raw original: `data/raw/inventec-practical-2026-10-04/golduck/inventec-golduck-v3-2020.pdf`, 515,103 bytes, two pages. SHA-256 `0488cdc161f08d86f3207b315fd087896c2f84871a2bf7b105f2a79edbc828dd`.
- source URL: https://ebg.inventec.com/en/tool-download/product_files/file/65/Inventec_Golduck_EN_PB_v3_2020?note=
- capture: 2026-10-05T00:03:45.086881+00:00 (2026-10-04 local); default-TLS curl, HTTP 200, no retry. `fetch-report.json` preserves exact outcome.
- `candidate-curated.json`: one exact Inventec Golduck record, 22 facts under unique `inventec-golduck-product-brief-v3-2020`; brand/platform_vendor roles only. existing dated manufacturer source stays separate. curated snapshot is relative to the candidate's own directory. if moving candidate to data root, integrator must adjust snapshot path and hash reviewed bytes.
- `inventec-golduck-v3-2020.pages.json`: full pypdf embedded-text envelope binding original PDF hash, extraction method and both ordered pages.
- `reference-pdf-proposal.json`: separately named single-document proposal with sibling PDF/text filenames and hashes. **never import this directly or replace the COMPLETE reference collection with it**. integrator must append the reviewed entry to a new complete collection and place byte-identical PDF/text siblings beside that collection.
- research `fact-provenance.json` supplies exact quotes, pages, printed table/paragraph locations and extracted line numbers. `page-1-text.txt` and `page-2-text.txt` bind those line numbers. `artifact-hashes.json` hashes proposed inputs.

## practical findings

page 2: 1U1N with slide rail; printed 435 × 43.2 × 800 mm (17.3 × 1.7 × 31.5 inch); dual AMD EPYC 7002, TDP up to 165W; 32 DDR4 DIMM slots; front up to twelve 2.5-inch U.2 NVMe/SATA SSDs and two internal M.2 SATA sockets; rear two PCIe Gen4 x16 FHHL and one Gen4 x16 OCP 3.0; OCP 10/25/100/200GbE options; direct CPU storage control; IPMI 2.0 + KVM with dedicated LAN; optional TPM2.0. power is **1+1, 800W/1300W, 100–220VAC & 240V HVDC, Platinum** as printed. cooling is **N+1, eight 4056 dual-rotor fans**.

page 1: up to 4TB using 128GB DIMMs, up to 128 cores and sixteen memory channels per single computing node; tool-less storage drives and hot-swappable fan modules. page 2 prints 162 available PCIe lanes and AMD Secure Root-of-Trust, Secure Run and Secure Move. these remain source statements without correcting unusual values.

both PDF pages rendered and visually inspected; printed table cells agree with extraction. `reference_documents.prepare` and `documents.read_pages` pass on the proposal (two pages). all sixteen literal fact quotes match full page text. no source PDF was edited.

## limits and exclusions

copyright 2020; PDF CreationDate 2020-07-27 16:33:05 +08 and ModDate 16:33:08 +08. URL name v3_2020 is document naming, not a release date. a search-only October 29 release article surfaced but was not captured/imported; its search snippet claims 4TB per processor, while this reviewed brief states up to 4TB with 128GB DIMMs. do not merge those capacities or treat the search snippet as corroboration.

no verified OS matrix, BIOS/BMC package availability, licensing, firmware update procedure, boot compatibility, current support, rack rail part number/inclusion, PSU derating by input voltage, acoustic data, power consumption, auction-unit configuration, operator/deployment or factory identity. 800W/1300W are alternatives, not additive consumption. table input range is retained literally without safety interpretation. printed metric/imperial width mismatch is retained. no alias to Horsea or another Inventec model inferred.

next: coordinator/source reviewer accept exact fact scope; append proposals through the existing complete-bundle workflow only if accepted. parent coordinator owns documentation.
