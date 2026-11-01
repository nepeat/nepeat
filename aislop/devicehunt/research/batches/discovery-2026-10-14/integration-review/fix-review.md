# integration helper fixes — accepted

**accept this exact helper revision for use with a separately reviewed concrete
accepted-input plan.** Reviewer `/root/odm_bulk_full`. No remaining blocking
findings in the helper. Accepted `integration-breadth/stage.py` SHA-256:

`e795eac0ec21a9315adfc13d0f18d45515ffdd9764a558a127747627ad86e6cc`.

This supersedes the helper rejection in [the original review](review.md) only for
this new revision. Original review/probes/snapshot and their manifest remain
unchanged. [Fixed source snapshot](stage-fixed-reviewed.py),
[fix probe code](probe_fixes.py), [results](fix-probes.json).

All three blocking findings are resolved:

1. Live → candidate preservation now compares every old non-FTS column and row
   multiplicity. The original counterexample, where only old timestamps/cache path
   change, is detected by the exact new column selection and preservation SQL.
2. `clone` requires the accepted hash before and after copying. Both source changes
   before copying and changes during copying are rejected. Every original and
   staged candidate/dependency has a pinned expected hash; missing sidecar hashes
   are rejected, while matching explicit/dependency hashes work. The initial parsed
   plan bytes are hash-pinned, reviews are pinned with source inputs, and original,
   staged and plan/review hashes are checked again before the success report.
3. Group names must be unique lowercase slugs. Candidate/review/dependency paths
   and resolved destinations remain confined; candidate/dependency output
   collisions are rejected. All groups/counts/dependencies/destinations are
   preflighted before the first staged write. The report destination must also be
   absent. Group source/record/fact counts must match the plan.

**Ten isolated fix probes passed, zero failures/errors.** They execute only the
actual extracted pure helpers and preflight-loop AST with review-owned fixtures,
plus the actual new column-selection expression/SQL and final guard inspection.
They cover valid clone, changes before/during clone, full-column preservation,
valid preflight with zero staging writes, unsafe/duplicate names, missing versus
explicit sidecar hashes, hashed sidecar dependencies, output collision rejection,
per-group count mismatch and final original/staged/plan/review guards.

`stage.main()` was never invoked; no root accepted-input plan was created or
reviewed, no staging import/rebuild ran, no shared state was written, no network
was used and no live handles remain. This acceptance does not approve pending ODM
candidate content or bind a plan that does not yet exist. The concrete plan still
needs its candidate/review hashes, finite groups/counts, live hashes and acceptance
bindings independently checked. Subsequent staging/rebuild/promotion acceptance
remains pending, as described in the original review.

Existing good behavior is retained: new-source-only checks; immutable snapshot
copies and complete existing PDF job; unchanged campaign gates/code freeze;
record/fact/source deltas; document/page preservation; owned baseline; integrity
and foreign keys; compatible manifest expected counts. Interrupted work remains
private and incomplete without a complete verified success report. Preserve any
partial artifacts and choose explicit new run paths if recovery becomes necessary.
