# NIC / Wiwynn / Hyve batch — 2026-10-04

The user resumed the existing liquidation-hunting goal with bounded subagents.
All workers in this ChatGPT run use `gpt-6.1-sol`, explicitly requested by the user.
The Claude preference is latest Sonnet (user's current baseline: 5.5); future
hosts verify availability rather than treating the baseline as permanently latest.

## Assignment and ownership

| Owner | Owned writes | Deliverable / stop condition |
| --- | --- | --- |
| Coordinator | This note, GOAL, AGENTS, PROGRESS, workflow/evidence policy, history index and final source-note status pointers | Priorities, acceptance, code/evidence review, concise final handoff |
| inspur_nics | `data/curated-inspur-nics-2026-10-04.json`, `research/inspur-nics-2026-10-04.md`, optional batch `inspur/` scratch | Eight literal NIC identities and all reviewed rows from existing pp14–16; no new acquisition |
| wiwynn_hyve | `data/curated-wiwynn-hyve-2026-10-04.json`, `research/wiwynn-hyve-2026-10-04.md`, optional batch `wiwynn-hyve/` scratch | Reviewed facts from the five existing PDFs and four product pages; no new acquisition |
| listing_engineering | `devicehunt/listings.py`, relevant listing tests/guide, batch `engineering/` logs | Exact plus-suffix behavior, bounded matching, affected/full tests |
| batch_review | Batch `review.md` only | Confirm source scope, suffixes, company roles, conflicts and checks |
| batch_integration | Sole owner of bundle, manifest, candidate/rebuild, live database and exports after acceptance | Reproduce accepted facts, preserve prior data, compare and promote after checks |

Scratch and logs are under `research/batches/nics-wiwynn-hyve-2026-10-04/`.
Researchers may read shared captures/database but must not import or change them.
The engineer does not touch shared data. One integrator owns all shared mutations.
No worker may expand the finite source set or delegate further without assignment.

## Inputs and acceptance

Existing captures: `data/raw/nics-wiwynn-hyve-2026-10-04/` plus the previously
archived InCloud Rail PDF. [Detailed source traps](workflow-review-2026-10-04.md#pending-batch-nics-wiwynn-and-hyve)
include V versus V+, X722 conflict, Hyve Orion prose and unusual JBOF voltage.
All originals are immutable; Hyve Ambient 403 remains unavailable.

Before integration: require independent review, source/hash checks and stable
code. Import only new uniquely named sources. Append five PDFs/eight pages to
the complete reference bundle using validated sibling filenames. Preserve every
prior identity/assertion/raw row and both validation sets; no new manufacture
or deployment claim follows from these sources. Compare a clean offline rebuild
against the candidate, separately auditing preservation against old live.

Baseline and current proof paths are in PROGRESS. Completion requires updated
exports, tests/rebuild evidence and a short refreshed handoff. Detailed results
belong in this dated note and source reviews, not growing narratives in GOAL.

## Completed result

Independent evidence/code review passed after the author corrected one ST7000G2
HTML fact that had borrowed PDF detail. Accepted facts and stable code are bound
by `acceptance.json`, `review.md` and `stable-code-checkpoint.json` under the batch
artifact directory. All workers used GPT-6.1 Sol; no new acquisition was needed.

Added 19 identities / 25 source records: eight Inspur NIC models, eight Wiwynn
identities and three Hyve identities. Five original PDFs/eight pages are appended
to the complete collection. Live catalog: 4,083 identities / 5,298 records /
278 sources; 55 PDFs / 3,649 pages. Manufacturer/factory counts, 22 hyperscaler
assertions, owned 10/10 with 9+1 evidence and ByteDance 0/2 gaps are unchanged.

The plus-suffix matcher distinguishes V/V+, preserves source-scoped seller and
alias evidence, and gives unknown variants no borrowed specs. Checks passed:
23 affected / 411 full Python / 6 Node tests; a 116-job rebuild from 2,171 hashed
inputs equals the candidate across 43 tables/indexes. Prior rows and duplicate
multiplicities passed preservation checks. Actual candidate/final CLI and DOM
checks passed. All 98 saved lookups and current reports were refreshed.

Final live SHA-256:
`ea7d121849c34cac801c178d48be80ea5f3af09958b526228b6b6f536d0662d8`.
See [final integration audit](../data/exports/nics-wiwynn-hyve-2026-10-04-final-verification.json),
[batch artifacts](batches/nics-wiwynn-hyve-2026-10-04/) and the current paths in
[PROGRESS](../PROGRESS.md). The one batch driver reuses existing import/rebuild
and export tools; do not blindly rerun its historical mutation phases.

Example read-only queries (using the runtime in README):

```sh
python3 -m devicehunt search E810_2Port_LC_PCIe --vendor Inspur
python3 -m devicehunt specs SV7220G3-V+ --vendor Wiwynn --exact-model
python3 -m devicehunt match-listing --title 'Wiwynn SV7220G3-V+ bare node' --vendor Wiwynn --with-specs
```

The listing title is illustrative, not a collected offer. Exact specification
statements remain historical source claims and do not identify an auction unit's
installed parts or software entitlement. Coverage remains incomplete; next bounded
work and documentation upkeep are in PROGRESS, rather than duplicated here.
