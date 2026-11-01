# Cisco models with Celestica factory evidence

Cisco's [published CB certificate archive](https://www.cisco.com/web/dofc/21435998.pdf)
contains two August 31, 2023 M3 certificates covering NCS1004 and NCS1014. Each
names Cisco Systems Inc as applicant and manufacturer, and Celestica (Thailand)
Limited and Fabrinet Co Ltd as factories. The Cisco trademark is visible on
both certificate cover pages. All four imported pages were visually reviewed.

This adds two **Cisco-branded identities** associated with Celestica through
`listed_factory`. It does not relabel the models as Celestica products, prove
which factory built an auction unit, or establish hyperscaler deployment.
The source does not allocate individual units or configurations among factories.

| Certificate | Original PDF pages | Model | Rating stated in that certificate |
|---|---|---|---|
| US-37447-M3-UL | 1–2 | NCS1004 | -48/-60 Vdc, 44A MAX (2X) |
| US-37447-M3-UL | 1–2 | NCS1014 | -48V/-60V, 60A MAX (2X) |
| US-37446-M3-UL | 3–4 | NCS1004 | 100–127 Vac, 15A MAX (2X); 200–240 Vac, 12A MAX (2X); 50/60Hz |
| US-37446-M3-UL | 3–4 | NCS1014 | 100–127V~, 16A MAX (2X); 200–240V~, 14A MAX (2X); 50/60Hz |

These are source-stated input ratings for different certificate configurations,
not measurements, actual consumption, or a claim that any installed PSU accepts
all listed inputs. Raw extracted text and per-model assignment remain available.
The original issue date is January 28, 2021. No current validity is asserted.

The model field permits additional alphanumeric suffixes or a blank suffix.
Only the two explicitly named base models are imported. No wildcard expansion,
ordering-code decoding, inferred alias, or PSU SKU is generated from that rule.
The archive also includes an older 2021 certificate on pages 5–6; it remains in
the original PDF/text cache but is outside this selected import.

## Local search

```sh
python3 -m devicehunt search --vendor Celestica --role listed_factory
python3 -m devicehunt specs NCS1014 --exact-model --vendor Celestica
python3 -m devicehunt match-listing --title 'Cisco NCS1004 chassis' --vendor Celestica --format json
python3 -m devicehunt import cb-certificates data/raw/cisco-ncs-cb-bundle.json
```

```sql
SELECT d.brand, d.model, a.company, a.role, r.source_url
FROM affiliations a JOIN records r ON r.id = a.record_id
JOIN devices d ON d.id = r.device_id
WHERE r.source_id = 'cisco-ncs-cb-certificates'
ORDER BY d.model, r.source_key, a.role, a.company;
```

Local archive: `data/raw/cisco-ncs-cb-certificates.pdf`, with embedded page text
and a checked dependency bundle. `data/exports/cisco-celestica-cb-facts.csv`
contains the imported facts. This source is published by the hardware vendor;
the certificates identify UL Solutions as the issuing certification body.

The bounded importer checks document hashes, PDF/text association, page count,
certificate/continuation pairing, date, company roles, explicit model syntax,
and one rating per model. All validation precedes source replacement. It uses
only the standard library to replay the checked text capture; pypdf was used
to create the capture and the original rendered pages were reviewed separately.

## Additional test-report lead, September 26, 2026

A public search lead pointed to the CIUS report at
`https://fccid.io/VGBCSCO4G710/Test-Report/Test-Report-DSS-1589048.pdf`.
The web tool returned a cache miss; a normal direct download returned HTTP 403.
Requests stopped without alternate access. No report text, model, manufacturer
or factory assertion from that lead was imported. A separate Extreme Altitude
report lead (`https://fccid.io/m/f37184ef916570d49cc7e738fa39ce4372bac8268aa4ec317e471bcfe5a11996.pdf`)
was inaccessible through the web tool and also remains unimported.
