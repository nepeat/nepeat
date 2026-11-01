# Searchable Taiwan company relationships

The `taiwan-targets` importer now adds **295 regulatory assertions** from three
published registers. The complete raw tables remain separate. The current catalog
contains 2,196 identities and 2,881 assertions. The company mapping, source
snapshots and dependency hashes are in `data/taiwan-targets-bundle.json`.

The latest Edgecore mapping adds 26 net assertions and 17 catalog identities;
23 more identities become associated with Edgecore when previously imported
records are also considered. These are identity/association counts, not counts
of distinct physical devices or proven factories.

## Company names and roles

[Oracle's Taiwan offices page](https://www.oracle.com/tw/corporate/contact/field-offices/)
explicitly pairs 美商甲骨文有限公司台灣分公司 with Oracle Taiwan LLC, Taiwan Branch.
[Accton's June 2026 announcement](https://www.accton.com/tw/accton-jun-2026-sales-revenue-report/)
pairs 智邦科技股份有限公司 with Accton. Both original HTML pages are cached under
`data/raw/taiwan-companies/`. The importer checks the reviewed name association
within the relevant company page structure, after checking its hash.

The [official Hsinchu Science Park company record](https://www.sipa.gov.tw/home.jsp?contlink=ap%2Fmanufacturers_1_3_view.jsp&dataserno=A0902&level2=Y&menudata=ChineseMenu&mserno=201002010011&qsipa=01&serno=201002010011&stype=C)
explicitly pairs 鈺登科技股份有限公司 with Edgecore Networks Corporation, under
company record A0902 and business number 25063543. Its original HTML is cached as
`data/raw/taiwan-companies/edgecore-sipa.html`. The parser requires the exact Chinese
name, English name, record number and business number in the same table. It cannot
join names from different company tables or take them from a script.

These three exact Chinese strings are mapped only within this importer. No global
company alias list changes. Substrings, subsidiary suffixes and multi-company
cells are not included by approximation. The canonical target affiliation and
its role are accompanied by raw registry values, the mapping source URL and the
dependency hashes on each record.

| Snapshot | Selected rows | Mapped roles |
|---|---:|---|
| BSMI | 42 | Oracle applicant: 41; Accton applicant: 1 |
| NCC | 252 | Oracle applicant: 6; Accton applicant: 184; Accton registry manufacturer: 89; Edgecore applicant: 38; Edgecore registry manufacturer: 1 |
| BSMI product certificates (162936) | 1 | Edgecore applicant: 1 |

Several NCC rows contain Accton in both roles; role counts cannot be summed as
distinct devices. BSMI has no brand field, so the catalog's Oracle/Accton/Edge-Core label
there is an applicant namespace. NCC keeps the source's printed brand, including
Aruba or Intel. A registry manufacturer claim is not independent verification of
the physical factory, and an applicant association is not a manufacturer claim.

## Local searches

```sh
python3 -m devicehunt search WH1CL --vendor Oracle --role regulatory_applicant
python3 -m devicehunt search AP-52-W --vendor Accton --role registry_manufacturer
python3 -m devicehunt search 9260NGW --vendor Oracle --role regulatory_applicant
python3 -m devicehunt search ECW3142-N --vendor Edge-Core --role registry_manufacturer
python3 -m devicehunt search ZAP5220 --vendor Edge-Core --role regulatory_applicant
python3 -m devicehunt import taiwan-targets data/taiwan-targets-bundle.json
```

`9260NGW` retains its **Intel** brand even though Oracle's Taiwan branch is the
NCC applicant. `AP-52-W` retains **ARUBA WIRELESS NETWORKS** as its printed brand
while Accton is the named NCC manufacturer. Neither is relabelled as an Oracle
or Accton retail product.

```sql
SELECT d.brand, d.model, a.company, a.role, r.source_url
FROM records r JOIN devices d ON d.id=r.device_id
JOIN affiliations a ON a.record_id=r.id
WHERE r.source_id IN ('bsmi-reviewed-targets', 'ncc-reviewed-targets', 'bsmi-product-reviewed-targets')
  AND a.target='Accton' AND a.role='registry_manufacturer'
ORDER BY d.brand,d.model;
```

Whole annotated model strings, order suffixes, raw expiry/status fields and the
source row number remain attached. For example, an Ampere-annotated `E2ES2U`
entry does not create an unannotated model alias. Blank/confidential model cells
are not promoted to catalog identities. No operator or hyperscaler association,
present certification validity, firmware availability or hardware support is
inferred. Raw registry tables are not altered by this importer.

See `data/exports/taiwan-target-roles.csv` or its JSON counterpart for all role
assertions; each source record may produce several rows. The catalog, ordinary
vendor searches, auction-title matcher and target exports now include these
identities. Cross-source identities still use the existing exact brand/model rule.

## Verification

All six dependencies and three complete registry summaries are validated before
replacing any target source. Version 1 bundles retain the original Accton/Oracle
scope; version 2 adds Edgecore government-name evidence and the additional BSMI
register. Six importer tests cover role separation, exact-name matching, unsplit
models, source hashes, idempotency, no global aliases and bilingual evidence that
cannot be combined across unrelated tables. The full suite passes **259 tests**.
The ten-device baseline remains 10/10 searchable and 8/10 corroborated.

## Edgecore associations: scope matters

The 39 selected rows containing the Edgecore mapping comprise 38 NCC rows and
one BSMI product row. One NCC row names Edgecore as both applicant and manufacturer.
Thirteen NCC rows were already selected through another target role, so the
expansion adds 25 NCC assertions and one BSMI assertion, not 39 new records.

Only **ECW3142-N** adds a new Edgecore manufacturer claim (NCC certificate
CCAO17LP0620T1). This is still a registry claim, not an independent factory audit.
The manufacturer inventory grows from 462 to 463 identities; Edgecore grows from
118 to 119. Other newly linked models have applicant relationships, including
SMC-labelled equipment and ZPE Systems, Inc. ZAP5220. Their printed brands and
separate manufacturer fields remain intact. BSMI's RD1201000-C47-2MG uses the
Edge-Core applicant namespace because that register has no brand column; it is
excluded from the manufacturer inventory.

The full mapping and identity delta are in
[`taiwan-edgecore-mapping-audit.json`](../data/exports/taiwan-edgecore-mapping-audit.json).
The three earlier BSMI supplementary-register scans used a stated seven-term name
list that did not include 鈺登科技股份有限公司. Their zero results remain scoped
to those terms; this newly verified legal name produces one match in dataset 162936.
No extra match occurs in the voluntary certificate or declaration registers.
