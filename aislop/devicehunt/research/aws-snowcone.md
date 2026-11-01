# AWS Snowcone declarations

The original public [Italian Snowcone safety guide](https://d1.awsstatic.com/Digital%20Marketing/House/Hero/products/Snow/16%20Compliance%20Notice_Snowcone_EU_Italian_it-IT_locked.pdf)
includes English declarations on pages 7–10. Their identity pages name:

| Printed product scope | Object | Manufacturer | Brand | Page |
|---|---|---|---|---:|
| AF1 | Data storage device | Amazon Data Services, Inc. | AWS | 7 |
| AF1 BD1 | Dongle | Amazon Data Services, Inc. | AWS | 9 |

These are two separate catalog identities, with the second field preserved
exactly as printed, including its space. The importer does not split it into
AF1 and BD1, equate it with the main appliance, or create spelling variants.
The legal manufacturer is explicit; a physical factory, operator deployment,
current certificate validity and firmware access are not established.

The guide's first page identifies AWS Snowcone and the AF1 radio-equipment type.
This supplies the family association independently of filenames. The word
Antfarm in other download filenames is not promoted to a model alias. No FCC
ID is assigned from the declarations, and no Snowball model is inferred from
the family guide.

The PDF opened normally in the readers used here. Although its metadata reports
encryption, no password was requested or supplied. The text extractor reported
malformed object-pointer warnings; pages 7, 8 and 9 were rendered and visually
checked against the extracted identity fields. Some visible declaration numbers
and signature/date content are absent from embedded text. These are not
machine-imported as dates or certificate identifiers. The original PDF and its
complete embedded-text capture are retained with hashes.

## Dated family context

The cached [AWS Snow safety index](https://aws.amazon.com/snow/safety-and-compliance-guide/)
links to the [September 2022 family guide](https://d1.awsstatic.com/English-Guide-for-Snowball-and-Snowcone_Final_Sep2022.pdf).
Page 3 states that Snow products must be returned to AWS, and that customers
do not obtain title or permanent possession. The catalog records this dated
source statement for AF1, with an explicit limit: it does not determine rights,
ownership, provenance or contractual treatment of any particular auction unit.
It is not extended to the separately named dongle.

The 2022 guide is not the old declaration PDF. The importer verifies the current
cached index's exact EU/English link to the family guide, and independently
verifies the declaration fields in the older PDF. It does not claim that the
current index links the older Italian guide. No request was sent to AWS for
additional declarations.

## Local use

```sh
python3 -m devicehunt search Snowcone --vendor 'Amazon Data Services' --role manufacturer
python3 -m devicehunt sql "SELECT model,name,value,source_url FROM device_facts WHERE model IN ('AF1','AF1 BD1') ORDER BY model,name"
python3 -m devicehunt documents AF1 --source reference-pdf-aws-snowcone-it
python3 -m devicehunt match-listing --title 'AWS Snowcone AF1' --vendor 'Amazon Data Services' --include-short-models
python3 -m devicehunt import aws-snowcone data/raw/aws-snowcone-bundle.json
```

The short-name option is required for AF1 in the auction matcher; ordinary SQL,
catalog and document searches already accept it. Three-character letter/digit
names are otherwise excluded to reduce incidental matches. The option retains
normal identity boundaries and marks suffix variants as leads. It does not
make a mention into identification of the item being sold.

`data/exports/snowcone-models.json` contains the reviewed model records,
`snowcone-facts.csv` includes the dated context, and `snowcone-listing-example.json`
matches an illustrative title, not a live auction. Both PDFs are also in the
full-text reference archive. The importer validates every dependency, the exact
declaration scopes, manufacturer/brand/object fields and family-context mapping
before replacing either source.
