# AWS Monitron product relationships backed by saved sources

Reviewed September 27, 2026 using the existing local AWS source collection.
The four-guide bundle validates the original safety-index heading, exact model
link labels, linked PDF URLs and hashes, extracted page associations, and named
company roles. No new web retrieval was needed for this update.

| Model | Product kind | Amazon Data Services role on the guide record |
|---|---|---|
| TE1A001 | Condition-monitoring sensor | Manufacturer |
| TE1A105 | Condition-monitoring sensor | Manufacturer |
| TE1A195 | Ex sensor | Manufacturer through exact index-to-family-guide mapping |
| TG1A001 | Wi-Fi gateway | Compliance declarant |
| TG1A002 | Ethernet gateway | Compliance declarant |

These five existing guide records now carry `AWS / documented_product`
relationships. They identify industrial monitoring products, not data-center
deployment. Gateway declarants are not promoted to manufacturers. The existing
dated service-history records remain separate and do not gain product or
deployment relationships. See [original role and service-notice review](aws-monitron.md)
for the source URLs and limitations.

The older `monitron-model` record used the same original AWS safety-index URL
but had no snapshot attached. Its saved HTML is now linked after verifying
the file against the validated bundle and checking the TE1A001 sensor-guide
link. SHA256: `97ec35f1a1adb8559e1c735f4c29e149e54ae6256fa4724cf17a625e840ab32f`.
The legacy record remains product documentation only; it does not inherit a
manufacturer affiliation from the separate guide record.

```sh
python3 -m devicehunt hyperscaler-inventory --vendor 'Amazon Data Services'
python3 -m devicehunt hyperscaler-inventory --vendor 'Amazon Data Services' --role manufacturer
python3 -m devicehunt hyperscaler-inventory --vendor 'Amazon Data Services' --role compliance_declarant
python3 -m devicehunt hyperscaler-inventory --vendor 'Amazon Data Services' --relationship documented_platform_deployment
```

The first three commands currently yield five, three and two records. The last
returns none. That is not a claim that ADS has no deployed hardware; it describes
what the collected records establish under this strict source-level filter.

The default report now has 17 saved-source assertions across 16 identities.
TE1A001 has both index and guide product evidence; those are separate records,
not two independent votes. Including unverified evidence yields 20 assertions
across 17 identities. Three legacy claims remain uncached, and documented
platform deployment stays at three assertions.

All identity rows, manufacturer/factory inventories and prior assertions remain
intact. Five existing search documents gain product-link text, and five source
metadata rows change. No SKU, alias, unit provenance or current service-access
guarantee is inferred. The source manifest changes only the reviewed curated
file hash; its raw inputs were already saved and hashed.

Audits use the `monitron-product-links-` prefix under `data/exports/`. The importer
regression verifies all five product links, source-specific company roles,
idempotence, no deployment promotion and no relationship on service-history
records. The complete offline rebuild is `data/rebuilt-with-monitron-links.sqlite`.
The owner baseline stays 10/10 searchable and 9/10 corroborated; full historical
coverage and the 7760-32X-A-AC-F corroboration gap remain open.
