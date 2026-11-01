# SmartNIC, QCT and lookup refresh — 2026-10-04

Status: **promoted 2026-10-05** by integrator `/root-claude` after rebasing onto
the schema-29/121-job catalog, a 52-table clean-rebuild match and an independent
[candidate review](batches/smartnic-qct-2026-10-04/integration/candidate-review.md)
(ACCEPT with notes); [post-promotion proof](batches/smartnic-qct-2026-10-04/integration/post-promotion-proof.json).
Text below records the original 2026-10-04 staging plan. The previous Inventec milestone made verified progress.
Live data and current exports remain read-only until candidate acceptance.
All workers use GPT-6.1 Sol with fresh
briefs; no further delegation. Coordinator owns acceptance. `smartnic_qct_docs`
owns current handoff documents during integration; final independent review closes them.

| Worker | Finite task | Exclusive writable paths |
| --- | --- | --- |
| `smartnic_research` | Exact Inventec FPGA SmartNIC C5020X specifications; at most three queries and one new capture | `data/raw/smartnic-qct-2026-10-04/smartnic/`; batch `smartnic/` |
| `t42s_research` | Exact QCT QuantaPlex T42S-2U specifications; same limits | Corresponding `t42s/` directories |
| `lookup_engineer` | Profile lookup refresh and make one measured optimization preserving complete output semantics | `devicehunt/lookup.py`, `scripts/refresh_exports.py`, `tests/test_lookup.py`, new `tests/test_lookup_batch.py`; batch `engineering/` |
| Coordinator | Reserve third capture for a concrete source lead; review and appoint independent QA/integration | Batch root proofs and current documentation |
| `smartnic_qct_integration` | Accepted inputs, candidate and clean rebuild; stop before live promotion | Batch `integration/`, named new data/backup/candidate/rebuild files in acceptance brief, import manifest, complete new reference bundle and byte-identical sibling dependencies; batch-prefixed staged exports |
| `smartnic_qct_docs` | Simplify current handoff and document verified results after promotion | PROGRESS.md, README.md, docs/buying-reports.md, this milestone note; batch docs-handoff.md |

Batch artifacts: `research/batches/smartnic-qct-2026-10-04/`. Total research limit
is six queries and three captures. Inspect saved captures/failures first; no paid
access, outreach, denied-resource retry or invented path to restricted material.
Preserve exact names and company roles. C5000X is not an assumed C5020X alias;
QCT/Quanta relationships stay separate from physical manufacture. Existing WSSD
configuration claims do not define all platform capabilities or node/chassis aliases.

The reserved third capture is assigned to the English C5020X file/83 lead from
the saved Inventec download index. The Chinese SmartNIC brief says English governs;
the index calls this other brief FPGA IPU C5020X. Preserve any actual heading,
version and specification differences as separate evidence, without inferring an
alias or treating an unrelated edition as the governing translation. Chinese
source/proof bytes stay intact; English work is an additive supplement.

Researchers return source captures, hashes, literal/page provenance, curated
proposals and explicit gaps. A PDF entry proposal is never a replacement for the
complete reference bundle. Stop at the named task/budget; return other leads only.

Engineering uses isolated outputs and before-code copies. Keep single-query
matching semantics, all datasets, duplicates, Unicode casefold, punctuation,
limits, ordering, state/metadata and caller transaction ownership. No persistent
search index, schema change or stale cache. Benchmark against all 98 saved reports
on the unchanged live snapshot, require exact equality and meaningful regression
tests, then run the full offline suite. Do not refresh shared exports during work.
If no useful safe speedup is demonstrated, retain the evidence and report that.

Independent source/code review precedes one designated integrator's candidate,
new offline rebuild and preservation checks. Promotion needs separate acceptance;
documentation review closes the milestone. [PROGRESS](../PROGRESS.md) holds the
verified live checkpoint; [workflow](../docs/agent-workflow.md) defines the gates.

## Accepted source and code gates

[Source review](batches/smartnic-qct-2026-10-04/source-review.md) accepted four
records/56 facts and three PDFs/53 pages. The English IPU is a separate printed
identity; its `TPM | 75W` row remains unresolved/unclassified. The Chinese brief's
reference-only/English-governs limitation remains. QCT portfolio specifications
and older QxVDI solution claims stay separate, with node/system and consumption
qualifiers. No new manufacture or deployment assertion is inferred.

[Code review](batches/smartnic-qct-2026-10-04/code-review.md) accepted streaming
batch lookup, with all 98 reports byte-identical on the unchanged starting
snapshot, 14 affected tests and 433 full-suite tests passing. Lookup computation
measured 176.17 to 13.81 seconds; this is not a whole-export timing claim.
The [acceptance contract](batches/smartnic-qct-2026-10-04/acceptance.json) freezes
source/code hashes and preservation requirements before staging.
