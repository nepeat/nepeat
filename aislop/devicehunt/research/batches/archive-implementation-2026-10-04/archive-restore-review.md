# independent archive restoration review

reviewer: storage_engine, 2026-10-04. outcome: **passed**. Detailed file hashes,
byte counts and results: [archive-restore-proof.json](archive-restore-proof.json).

Copied 422 immutable archive files and 105 workspace artifact locations to an
isolated temporary workspace under this batch. Every copy matched its original
SHA-256; workspace locations additionally matched durable artifact-registration
receipt SHA values. Deliberately omitted archive `index.sqlite` and
`capture-leases.sqlite`; confirmed both absent before reconstruction.

`Archive(new/data/archive, workspace=new, create=True)` rebuilt the metadata
index entirely from restored receipts. Audit passed with zero errors, orphans or
pending staging: **374 receipts, 146 artifacts, 153 locations**. Frozen release
validation passed on the copied release using only paths in the new workspace:
146 artifacts, 374 receipts, four statements/reviews/links. Release SHA-256:
`2eadea0c31d9ffdfc1b763b26b8500ebddcb78c94d2706900ed382549fb33736`.
Release ID: `c468ef0721b90f5935615f37dd9619dad6fc1bc2159a107cbf054ede6aad5b79`.

Opened candidate `data/catalog-archive-candidate-2026-10-04.sqlite` mode=ro and
resolved `Golduck` against the restored data root. All **four claims** resolved,
including hash verification of their three lineage artifacts. Every returned
saved-artifact path lay under the new workspace. Candidate SHA-256 remained
`62ab16eb8ab562ebbd6eaf6951987800fa8f4482879007d1713d0b8529ce0e4a`
before and after. Neither original archive metadata/index nor catalog bytes were
changed. No network requests were made; urllib acquisition was patched to fail.

TemporaryDirectory removed the isolated workspace after checks; only this report
and proof remain. The restoration demonstrated no dependence on old absolute
artifact paths or original metadata indexes. This verifies immutable evidence
archive reconstruction; historical SQLite chunk backup restoration remains a
separate consolidation acceptance check. No live promotion or original removal
was performed by this reviewer.
