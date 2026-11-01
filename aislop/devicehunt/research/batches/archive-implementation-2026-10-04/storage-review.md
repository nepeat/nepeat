# independent storage review

reviewer: archive_engine (did not author storage code). outcome: accepted for
coordinator-controlled explicit historical snapshot archival/removal after the
coordinator selects paths and keep set and records manifest SHA-256.

reviewed `devicehunt/storage.py` and `tests/test_storage.py` after final protection
fix. `python3 -B -m unittest discover -s tests -p 'test_storage.py'`: 14 pass.

one blocker found and fixed by storage_engine: `sources/` selection-string checks
could be bypassed by changing the root to `workspace/sources` or selecting a nested
`sources` path. final source preflight rejects `sources` in the absolute source
components, including root ancestors. tests cover both bypasses. live
`devices.sqlite` is protected by basename regardless of selected root. caller must
still explicitly pass current recovery checkpoints via `keep`; the module cannot
infer which dated backup the coordinator has selected for retention.

all objects and whole reconstructed file hashes are checked before removal starts.
every still-present original is rehashed in a whole-selection preflight before
any unlink, and rehashed again immediately before its own removal. sidecars,
symlinks, traversal, mismatched original bytes and missing/corrupt chunks prevent
removal. compression/manifest publication uses exclusive complete-file publication
and fsync. crashed unlink resumes from already verified immutable archive records;
no history is silently skipped. a failed mutation can remove earlier *verified*
originals but every removed file remains independently byte-restorable; that is
intentional resumability rather than atomic all-files deletion.

manifest selection is explicit and publication never overwrites existing
`manifest.json`. coordinator should pin its SHA-256 in current handoff metadata
before removal to preserve external identity. permissions alone cannot guarantee
immutability from an owner who intentionally edits a backup; verification catches
invalid content and original-hash checks prevent replacing extant inputs with a
changed manifest.

full and selected restores verify the entire archive first, reject unknown paths,
require a new destination, and publish each complete file only after its SHA-256
passes. tests demonstrate duplicate chunk/file deduplication, exact bytes,
interrupted manifest/removal recovery, corrupted unselected chunk rejection,
changed-original whole-selection protection and symlink destinations/objects.
byte identity is the contract; filesystem mode/mtime restoration is not promised.

no actual project snapshots were compressed, removed or restored during review.
next: coordinator explicit path/keep selection, archive creation, independent
restore byte checks, pinned manifest hash and only then authorized removal.
