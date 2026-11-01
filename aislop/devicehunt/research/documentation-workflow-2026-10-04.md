# Documentation simplification milestone — 2026-10-04

The user approved subagent delegation and asked for concise goal/docs, an operating
architecture usable by future Astra/Fable agents, and ongoing documentation
simplification and updates as part of the goal. "Add a claude" was interpreted
as "add a clause" and that interpretation was stated before editing.

## Assigned ownership and outcome

| Owner | Scope | Outcome |
| --- | --- | --- |
| Coordinator | GOAL, PROGRESS, AGENTS, evidence policy, history snapshots/index, this note and audit | Replaced historical narratives with current instructions and linked details; archived originals before editing. |
| usage_docs | README only | Concise usage entrypoint; checked CLI help and guide links; kept seller-only evidence distinct from validation expectations. |
| workflow_docs | docs/agent-workflow.md only | Bounded task/handoff templates, path ownership, independent review, exclusive integration and required documentation upkeep. |
| docs_review | Read-only independent review | Preserved critical evidence/continuation rules; found and verified the fix to candidate/rebuild comparison instructions; final result: no findings. |

This was a documentation-only assignment. No workers were assigned to resume
collection, import the pending batch or finish the pending listing-code change.
The root app goal's older wording/paused status remains separately documented
in PROGRESS; GOAL.md contains the newly requested maintenance clause.

## Acceptance evidence

- [Archive manifest](../docs/history/2026-10-04-before-workflow-simplification/manifest.json)
  records byte-identical pre-edit snapshots and hashes. The archived agent file
  is named AGENTS.snapshot.md to avoid acting as live instructions.
- [Audit](documentation-workflow-audit-2026-10-04.json) verifies archive hashes,
  current documentation links/anchors, line counts, unchanged database SHA-256
  and both validation sets. All ten owned checks still pass; the two ByteDance
  examples remain gaps. No full code suite/rebuild was rerun for document edits.
- The reviewer caught a real continuation risk: the comparison script defaults
  to the old live database. Workflow instructions now explicitly compare the
  clean rebuild with the candidate, and separately audit preservation of old data.
- Git reports the project as untracked, so git diff alone cannot validate these
  documents; direct whitespace, link and archive-hash checks were used.

This note is a dated milestone record. Current instructions and next work belong
in [PROGRESS](../PROGRESS.md); do not copy this narrative back into live entrypoints.
