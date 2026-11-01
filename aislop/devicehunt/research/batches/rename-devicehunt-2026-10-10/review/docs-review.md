# Documentation review: devicehunt rename

VERDICT: ACCEPT with notes (no must-fix). Reviewer: Claude Sonnet 5.5, one pass, read-only.

## Verified against files (all matched)
- audit json: before `b66dc555…`, after `4d525902…`, 292 verified cache paths, only `sources.cache_path`; live `devices.sqlite` and `devices-before-rename-2026-10-10.sqlite` hash prefixes agree; 297 sources, 5 with no cache path, 0 `fcc_shenanigans` cache paths left.
- `capture-authority.json` / `-ready.json`: archive_id `0d1ce7a8…cc2639`, authority_id `4d204994-…` (unchanged); index head sequence 1181; relocate backup `capture-leases.before-relocate-232a60e788bc6bc2-0d1ce7a86800440e-7.sqlite` exists; events table has the single `recovery`/`relocation` row (id 8).
- Sep 27 `relocation-audit.json` still exists, separate from the new dated audit. Script flags (`--audit`, `--decision`, `--quiesced`, `--from-root`) match README. Full-suite log: 590 OK; refresh log: 98 lookup reports; coverage-after: 12/12, 2 zeros. All relative links in the three docs resolve (including `data/history/README.md`, `docs/archive-storage.md`, the c2/c2b/c3a notes). Relocation review verdicts and remaining notes are quoted accurately.
- Pending items are labelled honestly in PROGRESS.md:41 ("post-rename rebuild pending") and :43 (collection reports last refreshed after SmartNIC/QCT).

## Findings

1. [should-fix] research/rename-devicehunt-2026-10-10.md:57 (end of "live execution and proof"). The milestone note never says what is unproven. It lists only passes, while PROGRESS lists the rebuild and collection reports as pending, and `acceptance/rebuild.log` (29 lines) exists as an in-progress artifact. Add after line 57:
   "Not done at this checkpoint: a clean offline rebuild/compare at the new path (`acceptance/rebuild.log` is an in-progress run, not acceptance evidence until its result is recorded here) and `scripts/refresh_collection_reports.py` (collection reports still reflect the pre-rename run). Neither is expected to change catalog content."
   Drop the last clause unless the rebuild comparison proves it.

2. [should-fix] PROGRESS.md:61. "latest verified rebuild | `data/rebuilt-with-smartnic-qct-2026-10-05.sqlite`" now sits beside "post-rename rebuild pending" and that file carries old `cache_path` values (note lines 62-66). Suggested: "latest verified rebuild (2026-10-05, old cache paths; superseded by the post-rename rebuild once it is recorded)".

3. [should-fix] PROGRESS.md:42 and :104, note :55, plus 98 reports at :43 and note :57: the 590 test count and "98 lookup reports" are duplicated changing totals. Keep the number in the verification row only. Line 104 suggested: "tests: `python3 -m unittest discover -s tests` (about 6-7 minutes; count in the verification row above)."

4. [should-fix] PROGRESS.md:42 / note :55-56. "no importer/catalog code changed" is stated as fact for why plain `unittest discover` suffices, but the same note says `campaign_state.py` changed and `relocate_catalog.py` rewrote catalog rows. Reword: "no importer or rebuild code changed; the new relocation code is covered by its own test". The note's phrasing "acceptable here because..." is a good place to say this too. Also state that the guarded runner was not used (it is currently only implied).

5. [should-fix] PROGRESS.md:123. "**c3b, chosen by the user 2026-10-10 and next (after the rename):**" is stale or ambiguous now the rename is done, and it is a bold label glued to the c3a item (item 2) so c3b has no own number. Suggested: "**c3b (next; chosen by the user 2026-10-10):**" and start it as item 3, renumbering c4, follow-ups and leads. Also add that the c3b pin must use catalog sha `4d525902…`, the post-rename sha (the note says this is why the rename came first).

6. [note] PROGRESS.md:7. "[session milestone note](research/campaign-c2-2026-10-05.md)" is the previous milestone. Either relabel to "[c2 milestone note]" or list the rename note first as the latest. Header also says "updated 2026-10-10" with the c2-era role line; fine, but one statement of who edited the rename is enough.

7. [note] PROGRESS.md:~100-103. `pin_inventory.py verify` text says it fails only on pinned accepted checkpoints "(live catalog/manifest moved with SmartNIC/QCT)". The catalog sha has since moved again via the rename (and `cache_path` changed). Append "and the 2026-10-10 rename".

8. [note] README.md:34-35 (relocation commands). Placeholders `OLD` and `DATE` are fine, but add the two constraints the review found: "the `--audit` file must be a new workspace-relative path with an existing parent directory; both scripts must run from the destination". The archive command also needs `--decision` text only; no `--to-root` needed since default is `.`. Also say the old absolute path of the example is `/home/erin/nepeat/aislop/fcc_shenanigans` only in history; no change needed.

9. [note] note :35-36. Reviewer caveat 3 (an existing-but-unreachable old root counts as gone, e.g. unmounted volume) is stated; add the operator instruction the reviewer asked for: "delete or keep no copy of the old tree; do not restore the old path while this authority is live".

10. [note] PROGRESS.md:147 "storage: `data/` gained ... 372 MB recovery catalog, one 367 MB rebuild" omits the new rename backup (`devices-before-rename-2026-10-10.sqlite`, same size class as the catalog) and `archive-registry-backup-2026-10-10/`. Add "plus the 2026-10-10 pre-rename catalog copy (compress through the history workflow when no longer needed)".

## Evidence boundary
No claims of new coverage, catalog evidence or live capture were added. "Byte-identical" claims (report, coverage validate) are supported by the acceptance outputs present only as post-move outputs; the pre-move outputs are not in the acceptance directory, so the comparison cannot be re-checked from files. Suggest noting that in the note ("pre-move outputs were compared in session, not retained") or copying them in.
