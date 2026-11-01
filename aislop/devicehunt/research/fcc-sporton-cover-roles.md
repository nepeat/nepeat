# Applicant and manufacturer fields in cached FCC report covers

Seven reviewed Sporton report-cover excerpts in the previously archived FCCID.co
HTML now supply ten model assertions. The new importer checks the cover's
explicit applicant, manufacturer, brand and model fields independently. These
are statements in mirror text, not original-PDF verification or factory audits.
No additional report PDF was downloaded during this work.

## Findings retained

| FCC ID | Complete model field | Manufacturer as printed |
|---|---|---|
| HED2555WAG2 | SMC2555W-AG2 | Accton Technology Corporation |
| HED7822GMN | EW-7822GMn | Edimax Technology co., Ltd |
| HEDECG9210 | ECG9210-04 | Joy Technology(ShenZhen)Co.,Ltd. |
| HEDOAP3211A | SU-A-MB-12-EZ | Accton Technology Corporation |
| HEDOAP6626A | WA6202AM, OAP6626A, SMC2890W-AG, BSAP-1600 | Accton Technology Corporation |
| YZKECW7210L | ECW7210-L | Accton Technology Corporation |
| YZKECW7211L | ECW7211-L | Accton Networks Corporation |

Seven model assertions explicitly name the recognized Accton Technology entity
as manufacturer. Three retain unassigned manufacturer names. In particular,
`Accton Networks Corporation` is not silently treated as Accton Technology
Corporation. Edimax and Joy Technology remain their own named manufacturers;
neither inherits a target-company relationship from the applicant or brand.

Each FCC ID links to its original cached source page, for example
[HED7822GMN](https://fccid.co/fcc/HED7822GMN),
[HEDECG9210](https://fccid.co/fcc/HEDECG9210), and
[YZKECW7210L](https://fccid.co/fcc/YZKECW7210L).
The literal applicant supplies the catalog identity namespace. That namespace
does not prove the applicant manufactured or branded the product. Comma-separated
models are separate identities; multiple brand names remain one uninterpreted
source field and are never paired with model names by position.

The three new catalog identities are Accton-namespace ECG9210-04 and
SU-A-MB-12-EZ, and Edge-Core-namespace ECW7211-L. Other rows add evidence to
existing identities. The manufacturer inventory increases by five target/identity
pairs because some already-known identities gain qualifying manufacturer claims.

## Validation and scope

The accepted cover must occur within the first 1,800 excerpt characters, name
Sporton, and contain the reviewed ordered fields. FCC ID and applicant must agree
with the filing/grantee. Simple model tokens may be comma-separated; whitespace
groups, slash-separated fields and mixed model/brand layouts remain excluded.
See `data/exports/fcc-sporton-exclusions.json` for the three excluded excerpts.

The capture records exact extracted values, excerpt ordinal/hash and original
HTML hash. Every entry is checked before source replacement; changed fields,
wrong categories, wrong identities, duplicate excerpt captures and escaping
paths are rejected. Non-ASCII letters in company names cannot disappear through
ASCII alias normalization to create a false target relationship.

Excerpts may be truncated. Their order does not establish document IDs or page
numbers. Counts are per accepted excerpt/model, not proof of distinct original
reports or independent corroboration. Existing applicant-only report-cover
extraction is preserved as a separate source; the new source does not upgrade its
claims. No grant validity, operator deployment or surplus-unit history is inferred.

```sh
python3 -m devicehunt import fcc-sporton-covers data/raw/fcc-sporton-covers.json
python3 -m devicehunt search ECW7210-L --vendor Accton --role manufacturer
python3 -m devicehunt lookup EW-7822GMn
```

```sql
SELECT d.model, a.company, a.target, a.role, r.source_url
FROM records r JOIN devices d ON d.id = r.device_id
JOIN affiliations a ON a.record_id = r.id
WHERE r.source_id = 'fcc-sporton-cover-roles'
ORDER BY d.model, a.role;
```

Five focused tests cover role separation, manufacturer names outside the target
list, grouped fields, ambiguity, source association, failed-refresh preservation,
path boundaries and stale-model cleanup. All 335 Python tests pass. Source-role
exports, exclusions, preservation checks and import counts are saved under
`data/exports/fcc-sporton-*`.

The fresh `data/rebuilt-with-fcc-sporton.sqlite` matches the live catalog across
37 tables/search indexes and the owned-hardware baseline, using 89 import jobs
and 1,920 hashed files. Previously stored device evidence is unchanged. The
generated offline page also passes 18 comparisons against SQL inventory result
sets in the inline-script harness; this is behavior verification, not browser
visual QA. Full FCC coverage remains unproven and no index totals were expanded.
