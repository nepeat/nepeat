# independent wave3 staging review

reviewer `/root/psu`; integrator `/root`; 2026-10-12 local date.
**ACCEPT STAGE ONLY**, no blocking finding. this accepts copied/pinned inputs
and the isolated database/manifest under the recorded **pre-repair code**.
curated-import repair/review, full guarded suite and fresh clean rebuild/equality
checks remain pending; this is **not promotion acceptance**.

reviewer authored the Oracle facts input, not these staging scripts. its content
has a separate frozen independent `/root/handoff_audit` acceptance. this stage
review checks integrator operations/results and preservation, not self-acceptance
of that candidate's source claims.

## frozen inputs and retry

read `integration-wave3/stage.py`, `stage-r2.py`, `old-code-under-test.json`,
`stage-report.json`, both frozen candidate/review pairs and copied dependencies.
independent proof/helper:
`review/wave3-stage-independent-check.json` and
`review/wave3-stage-independent-check.py`.
successful readonly command:
`PYTHONPATH=. python3 -B research/batches/discovery-2026-10-12/review/wave3-stage-independent-check.py`.
an initial helper invocation without PYTHONPATH stopped at ModuleNotFoundError
before audit execution; corrected command succeeded. no importer/rebuild/script
staging invocation or fixture/live write was performed by reviewer.

stage1's uppercase `ACCEPT` assertion succeeds on the PSU/Edge review and fails
on Oracle's lowercase verdict before the second group's copies, bundle save,
database backup or import. that control flow agrees with the coordinator's
reported first-group-only copy checkpoint and no stage1 database. stage2 keeps
exclusive absence requirements for candidate database/manifest/new bundle/report;
it only permits preexisting input directories/copies, requiring exact source/dest
hash equality before reuse. diff also adds old-code/design-profile checks and
case-insensitive acceptance for the actual frozen Oracle review. no importer,
counts, preservation or candidate-selection change hides the first failure.
both actual verdicts and their exact candidate hashes were independently read;
this is a fixed batch, not acceptance of a generic substring-based review parser.

| frozen candidate | independent content review | SHA-256 |
| --- | --- | --- |
| psu-edge-facts | psu-edge-facts-review.md ACCEPT | `0fbf35eb2c9e35364ca915cdba6c4622ae6cd7ae037e9955376576d38d7d03ab` |
| oracle7420-facts | oracle7420-facts-review.md accept | `a82e27385459ddafbe54d510d73959c237bad98b743a7cbd4c97f32032c5b25c` |

both original candidates and their data/raw copies still have those exact hashes.
all curated snapshots/sidecars are byte-identical to originals. **all3941 pinned
manifest inputs** were independently verified by `rebuild.read_manifest`, without
running rebuild. manifest has148 jobs; preserves every old pin unchanged and
adds exactly13 new pins. only the existing reference-document job path changes,
and exactly the two frozen curated jobs append. campaign release/coverage gate
equals accepted live configuration; input and projection checks independently pass.

## complete PDF collection and resulting database

new bundle has **57 entries /3710 pages**, preserving all55 previous entries
exactly and appending only SD6200 and Wedge. every entry's PDF/text dependency
uses a sibling filename, resolves beside the complete bundle, matches its declared
SHA and is manifest-pinned. `reference_documents.prepare` validates the full57
entry collection, including all strict Poppler/legacy sidecars. the two new complete
PDF/text copies are byte-identical to reviewed snapshots; no partial collection.

independent readonly stage counts and report match:

| item | staged count |
| --- | ---: |
| devices | 5117 |
| records | 7241 |
| facts | 74210 |
| sources | 2093 |
| all PDF documents | 68 |
| all document pages | 3784 |

four new curated sources contribute exactly five records/39 facts:
SD620011, Wedge16, Oracle internal10plain+1M2, attached1plain.
exact resulting namespaces/models are `Celestica / SD6200`,
`Edge-Core / Wedge 100-32X`, `Oracle / Sun ZFS Storage 7420`
and `Oracle / Sun ZFS Storage 7420 M2`. proof JSON retains actual strings; joined
WEDGE100-32X and existing Oracle regulatory namespaces are untouched.
zero new identifiers; all new affiliations documentation_vendor only.
manifest's expected source counts equal actual candidate source inventory.

independently repeated duplicate-sensitive preservation across all50 non-FTS
tables: **zero missing/changed old logical row groups**. environment fields
observed_at/fetched_at/cache_path and FTS internals are excluded. source SHA is
handled separately: exactly the55 old reference-PDF sources change from complete
old-bundle hash `23b0a010896326f43be6e6cc50c77f5e57e40f68ba4b8c0f2545ca34dde10b29`
to new-bundle hash `31b38a268680871b09b016e74b712e6b825f29866557d8560e95c038ef769e6d`.
no other old source SHA changes; records, roles, facts, identities, identifiers,
documents/pages and accepted lineage tables survive.

independent integrity_check=`ok`, foreign_key_check empty; owned baseline10/10;
ByteDance expectations remain0/2. campaign input/projection gates pass. no clean
rebuild or full-suite execution is claimed by this stage review.

## exact proof/code/state bindings

- stage report: `16bb5a658c486e407d3d2d71e8b002066380fad90883be641a0814ccb1853bfd`.
- candidate database: `4ae2cd0d6a614d582cf6fc47e6ba0e022521ac2b5e73bb268fcfa0dcb87c03ea`.
- candidate manifest: `9b4d93229e1519770a79f665e02987e9706e74cf86db086599160b905157ffec`.
- old importer: `99e5051a5c09c0abee1606e2127bea88e7539ca65b704f503b44ccb7d3ba7d51`;
  old catalog: `2c5731c8a1fbac7f7c6f3d57284cb0ae3448cd26277c5e1446e83f054192c8ef`.
  stage report, old-code-under-test and held design profile agree. code observed
  during reviewer check still equals those versions; pipeline hash also agrees
  with held profile. future repair acceptance must bind its own new hashes.
- authoritative database before/after reviewer checks:
  `53a9a0eabaef5656b38ad922f96d963b76839eeda2da3b28a18b22359d857dee`;
  authoritative manifest:
  `6ab359e22929bdb6a55e47c2a816c2c78fc3e5d0f90c8b252129f8fafea3aac5`.
  both match stage report/live baseline and remain unchanged. candidate pair
  also retains report-bound byte hashes after review.

proof timestamp2026-10-13T04:17:15–04:17:29UTC. saved JSON contains all50
preservation counts, exact models/roles, gate/count/provenance observations and
hashes. only this review and uniquely named reviewer proof/helper were written;
all data connections mode=ro/query_only. no network, imports, archive/export/
manifest/database/code/shared-doc edits or delegation. finite stage-review stop
met; sole integrator handles pending repair/fullsuite/rebuild before promotion.
