# candidate c review (independent, Claude Sonnet 5.5), 2026-10-11 discovery batch waves 5-6

Verdict: ACCEPT with notes. No must-fix.

Scope: data/devices-candidate-discovery-2026-10-11-c.sqlite (sha256 baa016d2...), data/import-manifest-candidate-discovery-2026-10-11-c.json (d5799672...), integration stage-c / rebuild-c / compare-c / pre-promotion-suite-c logs, data/rebuilt-discovery-2026-10-11-c.sqlite. Live data/devices.sqlite (ab8ed2e3...) and data/import-manifest.json (5b54fcc8...) re-hashed at the end: unchanged. All SQLite opened `?mode=ro`; scratch only in the review-candidate-c scratchpad.

## Checks run

1. Manifest vs live. Only `imports`, `files`, `expected` differ; `campaign_gate` identical.
   - imports 137 -> 140; live list is an exact prefix; appended in order: redhat-cert detail supplement, linuxhw-dmi facts, firmware-boards facts (all `curated`).
   - files 3299 -> 3497 (+198: redhat 159 = 158 `detail-wave5/*.html` + supplement; dmi 30 = 29 `DMI/...` snapshots + facts; firmware 9 = 8 `coreboot/...` + facts). 0 existing hashes removed or changed; all 198 added hashes recomputed against disk: 0 missing, 0 mismatches.
   - expected: records 6626 -> 6821 (+195), devices 5015 -> 5042 (+27), 195 new `sources` entries (redhat-cert-detail 158, linuxhw-dmi 29, coreboot-mainboard 8, each count 1); nothing dropped or changed. Equal to stage-c (sources/records +195, affiliations +187, identifiers +254, facts +2227, devices +27).
   - Absent from imports and files: `NOT-FOR-IMPORT-linuxhw-dmi-board-fallback-leads`, `NOT-FOR-IMPORT-redhat-cert-cloud-instance-leads`, everything under `discovery-2026-10-11-kernel-psus`, the U-Boot yosemite4 capture (no `uboot*` source in the DB; the `yosemite4` ids that exist are pre-existing entity-manager/fbobmc rows). Capture manifests are not imported.
2. Preservation and gates. `SELECT * FROM live.t EXCEPT SELECT * FROM main.t` on all 62 tables (same table set): 0 live rows missing in the 57 non-FTS tables. Only the 5 `search_index*` FTS tables differ (rebuilt text; row counts 5015 -> 5042 in search_index and content). Growth only in sources, devices, records, affiliations, identifiers, facts. `gate_inputs('data', candidate manifest)` passes and `gate_projections(candidate_db, release)` returns clean (no raise). integrity_check ok, foreign_key_check empty. pre-promotion-suite-c: 669 tests OK, 0 failures/errors, 0 blocked network calls.
3. Inputs. sha256 on disk: Red Hat supplement 33cc6cc23569c648..., linuxhw DMI 138dd33272e2f90d..., firmware boards 63c1291df6b635bd...; all equal the shas given and the manifest `files` entries.
4. DMI privacy. Scanned every new fact, identifier, `raw_json` and source url/coverage (2871 strings) for MAC (colon/dash and bare), UUID, 12+ hex, 16+ hex, serial/asset-tag/service-tag words and long mixed alphanumerics. With the `(path:line)` citation and `[n of m dumps]` suffix stripped from all 270 distinct dmi value strings: 0 MAC, 0 UUID, 0 bare 12+ hex, 0 long mixed tokens, 0 "serial/asset" words. The raw-only 12-hex Hyve Rigel-HS9216 `Version: 5411C4050059` is not in the DB (one `dmi_value_omitted` fact: "dmi_baseboard_version: 12+ hex characters, per-unit identifier-like"). The only 12-hex strings in the imported data are the upstream dump file names inside source URLs, `snapshot` paths and the 408 fact citations (see note N1).
   - S1: capture-manifest has `retained_value_notes` (path, line 52, text, reason; says withheld from candidate). Applied.
   - S2: `files_not_retained` has 6 entries (5 Foxconn MAC dumps + AWS Elemental) with reasons; hashes honestly recorded as `null` with an explanatory note. Applied. The five MAC files are not on disk.
   - S3: `Sun Ultra 40 M2 Workstation` and `Ultra20 M2` have kind `workstation`. Applied.
   - S4: `Inspur SA5248M4` new device has kind `system`. Applied. `NF5270M3` DMI record carries kind `system` but merges onto the existing `server` device, kind unchanged.
5. Attachment and merges.
   - Red Hat supplement: 158 records on 158 distinct devices, 0 new devices, 0 new identities. Every record's device already holds a listing record with the same `redhat_cert_catalog_id` (158/158 sibling match, each device exactly 1 listing record). Roles all `listing_holder` (Inspur 67, Quanta 50, no target 41). Facts added are detail-page facts only (cert ids, versions, platforms, form factors, file+sha, CPU certs on 65).
   - DMI: 29 records -> 10 merged onto 9 existing devices + 19 new devices (+ 8 coreboot = the +27). Merges: Inspur NF5280M6, SA5212M4, NF5270M3 (exact), Quanta QuantaPlex T41S-2U (exact), and Oracle Sun Fire X4170 M3, Sun Fire X4270 M3, Oracle Server X5-2, Sun Server X4-2, and `Sun Fire X4270 M2 Server` (two DMI model dirs `SUN FIRE X4270 M2 SERVER` and `Sun Fire X4270 M2 SERVER` onto one device, giving 10 records / 9 devices). The five Oracle merges are case-only differences under brand `Oracle`. Device brand/model/title/revision/kind: 0 of 5015 live devices changed in the candidate (checked all columns). Kinds on merged devices unchanged (`server platform` stays on X4270 M2).
   - Coreboot: 8 new devices, brand `coreboot mainboard`, kind `host firmware mainboard definition`, 0 affiliations (consistent with the review: labels only). Ids/shape as reviewed; `coreboot_kconfig_romsize_select` is the renamed fact (8), U-Boot dropped (firmware S1/S2 applied).
6. vendor-inventory, all 13 targets, live vs candidate, `--limit -1 --format json`: manufacturer scope byte-identical (13/13) and factory scope byte-identical (13/13). `all` scope identical for 8 targets; differs for 5, with 0 identities removed and no existing row dropped:
   - Hyve 33/33/8/41 -> 35/35/10/43 (identities/source_records/sources/role_assertions); +C4I, +Rigel-HS9216.
   - TD SYNNEX 5/5/4/5 -> 6/6/5/6; +HYVE-ZEUS.
   - Oracle 1281/1604/93/2447 -> 1281/1610/99/2453; 0 new identities, 6 DMI records added to 5 existing rows.
   - Quanta 170/233/146/239 -> 175/289/202/295; +5 identities (D51B-2U (dual 10G LoM), Freedom, Leopard ORv2-DDR4, QSSC-98J_C2, S210-X12MS), 50 Red Hat + 6 DMI records.
   - Inspur 220/309/167/439 -> 221/380/238/510; +SA5248M4, 67 Red Hat + 4 DMI records.
   All new assertions are `listing_holder` / `other_association`. The coreboot rows add nothing to any target.
7. Owned baseline. `validate-inventory` candidate = total 10 / hits 10 / gaps 0 (document_snapshot_hits 9, label_photo_only_hits 1); the whole report equals live except `generated_at` and `eligible_sources`. ByteDance `validation/vendor-examples.json`: 0 hits / 2 gaps, rc=2 on both live and candidate, report equal apart from the same two keys. `bd_egs` is catalog identity `coreboot mainboard bytedance/bd_egs` (label only, `Vendor name: ByteDance` as a printed fact); it does not move the ByteDance target (all-scope output byte-identical) or the B4020/B4010 result.
8. Integrity / rebuild. Candidate and rebuilt: integrity ok, FK empty. Two-way EXCEPT candidate vs rebuilt on devices, records, affiliations, identifiers, facts, sources (observed_at / fetched_at excluded): 0/0 each. compare-c.log: "All 52 tables/indexes and baseline results match", rc=0.

## Lookups (`--db`, `lookup Q --limit -1 --format json`, catalog dataset, live -> candidate)

- `NF5280M6`: 4 -> 4 rows, no new identity. `IEIT SYSTEMS Co.,Ltd. NF5280M6` matched_in "identity; facts[4]: redhat_cert_model_field_as_printed, ..." (Red Hat detail merged); `Inspur NF5280M6` matched_in "identity; facts[19]: dmi_baseboard_manufacturer, dmi_baseboard_product..." (DMI). Other datasets (inspur_support, documents, bsmi) identical.
- `Sun Server X4-2`: 6 -> 6 rows; `Oracle Sun Server X4-2` gains DMI evidence, matched_in "identity; facts[16]: dmi_baseboard_manufacturer, ..."; rra dataset identical.
- `tiogapass`: 0 -> 1: `coreboot mainboard ocp/tiogapass`, kind host firmware mainboard definition, matched_in "identity; facts[13]: coreboot_board_info_board_name, ...". Honest, project tier.
- `bd_egs`: 0 -> 1: `coreboot mainboard bytedance/bd_egs`, same shape. Label only.
- `CS8200`: all 9 datasets identical live vs candidate (catalog 5 rows, 0 changed).

## Findings

Must-fix: none.

Should-fix (non-blocking):

1. `matched_in` over-lists DMI fact names (low). Every `dmi_*` fact value ends with its `(DMI/.../Model/<dump>:line)` citation, so a model query hits all of them and `matched_in` lists 19 dmi fact names (`NF5280M6`) though only the system/baseboard product-name facts hold the model as a printed value. The identity match is also present, so nothing is wrongly asserted, but the "facts[19]" count reads as 19 corroborations. Either exclude the citation tail from the lookup fact match, or collapse same-record citation hits in `matched_in`. Same for coreboot (`tiogapass` facts[13]: citations include the directory).
2. DMI source `coverage` for NF5270M3 and SA5248M4 repeats the sentence "The upstream TYPE for this model directory is Desktop and the printed Chassis Type is not Rack Mount; the kind is deliberately neutral (system)..." twice. Cosmetic, but it is in the shipped text; dedupe on a later facts revision.

Notes:

N1. Dump file names in URLs/citations. 12-hex upstream hw-probe dump IDs (for example `Server/Quanta/Freedom/Freedom/53CBFD82B356`) appear in 29 source urls, 29 snapshots and 408 fact citations. They are upstream public file names, not MACs (the five dumps that did contain a MAC were deleted), and are needed to cite lines. Buyers should not read them as hardware identifiers.
N2. System SKU Number values are retained as printed and look like Oracle/Sun configuration part numbers (`30426023+5+1`, `4739029-1`, `7042862`) or short vendor strings (`BD3G7`, `SVR002255`, `HYVE-ZEUS-SH`). None is a serial, MAC, UUID or asset-tag field; coverage text says SKU "may differ between submitted units". Low risk; mention if the privacy rule is tightened.
N3. Brand-split twins continue (consistent with candidate b should-fix 1). New devices under `Quanta Cloud Technology Inc.` (QuantaGrid D54Q-2U, QuantaMicro X10E-9N, QuantaPlex T22HF-1U) sit outside the `Quanta` target; `Quanta D51B-2U (dual 10G LoM)` is a separate twin of `Quanta D51B-2U`; five Sun Microsystems / `SUN MICROSYSTEMS` devices (X2200 M2 x2, X2270, X4170 M2 SERVER, X4270 SERVER) sit beside `Oracle` Sun Fire rows, and `SUN FIRE X4170 M2 SERVER` is a case-variant of the existing `Oracle Sun Fire X4170 M2` under a different brand. Merging is exact brand+model only (Oracle case-fold of model under the same brand); no cross-brand merge occurred.
N4. 8 coreboot records have no affiliation (187 affiliations for 195 records). Intentional (labels, not roles) and reviewed.
N5. `eligible_sources` grows by 195 (all new sources carry sha256 + cache_path). The 158 Red Hat detail sources are real per-page captures; the 29 DMI sources use the first dump as snapshot and say so.
N6. Wording reminder for any promotion note: `bd_egs`/ByteDance coreboot label is not evidence for ByteDance B4020/B4010; ByteDance stays 0/2.
