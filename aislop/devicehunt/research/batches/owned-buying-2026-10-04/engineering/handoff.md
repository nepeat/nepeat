# engineering handoff

Outcome: implemented; independent review requested a documentation correction,
now fixed and awaiting reviewer recheck/coordinator acceptance.

Owned changes: `devicehunt/buying.py`, `devicehunt/__main__.py`,
`tests/test_buying.py`, `docs/buying-reports.md`, this directory.
No schema/importer/manifest/export/database changes or acquisitions.

API: `buying.run(db, query, vendor=None)` with `sqlite3.Row`; table helper
`buying.render_table(report)`. CLI: `python3 -m devicehunt buying-report QUERY
[--vendor TARGET] [--format table|json]`.

Explicit exact model/record-local alias/part matching, casefold whole identifier;
no fuzzy fallback, suffix decoding or cross-record technical borrowing. Company
filter checks affiliations on each selected record. Source fields and company
roles are attached to statements; full contextual facts are stored once per
matched record and referenced with `context_record_id`. Unknown fields remain
unclassified and inspectable. Source conflicts/blank or unusual literal values
are neither normalized nor reconciled.

Six category statuses distinguish mapped gaps, seller-only and other source
statements. Mapping is intentionally reviewed field names, not an exhaustive
interpretation of historical raw test/archive fields. Category collected does
not establish completeness, operability or entitlement. Unknown variant gets
an explicit gap report. Baseline supplies ten smoke queries only.

Verification: bundled Python `-m unittest tests.test_buying tests.test_specifications`:
19 tests passed (`tests.log`). Six new tests cover alias/part isolation, same-record
vendor, V/V+ suffix, unknown family, conflict/revision/source hash/context,
unclassified free text, empty categories, seller-only/mixed, JSON/table CLI,
missing DB without creation and byte-identical fixture DB. Existing 13 specs
regressions pass. Ten live owned queries matched (`live-smoke.json`); before/after
live SHA-256 both `ea7d121849c34cac801c178d48be80ea5f3af09958b526228b6b6f536d0662d8`.

Limit: system Git `diff --check` could not run because Apple's Git requests Xcode
license acceptance; system configuration unchanged. Coordinator can use its
available portable Git. Tests and direct file inspection completed.

Next: independent code/docs review, coordinator generate full ten reports and
update shared README/PROGRESS. No integration required for this code-only change.

Review correction: the SQL example originally used `E5-2C`, a record-local alias,
and returned zero rows. Changed it to literal `Oracle Server E5-2c`, regenerated
`sql-example-e5-2c.json`, and asserted its actual count is 10 rows. The report
example continues using alias `E5-2C` correctly. No code changed for this fix.
