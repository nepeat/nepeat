# broad ODM/vendor source extraction

Private offline discovery dataset, authored by `/root/odm_bulk_full` after exclusive
ownership transfer from stopped `/root/practical_sources`. All network collection
was already complete. Shared catalog, manifests, exports and production code were
read-only. Root owns integration and current project documentation.

## outputs and scope

| capture / dataset | extracted scope |
| --- | --- |
| NVIDIA certified systems | all 17 HTML tables, 742 data rows |
| NVIDIA historical NGC-Ready systems | all four HTML tables, 198 data rows |
| AMD Instinct solution catalog | all six PDF pages, eight table-header segments, 142 literal product-column fragments |
| MiTAC general-purpose index | 11 exact configuration cards |
| MiTAC AI index | 11 exact configuration cards |
| MiTAC multi-node index | 16 exact configuration cards |

`bulk-extracted.json` retains table headers/captions, rows, all literal cells,
attributes, links, blank cells, duplicate rows and exact source fragment hashes
with character/byte offsets and line/column locations. Text fields distinguish
raw decoded text from whitespace-normalized display text; original bytes remain
in immutable captures. Cell superscripts remain separate metadata as well as
literal raw text. No suffix, punctuation, spelling or case normalization occurs.

MiTAC records retain family headings, exact model headings, label/value fields,
links and card fragment proofs. Family headings are context, not aliases.
`/ per node` qualifiers remain literal; configuration cards are distinct from
family or chassis names. One printed family may have several configuration cards.

AMD output preserves the complete extraction sidecar plus every nonempty line
under each detected table header, raw partner-column slices and token spans.
Product fragments are exactly the token group starting at the header's PRODUCT
column. They may include continuation text such as `(water cooled)` or
`Two nodes (8 APUs) per tray`; **142 is a fragment count, not a device count**.
Partner names are vertically centered, sometimes multiline. The extractor never
forward-fills them, associates fragments with companies, joins multiline models,
or repairs oddities (`EXAACT`, `MI2104`, `AS -...`). Header/footnote interpretation
and any AMD company/model candidate require visual review first.

`discovery-rows.json` has 1,120 rows/fragments across four datasets. Each row links
to a source URL/hash/path and table row, card ordinal, or page/table/line locator.
There are 91 distinct literal printed company strings on HTML rows/cards and
743 distinct printed company/model pairs. Case variants and differing company
names stay separate. HTML tables preserve source-specific categories and tested
versus supported column names; their presence does not establish general support.

`catalog-novelty.json` compares literal strings to a stable read-only transaction
on the live schema-29 catalog: 5,157 devices and 2,121 sources. Both before/after
file hashes are `0879a77f5d2cdf4edfdce3a88ab03034504c02f99b4645537f43af5db5c32853`.
The report preserves every model hit, exact company/model hit, identifier hit,
source URL hit and row index. There are 15 exact printed company/model row hits,
35 same-literal-model hits in another or unknown company namespace, and 1,070
rows/fragments without a literal model/identifier hit. These are discovery-match
counts, never counts of new verified devices. All six exact source URLs are absent
from the live catalog. Alias or corporate mapping is deliberately not applied;
for example QCT/Quanta names remain literal. A nonempty WAL or changed catalog
file makes the comparison refuse an unstable snapshot.

`curated-proposal.json` is a private, independently unreviewed proposal with 154
whole printed company/model pairs and 214 source references. It selects NVIDIA
rows for Inventec, QCT, PEGATRON, Wistron, Supermicro, Inspur and Wiwynn plus all
38 exact MiTAC card models. Of these pairs, 145 lack an existing exact printed
company/model match. The source relationship is only “publisher lists this model
with this company in this row/card.” NVIDIA's literal `Partner` versus
`Manufacturer` labels remain source-scoped. MiTAC's role is product-index publisher.
This is **not importable curated evidence**: independent review and proper
source-scoped role decisions precede any importer input. Identity-cell
superscripts, table spans and missing cells are held rather than resolved. No
current selected proposal row triggers those exclusions. All AMD fragments stay
out. `Miramar - ICEMan` versus `Miramar -ICEMan` remain distinct printed strings.

## acquisition and provenance

`requests.jsonl`, `plan.json`, `capture-complete.json` and
`resume-full-access.md` retain the original acquisition scope and outcomes:
six searches, ten actual HTTP exchanges including three robots policies and one
same-origin 301 redirect. No further network calls were made by this replacement.
`capture-complete.json` closes `fetch.py`; do not reopen collection to spend the
unused budget. All captured bytes/headers are immutable and hash-verified.

`nvidia-ngc-ready.pdf` is an empty 301 body, **not a PDF**.
`redirect-nvidia-ngc-ready.pdf` is HTML from the redirect destination, **not a PDF**.
The reusable parser chooses content format from bytes, not the filename. Only
successful non-policy 200 bodies become datasets. All ten body/header pairs,
including the redirect/policies, remain in the verification receipt.

The AMD PDF sidecar is source-hash-bound, complete six pages, Poppler
`pdftotext -layout` embedded text/no OCR. This milestone performs no visual AMD
review and creates no manufacturer/deployment/fitment claims. Historical NGC
listings are source claims, not a current qualification guarantee. No PDF or HTML
rendering, external publication, seller/provider contact, installs or imports
were performed. Known OCP/Hyve access restrictions remain binding; none was retried.

## reproduce offline

Run from the project root using project direnv `python3` (verified 3.14.7), with
no package installation:

```sh
python3 -B research/batches/discovery-2026-10-14/odm-source-expansion/extract_bulk.py
python3 -B research/batches/discovery-2026-10-14/odm-source-expansion/compare_novelty.py --database data/devices.sqlite
python3 -B research/batches/discovery-2026-10-14/odm-source-expansion/verify_offline.py
```

The extractor accepts `--base` capture directory and `--output` destination;
use a separate output directory to compare deterministic reruns. The novelty
report pins its actual comparison time and current database hash; a later catalog
can change its results, while extraction from identical inputs is deterministic.

`checks.json` records 11 passing offline tests and all 20 acquisition body/header
hash checks. Fixtures cover suffixes/plus signs, duplicates, blank cells, nested
blocks, rowspan and footnote refusal, minified Unicode byte offsets, per-node
qualifiers, centered AMD labels, partial PDF sidecar rejection, exact versus
cross-company model matching, identifier-only leads, corrupt receipt rejection,
content routing despite misleading extensions, deterministic complete extraction,
and every extracted HTML fragment hash. No network is used by tests.

Next bounded step: root independently reviews the code, full dataset counts and
proposal source/role boundaries; select accepted source-scoped rows for a separate
curated input or create a reviewed discovery importer. AMD visual row mapping is
optional future work and is not required to retain this broad raw dataset.
