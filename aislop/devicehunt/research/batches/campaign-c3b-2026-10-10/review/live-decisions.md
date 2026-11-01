# c3b live decisions

2026-10-10, reviewer `c3b_decision_review (Claude Sonnet 5.5)`. Queue
`data/campaign-reviews/coverage-d2bc5c400bc3db0e95e5eff2ee92a9e926341b1d4bfe96c9f2a57dbc6cff2caa`.
Result: 12 accepted, 0 rejected (12 decision files present).

Checks applied to every unit (scripted over all 12 items, not sampled): outcome matches the coverage manifest
outcome and its comparison status is `matches_pinned` with no mismatches; `no_changes` true; candidate and baseline
`projection_sha256` and record counts equal; no added/removed/changed/moved records or raw rows; item `inputs` equal
item `baseline.inputs` and every input path/sha256 equals `data/import-manifest.json` `files` (all inputs, 0 mismatches,
0 missing).

| unit | outcome | records (cand = base) | inputs | conflicts | decision |
| --- | --- | --- | --- | --- | --- |
| rra-official-query-e5984834207f (accton RRA) | validated_nonempty | 188 | 205 | 15 repeated certificates, distinct devices, preserved | accepted |
| rra-official-query-78fc6b2e95f6 (edge-core-space RRA) | explicit_zero | 0 | 5 (empty-result.html 898bbd42 present) | none | accepted |
| ised-rel-25869 | validated_nonempty | 4 | 8 | 25869-TE1A001 with 3 distinct HVINs preserved | accepted |
| rra-official-query-63cb0b9098c7 (edgecore RRA) | validated_nonempty | 146 | 114 | 26 repeated certificates, distinct devices | accepted |
| ised-rel-query-edgecore | validated_nonempty | 4 | 8 | none flagged; 27052-EAP101N rows 1,2 kept as 2 separate records (same HVIN, same device id) | accepted |
| ised-rel-query-accton | validated_nonempty | 162 | 16 | 36 repeated certificates (incl. 3857104676A) + 15 html_csv_discrepancy; keys exactly equal c0 review_samples list | accepted |
| ised-rel-query-oracle | validated_nonempty | 12 | 8 | 9870A-BBK3 two HVINs preserved | accepted |
| rra-official-query-5827f2fe7092 (edge-core-hyphen RRA) | validated_nonempty | 13 | 11 | 4 repeated certificates, distinct devices | accepted |
| ised-rel-query-celestica | explicit_zero | 0 | 5 | none | accepted |
| rra-official-query-73009f906e4e (oracle RRA) | validated_nonempty | 896 | 520 | repeated certificates, distinct devices | accepted |
| rra-official-query-41e4ca912b33 (celestica RRA) | validated_nonempty | 149 | 160 | repeated certificates, distinct devices | accepted |
| rra-official-query-33f7d154085d (amazon-data-services RRA) | validated_nonempty | 161 | 61 | repeated certificates, distinct devices | accepted |

Every item conflict is of kind `repeated_certificate_distinct_devices` (same certificate, distinct hardware
versions or devices, kept separate), apart from the 15 accton `html_csv_discrepancy` entries. Zero units have empty
candidate and baseline projections.

## Observations (none block acceptance)

- SUN-6000 cross-query overlap: flagged in c0 `inventory.json` (see the correction below: the queue items also carry it). The two detail
  pages (celestica detail-090, oracle detail-215) are byte-identical (sha256 3c01977f...), consistent with a repeated
  fetch of one page and not independent corroboration.
- Neither live projection contains a device record for `SUN-6000`; see the correction below for why (superseded: not a gap).
- ised-rel-query-edgecore repeated 27052-EAP101N is not listed as a conflict because both rows share HVIN
  EAP101 (one device); both rows are preserved as separate records.
- Decisions accept only that the reviewed projection equals what the campaign observed; no manufacture or
  deployment claim is inferred.

## Correction (2026-10-10, at coordinator request, verified read-only)

- My earlier statement that the SUN-6000 overlap flag is absent from the queue items was wrong. Both items
  (rra-official-query-41e4ca912b33 and rra-official-query-73009f906e4e) carry a `conflicts` entry of kind
  `cross_query_certificate_overlap` for SUN-6000 with `also_in` naming the other unit and
  `independent_corroboration: false`. My earlier check only looked at conflict kinds I had filtered for. The decisions
  are unchanged; this satisfies the required flag.
- The missing SUN-6000 record is by design, not an importer gap. `rra_equipment` in `data/devices.sqlite` holds the
  raw rows (celestica row 90, oracle row 215, same detail sha256 3c01977f...), whose manufacturer cell is the composite
  `Celestica de Monterrey S.A. de C.V.,Celestica Kladno s.r.o. (Oracle America, Inc.)`; the RRA importer keeps such
  composite rows raw-only and creates no device record.
