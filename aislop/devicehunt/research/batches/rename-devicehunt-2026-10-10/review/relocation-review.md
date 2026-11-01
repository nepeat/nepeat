# Archive relocation review

VERDICT: ACCEPT with notes (no must-fix for the one-time fcc_shenanigans -> devicehunt move)

Evidence: `python3 -m unittest tests.test_campaign_state` = 35 OK. Throwaway run in scratchpad (temp archive): meta-committed/marker-old crash state is refused by `CampaignState(...)` and finished by a re-run; A->B->A works; A->B a second time is refused (finding 1).
Live data read via `sqlite immutable=1` only: schema 3, recovery ready, 0 leases, 0 reservations, 2 campaign_runs both `complete`, stored archive_id = sha256(".../fcc_shenanigans/data/archive"), so every quiesce check passes and the old id matches. New id will be 0d1ce7a8...cc2639.

## Answers to the checklist
1. Crash safety: sound. Reachable states (meta/marker/seal):
   old/old/old (normal, fails closed after the move), new/old/old (open refused: marker mismatch, relocate finishes), new/new/old (open refused: seal mismatch, relocate finishes), new/new/new (done, re-run is a no-op). Combinations that cannot arise from the write order (marker new with meta old, seal new with marker old) are refused by relocate. Commit failure/KeyboardInterrupt rolls back (sqlite context manager). Only "wrongly refused forever" cases are in findings 1 and 4.
2. Copy safety: old_root must hash to the id already stored, so an arbitrary old_root cannot be forged, and symlinks/`..` in root are rejected by `_safe` (fail closed). Trailing slashes are normalised by Path. Protection is only "old path does not exist": see finding 3.
3. authority_id/origin/legacy_backup_sha256 kept via `dict(marker, archive_id=...)`; only archive_id column updated; one event inserted in the same transaction as the id update, so exactly one across retries (verified by test). Retry with meta already new does not re-snapshot (finding 4).
4. Quiesce: leases, non-complete reservations, running campaign_runs. Adequate for this move; see findings 2 and 5.
5. `_replace_immutable`: tmp in same dir, fsync, chmod 0444, os.replace, dir fsync: atomic. Bytes = `_canonical(...)` with sorted keys, same as `_publish` originally, and `_authority_marker` accepts them (test + experiment).
6. Tests: good coverage of copies, leases, two crash points, idempotence, mode. Gaps in finding 6.

## Findings

1. [should-fix] devicehunt/campaign_state.py:489 (backup name `before-relocate-<new_id[:16]>`). The name is a function of the destination only. Moving A->B->A->B reuses the B backup taken from an older state, which no longer matches, so relocate refuses with "relocation backup conflicts with current state" and recovery means manually removing a file. Reproduced in scratchpad (A->B, B->A, A->B again fails). Fix: include the old id too (`<old[:16]>-to-<new[:16]>`), or the event count/timestamp. Not hit by the single planned move.

2. [should-fix] campaign_state.py:480-487 and 489-513. Quiesce checks and the backup snapshot run before `BEGIN IMMEDIATE`, and lease/reservation inserts (lines 737, 851) are serialized by key locks plus SQL, not by the `publication` flock this op holds. A writer racing between check and BEGIN could add a lease/reservation that is then covered by the move, or the snapshot could differ from the state committed. In practice the path-bound id makes any other process fail closed once the directory is renamed, so risk is tiny, but moving `db.execute('BEGIN IMMEDIATE')` before the checks and snapshot closes it (same pattern exists in upgrade_campaign_jobs, so low priority).

3. [note] campaign_state.py:460. `os.path.lexists(old)` returns False on any OSError (EACCES on parent, unmounted/offline volume, ENOTDIR), so an unreachable-but-existing original counts as "gone". Also a copy-then-delete-original is indistinguishable from a move, and if the old path is later re-created from a restore, two live authorities share authority_id with independent quotas. Inherent to the design; document that the operator must delete/keep no copy of the old tree. Optionally `os.stat` with explicit FileNotFoundError only.

4. [note] campaign_state.py:514-516. On the retry path (meta already new) the backup is only checked with `is_file()`. A zero-byte or foreign file would be accepted. The partial-backup case is stricter pre-commit (nonempty partial file from a crash mid-`db.backup` -> "unreadable/conflicts", refused until the operator deletes it; same behaviour as upgrade_campaign_jobs). Acceptable; could assert the backup opens and has an `operational_meta` row.

5. [note] campaign_state.py:480-487. Not checked: pending `publication_intent` in the Archive index, quarantined recovery (state is preserved, which is correct), expired lease rows (any row in `leases` blocks; there is no tool to purge, so a stale row would need manual handling; live archive has none). Pending archive intents survive the move unchanged and are resolved by the next writable archive open, so not blocking; relocate_archive's `require_ready()` would surface it after rebind (readonly). Fine.

6. [should-fix, small] campaign_state.py:468-475. marker fields beyond key-set/version/archive_id are not type-validated before writing (authority_id UUID, origin, legacy hash). `_authority_marker` does `uuid.UUID(...)`; a bad marker would be rewritten and then fail on open. Validate with the same checks before step 1 so the op never rebinds an authority that the normal open then rejects. Also tests miss: crash at `relocate_meta` (meta new / marker old), the quarantined-recovery and running-campaign/unfinished-reservation refusals, `relocation` event target/provenance, and A->B->A. The reproduced behaviour above shows the first works; add tests.

7. [note] campaign_state.py:104-117. `_replace_immutable` leaves a `.pending-*` temp file in data/archive if killed between mkstemp and replace; harmless to authority but remove manually. No `_safe` check on staging dir (parent already `_safe`d via marker_path). OK.

8. [should-fix] scripts/relocate_catalog.py:21-27, 62-68. The audit is checked for existence up front but written (`open('x')`) only after the DB commit. If the audit's parent directory does not exist (or `audit` is absolute/escapes the workspace, since `new / audit` accepts both), the catalog is already rewritten and no audit exists. Pre-validate: relative, no `..`, parent exists and is inside `new`, before the first sha/update. Retry is idempotent (paths already under new), so recoverable, but the audit's before-hash would then describe the no-op second run, not the real change. Also `old.resolve()`/`new.resolve()` follow symlinks while archive identity forbids symlinks; fine here.

9. [note] scripts/relocate_archive.py. Ordering is right (relocate -> read-only Archive.require_ready -> state.db() full validation); `--quiesced` as required store_true is odd but matches the library gate. Does not print the backup path or the event id; `--from-root` relative would just fail the id check (fail closed). Must be run from the destination with PYTHONPATH=. as documented; `--to-root .` resolves to the absolute cwd. Remember PROGRESS.md "host" section must be updated after running (it still says no relocation command exists).

## Run order reminder
mv the directory, then `scripts/relocate_archive.py --from-root /home/erin/nepeat/aislop/fcc_shenanigans --to-root <devicehunt> --decision ... --quiesced`, then `scripts/relocate_catalog.py ... --audit data/exports/<new>.json` (make sure the parent dir exists).

## Fix verification (second pass)

UPDATED VERDICT: ACCEPT (no must-fix remaining)

- `tests.test_campaign_state`: 35 OK.
- Scratch repro (temp archives): four consecutive moves A->B->A->B->A, with crashes injected at relocate_meta, relocate_marker, relocate_before_commit; each crashed state refused by normal open, each re-run finished it, an extra re-run after completion was a no-op. Result: 4 distinct backups (names bind old[:16], new[:16], events-before count), exactly 4 relocation events. Finding 1 fixed.
- Finding 2 fixed: BEGIN IMMEDIATE first; checks, snapshot and update in one write txn; backup via a separate read-only connection (safe under our write lock, verified by snapshot compare).
- Finding 6 fixed: marker types and UUID validated before any write; quarantined authority refused.
- Finding 8 fixed: relocate_catalog validates the audit path (relative, inside workspace, existing parent, not existing) before the sha/update.
- Retry-after-commit now requires the backup to exist and record (old_id, authority_id), closing note 4.
- Remaining notes (non-blocking): the lexists/unreachable-old-root caveat (3) is unchanged; a path like `data/../x` is resolved before the containment check in relocate_catalog, which is fine; `.pending-*` leftovers (7) unchanged.
