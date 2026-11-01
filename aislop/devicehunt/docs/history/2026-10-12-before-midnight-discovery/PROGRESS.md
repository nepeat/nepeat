# current handoff

updated 2026-10-11. objective: [GOAL](GOAL.md); rules: [AGENTS](AGENTS.md),
[workflow](docs/agent-workflow.md), [evidence policy](docs/evidence-policy.md).
coordinator/integrator/documentation editor this session: `/root-claude`
(Claude Code, Opus 5.5). independent reviewers: Claude Sonnet 5.5 workers.
this session's milestones: [rename note](research/rename-devicehunt-2026-10-10.md),
[c3b note](research/campaign-c3b-2026-10-10.md), the [parallel hunt + first live RRA refresh](research/hunt-2026-10-10.md) the [ops batch](research/ops-2026-10-10.md) and the 2026-10-11 [open-source discovery batch](research/discovery-2026-10-11.md) (earlier: [c2 note](research/campaign-c2-2026-10-05.md)); Astra's previous
handoff is [archived](docs/history/2026-10-05-before-c2-and-smartnic/PROGRESS.md).

## host

the project is **devicehunt**: renamed on 2026-10-10 (user request) from
`/home/erin/nepeat/aislop/fcc_shenanigans` to `/home/erin/nepeat/aislop/devicehunt`.
use the project's direnv `python3` (3.14.7 verified); no packages were installed.
the operational capture authority in `data/archive/` is bound to this absolute path.
an unexplained move makes every campaign operation refuse to run; a whole-workspace
move goes through `scripts/relocate_archive.py` then `scripts/relocate_catalog.py`
(README; [rename note](research/rename-devicehunt-2026-10-10.md)).
this directory is untracked in the parent git repository, so recovery relies on the
listed `data/*-before-*` files and backups rather than git history. older recovery
catalogs still carry `fcc_shenanigans` cache paths; re-run `relocate_catalog.py` after
restoring one.
library callers of `Archive` must pass the project root as `workspace`;
in-place registration now refuses the old `root.parent` guess.

## accepted catalog

| item | verified state |
| --- | --- |
| authority | `data/devices.sqlite`, schema 29, sha `63d3ac78…` (SUN-6000 option A promoted 2026-10-11; before it `e7cc01a1…` discovery wave 10, `d78b0b7e…` wave 9, `09891954…` wave 8, `baa016d2…` waves 5–6, `ab8ed2e3…` waves 3–4, `1cfd84ca…` waves 1–2, `0646f4f5…` hunt batch) |
| identities / records / sources | 5,112 / 7,230 / 2,076 (SUN-6000: +5 / +5 Celestica-brand records on the RRA Celestica source; [discovery batch](research/discovery-2026-10-11.md): waves 1–2 +289 / +300 / +286 software-reference identities; waves 3–4 +365 / +701 / +701 from Ubuntu/Red Hat certification listings, SPEC results and NetBox device types; waves 5–6 +27 / +195 / +195 from the Red Hat detail supplement, linuxhw DMI server dumps and coreboot boards; wave 8 +25 / +105 / +105 from Stratum/DENT platforms and the Red Hat Inspur Power and SPEC Quanta supplements; wave 9 +17 / +150 / +150 from SPEC Inspur/IEIT results; wave 10 +23 / +149 / +149 from SPECjbb2015 results) |
| facts | 74,129 (SUN-6000 +55; +4,946 waves 1–2, +11,853 waves 3–4, +2,227 waves 5–6, +1,573 wave 8, +2,548 wave 9, +4,023 wave 10) |
| catalog PDF archive | 61 PDFs / 3,716 pages |
| accepted lineage release | 146 artifacts / 374 receipts / four reviewed statements; unchanged |
| owned baseline | 10/10: nine documents plus the reviewed 7760 label |
| ByteDance B4020/B4010 expectations | 0/2 qualifying affiliation hits; unresolved |
| manufacturer inventory | 2,147 target/identity pairs / 2,146 identities (SUN-6000: Celestica 193 → 198 via Kladno `registry_manufacturer`; otherwise unchanged through the discovery batch) |
| all target associations | 4,591 pairs / 4,448 identities (SUN-6000 +10 / +5; discovery +237 from `listing_holder` and `brand` listing affiliations; `reported_system_vendor` adds none; manufacturer scope unchanged) |
| hyperscaler assertions | 22 rows on 18 devices; unchanged |
| manifest / offline rebuild | 145 jobs / 3,910 hashed inputs, sha `14c2a1f1…`, declares `campaign_gate` (SUN-6000 release `campaign-release-1d45317e…`, coverage `coverage-b8ee5df9…`); gated clean rebuild equals the promoted catalog in all 52 tables/indexes + baseline (2026-10-11, [log](research/batches/discovery-2026-10-11/integration/compare-f.log)) |
| verification | guarded offline runner: 614 OK **before** promotion ([proof](research/batches/campaign-c2-2026-10-05/acceptance/hunt-pre-promotion-2-suite-result.json)); final run after the live-bundle fix 618 OK, 0 blocked network/subprocess calls ([proof](research/batches/campaign-c2-2026-10-05/acceptance/hunt-final-suite-result.json), [log](research/batches/hunt-2026-10-10/integration/final-suite.log)); integrity and foreign keys ok; after the ops batch (S4 reconcile, queue baseline job) 644 OK, 0 blocked ([proof](research/batches/campaign-c2-2026-10-05/acceptance/ops-2026-10-10-suite-result.json), [log](research/batches/ops-2026-10-10/integration/guarded-suite.log)); catalog and manifest unchanged; discovery batch 653 OK **before** promotion, 0 blocked ([proof](research/batches/campaign-c2-2026-10-05/acceptance/discovery-2026-10-11-pre-suite-result.json), [log](research/batches/discovery-2026-10-11/integration/pre-promotion-suite.log)), `AcceptedManifestGateTests` OK after ([log](research/batches/discovery-2026-10-11/integration/post-promotion-gate.log)); waves 3–4 + `lookup matched_in` 658 OK before promotion, 0 blocked ([log](research/batches/discovery-2026-10-11/integration/pre-promotion-suite-b.log)), gate + lookup tests OK after ([log](research/batches/discovery-2026-10-11/integration/post-promotion-gate-b.log)); waves 5–6 + `model-collisions` 669 OK before promotion, 0 blocked ([log](research/batches/discovery-2026-10-11/integration/pre-promotion-suite-c.log)), gate tests OK after ([log](research/batches/discovery-2026-10-11/integration/post-promotion-gate-c.log)); wave 8 669 OK before promotion, 0 blocked ([log](research/batches/discovery-2026-10-11/integration/pre-promotion-suite-d.log)), gate tests OK after ([log](research/batches/discovery-2026-10-11/integration/post-promotion-gate-d.log)); wave 9 + lookup citation labelling 671 OK before promotion, 0 blocked ([log](research/batches/discovery-2026-10-11/integration/pre-promotion-suite-e.log)), gate tests OK after ([log](research/batches/discovery-2026-10-11/integration/post-promotion-gate-e.log)); wave 10 671 OK before promotion, 0 blocked ([log](research/batches/discovery-2026-10-11/integration/pre-promotion-suite-f.log)), gate tests OK after ([log](research/batches/discovery-2026-10-11/integration/post-promotion-gate-f.log)); final run after `specs --include-source-facts` 679 OK, 0 blocked ([log](research/batches/discovery-2026-10-11/integration/final-suite.log)); after the buyer-audit fixes 694 OK, then 702 OK after the buying-report hint, 0 blocked ([log](research/batches/discovery-2026-10-11/integration/final-suite-3.log)); exports refreshed after; catalog and manifest unchanged |
| exports | standard outputs, lookup reports and collection reports refreshed after the discovery promotion (2026-10-11) |

**SmartNIC/QCT promoted** 2026-10-05 ([batch note](research/smartnic-qct-batch-2026-10-04.md),
[candidate review: ACCEPT with notes](research/batches/smartnic-qct-2026-10-04/integration/candidate-review.md),
[post-promotion proof](research/batches/smartnic-qct-2026-10-04/integration/post-promotion-proof.json)).
Four records: Inventec FPGA SmartNIC C5020X (Chinese, reference-only brief), the
separate printed FPGA IPU C5020X identity (its `TPM 75W` row stays literal and
unclassified) and two QCT QuantaPlex T42S-2U specification records. no C5000X,
QCT→Quanta or manufacture alias. Astra's frozen contract was rebased from the
pre-archive 120-job base to the current 121→122-job manifest; data inputs were
unchanged. Recovery bytes: `data/devices-before-smartnic-qct-2026-10-05.sqlite`
and `data/import-manifest-before-smartnic-qct-2026-10-05.json`.

| role | current path |
| --- | --- |
| accepted manifest | `data/import-manifest.json` (145 jobs, gated by the c3b release) |
| pre-hunt catalog / manifest (byte copies) | `data/devices-before-hunt-2026-10-10.sqlite` / `data/import-manifest-before-hunt-2026-10-10.json` |
| campaign review queue / selected release | `data/campaign-reviews/coverage-b8ee5df9…/` / `data/campaign-releases/campaign-release-1d45317e…json` (SUN-6000, 2026-10-11; 12 independent decisions; superseded: `coverage-0db21eac…` queue, c3b `coverage-d2bc5c40…` / `campaign-release-2a5b2f0c…`); campaign `DEFAULT_DEFINITION`/`DEFAULT_INVENTORY` → `research/batches/discovery-2026-10-11/sun6000-a/inventory-r2/` |
| pre-gate manifest (byte copy) | `data/import-manifest-before-c3b-gate-2026-10-10.json`; `data/import-manifest-candidate-c3b-2026-10-10.json` is the candidate at promotion, byte-identical to the promoted manifest |
| frozen lineage release | `data/archive-release-2026-10-04.json` |
| complete reference collection (50 entries; PDF inputs are siblings beside it) | `data/raw/reference-documents-with-smartnic-qct-2026-10-05.json` |
| latest verified rebuild | `data/rebuilt-sun6000-2026-10-11.sqlite` (gated; equals the promoted catalog; [compare](research/batches/discovery-2026-10-11/sun6000-a/integration/compare.log)); `data/devices-candidate-sun6000-2026-10-11.sqlite` + `data/import-manifest-candidate-sun6000-2026-10-11.json` are byte-identical to the promoted pair (reducible); `data/import-manifest-pinbase-sun6000-2026-10-11.json` is a gate-less pin base for the re-pin script only — superseded, never an accepted manifest; recovery: `data/devices-before-sun6000-2026-10-11.sqlite` / `data/import-manifest-before-sun6000-2026-10-11.json` plus the c3b `DEFAULT_*` |
| superseded files (cleanup done 2026-10-11, user: "Delete reducible") | 23 catalog-sized files (8.83 GB: discovery candidates a–f, proof rebuilds a–e, before-backups a–e, hunt-era candidates/rebuilds, c3b-gate, after-rename and smartnic rebuilds) were deleted after hashing; per-file sha256, size and reason in the [cleanup ledger](research/batches/discovery-2026-10-11/integration/cleanup-2026-10-11.json). every `import-manifest-*.json` was kept, so each removed state can be rebuilt. kept for recovery: `data/devices-before-discovery-2026-10-11-f.sqlite` / `data/import-manifest-before-discovery-2026-10-11-f.json` (the pair before the newest promotion) and `data/rebuilt-discovery-2026-10-11-f.sqlite` (latest verified rebuild) |
| pre-rename catalog (byte copy, old cache paths) | `data/devices-before-rename-2026-10-10.sqlite` |
| pre-rename operational authority backup | `data/archive-registry-backup-2026-10-10/` and `data/archive/capture-leases.before-relocate-*.sqlite` |
| pre-SmartNIC recovery catalog / manifest | `data/devices-before-smartnic-qct-2026-10-05.sqlite` / `data/import-manifest-before-smartnic-qct-2026-10-05.json` |
| superseded, pending history compression | `data/devices-before-archive-2026-10-04.sqlite`, `data/catalog-archive-rebuilt-2026-10-04.sqlite` |
| acquisition metadata / immutable outputs | `data/archive/` |
| campaign coverage manifests (claim-free) | `data/coverage/rra-ised-saved-2026-10-04/` |
| pinned campaign definition / inventory (current default) | `research/batches/campaign-c3b-2026-10-10/inventory/{campaign-definition,inventory}.json`; c0 pilot revision unchanged beside it |

## acquisition registry and offline campaign

`data/archive/` is upgraded: index v2 with head marker, operational schema 3,
`ready`, zero leases. **c4 (first live RRA refresh, 2026-10-10) allocated 11 finite accounts**
(10 activated with a recorded decision; the first, misconfigured with an absolute `deadline`, was refused
before activation); 1,066 reservations: 1,065 complete and one settled
`uncertain` timeout (charged, its account stopped; `settle_unobserved` decision recorded). four saved denials
were imported as durable restrictions; no new restriction arrived. the live archive holds 4,772 receipts
(head sequence 4398); only 374 belong to the accepted lineage release. legacy metadata backup: `data/archive-registry-backup-2026-10-05/`;
schema-2 backup: `data/archive/capture-leases.before-c2.sqlite`.
[registry upgrade proof](research/batches/campaign-c2-2026-10-05/acceptance/live-upgrade.json).

**c2 accepted** ([acceptance](research/batches/campaign-c2-2026-10-05/acceptance/c2-acceptance.json),
[review: ACCEPT with notes](research/batches/campaign-c2-2026-10-05/review/c2-runner-review.md)).
`python3 -m devicehunt.campaign plan|run --offline|status|resume|report` replays the
pinned 12 RRA/ISED units through in-place registration, the existing validators and
candidate bundles. jobs bind unit revision + stage + chained recipe (full import-closure
code hashes, Python version, upstream recipe, pinned expectations). results are
deterministic receipts published before checkpoints. a crash between them is
recovered on resume without redoing work, and repeated runs only reuse. the live
first run validated 12/12 units (two explicit zeros), all
matching c0 pins, with 4.78 MB measured growth against the 64 MiB cap; the second run
reused 36 jobs with zero new bytes ([proof](research/batches/campaign-c2-2026-10-05/acceptance/live-campaign.json)).
candidate bundles are not catalog evidence and nothing was imported.

limits: saved snapshots only; coverage is per declared query, not regulator-wide.
`pin_inventory.py verify` fails only on pinned accepted checkpoints: the c0 revision on the
catalog and manifest (moved with SmartNIC/QCT and the rename), the c3b revision
(`--output research/batches/campaign-c3b-2026-10-10/inventory`) on the manifest and catalog (moved by the gate
and hunt promotions; it pinned catalog `4d525902…`). pinned sources still verify. checkpoints are context, not campaign inputs.
open [review](research/batches/campaign-c2-2026-10-05/review/c2-runner-review.md)
notes: malformed manifest structure (missing key) aborts the run
rather than checkpointing `failed`. the byte cap is a conservative estimate, not
a hard bound, and its baseline counts any archive publication after the first
run. `run()` does not compare a new definition hash with earlier runs. the c3b run
(`run-61cc2607…`, new definition) re-executed all 36 jobs; a further run reuses them unless
code in the stage closures changes.

## next bounded work

tests: `python3 -m unittest discover -s tests` (about 12–20 minutes; count in the table above).
For offline acceptance, use the guarded runner
`research/batches/campaign-c2-2026-10-05/acceptance/run_offline_suite.py` with a fresh
`FCC_C1_ACCEPTANCE_LABEL`. Campaign-only: `python3 -m unittest tests.test_campaign tests.test_campaign_state`.

1. **c2b is done** ([note](research/campaign-c2b-2026-10-05.md); review ACCEPT with notes).
   `devicehunt/campaign_rra_live.py` runs the exact RRA request sequence through
   `Collector.capture`, with per-cycle observation slots and a capped detail allowlist
   derived from the validated before-CSV. It is tested only with mocked transport
   replaying real saved bytes. It is not wired into the CLI; c4 drives it through `live_rra.py` (see item 4).
   Its c4 preconditions (a form-GET smoke first, redirects/User-Agent, a quota
   pre-check, a fresh cycle id) are listed in the note.
2. **c3a is done** ([note](research/campaign-c3a-2026-10-05.md); review ACCEPT with notes).
   `python3 -m devicehunt.coverage freeze RUN_ID --output DIR` / `validate PATH` writes
   claim-free, content-addressed coverage manifests. The published manifest for
   `c2-live-run-1` is `data/coverage/rra-ised-saved-2026-10-04/coverage-fc6fe1ca…json`:
   12/12 units validated, 2 explicit zeros.
3. **c3b is done** ([note](research/campaign-c3b-2026-10-10.md); code review ACCEPT after fixes;
   live decisions by an independent reviewer: 12/12 accepted, no changes).
   `python3 -m devicehunt.review_queue queue|show|decide|release|check-release` (README).
   `data/import-manifest.json` now declares `campaign_gate`; a gated clean rebuild equals live and
   the catalog is unchanged. **Every future integration that touches the 12 campaign sources needs
   a new queue → independent decisions → release → manifest gate update**, and
   `tests.test_review_queue.AcceptedManifestGateTests` must pass. Open notes: the gate is opt-in in
   `rebuild` (enforced by that test), `decide` race fails closed; bundle sha cross-check closed 2026-10-11
   ([bundle-sha handoff](research/batches/discovery-2026-10-11/bundle-sha/handoff.md), re-review ACCEPT; `queue --archive`).
   2026-10-10 ops batch ([queue handoff](research/batches/ops-2026-10-10/queue/handoff.md), re-review
   ACCEPT): the queue takes each unit's baseline job from the gated accepted manifest's release
   (verified with `gate_inputs`), so a unit whose bundle moved keeps its old job unless accepted; new
   releases carry optional `candidate_job`, which the gate forbids importing for non-accepted entries;
   `check_release` rejects two entries sharing one job. The promoted release (no `candidate_job`) still
   validates; it cannot itself forbid a double import.
   Next integrator: run the full suite before promotion excluding `AcceptedManifestGateTests`,
   then that class after promotion (c3b ran the full suite only after promotion).
4. **c4 first refresh is done** ([c4 notes](research/batches/hunt-2026-10-10/c4/c4-notes.md)): all 7 RRA
   units refreshed live — 1,063 requests, 43.5 min, no throttle/denial — and all 7 `matches_pinned` under
   the location-insensitive `compare_live`: **no change in these 7 declared manufacturer queries since
   2026-09-26/27** (including no new Amazon Data Services certificates in that query; not a regulator-wide
   claim); nothing imported. The user authorized "give it a shot"; the coordinator extended the planned
   three phases to all seven units after the measured cost proved small (same per-phase caps). Driver:
   `research/batches/hunt-2026-10-10/c4/live_rra.py` (smoke / refresh / settle / rebundle; each phase its own
   immutable account with exact request/byte caps, 30 min window, 120 s timeout). Live bundles:
   `data/raw/rra-*-official-live-2026-10-10d[-carried]/`. Next: the plan's second eligible cycle (same caps;
   **needs the user's authorization for that new scope**) before any periodic invocation; a changed unit would go through rebundle-with-carry → new definition
   revision → c3b queue (the queue now carries the baseline job separately; item 3). After a reboot inside a
   phase's window, `live_rra.py reconcile` (item 5). ISED stays offline.
5. **c1 follow-ups:**
   - S4 done 2026-10-10 ([handoff](research/batches/ops-2026-10-10/s4/handoff.md), review REJECT → fixed →
     ACCEPT with notes): `reconcile_clock(account, archive=, decision=)` and `reconcile_throttle(event, decision=)`
     re-baseline monotonic timing after a reboot/suspend/clock step with a recorded decision; remaining time
     only shrinks on the same boot; after a boot change only wall time is evidence (a wall step back across
     a reboot is undetectable). `_eligible` is unchanged. The coordinator applied re-review notes 1–2 (exact
     `start_wall` write-back, event reason) afterward; they were not independently re-reviewed. The driver's
     `reconcile` phase is only `--help`-checked (an anomalous account needs a real reboot);
   - S5 done 2026-10-10 (per-recipe AST identity; new helpers must be listed in `RECIPE_FUNCTIONS`);
   - `campaign_state.py` changed again (settlement, clock reconcile), so the next offline campaign run re-executes its 36 jobs;
6. **hardware leads** (2026-10-10 hunt added SONiC/ONL platform code, Edge-Core printed order codes and
   accessories, Oracle L40S/PCA and Sun Blade 6000 facts — [hunt note](research/hunt-2026-10-10.md); none
   matched the owned CS8200/CS8210/AZ4444/7760/DMC-12/AS7772 codes):
   - original vendors stay active;
   - exact-model Celestica/Edge-Core power, rack and software gaps need a distinct
     primary lead ([exhausted pass](research/owned-networking-buying-2026-10-04.md));
   - unsuffixed Ingrasys SV1120/SV1121/SV2121/SV2221 remain gaps, along with the
     separate [Oracle gaps](research/owned-oracle-buying-2026-10-04.md);
   - ByteDance needs a distinct primary affiliation lead; don't run another broad search.
   - new: `SUN-6000` (main model `6000`) names "Celestica de Monterrey S.A. de C.V.,Celestica
     Kladno s.r.o. (Oracle America, Inc.)" as manufacturer in both the Celestica and Oracle RRA
     queries (raw rows 90 and 215). the cell does not resolve to a target with the reviewed
     company-name evidence, so there is no catalog record. a certificate-specific
     Celestica-factory/Oracle lead (RRA equipment name "Sun Blade 6000 Chassis", country Mexico). the
     public Oracle chassis facts are now in the catalog without any Celestica role; recording the
     factories needs **the user's decision** between the options in the
     [sun6000 handoff](research/batches/hunt-2026-10-10/sun6000/handoff.md) (A: certificate-scoped reviewed
     company evidence; B: hand-curated record; C: leave raw-only);
   - Edge-Core: the four owned codes are not in any captured official ordering table; datasheet URLs for
     AS9716-32D (DCS510), AS7312-54X, DCS203 and EPS203 (AS4630-54NPE) returned 404 once (not retried);
     a current AS7712 datasheet was not captured;
   - 2026-10-11 discovery batch: the same hardware model now often appears under several brand strings by design
     (canonical `Quanta`/`Inspur` for reviewed aliases vs printed `QCT`, `Quanta Cloud Technology`, `Inspur Corporation`,
     `IEIT SYSTEMS Co.,Ltd.` …; e.g. NF5280M7 is three identities). nothing is merged across brand strings; use
     `python3 -m devicehunt model-collisions` (review leads only) and a later reviewed brand-family/alias layer;
   - SmartNIC/QCT candidate-review notes F2 and F4 (Quanta-filter assertion,
     stale "not a complete bundle" scope text) belong to the next bundle revision.
7. **2026-10-11 discovery batch** ([note](research/discovery-2026-10-11.md)): ten waves of public/open-source discovery
   promoted in six reviewed rounds (ONIE, OpenBMC entity-manager/machines, Linux BMC DTS, SONiC PDDF, Meta OpenBMC,
   Ubuntu/Red Hat certified listings, SPEC power/CPU/jbb results, NetBox device types, linuxhw DMI dumps, coreboot
   boards, Stratum/DENT) plus `lookup matched_in`, `model-collisions`, `specs --include-source-facts` and the
   buyer-audit fixes (`match-listing` hides software-reference identities by default; labelled source facts;
   `buying-report` points to source facts when a category is not collected). open follow-ups, all needing **the user's
   decision** or a new bounded wave:
   - brand-family layer: the [proposal](research/brand-family-proposal-2026-10-11.md) lists 7 decisions (recommend a
     reviewed display-only family table, Quanta/QCT and Oracle/Sun first; no import-time canonicalisation);
   - IEIT ↔ Inspur: first-party current-name evidence found ([handoff](research/batches/discovery-2026-10-11/ieit-source/handoff.md));
     the 2023 rename leg is mirror-only (provisional); nothing mapped;
   - leads not imported: Linux hwmon PSU drivers, ENERGY STAR diff (no target change), Wi-Fi Alliance (API disallowed by
     robots), Red Hat cloud-instance listings, DMI board-fallback/desktop dirs, 244 older Inspur SPEC CPU 2017 results;
   - ByteDance-named server platforms (`meta-bytedance`/`g220a`, coreboot `bytedance/bd_egs`) are labels only; B4020/B4010
     stay 0/2.
8. **user decisions 2026-10-11**:
   - cleanup "Delete reducible": done — 23 files / 8.83 GB removed after hashing ([ledger](research/batches/discovery-2026-10-11/integration/cleanup-2026-10-11.json)).
   - c4 "Authorize cycle 2": done — cycle `2026-10-11a`, 1,063 unit requests + smoke, all 200, ~43 min, ~12.9 MB; **all
     seven units `matches_pinned`** (no change since 2026-09-26/27; [c4 notes](research/batches/hunt-2026-10-10/c4/c4-notes.md)).
     two consecutive no-change cycles; a periodic schedule would still be a new scope needing authorization.
   - brand family "Display-only table": done — `data/brand-families.json` (sha `e1c70517…`; Quanta {Quanta, QCT, Quanta Cloud
     Technology[, Inc.]} and Oracle {Oracle, Sun Microsystems[, Inc.]}; all 7 rows signed by the independent reviewer,
     [review](research/batches/discovery-2026-10-11/review/brand-family-table-review.md) ACCEPT); `lookup` / `match-listing` /
     `model-collisions` show `brand_family`; ranking, rows and every vendor-inventory output byte-identical; no IEIT row;
     not in the import manifest (decision 7 still open: whether the table becomes a manifest input).
   - SUN-6000 "A: scoped company evidence": done, **Kladno-only** (per the independent design review; Monterrey kept as
     unmapped literal context like the SUN-X6200-Series / SUN-T6300 precedent; the user may override). new optional RRA
     evidence kind `rra-composite-manufacturer-cells` (exact certificate + cell hash + detail hash + query guards) and a
     sibling bundle `data/raw/rra-celestica-official/bundle-composite-cells.json` (old `bundle.json` untouched). full c3b
     path: re-pin → offline run → coverage → queue → 12 independent decisions → release → gate → gated rebuild (52 tables
     match) → suite 744 OK → promote → gate/campaign/coverage tests OK → final guarded suite 744 OK. effect: +5 Celestica-brand identities (`6000`,
     `Netra 6000 AC`, `Netra 6000 Chassis`, `Oracle Sun Blade 6000 Chassis M2`, `Sun Blade 6000 Chassis`; kind is the RRA's
     literal equipment string; distinct from Oracle's own `Sun Blade 6000 Chassis` identity), Celestica manufacturer
     scope 193 → 198 — **the first manufacturer-scope change since the hunt batch**. [handoff](research/batches/discovery-2026-10-11/sun6000-a/handoff.md),
     [reviews](research/batches/discovery-2026-10-11/review/sun6000-candidate-review.md).

storage: `data/` holds 11 catalog-sized SQLite files (~4.2 GB, listed in the role table; the SUN-6000 candidate copy is reducible) and a 66 MB archive. compress superseded catalogs only through the
[history workflow](data/history/README.md) under exclusive ownership.
worldwide coverage, current support and marketplace stock remain unproven.
