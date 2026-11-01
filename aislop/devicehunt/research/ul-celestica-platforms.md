# Celestica UL platform identifiers

Reviewed 2026-09-27 UTC. The [public UL AZOT.E362364 listing](https://productiq.ulprospector.com/en/profile/2858121/azot.e362364?page=379&term=switch)
names CELESTICA TECHNOLOGY CONSULTANCY (SHANGHAI) CO LTD as its listing holder.
Web search returned the factual Models section; opening the page returned HTTP
403. No further page retrieval was attempted. The cache is a search-index text
extract, **not original HTML**. Its freshness and the original visual grouping
are unverified. UL expressly says its abridged page may omit models and may
include products or components.

The existing capture/import covers 25 CS-series identifiers. The new
`data/raw/ul-celestica-platforms.json` preserves the returned model section and
a reviewed list of 163 additional, non-CS identifiers. Importer
`ul-celestica-platforms` records spans into that section, the retrieval timestamp,
the UL document number and the legal listing-holder name. Its separate source
leaves the earlier CS evidence intact.

Selected examples include `RM-400X3`, `XD4090-S4`,
`SC6110`, `SSE-T8032S`, `AZ9074-64X-DC-42` and
`Wedge400C-48VDC-F`; and complete whitespace-qualified names `DS4000-F 02`,
`DS4000-R 02`, `SRU 0101` and `Voyager 1.1`. These examples are names in the
listing; this import does not infer specifications or cross-brand equivalents.
The excerpt's final `IcePack Canister` is left unparsed pending grouping review.
The parenthetical HPE trademark note remains in the captured text, without
creating an HPE alias or manufacturing relationship.

**Evidence role: listing holder only.** Celestica is the identity namespace,
not an asserted retail brand, factory or physical manufacturer. No Meta/AWS
deployment, current certification validity or provenance of an auction unit is
inferred. Existing independent records for the same identity remain separate.
163 source assertions add 138 identities to this snapshot; do not interpret
either number as a count of distinct physical designs.

## Local use

```sh
python3 -m devicehunt import ul-celestica-platforms data/raw/ul-celestica-platforms.json
python3 -m devicehunt search 'Wedge400C' --vendor Celestica --role listing_holder
python3 -m devicehunt search 'DS4000-F 02' --vendor Celestica
python3 -m devicehunt match-listing --title 'Surplus SSE-T8032S switch' --vendor Celestica --format json
```

```sql
SELECT d.model, a.company, a.role, r.source_url
FROM devices d JOIN records r ON r.device_id=d.id
JOIN affiliations a ON a.record_id=r.id
WHERE r.source_id='ul-celestica-platforms'
ORDER BY d.model;
```

The fixture tests cover complete suffixes/qualifiers, case, duplicate mentions,
span provenance, wildcard and dotted-token exclusions, role isolation, refresh
and validation before source mutation. The inventory baseline remains 8/10
corroborated. Neither this source nor another exact-label search closes the
complete 7760 ordering code or bare AZ4444 label gap.
