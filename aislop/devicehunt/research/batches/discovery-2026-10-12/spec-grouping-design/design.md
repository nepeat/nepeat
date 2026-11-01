# F10: opt-in grouping of identical specification statements

**recommend an additive `specs --group-identical` presentation mode. no default behavior change.** grouping should collect complete eligible evidence, apply current record-local filters, group strict identical claims inside one device namespace, and only then apply the limit. every group's evidence remains separate and inspectable. this is a design proposal, not implemented behavior or acceptance to change frozen code.

## current evidence

[probe-results.json](probe-results.json) binds eight actual read-only CLI/API cases to post-wave3 `data/devices.sqlite` SHA `4ae2cd0d6a614d582cf6fc47e6ba0e022521ac2b5e73bb268fcfa0dcb87c03ea`; before/after hashes match. `*-baseline-cli.json` files contain exact current CLI output; `*-probe.json` retains unlimited baseline rows, pre-collapse enriched evidence and a pure in-memory grouping prototype. no database fixture, import or production edit was used.

| case | current unlimited rows | strict groups | implication |
| --- | ---: | ---: | --- |
| NF5280M7, exact model, source facts | 11 | 7 | repeated SPEC statements fold, while IEIT Systems and IEI device IDs stay separate |
| AS7326-56X, exact model, source facts | 14 | 14 | curated/source-fact labels now differ; skipping an apparent overlap would discard distinct claims |
| Sun ZFS Storage 7420, exact model | 9 | 9 | newly promoted service facts remain distinct; grouping adds no claims or M2 equivalence |
| AS6700-32X, exact model | 8 | 8 | useful unchanged comparator; no Accton/Edge-Core identity merge |
| NF5280 discovery, source facts | 201 | 79 | current first100 rows cover only47 distinct device/category/displayed statements; grouped limit100 would retain all79 groups and all201 evidence rows |
| AIS800-32O, exact model, Accton filter | 1 | 1 | existing record-local Accton software claim only; other records cannot lend affiliations |
| R4023-F9102-A0, exact part number, Celestica | 2 | 2 | only explicit part-number evidence records qualify |
| global source-fact presentation | 2,294 | 1,911 | first100 current rows have89 distinct displayed statements; finite grouping benefit without altering stored data |

the buyer audit is historical: **current CLI default is100**, not its earlier20. current `statement_rows()` already invokes `dedupe_source_facts()`, which drops repeats within `(device_id, category, displayed statement, source_fact_name, source_id)` to the first sorted record. existing tests explicitly pin this behavior. the current eight-case profile found no repeated same-source key, including the global whitelisted view; this is a negative observation, not proof that future inputs cannot contain them. the existing DS4000 duplicate-record test demonstrates that case in a fixture. grouped mode must preserve such records even though flat defaults retain their legacy collapse.

## smallest API and CLI contract

add keyword-only `group_identical=False` after the existing `search()` arguments, with matching internal exact-part-number plumbing. add a boolean CLI `--group-identical`, independent of `--include-source-facts`. without the new flag, current return rows, JSON/CSV/table output, sorting, limits, placeholder handling and within-source collapse remain unchanged. grouping without source facts groups curated statements only.

grouped mode uses the same category whitelist, source-fact labels/placeholder rules, discovery/exact-model/alias/exact-part-number rules and record-local vendor associations. **select records first**, including alias and matched part-number scope, then collect their full statement evidence without legacy display collapse. no record excluded by vendor/alias/part-number scope may appear as a group member. there is no new role filter; existing vendor matching still accepts any recorded role. each member carries its own affiliation rows with company/target/role rather than a promoted group affiliation.

use this exact grouping key:

```text
(device_id, category, origin, source_fact_name,
 displayed_statement, raw_fact_value)
```

`device_id` binds brand, model and revision; retain those fields explicitly in output. never use model text alone, canonical company family, substring/prefix, alias, normalized statement, converted units or rounded numbers as a key. strings compare exactly, including case, whitespace and punctuation. origin/fact-name/raw-value are deliberately conservative: curated and source-fact claims stay separate even if their displayed wording happens to match; `psu_configuration_1` and `_2` cannot become one configuration. DMI path stripping is existing display behavior; differing original raw values still remain separate groups. identical text does not establish shared hardware configurations or independent corroboration.

preserve the first occurrence order of keys from the existing unlimited sorted row sequence, and the original order of evidence inside each group. conflicting values stay separate in that order. **`--limit` counts groups** in the opted-in mode:0 returns `[]`; -1 returns every group; other nonnegative values select the firstN groups. build complete groups before slicing, so a retained group keeps later eligible members even if those members were beyond the old flat limit. preserve existing validation errors for invalid limits and mutually exclusive exact modes.

grouped JSON remains a list, with each object shaped as:

```json
{
  "device_id": "existing identity id",
  "brand": "exact stored brand",
  "model": "exact stored model",
  "revision": "exact stored revision",
  "category": "power_description",
  "statement": "unchanged displayed statement",
  "origin": "source fact",
  "source_fact_name": "spec_power_supply_quantity_and_rating_w",
  "evidence_count": 2,
  "record_count": 2,
  "source_count": 2,
  "evidence": ["complete eligible evidence row objects"]
}
```

the example's evidence array is schematic; real members are objects. each contains the full original statement row (`record_id`, `device_id`, brand/model/revision, category/statement, source_id/source_url/tier, origin/source_fact_name, matched_part_numbers when applicable), plus `source_key`, `raw_fact: {name,value}` and record-local `affiliations: [{company,target,role}]`. no arbitrary first record/source URL/tier is promoted to group level. counts distinguish statement rows, distinct records and distinct source IDs; none is called “results confirming” or “independent sources.” no raw SQL/table, import schema, existing flat export or stored fact changes.

table output should show each shared statement once, with evidence/record/source counts and separate provenance lines for **every** member, including record/source IDs, URL, tier, origin/fact name, affiliations and applicable matched part numbers. those lines need not repeat the shared statement. grouped CSV should emit shared fields/counts plus UTF-8 JSON in `evidence_json`; do not let `csv.DictWriter` serialize nested lists as Python repr. JSON retains the full evidence objects even where fields repeat. empty grouped results preserve current empty-output conventions. help/docs must explain group-count limits and that grouping identical statements is not merging devices/configurations or certifying agreement.

## implementation scope and compatibility

proposed future scope only: `devicehunt/specifications.py`, `devicehunt/__main__.py`, new `tests/test_specification_grouping.py`, and `docs/specification-search.md`. no Catalog/importer/schema/manifest/rebuild/export/listing/buying-report change. do not alter current source-fact dedupe tests; add explicit grouped tests beside them. factor collection to expose an internal pre-collapse evidence path while keeping default helpers identical. collect raw fact/affiliation metadata during that path, not by reverse-matching a rendered label after losing the raw row.

existing collapse currently precedes exact alias/part-number record selection. a duplicate fixture can therefore lose a qualifying second record when the first lacks the requested identifier. grouped mode avoids this by selecting/filtering records before grouping and keeping all eligible members. this proposal does **not** silently repair flat behavior; a default-mode scope correction would need separate authorization and compatibility review.

## acceptance tests

1. flag absent/False yields byte-identical existing CLI JSON/CSV/table and API rows, with source facts both off/on; existing within-source collapse remains pinned.
2. same exact statement across two sources groups with both URLs, tiers, record IDs and affiliation roles; flattening unlimited groups reproduces the multiset of every pre-collapse eligible row once. order remains stable inside each group; grouping need not retain the original global interleaving. counts reflect rows/records/source IDs, not confirmations.
3. two distinct records in one source survive as two members (`source_count=1`, `record_count=2`) despite legacy flat collapse. alias/part-number fixture where only the later record qualifies returns exactly that record; a vendor association on a sibling does not leak.
4. source-fact versus curated origin, different fact names/raw values, configurations/units/punctuation, contradictory statements and empty/placeholder exclusions retain their defined boundaries. do not suppress a source-fact row because its value resembles curated prose.
5. identical model text under different brand/device/revision namespaces remains separate groups even with identical categories/values; role/source matching is per member, with no parent/manufacturer transfer.
6. with enough intervening rows, `limit=1` retains the first group's late duplicate members; unlimited grouping is complete,0 empty and negative invalid values rejected. existing deterministic sort/group order remains reproducible.
7. exact full model/alias/part number preserves Unicode case-fold/outer-trim matching while internal spaces/suffixes remain literal; unknown suffixes and alias-only records without facts stay empty.
8. grouped JSON and CSV evidence_json parse to the same complete member objects; table prints the statement once but every provenance member. absent flag exposes no new columns.
9. read-only database bytes and caller transaction ownership remain unchanged; missing DB fails; no network/subprocess is used. bounded live checks repeat NF5280M7, NF5280, AS7326 and Oracle7420 against the then-current accepted DB, with hash bindings.

alternatives: automatic deduplication would change current JSON/evidence and loses attribution; grouping only already-limited rows still omits late members; collapsing only per source misses the demonstrated cross-source repetition; `(model,category,statement)` merges brand namespaces; stripping labels/units to force AS7326 overlap risks combining different claims. a looser display-only key could reduce more rows but would mix origins/fields/raw values; reject it for this bounded change. `--limit -1` remains a useful existing workaround, but does not make repeated statements easier to read. the strict opt-in mode has demonstrated utility on NF5280 while imposing no default change. stop here for independent design review and separate implementation authorization.
