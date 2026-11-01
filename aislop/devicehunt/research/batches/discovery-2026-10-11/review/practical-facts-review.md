# practical_facts review (2026-10-11)

**Verdict: ACCEPT with notes.** No must-fix items.

Scope: `SOURCE_FACT_NAMES` and `--include-source-facts` in `devicehunt/specifications.py`, the flag wiring in `devicehunt/__main__.py`, `tests/test_specifications_source_facts.py`, `docs/specification-search.md` and the README paragraph. All probes ran on copies under the scratchpad `review-practical/` directory. The live DB was opened read-only.

## Checks

1. **Default output is byte-identical with the flag off.** I made a scratch copy with the new code path removed:
   - Whitelist, `SOURCE_FACT_PATTERNS` and `source_fact_category` deleted.
   - `include_source_facts` plumbing removed.
   - `curated_statement_rows` renamed back to `statement_rows`.
   - The `--include-source-facts` argument and `args.include_source_facts` removed from `__main__.py`.
   - A `diff` of `__main__.py` against that copy shows only the flag and its pass-through differ. Nothing else in `__main__.py` changed.
   - I compared stdout (sha256) of baseline against new code over 8 queries, 3 formats and 6 argument sets: `""`, `NF5280M6`, `AIS800-32O`, `QuantaGrid D52B-1U`, `7726-32X-O`, `switch`, `PSU`, `R4023-F9102-A0`; table, json and csv; plain, `--vendor Accton`, `--limit -1`, `--limit 7`, `--exact-model`, `--exact-part-number --limit -1`.
   - Result: 144/144 identical. (My first loop reported diffs because of a harness import bug. The rerun with an explicit `sys.path` matched.)
   - The baseline is a reconstruction, not a git baseline, because the directory is untracked.

2. **Whitelist.** 58 of 1373 fact names are included. I read sample values for every one.
   - **Categories:** all map to a sensible category. Nothing is a software declaration, a benchmark score (no jOPS), a CPU TDP or a test-bench peripheral. `processor_power_limit` and `sm_*_processor_power_raw` are correctly out. The `sonic_*`, `pddf_*`, `stratum_*`, `em_*`, `dts_*`, `nos_*`, `obmc_*` and `fbobmc_*` names are out. `power` and `power_supply` are out (288 JSON blobs). `energystar_archive_*` and `redhat_cert_detail_form_factors` (`["Server"]`) are out. The exclusions are justified.
   - **Whole-name matching:** `fullmatch` is used, and the "no prefix leakage" mutant fails a test.
   - **Weak or misleading values** (none are blockers; see should-fix and notes):
     - `spec_form_factor = '--'` (2 rows) is a placeholder that gets shown as a physical fact.
     - `form_factor` includes PCIe card form factors on the Inventec FPGA IPU/SmartNIC C5020X records ("Full height / half length / single width standard PCIe card"). That is the device's own form factor, but not a chassis. The statement has no `U` value.
     - `energystar_{high,low,typical}_idle_watts` are bare numbers such as `485.80`. They are measured idle draw for a test configuration, with no unit, filed under `power_description`. Each device has 3 rows. They are not a PSU or hardware spec. A reader of the `statement` column alone would not know what they are.
     - `power_configuration_limit` (5 rows) and `power_limit` (1 row) are caveat sentences such as "this does not identify the PSU fitted to any particular unit" and "No aggregate power capacity... inferred". They are disclaimers, not facts. Shown as `source fact` power rows they add noise and could be skimmed as claims.
     - `power_statement_as_printed` includes `TDP 75W`, which is the SmartNIC card TDP, not PSU or system power. It is the device's own power, so it is acceptable.
     - Some `*_as_printed` cooling and physical names are procedure prose: `fan_service_constraint_as_printed` ("must complete the entire procedure within 25 seconds"), `failed_psu_slot_airflow_note_as_printed`, `back_panel_*_as_printed` (labels such as "Power supply (PS 0)") and `chassis_height_as_printed` (a sentence). They are all single Oracle PCA/Sun rows with correct attribution. They are not wrong, only not crisp practical facts.
     - `dmi_chassis_type` is a label, often `Desktop` or `Other` on servers. Both the code comment and the docs say so. Acceptable.
     - Five `netbox_airflow` and nine `netbox_power_ports` rows sit on NetBox PSU module-type devices (e.g. `PSU-48V-400-B`), not on the switches. They are correct for those devices and only appear when the query matches the module.

3. **Attribution and separation.**
   - Every row carries `origin` (`curated statement` or `source fact`) and `source_fact_name`. Source-fact rows keep `source_id`, `source_url` and `tier`. Verified in table, json and csv.
   - Curated rows get `origin='curated statement'` and an empty `source_fact_name`.
   - Ordering interleaves the two kinds by (brand, model, revision, device, source, record, category). Curated rows sort first within a category. Separation relies on the `origin` column, not on grouping. That is fine, but it should be stated in the docs.
   - Filters:
     - `--vendor` uses the same record-local affiliation `EXISTS` for both kinds. Live checks: `NF5280M6 --vendor Inspur` returned 0 rows without an Inspur affiliation. `T42S-2U --vendor Quanta` returned only the Quanta source-fact rows (0 curated).
     - `--exact-model` and `--exact-part-number` filter by `record_id`, so source facts are not borrowed from sibling records. A mutant that ignores `matched` fails a test.
     - `--limit` is applied after the merge and sort, for both modes. Limit 0, 3 and -1 behave as expected. A mutant that drops the limit fails a test.
   - With the flag on, the default 100-row limit can now truncate source facts that sort later. That is expected.

4. **Docs and README.**
   - The new `docs/specification-search.md` section is accurate against code and DB.
   - The README paragraph does not overclaim. It says "PSU quantity/rating" and not fitment, and `power_configuration_limit` is itself an explicit disclaimer. The docs correctly warn that family-name matching can pick up other configurations.
   - Small gaps (note):
     - The docs say source facts "are not vendor-reviewed statements", yet rows can show `tier=vendor` (SPEC submissions, vendor datasheets). That is the source tier, not review state, but the two can look contradictory. Say so explicitly.
     - ENERGY STAR idle watts are not described as per-test-configuration measurements.
     - NetBox module-type devices are not mentioned.
   - Pre-existing and unrelated: the line "`data/exports/specifications.json` and `.csv` contain all 913 current statements" is stale. See item 7.

5. **Tests (5 new, meaningful).** I ran 11 mutants against `tests/test_specifications_source_facts.py`:

   | Mutant | Result |
   |---|---|
   | add non-whitelisted `sonic_psu_count` to the list | caught |
   | `fullmatch` changed to `match` | caught |
   | drop `origin` | caught |
   | blank `source_fact_name` | caught |
   | curated rows labelled `source fact` | caught |
   | default path leaks the new code | caught |
   | source facts ignore the `--vendor` filter | caught |
   | limit not applied | caught |
   | exact part number ignores the match | caught |
   | provenance `source_url` blanked | caught |
   | `dmi_chassis_type` remapped to `power_description` | **survived** |
   | discovery filter (`device_id in ids`) removed | **survived** |

   The two survivors are gaps. The fixture has one device, so the discovery filter is never exercised for source facts. Only 5 of the 58 names have a pinned category.

6. **Test runs.**
   - `python3 -m unittest tests.test_specifications_source_facts tests.test_specifications tests.test_buying tests.test_listing_specifications tests.test_ocnos` ran 40 tests, OK.
   - `tests.test_catalog` and `tests.test_listings` also pass.
   - I did not run the full suite.

7. **Stale exports.**
   - `data/exports/specifications*.json` and `.csv` are dated Sep 27 03:30 and hold 913 rows. The live default output has 1237 rows (this predates the change).
   - `scripts/refresh_exports.py` imports `specifications` but never writes any `specifications*` file. The `grep` found no generator in `scripts/` or the current docs. Only the old history docs mention the CSV.
   - The only supported way to refresh them is to run the CLI and redirect. The exports were presumably made that way. Run these from the project root, flag off, so the files keep their old schema:
     - `python3 -m devicehunt specs --limit -1 --format json > data/exports/specifications.json`
     - `python3 -m devicehunt specs --limit -1 --format csv > data/exports/specifications.csv`
     - Per-vendor: `... specs --vendor Accton --limit -1 --format json > data/exports/specifications-accton.json`, and likewise for Celestica, Edge-Core, Oracle and Amazon Data Services (the last is expected to be empty).
     - `specification-scope-audit.json` and the `r4023-f9102-a0` exports were made separately. I did not verify the exact commands.
   - Then update the "913" claim in `docs/specification-search.md`. This is outside the scope of this change and is not a blocker.

## Must-fix

None.

## Should-fix

- Filter out placeholder values (`--`, empty, `N/A`) for the whitelisted names, or drop them in `statement_rows`. Two `spec_form_factor = '--'` rows currently show as `physical_description`.
- Reconsider `power_configuration_limit` and `power_limit`. They are caveats, not facts. Either drop them from the whitelist or note in the docs that they are disclaimers.
- Make `energystar_*_idle_watts` self-describing. Drop them, or document "measured idle watts, per test configuration, no unit". As bare numbers under `power_description` they are the easiest rows to misread as hardware ratings.
- Add tests:
  - A table-driven test pinning every included name (or at least one per regex group) to its category. This would have killed the `dmi_chassis_type` mutant.
  - A two-device fixture where only one device matches the query. This would kill the discovery-filter mutant.

## Notes

- `form_factor` also catches PCIe card form factors on the Inventec FPGA records. This is acceptable, but it is a card form factor and not a chassis.
- Procedure-prose and label facts (Oracle PCA/Sun `*_as_printed` rows) are low-value single rows. They are attributed correctly.
- Docs: say that rows interleave by device and category, so use `origin` to tell them apart. Say that `tier` is source tier, not review state.
- `buying-report` is untouched, as claimed. The flag changes only the `specs` command.

## re-review

**Verdict: ACCEPT.** No must-fix items. One cosmetic doc nit remains.

- **Byte-identity with the flag off:** 90 of 90 identical against the reconstructed baseline. That is 6 queries (`""`, `NF5280M6`, `AIS800-32O`, `7726-32X-O`, `switch`, `T42S-2U`) × 3 formats × 5 argument sets (plain, `--vendor Accton`, `--limit -1`, `--exact-model`, `--exact-part-number --limit -1`). `__main__.py` still differs from the baseline only in the flag and its pass-through.
- **Should-fixes verified against live data:**
  - **Placeholders:** the only two whitelisted values dropped across the whole DB are the two `spec_form_factor '--'` rows. Nothing else is filtered.
  - **Disclaimers:** `power_limit` and `power_configuration_limit` are out of the whitelist, with a code comment and docs text.
  - **ENERGY STAR idle watts:** these are now relabelled as "idle power as measured in the ENERGY STAR <high-end performance|low-end performance|typical or single> test configuration: N W".
  - **Component form factors:** exactly the two Inventec C5020X PCIe card rows get the `component form factor (not a chassis):` prefix. The broad `card|module` regex does not mis-prefix any chassis row.
  - **Docs:** the new wording is accurate. The "913" sentence is now a dated snapshot note with the regeneration command and says `refresh_exports.py` does not write the files.
- **Previously surviving mutants:** both now fail. Remapping `dmi_chassis_type` to power fails 2 tests. Removing the discovery filter fails 1. Additional mutants all fail: no placeholder dropping, no idle relabel, no component prefix, no `origin`, and prefix matching.
- **Tests:** the specs-related modules plus the new file ran 43 tests, OK. The full suite was not run, since it is running elsewhere. No repo files were modified.
- **Nit (cosmetic):** `docs/specification-search.md` still lists "`--`" among example printed values in the same paragraph that says placeholders are dropped. Remove it from the example list.
