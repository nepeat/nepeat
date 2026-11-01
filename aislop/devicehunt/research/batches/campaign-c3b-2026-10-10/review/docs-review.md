# Documentation review: c3b (review queue, release, import gate)

VERDICT: ACCEPT with notes (no must-fix). Reviewer: Claude Sonnet 5.5, one pass, read-only (the `campaign status` CLI and `review_queue --help` were run; no tests, rebuilds or campaign runs).

## Verified against files (all matched)
- `sha256sum data/devices.sqlite` = `4d525902cba5…`; "catalog unchanged" holds, and it equals the sha pinned in the c3b note (:58) and PROGRESS:31. `devices-before-rename-2026-10-10.sqlite` has the same byte size, as expected for the rename-only rewrite.
- `data/import-manifest.json`: keys include `campaign_gate` (`release` -> `campaign-releases/campaign-release-2a5b2f0c…json`, `coverage_manifest` -> `coverage/rra-ised-saved-2026-10-04/coverage-d2bc5c40…json`), 122 `imports`, 2,282 `files`; sha `5ddb1579…`. The pre-gate copy has 122 imports, 2,280 files, no `campaign_gate`, sha `984624e6…` (note:58 "98462…" is consistent). "2,280 + the release and its coverage manifest" is right.
- Release: 12 entries, all `accepted`; definition sha `0f095a42…` matches `campaign status` and DEFAULT_*. `devicehunt/campaign.py:27-28` DEFAULT_DEFINITION / DEFAULT_INVENTORY point at `research/batches/campaign-c3b-2026-10-10/inventory/`, as the note (:59) and PROGRESS:70 say.
- Queue dir: 12 items, 12 decisions. `acceptance/queue.json`: all `no_changes`; accton ISED has 51 conflicts (36 repeated + 15 html/csv), which matches live-decisions.md. `freeze.json`: 12 units, 2 explicit zero, 36 receipts, 1,117 artifacts. `live-run.json`: run `run-61cc2607…`, 36 executed, 0 network, `new_bytes` 1,388,097 (1.39 MB). `live-gate-negative.txt`: tampered input and dropped job refused, as described. `gated-compare.log`: "All 52 tables/indexes and baseline results match", rc 0. Counts in `gated-rebuild.log`: 4,091 devices / 5,323 records / 297 sources / 61 documents / 3,716 pages / 2,282 verified files, matching PROGRESS:30-34.
- Tests: `c3b-final-suite-result.json` and `c3b-final-suite.log` both say 602 tests, 0 failures, 0 blocked calls. `tests/test_review_queue.py` has 12 `test_` methods (note:48) and `AcceptedManifestGateTests` exists (PROGRESS:132).
- Archive: `index-head.json` sequence 1217 and 1,591 receipt files (PROGRESS:76-77).
- SUN-6000: item ids and the cross-query flag in `sun-6000-overlap-check.txt` match note:73. Live catalog `rra_equipment` holds two rows with certificate `SUN-6000` (row 90 in the Celestica query, row 215 in the Oracle query), both with the composite manufacturer string.
- Link check over PROGRESS.md, README.md and the c3b note: every relative link resolves. CLI `review_queue --help`: subcommands `queue|show|decide|release|check-release` match PROGRESS:128.
- Not verified: the `pin_inventory.py verify` behaviour claims (PROGRESS:95-98) were not run.

## Findings

1. [should-fix] README.md:178. "the reviewer must not be the author of the change under review." reads as enforced. `review_queue.decide` only requires a non-empty reviewer string and reason (`review_queue.py:361`; CLI help says nothing else). Suggested: "the reviewer must not be the author of the change under review (a procedural rule: the CLI records the reviewer name but does not check independence)."

2. [should-fix] research/campaign-c3b-2026-10-10.md:86-89 and PROGRESS.md:149-152. "composite cells stay raw-only, so no record exists" blurs two things. The two SUN-6000 raw rows exist in `rra_equipment`; what is absent is a device/manufacturer record for certificate `SUN-6000`, and `review/live-decisions.md` states the cause as an open importer question, not settled design. Suggested for the note: "...so the two raw `rra_equipment` rows (Celestica row 90, Oracle row 215) exist but no device record carries certificate `SUN-6000`; why the importer leaves them unmatched is not yet confirmed (live-decisions.md observation 2)." Suggested for PROGRESS:150-151: "composite cells stay raw-only; the two raw rows exist but no device record, cause unconfirmed."

3. [should-fix] research/campaign-c3b-2026-10-10.md:70-73 and `review/live-decisions.md` (observation 1). The note correctly corrects the reviewer's "flag absent from the items" error, but the correction lives only in the note and live-decisions.md keeps the wrong sentence (reviewer-owned, so do not edit it). Add to note:73: "The reviewer's report is left unedited; its statement that the queue items omit the flag is wrong (see the check output), and the same observation 1 also says the coverage manifest omits it, which was not re-checked here." (Drop the last clause if the coordinator re-checked it.)

4. [should-fix] Duplicated changing totals. "602 tests" appears at PROGRESS.md:42 and note:80; "122 jobs" at PROGRESS:41 and :58; "52 tables/indexes" at PROGRESS:41 and note:77; "12 tests" at note:48. Keep the number in PROGRESS's verification and manifest rows only. Note:80 suggested: "**Full suite**: passed under the guarded offline runner with 0 blocked calls (count and proof in PROGRESS verification row)." PROGRESS:58 suggested: "accepted manifest | `data/import-manifest.json` (gated by the c3b release)". Also PROGRESS uses "jobs" while the manifest key is `imports` (both 122); say "122 import jobs" once.

5. [should-fix] PROGRESS.md:42 and note:80-81. The proof JSON lives in `research/batches/campaign-c2-2026-10-05/acceptance/` (the guarded runner's home) while the c3b log is in `campaign-c3b-2026-10-10/acceptance/c3b-final-suite.log`. Add the second link so the evidence is findable from the c3b directory: "([proof](...c3b-final-suite-result.json), [log](research/batches/campaign-c3b-2026-10-10/acceptance/c3b-final-suite.log))".

6. [note] PROGRESS.md:63. The rebuild row reads "latest verified rebuild | c3b-gate (superseded: after-rename...; smartnic... has old cache paths)": "latest" and "superseded" in one cell is confusing. Suggested: "latest verified rebuild | `data/rebuilt-c3b-gate-2026-10-10.sqlite`; earlier, superseded: `rebuilt-after-rename-2026-10-10.sqlite` (pre-gate manifest) and `rebuilt-with-smartnic-qct-2026-10-05.sqlite` (old cache paths)".

7. [note] README.md:167-176. The `queue` example omits `--accepted-manifest`, `--catalog`, `--definition`, `--inventory` (defaults exist) and `check-release` is not mentioned. Add after the code block: "`check-release RELEASE` re-validates a release file; `queue` defaults to the live catalog and `data/import-manifest.json`." Also the campaign section above (README:139-159) says nothing about the c3b default definition now being the pinned c3b revision; one sentence "the default definition/inventory are the c3b pins under `research/batches/campaign-c3b-2026-10-10/inventory/`" would stop surprise.

8. [note] note:58. "manifest `98462…`" is a 5-character prefix while every other sha uses 8. Use `984624e6…`.

9. [note] `acceptance/live-run.err` exists, is empty and is referenced nowhere. Either mention it ("empty stderr of the live run") in the note's live-run bullet or ignore.

10. [note] PROGRESS.md:129-130 "the catalog is unchanged" is supported by the sha and by `gated-compare.log` (rebuild equals live); the note's own Evidence statement (:82) is the precise one and PROGRESS can link it. The independence of "independent reviewer" is another Sonnet 5.5 instance; the docs say so, which is accurate and sufficient.

## Evidence boundary
No catalog claims, coverage claims or live-capture claims were added. Docs say "saved snapshots only", "not regulator-wide", "zero new catalog claims" and `independent_corroboration: false` for SUN-6000. Open code-review notes (opt-in gate, `decide` race, unchecked bundle sha) are carried in both PROGRESS and the note. Only finding 2 touches evidence wording, and it tightens rather than loosens.
