# campaign planning and cross-review

User assignment: consult Fable through the Claude Code CLI, prepare a plan for an
ongoing regulator/trade/other-source device-hunting campaign, and cross-review it
with Astra. Planning only; no collection, code migration or catalog promotion.

Coordinator and sole documentation editor: `/root` (Astra review role).
Fable: independent read-only planning consultation and subsequent cross-review
through `claude --model fable --effort high`; actual model/result metadata will be
recorded with the returned consultation. No further delegation or source requests.

Owned paths: this note, `research/batches/campaign-planning-2026-10-04/`,
`docs/campaign-plan.md`, and a planning pointer in `PROGRESS.md`.
All source captures, databases, manifests, exports and code remain read-only.

Inputs: current GOAL, PROGRESS, workflow and evidence policy; archive, capture,
derivation, lineage and source-adapter implementations and relevant tests/notes.
The previous archive milestones describe a finite pilot, not an unattended campaign.

Acceptance: grounded plan with bounded milestones, ownership, tests, storage and
evidence boundaries, trade-source feasibility gates, two distinct review records,
resolved disagreements and an exact next assignment. CLI failure or unavailable
Fable must be reported without silently substituting a different model.

Status: complete for planning. [Final plan](../docs/campaign-plan.md).

## consultation and review trail

The installed Claude Code CLI is version 2.1.289. The initial sandboxed request
failed with EAI_AGAIN before any model response. The approved network retry and
the second cross-review both returned successfully with actual model
`claude-fable-5-1`, high effort, no permission denials and no web searches.
Only Read/Glob/Grep tools were enabled; Fable did not write project files.
The user explicitly selected Fable, overriding the routine Sonnet worker preference.

- [Fable proposal](batches/campaign-planning-2026-10-04/fable-proposal.md)
- [Astra/coordinator review and dispositions](batches/campaign-planning-2026-10-04/astra-review.md)
- [Fable cross-review](batches/campaign-planning-2026-10-04/fable-cross-review.md)
- [reviewed input hashes](batches/campaign-planning-2026-10-04/cross-review-inputs.json)
- [submitted draft snapshot](batches/campaign-planning-2026-10-04/plan-submitted-to-fable.md)
- raw CLI results: [proposal](batches/campaign-planning-2026-10-04/fable-proposal-retry-result.json),
  [cross-review](batches/campaign-planning-2026-10-04/fable-cross-review-result.json);
  briefs and the failed initial result are retained in that directory.

Fable's final verdict: ready for c0 and the c1 design contract with specified
edits. The coordinator incorporated those edits and independently checked their
code basis. Final edits were not sent for a third Fable pass. Historical proposals
retain their original wording; corrected counts/designs live in the final plan
and disposition record, not in overwritten consultation transcripts.

The accepted direction is RRA + ISED, initially offline, capture/index correctness
before scheduling, one transactional operational store, selected evidence plus
coverage manifests, and trade feasibility before any trade adapter. No new
database product, mass historical review, automatic claim acceptance or unbounded
crawl is proposed. The next assignment is c0 input pinning plus the c1 state/test
contract; this turn did not start either implementation or source collection.

[Validation](batches/campaign-planning-2026-10-04/validation.json) checks current
document links, consultation model/results, submitted draft/review hashes and
the manifest's pilot-job counts. The live catalog still matches its accepted
SHA-256 checkpoint. Implementation tests were not run for this documentation-only
assignment; the plan's future acceptance tests remain requirements, not results.
