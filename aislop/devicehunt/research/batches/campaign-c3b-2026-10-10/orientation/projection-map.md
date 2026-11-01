# c3b orientation: campaign candidates vs importer projections

2026-10-10, read-only mapping by a Claude Sonnet 5.5 Explore worker for `/root-claude`.
Code facts at the post-rename tree; verify line numbers before relying on them.

- Manifest job = `{source, path}` (path under `data/`); inventory unit `accepted_job` is exactly
  that job, `source_manifest = "data/" + path`; ISED jobs are ordinals 30–34, RRA 66–72.
  `rebuild()` dispatches via `pipeline.IMPORTERS` (`rra` → `rra.import_bundle`, `ised` →
  `ised.import_capture`).
- Importers replace a whole source: `Catalog.source` upserts `sources` and deletes that
  source's `records` (children cascade), raw tables (`rra_equipment`, `ised_equipment`) are
  refilled; nothing commits until `Catalog.finish`. Validation precedes writes.
- Source ids: RRA `rra-official-query-<sha12(query json)>` (= campaign unit id); ISED v1
  `ised-rel-<companynum>`, v2 `ised-rel-query-<normalize(query)>`.
- Record keys: RRA `<certificate>:<model>` per eligible manufacturer row/model; ISED v1
  `str(csv ordinal)`; ISED v2 `str(html ordinal)` for rows whose company targets the query.
- Candidate bundles hold `occurrence_ids`, `record_sets` hashes and coverage; full occurrences
  live in the validate receipt's `validated-records` object. Adapters call the importers' own
  validators/helpers (`rra.validate`, `rra.models`, `ised.html_rows`, ...).
  - RRA occurrences carry `projected_source_keys` = the exact `records.source_key` values.
  - ISED v2 html-row ordinal = `source_key` (target rows only); csv ordinal = `ised_equipment.row_number`.
  - ISED v1: importer keys are CSV ordinals, occurrences are HTML+CSV rows — no direct key map.
  - No shared hash between occurrences (`logical()`) and catalog `raw_json`; no fact/affiliation
    projection in the adapters; no stable claim id.
- Lineage (`lineage.py`, `data/archive-release-2026-10-04.json`): statements link to the
  catalog by value `(source_id, source_key, fact name/value | affiliation company/role)` and
  need an `accepted` review; all 4 current statements are Inventec GoldDuck, none RRA/ISED.
  Re-importing a source can orphan a statement if its key/fact disappears.
- No review/decision/gate mechanism exists in importers, `Catalog.add` or `rebuild`;
  `evidence_reviews.decision` ('accepted','rejected','superseded') exists only for lineage.
- Tests: `test_rra.py`, `test_rra_empty.py`, `test_ised.py`, `test_ised_paged.py`,
  `test_campaign.py` (22), `test_lineage.py` (7), `test_rebuild.py` (6). Nothing links
  campaign output to importer rows and nothing tests a gate.
