# handoff

**Outcome: complete bounded offline bulk extraction; unimported, pending root review.**
Owner `/root/odm_bulk_full`; sole write boundary this directory. Actual inherited
permissions: danger-full-access, network enabled, approval never; no escalation
flag. No new network/install/delegation/publication/import occurred. No live handles.

Four datasets / six captures produce **1,120 raw rows/fragments**: NVIDIA current
742 across 17 tables; NVIDIA historical NGC 198 across four tables; AMD 142 literal
product-column fragments across all six pages/eight header segments; MiTAC 38
exact configuration cards across three indexes. Full rows, suffixes, duplicates,
blank cells, links, categories and immutable source hash/offset provenance remain.
AMD company mapping is not inferred; fragments include continuations and are not
asserted device identities. Full interpretation: [README](README.md).

Files: [extractor](extract_bulk.py), [novelty/proposal builder](compare_novelty.py),
[offline fixtures/tests](test_bulk.py), [raw extraction](bulk-extracted.json),
[discovery rows](discovery-rows.json), [summary](extraction-summary.json),
[live novelty](catalog-novelty.json), [proposal](curated-proposal.json),
[verification](checks.json), [artifact hashes](artifact-manifest.json).
All acquisition artifacts and the prior checkpoint remain unchanged.

Stable read-only schema 29 catalog comparison: before/after SHA
`0879a77f5d2cdf4edfdce3a88ab03034504c02f99b4645537f43af5db5c32853`;
15 exact printed company/model row hits, 35 same literal model hits in other or
unknown company namespaces, 1,070 rows/fragments without literal model/identifier
hits. All six exact source URLs absent live catalog. Counts do not establish new
verified-device counts. Proposal: 154 exact whole printed company/model pairs,
214 references,145 without existing exact pair, source-scoped listing roles only;
independently unreviewed and not importer input. All AMD fragments excluded.

Checks: 11 offline tests pass, all 20 captured body/header receipt hashes unchanged;
deterministic extraction and every HTML fragment hash verified. No shared code,
catalog, manifests or exports changed. Source hash checks include the empty 301
body; content dispatch correctly treats `redirect-nvidia-ngc-ready.pdf` as HTML.

Excluded: physical manufacture/deployment, alias/corporate relationships, current
support guarantee, auction-unit fitment, AMD partner mappings and repaired spellings.
Original OCP/Hyve denial boundaries unchanged. Existing software/regulatory datasets
were checked in the prior source-selection checkpoint, not reacquired.

Acceptance dependency: root independent code/evidence review, then any sequential
integration is a separate assigned milestone. Root owns current PROGRESS updates.
Next finite step: review selected exact HTML/card proposals, produce reviewed
source-scoped importer input if desired; keep AMD raw until explicit visual review.
