# final report draft — 2026-10-11 discovery session (coordinator working file)

## what changed
- catalog `0646f4f5…` → `e7cc01a1…`; identities 4,361 → 5,107; records 5,625 → 7,225; sources 490 → 2,076;
  facts 46,904 → 74,074; manifest 127 → 145 jobs. six reviewed promotion rounds (candidates a–f), each: clean rebuild
  equals candidate (52 tables), guarded suite before (653 → 671 OK, 0 blocked), gate tests after, exports refreshed.
- new lookups possible: FRU probe strings (OpenBMC entity-manager, e.g. NF5280M7 baseboard probe), DMI product /
  baseboard names (linuxhw), PSU part strings in firmware (PFE1100-12-054NA, YM-2651Y, TDPS-1500AB-6 E), certified
  configs (Ubuntu / Red Hat: Quanta, Inspur, IEIT, Wiwynn, Inventec, Celestica, Oracle, Inspur Power), SPEC power /
  CPU / jbb results (PSU quantity/rating/model, e.g. shared ARTESYN MC2200B4 on T42S-2U), NetBox rack units /
  airflow / power draw for Edgecore switches, coreboot / Stratum / DENT / ONIE / BMC platform definitions.
- engineering: S4 reconcile (ops batch, yesterday), queue baseline job, bundle-sha cross-check, lookup `matched_in`
  (+ cited-path-only labelling), `model-collisions` report.

## what did not move
- manufacturer and factory scope byte-identical across all ten waves (new associations are `listing_holder`, `brand`,
  `reported_system_vendor`). owned baseline 10/10. ByteDance B4020/B4010 0/2. no owned-model hit (CS8200, CS8210,
  AZ4444, 7760/AS7760, DMC-12, AS7772) in any new source. `g220a` / `bd_egs` are ByteDance-named server platforms only.

## limits for a buyer
- same hardware under several brand strings by design (NF5280M7 = three identities; model-collisions 362 groups).
- part / PSU / FRU strings found only in facts = leads, not fitment. software-reference identities describe files.
- `specs` defaults to the four curated statement categories; `specs --include-source-facts` (new, reviewed) adds
  whitelisted source facts (SPEC PSU/form factor, NetBox U height/airflow/power draw, DMI chassis type, ENERGY STAR
  idle power) with `origin`. `buying-report` unchanged. final guarded suite 702 OK (after buyer-audit fixes: match-listing hides software refs by default; labelled source facts; buying-report source-fact pointer). exports refreshed 09:17.

## decisions
- parked from yesterday: SUN-6000 A/B/C; c4 second live cycle; cleanup (data/ now 31 files ~11.9 GB; ~8.8 GB
  reducible: discovery candidates a–f 2.3 GB, proof rebuilds a–e 2.0 GB, before-backups a–e 1.9 GB, hunt-era 2.6 GB;
  before-backup f == candidate e byte-for-byte).
- new: brand-family proposal (7 decisions; recommend display-only family table, Quanta/QCT + Oracle/Sun first);
  QCT ↔ Quanta now has first-party evidence (Quanta affiliates list 2025-12-31); IEIT ↔ Inspur: first-party
  current-name evidence, rename leg mirror-only, BIS rule corroborates aliases; `Inspur Corporation`/`(IEI)` unsourced.

## disclosures
- coordinator-checked, not independently re-reviewed: DMI should-fixes (later confirmed by candidate-c review), SPEC
  wave-10 S1/S2 (confirmed by candidate-f review), lookup scope sentence, docs follow-ups, S4 notes 1–2.
- S4 driver `reconcile` phase never exercised live. Wi-Fi Alliance dropped (API robots-disallowed). unfetched:
  244 older Inspur SPEC CPU 2017, 6 old jbb pages, Ubuntu 5xx pages (not retried), Red Hat cloud listings (not for import).

## where
PROGRESS item 7; research/discovery-2026-10-11.md; research/brand-family-proposal-2026-10-11.md.
