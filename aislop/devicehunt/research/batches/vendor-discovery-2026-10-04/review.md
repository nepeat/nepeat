# independent source review — vendor discovery 2026-10-04

status: source review passed for bounded ByteDance research and two QCT curated
records. candidate/promotion and final documentation review remain pending. review used
local captures only; no acquisition, tests or shared-state writes.

## ByteDance

**pass for evidence boundaries and bounded research; metadata finding closed.**

reviewed `bytedance/handoff.md` and the five artifacts under
`data/raw/vendor-discovery-bytedance-2026-10-04/`. all four report-listed artifacts
match their recorded SHA-256 and byte size; three saved tool-response JSONs parse.
report records eight exact search queries against eight allowed and two source
fetches against six allowed, both fetches for the newly discovered CSDN article
(one web extraction, one original HTTP-200 HTML capture). this is consistent with
the saved local record; local artifacts do not independently prove absence of
unrecorded network calls. known Eximpedia 402 is explicitly excluded from requests.

saved extraction lines 318–320 and original HTML explicitly contain the community
B4020/64*400GE and B5020/64*800GE statements under 字节大模型组网. original HTML also
preserves the author's qualification that public architectures were refined and
individual details may diverge from reality. author 阿拉斯加大闸蟹 is shown. no exact
original ByteDance source is identified for that statement. keeping affiliation,
port count and deployment unimported is correct; B5020 remains outside scope.
no curated input is proposed or present. the existing Accton B4010 applicant/model
source does not supply ByteDance affiliation; owned baseline and the two separate
ByteDance expectations must remain unchanged by this research-only source group.

minor correction (closed): initial handoff labelled the visible 2026-01-22 15:13:39 time
as timezone unspecified. that is true of the visible display, but captured HTML
also provides `bytedance:published_time=2026-01-22T15:13:39+08:00` and
`bytedance:updated_time=2026-04-24T01:23:45+08:00`. preserve the distinction and
record the explicit metadata timezone if reporting publication provenance. the
`bytedance:` metadata namespace itself is not evidence of ByteDance authorship,
product affiliation or deployment. this does not affect the excluded-claim result. researcher corrected the handoff
and independently verified its SHA-256 is
`8fe32e59ad9ebe2c6c5fe4a14cca389ce55ff88f13a1112c39cf6d597a9c7d16`.

## QCT — pass

reviewed completed handoff, every one of the 38 proposed facts and both identity/
role records against D51B page 2 rendered image and embedded extraction and a
fresh local parse of the original D52BQ HTML. all seven fetch-report capture
paths exist and match their recorded SHA-256 and byte lengths. status sequence
is 200/404/404/200/200/200/200; dates are UTC capture observations, not release
dates. handoff records seven fetches (limit eight), eight searches (limit eight).
search count is author-reported: query captures are not supplied for an independent
recount, which does not invalidate source-backed proposed claims.

accepted frozen proposed input:
`research/batches/vendor-discovery-2026-10-04/qct/proposed-qct-facts.json`
sha256 `3ce92ff81e88da770994c7f269f3fde2b82a32576bd925330c21968daea47dfb`.

| accepted source ID | exact model | facts | source snapshot |
| --- | --- | --- | --- |
| `qct-d51b-datasheet-mirror-2017` | QuantaGrid D51B-2U | 20 | `data/raw/vendor-discovery-qct-2026-10-04/d51b-datasheet.pdf` |
| `qct-d52bq-public-product-2026` | QuantaGrid D52BQ-2U | 18 | `data/raw/vendor-discovery-qct-2026-10-04/d52bq-product.html` |

D51B PDF sha256 `68418af147f05d4afa468a5fce8a5fad1026168fa90f37b9277b6b8e559ecf7e`,
2 pages. D52BQ HTML sha256
`d5b0fd941c02b83842157ce5e9a4ad8e188fe5abe4bd6e9e07a57fe927061455`.
only QCT brand/platform_vendor roles; no Quanta association, manufacturer,
operator or hyperscaler assertion. exact prefixes stay intact. all specification
values, alternatives and conditions agree with their sources, including D51B
PSU efficiency/options and literal 7+1 fan redundancy, D52BQ all five storage
SKU alternatives, 11+1 fan redundancy, memory arithmetic as printed, conditional
45°C and absent voltage. dimensions preserve printed inch/mm pairs. historical
WSSD configured memory facts remain separate. D51B copyright 2014–2017 and
filename are preserved without selecting a release date.

mirror URL is deliberately the actual acquisition URL, never mislabelled original
QCT hosting. source coverage and per-record provenance explicitly retain original
vendor-hosted byte-equality uncertainty; rendered page confirms QCT branding and
blank authorized-partner box. D52BQ original public HTML contains registration/
email gate; gated items were not captured. the independently accessible D51B
mirror follows a 404 original product lead rather than a denied gated D52BQ item.
T41S partner-logo PDF and its page remain research-only, excluded from curated
and reference-PDF integration.

snapshot paths are relative to an eventual input installed under `data/` and both
resolve there. **the current proposal resides in the research directory: importing
it directly there would resolve snapshots incorrectly.** integrator must copy
accepted JSON byte-identically into `data/` before importing/registering that path.
this is an integration instruction, not a claim correction.

accepted optional reference archive addition: only the two-page D51B PDF, with
new integrator-generated `documents.read_pages`-compatible embedded-text JSON
beside the COMPLETE bundle. author `.pages.json` is a bare page list, useful for
review but not itself reference importer input. no new bundle/text capture is
accepted yet; validate those after integrator generation, preserving all 44 prior
reference entries. all other QCT raw captures remain outcome/provenance research.

## candidate and final documentation — pending

await assigned candidate preservation/equality proofs, exact source deltas,
read-only CLI/baseline/FTS checks, exports and edited current docs. this staged
source review does not authorize integration or establish completed milestone QA.

