# independent final archive promotion review

reviewer: `/root/archive_final_review`, 2026-10-04. outcome: **accepted for
coordinator-controlled catalog promotion**. no blocking finding remains in the
reviewed staged bytes. reviewer authored none of the implementation or integration
inputs; all data access used read-only paths/connections. only this report was
written. no delegation, network, package installation, import, export refresh,
archive mutation or live promotion occurred during this review.

## exact accepted inputs

independently streamed SHA-256 hashes:

| input | SHA-256 |
| --- | --- |
| candidate `data/catalog-archive-candidate-2026-10-04.sqlite` | `62ab16eb8ab562ebbd6eaf6951987800fa8f4482879007d1713d0b8529ce0e4a` |
| clean rebuild `data/catalog-archive-rebuilt-2026-10-04.sqlite` | `c4eccb0c03ffac2c3810788098191866b2bce2ff9d0268f001d0837121be4281` |
| frozen release `data/archive-release-2026-10-04.json` | `2eadea0c31d9ffdfc1b763b26b8500ebddcb78c94d2706900ed382549fb33736` |
| final staged manifest `data/import-manifest-archive-2026-10-04.json` | `209f75aa9a913e8d12119a7db49defed1d094efd54b4c5d770a875a050ef93a8` |
| unpromoted live `data/devices.sqlite` | `a098c390431efd77d25f162acb039d2ddd938ff5effeffd7e93fa9b0bc34ff16` |

release ID: `c468ef0721b90f5935615f37dd9619dad6fc1bc2159a107cbf054ede6aad5b79`.
the earlier `4a53…` manifest hash in [candidate-proof.json](candidate-proof.json)
is superseded by [ntra-replay-manifest-proof.json](ntra-replay-manifest-proof.json).
acceptance binds the **final `209f…` manifest**, not the earlier staging manifest.

## independent acceptance checks

- rehashed all **2,255** final manifest files with zero missing/corrupt inputs.
  all **2,205** old manifest paths and hash values remain present and unchanged.
  original accepted capture bytes therefore remain pinned; consolidation selected
  historical SQLite snapshots, not source captures or reference dependencies.
- independently called `lineage.prepare()` against the real frozen release and
  immutable read-only candidate. all artifact paths/lengths/hashes, receipt DAG,
  selected PDF/text associations, excerpts and existing catalog projections passed.
  all **374** frozen wrappers match original durable receipt IDs/kinds/times/payloads.
  recomputed every explicit artifact/receipt reference array and every review/link
  canonical identity; all matched. current mutable acquisition state is unnecessary.
- independently resolved all **four Golduck claims**, including ancestor and saved
  text hash verification. candidate has one release, 146 artifacts, 148 release
  paths, 374 receipts, and four each statements/reviews/links/release statements.
  these are existing vendor-specification facts; XML discovery and candidate
  derivations create no hardware, manufacturer or deployment claim. literal width
  conflict, 162-lane wording, raw values and null normalized values are retained.
- checked row counts for all **43** legacy logical tables/indexes against old live
  and preservation proof. independently repeated duplicate-sensitive comparisons
  for sources, devices, records, affiliations, identifiers, facts, hyperscaler
  evidence, documents and document pages. every original value matched, excluding
  only `sources.cache_path`. independently verified **285** changed source paths
  lie in this workspace and their bytes match each original source SHA-256.
- inspected [comparison output](../../../data/exports/archive-2026-10-04-comparison.json)
  and comparator implementation: all **52** logical tables/indexes match with
  grouped row multiplicities and FTS rowids. clean rebuild comparison intentionally
  excludes operational `observed_at`, `fetched_at` and `cache_path`; this is logical
  equality, not identical database bytes. exact old-to-candidate preservation above
  does retain historical timestamps. validation output matches the owned **10/10**
  baseline; preservation proof retains separate ByteDance **0/2** gaps.
- accepted coordinator integrity/foreign-key results and
  [fts-proof.json](fts-proof.json): both candidate and clean rebuild FTS integrity
  checks passed on disposable copies and their original hashes stayed unchanged.
  [full-tests-final.log](full-tests-final.log) records **491 tests, OK**. resource
  warnings do not represent test failures. no redundant full-suite/rebuild rerun.

## restore, implementation and remaining publication work

[archive-restore-proof.json](archive-restore-proof.json) and its independent review
demonstrate an isolated copied workspace, both metadata indexes omitted before
receipt reconstruction, clean audit, unchanged candidate bytes, all four evidence
lookups and zero network calls. independently rehashed its **422 archive files
and 105 workspace artifacts** against proof hashes/sizes: all **527** match.
new-workspace resolution does not depend on old absolute paths.

historical SQLite consolidation is separately covered by the independent
[consolidation review](consolidation-review.md), pinned storage manifest
`83b5283c94cefc242c0e57decacdd27cbc6a4a633408bafe4b668c843fce83dc`,
complete 121-file stream verification, selected new-directory exact-byte restores,
and retained three-file recovery set. did not repeat the 32 GB decompression audit.
byte backup preserves history; filesystem timestamps/modes are outside its contract.

fresh focused inspection covered artifact publication/reindex, recipe reuse,
capture policy/leases, release dependency closure, lineage validation/import/lookup,
storage verification/removal/restore and NTRA saved replay. prior resolved findings
remain corrected. saved NTRA validates full positional/text captures and sibling
dependency hashes before replacing its source; optional parser installation is
unnecessary for the final replay route. the manifest description's older sentence
“NTRA requires optional PDF packages” is stale explanatory text, not a functional
dependency of this accepted route; correct that explanation in current docs without
silently changing the accepted manifest hash.

acceptance is for the exact staged release above. the coordinator remains responsible
for preserving pre-promotion live/manifest backups, promoting candidate and final
manifest with the existing exclusive workflow, recording resulting live hashes,
refreshing affected exports and updating current documentation with independent QA.
those completion tasks remain pending at review time. this acceptance does not
claim full archive coverage, current marketplace availability, newly inspected
visual PDF pages or resolved ByteDance affiliation gaps.

## acceptance addendum: description-only manifest correction

independent follow-up review, 2026-10-04: **accepted**. compared parsed JSON for
the preserved `data/import-manifest-archive-before-description-2026-10-04.json`
and the current staged manifest. exactly one field changed: `description` now
accurately states that complete cached NTRA text/positional captures avoid network
and optional PDF extraction libraries. `imports`, `files`, `expected`, and every
other field are unchanged. [manifest-description-proof.json](manifest-description-proof.json)
matches this independently inspected diff.

the preserved earlier manifest still hashes to
`209f75aa9a913e8d12119a7db49defed1d094efd54b4c5d770a875a050ef93a8`.
**promotion acceptance now binds the corrected staged manifest SHA-256
`27ff5fc4a6a0d170dfb1128e17c1a452aa6bdc725c3d3c7060584ca165bd3540`**.
independently rehashed candidate, clean rebuild and frozen release; all three
retain the exact hashes in the original acceptance above. the prior preservation,
input hashes, rebuild and test results still apply; no new rebuild/full-suite run
is warranted by a description-only change. this addendum resolves the earlier
stale-description note and supersedes only its manifest hash binding, while
preserving the original acceptance record and its pending coordinator tasks.
