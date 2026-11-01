# QCT D52BQ-2U PSU CCL candidate

owner `/root/handoff_audit`, 2026-10-13 05:13 UTC. **staged, not imported; one source / one existing record / eight facts.** [candidate.json](candidate.json) SHA `9f2f76cfb228e18298bd24c5c33d87b588a17e556b424a5ef41a8ee95aacebfd`. independent candidate review and root integration remain separate. [checks](checks.json), [provenance](provenance.json), [row mapping](row-mapping.json), [read-only catalog checkpoint](catalog-context.json), [builder](build_candidate.py), [read-only builder proof](builder-check.log).

the authorized routing boundary follows the [accepted discovery review](../review/qct-compatibility-discovery-review.md). parent product HTML explicitly prints **QuantaGrid D52BQ-2U** and links the exact CCL URL; the existing `QCT / QuantaGrid D52BQ-2U` id is **f65a474568c484317fc5**. candidate brand/model/title are `QCT`, `QuantaGrid D52BQ-2U`, `QCT QuantaGrid D52BQ-2U`; kind retains existing `server platform`, revision remains empty. the sole affiliation is **Quanta Cloud Technology / documentation_vendor**, as printed. identifiers are empty. separate Quanta identity, bare-name identities, manufacturer/factory roles, aliases and standalone PSUs are not added or bridged.

source `qct-d52bq-2u-psu-ccl-2026-10-12` binds `https://www.qct.io/microsite/ccl/D52BQ-2U.html`. the original capture and copied `snapshots/qct-d52bq-ccl.html` are byte-identical, SHA **9c377013ee3bde686ae7372eb45aeadef00361b1ebe1f81a67bd872b5379d8b8**,303,591 bytes. auxiliary original product HTML and `snapshots/parent-product.html` are byte-identical, SHA **d5b0fd941c02b83842157ce5e9a4ad8e188fe5abe4bd6e9e07a57fe927061455**,111,372 bytes. the parent copy is a routing anchor for an already imported source claim, not a second source or new factual assertion. originals are untouched.

each `power_description` begins **QCT CCL PSU row**, includes printed system/date, all nine **labeled cells as literal JSON**, a concise historical/reference/disclaimer/quantity/input/240VDC scope and table12/row3–10 locator. labels/order are:

`Power Capacity | Type | Efficiency | Dimension(mm) | 240VDC | Cold redundancy | QuantaPN | VendorPN | Note`

all 72 cells, seven empty Note values, headers, original raw row text fragments/CRLF, row/cell attributes, table footer, full introduction/date/company/disclaimer and original source hash are retained per fact in raw `evidence`. CSS and all other source tables remain in untouched full HTML; fourteen other tables supply no candidate facts.

| HTML row | QuantaPN | VendorPN | capacity / efficiency | 240VDC |
| --- | --- | --- | --- | --- |
| 3 | AF800C00002 | MC800B6-3-4R-02 | 800W / Titanium | Yes |
| 4 | AF800C00030 | MPS8146 | 800W / Titanium | Yes |
| 5 | AF800B00009 | MC800B4-3-4R-02 | 800W / Platinum | Yes |
| 6 | AF800B00019 | MC800B4-3-5R-02 | 800W / Platinum | Yes |
| 7 | AFC00F00019 | MC1200B4-3-4R-02 | 1200W / Platinum | Yes |
| 8 | AFC00B00032 | PS-2122-7Q | 1200W / Platinum | Yes |
| 9 | AFB00E00003 | FSF048-930G | 1100W / Gold | Yes |
| 10 | AFB00E00005 | FSF048-931G | 1100W / Gold | No |

every row independently prints Type `Redundant`, Dimension(mm) `86*196*40`, Cold redundancy `Yes`; those remain table labels, without installed quantity, summed capacity, axis decoding or safe-operating instructions. QuantaPN/VendorPN remain separate fields and paired only within their original row. Efficiency labels create no independent certification assertion.240VDC Yes/No does not become an input range, AC rating, connector/pinout or standalone wiring claim.

FSF048-930G's Note alone reads `Regulatory only for USA/EU. Taiwan/China/Russia will implement on end of Q2 2018 .`; retain the historical schedule without claiming implementation/current certification. all selected rows use `form01` and cell `style4`; footer is `*Grayed-out items: to be EOL soon or already EOL`. no current EOL/stock status follows. publication **2025-06-27** is printed source date, while capture finished **2026-10-13T05:00:32.403114+00:00**. the CCL is explicitly reference-only, not a replacement for full QCT hardware certification, with no guarantee of third-party component quality/reliability; full original disclaimer remains raw evidence. no per-unit fitment/configuration or manufacture inference is made.

`python3 -B research/batches/discovery-2026-10-12/qct-compatibility-facts/build_candidate.py --check` passed without writes: re-parses both copied HTMLs, verifies pinned hashes/parent heading/href, exact existing identity digest,15 tables/892 rows, eight nine-cell PSU rows, row mapping and serialized candidate/provenance equality. checks also decode each fact's literal JSON back to its original labels/values and verify both original/copy byte pairs. no curated importer, temp database or network was used. current accepted DB SHA **fc88dca7e8b6f23aa8078134be767c22fed902303c2880ee923ad1d55952e3bf** remained unchanged during the dated author anchor queries; later integration may change it.

future integration must preserve the candidate's relative `snapshots/` paths and explicitly pin **three inputs**: candidate.json, the CCL HTML and auxiliary parent HTML. `provenance.future_manifest_dependencies` records this list and hashes; `source.evidence_dependencies` names the auxiliary input. the curated importer ignores that metadata field, so **the manifest files entry must enforce the parent dependency**, rather than assuming automatic enforcement. no manifest was written here and no PDF/reference bundle change is required for this HTML-only candidate.

only `qct-compatibility-facts/` was written; production code, captures, shared DB/manifest/archive/exports/current docs remain untouched. no new source facts outside these eight rows, installed-unit evidence, identity/role expansion, imports, rebuild, installation or delegation. stop condition met; root owns independent candidate acceptance, staging and integration.
