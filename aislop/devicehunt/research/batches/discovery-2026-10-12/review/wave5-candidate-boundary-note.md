# wave5 ordering-row candidate boundary

recommendation by `/root/bytedance`, 2026-10-13 UTC. **design/routing recommendation only; no candidate built or import authorized here.** Frozen [discovery acceptance](original-vendor-wave5-review.md) is untouched.

The38 rows support **35 proposed separate exact Edge-Core hardware identities** and **three rows retained as literal unresolved/service context**. Use the printed Edge-Core header/logo as the catalog brand namespace and `Edgecore Networks Corporation / documentation_vendor` only. Keep full code strings exactly; no added AS prefix, suffix decoder, manufacturer/factory/operator role, alias or cross-brand merge.

| row set | recommended boundary |
| --- | --- |
| 24 page-3 configuration rows, twelve each under AS5710-54X and AS5712-54X | Separate exact switch-configuration identities using every complete printed5710/5712 code, including region suffixes. Each row supports its own processor, explicit dual AC/48V PSU versus one12V input, airflow and region cells. Preserve the source base-model heading as literal context. |
| `PSU-AC-400-F`, `PSU-AC-400-B`, `PSU-48V-400-F`, `PSU-48V-400-B` | Four separate power-supply FRU identities. Attach only their literal400W AC/-48V description, row airflow and “no power cord” fields. No summed switch capacity or inferred part-manufacturer/input rating. |
| `PSU-12V-400-1` | Separate **power input unit FRU** identity. Only12VDC input-unit description is supported;400 in the code establishes no400W output. |
| `RKIT-85-6712`, `ORSA-1U` | Two separate mechanical accessory identities. Preserve the printed rack-kit AS5710/AS5712/**AS6712-32X** list and adapter19-inch1U-to21-inch Open Rack description as source-listed context, not universal physical fitment or a link to AS6700/AS6701. |
| `CBL-PWR-US`, `CBL-PWR-EU`, `CBL-PWR-UK`, `CBL-PWR-JP` | Four separate power-cable identities. Keep region-specific ratings/lengths and “only required with spare power supplies”; Japan's row provides no numeric rating/length to invent. |
| `57xx-54X-SVC-RTF` | Literal service-order context, **no hardware identity or wildcard expansion**. The description explicitly names AS5710 annual post-warranty Return-to-Factory service; AS5712 does not inherit it. |
| Two fan rows | Literal shared-document ordering observations/queue items only. Preserve embedded `FAN-1U-1x1D-F/B` and visible `AN-1U-1x1D-F/B` with their methods, unresolved. No new fan identity, canonical spelling or bridge to older catalog rows. |

The page-3 Base Model headings explicitly state ONIE installer context for each row group; a later record may retain that exact historical heading. Per-model CPU/memory/software sections stay on their printed AS5710/AS5712 scope. Do not copy joint-family dimensions, configured weight,282W consumption, environmental limits or software compatibility onto every full code solely from its prefix. In particular, the one12V-input rows do not inherit a weight expressly conditioned on two installed PSU modules. Raw row evidence remains attached even when no standalone identity is created.

[Read-only lookup evidence](wave5-candidate-boundary-lookups.json) performs exactly38 bounded model/identifier equality checks, one per embedded row code. Authoritative catalog SHA at observation was `4ae2cd0d6a614d582cf6fc47e6ba0e022521ac2b5e73bb268fcfa0dcb87c03ea`, unchanged across reads; the JSON records the UTC observation. This is a dated checkpoint, not a permanent baseline. No additional AN-spelling lookup was made.

No exact Edge-Core identity matched any of these38 codes. Existing matches are under **Unattributed accessory**: the five PSU/input-unit codes, both disputed FAN codes, ORSA-1U and four cable codes. Keep all those existing identities separate from proposed Edge-Core records. Their model/identifier matches do not prove equivalence, repair the fan discrepancy or upgrade their source claims. Existing `PSU-12V-400-1` kind “Power Supply FRU” also remains separate from this document's explicit input-unit description.

Counts are recommendations from the current evidence:24 switch rows +5 PSU/input-unit rows +6 mechanical/cable rows =35 unambiguous hardware records; one service pattern +two disputed fan rows =three context-only rows. Root owns the later finite candidate brief, exact record/fact choices and independent candidate/integration acceptance. No build, database mutation, source change, network, production edit or import occurred.
