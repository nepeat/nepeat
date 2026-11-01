# exact buying reports

`buying-report` groups existing, record-local statements into power, cooling,
chassis/physical, firmware/boot/software, licensing and compute/network/storage.
It accepts any complete catalog model, record-local model alias or part number:

```sh
python3 -m devicehunt buying-report '7760-32X-A-AC-F' --vendor Edge-Core
python3 -m devicehunt buying-report 'E5-2C' --vendor Oracle --format json
python3 -m devicehunt buying-report 'SV7220G3-V+'
python3 -m devicehunt buying-report 'QuantaGrid D51B-2U'
python3 -m devicehunt buying-report 'Oracle Server X7-2C'
python3 -m devicehunt buying-report 'Oracle Server E4-2C'
python3 -m devicehunt buying-report 'AS7772-32X'
```

Use the bundled Python path from [README](../README.md) when the system runtime
is unavailable. `--db PATH` precedes the command when selecting another database.
The CLI opens SQLite in read-only mode; a missing file fails without creating it.
There is no source acquisition, import, baseline write or export refresh.

Matching casefolds the complete identifier; punctuation, suffixes and plus signs
remain significant. A literal device model can select that identity's records.
Aliases and part numbers qualify only records explicitly containing that value.
A vendor filter requires its affiliation on each selected record. An unknown
variant gets a gap report, without specifications borrowed from a family or
neighboring model. The owned fixture supplies validation queries only; it is
never report evidence.

For QCT platforms, keep the printed `QuantaGrid`/`QuantaPlex` prefix and omit the
vendor filter to inspect all exact-model records. `--vendor Quanta` selects only
records with a reviewed Quanta affiliation; it does not transfer that affiliation
to separate QCT specification records.

Each category has one status:

- `not_collected`: no statements from the reviewed fact-name mapping. This is
  limited to mapped fields; inspect unclassified facts and captures before
  concluding that a source lacks evidence.
- `seller_only`: all mapped statements are marketplace claims. Independently
  sourced evidence for that category remains missing.
- `collected_source_statements`: at least one mapped non-marketplace statement.
  This does not certify the category is complete, true for an auction unit,
  current or sufficient to buy or operate it. Seller claims remain labeled.

Classification uses explicit field names in `devicehunt/buying.py`, never free
text keywords or ordering-code decoding. Unrecognized fields remain inspectable.
The whitelist does not exhaust every raw certification/test field, including
historical ENERGY STAR archive fields. Thermal/temperature statements in cooling
are context, not a fan/chassis compatibility guarantee. Licensing includes
source-stated service/lifecycle restrictions; it does not establish entitlement.
Historical compatibility, source-code or document indexes, and photographed
firmware remain their own kinds of evidence, without boot or support guarantees.

A `not_collected` power, cooling or chassis/physical category may carry a
`source_facts_available` pointer (JSON: `count`, `fact_names`, `see`; table: one
appended line). It means whitelisted `specs --include-source-facts` fact names
(reused read-only from `devicehunt/specifications.py`) exist on the same matched
records, so the report's identity and vendor scope applies; it lists names only,
never values, and leaves the status, statements and key order unchanged. `see`
reproduces that scope: it uses `--exact-part-number` when the identity matched
only by part number, `--exact-model` otherwise (both, joined by ` ; `, when the
report mixed the two), the query as typed, and `--vendor` when the report was
vendor-filtered. The count is distinct (source, fact name, statement) rows over the
matched records; `specs` de-duplicates per device, so its row count for that
category can be lower. Absent when there are none.

JSON exposes `matched_records`, `categories`, `unclassified_facts` and
`limitations`. Each statement has the exact model/revision, record ID, source
ID/URL/tier/SHA-256, source coverage/date/revision and company roles.
`context_record_id` joins to `matched_records[].record_id`; `context_facts`
there preserves every record fact, including scope, caveats, dates, page references
and unresolved conflicts. A null source hash/date stays null. Statement values
remain literal; conflicting values are retained separately. Table output prints
category statements and then their complete record context.

For direct SQL inspection, use the catalog's literal model, which here is
`Oracle Server E5-2c`; `E5-2C` is a record-local alias accepted by the report:

```sh
python3 -m devicehunt sql "SELECT brand,model,name,value,source_id,source_url,tier FROM device_facts WHERE model = 'Oracle Server E5-2c' COLLATE NOCASE ORDER BY source_id,name,value" --format json
```

That view is convenient for inspection; join `records`, `devices`, `facts`,
`sources` and `affiliations` by record ID when comparing revisions or applying
company-role filters. SQL `NOCASE` handles ASCII identifiers; the report uses
Python casefold. Both are exact comparisons, with no family expansion.

## BIOS certification rows and photographed power markings

The Broadcom importer preserves its unfiltered BIOS table as one
`compatibility_bios_certification_rows` JSON-array fact on each source record.
The firmware category displays that array. Use SQLite `json_each` to query the
individual rows; the array index preserves order and duplicate certification IDs:

```sql
SELECT d.model, r.source_key AS guide_record_id, j.key AS source_row_index,
       json_extract(j.value, '$.bios') AS bios_literal,
       json_extract(j.value, '$.certId') AS certification_id,
       j.value AS complete_literal_row, r.source_url
FROM facts f
JOIN records r ON r.id = f.record_id
JOIN devices d ON d.id = r.device_id
JOIN json_each(f.value) j
WHERE r.source_id = 'broadcom-servers'
  AND f.name = 'compatibility_bios_certification_rows'
ORDER BY r.source_key, j.key;
```

Run this through the `sql` command or a read-only SQLite client. Each complete row
retains literal values and nested source fields. An absent table produces no fact;
a captured empty table produces `[]`. Neither array position nor certification ID
establishes a pairing with the separate release list. These are historical
submitted configurations, not a firmware recommendation or download entitlement.

`label_observed_input_voltage_range` belongs to the power category and describes
only the printed marking on the photographed unit. Its source-location reference
stays in the record context. It is not measured consumption or a model-wide PSU
specification. The [current handoff](../PROGRESS.md) links accepted imports and
report outputs, distinguishing current exports from historical buying snapshots.

Named Oracle `sm_1u_*` and `sm_2u_*` fields retain separate variant labels in
chassis and compute categories. Their processor-wattage fields are compute
statements, never PSU/input-power evidence. Use the full `Oracle Server X7-2C`
model to include the VPAT record; another record's short alias does not select it.

The firmware category can also contain published device-tree declarations.
For AS7772-32X these retain their community tier and complete artifact context,
without asserting installed firmware, flashing instructions or an owned-unit link.

`cmvp_tested_configuration` displays complete software-module test environments,
including PAA and single-user-mode qualifiers. Module version, saved status,
validation history and caveat stay in the same record's full context; the module
version is not installed firmware and a tested environment is not general/current
OS support. Use the full `Oracle Server E4-2C` name to include its reviewed detail.

For Ingrasys, preserve A/I suffixes and omit `--vendor Foxconn` when inspecting
vendor specification records. Foxconn portfolio records are separate dated
company-relationship evidence; that filter does not transfer the relationship
to a specification record. Unsuffixed names do not borrow A/I specifications.

Programmatic use: `buying.run(db, query, vendor=None)` requires a connection with
`sqlite3.Row` row factory and returns a JSON-serializable report.
`buying.render_table(report)` returns its text presentation.
