# open-source discovery batch — 2026-10-11

coordinator / integrator / documentation editor: `/root-claude` (Claude Code, Opus 5.5).
workers and reviewers: Claude Sonnet 5.5 (fresh context; common brief
[brief-common.md](batches/discovery-2026-10-11/brief-common.md)).
user direction 2026-10-11 ~02:00 PDT: "continue with open ended discovery and development until 1100".
read as public/open-source captures plus local development. **not** included: the c4 second live
RRA cycle, the SUN-6000 recording choice and the superseded-file cleanup (each still needs the
user's explicit decision). coordinator cutoff: no new agent launches after ~09:30; final suite and
report by 11:00.

## ownership

| owner | finite assignment | exclusive writable paths |
| --- | --- | --- |
| `onie_machines` | ONIE `machine/<vendor>/` definitions for target/related vendors at one pinned commit | `data/raw/discovery-2026-10-11-onie/`, `research/batches/discovery-2026-10-11/onie/` |
| `openbmc_entity_manager` | OpenBMC entity-manager `configurations/` for target/related vendors: FRU probe strings, exposed PSU/fan entries | `data/raw/discovery-2026-10-11-entity-manager/`, `research/batches/discovery-2026-10-11/entity-manager/` |
| `bmc_platforms` | OpenBMC `meta-*` machine confs for target vendors + Linux aspeed/nuvoton BMC device-tree model/compatible strings | `data/raw/discovery-2026-10-11-openbmc/`, `data/raw/discovery-2026-10-11-linux-dts/`, `research/batches/discovery-2026-10-11/bmc/` |
| `bundle_sha` | c3b open note: cross-check the candidate bundle sha in `build_queue` | `devicehunt/review_queue.py`, `tests/test_review_queue.py` (new tests), `research/batches/discovery-2026-10-11/bundle-sha/` |
| `fb_openbmc` (wave 2) | facebook/openbmc `meta-facebook` platform layers: machine/board/FRU/PSU strings | `data/raw/discovery-2026-10-11-fb-openbmc/`, `research/batches/discovery-2026-10-11/fb-openbmc/` |
| `sonic_pddf` (wave 2) | SONiC PDDF device JSON for the same 7 vendor dirs at the sonic batch commit `c431b65b…`, attached to the existing `SONiC platform` identities | `data/raw/discovery-2026-10-11-sonic-pddf/`, `research/batches/discovery-2026-10-11/sonic-pddf/` |
| `spec_results` (wave 3) | SPEC published results (SPECpower first) for target vendors — web, ≤250 requests | `data/raw/discovery-2026-10-11-spec/`, `research/batches/discovery-2026-10-11/spec/` |
| `ubuntu_certified` (wave 3) | Ubuntu hardware certification listings for target vendors — web, ≤200 requests | `data/raw/discovery-2026-10-11-ubuntu-cert/`, `research/batches/discovery-2026-10-11/ubuntu-cert/` |
| `netbox_devicetypes` (wave 4) | netbox-community/devicetype-library YAML for target vendors (community tier) | `data/raw/discovery-2026-10-11-netbox/`, `research/batches/discovery-2026-10-11/netbox/` |
| `redhat_certified` (wave 4) | Red Hat certified-hardware listings for target vendors — web, ≤200 requests | `data/raw/discovery-2026-10-11-redhat-cert/`, `research/batches/discovery-2026-10-11/redhat-cert/` |
| `lookup_match_context` (eng) | `lookup` catalog rows gain `matched_in` (where the query matched: identity / fact names) | `devicehunt/lookup.py`, `devicehunt/__main__.py` (lookup columns; disclosed), `tests/test_lookup_match_context.py`, `research/batches/discovery-2026-10-11/lookup-context/` |
| `redhat_certified` wave 5 | remaining Red Hat detail pages as a separate supplement candidate (≤170 requests) | `data/raw/discovery-2026-10-11-redhat-cert/detail-wave5/`, supplement candidate, `handoff-wave5.md` |
| `wifi_alliance` (wave 5) | Wi-Fi Alliance certifications for Accton / Edge-Core / Ignitenet (≤150 requests) | `data/raw/discovery-2026-10-11-wifi-alliance/`, `research/batches/discovery-2026-10-11/wifi-alliance/` |
| `model_collisions` (eng, wave 5) | read-only `model-collisions` review-lead report (same model under different brand strings) | `devicehunt/model_collisions.py`, CLI wiring in `devicehunt/__main__.py`, `tests/test_model_collisions.py`, README subsection, `research/batches/discovery-2026-10-11/model-collisions/` |
| `linuxhw_dmi` (wave 5) | linuxhw/DMI crowd dmidecode dumps for target vendors (community tier; Server dirs) | `data/raw/discovery-2026-10-11-linuxhw-dmi/`, `research/batches/discovery-2026-10-11/linuxhw-dmi/` |
| `kernel_psus` (wave 6) | Linux pmbus drivers + hwmon docs mentioning PSU part strings already in the catalog | `data/raw/discovery-2026-10-11-kernel-psus/`, `research/batches/discovery-2026-10-11/kernel-psus/` |
| `firmware_boards` (wave 6) | coreboot / U-Boot / EDK2 board definitions for target vendors | `data/raw/discovery-2026-10-11-firmware-boards/`, `research/batches/discovery-2026-10-11/firmware-boards/` |
| `brand_family_proposal` (wave 7) | decision proposal for the multi-brand-string problem (no code) | `research/brand-family-proposal-2026-10-11.md` |
| `energystar_diff` (wave 7) | lead-only diff of public ENERGY STAR datasets vs pinned snapshots (≤20 requests) | `data/raw/discovery-2026-10-11-energystar-check/`, `research/batches/discovery-2026-10-11/energystar-check/` |
| `ieit_source` (wave 7) | lead-only primary-source search for IEIT SYSTEMS ↔ Inspur Electronic Information (30 requests: a 25-cap round plus a ≤20-cap follow-up after review) | `data/raw/discovery-2026-10-11-ieit-source/`, `research/batches/discovery-2026-10-11/ieit-source/` |
| wave 8 | SPEC Quanta CPU 2017 supplement (≤70), Red Hat Inspur Power detail supplement (≤25), Stratum/DENT platforms (`nos_platforms_2`) | `…-spec/` wave-8 subdir, `…-redhat-cert/detail-wave8/`, `data/raw/discovery-2026-10-11-nos2/`, `research/batches/discovery-2026-10-11/nos2/` |
| `/root-claude` | briefs, integration, docs | manifest/catalog/exports at integration, PROGRESS/README/research notes |
| reviewers | independent review of each handoff | `research/batches/discovery-2026-10-11/review/` |

## results — wave 1–2 (promoted together; see PROGRESS for the catalog sha)

All five research inputs (six curated files) and the engineering change passed independent review (reviews in
[review/](batches/discovery-2026-10-11/review/); every input ACCEPT with notes, fixes applied, entity-manager
ACCEPT conditional on two must-fixes → fixed → re-review ACCEPT). None asserts a company role: every new identity is a
software-reference identity under its own brand, `affiliations: []`, and directory/layer/vendor tokens are
labels. PSU/FRU part strings in these sources are **discovery leads, not fitment evidence**: a platform
listing `PFE1100-12-054NA` or `TDPS-1500AB-6 E` says the firmware/plugin knows that string, not that a
given unit ships with it.

| input | records | what it adds | review |
| --- | --- | --- | --- |
| [ONIE machines](batches/discovery-2026-10-11/onie/handoff.md) | 61 | ONIE `machine.make` / INSTALL facts at `ce480be7…` for the 61 alphanetworks/quanta/inventec/ingrasys/wiwynn machines not in the existing `onie` source (which already holds 111 accton/celestica machines at the same commit; their extra INSTALL headings/console speeds are a lead in the handoff) | [onie](batches/discovery-2026-10-11/review/onie-review.md) |
| [OpenBMC entity-manager](batches/discovery-2026-10-11/entity-manager/handoff.md) | 183 | `configurations/` at `386a9933…`: FRU `Probe` strings (BOARD/PRODUCT name and manufacturer as the firmware matches them), exposed PSU/fan/hot-swap entries, Asset fields labelled reference/literal/placeholder — Quanta, Wiwynn, Inventec, Ingrasys, Celestica strings plus foxconn-industrial-internet and ieisystem dirs (directory label only) | [entity-manager](batches/discovery-2026-10-11/review/entity-manager-review.md) |
| [OpenBMC machines + Linux BMC DTS](batches/discovery-2026-10-11/bmc/handoff.md) | 14 + 12 | `meta-quanta/inventec/ingrasys/wistron/bytedance/fii/ieisystem` machine confs at `702e38b…`; aspeed/nuvoton device-tree `model`/`compatible` strings at torvalds/linux `a5ebb76…`; machine↔dts links derived by the stated expansion rule (8), not literal references | [bmc](batches/discovery-2026-10-11/review/bmc-review.md) |
| [SONiC PDDF](batches/discovery-2026-10-11/sonic-pddf/handoff.md) | 11 (attached to existing `SONiC platform` identities) | PDDF/pd-plugin declarations at `c431b65b…`: PSU/fan/thermal counts and names, airflow maps, plugin PSU-model lookup keys; conflicts (34 vs 32 ports etc.) kept with both values | [sonic-pddf](batches/discovery-2026-10-11/review/sonic-pddf-review.md) |
| [Meta OpenBMC](batches/discovery-2026-10-11/fb-openbmc/handoff.md) | 19 | facebook/openbmc at `94594682…`: machine/board/FRU/PSU strings for wedge/wedge100/wedge400/minipack/fuji/elbert/cmm/yosemite/fby2/bletchley/catalina etc.; layer-level evidence attached once to the conf it names; Catalina spec text "ODM partners are Quanta and Ingrasys." kept as printed text (lead for a reviewed company-evidence decision, no role) | [fb-openbmc](batches/discovery-2026-10-11/review/fb-openbmc-review.md) |

Engineering: [bundle-sha](batches/discovery-2026-10-11/bundle-sha/handoff.md) (review ACCEPT with notes →
fixes → re-review ACCEPT) closes the c3b note "bundle sha not cross-checked": `build_queue` validates the
coverage manifest, binds each item's campaign-bundle receipt (kind, unit, revision, outcome, sha) to the
archived bytes and re-derives the bundle from the pinned inputs; new `queue --archive`.

Owned-model check across every new source: no hit for CS8200, CS8210, AZ4444, 7760/AS7760, DMC-12, AS7772,
the unsuffixed Ingrasys SV models or B4020/B4010. `meta-bytedance` (machine `g220a`, dts
`aspeed-bmc-bytedance-g220a`) is a ByteDance-named BMC platform: a label only, unrelated to the B4020/B4010
switch examples. Catalog leads (exact text, not merged): `NF5280M7` (entity-manager ieisystem probe and
OpenBMC machine; existing Inspur device), `Minipack`, `Wedge400`, `Catalina` (name coincidence with an
Amazon Data Services device), `TDPS-1500AB-*` (existing Celestica filings).

Integration ([integrate.py](batches/discovery-2026-10-11/integration/integrate.py)): +286 sources / +289 devices /
+300 records / +4,946 facts, 0 live rows lost; clean rebuild equals the candidate in all 52 tables;
[candidate review](batches/discovery-2026-10-11/review/candidate-review.md) ACCEPT with notes.

## results — waves 3–4 (promoted together as candidate b)

Convention adopted for listings (from the ubuntu-cert review): brand = canonical target only when the printed
company is a reviewed `TARGETS` alias, else as printed; one affiliation `[company as printed, role]` with an
existing **non-manufacturer** role (`listing_holder` for certification/community listings; `brand` when a SPEC
result's Test Sponsor equals its Hardware Vendor, else `reported_system_vendor`); `IEIT SYSTEMS` is never mapped
to Inspur. Manufacturer and factory scope are byte-identical before and after. The same hardware can now
appear under several brand strings (Quanta / QCT / Quanta Cloud Technology; Inspur / Inspur Corporation / IEIT);
nothing is merged across brand strings — `model-collisions` lists them as review leads.

| input | records | what it adds | review |
| --- | --- | --- | --- |
| [Ubuntu certified](batches/discovery-2026-10-11/ubuntu-cert/handoff.md) | 89 | ubuntu.com/certified listings + certificate pages (192 requests; 16 server errors recorded once): Quanta, Inspur, IEIT, Wiwynn, Inventec servers — releases, kernels, BIOS, CPU; test-bench peripherals labelled `hw_testbench_*` | [ubuntu-cert](batches/discovery-2026-10-11/review/ubuntu-cert-review.md) |
| [NetBox device types](batches/discovery-2026-10-11/netbox/handoff.md) | 33 | community devicetype-library (`community` tier): 19 Edgecore switches, 11 PSU/fan modules (brand `Unattributed accessory`), 2 Inspur, 1 QCT — part numbers, U height, airflow, weight, power-port draw (library entries, not measurements), interface counts | [netbox](batches/discovery-2026-10-11/review/netbox-review.md) |
| [SPEC results](batches/discovery-2026-10-11/spec/handoff.md) | 237 | SPECpower_ssj2008 (57: PSU quantity/rating/model, form factor, nodes) and SPEC CPU 2017 (180) text results for Quanta, Inspur, IEIT, Oracle/Sun, FOXCONN (FII); 244 requests at robots' 10 s delay; model = printed system name minus a trailing CPU/config parenthetical (full name kept as `spec_system_name`); no E4-2C/E5-2C/X7-2C/X10-2C results exist | [spec](batches/discovery-2026-10-11/review/spec-review.md) |
| [Red Hat certified](batches/discovery-2026-10-11/redhat-cert/handoff.md) | 342 | catalog.redhat.com server certifications (195 requests): Quanta, Inspur, IEIT, Inspur Power Commercial Systems, Oracle, Wiwynn, Celestica, Inventec; per-vendor model rule (Oracle full printed name; others the model field), full product name kept as identifier; 123 cloud-instance listings kept as a NOT-FOR-IMPORT lead | [redhat-cert](batches/discovery-2026-10-11/review/redhat-cert-review.md) |

Engineering: [lookup `matched_in`](batches/discovery-2026-10-11/lookup-context/handoff.md) (review ACCEPT with notes;
coordinator applied the scope-sentence fixes, not independently re-reviewed) and
[`model-collisions`](batches/discovery-2026-10-11/model-collisions/handoff.md) (review ACCEPT with notes → fixes →
re-review ACCEPT): a read-only review-lead report; 351 groups on the wave 3–4 catalog (362 after wave 10).

Dropped: Wi-Fi Alliance product finder — its only data API (`prf.cert.wi-fi.org/products/view/`) is disallowed by
robots.txt; 5 requests, no product data captured (only robots files, the finder page and its app JS) ([handoff](batches/discovery-2026-10-11/wifi-alliance/handoff.md)).

Integration: candidate b +365 devices / +701 records / +701 sources / +11,853 facts, 0 live rows lost; clean rebuild
equals the candidate in all 52 tables; [candidate-b review](batches/discovery-2026-10-11/review/candidate-b-review.md)
ACCEPT with notes; owned baseline 10/10 and ByteDance 0/2 unchanged.

## results — waves 5–6 (promoted together as candidate c)

| input | records | what it adds | review |
| --- | --- | --- | --- |
| [Red Hat detail supplement](batches/discovery-2026-10-11/redhat-cert/handoff-wave5.md) | 158 (0 new devices) | detail pages for the 161 listing-only Quanta/Inspur/IEIT certifications (161 requests); detail-only facts attached to the promoted identities | [wave5](batches/discovery-2026-10-11/review/redhat-cert-wave5-review.md) |
| [linuxhw DMI](batches/discovery-2026-10-11/linuxhw-dmi/handoff.md) | 29 (19 new devices) | crowd dmidecode dumps (`community`, CC-BY-4.0) for Server dirs of Inspur, Quanta/QCT, Oracle/Sun, Hyve, TD SYNNEX, Foxconn: System/Base Board/Chassis/BIOS strings as firmware reported them; serials/UUIDs/asset tags masked upstream, 5 MAC-bearing dumps dropped; case-only Oracle merges follow the catalog's casefold id rule; board-only fallback and desktop/notebook dirs kept as NOT-FOR-IMPORT leads | [dmi](batches/discovery-2026-10-11/review/linuxhw-dmi-review.md) (should-fixes coordinator-checked, not re-reviewed) |
| [firmware boards](batches/discovery-2026-10-11/firmware-boards/handoff.md) | 8 | coreboot mainboards `ocp/tiogapass`, `ocp/deltalake`, `bytedance/bd_egs`, `inventec/transformers`, `facebook/fbg1701`, `facebook/monolith`, `foxconn/d41s`, `foxconn/g41s-k` (U-Boot: no target board dirs; EDK2: none) | [firmware](batches/discovery-2026-10-11/review/firmware-boards-review.md) |

Lead-only (not imported): [Linux hwmon PSU drivers](batches/discovery-2026-10-11/kernel-psus/handoff.md) (4 thin
driver identities; no catalog PSU model attaches; its 106-string catalog PSU list is a useful lead);
[ENERGY STAR diff](batches/discovery-2026-10-11/energystar-check/handoff.md) (no new or changed target-vendor rows
since the pinned snapshots; no refresh needed). `bytedance/bd_egs` (coreboot, `Vendor name: ByteDance`) joins
`meta-bytedance`/`g220a` as ByteDance-named server platforms — labels only, unrelated to the B4020/B4010 switches.
[Candidate-c review](batches/discovery-2026-10-11/review/candidate-c-review.md) ACCEPT with notes: `matched_in`
counts DMI facts whose citation path contains the model name (reads like extra corroboration); a duplicated
coverage sentence on two DMI sources.

## wave 7 — decisions prepared for the user

- [brand-family proposal](brand-family-proposal-2026-10-11.md): 351 collision groups (wave 3–4 catalog); recommends a reviewed,
  display-only family table (Quanta/QCT, Oracle/Sun first) plus own-brand model-prefix aliases, no import-time
  canonicalisation; 7 decisions listed.
- [IEIT primary source](batches/discovery-2026-10-11/ieit-source/handoff.md): the listed company's own filings
  (2024 annual report; Articles of Association) give `IEIT SYSTEMS Co., Ltd.` as the English name of
  浪潮电子信息产业股份有限公司 (000977) — first-party (CNINFO / SZSE: FY2023 and FY2024 annual reports, Articles of
  Association). The earlier English name `Inspur Electronic Information Industry Co.,Ltd` (same Chinese name) and the
  2023-08 amendment changing it rest on mirror copies (Xueqiu, Sina) — provisional; effective date unestablished
  (April 2023 – April 2024). Covers that legal entity only — not `Inspur Corporation`, Inspur Group or other strings.
  [review](batches/discovery-2026-10-11/review/ieit-source-review.md) ACCEPT with notes; round-2 fixes coordinator-checked.

## results — wave 8 (promoted as candidate d)

| input | records | what it adds | review |
| --- | --- | --- | --- |
| [Red Hat Inspur Power supplement](batches/discovery-2026-10-11/redhat-cert/handoff-wave8.md) | 21 (0 new devices) | detail pages for the 21 Inspur Power Commercial Systems certifications (21 requests; Red Hat total 377) | [wave8](batches/discovery-2026-10-11/review/redhat-cert-wave8-review.md) |
| [Stratum / DENT platforms](batches/discovery-2026-10-11/nos2/handoff.md) | 25 | Stratum `hal/config` chassis/phal configs (Accton 7, Inventec 7, Quanta 1: ports, speeds, counted fan/PSU/LED/thermal entries; placeholder names labelled) and the 10 DENT Accton ARM/PowerPC platforms not already in ONL (46 ONL-identical DENT ids not imported) | [nos2](batches/discovery-2026-10-11/review/nos2-review.md) |
| [SPEC Quanta supplement](batches/discovery-2026-10-11/spec/handoff-wave8.md) | 59 (0 new devices) | the remaining 59 `Quanta Cloud Technology` CPU 2017 results (60 requests; SPEC requests so far 304) | [spec-wave8](batches/discovery-2026-10-11/review/spec-wave8-review.md) |

Engineering: [lookup citation labelling](batches/discovery-2026-10-11/lookup-citations/handoff.md) (review ACCEPT
with notes → fixes → re-review ACCEPT): `matched_in` counts a fact name only when the query is in its value
proper; names hit only inside a citation locator are reported as `cited-path only: N fact names`
(`matched_in_detail.facts_citation_only`). Rows returned by lookup are unchanged.
[Candidate-d review](batches/discovery-2026-10-11/review/candidate-d-review.md) ACCEPT with notes.

## results — wave 9 (promoted as candidate e)

[SPEC Inspur/IEIT supplement](batches/discovery-2026-10-11/spec/handoff-wave9.md): 150 CPU 2017 results (150 requests;
SPEC requests so far 454) — all 64 remaining IEIT names and the 86 newest Inspur names (`Inspur Corporation`, `… (IEI)`),
brands as printed, never mapped; 17 new devices, 13 merged; 244 older Inspur names remain unfetched.
[review](batches/discovery-2026-10-11/review/spec-wave9-review.md) ACCEPT;
[candidate-e review](batches/discovery-2026-10-11/review/candidate-e-review.md) ACCEPT with notes. Target inventory
unchanged (non-alias brand strings).

## results — wave 10 (promoted as candidate f; last data wave)

[SPECjbb2015 supplement](batches/discovery-2026-10-11/spec/handoff-wave10.md): 149 full-disclosure HTML results
(150 requests; SPEC requests so far 604) for Oracle (7), Quanta (21), IEIT (40) and Inspur (81) — hardware hw_1
blocks parsed with the stdlib, `spec_jbb_mode` per result, jOPS printed with their mode label (benchmark
throughput, not hardware specs); 23 new devices, 37 merged, all 7 Oracle results onto existing Oracle identities.
The 244 older Inspur CPU 2017 results and 6 old Inspur jbb pages stay unfetched (lead).
[review](batches/discovery-2026-10-11/review/spec-wave10-review.md) ACCEPT with notes (S1/S2 applied, coordinator-
checked and verified by the [candidate-f review](batches/discovery-2026-10-11/review/candidate-f-review.md), ACCEPT with notes).

## batch totals

Promoted in six reviewed rounds (candidates a–f), each with a clean rebuild equal to the candidate in all 52
tables, a guarded full suite before promotion (653 → 671 OK, 0 blocked; final code runs 679, 694, then 702 OK) and the accepted-manifest gate tests after:
catalog `0646f4f5…` → `e7cc01a1…`; identities 4,361 → 5,107; records 5,625 → 7,225; sources 490 → 2,076; facts
46,904 → 74,074; manifest 127 → 145 jobs. Manufacturer and factory scope unchanged throughout; owned baseline 10/10;
ByteDance B4020/B4010 0/2. No owned-model hit (CS8200, CS8210, AZ4444, 7760/AS7760, DMC-12, AS7772) in any new source.

## engineering after wave 10

[`specs --include-source-facts`](batches/discovery-2026-10-11/practical-facts/handoff.md) (review ACCEPT with notes →
fixes → re-review ACCEPT): opt-in; adds source-recorded practical facts whose names fully match an explicit list of 8 reviewed name patterns (56 catalog fact names after review)
(SPEC PSU/form factor/nodes, NetBox U height/airflow/weight/power draw, DMI chassis type, ENERGY STAR idle power
labelled with its test configuration, …) mapped to the four specs categories, with `origin` and `source_fact_name`
columns; software declarations, benchmark scores and certification placeholders excluded; default output unchanged.
`buying-report` unchanged.

## lead-only evidence for the brand-family decisions (after wave 10)

[family-evidence](batches/discovery-2026-10-11/family-evidence/handoff.md) (13 requests;
[review](batches/discovery-2026-10-11/review/family-evidence-review.md) ACCEPT with notes): Quanta Computer's
affiliated-companies list (as of 2025-12-31) holds the Taiwan `Quanta Cloud Technology Inc.` 100%, and QCT's undated
About pages name Quanta Computer as parent — enough for a display-only Quanta/QCT family row. 000977's FY2024 report
lists 浪潮（北京）电子信息产业有限公司 as a separate 100% subsidiary; a 2025 Federal Register rule lists `Inspur Information`
and `IEIT Systems Co. Ltd.` as aliases of the listed company (government corroboration). No primary source names
`Inspur Corporation` or `(IEI)`. Nothing imported or mapped.

## buyer-workflow audit and fixes (after wave 10)

[Buyer audit](batches/discovery-2026-10-11/review/buyer-audit.md) (read-only, ~45 commands, before/after the batch): mixed,
leaning net helpful — no crashes, every command < 1.7 s, owned baseline unchanged; new matches (e.g. QuantaGrid D52B-1U)
and new leads via `matched_in` and `specs --include-source-facts`. It found one regression introduced by the batch and
several presentation problems; fixed the same morning:

- [listing-softref](batches/discovery-2026-10-11/listing-softref/handoff.md) (review ACCEPT with notes → brand-set test
  added): `match-listing` excludes software-reference identities by default (bare-word models like `wedge`, `fuji`,
  `yosemite` no longer match ordinary titles); `--include-software-references` re-adds them as
  `software_reference_mention`, sorted after every hardware match type. Pre-batch titles give identical hardware rows.
- [specs-dedupe](batches/discovery-2026-10-11/specs-dedupe/handoff.md) (review ACCEPT with notes → tests added):
  `--include-source-facts` rows are self-describing (`PSU quantity and rating (W): 1 x 650`), the NetBox draw is
  labelled as a library entry (not PSU capacity or measured draw), DMI chassis rows drop the dump path from the text.
  Same-source dedupe removes nothing on live data: the audit's 415 repeats are identical text from different sources,
  kept apart for attribution.

- [buying-report hint](batches/discovery-2026-10-11/buying-report-hint/handoff.md) (review REJECT → `see` scope fix →
  re-review ACCEPT): a `not_collected` power / cooling / chassis category now carries `source_facts_available`
  (count, fact names, and a scope-faithful `specs … --include-source-facts` command); statuses unchanged.

Still open from the audit (recommendations, not done): brand-split identities in `match-listing` and
`vendor-inventory` (needs the brand-family decision), `YM-2651Y` title vs `YM-2651Y B01R` / `YM-2651Y, B01R` identities, `shopping` queries built from legal
brand strings.

[Re-audit](batches/discovery-2026-10-11/review/buyer-audit.md) (read-only, after the fixes): the `match-listing`
regression (findings 1–4), the buying-report pointer (8) and the source-fact labels (9, 11, 12, 20) are resolved; no
new regression, no crash, every command < 0.4 s; owned models still hit with hardware rows only and
`validate-inventory` stays 10/10. Minor, open: `buying-report X7-2C` shows no source-fact pointer: the report matches only the records of
`Oracle Server X7-2C` whose own text carries `X7-2C` (e.g. `oracle-oss`), and the `SM 1U` / `SM 2U` source facts sit on
other records of that identity — the pointer deliberately uses the report's matched records (coordinator-checked), and
`specs YM-2651Y --exact-model` misses the PSU identities, which are printed `YM-2651Y B01R` / `YM-2651Y, B01R`.

## user decisions 2026-10-11 (after the batch)

The user answered four parked decisions: cleanup "Delete reducible", brand family "Display-only table", c4 "Authorize
cycle 2", SUN-6000 "A: scoped company evidence". Outcomes:

- cleanup: 23 files / 8.83 GB deleted after hashing ([ledger](batches/discovery-2026-10-11/integration/cleanup-2026-10-11.json)).
- c4 cycle 2 (`2026-10-11a`): 1,063 unit requests + smoke, all 200; all seven units `matches_pinned`
  ([c4 notes](batches/hunt-2026-10-10/c4/c4-notes.md)).
- brand family: [table handoff](batches/discovery-2026-10-11/brand-family-table/handoff.md) — `data/brand-families.json`,
  7 rows signed by the independent reviewer ([review](batches/discovery-2026-10-11/review/brand-family-table-review.md));
  display-only (`brand_family` in lookup / match-listing / model-collisions; ranking and vendor-inventory byte-identical).
- SUN-6000: [design](batches/discovery-2026-10-11/sun6000-a/design.md) →
  [design review](batches/discovery-2026-10-11/review/sun6000-a-design-review.md) (Kladno-only) →
  [handoff](batches/discovery-2026-10-11/sun6000-a/handoff.md) → [code review + decisions](batches/discovery-2026-10-11/review/sun6000-a-review.md)
  → [candidate review](batches/discovery-2026-10-11/review/sun6000-candidate-review.md); promoted (catalog `63d3ac78…`):
  +5 Celestica-brand identities, Celestica manufacturer scope 193 → 198.
