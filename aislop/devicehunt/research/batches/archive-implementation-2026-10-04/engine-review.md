# independent archive engine review — 2026-10-04

reviewer: archive_capture. reviewed devicehunt/artifacts.py, derivations.py and
their affected tests read-only; only this engine review note is reviewer-owned.
no live requests or shared data writes. disposable reproductions used /tmp.

accepted after one blocking concurrency correction. startup/reindex previously
read its receipt snapshot before locking SQLite. a concurrent writer could
publish/index a new object, then have its registration erased by the older
snapshot's table replacement. a temporary two-Archive reproduction confirmed
resolve failed while the registration receipt remained. author archive_engine
moved BEGIN IMMEDIATE before the snapshot and added a separate-thread connection
regression that publishes after the snapshot; the writer now completes its index
commit after reindex commits. receipt/object publication remains independent of
the short reconstructible-index transaction and never spans network.

checked immutable publication uses private staging inodes, fsync and hardlink
without replacing an existing pathname; identical concurrent receipt publication
retains the winning frozen timestamp. orphan objects/staging are reported rather
than deleted. source registration does not chmod/hardlink or alter original bytes.
checks reject parent traversal, outside-root paths, static symlinks/ancestors and
unexpected object paths. SHA-256 resolution verifies bytes; changing or removing
a registered location raises/reports corruption. read-only lookup/audit leaves
archive files identical and absent databases fail without creating empty state.
reconstruction and copied-workspace restoration retain original receipts and
resolve both registered legacy and newly stored objects. invalid receipt
registration rolls back without erasing the existing index.

legacy acquisition and extraction timestamps/tool/config/code remain unknown;
registration timestamps are kept separate. saved derivations cannot falsely match
known recipe runs. recipe identity includes source code hash, Python version,
configuration and output schema. direct temporary mock checks confirmed changed
runtime and changed code identity reject an old recipe before cache reuse.
configuration changes, force reruns, failed extraction persistence and multiple
consumer reuse are tested. duplicate and empty page/XML values retain order.
missing/corrupt derived output cannot produce a successful cached reuse.

verification after correction: `python3 -m unittest tests.test_capture
 tests.test_artifacts tests.test_derivations` passed all 30 tests (17 capture,
9 artifact, 4 derivation). engine scope passes; coordinator may proceed with
finite isolated registry integration and its separate release/rebuild checks.
