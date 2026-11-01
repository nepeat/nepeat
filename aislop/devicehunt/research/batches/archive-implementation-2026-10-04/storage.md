# storage engineering handoff

outcome: complete tooling; real databases remained read-only. coordinator owns
acceptance, exact snapshot selection, archival execution and removal. no network,
package installs, SQL connections, imports or shared-data mutation performed.

owned files: `devicehunt/storage.py`, `tests/test_storage.py`, this note and
`storage-inventory.json` beside it.

## measured inventory

124 SQLite files: 33,168,826,368 logical bytes; 8,121,968,640 allocated bytes.
Read-only first-1MiB sampling of each file found 110 unique chunks among 124
samples: 130,023,424 uncompressed bytes become 30,374,501 compressed unique
bytes. This prefix sample is **not a reliable full archive size estimate**.
Full archive output must be measured against actual allocated bytes, not 33GB
logical length; sparse files and filesystem sharing affect savings. The inventory
lists every relative name and size/allocation without changing files.

recommended keep set: `data/devices.sqlite`,
`data/rebuilt-with-inventec-practical-2026-10-04.sqlite`, and
`data/devices-before-inventec-practical-2026-10-04.sqlite`. Coordinator should add
active SmartNIC/QCT and archive integration candidates/checkpoints to explicit
`keep`. Every non-SQLite input, manifest, capture and proof stays outside this
selection. any `devices.sqlite` basename and any resolved `sources` path component are hard-protected; extra keep
paths must be supplied explicitly. Only named `.sqlite` paths are eligible.

## API and commands

`inventory(root)`, `archive(root, paths, archive_dir, keep=(),
remove_originals=False)`, `verify(archive_dir)`, `restore(archive_dir,
destination, paths=None)` return metadata dictionaries. All paths may be pathlib Paths;
source selection uses strict POSIX relative strings. CLI is independent of the
main catalog command routing:

```sh
python3 -m devicehunt.storage inventory .
python3 -m devicehunt.storage archive . data/history-archive-v1 data/EXPLICIT-HISTORICAL.sqlite --keep data/rebuilt-with-inventec-practical-2026-10-04.sqlite
python3 -m devicehunt.storage verify data/history-archive-v1
python3 -m devicehunt.storage restore data/history-archive-v1 /tmp/NEW-history-restore
```

Replace the uppercase placeholder with reviewed exact names. `archive` creates
an immutable `manifest.json` and compressed SHA-256-addressed objects. 1MiB chunks
align with every supported SQLite page size. Manifest version 1 preserves exact
relative paths, byte lengths, full SHA-256 and ordered chunk SHA/length pairs.
Identical file/chunk bytes deduplicate without interpreting SQL. Objects and
manifest publish exclusively via atomic hard links after flushing file data;
publication directories are fsynced. Existing objects are checked before reuse;
existing immutable manifest selections must match exactly.

Archival defaults to retaining originals. Only an explicit repeat invocation
with identical paths and `--remove-originals` applies removal. Before any unlink,
all archive objects and reconstructed whole-file hashes pass verification, then
all remaining originals' hashes pass preflight. Every original is hashed again
immediately before its unlink. WAL/SHM/journal siblings cause refusal, including
checks before removal. Paths/symlinks, malformed hashes, sizes, duplicate names,
traversal, oversized compressed inputs, trailing compressed bytes and hash
mismatches are rejected. No input manifests are deleted or rewritten.

Interrupted object creation leaves reusable objects with originals intact;
interrupted removal leaves immutable recovery manifest and verified objects.
Re-running the exact selection resumes after already-removed originals. The
archive directory must itself remain under coordinator control: the format is
hash-verified but does not provide external signature/authenticity guarantees.
Removal assumes exclusive ownership of quiescent snapshot files; do not run any
writers or another consolidation concurrently. As with any path-based filesystem
operation, hashes cannot prevent malicious concurrent substitution after a check.

Restore verifies all bytes before creating its destination. The destination must
be new; an existing directory is refused. Each restored file is published only
after its reconstructed SHA passes. Failed restoration can leave a partial new
destination; preserve it for inspection and restore again to another new path.
Optional repeated `restore --path RELATIVE_PATH` materializes selected files after
verifying the complete archive; unknown selections are refused. Only bytes and relative paths are promised; inode numbers, timestamps, permissions,
sparse layout and reflink relationships are not preserved.

## checks

`python3 -m unittest tests.test_storage -v`: **14 tests passed**. Fixtures cover
exact duplicate files/chunks and restoration, empty files, read-only inventory,
protected live/keep/input paths, all three sidecars, corrupt/missing archive
objects preserving originals, changed source preserving the whole preflight
set, interruption before manifest, resumable interruption during removals,
path traversal and symlink rejection, reconstruction SHA mismatch, immutable
selection and restore refusing overwrite.

next bounded step: independent code review, coordinator's explicit historical
selection/keep manifest, archive without removals, measure actual object storage,
full verify and representative independent restore SHA checks, then accepted
explicit removal under exclusive coordinator ownership.
