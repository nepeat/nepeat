# independent candidate and clean-rebuild review — 2026-10-04

accepted; no actionable findings. promotion may proceed within coordinator-owned
acceptance. final promoted exports and documentation remain separate checks.

candidate SHA-256:
`1e7a877f46b0140c62993de0ebc48b0378773db30c0e889cdfb67244371d7074`.
clean rebuild SHA-256:
`15f4d7aa04ff5fea619c2ea1bbac0f136e2c0122eddd1b9f01c113a58f0aadf3`.
live original remained:
`ea7d121849c34cac801c178d48be80ea5f3af09958b526228b6b6f536d0662d8`.

independent read-only checks:

- Counter-compared all 43 logical tables from candidate to clean rebuild,
  excluding only observed_at/fetched_at/cache_path as normal rebuild metadata.
  both logical FTS tables and their row multiplicities were included. also
  inspected generic 43-table equality report: zero one-sided rows throughout.
- Counter-compared ordinary old/live versus candidate tables: every table is
  identical except the permitted facts additions, records, sources and search
  content. document FTS is unchanged. all 44,664 old facts remain; exactly 26
  added facts produce 44,690 total. roles, identifiers, devices, regulatory raw
  tables, document records and hyperscaler rows remain unchanged.
- compared every existing record column: only 25 timestamps change, belonging
  to the 24 Broadcom records and one label record; only label raw_json changes,
  exactly matching the full accepted candidate input row. Broadcom raw_json is
  byte-identical. every other record field remains unchanged.
- compared every source column: the only actual change is label coverage, equal
  to accepted input and retaining original wording as an exact prefix.
- independently traversed all 24 original hashed Broadcom detail captures and
  compared every complete ordered dictionary to SQL json_each output: all 206
  rows match, including duplicates and nested metadata. no release join.
- regenerated all ten saved buying reports read-only and compared exactly. the
  generator intentionally leaves vendor unfiltered and retains expected_vendor
  as validation metadata, consistent with prior full-role reports. label power
  contains the scoped literal marking; E4/E5/X10 each expose one array with three
  BIOS rows. X7 has no imported BIOS array and remains a gap.
- inspected candidate CLI proof and hash-verified generated HTML: six DOM tests
  passed against its payload/inline script; full identifiers and empty ByteDance
  filter are checked. this is DOM evidence, not screenshot review.
- independently ran integrity_check and foreign_key_check on candidate and clean
  rebuild: ok and no violations. frozen code/input/log/review hashes match.
- reviewed owned/FTS proof: 10/10 owned, nine document snapshots plus one label,
  separate ByteDance 0/2; source-aware FTS content complete. inspected recorded
  timestamp audit and the corrected manifest plan, which keeps unsupported facts
  out of manifest.expected.

proof-helper corrections are accurately treated as tooling corrections: the
first helper used row.value instead of report statement; another required an X7
BIOS row absent from the source; supplemental Counter comparison originally
included FTS physical shadow segment layouts. final proof requires all actual
three owned BIOS models and preserves X7's gap, while comparing all 43 logical
contents including both FTS tables. none of these corrections waive data loss or
an evidence mismatch. this reviewer also initially compared reports with an
extra vendor filter, then verified the documented generator's unfiltered reports
exactly; that mismatch was a reviewer invocation difference, not an export defect.

no rebuild, imports, tests, exports, network or mutation of shared inputs were
performed during this review. only this new report was written; original stage
1/2 review remains frozen.
