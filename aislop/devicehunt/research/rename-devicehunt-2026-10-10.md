# workspace rename: fcc_shenanigans → devicehunt

2026-10-10, coordinator/integrator/documentation editor `/root-claude` (Claude Code,
Opus 5.5). The user asked to rename the project to "devicehunt" because the scope
outgrew FCC data. This implements the c1 follow-up "explicit decision-gated relocation
operation" and was done before c3b, because c3b pins the catalog sha and the
rename changes it.

## what changed

- Directory: `/home/erin/nepeat/aislop/fcc_shenanigans` → `/home/erin/nepeat/aislop/devicehunt`
  (plain `mv`; untracked in the parent repo). The Python package was already `devicehunt`.
- `CampaignState.relocate` (`devicehunt/campaign_state.py`) + `scripts/relocate_archive.py`:
  - the archive identity is sha256 of the absolute `data/archive` path, stored in
    `operational_meta.archive_id`, `capture-authority.json` and `capture-authority-ready.json`;
  - relocate rewrites only that id, in the order meta → marker → seal, and keeps
    `authority_id`, `origin`, `legacy_backup_sha256` and every quota/lease/event row;
  - it refuses: while the old root still exists (a copy never gets authority), active
    leases, unfinished reservations, running campaign runs, quarantined authority,
    missing decision/quiesced flag;
  - the write lock is taken first; a verified backup
    `capture-leases.before-relocate-<old16>-<new16>-<events>.sqlite` is written before the
    update; one `recovery`/`relocation` event is recorded; re-running finishes an
    interrupted relocation.
- `scripts/relocate_catalog.py` now requires `--audit` (a new workspace-relative file) and
  never overwrites the Sep 27 macOS→Linux `data/exports/relocation-audit.json`.
- Test: `tests/test_campaign_state.py::test_relocation_moves_identity_keeps_authority_and_refuses_copies`
  (copy refusal, blockers, crashes at three points + retry, A→B→A→B).

## review

Claude Sonnet 5.5 reviewer, [report](batches/rename-devicehunt-2026-10-10/review/relocation-review.md):
ACCEPT with notes → fixes verified, ACCEPT. Remaining notes: an old root that exists but
is unreachable counts as gone (copy-then-delete is indistinguishable from a move); a
killed run can leave a `.pending-*` temp file in `data/archive`.

## live execution and proof

Before the move: 0 leases, 0 open reservations, 0 accounts/activations, 0 running runs,
0 publication intents; no project processes. Byte-identical backups:
`data/devices-before-rename-2026-10-10.sqlite` (sha `b66dc555…`) and
`data/archive-registry-backup-2026-10-10/` (marker, seal, `capture-leases.sqlite`).

- Archive: new `archive_id` `0d1ce7a8…`, same `authority_id` `4d204994-…`, ready,
  archive head sequence 1181 unchanged.
- Catalog: 292/297 `sources.cache_path` values verified by hash and rewritten (5 have no
  cache path); sha `b66dc555…` → `4d525902…`; integrity ok, no FK violations.
  [audit](../data/exports/relocation-audit-2026-10-10.json).
- `campaign report c2-live-run-1` and `coverage validate` on the published manifest:
  byte-identical to the pre-move outputs (`*-before.json` vs `*-after.json`). `campaign plan`: unit revisions identical; only
  recipe/job ids changed, because `campaign_state.py` (edited here) is in every stage's
  code-hash closure. All jobs were already `unplanned` since c2b, so nothing extra reruns.
  [acceptance outputs](batches/rename-devicehunt-2026-10-10/acceptance/).
- `validate-inventory`: owned baseline 10/10.
- Full suite: 590 tests OK (plain `unittest discover`, not the guarded runner; only
  `campaign_state.py`, its test and the two relocation scripts changed, no importer/catalog/rebuild module; [log](batches/rename-devicehunt-2026-10-10/acceptance/full-suite.log)).
- Standard exports refreshed (`scripts/refresh_exports.py`, 98 lookup reports).

- Clean offline rebuild at the new path (`data/rebuilt-after-rename-2026-10-10.sqlite`, 2,280
  verified inputs, archive release `c468ef07…` re-imported) equals live in all 52
  tables/indexes and the baseline, so the rewritten cache paths are exactly what a fresh
  import writes ([rebuild](batches/rename-devicehunt-2026-10-10/acceptance/rebuild.log),
  [compare](batches/rename-devicehunt-2026-10-10/acceptance/compare.log),
  `data/exports/rebuilt-after-rename-2026-10-10-*.json`).
- Not re-run: `scripts/refresh_collection_reports.py`.

## left as history (intentionally)

Historical logs, dated `data/exports/*-2026-10-0*` verification records, research batch
artifacts, ad-hoc exports (`accepted-evidence.json`, `documents-thin-client.json`,
`document-search-example.json`), `docs/history/` and `docs/handoff-AGENTS.md` keep the old
path as a record of when they ran. Superseded/recovery catalogs
(`devices-before-*`, `rebuilt-*`, `catalog-archive-rebuilt-*`) still carry old
`cache_path` values: after restoring one as `data/devices.sqlite`, re-run
`scripts/relocate_catalog.py` with `--from-root /home/erin/nepeat/aislop/fcc_shenanigans`.
