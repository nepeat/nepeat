# AWS Monitron hardware evidence

The five guide records now also expose explicit AWS product relationships in
`hyperscaler-inventory`, without asserting data-center deployment. The older
TE1A001 index claim is backed by its saved original HTML. See the
[product-link update](monitron-product-links.md) for current report counts.

The cached [AWS safety index](https://aws.amazon.com/monitron/terms/safety-and-compliance-information/)
links guides for five exact models. The importer combines those links with
visually reviewed PDF identity/compliance pages. Original PDFs and complete
embedded-text captures are local; the bundle and every dependency are hashed.
This adds two gateway identities and strengthens three existing ISED sensor
identities. It does not enumerate all Amazon hardware.

| Model | Device | Amazon Data Services relationship | Evidence |
|---|---|---|---|
| TE1A001 | Sensor | Manufacturer | Sensor guide page 2 |
| TE1A105 | Sensor | Manufacturer | Sensor guide page 2 |
| TE1A195 | Ex sensor | Manufacturer through explicit guide mapping | Exact AWS index label links to Ex family guide; page 1 names manufacturer |
| TG1A001 | Wi-Fi gateway | Compliance declarant | EU guide page 2 names this exact model |
| TG1A002 | Ethernet gateway | Compliance declarant | EU guide page 2 names this exact model |

The Ex guide's printed model is `TE1Ax9z`, explicitly a configurable family.
Only the index's exact `TE1A195` label becomes a device identity. The family is
preserved as `source_model_field`, not an alias, and other possible variants
are not generated. The two ordinary sensors share printed FCC and Canadian IDs;
they remain separate models. Sensor manufacturer means the named entity in the
guide, not an independently verified physical factory.

The gateway guides make declarations; this importer does not promote those
statements to manufacturer claims. AWS's Ethernet guide also contains a Wi-Fi
frequency paragraph; it is not imported as verified Ethernet-model radio
specifications. Copyright owners and page footers do not establish factories.
The downloaded US gateway guides were reviewed but are not used by the importer.

The [AWS documentation history](https://docs.aws.amazon.com/Monitron/latest/user-guide/doc-history.html)
contains an October 31, 2024 notice that new-customer access closed while existing
customers could continue. This dated service context is attached to the five
models as a separate source with only an AWS documentation-vendor role. It does
not assert that a surplus unit can enroll, transfer between accounts or receive
warranty coverage. None of the new records asserts hyperscaler deployment.

```sh
python3 -m devicehunt search Monitron --vendor 'Amazon Data Services'
python3 -m devicehunt search Monitron --vendor 'Amazon Data Services' --role manufacturer
python3 -m devicehunt search Monitron --vendor 'Amazon Data Services' --role compliance_declarant
python3 -m devicehunt sql "SELECT model,name,value,source_url FROM device_facts WHERE name LIKE 'service_%' AND model IN ('TE1A001','TE1A105','TE1A195','TG1A001','TG1A002')"
python3 -m devicehunt import aws-monitron data/raw/aws-monitron-bundle.json
```

`data/exports/monitron-roles.csv` and `monitron-service-notices.csv` provide local
query examples. `show` exposes the guide page, original PDF URL, exact index
label and hashes. Shared certification IDs never collapse the sensor identities.

The importer validates all dependencies before replacing any source, including
exact link labels, PDF-source hashes, consecutive text pages, manufacturer or
declarant wording, and the dated history row. Tests reject swapped model links,
changed company names, wrong gateway identifiers, absent family qualifiers,
future-tense notices, duplicate notices and malformed dependencies. Refreshes
replace the same five sources without duplicating identities.

The Ex guide has malformed embedded glyphs in parts of its multilingual text.
The identity block was visually checked; no attempt was made to repair or infer
other specifications from the garbled text. These AWS PDFs are evidence inputs
for this importer. The full AWS page captures, plus both cached US gateway
guides, are now searchable with `documents` through the separate
[reference archive](reference-document-archive.md). Page-text matches do not
expand the reviewed identity assertions above.
