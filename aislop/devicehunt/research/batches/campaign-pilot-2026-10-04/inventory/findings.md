# c0 saved-input inventory

owner: `campaign_inventory`; campaign `rra-ised-saved-2026-10-04`, definition
version 1. complete offline pinning; no collection, imports, packages, source
copies, catalog copies or accepted-data writes. inputs are the actual exact
`source == "rra"` and `source == "ised"` jobs in the accepted manifest.

[inventory](inventory.json) separates 1,121 pilot source paths from accepted
checkpoints, code/policy references, saved restriction/status context and two
existing Golduck PDF/text review fixtures. recursive sibling dependencies include
the Celestica historical bundle, its base and both reviewed exhibits, plus the
Accton extract. every pilot input agrees with both its dependency declaration
and `data/import-manifest.json`; no partial bundle substitutes for the source.

| zero-based import ordinal | accepted source id | exact query | validated rows | expected source records |
| --- | --- | --- | --- | --- |
| 30 | `ised-rel-25869` | Amazon Data Services | 4 HTML / 4 CSV | 4 |
| 31 | `ised-rel-query-accton` | Accton | 167 HTML / 579 CSV | 162 |
| 32 | `ised-rel-query-edgecore` | Edgecore | 4 HTML / 29 CSV | 4 |
| 33 | `ised-rel-query-oracle` | Oracle | 12 HTML / 16 CSV | 12 |
| 34 | `ised-rel-query-celestica` | Celestica | explicit zero / no CSV | 0 |
| 66 | `rra-official-query-41e4ca912b33` | Celestica | 151 | 149 |
| 67 | `rra-official-query-33f7d154085d` | Amazon Data Services | 57 | 161 |
| 68 | `rra-official-query-63cb0b9098c7` | Edgecore | 110 | 146 |
| 69 | `rra-official-query-e5984834207f` | Accton | 200 | 188 |
| 70 | `rra-official-query-73009f906e4e` | Oracle | 516 | 896 |
| 71 | `rra-official-query-5827f2fe7092` | Edge-Core | 7 | 13 |
| 72 | `rra-official-query-78fc6b2e95f6` | Edge Core | explicit zero | 0 |

RRA totals are 1,041 raw occurrences and 1,553 current projection records.
ISED totals are 187 labelled HTML entries, 628 original CSV rows and 182 target
projection records. these counts are not distinct physical devices. all source
keys, raw counts and `records_seen` match the live catalog through read-only
immutable SQLite checks in [projection-check.json](projection-check.json).

RRA source ids follow the existing importer: prefix `rra-official-query-` plus
the first 12 SHA-256 hex characters of Python's default
`json.dumps(query, sort_keys=True).encode()`. this differs intentionally from
the helper's compact canonical JSON hashing. each RRA record key is literal
`certificate:model`; each raw row keeps its before-export ordinal. ISED v1 uses
`ised-rel-` plus the checked company number and CSV row ordinals as record keys.
ISED v2 uses `ised-rel-query-` plus existing catalog `normalize(company_query)`
and selected HTML global row ordinals. snapshot positions remain occurrence
selectors, not durable reviewed-claim identity. algorithms/code bytes are pinned.

Every RRA query has `19900101`–`20260926`, all other filters blank, and saved
`byte_identical` before/after comparison. the helper supports and retains the
importer's `exact_rows_ignore_order` mode if declared. hashes retain each file
separately even when bytes match. saved stability proves the importer comparison;
distinct legacy HTTP attempts and exact times are unverified. Celestica's first
result page is context; CSV/detail coverage is the denominator, not that page's
displayed ten rows. neither family exposes a full upstream-database denominator.
ISED has no declared date window or exact complete original POST payload beyond
the saved company query. live ISED cross-session reuse is disabled.

[27 finite review selectors](inventory.json) cover RRA stability, both explicit
zeros, reviewed company dependencies, separate applicant/manufacturer claims,
SUN-6000 in Celestica row 90 and Oracle row 215 (one certificate, two query
observations), AZ9074-64X-DC-42's separate manufacturer associations, ISED repeated
certificates, discrepancies and four accepted Golduck lineage statements. the
PDF/text pair is a regression fixture outside pilot source bytes; its accepted
release links retain page 2, row/excerpt and receipt selectors. no whole release
closure is claimed by this inventory.

Accton retains 15 HTML entries without an exact CSV match, six unmatched CSV
ordinals (307, 308, 309, 324, 325, 368), one blank HTML certificate and five
non-target group entries. Oracle preserves group number 1894B versus matched
CSV company numbers for HTML ordinals 2–12. Amazon's TE1A105, TE1A195 and TE1A001
share certificate 25869-TE1A001 but remain three identities. no exact full
44-cell duplicate CSV rows were found in these snapshots; repeated certificates
and RF configurations are preserved, and no deduplication is performed.

Restriction inputs are explicit in `source_restriction_inputs`: seven source
notes, seven saved RRA progress reports, the 12 manifests and relevant company
extracts. saved Accton original-PDF 402, Celestica original-filing 403 and
archived original-exhibit 403 remain restricted resources; no retry or alternate
route is permitted by the pilot. reviewed extracts are not original PDF/HTML.
The documented ISED `Edge-Core` form error is failed query context, not zero;
the prior RRA emsit API 400 is failed endpoint context. neither adds an import
unit. dedicated transport-failure reports were not found in the 12 source
directories; their absence means unknown, never unrestricted. current access,
throttles and precise legacy times/collector versions remain unknown. source
note dates and review dates are contextual assertions; file mtime is never used.

Storage: 7,764,220 logical source bytes across paths; 1,093 distinct hashes cover
7,160,749 unique-content bytes; filesystem allocation is 9,765,376 bytes. all
registered in place, zero copied source bytes. local registration measurement is
separate in [measurement-receipt.json](measurement-receipt.json); its timestamp
is not a capture time. measured free space is about 670.82 GB decimal. recommend
64 MiB aggregate new metadata/candidate output cap with a 2 GiB free-space floor
for the metadata-only c0/c2 pilot. overflow stops/checkpoints without discarding
mandatory dependencies or deleting evidence. integration catalog/rebuild space
is outside this cap and needs a separate assignment. current inventory/definition
payloads are approximately 1.58 MB, well below the cap.

[validation-results.json](validation-results.json) records two identical independent
inventory builds, successful exact-file/deterministic-replay verification,
12/12 source validators and 18 temporary-fixture checks: recursive closure,
missing/changed bytes, accepted-manifest disagreement, six unsafe path variants,
file/parent/root/output symlinks, nested company dependencies, exclusive exact-byte
publication reuse and refusal of changed frozen output before sibling publication.
fixtures use tiny `/tmp`
files and never copy the database. no broader implementation test suite was
needed for this research-only helper. checkpoints stayed unchanged:

- accepted manifest: `27ff5fc4a6a0d170dfb1128e17c1a452aa6bdc725c3d3c7060584ca165bd3540`
- accepted release: `2eadea0c31d9ffdfc1b763b26b8500ebddcb78c94d2706900ed382549fb33736`
- live catalog: `62ab16eb8ab562ebbd6eaf6951987800fa8f4482879007d1713d0b8529ce0e4a`

Read-only repeat verification:

```sh
python3 research/batches/campaign-pilot-2026-10-04/inventory/pin_inventory.py verify
python3 research/batches/campaign-pilot-2026-10-04/inventory/pin_inventory.py self-test
```

`build` atomically publishes each file exclusively; identical outputs are reused,
changed bytes at an existing output path fail and require a new output directory.
the first local measurement receipt is preserved unchanged on rerun. partial
publication after a crash is completed only by a compatible exact-byte rerun;
this is not a multi-file transaction. the build gate counts all existing logical
bytes under its output directory, including draft/helper/review files, plus all
proposed payload growth against 64 MiB. future c2 must apply this aggregate policy
before every operational write; this helper does not monitor other writers.
the pre-exclusive in-progress drafts are retained under `drafts/`; their hashes
describe that earlier helper, not the final frozen inventory. inventory canonical and exact-file
hashes are separate in the definition, validation result and `inventory.sha256`.
next step belongs to the coordinator: contract acceptance and a separately owned
c1 assignment. c0 does not implement campaign execution or authorize refresh.
