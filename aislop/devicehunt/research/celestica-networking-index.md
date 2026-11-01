# Celestica networking index: original local snapshot

The [Celestica networking index](https://www.celestica.com/our-expertise/platform-solutions/networking)
was downloaded successfully on September 26, 2026. The original HTML is now
`data/raw/celestica-networking-index.html`; its hash, reviewed cards and source URL
are in `data/raw/celestica-networking-index-bundle.json`.

This upgrades the DS6000, DS6001 and DS6010 records from uncached web-review leads
to imported external-source evidence. Their three older seed records and source
are removed from `data/curated.json`, and the importer replaces the old
`celestica-networking-webreview` source with `celestica-networking-index` after
validating the entire input. A regular curated refresh can no longer overwrite
this source. The earlier unusable browser-message HTML remains a historical local
file; it is not used as evidence or substituted for the successful snapshot.

The snapshot contains **15 product cards, 17 model assertions**:

- DS6000, DS6001, DS5000, DS4101, DS4100, DS6010
- DS3000, DS3001, DS4000, DS4001, DS1000, DS2000
- ES1000, ES1010, ES1050, ES1500, EG1050

All 17 already existed as catalog identities. Net assertions increase by 14
because three weaker assertions were replaced. Related tiles for the documentation
portal, rack integration services and network connectivity solutions are recorded
as excluded headings, not hardware models. Coverage is the displayed index at
capture, not every historical or custom Celestica device.

Brand is the supported relationship. The source does not prove the physical
factory or the operator of an auction unit. Grouped DS4000/DS4001 and ES1010/ES1050
cards become separate identities sharing a qualified description; alternatives
are not allocated to particular models and no aliases are inferred. All original
introductions and product links remain searchable. Form-factor tokens retain the
published spelling, including `2 OU` and `1OU`.

Examples of useful distinctions: DS6000's description specifies a 3U format for
19-inch racks; DS6001's describes hybrid cooling and 2 OU ORv3. DS6010's describes
integration with AMD Helios and Open-Rack-Wide. These are vendor descriptions, not
proof of installed options, present stock, firmware entitlement or deployment by
a named hyperscaler. “Optimized for” marketing lists are not promoted to deployment
claims. ES1500's “Newest Addition” suffix is retained in its source heading but
not its model identifier.

```sh
python3 -m devicehunt lookup DS6000
python3 -m devicehunt specs DS6001 --exact-model
python3 -m devicehunt search 'Helios' --vendor Celestica
python3 -m devicehunt import celestica-networking data/raw/celestica-networking-index-bundle.json
```

```sql
SELECT model, name, value, source_url
FROM device_facts
WHERE source_id = 'celestica-networking-index'
ORDER BY model, name, value;
```

The parser uses product-tile boundaries, a linked heading and that tile's
introductory description. It handles the source's nested paragraph tags, preserves
entities, checks local product-link hosts and refuses duplicate models, malformed
or truncated tiles, changed reviewed cards and hash mismatches before replacing
existing evidence. New qualifiers require review. All extracted and excluded
cards must match the reviewed bundle.

Three regression tests cover source migration, separate evidence preservation,
grouped-model scope, false manufacturer/operator inference, malformed cards,
entities, changed snapshots and hashes. All **256 tests pass**, including the read-only command regression tests below. The source, HTML
and updated curated file are covered by the offline rebuild manifest.

Local outputs:

- [`celestica-networking-index-facts.csv`](../data/exports/celestica-networking-index-facts.csv)
- [`celestica-networking-snapshot-validation.json`](../data/exports/celestica-networking-snapshot-validation.json): separate DS6000/DS6001/DS6010 evidence check; the user's ten-device baseline is unchanged.
- [`celestica-networking-readonly-check.json`](../data/exports/celestica-networking-readonly-check.json)

## Read-only search commands

Verifying the specification example exposed that ordinary catalog queries used
the write-mode initializer. That rewrote database metadata even for a query and
silently created an empty database when a user mistyped the path. Search, specs,
show, shopping, stats, raw-register search, export and report now use a read-only
connection and one read transaction. Missing paths fail without creating a
folder or empty report. They do not migrate older schemas; explicit `init`,
`import` and `sync` retain write access. The pre-existing dedicated read-only
lookup/SQL/inventory paths are unchanged.

Three tests exercise eleven query/report commands against real fixture data,
compare database bytes, reject missing paths, verify actual write denial, and
check that an old schema remains unchanged until explicit initialization.
