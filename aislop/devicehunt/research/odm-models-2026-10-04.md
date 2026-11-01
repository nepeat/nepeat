# Model-specific ODM evidence — 2026-10-04

Later work supersedes this pass's next-lead list: the
[Ingrasys batch](ingrasys-servers-2026-10-04.md) imported the listed server/storage
names, A/I variants and separate Foxconn portfolio assertions. Its unsuffixed
server-catalog original remains malformed; the later
[server-evidence milestone](server-evidence-batch-2026-10-04.md) imports eight
precise A/I records from a separately documented reconstructed derivative.
The [Wiwynn/Hyve batch](wiwynn-hyve-2026-10-04.md)
also imported JBOF and Hercules. Use [current progress](../PROGRESS.md) for next
work; the findings and original pass's lead list below are historical.

This bounded pass adds two original vendor PDFs, three reviewed product records
and two new identities. It does not add a manufacturer or operator claim.
See PROGRESS.md for the current promotion status and database hash.

| Identity | Evidence and useful auction distinction |
|---|---|
| Wiwynn SV7100G4 | Two-page vendor datasheet identifies a Yosemite V3 / Open Rack V2 platform. Its chassis uses a centralized 12.5 V DC busbar. |
| Wiwynn SV7100G4-C | Page 2 explicitly names this blade. Preserve `-C`; the platform and blade are separate identities with a stated relationship, not interchangeable aliases. |
| Hyve Jade | May 2026 datasheet: 64 x 800GbE OSFP, 51.2 Tbps, Broadcom BCM78900, 2RU, ONIE/SONiC support. No numeric order code printed. |

Wiwynn is attached as brand/platform vendor. Hyve is attached as brand/platform
vendor, and its datasheet footer supplies a separately dated TD SYNNEX parent
association. No manufacturer role flows to the parent. Neither source names a
specific hyperscaler customer or establishes the provenance/configuration of
an auction unit. Jade's FCC Class A statement is not an FCC ID.

## Sources and capture

- [Wiwynn product page](https://www.wiwynn.com/products/21-inch/sv7100g4)
  links the [two-page datasheet](https://www.wiwynn.com/hubfs/Datasheet%20-%20SV7100G4_210917_internal.pdf).
- [Hyve datasheets](https://www.hyvesolutions.com/datasheets) links the
  [Jade datasheet](https://cdn.prod.website-files.com/697d7e6ffbb0348d6224a85d/69f8ec237474bffbedb6ffcd_Hyve%20-%20Jade_RGB_Rev1_050126.pdf).
- Original HTML/PDFs and all retrieval outcomes are in
  `data/raw/odm-models-2026-10-04/`. `fetch-report.json` records hashes and methods.
  Initial Python certificate-store failures remain recorded. System curl with
  its default TLS verification captured the Hyve/Ingrasys HTML and Jade PDF;
  certificate verification was not disabled.
- The older Hyve Orion 2024 PDF appeared in web search text, but a subsequent
  web open returned 404; Python retrieval also failed certificate validation.
  No original Orion PDF or product assertion was imported from this result.

Both Wiwynn pages and the Jade page were rendered and visually inspected.
Poppler warned about a missing Adobe-CNS1 language pack on Wiwynn page 2; the
reviewed English model/specification labels were legible. Embedded text retains
the source's glyph artifacts; no OCR or silent text repair was performed.

## Specification boundaries

- Wiwynn's PDF uses both 1U and 1OU and says four blades in a 1OU sled in its
  blade table. Preserve the wording; do not infer a compatible generic rack.
  Printed blade-table dimensions/weight were not normalized as loose-blade
  shipping specifications. The 130–165 W row is explicitly labelled as a
  blade-table claim, separate from the chassis supply.
- The Wiwynn download table says 2021/09/17, while the PDF footer says copyright
  2015. Neither was treated as the platform's release date.
- Jade's input range and the voltage ranges in its PSU output-power table
  differ. Both are retained as printed. Do not turn PSU rating into measured
  system draw or infer that a liquidated unit includes both supplies/fans.

## Local use

```sh
python3 -m devicehunt search SV7100G4-C --vendor Wiwynn
python3 -m devicehunt specs SV7100G4 --exact-model
python3 -m devicehunt specs SV7100G4-C --exact-model
python3 -m devicehunt search Jade --vendor Hyve
python3 -m devicehunt specs Jade --exact-model
python3 -m devicehunt documents SV7100G4
python3 -m devicehunt documents Jade
python3 -m devicehunt vendor-inventory 'TD SYNNEX' --scope all
```

`data/odm-model-facts-2026-10-04.json` contains the reviewed structured assertions.
The new reference bundle is a complete additive replacement for the older
reference collection, not a two-document partial collection. It retains every
old reference entry and adds the two PDFs. The existing reference importer
replaces its entire collection; never import a partial bundle over the live one.
Original PDF bytes and page text are copied as sibling files under `data/raw/`
because the archive validates sibling-only dependency paths.

## Next bounded leads

Original Ingrasys DOCS HTML was captured, but its product groups are not yet
assertions in the device catalog. The visible grouped server link is
[SV1120/SV1121/SV2121/SV2221](https://www.ingrasys.com/docs-detail/sv1120-sv1121-sv2121-sv2221/).
Other printed leads include GB12481N, GB6181I, GB10181A, GB10281N, GB6181N and
ES2000/ES2100. Review the linked documents and exact roles before splitting
grouped labels or attaching dated Foxconn parent associations. Ingrasys is not
an exact Foxconn alias. The prior original Foxconn relationship release is saved
under `data/raw/odm-primary-2026-10-04/foxconn-ingrasys.html`.

The saved Hyve datasheets HTML also links Orion Rubin, JBOF, Hercules and a
ruggedized edge platform. Those PDFs are not captured in this pass. The current
Hercules card says 112 HDDs; do not overwrite the separately dated 2023 Hercules
9120 96-drive assertion or invent equivalence without model-level evidence.

Continue older liquidation-friendly models, Inventec/Quanta product evidence,
new-vendor NCC/BSMI mappings, and original-vendor FCC/order-code gaps. The full
vendor goal remains active and worldwide historical completeness is unproven.
