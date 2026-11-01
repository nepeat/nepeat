# lookup match context: citation-only fact matches — handoff

## change
- `devicehunt/lookup.py` (match-context helpers only): new `strip_citations()` (+ `import re`, three module regexes, rule documented in the comment above them).
  `match_context` now tests each matching fact value twice: value proper (citations stripped) vs raw. Names matching in the value proper keep going to
  `fact_names` / `fact_name_count` (as before); names whose every matching value hits only inside citation text go to new detail key `facts_citation_only`
  (full sorted list) and `matched_in` gets `; cited-path only: N facts` (after `facts[...]`, before affiliations/hyperscaler/exhibits).
  A name with both a value match and a citation-only match counts as a value match. Which rows lookup returns is untouched; all other keys/column order unchanged (one new detail key).
- `tests/test_lookup_match_context.py`: +2 tests (rule table incl. non-citation parentheses; fixture labelling). 

## citation rule (conservative)
A parenthesised group is a citation only if every comma/semicolon-separated item is a locator:
- path + `:N`, `:N-N`, `:N,N`: path contains `/` (starts with letter/`_`; spaces allowed, e.g. DMI model dirs), or is a bare filename with an extension starting with a letter (`fani.c:192`); optional `first entry ` prefix (EM);
- `L<n>` (EM fan lines) or `line <n>`.
Plus a bare trailing `file:line` at the very end of a value (file without spaces, with extension or `/`); none exist in the current catalog, so it is rule-only.
Not citations (tested): `D52B-1U (C621)`, `FB_temp(0x4C)`, `Length(m):1/2/2.5`, PCI/USB ids `(8086:1521)`, `(Rack, 2U, 4:3)`, `(INSTALL:2)` (word-only path, left as value), `ratio 3:1`.
Catalog sample (read-only, all 65,930 facts): 7,126 groups stripped across dmi, coreboot, onl/sonic nos_*, em (L<n>, first entry), obmc, dts, onie-style; shapes inspected, all path/line locators.

## before / after `matched_in` (data/devices.sqlite read-only, `--limit -1`, "before" = same code with stripping disabled)
- NF5280M6, Inspur NF5280M6: `identity; facts[19]: dmi_baseboard_manufacturer, ... (+14 more)` ->
  `identity; facts[7]: dmi_baseboard_product_name, dmi_system_product_name, linuxhw_dmi_model_dir_as_printed, redhat_cert_model_field_as_printed, redhat_cert_product_name_as_printed (+2 more); cited-path only: 12 facts`
- NF5280M6, Linux BMC device tree aspeed-bmc-inspur-nf5280m6: `facts[9]` -> `facts[3]: dts_compatible, dts_model, dts_path; cited-path only: 6 facts`
- NF5280M6, IEIT SYSTEMS ... / Inspur IR5280M6: unchanged.
- Sun Server X4-2, Sun Server X4-2: `facts[16]` -> `identity; facts[5]: dmi_system_product_name, linuxhw_dmi_model_dir_as_printed, redhat_cert_product_name_as_printed, retired_product_label, source_heading; cited-path only: 11 facts`; other 5 rows unchanged.
- PFE1100-12-054NA: unchanged (`facts[1]: fbobmc_psu_part_string_as_printed`).
- D52B-1U (3 rows incl. `(C621)`, `(C624)`): unchanged.

## verification
`python3 -m unittest tests.test_lookup_match_context tests.test_lookup tests.test_lookup_batch`: 21 tests OK (19 prior + 2 new; pre-existing ResourceWarnings only).

## notes
- `scripts/refresh_exports.py` would regenerate lookup-*.json with the new key/string; not run.
- Remaining value-proper hits like `dts_path`, `linuxhw_dmi_model_dir_as_printed`, `source_heading` embed the model as printed path/heading text, not as a locator; intentionally still value matches.

## review follow-up (lookup-citations-review ACCEPT with notes)
- S1 rule tightened: a slash is no longer enough. A path locator needs a final segment with a letter extension (`fani.c`, `dir/x.dts`; no-dir filenames cannot contain spaces),
  or one of the three extensionless shapes derived from the live stripped groups: DMI dump dirs `<Class>/.../<12-hex id>` (Server 378, Desktop 30), `rootconf/...` (ONIE, 23),
  `src/...` (coreboot Kconfig, 52). `L<n>`, `first entry <file>:N`, `line <n>` kept. Bare trailing `file:line` uses the same path rule. Rule documented in the comment above `_PATH`.
  Now NOT stripped (tested): `(TX/RX:1,2)`, `(Rack A/B:3)`, `Ethernet0 (Eth1/1:4)`, `port Eth1/1:4`, `(RJ45/SFP:1-4)`, `speed 10G/25G:2`, `(Slot A/B:2)`, `(e1/1:2)`.
- Live recheck (read-only, current DB): stripped groups 7,378 before and after the tightening (same number; bare-trailing hits 0). One intermediate draft briefly gave 7,385 by accepting a spaced filename
  (`first base_bcm_chassis_map.pb.txt:25`); fixed so that stays a value.
- S2: catalog `scope` now says facts[N] / fact_name_count count fact names whose value proper contains the query, and citation-only names are counted separately as
  `cited-path only: N fact names` (`matched_in_detail.facts_citation_only`). The string now reads `fact names` (was `facts`) since it counts distinct names.
- Tests: fixture now includes the surviving-mutant case (same fact name `both` hit in a value and in a citation -> value match only, not in `facts_citation_only`) plus S1 labels
  as fixture facts and in the rule table. `unittest tests.test_lookup_match_context tests.test_lookup tests.test_lookup_batch`: 21 OK.
