# AWS Outposts model evidence

Reviewed and cached 2026-09-26. Imported through `data/aws-outposts.json`.

| Model as published | Product section linking its declaration | Declared manufacturer |
|---|---|---|
| SCOUT01 | AWS Outposts 2U Server | Amazon Data Services, Inc. |
| SCOUT02 | AWS Outposts 2U Server | Amazon Data Services, Inc. |
| CubScout | AWS Outposts 1U Server | Amazon Data Services, Inc. |

The [AWS compliance index](https://aws.amazon.com/outposts/safety-and-compliance-guide/)
links its 2U English entry to the
[SCOUT declaration](https://d1.awsstatic.com/product-marketing/Outposts/safety-compliance/CE%20Declaration%20of%20Conformity_Scout_Final-English.pdf),
and its 1U English entry to the
[CubScout declaration](https://d1.awsstatic.com/product-marketing/Outposts/safety-compliance/CE%20Declaration%20of%20Conformity_CubScout_Final-en_us.pdf).
Page 1 of each PDF names the models, brand AWS, rack mounted server type and
Amazon Data Services legal entity. Page 2 has a blank signature and a placeholder
date, so these are published identity statements, not evidence of a completed
signed declaration or current certification validity.

Three vendor-document model identities and six assertions are stored: the PDF
manufacturer claims and the HTML product-family mappings have separate source
records. The latter carry `documented_product_family` relationships for the
hyperscaler filter. No factory/OEM, deployment or previous unit owner is inferred.
No CPU, power rating, firmware availability or used-unit service eligibility is
assigned from these documents.

The AWS brand namespace remains separate from the Amazon Data Services registry
namespace. In particular SCOUT01/AWS and SCOUT02/AWS retain their complete register
strings; no silent alias or cross-brand merge is introduced. CubScout also has
an existing FCC/mirror identity; these entries may describe overlapping hardware
and should not be counted as distinct physical designs.

## Local queries

```sh
python3 -m devicehunt search SCOUT --vendor 'Amazon Data Services' --hyperscaler
python3 -m devicehunt search CubScout --vendor 'Amazon Data Services'
python3 -m devicehunt import curated data/aws-outposts.json
```

```sql
SELECT d.model, h.relationship, h.detail, r.source_url
FROM devices d JOIN records r ON r.device_id=d.id
JOIN hyperscaler_evidence h ON h.record_id=r.id
WHERE r.source_id='aws-outposts-compliance';
```

Raw HTML and PDFs are under `data/raw/aws-*`; all four inputs, including the
reviewed JSON, are hash-checked by `data/import-manifest.json` during rebuild.
