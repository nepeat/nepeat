# specs_dedupe handoff (2026-10-11)

## Changed
- `devicehunt/specifications.py`: `SOURCE_FACT_LABELS` (explicit label per whitelisted name), `source_fact_statement` now returns `<label>: <raw value>`, DMI citation stripping (`DMI_CITATION`), `dedupe_source_facts` applied only to `origin == 'source fact'` rows at the end of `statement_rows` (flag-on path only).
- Tests: 6 appended to `tests/test_specifications_source_facts.py` (labels, label coverage of every whitelisted name, DMI strip, dedupe incl. curated untouched, same-source multi-record collapse, flag-off byte-identical). Three existing assertions that pinned the bare values (`'2'`, `'2U'`, `'2U Rackmount, 4 Nodes'`) were updated to the new labelled text; that is the requested behaviour change, no assertion was loosened otherwise.
- Docs: short paragraph in `docs/specification-search.md`.

## Labels
`PSU quantity and rating (W): 1 x 650`; `form factor: 2U`; `NetBox power port maximum draw entry (W, library entry; not PSU capacity or measured draw): <raw>`; ENERGY STAR idle watts and component form factor keep their earlier wording. NetBox raw values keep their `(file.yaml:line)` suffix and per-port `maximum_draw=N` text (not parsed to a bare number, so the raw value stays visible).
DMI: `chassis type reported by firmware in submitted dmidecode dumps: Rack Mount Chassis`; path removed from text, still in `source_url`. A trailing `[N of N dumps]` is kept verbatim (consistent with the "submitted dmidecode dumps" label).

## Dedupe: important finding
Live data, flag on: 1009 source-fact rows before, 1009 after (2246 total rows; flag-off 1237 unchanged). The requested key (device_id, category, statement, source_fact_name, source_id) has ZERO duplicates in the catalog: no source has two records with the same fact on a device. The audit's 415 comes from collapsing on (model, category, statement) across DIFFERENT sources (594 distinct groups of 1009; 320 repeats are the same text from different SPEC submissions, 75 differ by name and source, 20 differ only by fact name within one source). Per the instruction that sources stay separate for attribution, those are deliberately not merged. If the repeats should still not eat the 20-row limit, the option is a display-only "x N sources" fold (new decision, not done here).

## Verification
`python3 -m unittest tests.test_specifications_source_facts tests.test_specifications tests.test_buying tests.test_listing_specifications tests.test_ocnos`: 49 OK (was 43; +6). Full suite not run. data/ read-only; `__main__.py` untouched.
