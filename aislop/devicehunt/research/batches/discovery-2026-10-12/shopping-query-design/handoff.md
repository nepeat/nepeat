# F17 shopping-query design handoff

owner `/root/psu`; **bounded design complete, not implemented**.
recommend adding the selected identity's exact literal model alongside current
title/explicit identifiers in `shopping_links`, and returning `[]` for the existing
`listings.is_software_reference` classification. no brand abbreviation, corporate
equivalence, model-prefix stripping or fact-derived query. row schema, URLs,
quoted strings and deterministic exact-string de-duplication remain compatible.

`design.md` gives concrete helper behavior, source/provenance boundaries,
backward-compatibility risks, minimal optional CLI/report presentation changes,
focused checks and coordinator choices. actual current helper starts from
**stored title**, rather than assembling brand+model; F17's legal-brand strings
are embedded in those titles. CLI and generated HTML share this helper.

eight live examples only: four legal-brand NF5280M6/NF5280A7 variants, software
wedge, owned7760/CS8200 full codes, ordinary SD6200. captured actual helper rows,
actual shopping CLI stdout/stderr/returncode, exact fields and source-record/
identifier context in `examples-before-and-proposal.json`; six hardware cases
gain one literal query, SD6200 is unchanged because it already has that explicit
identifier query, and software wedge gets zero hardware search rows.

before/proposal artifact SHA-256:
`2f6db578c3bc8d7680f4596253e106d1d24e132a184bf4bd4c8485d7b7934faf`.
observed2026-10-13T04:22:56.392702–04:22:56.791009UTC; catalog hash before/after
unchanged `53a9a0eabaef5656b38ad922f96d963b76839eeda2da3b28a18b22359d857dee`.
read-input hashes also bind relevant code/docs/tests. `capture_examples.py`
refuses to overwrite saved before outputs; no report/export was regenerated.

nine synthetic examples in `synthetic-risk-examples.json` assess blank fields,
exact duplicates, generic/short hardware names, bareword hardware wedge,
kind-only software, printed case variants, suffix/punctuation URL escaping and
missing optional newly consulted fields. do not normalize broad names into
invented precision; retain the existing contextual title. no URL was fetched,
clicked or ranked using live marketplace results.

optional minimal presentation companions: CLI explains software-reference
suppression instead of generic no-record text; HTML hides an empty shopping
details block while keeping `shopping: []`. preserve query/ebay/web keys and
CLI syntax; adding rows can change indices but old hardware rows retain relative
order and exact URL bytes. current tests cover shopping readonly behavior, not
these semantic/link-rendering cases. concrete meaningful test scope is in design.

stop condition met. only this task directory written; no production edits,
imports, database/manifest/export/shared-doc writes, network, broad crawl,
benchmark or delegation. coordinator can choose implementation scope later;
existing printed identities/company roles/family-table semantics stay intact.
