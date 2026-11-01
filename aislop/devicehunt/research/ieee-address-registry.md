# IEEE address assignments for auction-label research

The [IEEE Registration Authority index](https://standards.ieee.org/products-programs/regauth/)
links downloadable public CSV listings. This capture contains every row in four
files, downloaded on 2026-09-26 through ordinary HTTPS requests. A second download
of each file is byte-for-byte identical. Original files, the index, repeat copies,
row counts and SHA-256 checksums are in `data/raw/ieee/bundle.json`.

| Registry | Prefix bits | Published rows |
| --- | ---: | ---: |
| MA-L | 24 | 40,253 |
| MA-M | 28 | 6,606 |
| MA-S | 36 | 7,205 |
| IAB | 36 | 4,575 |
| Total | | 58,639 |

Completeness means the four published files, not all confidential registrant
identities or all IEEE registry types. The site exposes no independent row total.
An identical second copy checks download consistency but is not independent
proof of the underlying database's completeness.

The importer preserves exact organization names, whitespace, addresses and
duplicate assignments in `ieee_assignments`. MA-L has two prefixes appearing
under multiple names: `080030` has three rows and `0001C8` has two. It does not
pick one name or create a catalog identity from any assignment. The source tier
is `registration_authority`; these rows cannot validate inventory models.

`data/exports/ieee-target-name-leads.csv` contains 78 rows selected by literal
case-insensitive substrings: `accton`, `edgecore`, `edge-core`, `celestica`,
`oracle`, or `amazon data services`. These are name leads, with legal names
preserved; for example, Edgecore Americas is not silently rewritten to another
entity. No organization name contains `amazon data services` in this snapshot.
Other Amazon names are not evidence of that particular legal entity, and this
does not establish the absence of its hardware or its use of another supplier's
network interfaces.

## Local lookup

```sh
python3 -m devicehunt mac 8C:1F:64:72:30:01
python3 -m devicehunt mac 8c1f.6472.3001 --format json
python3 -m devicehunt lookup 'Celestica' --limit -1
python3 -m devicehunt sql "SELECT registry, assignment, prefix_bits, organization_name FROM ieee_assignments WHERE organization_name LIKE '%Accton%'"
```

The example MAC has an **invented device suffix**, not an observed device or an
owned unit. It matches the actual 36-bit assignment `8C1F64723` for
`Celestica Inc.`; the shorter IEEE Registration Authority prefix is not returned
as the assignee. Saved example outputs are `data/exports/mac-example-celestica.json`
and `data/exports/mac-example-local.json`.

The command accepts a complete 48-bit address as 12 hex digits, six uniformly
colon- or dash-separated octets, or three dotted four-digit groups. It returns
all rows at the longest matching prefix. It does not strip arbitrary characters,
guess truncated addresses, or derive a MAC from IPv6/EUI-64.

[IEEE's identifier guidance](https://standards-support.ieee.org/hc/en-us/articles/4888705676564-Guidelines-for-Use-of-Extended-Unique-Identifier-EUI-Organizationally-Unique-Identifier-OUI-and-Company-ID-CID)
defines individual/group and universal/local bits. The lookup classifies local,
multicast, broadcast and all-zero addresses without assigning a hardware owner.
Historical BlockID entries with the local bit set remain available through raw
lookup/SQL. CID is not imported for universal MAC attribution. An assignment
match identifies the registered block organization, not the chassis model,
physical factory, hyperscaler operator or provenance of a surplus unit; a MAC
may be changed or spoofed.

Missing registry data reports `not_loaded`; a loaded snapshot without a prefix
reports `no_public_match`. The command opens the existing database read-only and
does not create or migrate it. `lookup` also exposes the registry as a separate
dataset whose counts are address-assignment rows, not devices.

## Import and refresh

```sh
python3 -m devicehunt import ieee data/raw/ieee/bundle.json
python3 -m devicehunt sync --ieee
```

`sync --ieee` includes the ordinary default sync sources too. Existing checked
IEEE bundles are reused; `--refresh` downloads again. Refresh replaces working
snapshot files, so a pinned offline rebuild manifest needs new reviewed hashes
and counts afterward. No import runs unless all file hashes, CSV schemas,
prefix widths, index links, row counts and repeat downloads pass validation.

Eight tests cover longest-prefix allocation, duplicate/private names, format
validation, local/group suppression, multiline addresses, changed downloads,
malformed data, old-schema read-only behavior and preserved caller transactions.
