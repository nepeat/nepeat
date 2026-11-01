# F17 shopping-query design

recommend a small shared-helper change: **add the selected identity's exact
literal model to its existing title/explicit-identifier query set, and return
no hardware shopping links when the existing software-reference classifier
matches**. keep all usable legacy queries and the existing three-field row
shape. do not shorten legal company names, strip a model's printed vendor
prefix, add guessed corporate names, or consult the display-family table.

design only; no production changes, imports or external searches. eight live
identity examples captured read-only in `examples-before-and-proposal.json`,
with actual CLI output, source-record/identifier context and before/after database
hashes. `synthetic-risk-examples.json` covers nine additional synthetic boundaries;
those are not more live identities.

## actual behavior and minimum implementation

F17 describes the effect as brand+model. current `devicehunt.__main__.shopping_links`
actually starts from **`detail['title']`**, then adds explicit identifier values
for existing allowed kinds: fcc_id, part_number, manufacturer_model,
model_search, alias and onie_platform. the legal-company text comes from stored
titles in these examples. `shopping` CLI and `report()` both call this helper;
report embeds the resulting `shopping` array, and inline JS only renders it.

proposed shared-helper behavior:

```python
if listings.is_software_reference(detail.get('brand'), detail.get('kind')):
    return []

candidates = [detail.get('title'), detail.get('model')]
candidates.extend(i['value'] for i in detail['identifiers']
                  if i['kind'] in EXISTING_SHOPPING_IDENTIFIER_KINDS)
queries = {q for q in candidates if isinstance(q, str) and q.strip()}
# Keep current sorted order, URL templates, quote_plus and row keys.
```

`EXISTING_SHOPPING_IDENTIFIER_KINDS` means the unchanged tuple already present,
not a new registry or expansion. `get()` for newly consulted model/brand/kind
keeps old title/identifier-only helper fixtures usable. model absence adds
nothing. nonblank strings remain **byte-for-byte query literals**, including
case, internal/outer spacing, prefixes, suffixes and punctuation; `.strip()`
only decides whether to omit an empty/whitespace-only candidate. no stringifying
None into an accidental search term.

use **only** `listings.is_software_reference`'s current brand-or-kind rule.
do not treat the bareword wedge, minipack, a short code, source tier, absent
manufacturer evidence or company-family label as a new software/hardware rule.
source facts, PDF strings, compatibility text and related component names are
not query inputs; explicit identifier kinds retain their original scope and
provenance in `detail`. these URLs are search suggestions, not aliases,
identity proof, hardware fitment, stock, price or entitlement assertions.

recommended small presentation companions:

- shopping CLI: if software classifier matches, print
  `Software reference; no hardware shopping links.` instead of generic
  `No matching records.`. preserve the existing hardware table columns and
  command arguments; no new format/override flag needed.
- report HTML: append `marketplace search terms` details only when
  `d.shopping.length` is nonzero, so empty software/blank-query groups disappear.
  keep `shopping: []` in embedded data; don't omit/change its schema.

the helper alone already suppresses URLs in both newly generated surfaces.
the two presentation edits explain/avoid empty output and are separable if the
coordinator wants the narrowest patch. regenerating the current HTML report
after accepted code is necessary for its embedded query arrays to change;
historical exports should not be overwritten by this implementation task.

## eight actual examples

existing title queries remain alongside these suggested model queries:

| selected identity context | actual current title query | additional literal-model query / result |
| --- | --- | --- |
| IEIT SYSTEMS Co.,Ltd. / NF5280M6 | IEIT SYSTEMS Co.,Ltd. NF5280M6 | `NF5280M6` |
| Inspur Corporation / Inspur NF5280M6 | Inspur Corporation Inspur NF5280M6 | `Inspur NF5280M6`; do not strip printed Inspur |
| Inspur Electronic Information Industry Co., Ltd. (IEI) / Inspur NF5280A7 | Inspur Electronic Information Industry Co., Ltd. (IEI) Inspur NF5280A7 | `Inspur NF5280A7` |
| IEIT Systems Co., Ltd. / NF5280A7 | IEIT Systems Co., Ltd. NF5280A7 | `NF5280A7` |
| Meta OpenBMC platform / wedge | Meta OpenBMC platform wedge | no hardware shopping rows; current classifier true |
| owned Edge-Core / 7760-32X-A-AC-F | Edge-Core 7760-32X-A-AC-F plus explicit `AS7760-32X` identifier | add `7760-32X-A-AC-F`; retain existing reviewed identifier query |
| owned Celestica / CS8200-32X-UN-00 | Celestica CS8200-32X-UN-00 | add complete `CS8200-32X-UN-00`, not shortened CS8200 |
| ordinary Celestica / SD6200 | Celestica SD6200 plus explicit `SD6200` query | unchanged; exact duplicate model already supplied by an identifier |

six hardware examples add one model query each. ordinary SD6200's output is
unchanged, and software wedge's one current URL row becomes zero.
all eight actual CLI calls returned0; only URL text was generated, with no
browser, search engine request or marketplace click. no claim about live search
quality/result count follows from a local reduction in query wording.

exact device ids, source record ids/URLs and all identifier values are in the
saved examples. existing title and identifier queries are independent provenance
inputs; this design does not attribute a global stored title to every record
or spread identifier semantics across related models/namespaces.

## risks and compatibility decisions

**Generic/short names:** a non-software identity literally called `Switch`, `S1`
or `wedge` gets that literal query as an additional choice. it may be broad;
the existing title remains available for context. do not invent a minimum length,
letter/digit rule, brand token or equivalence to make it appear more precise.
`listings.usable` is a listing-match heuristic, not a shopping-query identity
policy. a separately scoped future quality rule could hide broad terms, but is
unnecessary to fix these six exact-model cases and should not slip into this patch.

**Duplicates:** retain existing exact-string set de-duplication and sorted order.
case/spacing variants remain separate literal choices; no casefold/whitespace
normalization merges evidence spellings. adding a new row can change indices,
but old hardware rows keep relative ordering and exact URL bytes. code that
assumes the first shopping row is canonical already relies on unsupported
alphabetical behavior; this patch should not introduce that label or ranking.

**Blank/malformed candidates:** drop non-string or whitespace-only candidate
values, producing an empty list rather than blank eBay/quoted-empty web URLs.
do not remove otherwise nonblank title/identifier text. internal `Catalog.detail`
supplies identifiers and strings; missing optional newly consulted fields should
not break historical helper fixtures. no additional source validation belongs in
this display helper.

**Output compatibility:** keep list return type and row keys/order
`query`, `ebay`, `web`; unchanged URL roots and quoting. query count intentionally
increases for some hardware, becomes zero for software. report continues to
receive a `shopping` array. preserve CLI syntax, table headers, external-search
disclaimer and all identity/evidence data. no new provenance/quality columns,
brand abbreviations or automatic link opening.

**Classifier scope:** existing classifier may conservatively suppress an identity
whose kind includes firmware/software wording even with hardware identifiers.
that is the accepted existing rule, not a new shopping-specific classification.
tests should prove branded software rows, kind-only software rows and ordinary
hardware platforms follow the same current rule; don't change the classifier
while addressing F17.

## checks for an authorized implementation

focused meaningful checks should cover: legal-title model addition while preserving
old URL bytes; exact duplicate model/title/identifier; full suffix and plus/slash/
comma URL encoding; blank candidates and missing optional new fields; existing
software brands and kind-only rule; hardware generic name kept; ordinary SD6200
already-literal output unchanged. assert no fact-derived or corporate-family query.
check the CLI's software message and report's empty-shopping rendering through
the actual helper/template, then the existing readonly catalog command test.

likely owned implementation files: `devicehunt/__main__.py`, optional
`devicehunt/report.html`, a new focused shopping-helper/CLI test and small
`tests/report.test.cjs` rendering checks. existing tests currently cover shopping
only through readonly-command behavior; JS fixture shopping arrays are empty and
do not assert link rendering. no schema/importer/source/manifest change is needed.
run these affected suites, not a full catalog rebuild or network benchmark for
this presentation-only change. coordinator decides normal export refresh after
code review; design does not authorize edits yet.

open coordinator decisions, with defaults above: helper-only versus two small
presentation companions; whether a later separate broad-query quality policy is
worth pursuing. none blocks this concrete source-faithful helper design.
