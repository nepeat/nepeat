# AS8000 / SC6100 practical-fact candidate

owner `/root/bytedance`; staged 2026-10-12 PDT / 2026-10-13 UTC. **candidate complete; independent review required; not imported**. only this directory written. no network, new extraction/rendering, code/shared-doc edits, archive/catalog/manifest writes or delegation.

[candidate](candidate.json), [builder](build_candidate.py), [checks](checks.json),
[routing](routing.json), [copy provenance](provenance.json).
Inputs: [accepted discovery review](../../review/as8000-sc6100-discovery-review.md),
[original findings](../as8000-sc6100/findings.json),
[capture handoff](../as8000-sc6100/handoff.md),
[requests](../as8000-sc6100/requests.json),
[render provenance](../as8000-sc6100/render-provenance.json).

## exact finite payload and identity routing

Two sources, two records, **26 facts**: 14 AS8000 observations and 12 SC6100 observations. Each fact is one selected observation with its unchanged reviewed value plus explicit physical PDF page. Raw selected observation, scope, complete page text, PDF/sidecar hash and corresponding visual-page hash accompany each in `evidence_per_observation`. No extra fact was added. Practical power/cooling/physical facts use the existing categorized names; other declarations remain literal scoped observation names. Identifiers are empty, with no alias or component-code bridge.

| printed identity | routing | record/source |
| --- | --- | --- |
| Edge-Core / Minipack-AS8000 | proposed new system identity `7def8cc8592b77d60107`, exact name absent live | key `Minipack-AS8000`; `edgecore-as8000-r02-practical-facts-2026-10-12` |
| Celestica / SC6100 | existing exact identity `28d8c44c9c82eff65231`, kind retained | key `SC6100`; `celestica-sc6100-datasheet-practical-facts-2026-10-12` |

Read-only routing records every nearby AS8000 identity. Accton's same-spelling model, Edge-Core's product-index long name, `-16Q`, `-O-AC-F`, Chassis, Cover, SCM, FAN and RAIL KIT identities remain separate. PIM is only a source-scoped module declaration. Both new source IDs were absent live. The affiliations are solely `Edgecore Networks Corporation / documentation_vendor` and `Celestica Inc. / documentation_vendor`; no manufacturer, factory, hyperscaler or deployment role is added.

## preserved evidence boundaries

Byte-identical snapshots retain both raw PDFs and complete honest Poppler sidecars: AS8000 twelve pages, SC6100 two pages. Only AS8000 physical pages 1–4 and SC6100 pages 1–2 were visually reviewed by discovery author and independent reviewer. Six original PNGs stay in discovery, hash-bound through provenance and each selected observation. AS8000 pages 5–12 supply no selected fact; page-12 garbling remains unchanged and unused. The unreviewed page-5 `Minipack-AS80000` warning spelling is not an alias. Complete extraction is not whole-document visual review.

AS8000 retains configured 55 kg weight with one SCM/eight PIMs/eight fan trays/four PSUs; all three conditioned per-PS input lines including the Bel Fuse/Murata exception; 3000 W consumption rather than PSU output; 1829 m altitude conditioned on 0–35 C; separate general two-PSU operation and three-installed-PSU ten-minute service limit; rail/rack/baffle, separate-circuit, shipping-fan and module service scopes. No transfer to standalone chassis/modules or auction configuration.

SC6100 retains platform versus node scopes, optional BBU without runtime, fourteen fans hot-swapped with nodes/seven per node, two 2400 W PSUs with 1+1 redundancy at 200–240 VAC without summed capacity, H/W/D order, distinct operating/storage environment and derating, sound power rather than sound pressure, and the populated-drive 737.28 TB example. Conflicting low-profile/HHHL slot wording remains raw exclusion, not a resolved fact. Neither source establishes exact PSU models, airflow direction, current support or unit provenance.

Capture UTC dates and exact final PDF URLs remain in candidate sources and original four-exchange ledger. AS8000 literal `E082019-CS-R02`/`150200002142A` and SC6100 copyright2024/`092724` remain document observations, not manufacture/firmware dates. Source rights remain upstream.

## reproducibility and verification

Run from project root with the existing runtime:

```sh
PYTHONPATH=. python3 research/batches/discovery-2026-10-12/original-vendor-wave4/as8000-sc6100-facts/build_candidate.py --check
```

`--check` writes nothing: compares candidate/provenance bytes, snapshot copies and reviewed finding pins; validates all fourteen sidecar pages; checks exact routing/new-source absence through `mode=ro`; verifies six PNG hashes and all selected page boundaries. Initial build uses exclusive creation or byte-equality checks and cannot overwrite differing staged captures. No importer is invoked.

Final independent local checks passed: exact keys/counts, all selected values/observations equal discovery findings, every raw page equals its complete sidecar, four snapshot copies equal originals, all 22 original discovery artifact hashes/sizes still match their manifest, and `--check` leaves staged bytes unchanged. Live catalog SHA remains `53a9a0eabaef5656b38ad922f96d963b76839eeda2da3b28a18b22359d857dee`; manifest remains `6ab359e22929bdb6a55e47c2a816c2c78fc3e5d0f90c8b252129f8fafea3aac5`. These are timestamped routing observations, not future promotion assertions.

Candidate SHA-256: `66fbabc7252652a5748d71e53a45cfd29ecf26d434a77ccedd45e18c2d3aae00`.
Builder SHA-256: `1aa25c79673dd02b56433025e05ed7de8da4832998bb414a6e43b9b06aed499a`.
Other artifact hashes are in checks/provenance and the local `SHA256SUMS`.

Stop condition met. Next is independent candidate review; only the coordinator may later integrate facts and both complete PDFs into a successor full collection while preserving every accepted entry. Importer repair/code review and combined rebuild acceptance remain separate work.
