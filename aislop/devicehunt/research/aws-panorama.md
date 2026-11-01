# AWS Panorama appliance and service dependency

Reviewed 2026-09-26. **AMHT-1** is now in the local catalog as AWS Panorama
Appliance (Hub), with three assertions from two primary AWS sources.

## Identity and company roles

The original eleven-page [AWS safety and compliance manual](https://d1.awsstatic.com/product-marketing/panorama/1_MHT_Safety_ComplianceManual_ENGLISHv3.pdf)
is saved as `data/raw/aws-panorama-safety.pdf`. Its SHA-256 is
`d8c04a0bd82a912c925c9e1a66c5404e1529a286115bf62dac6de52830559293`.

| Page | Evidence imported | Role |
|---|---|---|
| 3 | Device name AWS Panorama Appliance (Hub), model AMHT-1; Amazon Development Center US, Inc. is responsible for FCC compliance | `fcc_responsible_party` |
| 10 | UK declaration CC SVC 00233019 2022 TR names model AMHT-1, Amazon Data Services, Inc. as manufacturer, and AWS as brand | `manufacturer`, `brand` |

Amazon Data Services and Amazon Development Center US remain distinct companies.
AWS's page footer is not manufacturer evidence. The named manufacturer is the
legal manufacturer in the declaration; a physical factory is not established.
`devices.brand` uses the Amazon Data Services namespace consistently with the
other AWS hardware records; the explicit brand affiliation is AWS.

Pages 3, 8, 9, 10 and 11 were rendered and visually reviewed. The embedded text
on pages 8, 9 and 11 omits their declaration images. These pages remain in the
original PDF but are not parsed, and no OCR or signature/date extraction is
claimed. The EU declaration on page 8 visually supports the same model and
manufacturer. The machine-imported manufacturer evidence comes from page 10.
The PDF text extractor reported malformed object pointers but extracted the
reviewed identity pages correctly; page images were checked against that text.

Page 3 refers readers to an FCC ID on the physical product but does not print
the ID in its identity block. No FCC ID is invented. Page 7 describes an Anatel
approval for an incorporated product; it is not promoted to a chassis approval.

The old [AWS safety index](https://aws.amazon.com/panorama/terms/safety-guide/)
returned a generic AWS machine-learning page with no manual link when downloaded.
That HTML is retained as `aws-panorama-safety-index.html` for the coverage record,
but is not used as model evidence. The original manual's direct public URL
remained retrievable and provides the explicit appliance identity itself.

## Buying context: service ended

AWS's [end-of-support notice](https://docs.aws.amazon.com/panorama/latest/dev/panorama-end-of-support.html)
states that new-customer access ended May 20, 2025 and service support ended
May 31, 2026. Its FAQ says the appliance and applications depend on the Panorama
cloud service and will not continue to function after that service ends.
These are dated AWS statements, not a hands-on test of a surplus unit.

The same notice identifies the appliance's platform as Nvidia Jetson Xavier.
This is retained as a sourced platform statement, not a claim of generic Jetson
board equivalence, unlockability or alternative-firmware support. A used
AMHT-1 should not be presented by this catalog as a working Panorama-service
purchase. Whether someone can repurpose the hardware remains unverified.

The service notice is a separate source, with only an AWS documentation-vendor
role. Its appliance-to-AMHT-1 association cites safety-guide page 3. It does not
create manufacturer evidence, an FCC grantee, a hyperscaler deployment claim,
or an entitlement to transfer warranty or service access.

## Local queries

```sh
python3 -m devicehunt import aws-panorama data/raw/aws-panorama-bundle.json
python3 -m devicehunt search AMHT-1 --vendor 'Amazon Data Services' --role manufacturer
python3 -m devicehunt match-listing --title 'AWS Panorama AMHT-1 appliance'
python3 -m devicehunt sql "SELECT model,name,value,source_url FROM device_facts WHERE model='AMHT-1' AND (name LIKE 'service_%' OR name='compute_platform_description')"
```

`data/exports/panorama-device.json` contains the complete device evidence;
`panorama-service-facts.csv` provides service and platform context, and
`panorama-listing-example.json` matches an illustrative title, not a live offer.
`show` and the offline catalog expose the facts and original source links.
The full embedded text is also searchable through the
[reference archive](reference-document-archive.md):
`python3 -m devicehunt documents AMHT-1 --source reference-pdf-aws-panorama`.
The image content on pages 8, 9 and 11 still requires visual review.

The importer checks PDF magic, dependency hashes, text/PDF association,
consecutive pages, exact identity and manufacturer blocks, the distinct FCC
responsible party, and the dated service/cloud-dependency paragraphs before
replacing either source. Tests cover changed entities/models/brand, changed
dates, missing or duplicate notices, altered platform text and damaged
dependencies, as well as idempotent import and absence of inferred aliases,
FCC IDs or deployment claims.
