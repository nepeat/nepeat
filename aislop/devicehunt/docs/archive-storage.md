# evidence archive and database history

`data/devices.sqlite` is the accepted hardware catalog. acquisition writes to
`data/archive/`; frozen releases connect the two. current release identity,
verification and counts belong in [PROGRESS](../PROGRESS.md).

## use saved evidence

```sh
python3 -m devicehunt evidence Golduck
python3 -m devicehunt.archive audit
python3 -m devicehunt.archive derive page-text-flatten TEXT_SHA256 --consumer my-local-question
python3 -m devicehunt.archive derive xml-locations XML_SHA256 --consumer my-local-index
```

replace hashes with registered input hashes. `evidence` accepts an exact source
key, claim ID or statement ID. the first release links four existing Golduck facts;
it does not pretend every catalog fact already has reviewed lineage. it returns
original local paths, selectors, literal excerpts, review decisions and capture/
derivation ancestry. a missing or corrupt required artifact fails visibly.

The c1 code requires an explicitly upgraded acquisition registry for writes.
`data/archive/` was upgraded on 2026-10-05 after a scratch rehearsal (index v2 with
head marker, operational schema 3). The upgrade binds the operational authority to
the archive's absolute path, so a copied or moved archive is refused. Library callers
must pass the project root as `workspace`; the c2 campaign runner registers saved
inputs in place there ([proofs](../research/campaign-c2-2026-10-05.md)). A `reindex`
changes the index generation and requires an explicit operational
`synchronize_archive(..., allow_generation_change=True)` decision before campaign writes.

`derive` runs a named local recipe or reuses a verified compatible run. it keeps
input order, recipe code/runtime/configuration, actual outputs, failures, reruns
and separate consumers. the supported recipes flatten saved page text and extract
ordered XML locations; they do not perform PDF parsing, OCR or automatic hardware
claim acceptance. saved text remains tied to its PDF hash, including empty pages.
legacy extractor versions and capture times stay unknown where not recorded.

```sh
python3 -m devicehunt.archive --root research/my-new-archive init
python3 -m devicehunt.archive --root research/my-new-archive register-reference research/batches/artifact-archive-pilot-2026-10-04/inventory.json
python3 -m devicehunt.archive --root research/my-new-archive reindex --quiesced --decision "reviewed offline index reconstruction"
```

registration validates the entire pinned complete collection before adding
metadata and is idempotent. original PDFs stay in place with sibling dependencies.
new derived/captured bytes live at `data/archive/objects/<first-two>/<sha256>`;
original locations and immutable JSON receipts retain separate provenance.
the Ingrasys reconstructed PDF retains its malformed response and explicit
byte-repair ancestry. identical bytes never imply independent corroboration.

`init` is only for a new, absent archive root. Existing legacy registries require
an explicit, quiesced `upgrade --quiesced --decision TEXT` after a verified backup
and rehearsal; this does not migrate operational quota state automatically.
`index.sqlite` is reconstructible from receipts. `reindex` explicitly rebuilds
the index and changes its generation; ordinary reads fail on absence and never
create empty databases. `recover --decision TEXT` handles documented interrupted
publication states. Maintenance must be exclusive and preserve immutable receipts.
audit reports corrupt/missing locations, unreferenced objects and interrupted
staging files without deleting them. acquisition leases are operational state in
`capture-leases.sqlite`, not accepted evidence.

## bounded source adapter

```sh
python3 research/fetch_fcc_sitemaps.py --archive data/archive --report-dir research/my-sitemap-check
```

the default is offline registration of the existing finite saved collection.
each source row retains its report hash/ordinal, status and legacy metadata;
repeated registration does not fabricate a new HTTP observation. new reports get
new filenames. XML discovery does not become a hardware claim.

network budget defaults to zero; setting `--network-budget` cannot activate live
collection. The [campaign engine](../research/campaign-c1-2026-10-04.md) requires
an acknowledged modern archive, durable operational authority, synchronized safety
references and a separately recorded activation for a finite shared account.
Only temporary mocked accounts have exercised activation. No live operational
migration, account activation or source refresh has occurred in this stage.
Saved-only registration is supported on modern archives. A collector-local budget
cannot replenish quota; known restrictions survive body or receipt failures.
The accepted archive release required no new network requests.

Operational quotas and stops are not reconstructible from evidence receipts.
Back up `capture-leases.sqlite` with its `capture-authority.json` and
`capture-authority-ready.json` identity files, plus archive head/index state.
Lost or inconsistent authority remains blocked; never initialize over it or use
an older migration backup to reset spend. Explicit restore needs coherent state
and quarantine/reconciliation; index reconstruction invalidates live activation.
The [state handoff](../research/batches/campaign-c1-2026-10-04/state-integration/handoff.md)
records APIs and recovery limits, including unresolved allocation interruption
before the authority ready seal is published.

## accepted releases and rebuilds

`data/archive-release-2026-10-04.json` freezes artifact paths/hashes, complete
receipt payloads, reviewed statements, selectors and review mapping. SQL stores
the accepted release and lineage, so evidence lookup and rebuild do not query a
changing acquisition registry. new pending receipts do not change accepted facts.
candidate extraction remains outside the hardware projection until reviewed.

all required bytes must be present. preserve relative paths when moving the
workspace; neither rebuild nor evidence inspection silently refetches missing
files. the full reference collection remains complete, including its sibling
PDF/text dependencies. the current manifest also uses saved complete NTRA text
and positional tables, avoiding optional PDF packages during offline rebuild.

use the ordinary manifest rebuild into a **new** path and compare it with the
accepted candidate. the comparison includes all lineage tables and grouped row
multiplicities, with the established source timestamp/cache-path exclusions.
read-only reports, searches and evidence lookups leave the catalog unchanged.

## historical databases

historical snapshots retain their **exact original bytes** in compressed,
SHA-addressed 1 MiB chunks under `data/history/`. manifests map original relative
filenames to chunk sequences, lengths and full-file hashes. these are backups,
not alternative hardware catalogs. repeated chunks are stored once per archive.

```sh
python3 -m devicehunt.storage inventory .
python3 -m devicehunt.storage verify data/history/database-snapshots-2026-10-04
python3 -m devicehunt.storage restore data/history/database-snapshots-2026-10-04 /tmp/fcc-history --path data/rebuilt.sqlite
```

restore requires a new destination. omit `--path` to restore every historical
database, which requires their full logical size. verification checks every
reconstructed file, even when restoring only one; allow time for this check.
historical proof paths that name archived databases now resolve through the
[history index](../data/history/README.md), not a symlink to the current catalog.

keep the live catalog, one immediate recovery checkpoint and the latest verified
rebuild directly accessible. after later milestones, archive superseded databases
using explicit paths, verify all bytes, test restoration, and only then use the
storage tool's explicit `--remove-originals` option. it protects `devices.sqlite`,
`sources/`, supplied keep paths and databases with SQLite sidecars. removal assumes
the coordinator has exclusive ownership of quiescent selected snapshots.
never remove history without a verified archive, or remove immutable raw evidence.

back up `data/raw`, other manifest inputs, archive objects/receipts, accepted
release/manifest, catalog and history manifests/chunks together. the local restore
checks prove recoverability in a new directory; they are not an off-machine backup.
