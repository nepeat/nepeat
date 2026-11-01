# promotion/export plan — preparation only

Independent review is active. This plan does not authorize promotion, exports,
changes to proofs, or current documentation. Wait for coordinator acceptance.
No helper executed; stage/candidate/manifest/proof freezes remain intact.

Upon explicit authorization, perform these sequentially in this integration
owner, without rerunning tests, a rebuild, or historical mutation helpers:

1. Read new `promotion-acceptance.json`; require its review/proof hashes and
   candidate/manifest/rebuild hashes to match `review-ready.json`, recheck all
   11 accepted input/proof hashes and 162 code/test hashes. Live must still equal
   `before.json` and its immutable before-networking-primary backup. Verify all
   existing manifest inputs before replacing live. Check new final output paths
   do not already exist and log executed commands with exit codes.
2. Copy accepted candidate to fresh sibling staging file, verify exact hash,
   then `os.replace` staging into `data/devices.sqlite`. Require live/candidate
   byte equality before and after every read-only report/refresh operation.
3. Run inspected ordinary `scripts/refresh_exports.py`, then inspected
   `scripts/refresh_collection_reports.py`, with bundled Python and `PYTHONPATH=.`.
   These refresh normal catalog HTML, targets CSV/JSON, inventory validations,
   manufacturer/factory/all inventories, 98 eligible saved lookups, FCC coverage,
   FCC discovery summaries and hyperscaler JSON/CSV outputs. Require 98 lookups
   and every generated audit database hash to equal promoted candidate.
4. Run `scripts/compare_rebuild.py` against the existing accepted
   `data/rebuilt-with-networking-primary-2026-10-04.sqlite`, explicitly selecting
   live via `--database data/devices.sqlite` and generic `--output-prefix rebuild`.
   Require 43 comparison entries and equal counts/no differences. The accepted
   duplicate-sensitive candidate/rebuild audit remains frozen; exact promoted
   byte equality binds this stronger audit to live without rerunning it.
5. Run actual live unfiltered buying CLI for literal full `Oracle Server X7-2C`
   and `AS7772-32X`. Require parsed outputs to equal staged CLI snapshots,
   including all 25/10 context facts and no vendor filter. Save new
   `data/exports/networking-primary-2026-10-04-buying-reports.json` containing
   promoted database SHA, vendor null and these two full reports. Recheck named
   variants, processor watts outside power, source-scoped software limits and
   unbranded community record with no affiliations/borrowed roles. Preserve
   prior snapshots; no replacing research proofs.
6. Use the existing generated-report DOM harness in `tests/report.test.cjs`.
   Require generated script equal current `devicehunt/report.html` script and
   embedded payload 4,084 identities. Check full Oracle and AS7772 searches,
   separate community software-reference row without brand/affiliation, and
   ByteDance vendor yields zero. Record harness output and HTML hash. Do not
   claim a screenshot; this is offline DOM verification.
7. Bind final verification to counts 4,084/5,302/44,763/283/56/3,651/22,
   118 manifest jobs/2,181 inputs, 56 immutable PDF hashes, unchanged complete
   bundle and original 2,178 input hashes, accepted test logs, 43-table logical
   and duplicate-sensitive proofs, both reconstructed FTS, 9+1 owned/0+2 examples,
   frozen code/input hashes, exact old-row preservation, candidate/live equality,
   normal export audit hashes, new snapshot hash, HTML/DOM checks and completed
   process status. Verify no database changes from CLI/export commands.
8. Return final proof paths/hashes and actual outcomes to coordinator. Root owns
   all current-document edits and ongoing milestone summary. Stop here; no new
   acquisition or next-batch work follows automatically.

For failure recovery, preserve logs/staging artifacts and the immutable prior
live/manifest backups. Do not delete candidates or historical data. A failed
read-only export can be corrected and rerun locally within accepted scope; any
failed frozen hash or unexpected database state stops promotion for coordinator.
