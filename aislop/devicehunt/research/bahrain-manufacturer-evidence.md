# Bahrain TRA manufacturer evidence

Captured from the public endpoint used by
https://etra.tra.org.bh/approved-equipment on 2026-09-26:
https://etra.tra.org.bh/api/public/approved-equipment/list

The original page and its public approved-equipment JavaScript were inspected.
The script requests this endpoint with an optional `search` parameter, paginates
its returned array locally, and exports spreadsheet rows from that array. It
maps the manufacturer field directly to the visible Manufacturer column.
The importer does not execute downloaded JavaScript or use authentication.

Two unfiltered downloads matched by reference and content: 10,169 rows, nine
fields, unique authority references. Six query snapshots (`accton`, `celestica`,
`oracle`, `amazon data`, `edgecore`, `edge core`) were cross-checked against the
unfiltered result. No independent API total is exposed; historical/global
completeness is not claimed. `data/raw/bahrain-bundle.json` hashes all responses.
Discovery references are cached separately as `bahrain-oracle.html` and
`bahrain-approved-equipment.js`.

Manufacturer matches: 16 Amazon Data Services rows, 13 Accton rows, three
Celestica Shanghai rows and one Oracle America row. One Accton row explicitly
lists both DMC-12-48C-AS0 48V and DMC-12-48R-AS0 48V. Their suffixes are retained.
Thus 33 source rows create 34 target assertions. No Edgecore-named manufacturer
row is present in this captured response; Accton rows do not automatically gain
an Edgecore brand or AWS operator relationship.

The 16 Amazon identifiers and source references are exported in
`data/exports/amazon-bahrain-leads.csv`. These materially expand discovery beyond
the two FCC identities. Exact SCOUT/Atlanta/Hopper/Ambrose/Boxer/Monza/Barreleye/HP
names remain separate; no correspondence to CubScout, server CPU generations,
physical ODMs or marketplace unit provenance is inferred.

The older official 84-page PDF dated 7/9/2021 was located but not imported in this
pass. The structured endpoint is the source for all assertions described here.
