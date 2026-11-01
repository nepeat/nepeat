# agent workflow

This is the operating guide for future agents, including Astra or Fable, without
requiring chat history. On 2026-10-04 the user authorized bounded subagent
delegation and the documentation cleanup described here. This authorization does
not itself resume a paused collection goal or authorize external actions.
Read [AGENTS.md](../AGENTS.md) for project rules and
[PROGRESS.md](../PROGRESS.md) for the current handoff;
[GOAL.md](../GOAL.md) defines the durable objective. Those rules apply to every worker.

## roles and capacity

The coordinator owns priorities, scope, acceptance decisions and user updates.
Delegate execution with clear ownership; keep compact conclusions and evidence
paths in the main context. Keep long logs in task artifacts.

This session allowed one coordinator plus three workers. Check the available
slots on each future host. The user's worker-model preference is **GPT-6.1 Sol**
on ChatGPT (`gpt-6.1-sol` here), or **latest available Sonnet** on Claude (Sonnet
5.5 is the user's 2026-10-04 baseline, not a permanently pinned "latest" claim).
For Sol workers, use high reasoning effort (the user's 2026-10-04 preference is
medium-high; the tool exposes medium and high separately). Set it explicitly
when spawning a fresh worker; do not rely on the low default.
Verify model availability on the current host; report an unavailable requested
model instead of silently substituting. This preference applies to subagents,
not the coordinator. The workflow needs no permanent worker service.
If subagents are unavailable, perform the same stages sequentially.

An example allocation is two researchers and one engineer:

| Role | Deliverable | Write boundary |
| --- | --- | --- |
| Research A | One finite source group, captures, hashes, assertions and exclusions | Its own new capture directory and research note |
| Research B | A different finite source group with the same evidence contract | A separate new capture directory and research note |
| Engineer | One bounded code change and meaningful affected tests | Named modules and tests only |
| Independent reviewer | Evidence and/or code findings with proof paths | Assigned review report only |
| Integrator | Accepted imports, verification, current catalog and exports | Exclusive ownership of the batch's shared state |

Rotate a freed worker slot into independent review, then appoint one integrator.
The reviewer must not have authored the work under review; use another worker
or the coordinator. Evidence and code can need different reviewers. Authors fix
findings in their own paths, and the reviewer checks the fixes. Integration
starts only after the coordinator accepts the handoffs and the code is stable.

## ownership before work

Record the active assignments and exact owned paths in a short milestone note
under `research/`; link it from PROGRESS. The coordinator appoints its editor.
Every changed file has one writer. A worker needing another owner's path returns
a dependency request; the coordinator transfers ownership or orders the work.
Do not resolve overlap by having both workers edit the same file.

Give each researcher a distinct new directory, for example
`data/raw/<batch>-<source-group>/`, and an individual research note. Preserve
original captures unchanged; record URLs, fetch outcomes, capture dates and
SHA-256 hashes. Existing captures may be read by all workers. `sources/` stays
read-only. Save long extraction output and logs with the assigned batch artifacts.

Never concurrently write the same database, import manifest, complete reference
PDF bundle or generated export. Researchers hand off candidate inputs, not live
imports. Engineers do not refresh shared catalog state. Tests that write fixtures
or candidate databases need distinct temporary paths. During integration, other
workers may read stable inputs or work on unrelated assigned paths; they must
not modify dependencies being verified. A separate checkout alone does not
protect shared absolute data paths.

## bounded assignments and handoffs

Start with fresh bounded context when supported (`fork_turns="none"`), supplying
the task brief and relevant file paths rather than the full conversation.
Include project instructions, source-specific notes, existing capture/failure
reports and relevant modules/tests. Workers must read the actual files.

Use this task brief:

```text
Task / question:
Read first: AGENTS.md, PROGRESS.md, relevant source note and code/tests
Owned write paths:
Read-only dependencies / other owners:
Finite scope: named sources or files; maximum pages/items/requests as applicable
Existing captures and known access failures:
Expected artifacts and evidence fields:
Acceptance checks:
Stop condition and exclusions:
Handoff destination:
```

Set limits before starting. Stop when the named source set or change is complete,
the agreed limit is reached, evidence is unavailable, or throttling/access controls
require stopping. Checkpoint useful partial work and explain the gap. Do not turn
new links into an expanding crawl, retry known denied items, or find another route
to the same denied item. Unavailable is distinct from empty. Return discoveries
outside scope as optional next work, without silently starting another batch.

Use this compact completion handoff:

```text
Outcome: complete / partial / blocked, with the specific reason
Files written and captures/hashes:
Supported exact identities, roles, practical facts and source/page references:
Excluded claims, conflicts, access failures and remaining gaps:
Checks run and actual results:
Dependencies or decisions needed for acceptance:
Next bounded step:
```

The coordinator resolves routine implementation choices and task dependencies.
Escalate only decisions that need the user: a material scope change, an unresolved
preference, destructive replacement, or an external action requiring authorization.
Do not ask again for already authorized local work. No seller/provider contact,
purchases, paid-data outreach, publication or repository push follows from this
workflow. Do not install tools or alter system configuration just to run the project.

## review and exclusive integration

Independent QA checks claims against captures, including visible PDF tables when
text extraction is ambiguous. Validate proposed PDF/text inputs with the existing
`reference_documents.prepare` / `documents.read_pages` checks before acceptance;
the text envelope must bind the PDF hash, extraction method and complete page list.
Use isolated preparation paths and leave live data read-only.
Preserve full suffixes, revisions and printed case;
page/table scope; explicit chassis/node relationships; conflicting statements;
and brand, manufacturer, applicant, grantee, listing-holder and operator roles.
Do not merge identities or infer roles from a familiar name. Discovery rows and
marketplace leads remain discovery. Keep owned validation's nine documents plus
one label photo separate from vendor examples and unresolved affiliation gaps.

Code review checks the intended behavior, source replacement transactions,
failure handling and meaningful affected tests. Follow AGENTS for schema routing,
read-only commands, offline tests and complete sibling PDF dependencies.

The coordinator names one exclusive integrator for these sequential steps:

1. Accept reviewed inputs and stable code; record outstanding exclusions.
2. Prepare candidate inputs, reviewed manifest entries and a complete PDF bundle
   with validated sibling filenames. Preserve originals and historical snapshots;
   verify reviewed hashes before registering accepted inputs in the import manifest.
3. Build a new candidate database. Separately audit preservation of existing
   records and roles from old live to candidate, then run affected tests.
   Run the full suite for schema/importer changes and
   before promoting a combined code/data batch.
4. Rebuild offline into another new path from `data/import-manifest.json`.
   Compare candidate versus clean rebuild for equality, explicitly selecting the
   candidate because the script defaults to the live database:

   `PYTHONPATH=. python3 scripts/compare_rebuild.py <clean-rebuild> --database <candidate> --output-prefix <batch>`

   Replace the first two placeholders with database paths and `<batch>` with a
   unique audit filename prefix. Check baseline, integrity, foreign keys, source
   counts and preservation according to AGENTS and the current handoff.
5. Promote only validated results through the project's existing workflow; update
   affected exports. `data/devices.sqlite` remains
   authoritative. Do not overwrite historical rebuilds to reclaim space.
6. Finish the documentation stewardship below and report actual coverage, limits,
   checks and the next finite step. A finished batch is not a complete archive.

## documentation stewardship at every milestone

The coordinator appoints one editor for current handoff documents and checks
their update before accepting the milestone. Simplification is ongoing work:
remove repetition and stale instructions while retaining evidence and useful
continuation details. Replace current status; do not append another history log.

| Document | Keep here |
| --- | --- |
| GOAL.md | Durable objective, scope and acceptance principles |
| PROGRESS.md | Latest verified checkpoint, pending work, gaps and exact next steps |
| README.md | Normal setup, commands and user-facing operation |
| AGENTS.md | Stable project rules and evidence/verification constraints |
| docs/agent-workflow.md | Ownership, delegation, review and integration procedure |
| docs/evidence-policy.md | Detailed evidence semantics and source-specific traps |
| research/ | Source details, milestone assignments, review findings and proof paths |
| docs/history/ | Preserved historical handoffs and milestones |

Archive unique historical detail before removing it from current documents; keep
clear links to the archive and relevant source notes. Do not duplicate changing
totals across documents. Label unverified or older results, distinguish pending
captures from imported evidence, and update commands when their inputs change.
Finish with a readable, self-contained handoff that another agent can act on.
