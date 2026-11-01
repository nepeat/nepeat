# independent ODM candidate acceptance

reviewer/editor `/root/vendor_bulk_full`. **accept the exact candidate**
`../odm-review/candidate.json`, SHA-256
`f4bb69c73b809f5d35da8e4de58b6b933d3024136f4ed18228f473db1befcc71`.
no blocking findings. this is independent acceptance of the bounded HTML/card
candidate, not production promotion or acceptance of AMD fragment identities.

effective permissions remained danger-full-access, network enabled, approval
never. this review used zero network and wrote only `odm-candidate-review/`.
all64 files under the two reviewed author/source directories stayed byte-identical;
no original, shared production, catalog, manifest or export edits occurred.

## complete evidence audit

[review.py](review.py) recomputed all1,120 original discovery rows/fragments from
the immutable captures in memory and matched the original bulk extraction and
discovery outputs. all20 original body/header hashes and all10 copied HTML/header
files match. the NGC `.pdf` filename is HTML; the empty301 response is excluded.
AMD remains unaccepted raw discovery, with no company/model mapping in the
candidate.

all **214 records and all2,255 facts** were independently reconstructed and checked
against their exact source rows/cards. a third small fragment reader verified
**1,418 source-fragment/text checks** using byte offsets, character offsets,
line numbers and SHA-256, rather than trusting the curator's generator alone.
source headers, row/card fields, full company/model strings, whole raw JSON,
family headings, links and scope facts agree. original extraction provides an
additional independent whole-row/card comparison.

- **154 exact company/model pairs, 214 references**: all60 additional references
  remain distinct source/key records. tables/cards are never collapsed into one
  assertion merely because the company/model repeats. no source-key duplication.
- **137 `program_partner`,39 `listing_holder`,38 `documentation_vendor`**:
  these match the printed Partner/Manufacturer/publisher contexts. Manufacturer
  remains the literal historical column label; physical manufacture is not
  accepted. company strings are identity namespaces, with no corporate mapping.
- all27 selected blank table cells remain blank. Tested/Supported and the
  east-west/north-south columns stay attached to their own rows. no forward-fill,
  row/column span or identity superscript is hidden.
- complete suffixes and whole grouped names survive, including both distinct
  `Miramar - ICEMan` / `Miramar -ICEMan` spellings, `X-wing-Flyboy (Tacoma)`,
  `B8056G68CE12HR-2T-TU-HE` and `B8261T85E8HR-2T-N-HE`.
- MiTAC family headings remain context; per-node quantities and literal `2OU`
  units are preserved. the four three-field cards retain the absence of storage
  bays rather than inserting a value: `D50DNP1MHCPAC`, `D50DNP1MHCPLC`,
  `D50DNP1MHEVAC`, `B8016H68D5`.

## dates and footnote scope

NVIDIA current source prints last update **Oct04,2026**; NGC prints **Sep30,2026**.
source capture timestamps are separate **2026-10-15UTC** receipt values, retained
verbatim. they are not publication/manufacture dates or current-support promises.
MiTAC has no matching printed docbuild last-update field; none was inferred.

exactly two selected nonidentity superscripts occur in `table-arm-cpu-systems`:
PEGATRON `AS201-1N0` and QCT `QuantaGrid S74G-2U` attach `*` to **GH20096GB**
module text. the captured note reads **“Tested as a standalone system.”** the
candidate preserves each marker, complete row and full hash-pinned snapshot;
it asserts no general compatibility from that cell. any later interpreted GPU
compatibility work must carry this standalone-test qualifier explicitly.
no selected company/model identity cell has a superscript.

historical NGC's NVIDIA AI Enterprise statement is conditional on installed
**V100 or T4** GPUs. candidate source scope retains that condition and disclaims
installed GPUs, current support and licence entitlement. no universal software
or qualification assertion is accepted.

## execution and preservation

all **10 unchanged curator tests pass**. their source has a hardcoded `ROOT/tmp`
fixture, so the test data root was relocated to an owned byte-identical
[test-view](test-view/) before execution. test/parser source files were read
unchanged from `odm-review/`; the original directory was never written.
[tests.log](tests.log) records the run.

`prepare_curated` validates five source snapshots and214 rows against their
pinned hashes. exact candidate import and source replacement both report214
records in [scratch.sqlite](scratch.sqlite). final scratch counts:

| object | count |
| --- | ---: |
| sources | 5 |
| exact identities | 154 |
| records | 214 |
| facts | 2,255 |
| affiliations | 214 |
| identifiers / hyperscaler evidence | 0 /0 |

all214 imported `raw_json` records match the candidate exactly. scratch integrity
`ok`, FK violations0 and FTS probe4. actual catalog identity IDs reuse exactly
nine existing exact company/model identities, with **zero unexpected namespace
collisions**. QCT/Quanta and other company relationships remain unmerged.

live catalog stays
`0879a77f5d2cdf4edfdce3a88ab03034504c02f99b4645537f43af5db5c32853`;
manifest stays
`86f6179ff8dd490739936d5ab416e39e606380acf547ac009b1dc5b269dca81b`.
[checks.json](checks.json) contains source hashes, all date/marker details,
actual identity reuse IDs and actual checks; [row-audit.json](row-audit.json)
contains every accepted source/key/namespace/fact-count/role. `SHA256SUMS` freezes
all review artifacts and the relocated fixture data.

repeat the offline review, with scratch work confined to this directory:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 research/batches/discovery-2026-10-14/odm-candidate-review/review.py --check-only
```

next step: root's exclusive combined integration, preserving old logical rows,
required full tests, fresh offline rebuild/equality and documentation/exports.
this isolated scratch proof does not replace those gates. no further source
collection, AMD interpretation, manufacturer mapping or per-device study follows
from this acceptance. no active processes remain.
