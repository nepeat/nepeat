ACCEPT with notes: no blockers. PROGRESS is accurate and its evidence boundaries hold. Findings are stale statements in older docs (N1-N5) and handoff gaps (N6-N8).

Reviewer: Claude Sonnet 5.5, read-only except this file. No network, no full suite.

## Verified
- Links: scripted check of every relative markdown link in the 10 edited files plus GOAL, workflow, evidence-policy and the snapshot README. All resolve. The archived snapshot PROGRESS.md has 25 unresolvable links, because they are rooted at the project dir. That is documented in docs/history/README.md and exempt.
- Live DB (`mode=ro`): devices 4,091; records 5,323; sources 297; facts 45,073; documents 61; document_pages 3,716; user_version 29; sha prefix b66dc555; integrity ok; FK check empty; archive_artifacts 146; archive_receipts 374; evidence_statements 4; hyperscaler 22.
- Manifest: 122 imports / 2,280 files.
- `vendor-inventory --limit 0`: manufacturer scope 2,142 pairs / 2,141 identities; all scope 4,228 / 4,090. Both match PROGRESS.
- post-promotion-proof.json, c2-acceptance.json, live-campaign.json, live-upgrade.json and protected-final.json all match PROGRESS:
  - 562 tests; 52-table rebuild equality; 12/12 validated with 2 explicit zeros; 4,782,388 B growth against the 64 MiB cap;
  - second run reused 36 jobs with 0 B; 4 restriction events; 0 accounts, activations and leases;
  - protected-final changed only devices.sqlite and import-manifest.json.
- Live operational DB: meta version 3, state `ready`, 2 runs / 36 jobs / 72 executions. index-head.json sequence 1181.
- validate-inventory: owned baseline 10/10; vendor-examples 0/2 (B4020 gap, B4010 lead-only, not evidence).
- CLI surface (plan/run --offline/status/resume RUN_ID --decision/report) matches README.
- Evidence boundaries intact: candidate bundles are "not catalog evidence"; coverage is "per declared query"; C5000X and QCT-to-Quanta are not aliases; the TPM 75W row stays unclassified; ByteDance stays a gap.
- Old counts (4,090 as a device count, 45,017, 58 PDFs, 121 jobs) appear in current docs only as rebase/history context.

## Findings (suggested exact replacements)

N1 docs/vendor-inventory.md:75-76 (stale totals, the only current-doc contradiction of the new state). Replace with:
"Current pair/identity totals for each scope are in [PROGRESS](../PROGRESS.md); `python3 -m devicehunt vendor-inventory --scope manufacturer --limit 0` prints them for the opened database. A device associated with several target companies appears under each."
Then drop the 2,134 / 2,133 / 3,971 / 3,837 sentence. The rest of the paragraph, on factory versus manufacturer not being additive, stays.

N2 docs/campaign-plan.md:7-8. The sentence "This is a plan, not implemented campaign functionality or authorization for an unbounded crawl." now contradicts line 9. Replace with:
"This began as a plan; c0, c1 and offline c2 are implemented (see the status line below). It is not authorization for an unbounded crawl, and live collection (c2b/c4) is not implemented."
Also line 46: "Proposed commands are" becomes "Commands are"; line 200: "(proposed)" becomes "(implemented in c2)".

N3 docs/campaign-plan.md:339-341. "SmartNIC/QCT integration remains a separate pending batch and must not compete for the manifest/catalog integration lock." is stale. Replace with:
"SmartNIC/QCT integration was a separate batch and was promoted 2026-10-05 (see [PROGRESS](../PROGRESS.md)); future batches must likewise not compete for the manifest/catalog integration lock."

N4 docs/campaign-state-contract.md:6-7, 45 and 248-251 (stale "not implemented" wording).
- Lines 6-7: "this file specifies behavior, not implemented functionality" becomes "this file specifies behavior; c1 and offline c2 implement the parts marked below, and live-transport gates remain open".
- Line 45: "index-head.json (proposed)" becomes "index-head.json (implemented; present in the live archive)".
- Lines 250-251: "These names describe responsibilities, not an existing API." becomes "Offline c2 implements these responsibilities in `devicehunt/campaign_sources.py` (`SavedAdapter`, `RraSavedAdapter`, `IsedSavedAdapter`) and `devicehunt/campaign.py` (`Runner`); the live-descriptor part (`next_work` through shared capture) is c2b and unimplemented."

N5 docs/handoff-AGENTS.md:47 says "See PROGRESS for the working bundled Python path if macOS system Python is blocked by the Xcode license." PROGRESS no longer has that path, and the host is now Linux. The file is a historical copy that README does not reference. Either prepend "Historical (macOS-era); current runtime is in README" or replace the sentence with "Use the project runtime named in README." Low priority. docs/model-fragment-search.md:63-64 ("3,522 identities... 329 tests") is likewise a dated milestone figure; add "(at that milestone)".

N6 PROGRESS.md:12-17 and :99. "no relocation command exists yet" is ambiguous: `scripts/relocate_catalog.py` exists and rewrites only `sources.cache_path`. Replace line 16 with:
"no relocation command exists yet for the archive authority (`scripts/relocate_catalog.py` only rewrites catalog `sources.cache_path`, not `data/archive/`)."

N7 PROGRESS.md:71 and :110 (precision).
- `pin_inventory.py` has no path. Replace with: "`python3 research/batches/campaign-pilot-2026-10-04/inventory/pin_inventory.py verify`".
- "366 MB rebuild" measures 367.3 MB on disk; use "367 MB". The recovery catalog is 371.6 MB, so "372 MB" is right.
- Line 28 could add: "the live archive now holds 1,555 receipts (374 release-linked plus campaign receipts); only the 374 are in the accepted release."

N8 PROGRESS "next bounded work" gaps for a fresh agent:
- No test command appears anywhere in README, AGENTS or PROGRESS. `tests/` has no `__init__.py` and uses unittest-style modules. Add: "tests: `python3 -m unittest discover -s tests -p 'test_*.py'`; affected-only: `python3 -m unittest tests.test_campaign tests.test_campaign_state` (562 total, about 430 s)". I did not run this.
- c2b: add file pointers. Work lands in `devicehunt/campaign_sources.py`; `Collector.capture` and `register_saved` are in `devicehunt/capture.py` (c1 review cites capture.py:440-464 for B1). The contract's gate A1 is row A1 in the test table at docs/campaign-state-contract.md:307.
- c3: "re-pin accepted checkpoints in a new definition revision" should name the file: `research/batches/campaign-pilot-2026-10-04/inventory/campaign-definition.json` (DEFAULT_DEFINITION in `devicehunt/campaign.py`) and `inventory.json`. Per the contract, changing a pin creates a new revision; never edit the old one.
- Add that the c2 review's open notes are in [c2-runner-review.md](research/batches/campaign-c2-2026-10-05/review/c2-runner-review.md); line 73-76 lists them without the link.
- Note once in PROGRESS that `git status` shows this workspace untracked in the parent repo, so recovery relies on the listed `data/*-before-*` files rather than git history.

## Minor, no action needed
- research/smartnic-qct-batch-2026-10-04.md:3-9: the status is current; the original staging text ("remain read-only until candidate acceptance", "All workers use GPT-6.1 Sol") is correctly labelled as the original plan. Optionally add "(historical)" to those paragraphs.
- README.md:143-148: accurate. The claim that `plan`/`status`/`report` are read-only was not independently proven; I ran `status` and `report` once and saw no errors, but did not diff the archive.
