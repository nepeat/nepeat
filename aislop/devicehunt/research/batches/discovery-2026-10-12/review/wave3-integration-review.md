# wave3 integration — final independent review

reviewer `/root/psu`; sole integrator `/root`; 2026-10-13 UTC.
**ACCEPT FOR LOCAL PROMOTION of the frozen wave3 pair and accepted code.** no remaining material finding. promotion and export refresh have not occurred; this verdict accepts their reviewed inputs/helpers and completed verification, not future execution results or publication. this supersedes the [preserved preparatory report](wave3-integration-review-preparatory-preserved.md).

## accepted pair and finite scope

| artifact | accepted SHA-256 |
| --- | --- |
| `data/devices-candidate-wave3-2026-10-12.sqlite` | `4ae2cd0d6a614d582cf6fc47e6ba0e022521ac2b5e73bb268fcfa0dcb87c03ea` |
| `data/import-manifest-candidate-wave3-2026-10-12.json` | `9b4d93229e1519770a79f665e02987e9706e74cf86db086599160b905157ffec` |

accepted candidate counts: **5117 devices, 7241 records, 74210 facts, 2093 sources, 68 documents, 3784 pages**. manifest:148 jobs/3941 hashed inputs. this wave adds the independently accepted SD6200/Wedge and Oracle7420 candidates: four curated sources/five records/39 facts, plus two complete PDF documents/five pages. it does not include subsequent wave4 or SPEC continuation candidates.

frozen candidate content hashes remain `0fbf35eb2c9e35364ca915cdba6c4622ae6cd7ae037e9955376576d38d7d03ab` (PSU/Edge) and `a82e27385459ddafbe54d510d73959c237bad98b743a7cbd4c97f32032c5b25c` (Oracle). [independent PSU/Edge review](psu-edge-facts-review.md) and [independent Oracle review](oracle7420-facts-review.md) bind these exact bytes. reviewer authored Oracle input; its content acceptance came from a different reviewer. this review checks integrator operations and proofs, not self-acceptance of source claims.

## completed verification

final checks were performed on the actual completed artifacts, using read-only connections. uniquely named helpers/results preserve the earlier independent checks:

- [stage/preservation recheck](wave3-integration-final-stage-check.py) / [actual results](wave3-integration-final-stage-check.json): all3941 pins verify; all old manifest pins retained with exactly13 additions; full57-document/3710-page bundle retains every old55 entry and validates complete sibling PDF/text dependencies. frozen curated snapshots and sidecars match originals byte-for-byte. all50 non-FTS logical tables retain old row multiplicities; exactly55 old reference-source SHA values change solely from the old complete-bundle hash to the new one. no other old-source SHA change. campaign input/projection gates pass. integrity=`ok`, zero FK violations; owned10/10 and ByteDance0/2 remain distinct.
- [final comparison recheck](wave3-integration-final-comparison-check.py) / [actual results](wave3-integration-final-comparison-check.json): all209 production/test/dependency hashes still match `integration-wave3/code-under-test.json`. independently repeated duplicate-sensitive, bidirectional comparison of all52 logical tables/indexes, including both FTS indexes **and their rowids**: zero differences, equal counts. complete baseline summary/results/search_summary agree; rebuilt integrity=`ok`, zero FK violations. comparison/validation paths explicitly bind the staged candidate and `data/rebuilt-wave3-2026-10-12.sqlite`, rather than the live database.
- [final proof/freeze check](wave3-integration-final-proof-check.py) / [actual results and receipt hash set](wave3-integration-final-proof-check.json): completed guarded suite result and final log agree: **791 tests, zero failures/errors/skips/blocked network or external-process calls, 783.32 seconds**. runner's unittest timing is783.248 seconds, distinct from rounded whole-run elapsed time. guard runner was read; no trace entry exists. all209 frozen files still match. independently accepted shopping focused proof records15 Python tests and7 JavaScript tests, all passing; existing SQLite ResourceWarnings are non-failing. accepted curated-import code review includes71 focused tests and isolated compatibility/preparation probes. reviewed helper hashes and all20 mandatory proof paths plus seven supporting artifacts are saved for the root's receipt.

completed coordinator logs `integration-wave3/rebuild.log` and `compare.log` report successful clean new-code rebuild/pin/count verification and all52-table/baseline equality. the held old-code stage profile agrees with `old-code-under-test.json`, `stage-report.json` and the saved design profile. old importer/catalog hashes remain provenance of the staged candidate; current accepted hashes are bound by the209-file freeze and new-code clean rebuild. no claim relies on running old code now.

## reviewed helper behavior and remaining execution duties

reviewed `stage.py`, `stage-r2.py`, `stage-report.json`, `promote.py`, `refresh_exports.py`, `check_queries.py`, prior stage acceptance and current docs. interrupted stage1 stopped on uppercase-verdict matching after first-group input copies and before DB creation; stage2 verifies exact preexisting bytes, retains exclusive DB/manifest/bundle/report creation and completes staging. [stage review](wave3-stage-review.md) preserves that history.

initial promotion-helper findings were fixed before this final review: mandatory critical proof membership, confined relative paths, exact final-review path/hash and exact candidate-derived52-table comparison keys including both FTS indexes. `promote-before-proof-guards.py` preserves the old helper. current helper SHA is unchanged from the separately recorded preparatory guard review; syntax compiled without execution. absolute paths, `..` and resolved paths outside the workspace are rejected. the final receipt must bind this final report's exact bytes, candidate pair, current freeze and proof hashes; pending preparatory bytes are superseded. recommended receipt proof hashes include complete suite/rebuild logs and the guard runner in addition to every helper-mandatory artifact.

promotion revalidates the candidate pair, manifest pins, live pair, suite/code proofs, comparison and baseline, then preserves both old authoritative files through exclusive byte-checked copies. pending output paths are exclusive; a journal binds old/new pair, backups, suite and review before two `os.replace` operations. this is two file replacements rather than a single transactional pair swap; retained backups/journal support recovery if interrupted. final pair hashes and live manifest pins are verified before result creation. root must keep the authoritative pair quiescent and execute the frozen helper only after producing its hash-bound acceptance receipt. no live SQLite journal/WAL/SHM, old-wave3 backup/pending paths or prior promotion journal/result existed at final check.

export helper verifies accepted promotion result and live DB hash, opens `Catalog` read-only, preserves seven current generated exports in an exclusive historical backup directory with provenance, then refreshes normal exports and catalog HTML. owned10/10 is asserted; DB byte hash must remain unchanged. new dated `wave3-specifications-2026-10-12.json` uses exclusive creation; prior dated exports remain untouched. current export backup and dated output paths were absent at final check. root retains responsibility for actual promotion/export-result checks and final documentation update.

four-model query proof binds the held candidate hash and records9 SD6200,8 spaced Wedge,9 plain Sun7420 and1 M2 categorized statements. M2's single literal fact remains isolated; nothing transfers to existing registry spellings. six Wedge ordering rows remain literal facts without new identifiers; all new roles are documentation_vendor. document results in the planned export are text-search hits with provenance, not identity/fitment evidence. buying's exact-model lookup can span existing brand namespaces and does not merge them. shopping code retains title/approved identifiers, adds exact literal model, suppresses existing software references and hides the empty HTML shopping group; accepted independent code review and JS/focused proofs are included.

## unchanged authoritative state and stop

authoritative pair before/after all final reviewer checks remains:

- database `53a9a0eabaef5656b38ad922f96d963b76839eeda2da3b28a18b22359d857dee`;
- manifest `6ab359e22929bdb6a55e47c2a816c2c78fc3e5d0f90c8b252129f8fafea3aac5`.

candidate pair, clean rebuilt DB and all frozen dependencies also remain byte-identical through checks. PROGRESS/README correctly separated live/staged evidence at preparatory review; root should now replace pending suite/review prose with these actual results, and later record actual promotion/export outcomes. no documentation edit was made by reviewer.

final stop condition met: **ACCEPT for root's local promotion of this exact pair using the reviewed frozen helpers**, followed by root-owned export/state verification. only this report and uniquely named reviewer artifacts were written. no network, install, import, temporary/shared DB write, full-suite/rebuild/promotion/export execution, production/doc/manifest/archive change or delegation.
