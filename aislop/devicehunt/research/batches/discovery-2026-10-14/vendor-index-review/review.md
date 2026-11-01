# independent vendor index review

reviewer `/root/network_bulk_full`, 2026-10-15 UTC. **accepted as the bounded,
separate discovery dataset; no blocking findings.** this does not approve device
imports, crawling the discovered URLs or production integration.

actual developer permissions remain **danger-full-access filesystem, network
enabled, approval never**. no sandbox override was supplied. all review work was
offline and writes were confined to this new `vendor-index-review/` directory.
author artifacts, shared captures, production code, database, import manifest and
exports were read-only. no active tool handles remain.

## exact reviewed artifacts

| artifact in `vendor-source-expansion/` | SHA256 |
| --- | --- |
| inputs-ready.json | `9ae6dd67f693bcd25797e271fb133c6cd9724ee782f6018d8b53d554acb21095` |
| discovery-ready.json | `678a3598a2cd3677d8bf883ade34f41da8661bb89a2a94fa7d0c25ee9299e85e` |
| discovery.sqlite | `ad3235bef94c46819d117898db2f28b06466c2d5f7c682ef0da3ee8ebc092f61` |
| offline_parser.py | `fee1c25c3a68d8fb492329bc1efd47ba5bd82fe18ada843b675589db777f8e50` |
| build_discovery_db.py | `7bdae45e429088572882f0aa83a20ffd81cfe8ef00b0445b5d20ff726c7aa739` |
| verify_offline.py | `c41abf012536afe167882e0b41163acc8b084954f257caf7bdcbf6543d9c42f7` |
| integration-contract.md | `5995ec75d1aaeed16983b8f86be08e4238e2be7c1aac48c0d59c0a90d936d44a` |

only the **ready** JSON pair is accepted. older `accepted` development filenames
remain superseded and do not imply independent acceptance.

## evidence and projection results

[audit_inputs.py](audit_inputs.py) independently verified all **243 capture
hashes**, all five pinned dependency hashes and every **889 source projection**.
it uses a separate event-buffer HTML parser and XML traversal, without importing
the delivered parser. the review exceeds sampling: all exact hrefs, anchor
ordinals/lines, raw/normalized labels, Edge-Core document headings/metadata,
Oracle table/group headings and Celestica locations/lastmod values agree.
[audit.json](audit.json) records each input path/hash and per-capture row count.

| source | independent rows verified | source boundary checked |
| --- | ---: | --- |
| 239 Edge-Core saved pages | 615 | nearest original document card, exact document revision/metadata and query pairs preserved |
| Oracle legacy x86 | 18 | complete printed labels remain distinct, including X2100 and X2100 M2 |
| Oracle older M/T | 67 | tooltip-only documentation anchors within linktitlecell; printed language/group scope and shared URLs retained |
| Celestica sitemap | 189 | exact XML entries; six platform navigation URLs and 183 corporate/other URLs remain navigation |

representative source checks included `AGR100 (AS5912-54X) DS R20` card heading
and metadata, `DCS203 (AS7326-56X) DS R10` paired with an R09 URL, multilingual
`SPARCT3 ...` groups, Fujitsu-labelled documentation, and T8 headings sharing one
URL. source spelling, duplicated raw URLs, query variants and different labels
survive independently. no suffix expansion, family alias or manufacturer mapping
is introduced.

all **40 revision disagreements** remain explicit. the known DCS203 R09 URL row
is quarantined as `excluded_known_restriction_or_missing`, retaining its R10
heading and both raw revision strings. other AS7710/AS7760/AS7761 denial patterns
remain in the fixed config; there are no matching rows in this finite dataset.
no discovered link was followed or scheduled. Accton's 404 HTML capture is absent
from parsed inputs; it is unavailable, not an empty device inventory.

## SQL, novelty and reproducibility

all 889 SQL row projections and complete `raw_json` objects agree with the pinned
ready JSON. captures have foreign keys, row ordinals preserve multiplicities,
all rows are `discovery_only`, and the three identity/manufacturer/deployment
assertion flags are false. publisher roles are restricted in these reviewed
inputs to product-page, sitemap and documentation-index association.

the author **8 offline tests pass**: [tests.log](tests.log). the delivered
`verify_offline.py --check-only` also passes without rewriting author artifacts:
[verify-check-only.json](verify-check-only.json). its own 8-test subprocess passed.
temporary test files were directed into this review's `tmp/` directory.

a fresh private rebuild at [reconstructed.sqlite](reconstructed.sqlite) is
**byte-identical** to the delivered database, SHA256
`ad3235bef94c46819d117898db2f28b06466c2d5f7c682ef0da3ee8ebc092f61`.
both directions of logical comparison match for datasets, captures, links and
link_search. integrity `ok`, FK violations zero, all 889 FTS entries present;
the delivered read-only FTS probe returns three source-scoped rows.

[novelty_review.py](novelty_review.py) independently recomputed all 889 URL,
query-free endpoint, complete-label model/identifier and token-hit projections
using a read-only transaction and confirmed no nonempty WAL. results match:

- 869 distinct raw URLs and 727 comparison endpoints absent from catalog URLs;
- 700 document/index rows spanning 548 endpoints, **540** absent;
- 22 rows with whole-label model equality and four with whole-label identifier
  equality; whitespace/case handling remains explicit, token matches heuristic.

[novelty-review.json](novelty-review.json) records these checks. query-free
endpoints are comparison keys only: raw URLs, `wpdmdl`, refresh parameters and
duplicates are preserved. absence from catalog URLs does not establish absence
from historical captures, new devices or document equivalence.

live catalog SHA before/after:
`0879a77f5d2cdf4edfdce3a88ab03034504c02f99b4645537f43af5db5c32853`.
manifest SHA before/after:
`86f6179ff8dd490739936d5ab416e39e606380acf547ac009b1dc5b269dca81b`.
all pinned author files also remain unchanged.

## security and integration boundary

the reviewed parser has no network operations and does not execute HTML. capture
paths resolve inside the project and bytes are hash-checked before parsing;
non-200 inputs fail. filtering keeps permitted HTTP(S), same-host/path/class
scopes. SQL data/query values use parameters; read-only search uses `mode=ro` and
`query_only`. a missing search database fails rather than creating one. private
build uses exclusive new-path creation, refusing replacement of an existing DB.

the accepted SQLite schema is a separate discovery store. it contains no
authoritative devices, records, affiliations, identifiers or facts tables; no
discovery item becomes a hardware, compatibility, support, deployment or unit
claim. the integration contract correctly requires a separate reviewed production
command/schema path and transaction/preservation/rebuild checks if later adopted.
the builder is reviewed here for this exact hash-pinned dataset, not arbitrary
untrusted JSON inputs or concurrent catalog mutation. the parser's catalog
comparison uses main-file before/after hashes; this independent review additionally
used a stable transaction and WAL check. future concurrent collection should use
the same pinned-snapshot discipline.

no code or evidence fix is required for acceptance of this finite dataset. no
production suite, production import, export refresh or promotion was performed or
claimed. root owns any next integration decision. there is no remaining collection
step in this review.
