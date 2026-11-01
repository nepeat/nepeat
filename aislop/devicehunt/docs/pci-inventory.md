# Identify components from a saved Linux inventory

DeviceHunt can decode a numeric, machine-readable PCI inventory using its local
database. On the Linux machine you want to identify, save:

```sh
lspci -D -vmm -n > pci-inventory.txt
```

Copy that text file into your research workspace and run:

```sh
python3 -m devicehunt pci-report pci-inventory.txt
python3 -m devicehunt pci-report pci-inventory.txt --format json > pci-report.json
python3 -m devicehunt pci-report pci-inventory.txt --format csv > pci-report.csv
```

The decoder reads the supplied file and local database. It does not scan local
hardware, run commands from the dump, contact a server, or import observations
as external catalog evidence. Each record describes a PCI function at a slot,
not necessarily a separate card or chassis. Use one machine's dump per report.

The input format follows the [PCI Utilities machine-readable output specification](https://github.com/pciutils/pciutils/blob/master/lspci.man).
Use the numeric `-n` option and tagged `-vmm` format. The normal human-readable
listing, name-plus-number `-nn` format and old `-vm` format are not accepted.
Those inputs produce an explicit error instead of guessed identifiers.

## Reading the result

- `device_name_found`: the local snapshot names the complete vendor/device pair.
- `vendor_only`: it knows the vendor ID but not this device ID.
- `unknown_id`: the vendor ID is absent from the local snapshot.
- `not_loaded`: this database has no loaded PCI ID snapshot.

Subsystem names are decoded separately. A device-specific subsystem entry takes
precedence over an explicitly published generic subsystem name. An unresolved
subsystem remains empty even when the controller itself is known.

Class and subclass are decoded from the supplied four-digit Class field. ProgIf
is added only when present in the input. Missing ProgIf never implies `00`.
The single-ID `pci --class-code` option likewise accepts either four or six hex
digits, preserving whether a programming interface was supplied.

JSON retains original tag values, duplicate Module tags, unknown future tags,
line numbers, normalized slot addresses, an input checksum and full local
lookup provenance. Unknown tags remain observations without interpretation.
CSV/table output is a compact component list with reported revision and driver
fields; those reported values have not been independently verified.

Malformed records, duplicate singleton tags, duplicate slot addresses, incomplete
subsystem pairs and missing required fields fail the entire report. All input
is checked before lookup. A missing domain is normalized to `0000` when detecting
duplicate slots. Two functions with the same vendor/device ID remain separate
when their slot addresses differ. Limits: 10 MB, 5,000 functions, 128 fields per
record and 4,096 characters per field value. UTF-8 BOM and CRLF are accepted.

All lookups share one read transaction. Source names remain community assertions,
and supplied observations can be inaccurate. A matching ENA, NVMe or Neuron name
does not establish an Amazon Data Services chassis or hyperscaler provenance.

## Reproducible demonstration

`examples/pci-inventory-demo.txt` is a **synthetic demonstration**, not a dump
from the user's equipment or an auction listing. It includes two resolvable
device pairs, one vendor-only pair and one unknown pair. Its outputs are saved
under `data/exports/pci-inventory-demo.json` and `.csv`.

```sh
python3 -m devicehunt pci-report examples/pci-inventory-demo.txt --format json
```
