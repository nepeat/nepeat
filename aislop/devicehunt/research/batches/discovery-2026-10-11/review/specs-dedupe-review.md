# specs_dedupe review (2026-10-11)

**Verdict: ACCEPT with notes.** No must-fix items. The test gaps below are should-fix.

Scope: `SOURCE_FACT_LABELS`, `source_fact_statement`, `DMI_CITATION`, `dedupe_source_facts` in `devicehunt/specifications.py`; the 6 appended tests plus 3 changed assertions in `tests/test_specifications_source_facts.py`; and the docs paragraph. The listings.py change was ignored. All probes ran on copies under the scratchpad `review-practical/`. The live DB was opened read-only and no repo files were touched.

## Checks

1. **Flag-off output is byte-identical.** I rebuilt the baseline from the current module, removing the source-fact path, dedupe, and the flag in `__main__.py`. I compared sha256 of stdout over 6 queries (`""`, `NF5280M6`, `AIS800-32O`, `7726-32X-O`, `switch`, `T42S-2U`) × 3 formats × 5 argument sets (plain, `--vendor Accton`, `--limit -1`, `--exact-model`, `--exact-part-number --limit -1`): 90 of 90 identical. `__main__.py` is not touched by this change (the flag wiring is from the earlier review).

2. **Labels.** Coverage is exact: every whitelisted name that occurs in the DB has a label or special wording, and no label is orphaned. I reviewed all of them and none overclaims.
   - Provenance is stated on the labels where it matters: "as submitted to SPEC", "(library entry)", "(as printed)", "ENERGY STAR ... configuration", "(compatibility list)".
   - The NetBox `maximum_draw` label says "library entry; not PSU capacity or measured draw", which is the right hedge. The raw `PS1: maximum_draw=337 (line 12)` text and the `(file.yaml:line)` suffixes are kept.
   - The label `(W)` on `spec_power_supply_quantity_and_rating_w` and the ENERGY STAR rated-output labels is accurate. The values are things like `2 x 800W` or `1,400`, and the label does not hide them.
   - Minor wording points, not defects: `chassis height (as printed): only components accessible from the rear of the 3U server chassis may...` is a prose sentence under a "height" label; `power statement (as printed): TDP 75W` is a SmartNIC card TDP. Both are single rows with correct attribution and were already noted in the first review.
   - Raw values stay visible in every case: the format is always `<label>: <raw value>`. The special cases (ENERGY STAR idle watts, component form factor) keep their earlier wording and include the value.

3. **DMI stripping.** I ran all 29 live `dmi_chassis_type` values through `source_fact_statement`. Each came out as `chassis type reported by firmware in submitted dmidecode dumps: <type>`, with the type text intact (`Desktop`, `Other`, `Rack Mount Chassis`, `Main Server Chassis`) and the `[N of N dumps]` tail kept verbatim. The regex only removes a parenthesised group containing `/` and `<12 hex>:<n>`, and a value with other parentheses and no path is left alone (test checks the no-citation case). Dropped from the text: the vendor/family/model path and the `:NN` suffix. The directory path, including the `Desktop`/`Server` probe category and the 12-hex dump id, remains in `source_url`. The `:NN` suffix is not in `source_url`, so it is lost. It is a line number or id in the listing and is of negligible value. The label wording "firmware in submitted dmidecode dumps" is accurate and not overclaiming. Docs and the code comment both say that Desktop/Other on servers is a label, not a spec.

4. **Dedupe.** The key is (device_id, category, statement, source_fact_name, source_id). It applies only to `origin == 'source fact'` rows, at the end of the flag-on path, so curated rows are untouched and different sources are never merged. I verified live: flag on gives 2246 rows (1237 curated, 1009 source-fact rows) and zero duplicate keys, so dedupe currently removes nothing. The handoff's finding is correct. The audit's 415 repeats come from collapsing across different sources and device-wide fact names, which the instruction forbids. The code is therefore a safeguard, not a visible change. The docs paragraph describes it as such.

5. **The 3 changed assertions** now expect labelled text (`'PSU quantity and rating (W): 1 x 650'`, `'form factor: 2U'`, the NetBox entry with its raw `PS1: maximum_draw=337 (line 12)` tail). They still test the same facts, categories, sources and raw values. The new `endswith`/`assertIn` checks add to them. They were not loosened, as far as I can tell (no git baseline to diff against).

6. **Mutants.** 12 run against `tests/test_specifications_source_facts.py`:

   | Mutant | Result |
   |---|---|
   | drop labels (return raw) | caught (5 failures) |
   | blank label lookup | caught |
   | greedy DMI regex `\(.*` | caught |
   | DMI regex also eats `[N of N dumps]` | caught |
   | dedupe ignoring `source_id` (cross-source merge) | caught |
   | dedupe applied to curated rows | caught |
   | dedupe off | caught |
   | NetBox label hedge removed | caught |
   | DMI label shortened | caught |
   | DMI regex strips any `(...)` | **survived** |
   | dedupe key without `device_id` | **survived** |
   | dedupe key without `source_fact_name` | **survived** |

   No live harm today, since live data has no duplicates and no DMI value has other parentheses, but the guards are untested.

7. **Test runs.** `python3 -m unittest tests.test_specifications_source_facts tests.test_specifications tests.test_buying tests.test_listing_specifications tests.test_ocnos` ran 49 tests, OK. I did not run the full suite.

## Must-fix

None.

## Should-fix

- Add a DMI case whose type text contains parentheses, e.g. `'Other (Foo) (Server/X/Y/Z/AABBCCDDEEFF:66)'`, and assert `(Foo)` survives. This kills the "strip any parentheses" mutant.
- Extend the dedupe test with:
  - the same statement and source on two different devices, which must be kept;
  - two different fact names with the same statement from one source, which must be kept.
  These kill the `device_id` and `source_fact_name` key mutants.

## Notes

- Dedupe currently has no visible effect on live data; the docs say so, which is good. If repeats from different SPEC submissions eat the default 100-row limit, a display-only "x N sources" fold would be a new decision, not part of this change.
- The `:NN` suffix of the DMI citation is dropped from both the text and `source_url`.
