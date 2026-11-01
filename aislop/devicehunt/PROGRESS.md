# paused handoff

updated 2026-10-14 PDT (2026-10-15 UTC). **The user requested graceful spin-down
because the quota reset arrived. Stop new work; resume only on a new user request.**
Root coordinated, integrated and edited documentation. All three replacement
GPT-6.1 Sol/high workers confirmed full filesystem access, network enabled and
approval-never; each has now checkpointed and stopped with no active handles.

Read [GOAL](GOAL.md), [AGENTS](AGENTS.md), [workflow](docs/agent-workflow.md) and
[evidence policy](docs/evidence-policy.md). User's latest scope correction remains:
prioritize broad vendor/ODM sources and reusable collectors over single-device
studies, including Celestica, Edge-Core, Accton and Oracle. Previous active status
is archived [here](docs/history/2026-10-14-before-reset/PROGRESS.md); earlier
shutdown history is [here](docs/history/2026-10-14-before-resume/PROGRESS.md).

## authoritative catalog

Wave5 + QCT was promoted during this resumed run. The later breadth batch was
**not staged or promoted**. [Shutdown read-only proof](research/batches/discovery-2026-10-14/shutdown-live-state.json)
confirms the live hashes, counts, no live SQLite sidecars and no breadth candidate,
manifest, stage report or promotion result. Root has no pending tool sessions.

| item | verified live state |
| --- | --- |
| authority | `data/devices.sqlite`, schema 29 |
| database SHA256 | `0879a77f5d2cdf4edfdce3a88ab03034504c02f99b4645537f43af5db5c32853` |
| identities / records / sources / facts | 5,157 / 7,302 / 2,121 / 74,837 |
| documents / pages | 72 / 3,804 |
| complete reference bundle | `data/raw/reference-documents-with-wave5-2026-10-12.json`; 61 entries / 3,730 pages |
| manifest | `data/import-manifest.json`; 152 jobs / 3,984 inputs |
| manifest SHA256 | `86f6179ff8dd490739936d5ab416e39e606380acf547ac009b1dc5b269dca81b` |
| owned baseline | 10/10; nine document snapshots + one reviewed 7760 label |
| ByteDance B4020/B4010 expectations | 0/2 qualifying affiliation hits; unresolved |
| manufacturer inventory | 2,147 target/identity pairs / 2,146 identities |
| all target associations | 4,629 pairs / 4,486 identities |
| hyperscaler evidence / lineage | 22 rows on 18 devices; 146 artifacts / 374 receipts / four reviewed statements |
| production validation | all 209 production/test/dependency files match the accepted 791-test freeze; no new full-suite run for data-only work |

Wave5 adds 35 identities / 41 records / 235 source-scoped facts. Independent
[final acceptance](research/batches/discovery-2026-10-12/review/wave5-integration-review.md),
[promotion receipt](research/batches/discovery-2026-10-12/integration-wave5/promotion-acceptance.json),
[promotion result](research/batches/discovery-2026-10-12/integration-wave5/promotion-result.json),
[all-52-table rebuild comparison](data/exports/wave5-2026-10-12-comparison.json)
and [eight refreshed exports](research/batches/discovery-2026-10-12/integration-wave5/exports-report.json)
are complete. All 1,060,700 prior logical rows were preserved; 59 expected old
reference-bundle source hashes changed. Candidate and rebuild integrity/FK,
baseline, duplicate multiplicities, FTS rowids and campaign gates passed.
Fresh rebuild: `data/rebuilt-wave5-2026-10-12.sqlite`. Recovery pair:
`data/devices-before-wave5-2026-10-12.sqlite` and
`data/import-manifest-before-wave5-2026-10-12.json`. Retain all historical files.

## ready breadth batch — unexecuted

[Accepted inputs](research/batches/discovery-2026-10-14/integration-breadth/accepted-inputs.json)
pin three independent source reviews and exact candidate hashes:

| group | sources | records | facts | source meaning |
| --- | ---: | ---: | ---: | --- |
| Red Hat nine-company lists | 9 | 242 | 3,388 | certification listing holder; no manufacture assertion |
| NVIDIA current/historical + MiTAC indexes | 5 | 214 | 2,255 | program partner, historical listing holder or documentation vendor |
| complete BISDN HCL | 1 | 11 | 77 | network-OS compatibility reference, default buying matches excluded |
| total | 15 | 467 | 5,720 | whole source rows/cards; no new hyperscaler claims |

The Red Hat companies are ASRock Industrial, ASRockRack, Aivres, Gigabyte,
MiTAC, Pegatron, TYAN, Wistron and xFusion. NVIDIA/MiTAC contribute 154 exact
company/model pairs across 214 separate references; grouped spellings, blanks,
qualifiers and long suffixes stay source-specific. AMD fragments are excluded.

Root owns `research/batches/discovery-2026-10-14/integration-breadth/` and all
shared writes. Stage helper hash/preservation/path findings were fixed and
independently accepted with ten isolated probes. [Plan review](research/batches/discovery-2026-10-14/integration-review/plan-review.md)
and [final helper checkpoint](research/batches/discovery-2026-10-14/integration-review/shutdown-handoff.md)
accept the concrete plan and helpers, **not actual integration results**.

On explicit resume: revalidate pinned hashes and exclusive ownership, run the
accepted `stage.py`, then `check_queries.py`; perform a fresh offline rebuild to
`data/rebuilt-breadth-2026-10-14.sqlite` from the candidate manifest and explicitly
compare against `data/devices-candidate-breadth-2026-10-14.sqlite`. Obtain final
independent stage/rebuild acceptance before producing the promotion receipt,
running `promote.py` and refreshing exports. Do not run historical helpers in bulk.
Predicted counts are 5,555 identities / 7,769 records / 2,136 sources / 80,557
facts; these are preflight predictions, not staged or live results. The complete
61-entry PDF collection remains unchanged by this batch.

## other saved breadth work

- [Accepted separate vendor discovery index](research/batches/discovery-2026-10-14/vendor-index-review/review.md):
  889 rows / 243 captures: 615 Edge-Core links, 85 Oracle legacy documentation
  links and 189 Celestica sitemap entries. All independently checked; eight tests
  pass and SQLite reconstruction is byte-identical. Of 548 document/index
  endpoints, 540 were absent from the pinned catalog URL inventory, not proven
  new devices. Forty revision disagreements and one quarantined missing route
  remain. Only `inputs-ready.json`, `discovery-ready.json` and `discovery.sqlite`
  are accepted; older development filenames are superseded. Use the read-only
  query commands in the [contract](research/batches/discovery-2026-10-14/vendor-source-expansion/integration-contract.md).
  Accton's failed sitemap is unavailable, not an empty inventory.
- [Nine Edge-Core datasheets / 40 pages](research/batches/discovery-2026-10-14/edgecore-corpus-review/review.md):
  captured in 28 exchanges across nine family buckets. All pages independently
  re-extracted exactly. Accepted for document text search only; unimported.
  [Corpus staging draft](research/batches/discovery-2026-10-14/corpus-integration-plan/handoff.md)
  is **unreviewed, untested and unexecuted**, including no syntax/import check.
  Inspect and test before review; any later integration must append nine entries
  to the complete existing bundle with sibling dependencies, after breadth promotion.
- [AMD six-page checkpoint](research/batches/discovery-2026-10-14/amd-qualified-review/handoff.md):
  all six saved PDF pages rendered and visually viewed; 142 original raw fragments
  remain uncurated. Centered partner spans, multiline qualifiers, Dell footnote4
  and a page6 column anomaly need resolution. No grouped dataset, curated
  candidate, tests, scratch import or live writes. Renderings and hashes retained.
- LVFS and Dell catalog probes stopped at disallowing robots policies. Metadata
  was never requested; no alternate route is planned. Cumulus matrix retrieval
  was unavailable; only BISDN was curated. Saved source failures remain binding.

## parked earlier work and limits

[Practical-source candidate](research/batches/discovery-2026-10-14/practical-sources/handoff.md)
(3 sources / 4 records / 60 facts: SD4100 and Oracle ZS3 internal components) is
unimported and awaits final independent candidate acceptance. SD4100 exact order
code stays separate; Oracle component numbers are parent facts, not identifiers.
[Specification-grouping fix](research/batches/discovery-2026-10-14/grouping-fix/handoff.md)
is private, not production; 49 focused and 49 normal-layout tests passed, but
final independent acceptance/application remains pending. Breadth has priority.

Campaign gate remains release `campaign-release-1d45317e…`, coverage
`coverage-b8ee5df9…`, 12 decisions; definition is
`research/batches/discovery-2026-10-11/sun6000-a/inventory-r2/`. Campaign-source
changes require queue → independent decisions → release → manifest gate update.
Archive authority `data/archive/` was not changed. SUN-6000 stays Kladno-only;
Monterrey is unmapped context. Oracle/Celestica identities stay separate. Corporate
display groups do not merge identities; IEIT/Inspur mapping remains unresolved.

Use project direnv `python3` 3.14.7; no installs/system changes. Preserve unrelated
parent-repository changes. No cleanup/deletion, denied-source retries, RRA cycle,
paid Datadory work, seller contact, publication or push. Worldwide coverage,
current support, marketplace stock and auction-unit configuration remain unproven.
Shutdown documentation editor: root. [Independent final documentation review](research/batches/discovery-2026-10-14/integration-review/shutdown-doc-review.md)
accepted the handoff, verified 112 local links and live/pending state, then stopped.
