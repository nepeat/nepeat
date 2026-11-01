# Search specifications without mixing source records

`specs` returns sourced power, cooling, physical and installation statements.
Each output row includes its source URL, source tier, record ID and device
revision. Conflicting records remain separate.

```sh
python3 -m devicehunt specs AIS800-32O --exact-model --vendor Accton
python3 -m devicehunt specs AIS800-32O --exact-model --vendor Edge-Core
python3 -m devicehunt specs R4023-F9102-A0 --exact-part-number --vendor Celestica
python3 -m devicehunt specs --limit -1 --format csv
```

## Which records qualify

| Mode | Matching scope |
|---|---|
| Default | Broad device discovery, including facts and compatibility mentions; then specifications on those devices |
| `--exact-model` with a stored model | Records for that complete model string, retaining separate brands and revisions |
| `--exact-model` with an explicit model alias | Only records that actually carry the matching alias, model-search identifier or manufacturer-model identifier |
| `--exact-part-number` | Only records that explicitly carry the complete part-number identifier |

Every mode applies `--vendor` to the specification's own source record. Another
record on the same catalog identity cannot supply that association. The filter
accepts any recorded company role; it does not by itself assert manufacturing.
Use `vendor-inventory Accton --scope manufacturer` to inspect manufacturer claims.

Exact modes compare complete strings using Unicode case folding and trimming
outer whitespace. They preserve internal spaces, punctuation and suffixes and
work without the full-text discovery index. An unknown ordering-code suffix
does not fall back to family specifications. An alias record with no categorized
statements returns none, even when sibling records have specifications.

A complete family-model query can still return multiple configurations from
separate records. Select the full ordering code with `--exact-part-number` when
available; family statements do not establish an auction unit's configuration.
Matching a specification never establishes installed software, firmware access,
licence entitlement or the prior owner of a surplus unit.

## Checked example and exports

The local AIS800-32O identity has Edgecore product-page specifications and a
separate PICOS table record associated with Accton. The Accton-filtered exact
query now returns the latter record's software statement; the Edgecore-filtered
query still exposes the product-page statements. The catalog preserves both.

`data/exports/specifications.json` and `.csv` were refreshed with the accepted
2026-10-12 wave3 batch and contain 1,285 curated statements. Their previous
PSU snapshots are preserved, with hashes, under
`research/batches/discovery-2026-10-12/integration-wave3/exports-before/`; the earlier
2026-09-27 snapshots remain under `integration/exports-before/`.
The general `scripts/refresh_exports.py` does not write these specification
files. To create a new snapshot, use `python3 -m devicehunt specs --limit -1
--format json` (or `--format csv`); preserve any historical output before replacing
it. The dated `data/exports/psu-specifications-2026-10-12.json` contains the three
accepted PSU models' exact-model statements and document-search results.
`data/exports/wave3-specifications-2026-10-12.json` adds the four wave3 model queries;
document text-search hits are leads with provenance, not fitment or identity proof.
Per-vendor JSON files are named `specifications-accton.json`,
`specifications-edge-core.json`, `specifications-celestica.json`,
`specifications-oracle.json` and `specifications-amazon-data-services.json`.
The last file is currently empty: this means no categorized statements qualify,
not that Amazon Data Services has no cataloged devices.

`data/exports/specification-scope-audit.json` is the **historical scope-fix
checkpoint**, bound to an older database with 913 statements and 277 model
strings. It documented removal of 76 statements from the Accton-filtered view
because their own records lacked an Accton affiliation, while preserving
unfiltered content and cross-source duplicates. Its read-only/database and
rebuild claims describe that checkpoint. See [current status](../PROGRESS.md)
for the accepted catalog, current validation and rebuild proofs.

## Opt-in practical source facts

`--include-source-facts` adds rows from source facts whose complete fact name is on the
reviewed `SOURCE_FACT_NAMES` list in `devicehunt/specifications.py`, mapped to the four
categories above. Without the flag, output is unchanged. With it, every row gains
`origin` (`curated statement` or `source fact`) and `source_fact_name`; source facts
keep their own `source_id`, `source_url` and `tier`. They are source-recorded facts, not
curated statements; `tier` shows the source type (a SPEC submission or datasheet can be
`vendor`) and says nothing about project review. Curated and source rows are interleaved
per device; tell them apart by `origin`. Placeholder values (`--`, empty, `N/A`) are dropped.
ENERGY STAR idle watts are rewritten as labelled per-test-configuration measurements, and a
`form_factor` naming a PCIe/OCP card or module is prefixed `component form factor (not a
chassis)`. A few NetBox rows describe PSU module types, not switches. Values are as printed (for example `2 x 800W` or `1.0 (file.yaml:6)`).

Source-fact statements read `<label>: <raw value>` (labels live in `SOURCE_FACT_LABELS`), for example
`PSU quantity and rating (W): 1 x 650` or `form factor: 2U`; the raw value stays visible. NetBox
`maximum_draw` is labelled as the library's power port maximum draw entry, not PSU capacity or measured
draw. DMI chassis rows read `chassis type reported by firmware in submitted dmidecode dumps: <type>`; the
linuxhw dump path is dropped from the text and remains in `source_url`. Source-fact rows identical in
device, category, statement, fact name and source collapse to one; different sources are never merged,
and curated statements are never deduplicated.

```sh
python3 -m devicehunt specs NF5280M6 --include-source-facts
python3 -m devicehunt specs 7726-32X-O --exact-model --include-source-facts
```

Included: SPEC PSU quantity/rating/details and form factor/node counts; NetBox power ports
(maximum draw), U height, full depth, weight and airflow; ENERGY STAR PSU and labelled
idle-watt summaries; catalog form factor, dimensions and weight; DMI chassis type (a label,
often `Desktop`/`Other` on servers); vendor as-printed PSU, redundancy and fan facts.
Excluded on purpose: SONiC/PDDF/Stratum/entity-manager/DTS/NOS/OpenBMC declarations
(they describe software platform trees, not a SKU's hardware), devicecode `power` JSON
blobs, CPU power limits, `power_limit`/`power_configuration_limit` disclaimer sentences, Red Hat/Ubuntu certification component lists and form-factor
class (`Server`), and ENERGY STAR archive test-configuration columns. All existing
filters (`--vendor`, `--exact-model`, `--exact-part-number`, `--limit`) apply to source
facts exactly as to curated statements. The family model name still matches by
discovery search, so a record may show facts for a different configuration; confirm the
ordering code. A model with only certification facts (for example `QuantaGrid D52B-1U`)
still returns nothing. `buying-report` already lists unclassified facts per record and
has no change.
