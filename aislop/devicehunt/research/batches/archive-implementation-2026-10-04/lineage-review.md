# independent lineage review

reviewer: storage_engine, 2026-10-04. preliminary review of
`devicehunt/lineage.py`, schema 29 in `catalog.py`, archive-release routing in
`pipeline.py`, expected-count routing in `rebuild.py`, read-only evidence CLI and
`scripts/compare_rebuild.py`. outcome: accepted at engineering review; all three findings corrected.
Catalog promotion still requires coordinator full-suite and rebuild evidence.

## resolved review findings

1. `prepare()` validates globally present receipt/artifact dependencies and receipt
   cycles but does not require an evidence link's artifact to belong to its linked
   receipt's transitive artifact closure. An unrelated receipt can therefore be
   attached to otherwise valid saved evidence. Bind selected source and PDF text
   artifacts to the linked receipt closure; reject disconnected lineage.
2. `show()` checks linked original bytes but does not verify saved text or other
   artifact ancestors referenced by its receipt closure. Missing/corrupt ancestry
   can succeed silently despite a purported reconstructible chain. Verify all
   closure artifact paths/hashes and PDF text dependency during resolution.

## inspected positive properties

Release ID hashes canonical frozen contents; statement revision IDs hash exact
content while excluding the acceptance pointer. Claim IDs are explicit durable
strings; catalog mapping uses source_id/source_key and exact projection fields,
not unstable record rowids. No foreign key to mutable projection records exists,
so source replacement retains reviewed statement/history and can report an absent
current projection. Statements require explicit accepted reviews and evidence
links before insertion. Candidates/unreviewed extraction alone cannot insert a
hardware fact: this importer adds lineage only to projections already present.
Original roles, raw and normalized values and exact excerpts remain separate.

Artifacts validate all listed local saved paths, size and SHA before writes.
Selectors validate PDF/text association and complete ordered pages, JSON-pointer
selection or line ranges, then whitespace-only excerpt presence. Receipt closure
is finite and cycle checked. Import validates before writes and uses a savepoint;
immutable key collisions compare full existing rows and reject alterations,
rolling back the entire release. Expected rebuild counts include all nine lineage
tables. Duplicate-sensitive comparator groups every projected column plus
multiplicity; lineage tables participate. Evidence CLI opens SQLite mode=ro and
no network paths are invoked.

3. JSON array selectors originally accepted negative/signed/leading-zero indices
   via `int()`. Corrected to strict RFC-style nonnegative indices and valid ~0/~1
   escapes; independent temporary checks rejected /-1, /+0, /00 and /~2, and
   accepted /0.

All findings above were re-inspected after coordinator corrections. Link original
and saved PDF text must belong to transitive receipt artifact closure; show checks
all closure artifact hashes plus PDF text SHA.

## actual checks and limits

`python3 -m unittest tests.test_lineage tests.test_rebuild tests.test_storage -q`:
**27 passed** (7 lineage, 6 rebuild, 14 storage). Coordinator tests cover graph
cycles, disconnected lineage, saved-text corruption, invalid page selectors, source
replacement retaining statement IDs, rejection before mutation, immutable receipt
collision rolling back release insertion and read-only database byte preservation.

Independent temporary curated + archive-release rebuild passed with all nine
lineage expected-table counts, two conflicting width claims, offline network mock,
integrity/foreign keys and evidence resolution. Independent temporary JSON array
selector probes passed as described above. Fixtures only; no real data writes.

Full suite and accepted-candidate versus clean rebuild remain coordinator
integration checks. Review decisions are explicit assertions from the frozen
release; software validates mapping and quoted-byte selection but cannot itself
establish reviewer judgment or manufacturer/deployment interpretation.
