# buying-report source-fact hint (audit finding 4 / F8)

Change: `devicehunt/buying.py` only (imports `source_fact_category`, `source_fact_statement` from specifications.py read-only; no CLI change).
For power / cooling / chassis_physical categories whose status is `not_collected`, when whitelisted `--include-source-facts` names exist on the matched records (same identity + vendor scope as the report), the category gains a new last key `source_facts_available: {count, fact_names, see}`; the table gains one appended line `source_facts_available: N (names: ...; see: ...)` under that category. Absent otherwise, so output for identities without such facts is byte-identical. Statuses, statements, limitations and key order are unchanged. Placeholder values are skipped as in specs. `see` uses the single matched model name (else the query).
N = distinct (source_id, fact name, statement) rows over matched records; specs de-duplicates per device, so counts may differ slightly.

Verification (read-only, before/after):
- NF5280M7: power `source_facts_available: 5 (spec_power_supply_details, spec_power_supply_quantity_and_rating_w)`; chassis_physical 6 (spec_form_factor, spec_nodes_per_system, spec_number_of_identical_nodes); cooling none. Rest identical.
- 7726-32X-O: chassis_physical 2 (netbox_is_full_depth, netbox_u_height). Rest identical.
- E5-2C: output byte-identical (no whitelisted facts on its matched records; the specs rows the audit saw for X10-2c come from a differently named identity, which the exact-identity report deliberately does not borrow).
- JSON with the new keys stripped equals the before JSON for all three.

Tests: tests/test_buying_report_source_hint.py (4 new); tests.test_buying + new = 15 OK; tests.test_specifications 13 OK; test_broadcom run too.
Docs: one paragraph in docs/buying-reports.md.

## Review fix (see: scope)
`see` now reproduces the report scope: query as typed (not the model name); `--exact-part-number` when records matched only by part number, `--exact-model` otherwise, both joined by ` ; ` for a mix; `--vendor <canonical>` when the report was vendor-filtered; shlex-quoted. Tests added for part number spanning two models (incl. `--exact-model` returning 0 rows), alias, vendor + quoting, and mixed; each asserts the `see` string and that the equivalent `specifications.search` returns exactly `count` rows. Docs reworded. tests.test_buying + hint + test_specifications: 32 OK.
