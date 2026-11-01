# campaign pilot: input pinning and durable-state contract

User authorization: "Continue, lets go with it" after the Fable + Astra campaign
plan and its next assignment, c0 plus the c1 design/test contract.
Read [the plan](../docs/campaign-plan.md), [current state](../PROGRESS.md),
[workflow](../docs/agent-workflow.md) and [evidence policy](../docs/evidence-policy.md).

Coordinator/documentation editor: `/root`. All work is offline. Accepted catalog,
import manifest, archive release, source captures and generated exports remain
read-only. No interference with pending SmartNIC/QCT integration.

## ownership

- `/root`: this note, PROGRESS, campaign plan status, and
  `docs/campaign-state-contract.md`, batch `acceptance/` proofs; prioritization
  and final acceptance. The originally reviewed campaign plan remains unchanged.
- `campaign_inventory`: `research/batches/campaign-pilot-2026-10-04/inventory/`
  only; reproducible pinning script, inventory/definition and source expectations.
- `campaign_contract_review`: assigned review report under this batch after the
  draft contract is ready; read-only source/code review, no implementation edits.

Workers use GPT-6.1 Sol with explicit high effort. No further delegation.
Each worker uses actual saved files; original capture times/versions that are
unknown stay unknown. This batch pins the seven current `rra` and five `ised`
manifest jobs and all their declared dependencies, without widening collection.

## acceptance and stop

Hash-verified finite inventory, source-specific validation, explicit replay and
coverage expectations, recorded source restrictions/unknowns, capacity policy,
durable budget/lease/index recovery contract and meaningful fault/test matrix.
Independent reviewer must resolve blockers before accepting c1 implementation.
Keep current data hashes unchanged. Next phase ownership is assigned separately
after this gate; no automatic live collection follows from a passing design review.

Status: c0 complete; contract accepted for a separately assigned c1a/c1b
implementation. No unresolved design blockers. This is not implementation or
live-collection acceptance.

## accepted outputs

- [Finite inventory and findings](batches/campaign-pilot-2026-10-04/inventory/findings.md),
  [inventory JSON](batches/campaign-pilot-2026-10-04/inventory/inventory.json),
  [offline definition](batches/campaign-pilot-2026-10-04/inventory/campaign-definition.json)
  and [pin/verify helper](batches/campaign-pilot-2026-10-04/inventory/pin_inventory.py).
- [Durable-state contract](../docs/campaign-state-contract.md): admission and
  quotas, status-before-body restriction persistence, clocks/locks, migration,
  indexed publication/recovery, stable receipt reuse, session metadata and later
  adapter/review/coverage boundaries. Test gates are assigned to c1a/c1b/c2/c3.
- [Independent review](batches/campaign-pilot-2026-10-04/contract-review.md) and
  [coordinator's network-disabled checks](batches/campaign-pilot-2026-10-04/acceptance/coordinator-checks.json).

The final exact inventory hash is
`0b891f166c67093515fd7dc6ebe4c41281a2db5501aeb20aa5e4b6ee2083629a`.
Its 12 source jobs account for 1,121 paths / 1,093 distinct byte hashes, registered
as an inventory in place with zero source/DB copies. This does not mean the future
campaign adapter has registered them in the live archive. 27 finite review
selectors include source zeros, discrepancies, duplicates, separate roles and
four existing accepted Golduck lineage statements. Current counts and proof links
remain distinct from claims of upstream completeness.

The helper verifies every pinned dependency, rejects bad/missing hashes and paths,
and publishes immutable outputs atomically with exact-byte reuse. Earlier draft
inventories remain under its owned drafts directory. Independent builds and an
idempotent CLI rerun agree. Eighteen small fixture checks pass. The coordinator
ran read-only verify twice and all fixtures with socket/HTTP/subprocess network
entrypoints forbidden; zero attempted network calls, protected hashes unchanged.
No application code or accepted data changed; no full suite/rebuild was required
or claimed. The c1 fault matrix is a future requirement, not tests already passed.

Known denied resources and the failed ISED query remain explicit. Exact legacy
capture times/versions, current upstream access/throttle state, full regulator
totals and missing historical failure reports remain unknown. Offline quota is
zero, new output cap 64 MiB, free-space floor 2 GiB; exceeding a cap stops without
dropping dependencies or deleting history.

## exact next implementation assignment

**c1a: durable admission and restriction state**, one engineer owning proposed
`devicehunt/campaign_state.py`, `devicehunt/capture.py` and their tests. Extend the
existing lease store transactionally in isolated fixtures; implement explicit
budget accounts/reservations, owner fences, metadata filtering, and header-status
callbacks that save restrictions before risky body processing. Preserve legacy
rows and receipt semantics. No instance-local live-budget bypass may remain.

Pass contract Q1–Q3, R1–R2, L1, M1 and P1 with multi-process, fake-clock and
interruption tests plus affected existing tests. Independent review gates code
acceptance. c1b then supplies the complete indexed publication/recovery protocol;
live transport stays disabled until both phases pass. Keep actual acquisition
state and the catalog read-only until their separately named migration/integration
gate. Pending SmartNIC/QCT integration remains distinct.
