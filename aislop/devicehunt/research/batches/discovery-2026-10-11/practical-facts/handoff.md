# practical_facts handoff (2026-10-11)

## What changed
- `devicehunt/specifications.py`: module constant `SOURCE_FACT_NAMES` (reviewed, whole-name regexes, one comment per group, mapped to the four categories), `source_fact_category()`, and `include_source_facts=False` on `search`/`statement_rows`/`part_number_statements`. The original query is now `curated_statement_rows` unchanged.
- `devicehunt/__main__.py`: only the `specs --include-source-facts` flag.
- Tests: `tests/test_specifications_source_facts.py` (5 tests). Docs: README paragraph, `docs/specification-search.md` section.
- Flag on: every row gains `origin` (`curated statement` / `source fact`) and `source_fact_name`; `category` stays canonical; `source_id`/`source_url`/`tier` kept. `--vendor`, `--exact-model`, `--exact-part-number`, `--limit` apply to source facts identically (same record-local joins). `buying-report` has no natural hook (it already prints unclassified facts per record); not touched.

## Default output unchanged
No git baseline (directory untracked), so: flag-off rows (1237) equal the flag-on `origin=curated statement` rows minus the two new columns (`True`), and the original query body is moved verbatim. Existing tests pass. Note `data/exports/specifications.{json,csv}` already differ from live default output (stale: 913 statements in docs vs 1237 live); not caused by this change, not refreshed (no writes under data/).

## Included fact names (matches in catalog; counts of device-attached facts)
- power: spec_power_supply 4, spec_power_supply_details 60, spec_power_supply_quantity_and_rating_w 200, spec_shared_power_supply_details 1, spec_shared_power_supply_quantity_and_rating_w 1, netbox_power_ports 20 (maximum_draw), netbox_power_port_count 20, energystar_{high,low,typical}_{psu_count,psu_model,psu_rated_output_watts,idle_watts}, psu_configuration_1-3, psu_rated_output_raw 4, psu_model_as_printed 2, psu_redundancy_as_printed, psu_interchangeability_as_printed, power_configuration_limit 5, power_limit, power_statement_as_printed 6, power_redundancy_statement_as_printed, power_supply_note_as_printed, power_supply_part_numbers_as_printed, power_supply_fan_tray_part_numbers_as_printed, back_panel_power_supplies_as_printed
- cooling: netbox_airflow 20, failed_psu_slot_airflow_note_as_printed, fan_service_constraint_as_printed, back_panel_fan_modules_as_printed, rear_fan_part_numbers_as_printed
- physical: netbox_u_height 22, netbox_is_full_depth 22, netbox_weight 15, form_factor 42, spec_form_factor 206 (some values are `--`), spec_shared_form_factor, energystar_form_factor 3, chassis_height / chassis_height_as_printed, chassis_dimensions 8, compatibility_dimensions_in/mm 21, compatibility_weight 21, sm_1u/2u_chassis_description, dmi_chassis_type 29 (label only; often Desktop/Other on servers), spec_nodes_per_system 149, spec_number_of_identical_nodes 57, spec_shared_enclosure
- Total 1017 source-fact rows across 2254 rows (unlimited, flag on).

## Deliberately excluded
- sonic_* (psu/fan/thermal counts+names), pddf_*, stratum_phal_*/stratum_node_count, em_exposes_psu/fan_entries, dts_psu_compatibles, nos_airflow_*, nos_psu_*, obmc_fan_psu_related_lines, fbobmc_psu_*: declarations read from NOS/BMC source trees about a platform directory (can span SKUs, count code entries), not hardware specs; still findable via `lookup`/`buying-report`. Could be added later under a separate "software declaration" label.
- devicecode `power` / `power_supply` (288 each): JSON adapter-schema blobs with empty fields.
- processor_power_limit, sm_*_processor_power_raw (CPU TDP); energystar_archive_* (test-config columns, many non-PSU); redhat_cert_detail_form_factors (`["Server"]` = system class, not chassis); ubuntu_cert_*/redhat_cert_* component lists; node_models/server_node_model/head_node (identity, not counts); compatibility_operating_temperature; fbobmc_spec_overview_lines.

## Verification (read-only, trimmed)
- `specs NF5280M6 --include-source-facts`: now returns rows, e.g. `Inspur NF5280M6 power_description 2 x 800W spec-jbb2015-20210519-00650 vendor source fact spec_power_supply_quantity_and_rating_w`; `physical_description 2U ... spec_form_factor`; `Rack Mount Chassis (...) linuxhw-dmi-server-inspur-nf5280-nf5280m6 community source fact dmi_chassis_type`.
- `specs 'QuantaGrid D52B-1U' --include-source-facts`: still "No matching records." Honest: its records carry only Red Hat/Ubuntu certification facts (excluded; no practical fact). Sibling `QuantaGrid D52B-2U` shows `spec_form_factor 2U Rack`.
- `specs 7726-32X-O --include-source-facts`: `netbox_u_height 1.0`, `netbox_is_full_depth true` (community, netbox-dt-edgecore-7726-32x-o), plus AS7726-32X `compatibility_dimensions_mm 438 x 500 x 43.4 mm`, `compatibility_weight ~9.8 kg`, alongside the curated rows.
- `specs T42S-2U --include-source-facts`: QCT `form_factor 2U Rackmount, 4 Nodes`; Quanta `spec_form_factor 2U`, `spec_number_of_identical_nodes 4`, `spec_shared_enclosure T42S-2U`, `spec_power_supply_quantity_and_rating_w 1 x 2200`, `spec_shared_power_supply_details ARTESYN MC2200B4-3-4R-02`; curated QxVDI statements retained.
- Caveat: default discovery search matches family names, so rows can belong to a different configuration of the family; use `--exact-model`/`--exact-part-number` before buying.

## Tests
`python3 -m unittest tests.test_specifications_source_facts tests.test_specifications tests.test_buying tests.test_listing_specifications tests.test_ocnos`: 40 tests OK (5 new). Full suite not run.

## Review follow-up (applied)
- Placeholder values (`--`, empty, `N/A`, none, TBD etc.) are dropped from source-fact rows.
- `power_limit` and `power_configuration_limit` removed from the whitelist (disclaimer sentences).
- ENERGY STAR idle watts kept, rewritten as `idle power as measured in the ENERGY STAR <config> test configuration: N W`.
- `form_factor` values naming PCIe/OCP card, module or mezzanine are prefixed `component form factor (not a chassis): `.
- Docs: source facts are "source-recorded facts, not curated statements; tier shows the source type"; notes on interleaving by `origin`, labels, NetBox PSU-module devices.
- Tests: table pinning every whitelisted name to its category (plus excluded probes), placeholder/label test, two-device discovery-filter test (kills both survivors). 
- Re-verified: flag-off `specs --limit -1 --format json` byte-identical to the pre-change capture. Flag on now 1009 source-fact rows (was 1017), 0 placeholders.
- Tests: `tests.test_specifications_source_facts tests.test_specifications tests.test_buying tests.test_listing_specifications tests.test_ocnos`: 43 OK. Full suite not run. Exports not refreshed (no writes under data/).

## coordinator follow-up

- re-review ACCEPT; its docs nit applied by `/root-claude` (removed `--` from the "values are as printed" examples).
- `docs/specification-search.md` "913 current statements" sentence rewritten as a dated 2026-09-27 snapshot note with
  the regeneration command; `data/exports/specifications*` were not regenerated.
