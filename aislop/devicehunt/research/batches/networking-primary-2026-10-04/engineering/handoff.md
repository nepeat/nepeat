# buying-report engineering handoff

Outcome: complete, code/tests frozen for coordinator review. Only
`devicehunt/buying.py`, `tests/test_buying.py` and this engineering directory
were written. No collection, source proposal edits, imports, manifest/database,
bundle, exports or current documentation edits.

Explicit reviewed SM 1U/SM 2U chassis fields map to chassis_physical; each
variant's memory, PCIe, drive-bay and processor-wattage fields map to
compute_network_storage. Processor maxima never enter power. Fact names remain
separate variant labels, and literal multi-category product descriptions stay
unclassified with full record context. No field value parsing or variant inference.

Four published device-tree declaration fields map to firmware_boot_software.
Their source tier, URL, hash, record context and configuration limitations remain
intact. Artifact declarations never create aliases, join company roles or bind
an artifact to an owned unit. Vendor-filter, matching and claim-status behavior
is unchanged. No installed firmware, safe flashing or vendor endorsement inferred.

Tests use separate named variants and source records, isolate part-number scope,
keep a related regulatory identifier unmatched, retain community provenance, and
prove source-role filters cannot borrow community declarations. No live database
is accessed by the tests; fixtures use independent temporary directories.

Checks: 10 focused buying tests pass; full application suite: 427 tests pass in
19.736 seconds. The earlier `git diff --check` success was not meaningful:
these project paths are untracked, so Git emitted an empty diff. That claim is
withdrawn. The replacement explicit check examined all 86 added lines for
trailing whitespace and tabs; zero errors. Frozen code/test hashes are unchanged;
tests were not rerun for this artifact-only correction.
Runtime: `/Users/nep/.cache/codex-runtimes/codex-primary-runtime/dependencies/python/bin/python3`.
Commands: `-m unittest tests.test_buying -v`; `-m unittest discover -s tests -v`.
Logs: `focused-tests.log`, `full-tests.log`. Owned pre/post SHA-256 hashes:
`pre-code-sha256.txt`, `post-code-sha256.txt`. Review diff: `code.diff`.

No original pre-change file copies were retained. `reconstruct-diff.py` removes
exactly the additive mapping block and three additive tests from read-only frozen
files, verifies each reconstructed preimage against its recorded pre-change
SHA-256, then saves `reconstructed-before-buying.py` and
`reconstructed-before-test_buying.py` and emits a real unified before/after diff.
Both preimage hashes match; both frozen post hashes remain unchanged. Detailed
proof and whitespace results: `diff-verification.json`. The diff is 7,779 bytes,
SHA-256 `14325a0be365ae2ae677a792c0e3fbfc02f282e33270a1654faa07c6653c222a`.

Acceptance still requires independent code and source review; the proposed Oracle
and community inputs were read-only, pending coordinator acceptance. Next bounded
step: independent review, then coordinator-directed exclusive integration.
