# integration preparation — not acceptance

exclusive integrator: server_evidence_integrator. coordinator owns docs and gates.
shared data, manifest, live database, PDF bundle, exports and code remain read-only.
preflight.json records independently verified live/input hashes and counts.
old-complete-bundle.json is a byte-identical research-local preservation copy only.

## anticipated deltas, conditional on frozen source/code acceptance

merge all three separate curated proposals under a new data/ input; their snapshot
paths are relative to data/, so direct research-path import is invalid. additions:
Oracle 1 source/1 record/16 facts; Ingrasys vendor 1/8/124; separate Foxconn parent
portfolio 1/6/72. no source-ID collisions or duplicate source/key pairs found.
six new exact Ingrasys model identities; existing SV2221A/I and Oracle E4-2C retained.
with derivative archive: expected schema28, 4090 identities, 5317 records,
287 sources, 44975 facts, 57 PDFs / 3661 pages, 46 complete-reference entries.
manifest should become119 jobs when one merged curated job is added and the existing
complete-reference job redirected. hashed-input total depends on which original
and derivative provenance dependencies are explicitly registered; compute from
accepted manifest rather than guessing. no new hyperscaler assertion.

## actual importer constraints

curated() calls Catalog.source(), which deletes records only for the same source
ID before inserting replacement rows. all three new additive IDs are necessary.
Oracle reused capture is a new extraction source, never independent corroboration.

reference_documents.prepare() validates ALL entries and sibling dependencies before
import_bundle() replaces archive documents/pages. preserve the original45 entry
objects exactly, append only accepted derivative entry, copy PDF/text byte-for-byte
beside NEW COMPLETE bundle in data/raw/. source upsert necessarily refreshes
sha256/cache_path/fetched_at for the45 old reference sources because these identify
the full JSON bundle. no other old source columns may differ. documents/pages have
deterministic IDs and must remain exact; all45 old PDF/text bytes/hashes stay exact.
original malformed capture and EOF-restored derivative stay separate immutable files.
source-derived complete bundle scope must clearly name reconstruction; archive text
has ten blank pages and cannot supply model facts by embedded-text search.

## sequential execution after explicit coordinator gate

1. verify reviewed hashes/code freeze and full-suite result; verify live SHA and
manifest SHA still match preflight. retain unique before live/manifest copies; avoid
any overwrite of historical outputs. record all original PDF dependency hashes.
2. assemble merged curated input, accepted new complete bundle plus sibling bytes,
validate captures and full reference.prepare(), record frozen provenance hashes.
3. build candidate from byte-identical old live copy; apply complete archive then
new curated sources sequentially. old rows/timestamps/raw_json/roles/facts and all
old documents/pages must survive exactly. allow only the45 identified archive
sources' three refresh columns. use duplicate-sensitive Counter containment, not
EXCEPT alone, and assert old device identities unchanged.
4. update manifest's reference job and hashes, append one curated job and all
reproducible dependencies, derive exact expected source counts and totals from
accepted additions. rebuild offline into NEW path using bundled Python. never
reuse prior historical mutation helpers.
5. invoke scripts/compare_rebuild.py <new-rebuild> --database <candidate>
--output-prefix server-evidence-2026-10-04; script writes audits under data/exports,
so requires shared-state ownership. supplement its43-table EXCEPT check with
Counter equality excluding only observed_at/fetched_at/cache_path as rebuild
semantics require. independently reconstruct both FTS indexes and compare rows.
6. integrity/foreign keys, expected deltas/source roles, original vendor preservation,
owned9-document+1-label baseline, ByteDance0/2, practical buying/search queries;
confirm all eight exact suffixed models carry reviewed facts, six new identities,
Foxconn parent-only affiliations, and unsuffixed four remain specification gaps.
check E4-2C literal two CMVP rows plus complete module/history context.
7. run frozen full suite before promotion. coordinator accepts candidate proof;
then promote validated database and refresh affected standard/catalog/manufacturer/
collection/buying/lookups exports, preserving historical snapshots. hash read-only
query runs before/after. hand coordinator verified counts/proof for docs update.

stop condition for this assignment: preparation artifacts complete; no integration
or export refresh until coordinator transfers shared ownership after acceptance.

FTS detail: old FTS physical rowids may shift on deterministic regeneration.
compare old document FTS by logical (document_key,page,text) multiplicities against
underlying pages; new archive adds ten blank page rows. existing E4-2C/SV2221A/I
search text intentionally gains facts; six device search rows are new. do not
classify these intended derived-index changes as old evidence deletion. clean
candidate/rebuild comparison should still verify each regenerated index exactly.
