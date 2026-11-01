# milestone 0: pilot contract and authority boundary

this is the coordinator's local design decision for a future isolated pilot,
not an implemented schema or a claim that every later schema detail is selected.
the [plan](../../../docs/artifact-archive-plan.md) remains the broader proposal.

## frozen scope

[inventory.json](inventory.json) pins the manifest-selected complete reference
bundle, all 47 PDF/text pairs and six directly relevant Ingrasys provenance
files. it includes 3,589 pages: 3,575 with embedded text and 14 without it.
these are reference-collection counts, not whole-catalog counts or coverage claims.
all 94 declared dependency paths have distinct byte hashes in this collection.
synthetic duplicate-byte and changed-URL cases are still necessary in milestone 1.

the complete bundle is
`data/raw/reference-documents-with-inventec-practical-2026-10-04.json`.
its digest, import-manifest digest and live checkpoint digest are in `pins`.
the inventory SHA-256 is
`d7be503676cb832824d4e0fd33e5390417cc7af55c476027563d75391c11b327`.
this is a hashed inventory, not an accepted archive release or a backup.
it registers nothing, copies no originals and changes no catalog tables.

SmartNIC/QCT remains a separate pending integration. a later promotion must not
silently change this pilot's finite input set. if current state changes, the audit
below fails; retain this inventory and explicitly select a new freeze or audit
these pinned inputs separately. do not overwrite this inventory or the complete
collection to make a check pass.

## selected ownership boundary

| owner | future durable content | catalog relationship |
| --- | --- | --- |
| immutable files and receipts | byte objects, original locations, frozen capture and derivation receipts, recipe definitions, review history and release manifests | evidence authority; indexes can be rebuilt from these inputs |
| isolated archive registry | artifact/location indexes, resources and discovery paths, all capture attempts, all derivation runs and dependency edges, candidate statements, review history | acquisition and evidence index only; no competing device catalog |
| operational registry | jobs, leases, retry budgets, source policies, checkpoint and local reuse events | excluded from accepted catalog; job success does not accept evidence |
| frozen accepted release | a reviewed, hashed, transitively closed subset of artifact metadata, capture/derivation receipts, recipes, statements, review decisions and evidence locators | explicit sole input for future lineage import; never a query of mutable registry state |
| `data/devices.sqlite` | current hardware projection plus the release identity and the accepted subset of lineage needed to resolve claims offline | remains authoritative for accepted hardware assertions |
| search and exports | current accepted document pages and hardware views | reproducible projections; no promotion of candidates through FTS |

milestones 1–2 use only an isolated registry and existing files. no catalog schema
changes are needed for registration or saved-output reuse. eventual catalog
lineage tables must include artifact identities, release membership, accepted
capture/run/recipe references, stable statements/claim revisions, evidence links
and the review decision establishing each accepted revision. include ancestors
needed to explain an accepted derivative, even if those ancestors contain no
hardware claim. superseded accepted revisions remain addressable. complete
candidate/rejected histories remain in the archive and versioned receipts.

table names and column-level migrations are deferred to milestones 4–5; the
ownership split above is settled for the pilot. imports must validate the entire
frozen dependency closure locally before replacing catalog projections. file
hashes and manifest membership bridge the databases; SQLite foreign keys cannot
validate an external registry. catalog lookups must not need that mutable registry.
relative object locations may relocate on restore; byte identities cannot change.

the catalog must map a stable claim/revision identifier to its current source/fact
projection explicitly. neither SQLite rowids nor a re-created `records` row can
be that identity. changing a normalized value creates a reviewable revision,
not an in-place rewrite of the source's printed statement. role conflicts and
duplicate source rows retain separate identities and locators. final key encoding
is a milestone-4 task; no claim IDs are manufactured by this inventory.

## mapping saved evidence

each inventory document preserves its whole bundle entry, including URL, exact
title, tier and scope. `inventory_key` is a bundle-hash/JSON-pointer locator for
this snapshot only; it is not a permanent claim or upstream resource identifier.
the proposed PDF and page-text artifact objects are keyed by full SHA-256, with
their existing workspace-relative locations. metadata describing acquisition or
source meaning belongs on provenance records, not globally on shared bytes.

ordinary entries become explicit **legacy registrations**, not invented HTTP
attempts. their source URLs are known from the bundle; actual requested/final
URLs, request context and event identity are not established by that association.
do not deduplicate request histories by URL alone. later registration receipts
need stable IDs, a separately recorded registration time, and the frozen inventory
locator. repeating a registration must reuse that receipt; an actual later network
attempt gets its own identity. no registration timestamp is an acquisition time.

existing page-text JSON is a saved derivative tied to the exact PDF hash. all
47 envelopes expose only `pdf_sha256`, `extraction_method`, and `pages`; the
method label is `pypdf embedded text; no OCR`. preserve the complete page array,
including blank text. PDF page numbers are one-based; JSON array locators are
zero-based. printed page labels and bounding boxes are unknown here.

tool version, code hash, dependency versions, configuration, extraction time,
historical run status and declared output-schema version are absent from these
envelopes. the new inventory's format version does not backfill their schema.
reuse a verified saved output as saved evidence; do not use the broad method
label as a compatibility key for a newly executed extraction recipe. milestone 2
must declare its own actual recipe and retain each genuine run separately.

capture times, original collector version, headers, validators, response status,
wire-versus-decoded representation, and original acquisition completeness are
unknown from most bundle entries. these are limits of this finite inspection;
additional historical notes may contain details. neither filesystem mtime nor
current installed tools supply missing legacy metadata. the imported source's
bundle timestamp is also unsuitable. registration time is not assigned yet.

## special evidence boundaries

Ingrasys is an existing **reconstructed derivative**, not original response bytes.
the pinned original and fresh response have the same SHA-256 but retain distinct
locations and saved acquisition context. the six supporting files include both
response copies, saved headers, the capture ledger, derivative provenance and
handoff. the ledger reports HTTP 200 and a curl method, while explicitly reporting
a malformed body; a successful status is not proof of a valid original PDF.
the inventory preserves that ledger literally in `special_lineage`; it does not
invent event times, collector versions or a new acquisition.

verified chain: malformed body → exact byte span `[4128654,8355366)` plus
hex `460d0a` → sibling reconstructed PDF → saved page-text JSON. byte offsets
are zero-based and half-open. this audit reproduced the derivative bytes exactly
in memory. all ten pages have blank embedded text; existing visual review is
historical provenance, not a new visual check. a future release using this PDF
must include that original-body and transformation lineage, not just the PDF/text
pair. reconstruction run/tool metadata remains unknown where not recorded.

the QCT D51B datasheet remains a vendor-authored document from the independent
Art of Server mirror. its bundle `tier: vendor` does not make the mirror the
vendor or establish equality with a QCT-hosted original. scope and URL survive
verbatim. the inventory makes no new authenticity, manufacture or deployment claim.

## checks and next assignment

existing `reference_documents.prepare` validated the whole collection, sibling
paths, hashes, PDF signatures, unique entries and complete text envelopes.
every declared dependency also matches the pinned import manifest. read-only
SQL matched all reference source IDs, PDF hashes, source URLs, page counts,
page text and text-status rows to the bundle. all pinned inputs were hashed again
after inspection and stayed unchanged. the first exploratory SQL audit used the
wrong column name and failed before producing an inventory; the saved helper uses
the actual `pdf_sha256` column and passed.

recheck from the workspace (standard-library Python; no installs):

```sh
PYTHONPATH=. python3 -B research/batches/artifact-archive-pilot-2026-10-04/pin_inventory.py --check
```

the helper is a finite research audit, not a product command. `--check` writes no
files; default creation refuses an existing output. no imports, network, extraction,
FTS refresh, full tests, rebuild or whole-catalog validation were needed or run.
the owned 9+1 baseline and unresolved ByteDance expectations retain their existing
status; this audit does not newly validate them. see [review](review.md) for
independent acceptance and limitations.

next bounded assignment: implement milestone 1 registration/storage against this
freeze in new `devicehunt/artifacts.py`, `tests/test_artifacts.py`, and an isolated
pilot output directory, after recording fresh writer ownership. register existing
originals in place; add immutable object storage for new fixture bytes. verify
idempotence, distinct provenance for duplicate bytes, changed bytes at one URL,
missing/corrupt files, path/symlink safety and crash-safe publication. include
legacy receipts and Ingrasys lineage; no live collector or catalog migration.
