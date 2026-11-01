# buyer-workflow audit — discovery batch 2026-10-11 (candidate f catalog)

auditor: independent read-only buyer persona (Claude Sonnet 5.5). Live catalog `data/devices.sqlite`
(5,107 identities); pre-batch comparison `data/devices-before-discovery-2026-10-11.sqlite` via `--db`.
Method: ~45 CLI invocations covering ~28 workflows; scratch outputs outside the repo. No writes, no network.

## verdict: mixed, leaning net helpful

The additions are a clear gain for the actual buyer questions: exact hardware titles now reach more evidence
(QuantaGrid D52B-1U went from no match to a match; NF5280M6 gained Red Hat/Ubuntu/SPEC/DMI sources; Sun Server X4-2
gained DMI and Red Hat sources), `lookup` `matched_in` explains why a row appeared, PSU strings now lead to the
servers and platforms that mention them, and `specs --include-source-facts` surfaces PSU ratings, U height and airflow
that were invisible before. Owned baseline is intact (10/10, search_hits/evidence/leads identical before and after),
manufacturer scope is byte-identical, there were no crashes or tracebacks, and every command ran under 1.7 s
(`lookup` about 1.4-1.7 s because it scans all registries; everything else under 0.4 s).

It is not net-clean because of one real regression and several confusing presentation issues:

1. Bare-word software-reference identities (`wedge`, `fuji`, `yosemite`, `catalina`, `transformers`, `zaius`, `mori`, ...)
   are scored as `model_mention` in `match-listing`, the strongest label, and produce false positives on ordinary words
   and on genuine hardware titles. They also sort ahead of real hardware under a small `--limit`.
2. Brand-split identities (IEIT / Inspur / Inspur Corporation / IEI; Quanta / QCT / Quanta Cloud Technology) make one
   box a 3-5 candidate list, and `vendor-inventory Inspur|Quanta` silently omits the other brand strings.
3. `specs --include-source-facts` emits bare values ("1", "2U", "1 x 650") and 41% duplicate rows; `buying-report`
   still says `power: not_collected` for models whose PSU is shown by `specs`.

## findings

Severity: bug = wrong or regressing result; confusing = correct but a buyer will likely misread it; note = observation.
Timings are wall-clock for one invocation.

| # | sev | command | evidence |
|---|-----|---------|----------|
| F1 | bug | `match-listing --title 'Wedge 100BF-32QS'` | Before: `no_catalog_mention`. Now: sole result `model_mention Meta OpenBMC platform wedge` (source `fbobmc-wedge-wedge`, `affiliations: []`). The real hardware (`Accton Wedge100BF-32QS`, `Wedge 100BF-32`, etc.) is not returned because the title has no space-free token; the regression replaces an honest miss with a software-file hit labelled as the strongest match type. Same for `Wedge 100S-32X Facebook` (extra `wedge` row beside the real Accton row). |
| F2 | bug | `match-listing --title 'Fuji Xerox DocuCentre printer'`, `'Transformers Optimus Prime figure'`, `'Yosemite sample pack of Chocolate'`, `'Zaius Power9 Barreleye server'`, `'Quanta Mori server board'` | Before: all `no_catalog_mention`. Now each returns `model_mention` on `Meta OpenBMC platform fuji` / `OpenBMC machine transformers` / `...yosemite` / `...zaius` / `...mori`. There are 19 such bare dictionary-word model strings (Meta OpenBMC: acctonbmc, bletchley, catalina, clemente, celesticabmc, clearcreek, elbert, emeraldpools, northdome, fuji, minipack, wedge, yosemite; OpenBMC machine: kudo, mori, zaius, starscream, transformers, lannister). The match labels give no indication the identity is a software file. |
| F3 | bug | `match-listing --title 'Oracle Sun Server X4-2 Fuji Transformers Zaius Mori Yosemite Wedge Elbert' --limit 5` | Top 5 of 8 candidates are all software-reference rows (`elbert fuji wedge yosemite mori`); the only hardware candidate (`Oracle Sun Server X4-2`) sorts last (within a match type the order is by brand, and `Meta`/`OpenBMC` precede `Oracle`). Contrived, but it proves software rows can truncate out hardware. The same ordering put `Meta ... minipack/wedge/fuji/catalina/elbert` ahead of Celestica/Oracle rows in a 26-candidate Wedge/Minipack title. |
| F4 | confusing | `match-listing --title 'Catalina 2.4GHz Wi-Fi router Quanta'`, `'Facebook Minipack switch'` | Hardware row (`Amazon Data Services Catalina`, `Accton/Edge-Core Minipack`) now accompanied by an equal-rank `Meta OpenBMC platform` row of the same name. Two same-name candidates with no hint which is hardware. |
| F5 | confusing | `match-listing --title 'Inspur NF5280M6 2U server 2x Xeon Gold'` | Before: 1 candidate (`Inspur NF5280M6`). Now 3 equal `model_mention`s: `IEIT SYSTEMS Co.,Ltd. NF5280M6`, `Inspur NF5280M6`, `Inspur Corporation | Inspur NF5280M6`. `Inspur NF5280A7` yields 5 (IEIT Systems / Inspur / Inspur Corporation x1 / IEI x2, including same-brand "Inspur NF5280A7" vs "NF5280A7"). Each has distinct sources, so a buyer must merge by hand. Helpful that evidence is richer; confusing that nothing says these are probably the same product. `model-collisions --target Inspur` has the group list (59 groups) but match-listing does not point to it. |
| F6 | confusing | `vendor-inventory Inspur --scope all --limit -1` / `Quanta --scope all` | Inspur: 223 identities, all brand `Inspur`; the 67 `IEIT SYSTEMS Co.,Ltd.` + 28 `IEIT Systems Co., Ltd.` + 33 `Inspur Corporation` + 11 IEI + 21 Power Commercial + 3 other identities are not counted. Quanta all: 175 (was 78; +94 `listing_holder`); 14 `Quanta Cloud Technology`/`Quanta Cloud Technology Inc.` identities (incl. QuantaGrid D43K-1U with 27 sources) absent; the 9 `QCT` identities appear only through a `historical_subsidiary_portfolio` affiliation. Manufacturer scope is identical to before (Quanta 41, Inspur 3), so these are the intended conservative counts, but the headline "Quanta 175" understates family coverage while `listing_holder` rows (94 of 175) make `--scope all` look like production coverage. |
| F7 | confusing | `vendor-inventory Quanta --scope all` vs `--scope manufacturer` | all = 175 identities, 94 are `listing_holder` (certification/benchmark listings), 41 manufacturer. Role column is shown, so it is correct, but the scope=all header sentence does not warn that most new rows are listing holders. |
| F8 | bug (consistency) | `buying-report NF5280M7` / `NF5180M7` vs `specs NF5280M7 --exact-model --include-source-facts` | buying-report: `power: not_collected` for all six categories (964 unclassified facts). specs with the flag: `1 x 1300w Great Wall Power; G_M_GW-CRPS1300D2W_1300W_1U_P; IEIT PN:V031052000000000`, `2U`. A buyer who reads only the report concludes there is no PSU data. Documented ("buying-report unchanged") but contradictory in practice; the not_collected text points to "unclassified facts" which are not summarised. |
| F9 | confusing | `specs --include-source-facts` (any SPEC/NetBox model) | Statements are raw values with no label in the statement column: `1` (spec_number_of_identical_nodes, category physical_description), `1 x 650` (no unit/meaning), `2U`, `1 (module-types/Edgecore/YM-2651Y-B01R.yaml:7-9)` as a power_description. Only the `source_fact_name` column explains them. In CSV/JSON a consumer will not see the meaning unless it reads that column. |
| F10 | confusing | `specs --include-source-facts --limit -1 --format json` | 415 of 1,009 source-fact rows (41%) are exact duplicates by (brand, model, category, statement) because several SPEC results exist per model (e.g. `Mt. Collins` PSU/form factor rows each appear twice; `NF5280M5` appears 4x). Default `--limit 20` is therefore consumed by repeats. Also redundant with curated: `AS7326-56X` curated "438x536x43.5mm; 10 kg" plus 3 source-fact rows saying the same. |
| F11 | confusing | `specs "Oracle Server X5-2" --exact-model --include-source-facts`; `specs NF5270M3 ...` | Only source fact is `dmi_chassis_type`: `Main Server Chassis (Server/Oracle/SERVER/SERVER X5-2/B6860ED621C9:68)`. The parenthetical is an internal linuxhw path with a 12-hex probe hash that looks like a MAC address. `Inspur NF5270M3` shows `Desktop (Desktop/Inspur/...)` and `SA5248M4` `Other (Desktop/...)` (from the desktop directory; the catalog identity is a server); `Sun Ultra 40 M2 Workstation` shows "Main Server Chassis". These are firmware-reported strings but read as chassis facts. |
| F12 | confusing | `specs 5912-54X-O-AC-F --include-source-facts`; `specs YM-2651Y --include-source-facts` | NetBox `maximum_draw` values are shown under `power_description`: switch `5912-54X-O-AC-F` `PS1/PS2 maximum_draw=337` vs `5812-54X-O-AC-B` `750`; the PSU module `YM-2651Y B01R` `maximum_draw=650` (a 650 W PSU rating, per the Edgecore ordering doc) is the same field name with a different meaning (supply capacity vs device allotment). Tier `community` is shown but the statement does not say "library value". A buyer sizing a PDU could misuse 337. |
| F13 | confusing | `match-listing --title 'YM-2651Y power supply'`; `'Edgecore YM-2651Y-B01R 650W AC PSU AS7326-56X'` | `no_catalog_mention` for bare `YM-2651Y` although the catalog holds four identities (`YM-2651Y B01R/C01R` NetBox, `YM-2651Y, B01R/C01R` ordering doc, brand `Unattributed accessory`). Hyphenated `YM-2651Y-B01R` (how eBay titles write it) also misses; only the exact space form `YM-2651Y B01R` or comma form hits. Same PSU therefore exists as two differently spelled identities. `lookup YM-2651Y` finds all four plus an unrelated `FOXCONN (FII) Mt. Collins` (`YM-2651YA10R`, substring) with `matched_in facts[1]: spec_power_supply_details`. |
| F14 | confusing | `match-listing --title 'Delta DPS-800AB PSU'`; `lookup DPS-800AB` | Match-listing: no match. Lookup: six Inspur servers (`NF5280M4/M5`, `NF5180M5`, ...) via `spec_power_supply_details` = "Delta DPS-800AB-9X P/N:V07LP0400000000F". Useful lead (which server uses that PSU), but lookup shows only the fact name; the PSU string requires `show`/SQL. No PSU identity exists for DPS-800AB (only `DPS-850AB-5 A/6 A` from the Edgecore ordering doc), and a titled "DPS-800AB-9X" would hit nothing. |
| F15 | note | `search PFE1100`, `search TDPS-1500AB` | Before: only ONL platforms / Celestica rows. Now `Meta OpenBMC platform minipack/wedge/wedge100` rank first for `PFE1100` (they contain the string as a PSU part). Labelled `(software reference)` in `kind`, so this reads as a lead. `TDPS-1500AB` additionally lists a SONiC platform. Helpful for PSU hunting; only rank placement above hardware is debatable. |
| F16 | note | `lookup PFE1100-12-054NA`, `lookup 'Oracle Corporation Asm,MB+Tray,E5-2c'` | PFE1100 string resolves to `Meta OpenBMC platform minipack` with `matched_in facts[1]: fbobmc_psu_part_string_as_printed`; the E5-2c FRU string resolves to the owned E5-2C via `reported_motherboard`. Helpful: exactly the identification a buyer wants from a FRU/DMI string, and the "discovery lead, not fitment" caveat is in the README. `lookup S5Q-1U` and `YM-2651Y` degrade gracefully. |
| F17 | confusing | `shopping 0401e1a26671f300df5b`, `shopping 32598b15a60784f4cfd2`, `shopping b764ab4c2c47be5a1677` | Query string is `brand + model`. New brands give `IEIT SYSTEMS Co.,Ltd. NF5280M6`, `Inspur Electronic Information Industry Co., Ltd. (IEI) Inspur NF5280A7` and `Meta OpenBMC platform wedge`; the eBay link will find nothing useful. |
| F18 | note | `model-collisions --target Quanta --limit 10` (215 ms) | 19 groups, useful: D54Q-2U x3, D43K-1U, D51B-2U Quanta vs QCT. One false lead: `A5QB` Hyve server vs Quanta "wireless device server" (flagged `short_key kinds_differ`, so appropriately warned). `--target Inspur` 59 groups (22 `brand_token_prefix_different_brand`, 37 same_model) shows the five-way NF5280A7 splits. Not linked from `match-listing` output. |
| F19 | helpful | owned set (all 10): `lookup` / `buying-report` / `specs` | 10/10 exact; unchanged vs before. `lookup` gives `matched_in` (e.g. `E5-2C` row 1 `identity; facts[2]: reported_motherboard, reported_system_model`; the RRA rows `E5-2c 1U` show `identity` with `identity_text_match False`). New benchmark/VMware identity for E5-2c adds `compute_network_storage` and BIOS rows to `buying-report E5-2C` (chassis/firmware/compute now collected; power and cooling still `not_collected`). `buying-report` power is collected only for `7760-32X-A-AC-F`. `specs` for all ten returns "No matching records." by default; with the flag only X7-2C (`SM 1U`/`SM 2U`) and X10-2C (via a differently named `X10-2c GPU L40S Compute Server` identity: 3 RU, fans, A271/A271A 1400 W PSU) return rows. |
| F20 | confusing | `specs X7-2C --include-source-facts` | Statements `SM 1U` and `SM 2U` (fact names `sm_1u/2u_chassis_description`) come from an Oracle VPAT accessibility template; "SM" is unexplained and the two heights for one model read as an option choice. X7-2C `buying-report` has none (not mapped). |
| F21 | note | `match-listing 'Sun Oracle X4-2 server'` | Before and after: one `identifier_mention Oracle Sun Server X4-2`. Source list grew (linuxhw DMI, Red Hat, oracle-eol): helpful. `search 'Sun Server X4-2'` ranks Netra/Exalytics/X3-2 above it: unchanged. |
| F22 | helpful | `match-listing 'QCT QuantaGrid D52B-1U barebone'`, `'Celestica DX010 Seastone'`, `'Edgecore AS7726-32X ...'` | D52B-1U: none -> match (ubuntu/redhat sources). DX010/AS7726: identical results before and after; software `SONiC platform x86_64-cel_seastone-r0` rows correctly do not appear in match-listing because their model strings are long. |
| F23 | helpful | `lookup` on `wedge`, `Minipack` (56 / 33 catalog matches) | Output is brand-sorted and capped at 20, so hardware (Accton, Celestica, Edge-Core) fills the page; software rows fall past the cap. No crowding in `lookup`, unlike `match-listing` (F3). |
| F24 | helpful | `match-listing ... --vendor Accton` on `Wedge 100BF-32QS` | `--vendor` requires an affiliation on the matching record, and software identities have `affiliations: []`, so the vendor filter removes all F1-F4 false positives. This is an existing workaround, not documented as such. |
| F25 | note | performance, whole run | Every command exited 0, empty stderr, no traceback. Times: `lookup` 1.4-1.7 s (even `--limit -1` with 19k-line JSON); `match-listing` 0.34-0.4 s; `specs` 0.1-0.16 s (31k-line JSON with flag 0.16 s); `vendor-inventory`/`search`/`buying-report` 0.08-0.1 s; `model-collisions --limit -1` 0.21 s. Nothing near 10 s. `match-listing --csv /dev/null` gives a clean usage error. |
| F26 | note | `model-collisions`, `vendor-inventory` header text | Both carry explicit "review lead / no merge" disclaimers. Good. |

## concrete recommendations

Each is small and independent. Priority order.

1. Software-reference identities must not be `model_mention`. In `match-listing`, either exclude identities whose kind
   contains "(software reference)" / brand in {`Meta OpenBMC platform`, `OpenBMC machine`, `Linux BMC device tree`,
   `coreboot mainboard`, `Stratum platform`, `DENT platform`, `ONIE machine`, `ONL platform`, `SONiC platform`} by default,
   or emit a distinct `match_type` such as `software_reference_mention` that sorts after variant_leads. Add
   `--include-software-references` to opt in. This fixes F1-F4 with one filter; `lookup` and `search` can keep them.
2. At minimum (if 1 is rejected), require that bare-word software model strings (no digit, <=12 chars: the 19 listed
   above) not be eligible for `match-listing`, in the same spirit as the existing "no pure numbers / short names" rule.
3. Sort order inside a match type: put identities with at least one non-empty affiliation first, then software
   references, so truncation never removes hardware (F3).
4. Add a `family_note` / "also appears under: <other brand strings>" field to `match-listing` output (display only) for
   identities in a `model-collisions` group, pointing to `model-collisions --target X`. Do not merge. This addresses F5
   without the brand-family decision (the wave-7 proposal's display-only table would feed it).
5. `vendor-inventory X --scope all`: print a one-line role breakdown in the header (for example `listing_holder 94,
   registry_manufacturer 32, ...`) and a note naming the brand strings of the same company family that are excluded
   (IEIT/Inspur Corporation/IEI; Quanta Cloud Technology) (F6/F7).
6. `specs`: when `--include-source-facts` is set, render the statement as `<readable fact label>: <value>` (for example
   `PSU quantity x rating W: 1 x 650`, `nodes per system: 1`, `NetBox power-port maximum_draw (library value): 650`,
   `DMI chassis type (firmware string): ...`). Strip the linuxhw path/probe-hash parenthetical from `dmi_chassis_type`
   statements (F9, F11, F12). Drop `Desktop`/`Other` DMI chassis rows from `Desktop/...` dump directories or label them.
7. De-duplicate source-fact rows on (model, category, statement) and fold the count into one suffix (`x4 results`) so the
   default 20-row limit is not eaten by repeats (F10). Skip source-fact rows whose statement equals an existing curated
   statement for the same device (AS7326-56X).
8. `buying-report`: add a one-line pointer when `specs --include-source-facts` would return power/chassis rows for the same
   model ("source facts available: PSU, form factor; run specs ... --include-source-facts") instead of changing the
   category status (F8). Tiny change, keeps "unchanged" promise.
9. `shopping`: build the eBay query from a short brand token, not the legal brand string; skip software-reference
   identities (F17).
10. PSU-title matching (F13/F14): allow a variant lead for a plain `<code> + suffix` PSU family (`YM-2651Y` -> `YM-2651Y B01R`
    and `YM-2651Y, B01R`; `YM-2651Y-B01R` -> the same), limited to `Unattributed accessory`/FRU kinds, or add the two
    spellings as aliases of one identity. Spaces are intentional elsewhere, so scope it to the accessory kind.
11. Docs: add one sentence to docs/listing-matching.md that `--vendor` excludes software-reference identities and
    that unaffiliated `model_mention` rows are not hardware evidence.

## coverage list (workflows run)

1-10 owned models: `lookup`, `buying-report`, `specs` and `specs --include-source-facts` each (30 runs; plus `validate-inventory` 10/10); 11-18 the eight
suggested titles with before/after comparison; 19-24 extra titles (Wedge 100S/100-32X, Fuji, Catalina, Yosemite,
Transformers, Zaius, Minipack, Mori, YM-2651Y B01R forms, DPS-800AB-16, NF5280A7, QCT D43K-1U, Sun Server X4-2, Oracle
X5-2); 25 `search` on 8 terms vs before; 26 `lookup` on PFE1100-12-054NA, TDPS-1500AB, YM-2651Y, DPS-800AB, an
E5-2c board FRU string, S5Q-1U, Quanta Computer Inc; 27 `specs ... --include-source-facts` on AS7326-56X, QuantaGrid
D52B-1U (no rows either way), NF5280M5, Wedge 100S-32X (no rows), Oracle Server X5-2, YM-2651Y, 5912-54X-O-AC-F, Mt.
Collins, NF5280M7/NF5180M7, plus a global flag-on vs flag-off diff (1,237 curated + 1,009 source-fact rows); 28
`vendor-inventory` Quanta/Inspur/Oracle scope all vs manufacturer, with before comparison; 29 `model-collisions` three
variants; 30 `shopping`/`show` on software and brand-split identities.

## re-audit

Same rules (read-only, no network); live catalog after the three fixes. About 40 further invocations. No crash or
traceback (all rc 0, empty stderr). Timings: `match-listing` 0.32-0.37 s, `buying-report` about 0.1 s, `specs` about
0.15 s (unchanged from the first audit; nothing near 10 s).

| finding | status | evidence |
|---|---|---|
| F1 `Wedge 100BF-32QS` | resolved | default now `no_catalog_mention` (as pre-batch); with `--include-software-references` the same row returns as `software_reference_mention Meta OpenBMC platform wedge`. |
| F2 bare-word titles | resolved | `Fuji Xerox DocuCentre printer`, `Yosemite sample pack of Chocolate`, `Transformers Optimus Prime figure` all `no_catalog_mention` by default. `Wedge 100S-32X` and `Catalina ... Quanta` now return only the hardware row (`Accton Wedge 100S-32X`, `Amazon Data Services Catalina`). |
| F3 `--limit 5` Sun X4-2 title | resolved | default: one candidate, `Oracle Sun Server X4-2`. With `--include-software-references --limit 12`: hardware first, then 7 `software_reference_mention` rows (elbert, fuji, wedge, yosemite, mori, transformers, zaius), so software sorts last as specified. |
| F4 hardware/software same-name pair | resolved | the Minipack/Catalina duplicate software row is gone by default. |
| F8 `buying-report NF5280M7` | resolved | `power: not_collected` now followed by `source_facts_available: 5 (names: spec_power_supply_details, spec_power_supply_quantity_and_rating_w; see: python3 -m devicehunt specs NF5280M7 --exact-model --include-source-facts)`; chassis likewise (6). Category status is unchanged and the command is scope-faithful (`--exact-model`). |
| F9 bare values in specs | resolved | `PSU quantity and rating (W): 1 x 1300`, `PSU details as submitted to SPEC: ...`, `form factor as submitted to SPEC: 2U`, `number of identical nodes as submitted to SPEC: 1`, `dimensions (mm, compatibility list): ...`. |
| F11 DMI chassis text | resolved | `chassis type reported by firmware in submitted dmidecode dumps: Main Server Chassis` for X5-2; no dump path or hash. Residual: the NF5270M3 / SA5248M4 `Desktop` / `Other` rows keep the wording (now clearly firmware-reported, but still a server identity labelled Desktop). |
| F12 NetBox draw | resolved | `NetBox power port maximum draw entry (W, library entry; not PSU capacity or measured draw): PS1 ... maximum_draw=337 ...`; other NetBox fields carry `(library entry)`. |
| F20 `SM 1U` | resolved | now `chassis description (1U): SM 1U` (still opaque "SM", but labelled as a chassis description). |
| F10 duplicate source-fact rows | not resolved (not in scope of the fixes) | 382 of the exact duplicates remain (was 415; label changes merged a few); `specs NF5280M7 --exact-model --include-source-facts` still repeats each SPEC row. |
| owned 10 via match-listing | pass | all 10 still hit with only expected hardware rows: CS8200 (model + variant_lead CS8200-32X), CS8210, DMC-12-48R-AS0 (Accton + Edge-Core), 7760-32X-A-AC-F, AZ4444-52X-DC-10 (Accton + Edge-Core), AS7772-32X (2 model + 2 variant), E4-2C / E5-2C / X7-2C / X10-2C (`identifier_mention` on the Oracle Server identity). No software rows in any. |
| owned 10 via buying-report | pass | category statuses identical to the first audit (7760: power+firmware collected; E4/E5/X10: chassis+firmware+compute; X7-2C firmware only; CS/DMC/AZ4444 none). `validate-inventory` still 10/10. |

New issues (minor, none a regression):
1. `buying-report X7-2C` shows no `source_facts_available` although `specs "Oracle Server X7-2C" --exact-model
   --include-source-facts` returns the two `SM 1U` / `SM 2U` chassis rows. `buying-report X10-2C` likewise has none
   (its rows sit on the separate `X10-2c GPU L40S Compute Server` identity). Consistent with the scope-faithful
   design, but the hint is absent where specs would show data via the alias `X7-2C`; possibly the `sm_*_chassis_description`
   names are in the specs list but not the buying-report category map.
2. The `software_reference_mention` rows under `--include-software-references` still carry full source lists and
   no marker in the table beyond the match type; acceptable.
3. `specs YM-2651Y --exact-model --include-source-facts` returns "No matching records." (only the broad non-exact query
   finds the `YM-2651Y B01R/C01R` PSUs); unchanged and by design, but F13 (bare/hyphenated PSU titles missing) is
   still open.

Open from the first audit and untouched: F5/F6/F7 (brand-split candidates, inventory omissions), F10 (duplicates),
F13/F14 (PSU title matching), F17 (`shopping` query strings).
