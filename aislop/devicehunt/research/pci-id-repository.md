# Local PCI ID database

The [PCI ID Repository](https://github.com/pciutils/pciids) publishes a complete
`pci.ids` snapshot with community-maintained names for PCI vendors, devices,
subsystems and classes. This local copy is file version **2026.09.25**, pinned to
commit `fe4b3f80e3234847cc7e8b8a32496cb94d4a6e7e`. Original data, commit metadata,
license README and a checked bundle are under `data/raw/pciids/`.

This is a complete parse of that published file, not a claim that every PCI
device is known, an official PCI-SIG allocation register, or a chassis/OEM
inventory. No PCI row creates a catalog device or baseline-validation pass.
The [maintainer's format specification](https://github.com/pciutils/pciutils/blob/master/pci.ids.man)
distinguishes vendor/device IDs from subsystem vendor/device IDs. A controller
and its board can have different vendors. Device-independent subsystem names
are supported as explicit fallback entries, never synthesized from another
controller's subsystem row.

| Entry kind | Rows |
|---|---:|
| Vendor | 2,518 |
| Device | 21,607 |
| Subsystem | 18,089 |
| Class | 22 |
| Subclass | 118 |
| Programming interface | 84 |
| **Total** | **42,438** |

There are no device-independent subsystem entries or unknown extensions in
this snapshot. The parser supports both, with unknown extensions explicitly
reported in the bundle. Six referenced subsystem vendor IDs have no vendor
definition: `0000`, `1234`, `1b50`, `1b6c`, `1bab`, `2210`. Their rows remain
present with empty resolved vendor names. Comments and uncertainty annotations
such as “wrong ID” are retained, rather than silently corrected.

Target-related names include `1113` Accton Technology Corporation, `18d4`
Celestica, `108e` Oracle/SUN, and `1d0f` Amazon.com, Inc. Those are verbatim PCI
names: Amazon.com is not relabeled as Amazon Data Services. The Amazon namespace
contains ENA/EFA, NVMe and Neuron device names. They identify PCI functions;
they do not establish an auction chassis model, physical factory or deployment.

## Search and SQL

```sh
python3 -m devicehunt pci 1d0f:ec20
python3 -m devicehunt pci 1113:1211 --subsystem 1113:1211 --class-code 020000 --format json
python3 -m devicehunt lookup 'Oracle/SUN' --limit 10
python3 -m devicehunt lookup '1d0f:ec20'
python3 -m devicehunt import pci-ids data/raw/pciids/bundle.json
```

`pci` accepts complete four-digit hex ID pairs. Class decoding accepts four or
six hex digits supplied by the caller, with no assumed programming interface
when only class/subclass are supplied; it does not infer a class from a device name.
Exact device-specific subsystem matches take precedence over explicitly
published generic subsystem entries. Missing matches stay empty. Read-only
commands do not migrate an older database or create a missing database.

For a complete numeric `lspci -D -vmm -n` dump, use `pci-report FILE`.
See [saved inventory decoding](../docs/pci-inventory.md) for the supported format,
unknown-ID handling and JSON/CSV output. These observations stay outside the
evidence catalog.

```sql
SELECT entry_kind, id_path, name, vendor_name, subvendor_name
FROM pci_entries
WHERE vendor_id = '1113' OR subvendor_id = '1113'
ORDER BY entry_kind, id_path;

SELECT id_path, name
FROM pci_entries
WHERE entry_kind = 'device' AND vendor_id = '1d0f'
ORDER BY id_path;
```

The full `pci_entries` SQL table retains line numbers, comments, separate ID
fields and resolved names. Unified lookup now includes thirteen datasets.
The offline HTML catalog continues to display catalog identities; PCI lookups
are available through the CLI and SQL. The raw database is not embedded in HTML.

## Reproducibility and refresh

The importer verifies all three dependency hashes, commit metadata, pinned
data URL, version header, entry counts and unresolved-vendor scope before
replacing source rows. All lines must parse or belong to an explicitly reported
unknown extension. Duplicate identities and orphan child rows fail the import.
The schema adds `pci_entries` at version 13; existing catalog evidence is unchanged.

To refresh, capture a new immutable commit's `pci.ids` and README with its commit
metadata in a separate directory, construct a bundle with the same fields and
review its parsed counts. Do not overwrite the files referenced by the existing
rebuild manifest. Import the new bundle, then update the manifest dependency
hashes and expected counts only after review and baseline validation. This turn
adds offline import and lookup; there is no automatic PCI network refresh command.

The upstream README and file header offer GPL version 2 or later, or the
3-clause BSD license. Preserve the upstream copyright and license notices when
redistributing the snapshot; it is not described here as public domain.
