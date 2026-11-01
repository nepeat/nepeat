# bios engineering handoff — 2026-10-04

outcome: complete; ready for independent review and exclusive integration.

changed only `devicehunt/broadcom.py`, `devicehunt/buying.py`, `tests/test_broadcom.py`, `tests/test_buying.py` and this engineering artifact directory. no live database/import manifest/export/capture changes, no network, packages or schema changes.

## source shape and fact contract

existing `data/raw/broadcom-servers-bundle.json` validates 24 exact source records. each capture contains one Certification Details table with BIOS header; 206 total literal rows. new fact `compatibility_bios_certification_rows` is one JSON array per record, preserving row order, duplicate rows, duplicate certIds, raw scalar types and every nested/unknown row field. SQLite `json_each.key` is its zero-based source row index. certId is not a unique row key. null/empty/numeric BIOS and certId remain raw values rather than valid firmware recommendations.

absent BIOS table emits no fact; a present empty table emits `[]`. missing relevant headers/rows, malformed scalar types, nonfinite numeric JSON and ambiguous multiple BIOS tables fail validation for every captured record, including non-target partners, before `Catalog.source` can replace any data. pipeline remains unchanged: it dispatches to this importer, whose prevalidation preserves old state on malformed capture inputs. database-error transaction semantics remain those of the existing Catalog caller.

existing raw_json serialization, prior facts/identifiers/roles and source metadata wording are unchanged. no release filter-to-BIOS join, current support claim, firmware access, entitlement or flashing recommendation is introduced. buying mapping exposes the array in firmware_boot_software and `label_observed_input_voltage_range` in power, with existing record contexts and limitations.

## useful sql

```sql
SELECT r.source_key,d.model,j.key AS source_row_index,
       json_extract(j.value,'$.bios') AS bios_literal,
       json_extract(j.value,'$.certId') AS certification_id,
       json_type(j.value,'$.certId') AS certification_id_json_type,
       j.value AS complete_literal_row
FROM facts f JOIN records r ON r.id=f.record_id
JOIN devices d ON d.id=r.device_id, json_each(f.value) j
WHERE r.source_id='broadcom-servers'
  AND f.name='compatibility_bios_certification_rows'
ORDER BY r.source_key,j.key;
```

never pair array positions with the separate OS/release list. scope remains submitted compatibility configurations and exact model/record; the array is not evidence about a particular auction unit.

## checks

runtime: `/Users/nep/.cache/codex-runtimes/codex-primary-runtime/dependencies/python/bin/python3`.

- `-m unittest tests.test_broadcom tests.test_buying -v`: exit 0, 18 tests pass (`affected-tests.log`).
- `-m unittest discover -s tests -v`: exit 0, 424 tests pass (`full-tests.log`).
- meaningful coverage includes all 24 saved captures/206 rows, per-model buying isolation, exact raw rows/raw_json, duplicate certIds/rows, SQL JSON scalar types, null/empty BIOS values, absent/empty table distinction, repeat imports, old-fact/identifier/role and unrelated-source preservation, malformed late non-target record preserving complete old source snapshot, header drift/ambiguous tables/nonfinite values, and no inferred release pairing/manufacturer/hyperscaler roles.
- `git diff --check` could not execute because system Git requires the Xcode license; no configuration changed. Python fallback found no trailing whitespace in four owned module/test files (`whitespace-check.log`). coordinator may use its existing non-system Git to check diff.

module/test bytes and test logs are frozen in `hashes.json`. next: independent review, then coordinator-approved integrator imports existing Broadcom bundle alongside reviewed label input, checks old-data preservation, rebuilds to new path and compares, and refreshes exports after acceptance. do not overwrite historical snapshots.
