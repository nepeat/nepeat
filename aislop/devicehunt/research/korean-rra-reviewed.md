# Selected Korean approval mirror evidence

Follow-up: original RRA detail pages now corroborate the two Celestica models.
The official certificate spelling and country field differ from the mirror and
remain separate source claims. See [official query evidence](korean-rra-official.md).
Counts and outstanding checks below describe the earlier mirror-only milestone.

Reviewed 2026-09-26. The public English
[RR-ZZR-CS8000-32X-DC detail table](https://fccid.io/RR-ZZR-CS8000-32X-DC)
explicitly identifies:

| Field | Captured value |
|---|---|
| Main model | CS8000-32X-DC-11 |
| Derived model | CS8001-32X-DC-11 |
| Applicant and manufacturer | CELESTICA TECHNOLOGY CONSULTANCY (SHANGHAI) CO LTD |
| Product | Switch |
| Country field | China (Mexico, Thailand) |
| Displayed date | 2020-05-19 |

Both models already existed in the catalog. This adds two manufacturer/applicant
assertions from one additional source; it does not add two new identities.
Celestica's manufacturer inventory remains 40 identities, now with 50 source
records across six sources. The total catalog contains 2,196 identities and
2,883 assertions across 167 sources.

## Provenance and limits

`data/raw/rra-reviewed/celestica-cs8000.json` is a selected factual web-tool table
extract, not original HTML, an original RRA export, or a certificate PDF. The
English table was successfully retrieved before a direct HTML request returned
HTTP 403. The download was stopped without an alternate access attempt. The
Korean link returned a web cache miss; no authoritative Korean text was reviewed.
Original RRA corroboration remains outstanding. The source is explicitly tiered
`regulatory_mirror` throughout the local catalog and exports.

The manufacturer and applicant fields remain separate affiliations. The country
cell stays intact; it is not split into named factories or assigned per model.
The displayed date stays a raw mirror claim. Celestica is the catalog namespace
for this manufacturer association, not an assertion of a printed retail brand.
No AWS use, unit provenance, current approval validity or software support is
established.

The main and derived models are separate identities with a `rra_model_role`
fact. Neither is an alias of the other. The certificate uses identifier kind
`rra_certificate_id`, never `fcc_id`, and its shortened certificate suffix is
not imported as a model. A search for the main model can also find the derived
record through its explicit `rra_main_model` relationship fact; this is a text
search result, not identity equivalence.

## Local use

```sh
python3 -m devicehunt import rra-reviewed data/raw/rra-reviewed/bundle.json
python3 -m devicehunt lookup RR-ZZR-CS8000-32X-DC
python3 -m devicehunt search CS8001-32X-DC-11 --vendor Celestica --role registry_manufacturer
python3 -m devicehunt sql "SELECT d.model,f.name,f.value FROM records r JOIN devices d ON d.id=r.device_id JOIN facts f ON f.record_id=r.id WHERE r.source_id='rra-reviewed-rr-zzr-cs8000-32x-dc' ORDER BY d.model,f.name"
```

The importer validates all dependency hashes and captured fields before replacing
any source records. An explicit derived-model list must agree with its captured
cell; ambiguous model cells, duplicate models/certificates and invalid dates are
rejected. Three focused tests cover those boundaries, source role isolation,
reimport and refresh. The full 262-test suite passes.

This selected record does not establish coverage of the Korean approval database.
The owned baseline remains 10/10 searchable and 8/10 corroborated; neither open
Edge-Core exact-label gap is closed by these Celestica records.
