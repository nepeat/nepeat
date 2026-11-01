# artifact archive planning handoff

date: 2026-10-04. scope: summarize the user's archive/pipeline discussion and
prepare a complete implementation handoff. this is documentation work; the
proposed archive architecture is not implemented by this batch.

coordinator and documentation editor: `/root` in the archive-planning chat.
owned writes: this note, `docs/artifact-archive-plan.md`, and a narrowly scoped
link in `PROGRESS.md`. preserve all unrelated current-work text.

independent reviewer: `/root/archive_handoff_review`, a fresh GPT-6.1 Sol worker.
reviewer owns only `research/artifact-archive-handoff-review-2026-10-04.md`.
all code, captures, databases, manifests, complete PDF bundles and exports are
read-only for both participants. no collection, imports, package installation,
publication, commits or pushes are part of this handoff.

inputs: `AGENTS.md`, `GOAL.md`, `PROGRESS.md`, `docs/agent-workflow.md`,
`docs/evidence-policy.md`, the current catalog/document/fetch/rebuild code, and
the archive discussion in this chat. external tool documentation was checked
during that discussion; the plan carries primary-source links for future checks.

acceptance: a self-contained plan distinguishes inspected behavior from proposed
behavior; explains authority, metadata, capture history, derivation reuse and
evidence boundaries; provides bounded milestones and meaningful acceptance
checks; preserves sibling-file validation and active integration ownership; has
working local links and an independent review. stop after documentation is
verified. implementation is a subsequent assignment.

status: complete. [independent review](artifact-archive-handoff-review-2026-10-04.md)
passed with no blocking findings. coordinator verified local links, whitespace and
code fences; reviewer checked code descriptions, evidence boundaries, migration,
authority and acceptance criteria. no application tests or database checks were
run for this documentation-only change. the plan is linked from PROGRESS.

next: after implementation is assigned, perform milestone 0 in the plan: pin a
finite offline reference-PDF pilot and finalize the archive/catalog lineage split.
