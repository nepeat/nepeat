# buying_report_hint review (2026-10-11)

**Verdict: REJECT until the `see:` command is fixed (one must-fix, small). Everything else is sound and would be ACCEPT with notes.**

Scope: `add_source_fact_hints` and the table line in `devicehunt/buying.py`, `tests/test_buying_report_source_hint.py`, and the docs paragraph in `docs/buying-reports.md`. All probes ran on scratch copies or temporary catalogs. The live DB was opened read-only and no repo files were touched.

## Checks

1. **Outputs unchanged apart from the new key and line: PASS.** I built a scratch copy with the hint code removed (import, call, function and table lines). I compared 11 queries, each plain and with `--vendor Inspur`, in both formats: `NF5280M7`, `7726-32X-O`, `Oracle Server E5-2c`, `AIS800-32O`, `R4023-F9102-A0`, `QuantaGrid D52B-1U`, `T42S-2U`, `NF5280M6`, `Sun Fire X4270 M2`, `QuantaPlex T42S-2U`, `nonexistent`. That covers the owned models (E5-2c, AIS800-32O, R4023) and identities without source facts.
   - JSON with `source_facts_available` stripped equals the baseline for all 22 runs.
   - Per-category key order is unchanged and every status is identical.
   - Table output with the new lines removed equals the baseline for all 22 runs.
   - Hints appeared only for `NF5280M7` (power 5, chassis_physical 6), `7726-32X-O` (chassis_physical 2) and `NF5280M6` (chassis_physical 1). That matches the handoff.

2. **Scope: PASS.** The hint reads only the report's own `records` (already filtered by identity and `--vendor`) and only fires when the category is `not_collected`. Evidence:
   - `NF5280M7 --vendor Inspur` gives no hints.
   - A mutant that hints from every record in the DB fails 2 tests.
   - In the live cases above, the counts equal the number of `specs` source-fact rows on the same matched records, with no rows on other records.

3. **Statuses never change: PASS.** The status is computed before the hint runs. A mutant that flips the status when it adds the hint fails 2 tests.

4. **The `see:` command: FAIL (must-fix).** `see` is built from `models.pop()` if the matched records have a single model, otherwise the query, and is always `specs <that> --exact-model --include-source-facts`. It ignores `--vendor` and the identifier kind that matched. Three consequences, shown on a temporary catalog:
   - Part-number match: report for `PN-A` (one record, form factor `2U`) hints count 1. The `see` command `specs M1 --exact-model ...` returns 3 source-fact rows, including another part number's NetBox row and another vendor's SPEC row.
   - Vendor filter: `buying-report M1 --vendor Wiwynn` hints count 1. `see` omits `--vendor` and returns the Accton record's facts too.
   - Part number spanning two models: `buying-report PN-1` hints count 2. `see` is `specs PN-1 --exact-model ...`, which returns 0 rows. `--exact-model` does not match `part_number` identifiers; `--exact-part-number` returns the 2 rows.
   - The same widening happens for a single-model alias match: `see` uses the model name, not the alias, so it covers all records of the model.
   - On the live DB the three checked identities (NF5280M7, 7726-32X-O, NF5280M6) all match literal models without `--vendor`, so `see` returned exactly the counted rows. That is why the handoff verification did not catch this. The command is runnable, but "correct" depends on how the identity matched.
   - The docs say the hint means facts "exist on the same matched records, so the report's identity and vendor scope applies" and that the count "can differ slightly". When `see` is broader than the report, that is no longer slightly.
   - Suggested fix: pick the flag from the matched identifier kinds (`--exact-part-number <pn>` when the match was a part number; `--exact-model <alias or model>` otherwise), and append `--vendor <canonical>` when the report was run with `--vendor`. When the report has several matched records with differing scopes, prefer the query string with the right flag. Add a test for the part-number and `--vendor` cases.

5. **Count semantics documented: PASS with a note.** The docs say N is distinct (source, fact name, statement) rows over matched records, and that it can differ from `specs`. The wording should also say the count is restricted to the report's own records and is not the number of rows `see` prints when scopes differ (mooted if the must-fix lands). Placeholder values are skipped as in `specs`, and labelled statements are not used. Names only, never values, as documented.

6. **Tests: partially meaningful.** `tests/test_buying_report_source_hint.py` has 4 tests. I ran 9 mutants:
   - Caught: hint on collected categories; status change; placeholder not skipped; wrong count; wrong `see` flag text; fact names replaced by values; no whitelist (wrong category mapping); table line dropped; hint from every record in the DB (2 failures).
   - Gaps (should-fix):
     - `test_scope_follows_matched_records_and_vendor_filter` never passes a vendor, so the vendor part is untested.
     - There is no test for `see` correctness with part-number, alias or `--vendor` queries. That is why the must-fix escaped.

7. **Test runs.** `python3 -m unittest tests.test_buying tests.test_buying_report_source_hint tests.test_specifications` ran 28 tests, OK. I did not run the full suite.

## Must-fix

- `see:` must reproduce the report's scope. Use `--exact-part-number` for part-number matches, and carry `--vendor` through when the report was filtered. Today it can print a wider set of rows or none.

## Should-fix

- Add tests for the `see` scope cases above, and make the vendor test pass a vendor.
- Adjust the docs wording on counts once `see` is fixed.

## Notes

- Hints show only when a category is `not_collected`. If the report already classifies a whitelisted fact (e.g. `energystar_*_idle_watts`, `psu_configuration_*` and `form_factor` are in `CATEGORY_FIELDS`), the category is collected and no hint appears. That is as intended.
- The listing of hint category mapping reuses `source_fact_category`, so a future whitelist change flows through automatically.

## re-review

**Verdict: ACCEPT.** The must-fix is resolved. There are no remaining must-fix items.

- **Scope reproduction.** I re-ran the failing scenarios on a temporary catalog and executed each printed `see` command through `specifications.search` with the flags it names. I counted the rows from all `see` segments and checked whether any fell outside the report's matched records.

  | Scenario | Printed `see` | Result |
  |---|---|---|
  | `PN-A` (part number) | `specs PN-A --exact-part-number --include-source-facts` | 1 row, count 1, none outside the matched records |
  | `M1 --vendor Wiwynn` | `specs M1 --exact-model --include-source-facts --vendor Wiwynn` | 2 rows, count 2, the Accton record is excluded |
  | `PN-1` across two models plus a record whose model literally equals `PN-1` | `specs PN-1 --exact-model ... ; specs PN-1 --exact-part-number ...` | 3 rows, count 3 |
  | alias `AL X` (shell quoting) | `specs 'AL X' --exact-model ...` | 1 row, count 1 |

  - No `see` command returned rows outside the report's matched records.
  - Plain `M1` shows more rows than one category's count because `specs` rows span all three categories; none are outside the matched records.
  - Part-number-only records get `--exact-part-number`, everything else `--exact-model`, and a mix gets both joined by ` ; ` (valid in a shell). `--vendor` is the report's canonical target (`target_for` is idempotent on the canonical names I checked). The query is used, not the model name, so alias and part-number reports no longer widen to the whole model.
- **Unchanged output.** I rebuilt the scratch baseline from the current module with the hint code (including `specs_command`) removed. Across 9 queries, each plain and with `--vendor Inspur`, in JSON and table: JSON with `source_facts_available` stripped is identical, key order and statuses are identical, and table output with the new lines removed is identical. Hints appear only for `NF5280M7` (power 5, chassis_physical 6), `7726-32X-O` (2) and `NF5280M6` (1), as before. `NF5280M7 --vendor Inspur` gives no hint.
- **Tests.** `python3 -m unittest tests.test_buying tests.test_buying_report_source_hint tests.test_specifications` ran 32 tests, OK. The 4 new tests cover the failing scenarios. I did not re-run my mutant set against the new tests. I did not run the full suite.
- **Docs.** The `see` / count paragraph in `docs/buying-reports.md` now describes the part-number, vendor and mixed behaviour; I read it and found no contradiction with the code.
