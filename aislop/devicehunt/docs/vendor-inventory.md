# List hardware by company and evidence role

`vendor-inventory` produces an offline inventory for one target company or all
eleven requested target groups. Its default scope selects explicit manufacturer/OEM claims from imported,
hashed external sources. Legal manufacturer, regulatory manufacturer and declared
OEM remain distinct roles; their inclusion does not prove a physical factory.

```sh
python3 -m devicehunt vendor-inventory
python3 -m devicehunt vendor-inventory Accton --limit -1
python3 -m devicehunt vendor-inventory Celestica --scope factory
python3 -m devicehunt vendor-inventory Oracle --scope all --format json --limit -1
python3 -m devicehunt vendor-inventory --format csv --limit -1 > manufacturers.csv
```

The default cap is 100 target/identity pairs. Totals are computed before the cap;
`--limit 0` returns counts only and `--limit -1` returns the full collected set.
No network access, schema updates or imports occur. A missing database fails
without creating a file. Existing caller-owned SQL transactions are preserved.

The offline `data/exports/catalog.html` browser now offers the same three scopes,
starting with manufacturer claims. Select a target company and optionally an
individual role, text query or hyperscaler filter. All filters on company roles
must match a single affiliation; snapshot eligibility belongs to that affiliation's
source record. Expand “evidence matching your filters” to see why a card qualified.
Other associations are still visible on each card and do not inherit that status.
The all-associations view retains every catalog identity when no company or role
is selected, including any identity without a target-company association.

## What each scope means

| Scope | Roles included | Source requirement |
|---|---|---|
| `manufacturer` (default) | `manufacturer`, `registry_manufacturer`, `declared_manufacturer`, `declared_oem` | External evidence tier with stored source hash and cache path |
| `factory` | `listed_factory` | Same source requirement |
| `all` | Every recorded target affiliation, including brand, applicant, listing holder, design/build partner, seller and unknown future roles | All; snapshot eligibility and tier shown per assertion |

Eligible tiers follow the inventory validator: vendor, project, community,
regulatory, regulatory mirror, primary-document mirror, certification body and
reviewed physical-label photo. A label still needs the explicit role claimed;
a brand-only label does not qualify as manufacturer evidence.
These tiers do not carry equal evidential weight. The output preserves tier,
role, source URL, source hash and coverage statement so each claim can be assessed.
Eligibility uses stored import metadata; this command does not re-audit source
files on disk or determine present certification validity.

Every returned role belongs to the requested target on the same source record.
An Oracle applicant affiliation cannot borrow a Celestica manufacturer role from
the same device or another record. A printed brand alone does not qualify for
manufacturer scope. For example, the Oracle-applicant Intel 9260NGW record remains
in the broader Oracle association inventory, not its manufacturer inventory.

## Current collected inventory (2026-10-04)

| Target | Manufacturer-claim identities | Factory-listing identities | All association identities |
|---|---:|---:|---:|
| Accton | 425 | 0 | 1142 |
| Edge-Core | 219 | 0 | 676 |
| Celestica | 193 | 2 | 368 |
| Oracle | 859 | 0 | 1267 |
| Amazon Data Services | 176 | 0 | 213 |
| Inventec | 39 | 0 | 46 |
| Hyve | 11 | 0 | 12 |
| Foxconn | 169 | 0 | 170 |
| Wiwynn | 2 | 0 | 14 |
| TD SYNNEX | 0 | 0 | 1 |
| Quanta | 41 | 0 | 62 |

These are catalog identities, not a count of uniquely identifiable physical
hardware models. Regulatory codes, revisions, component parts and commercial
names may represent overlapping hardware. Zero means no qualifying collected
evidence, not proof a company did not manufacture a device. None of these lists
claims complete historical coverage.

Current pair/identity totals for each scope are in [PROGRESS](../PROGRESS.md);
`python3 -m devicehunt vendor-inventory --scope manufacturer --limit 0` prints them
for the opened database. A device associated with several target companies appears under each. Factory and manufacturer scopes
are not mutually exclusive inventories and should not be added as unique devices.

Generated files:

- `data/exports/vendor-inventory-manufacturer.json` and `.csv`
- `data/exports/vendor-inventory-factory.json` and `.csv`
- `data/exports/vendor-inventory-all.json` and `.csv`

JSON groups matching evidence under each target/identity pair. CSV has one row
per matching role assertion, so several rows can describe the same identity.
Counts distinguish identities, distinct source records, sources and role assertions.
The seven target-company Korean queries supply 1,553 manufacturer model assertions,
including 896 for Oracle and 161 for Amazon Data Services. The
[literal main-model list parser](../research/rra-main-model-lists.md) recovers 84
assertions from existing pages. Main and derived models remain separate identities. See
[official RRA scope](../research/korean-rra-targets.md) and the separately retained
[mirror provenance](../research/korean-rra-reviewed.md).
The additional [reviewed multi-company extraction](../research/rra-manufacturer-lists.md)
adds 45 manufacturer assertions from those same original pages. Its separate
source entry is a derived extraction, not independent corroboration. Individual
legal names and the complete manufacturer field remain visible; none establishes
sole manufacture or the factory that built a particular unit.
Reviewed Celestica company spellings depend on a separately hashed filing text
extract; [mapping scope and per-record provenance](../research/rra-celestica-company-names.md)
distinguish exact legal names from reviewed Ltd/Limited abbreviations.
The hyphenated Edge-Core query adds seven approvals missed by the Edgecore
spelling. WA6202A retains separate Accton/NCC and Edge-Core/RRA manufacturer
claims. See [spelling coverage and confirmed-zero handling](../research/rra-edge-core-spellings.md).
The [reviewed Accton Singapore spelling](../research/rra-accton-company-name.md)
adds 13 manufacturer assertions across 12 identities, using a separately hashed
historical company-name extract with its dated parent relationship retained.
The [PICOS hardware table](../research/picos-hardware.md) adds compatibility
associations for 26 platforms under its joint Edgecore/Accton heading. It does
not increase manufacturer or factory counts.
The [Sun legacy query](../research/sun-legacy-portfolio.md) adds 26 identities
under Oracle through the separate `acquired_company_portfolio` role. Original
Sun manufacturer names remain unassigned to the Oracle target, so these records
enter `--scope all` without increasing Oracle manufacturer counts.
The table display gives compact identity rows; use JSON/CSV for full provenance.
The [Kenya CA reviewed rows](../research/kenya-approved-equipment.md) add six
manufacturer assertions and three identities. Source ambiguities are explicitly
retained, including a tablet attribution and a conflicting neighboring row.
The [Sporton cover extraction](../research/fcc-sporton-cover-roles.md) separates
named manufacturer from applicant in seven cached mirror excerpts. It adds seven
Accton manufacturer assertions while keeping three other company spellings
unassigned; source tier and format remain visible.
The [Amazon UL extract](../research/ul-amazon-platforms.md) contributes 57
listing-holder assertions and 34 new identities, without changing manufacturer
or factory counts.

## Equivalent SQL selection

```sql
SELECT DISTINCT d.id, d.brand, d.model, a.target, a.company, a.role,
                r.source_url, s.tier, s.sha256, s.coverage
FROM affiliations a
JOIN records r ON r.id = a.record_id
JOIN devices d ON d.id = r.device_id
JOIN sources s ON s.id = r.source_id
WHERE a.target = 'Accton'
  AND a.role IN ('manufacturer', 'registry_manufacturer',
                 'declared_manufacturer', 'declared_oem')
  AND s.tier IN ('vendor', 'project', 'community', 'regulatory',
                 'regulatory_mirror', 'primary_document_mirror', 'certification_body')
  AND COALESCE(s.sha256, '') <> '' AND COALESCE(s.cache_path, '') <> ''
ORDER BY d.brand, d.model, r.source_url;
```

Six tests verify target/role isolation, cross-brand factory evidence, seller and
unhashed exclusions, unknown-role preservation in all scope, totals/limits,
read-only JSON/CSV operation, caller transaction preservation and report metadata
parity. The full suite passes 335 tests. Six JavaScript checks exercise the actual
inline page script, including scope isolation, pagination and combined filters.
The live snapshot audit compares all result IDs against the SQL inventories for
each scope and each target plus all targets (18 comparisons); it is a behavior
check in a minimal DOM harness, not browser visual verification.
A full-file database hash before and after live inventory export
and baseline validation is unchanged. No source or schema change was needed.
