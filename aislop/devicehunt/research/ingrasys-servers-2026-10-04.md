# Ingrasys server and storage evidence — 2026-10-04

Follow-up: the [server-evidence milestone](server-evidence-batch-2026-10-04.md)
recovered ten image-only catalog pages through a documented EOF reconstruction.
Eight exact A/I variant records are now imported, including six new identities
and separate Foxconn portfolio records. The original remains malformed; the four
unsuffixed models still lack specifications. The account below preserves this
earlier batch and its then-current findings, not the current recovery status.

This bounded batch adds eight exact Ingrasys platform names. Ingrasys remains
a separate brand; it is **not a global Foxconn alias**. Product claims and the
dated parent-company association are separate source records.

| Model | Evidence | Useful details / limits |
| --- | --- | --- |
| SV1120 | DOCS index, Oct/2023 heading | Model name only; linked PDF response malformed |
| SV1121 | Same index heading | Model name only |
| SV2121 | Same index heading | Model name only |
| SV2221 | Same index heading | Unsuffixed name; not an alias for the following variants |
| SV2221A | Current AMD EPYC solution page | Two fifth-generation EPYC CPUs; 2 OU, liquid cooling, centralized 48 V busbar/PDB |
| SV2221I | Current Intel Xeon solution page | Two Xeon 6 CPUs; 2 OU, air or liquid cooling, centralized 48 V busbar/PDB |
| ES2000 | Two-page storage datasheet, 2024 footer | Marvell 98EX5630; 12 × 200GbE QSFP56 |
| ES2100 | Same datasheet and current storage page | NVIDIA Spectrum-2; 24 × 200GbE QSFP56 |

The two storage systems share a 2U chassis, 24 U.2/E3.S NVMe drive bays with
RapidFlex A2000 controllers per carrier, Atom C3538 management, ONIE/SONiC,
and 1+1 redundant 1600 W PSUs. The datasheet states 100–240 V AC input; it does
not specify output derating. These are vendor platform specifications, not
verified configurations, firmware access, completeness or provenance of auction units.
No named hyperscaler, FCC identifier, physical manufacturer or factory is assigned.

## Original sources and provenance

- [DOCS index](https://www.ingrasys.com/docs/): saved earlier at
  `data/raw/odm-models-2026-10-04/ingrasys-docs.html`. Its Oct/2023 and Jun/2024
  dates are index dates, not established product-release dates.
- [Server catalog link](https://www.ingrasys.com/docs-detail/sv1120-sv1121-sv2121-sv2221/):
  original HTTP 200 body saved with `.html` extension, actually malformed PDF
  bytes. It has a leading UTF-8 BOM, repeated PDF headers and a truncated tail.
  `pypdf` cannot resolve `/Pages`. No server specifications or document pages
  imported from this response; the index remains the evidence for four names.
- [AMD product page](https://www.ingrasys.com/solutions/amd/amd-epyc-processor-solution/)
  and [Intel product page](https://www.ingrasys.com/solutions/intel/intel-xeon-processor-solution/):
  original HTML captured successfully, exact A/I suffixes retained.
- [Storage datasheet](https://www.ingrasys.com/docs-detail/es2000-es2100/):
  original body saved at `data/raw/ingrasys-servers-2026-10-04/es2000-es2100.html`.
  A separately hashed derivative removes **only its three leading bytes EF BB BF**.
  Strict PDF parsing then succeeds; both pages were rendered and visually reviewed.
  The PDF is image-only, so its two archived page texts remain blank. Structured
  facts are explicitly manual visual transcriptions, not embedded text or OCR.
  The original bytes remain unchanged and their hash is recorded alongside the derivative.
- [Current ES2100 page](https://www.ingrasys.com/solutions/15/nvme-of_ebof_solution/):
  independently supports ES2100 specs; its current assertions do not replace
  the older shared datasheet's assertions.
- [Ingrasys company page](https://www.ingrasys.com/company/about/): captured
  2026-10-04. Names Foxconn Technology Group as parent. It does **not** name the
  eight models. `ingrasys-foxconn-portfolio-2026-10-04` is an explicit cross-source
  inference joining this company relationship with separate model evidence.
  Each portfolio record retains the raw parent name, observation date, both URLs
  and both input hashes. Canonical Foxconn is only a portfolio search label.
  No ownership-start date or manufacturer role is inferred.

The previously captured Foxconn 2023-12-25 press release remains available locally;
a new web check returned 403. This batch relies on the accessible current Ingrasys
company page. Do not retry that denied press URL through alternate routes.

New captures and outcomes: `data/raw/ingrasys-servers-2026-10-04/`.
Curated assertions: `data/ingrasys-facts-2026-10-04.json` (6 sources, 17 records).
Complete replacement reference collection:
`data/raw/reference-documents-with-ingrasys-2026-10-04.json`.
Never import a partial collection: the reference importer replaces that collection.

## Local queries

With `python3 -m devicehunt` (bundled Python path in PROGRESS):

```sh
python3 -m devicehunt search SV2221A --vendor Foxconn
python3 -m devicehunt search ES2000
python3 -m devicehunt specs ES2100
python3 -m devicehunt vendor-inventory Foxconn --scope all --limit -1
```

The eight additions appear in Foxconn's **all-association** inventory through
`parent_company_portfolio`; they add zero manufacturer claims. The command
`documents --source reference-pdf-ingrasys-es2000-es2100` lists its two archived
pages. `documents ES2000` cannot find their image-only text. Use curated facts
and the visually reviewed PDF together.

## Remaining gaps

Recover a valid original server catalog through a normally accessible source
before assigning specs to the four unsuffixed names. Exact orderable variants,
installed rack/power components, firmware access and historical deployment remain
unverified. ByteDance B4020/B4010 stay separate unresolved coverage examples.
