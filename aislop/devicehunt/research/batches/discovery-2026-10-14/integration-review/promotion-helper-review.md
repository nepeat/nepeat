# promotion and export helper review — accepted

**accept these exact helper revisions, subject to the still-pending concrete plan,
staging/rebuild verification and final promotion acceptance.** Reviewer
`/root/odm_bulk_full`; no blocking findings in this bounded helper review.

| helper | accepted SHA-256 |
| --- | --- |
| `integration-breadth/promote.py` | `d7b46c9baca8bea22397ca89f2f5d3a440da4338e73f8ba6ffeaffd7552c1695` |
| `integration-breadth/refresh_exports.py` | `3942040b7a90b0d7c5beeaac909141f3bf5f1bea185ff37eaab6807694ef00c3` |

[Promotion source snapshot](reviewed-promote.py),
[export source snapshot](reviewed-refresh_exports.py),
[static probe code](probe_promotion_helpers.py),
[results](promotion-helper-probes.json). Both sources were reviewed against the
previous accepted wave5 helpers and the new accepted staging helper contract.

## proof and identity binding

Promotion requires a literal accepted acceptance object, the new stage report,
accepted-input plan, all three current helpers, query check, all four current
integration reviews, full comparison/validation outputs, exact rebuilt database,
comparison script, frozen code manifest and the prior passing full-suite proof.
Every proof path rejects absolute/parent traversal and must resolve inside the
workspace. The final review path is fixed and separately hash-checked.

The candidate DB and manifest must each match both the stage report and explicit
promotion-acceptance hashes. The helper rechecks all original accepted plan,
candidate, capture and source-review pins from the stage report, all copied
staged inputs and the complete manifest's offline hashes/gate. Source reviews are
therefore transitively bound even though their paths vary by accepted plan.

The code freeze is reverified against the pinned previous full-suite proof;
production code changes would fail that gate. Comparison must cover exactly all
52 catalog tables/indexes, including both FTS tables, with equal counts and zero
differences. Counts must match the stage report. Baseline must be 10/10 and name
these exact candidate/rebuild paths. This retains complete reference-document
inputs through the unchanged manifest; no PDF bundle replacement occurs.

These checks bind the specific accepted artifacts; they do not replace the
pending independent inspection of the eventual plan, query checks, preservation
receipt and rebuild/comparison results. No such results existed or were accepted
by this helper-only review.

## live backup and interruption handling

The live DB/manifest hashes must still equal the staged old pair. Live SQLite
journal/WAL/SHM files must be absent. Existing backup or pending destinations are
refused. Both old live files are copied using their exact expected hashes, then
both candidate files are copied to pending paths with the exact accepted hashes.

A journal records old/new pair hashes, immutable backup paths/hashes and final
review/suite hashes **before either live replacement**. After both replacements,
the new live pair is hash-verified and its manifest revalidated before a success
result is saved. No import or database writer runs during promotion.

The two file replacements are sequential, so an interruption between them can
leave a mixed live pair. This behavior is unchanged from the previously accepted
workflow. The preserved old pair and prewritten journal make that state explicit
and recoverable. Do not treat absent `promotion-result.json`, or a pair differing
from journal `new`, as completed promotion. Do not blindly rerun over existing
backup/pending/journal artifacts; inspect their hashes and recover the pair under
root's exclusive integration ownership. This review does not execute recovery.

## exports

Refresh requires a successful promotion result and its exact new DB hash. The
catalog is opened read-only. All seven current export files it may replace are
first copied into a new exclusive `exports-before/` directory, using the new
three-argument expected-hash clone API. A provenance file records those old
hashes. Historical wave5-specific exports remain untouched.

It refreshes targets CSV/JSON, catalog HTML, specifications CSV/JSON, complete
reference-document CSV and inventory-validation JSON. Owned baseline remains
10/10. DB unchanged hash is checked before the final export receipt records every
new export path, byte count and hash. Failure/interruption may leave a partially
refreshed export set; the full old copies remain available and absence of a
complete verified export receipt means refresh is incomplete. The export helper
itself does not change the catalog or manifest.

## validation and scope

Nine static AST/source checks passed, zero failures/errors. They verify required
new proof paths; confined proof paths/fixed final-review path; candidate/stage/
acceptance hash binding; explicit expected hashes on every clone; journal-before-
replacement ordering; live hash/SQLite sidecar guards; complete comparison and
baseline requirements; read-only export catalog and SELECT-only direct queries;
and backup-before-refresh/success-report ordering. These are static checks, not
an executed promotion or export test. The cloned-hash implementation already
passed separate before/during-copy mutation probes in the accepted staging review.

Neither helper nor its `main()` was executed. No accepted-input or promotion plan
was created, no shared state was written, no network was used and no live handles
remain. Original staging reviews/probes and the Red Hat source review remain
unchanged. Root must supply the pending concrete plan and later stage/rebuild
results for the next bounded independent review.
