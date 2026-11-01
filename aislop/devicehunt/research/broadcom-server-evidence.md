# Broadcom server compatibility evidence

Captured 2026-09-26 from the public compatibility guide's `live` persona.
The original VMware PDF URL redirects to the new web application. Its public
search scripts expose the read-only API used here; no login or private persona
was used. API responses are cached original JSON, not search-engine extracts.

The seven queries are Accton, Edgecore, Edge-Core, Edge Core, Celestica, Oracle
and Amazon Data Services, restricted to the `server` program. Oracle returns 24
records; the other six return zero. A zero here means no result for this query
in this program/persona, not that the manufacturer has no devices. Each response
has a `totalCount`; it must equal the returned row count. All searches were
repeated after collecting details and their result dictionaries must agree.
There are 38 response files plus a bundle with URLs, request methods and hashes.
The importer currently rejects a query that needs additional pages rather than
silently importing a truncated result.

[Oracle Server E5-2c, record 57328](https://compatibilityguide.broadcom.com/detail?program=server&productId=57328&persona=live)
provides the exact model and Oracle partner name in both search and detail data.
This closes the E5-2C source-coverage gap. Its configuration describes an AMD EPYC
9004-series rackmount platform with two sockets. The CPU-series maximum-cores
field is retained under its full compatibility-specific name; it is not an
installed CPU count or a claim about the owner's machine. The guide's notes
explain that entries concern configurations submitted for compatibility testing.
BIOS and release data from unfiltered tables are retained in raw JSON without
inventing their pairings. The BIOS-row importer described below also exposes the
literal certification rows for SQL. No factory/OEM, previous hyperscaler deployment,
software entitlement or access to vendor firmware is inferred.

The 24 records describe 21 distinct exact model strings after case folding;
multiple certification IDs for one model remain separate evidence records.
Four previously absent model identities are X5-2M, X6-2M, E6-2c and X11-2c.
Existing E5-2c benchmark evidence remains visible with its original weaker tier.
All four Oracle baseline labels now have corroboration from external snapshots.

## Structured BIOS rows

The 2026-10-04 importer review found one BIOS table in every saved detail capture:
206 rows across 24 records. `compatibility_bios_certification_rows` stores each
complete ordered array on its own record. It retains duplicate rows and certIds,
raw JSON scalar types, blank/null values and nested metadata. A certification ID
is not a unique row key; use `json_each.key` for the zero-based source row index.
An absent table produces no fact; a captured empty table produces `[]`.

The importer validates all captured records before replacing source evidence,
including late non-target records. Every original record, role, identifier and
raw detail remains intact. E4-2C, E5-2C and X10-2C each have three BIOS rows in
this snapshot. No Oracle X7-2C BIOS record or release/BIOS pairing is inferred.
The [buying-report guide](../docs/buying-reports.md) gives the SQL query;
[PROGRESS](../PROGRESS.md) owns live import status and verification paths.

## Reproduce locally

```sh
python3 -m devicehunt import broadcom data/raw/broadcom-servers-bundle.json
python3 -m devicehunt search E5-2C --vendor Oracle
python3 -m devicehunt validate-inventory
```

```sql
SELECT d.model, i.value AS guide_record_id, r.source_url
FROM devices d JOIN records r ON r.device_id=d.id
JOIN identifiers i ON i.record_id=r.id
WHERE r.source_id='broadcom-servers' AND i.kind='broadcom_server_id';

SELECT model, name, value, source_url
FROM device_facts WHERE source_id='broadcom-servers';
```

The public endpoints observed in the application are
`/compguide/programs/viewSearchResults?limit=100&page=1&keyword=Oracle&programIdList=server`
(GET) and `/compguide/programs/viewDetails?programId=server&id=57328&filterBy=`
(POST used as a read-only detail query). Both use `USER-PERSONA: live`.
The bundle records every exact URL and method. The importer checks hashes,
partner/model agreement, unique record IDs, response shapes and query stability
before replacing any catalog evidence. A refreshed capture requires the same
checks and a reviewed manifest update; no unattended refresh is configured.
