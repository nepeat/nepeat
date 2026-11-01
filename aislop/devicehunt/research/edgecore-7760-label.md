# Edge-Core 7760 label evidence

The user supplied a chassis-label photograph on 2026-09-27 and attributed it to
[eBay item 389108536363](https://www.ebay.com/itm/389108536363). Visual review
of the supplied image reads:

| Printed field | Transcription |
|---|---|
| Brand/logo | Edge-corE |
| Model No. | 7760-32X-A-AC-F |
| Supplier Sku | AS7760-32X |
| Adjacent compliance label model | AS7760-32X |
| F/W | V2015.02.00.30 |
| H/W | R011 |
| Rev. | R20 |
| Adjacent compliance label Input Voltage Range | 100-240~, 50 - 60Hz, 6A Max |

The model number and supplier SKU are printed together on the same label. This
directly supports the mapping for the pictured configuration; no prefix or
suffix was stripped. The exact original photo is saved in
`data/raw/edgecore-7760-label.png`. Its checksum, transcription and provenance
are preserved by `data/label-evidence.json` and the rebuild manifest. Serial and
MAC identifiers are not copied into searchable fields.

The input marking was reviewed and imported in the 2026-10-04
[buying-facts batch](owned-buying-import-batch-2026-10-04.md). Its complete versioned
input is [label-evidence-with-input-marking-2026-10-04.json](../data/label-evidence-with-input-marking-2026-10-04.json);
the original JSON and image remain intact. The two new facts preserve the literal
marking and its location on the adjacent AS7760-32X label. This is a photographed
marking, not measured draw, a PSU inventory or a model-wide electrical requirement.

The source has the explicit tier `physical_label_photo`, separate from the
listing's text claims. The exact match between the supplied image and the live
listing gallery was not independently verified. The brand affiliation is
Edge-Core; no physical-manufacturer, factory or AWS deployment claim is added.
Firmware, hardware and revision values describe the photographed unit only.
They do not establish current firmware or every unit's configuration.

The new record joins the existing exact full-code identity. `AS7760-32X` is a
source-scoped part-number identifier and a `supplier_sku` fact on that record;
existing AS7760 family records stay separate. No universal interchangeability,
cross-brand merge or automatic family-wide reverse alias is inferred.

```sh
python3 -m devicehunt lookup 7760-32X-A-AC-F --format json
python3 -m devicehunt sql "SELECT d.model, i.kind, i.value, r.source_url, s.tier FROM identifiers i JOIN records r ON r.id=i.record_id JOIN devices d ON d.id=r.device_id JOIN sources s ON s.id=r.source_id WHERE r.source_id='edgecore-7760-label'"
python3 -m devicehunt validate-inventory --inventory validation/owned-hardware.json
```

Validation now accepts reviewed, locally saved physical-label photos as identity
evidence with their expected brand on the same record. Seller text and owner
assertions remain insufficient. The report keeps photo provenance and separate
document/photo counts: **9 document snapshot hits, 1 label-photo-only hit,
0 unresolved identity gaps**. This resolves the owned identifier question;
document-only scraped coverage for the full 7760 ordering code remains a gap.

Tests confirm that the source must be saved, seller-only text still fails, and
a brand-only photograph adds neither manufacturer-inventory nor hyperscaler
deployment evidence. The baseline fixture itself is unchanged.
