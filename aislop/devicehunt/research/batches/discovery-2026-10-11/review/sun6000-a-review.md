# SUN-6000 A (Kladno-only) implementation review (independent, Claude Sonnet 5.5) - 2026-10-11

Verdict: ACCEPT. No must-fix. Two should-fix notes (wording, not data semantics) and notes below. The reviewer did not author this change.

Method: read-only on devicehunt/ and data/ (mutants ran on a scratchpad copy of code/tests with `data` symlinked). No network. Nothing under `data/import-manifest.json`, `data/devices.sqlite` touched (mtimes still 2026-10-11 07:54:20).

## Checks

- Prototype applied unchanged: `patch -R -p1 --dry-run < prototype.diff` applies cleanly to `rra.py`, `rra_company_names.py`, `campaign_sources.py`, so the live files contain exactly the diff's hunks (and nothing else was added on top at those hunks).
- Data: wrapper sha `6daf8c28...54b7` and sibling bundle `2e32bff9...7fc` equal the design drafts byte for byte; `diff bundle.json bundle-composite-cells.json` is exactly the two-line `company_evidence` change; `bundle.json` sha `6aacf7a8...7022` equals its pin in `data/raw/rra-manufacturer-lists.json` captures[0] (untouched). Re-hashed `detail-090.html` (`3c01977f...870b`, matches entry); raw cell slice and text hashes are re-checked by tests/loader.
- Guards (code read + mutants): certificate + sha256(cell text) key; entry detail sha and url; exact text equality; `query_maker` == bundle query maker (Oracle query refuses with ValueError, not skipped); raw HTML slice re-hash through the pinned detail dependency; "cell must not already resolve by `company_target`"; entry matching no row refused; load-time exact reconstruction, role set `{registry_manufacturer}`, target Celestica, `name_evidence` base verified against the reviewed base both ways, duplicate refusal, wrong-base refusal.
- Kladno-only: Monterrey part is `role: null, target: null, name_evidence: unmapped_context`, loader refuses it if mappable or given a target/role; import builds affiliations only from parts with a role, so Monterrey is never an affiliation (it appears only as the as-printed cell fact and a `manufacturer_cell_part` fact). The Oracle parenthetical is a `manufacturer_cell_unlabelled_text` fact only; `target_for('(Oracle America, Inc.)')` is Oracle, so the test that it never becomes an affiliation is load-bearing and present. Affiliations of each new record in the queue item: Kladno -> Celestica `registry_manufacturer`, ORACLE AMERICA. INC -> Oracle `regulatory_applicant`. Nothing else.
- `claim_target` returns the first part with a target (Monterrey first and unmapped, so first-part-only would break; mutant killed).
- Isolation: the guard only fires for key (SUN-6000, cell hash) in the Celestica-query capture. Oracle bundle (still pins `bundle.json`/old evidence) keeps SUN-6000 raw-only. Queue: 11 items `no_changes`; I verified each one's baseline input hashes on disk, projection equal baseline/candidate, and for the Oracle item 896 unchanged records (no SUN-6000 record leaks in via the Oracle query), manufacturer-lists summary still 45 assertions (test).
- Celestica item: +5 records (`SUN-6000:6000`, `Netra 6000 AC`, `Netra 6000 Chassis`, `Oracle Sun Blade 6000 Chassis M2`, `Sun Blade 6000 Chassis`), 149 unchanged, 0 removed/changed/moved, raw rows identical (no added/removed/reordered), source fields changed: `coverage` text and `sha256` only. Conflicts kept: `repeated_certificate_distinct_devices` for the five devices and `cross_query_certificate_overlap` (`independent_corroboration: false`).

## Tests and mutants

`python3 -m unittest tests.test_rra_composite_cells tests.test_campaign_rra_live tests.test_review_queue`: Ran 77 tests, OK (325 s).

Mutants on a scratchpad copy of `rra.py`, run against `tests.test_rra_composite_cells`; all killed:
map Monterrey (context parts given `registry_manufacturer`) KILLED; drop cell-hash key guard KILLED; drop cell-hash key plus text equality KILLED; accept under Oracle query (drop query_maker check) KILLED (2 failures); drop raw-slice re-hash KILLED; drop detail-hash check KILLED; `claim_target` first-part-only KILLED; drop "entry matches no row" KILLED. Loader-side data mutants (Monterrey given a target/role/`base` evidence; Kladno declared as context; wrong base; duplicate entry) are covered by `RefusalTests`.

## Findings

Must-fix: none.

Should-fix (wording; do not block decisions, the per-record facts are accurate):
- S1: the source `coverage` text from `composite_coverage` says each entry "names ... its split parts as registry_manufacturer" and "Part names without reviewed legal-name evidence are scoped to that certificate only". Under Kladno-only, parts without reviewed evidence are unmapped context with no role, not mapped certificate-scoped. The per-record `manufacturer_cell_basis`/`manufacturer_cell_part` facts and the wrapper are correct; only this sentence overstates. Fix it in a future revision (it changes `rra_company_names.py`, code hashes, pins and the queue, so do it only if the coordinator accepts re-running the pipeline; otherwise record it as a known wording limit).
- S2: `manufacturer_cell_part` and the basis facts are repeated on all five records (11 facts each), by design; fine, but exports will show Monterrey only as text inside those facts.

Notes:
- N1: Kladno's basis is the saved archived Exhibit 21.1 reviewed search extract with unestablished period (pre-existing standard). Registry-manufacturer only; the 멕시코 country field is raw and unsplit, so no factory or country claim is made, and the five Celestica-brand identities (including bare `6000`) are RRA-literal strings distinct from Oracle-brand `Sun Blade 6000 Chassis`.
- N2: the user approved A with Monterrey mapped; Kladno-only narrows that and the user may override (noted in the wrapper `decision`).
- N3: `data/import-manifest-pinbase-sun6000-2026-10-11.json` must never be passed to `--accepted-manifest`.

## re-review and new-queue decisions

Re-review 2026-10-11 (same independent reviewer) after the coordinator applied should-fix S1 and re-ran the pipeline (new queue `data/campaign-reviews/coverage-b8ee5df9984de206d250cf144da8d40ac60535b6caaee2d84fc07506dfdcbacd/`, definition `39747f31...`). The old queue `coverage-0db21eac...` was not touched (its 12 decisions stay on the superseded queue).

Verdict: ACCEPT, no must-fix.

- Only code change: reverse-applying `prototype.diff` now succeeds for `rra.py` and `campaign_sources.py` and fails in exactly one `rra_company_names.py` hunk (the `composite_coverage` sentence); the other three hunks apply. A line-level comparison of the added lines in the diff against the current file shows only the three sentence lines differ. So the sentence is the only change since my first reading.
- New sentence, checked against the wrapper and loader under Kladno-only: "each keys one exact manufacturer cell (certificate plus cell text and hash, with its detail page hash) and applies to that certificate and cell only. A split part carries registry_manufacturer only where reviewed legal-name evidence for that name exists; every other part, and any trailing unlabelled parenthetical, is unmapped literal context with no role, target or affiliation. No factory, per-factory country or sole-manufacturer claim ..." This is accurate (Kladno mapped on base evidence; Monterrey and the Oracle parenthetical unmapped). S1 resolved.
- Tests: `tests.test_rra_composite_cells tests.test_campaign_rra_live tests.test_review_queue` Ran 77, OK (325 s).
- New queue: 12 items, all at definition `39747f31`; 11 `no_changes` (0 added/removed/changed/moved, baseline input hashes re-verified on disk). Celestica item `review-item-6fe3b47a8dad...`: +5 records, 149 unchanged, 0 removed/changed/moved, source fields changed `coverage` and `sha256` only; the five added records are byte-identical to the superseded queue's (Kladno -> Celestica `registry_manufacturer`, ORACLE AMERICA. INC -> Oracle `regulatory_applicant`); candidate bundle sha unchanged (`2e32bff9...`); the coverage text carries the new sentence; cross-query overlap conflict kept with `independent_corroboration: false`.
- Decisions: 12 accepted (11 no_changes, plus Celestica on its merits with the Kladno-only reason, counts, `registry_manufacturer`-only role and the wording-fix note) with reviewer "Claude Sonnet 5.5 independent reviewer (SUN-6000 A, 2026-10-11)", written to `.../coverage-b8ee5df9.../decisions/` (12 files). No release, manifest or catalog change (`data/import-manifest.json` and `data/devices.sqlite` mtimes still 07:54:20).
