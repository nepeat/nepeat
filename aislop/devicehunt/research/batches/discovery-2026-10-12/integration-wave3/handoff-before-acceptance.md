# wave 3 staged integration

/root is sole integrator. **Staged, not promoted.** Live pair remains the accepted
PSU checkpoint. `stage-report.json` binds all proofs and new input hashes.

Candidate `data/devices-candidate-wave3-2026-10-12.sqlite` was built with the
accepted pre-repair curated importer. It contains5117identities/7241records/
74210facts/2093sources/68documents/3784pages. Candidate manifest
`data/import-manifest-candidate-wave3-2026-10-12.json` has148jobs/3941inputs.
Complete reference bundle `data/raw/reference-documents-with-wave3-2026-10-12.json`
retains all55oldentries and adds two complete PDFs/five pages. Source-specific
curated captures also pin both complete text sidecars.

All old ordinary logical rows/multiplicities preserved. Exactly55old reference
source bundle hashes changed, checked separately. Integrity ok, no FK violations,
owned baseline10/10 and campaign inputs/projections pass. FTS/internal-table
comparison awaits separate clean rebuild using repaired code. No current shared
manifest/database/export/archive write performed.

`old-code-under-test.json` binds importer/catalog hashes to the completed design
profile. Root then lifted production freeze for scoped importer repair. New-code
clean rebuild must match this old-code candidate across all52tables/indexes.
Full guarded suite, independent code and integration acceptance remain required
before promotion. No completed process is still running: stage r2 session22102
exited0.

History: immutable `stage.py`/`stage.log` stopped on an uppercase-only acceptance
text check (Oracle review says lowercase accept), after first-group copies and
before candidate DB creation. `stage-r2.py`/`stage-r2.log` verifies any existing
copies byte-identical, binds production hashes, accepts case-insensitive review
text and completes the operation. No captures were overwritten or reimported
from an interrupted DB. Independent reviewer /psu owns stage acceptance report.


## active acceptance — 04:35 UTC

Curated and shopping code independently ACCEPT. Frozen209production/test/dependency
files in `code-under-test.json`. Full guarded suite running as session43695,
label`discovery-2026-10-12-curated-wave3`, log`full-suite.log`. Separate clean
rebuild running as session82624 toNEW`data/rebuilt-wave3-2026-10-12.sqlite` using
candidate manifest, log`rebuild.log`. No promotion yet. Check these actualhandles;
neverrestartinplace. /psu preparatoryhelper/docreview thenfinalproofreview.
Read-only four-model queries pass in `model-query-check.json`:9SD/8Wedge/9Oracle/
1M2categorizedstatements; M2onlyitsreviewedliteral; no transfer to registryspellings
and orderingrows notidentifiers. An initial exploratory assertion used 'may vary'
where acceptedfact says 'can vary'; corrected check compares exactreviewedliteral.
