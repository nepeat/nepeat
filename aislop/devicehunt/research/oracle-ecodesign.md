# Oracle compliance-code relationships

Four original Oracle EcoDesign PDFs now supply named legal-manufacturer evidence
and qualified product/compliance-code relationships. Page 1 of each PDF was
rendered and visually checked against the extracted table fields. The complete
PDFs and all ten pages of embedded text are archived locally.

| Oracle source | Compliance code | Product scope | PSU rated output |
|---|---|---|---:|
| [X9-2](https://www.oracle.com/a/otn/docs/WH1CL_X9-2_EcoDesignReportingInfo.pdf) | WH1CL | X9-2 Server, all models as defined in the DoC | 1200 W |
| [X9-2L](https://www.oracle.com/a/otn/docs/WH2CL_X9-2L_EcoDesignReportingInfo.pdf) | WH2CL | X9-2L Server, all models as defined in the DoC | 1200 W |
| [X10M](https://www.oracle.com/a/otn/docs/x10m_2u_zu2xl_ecodesignreportinginfo.pdf) | ZU2XL | X10M Server, all models as defined in the DoC | 1400 W |
| [X11M](https://www.oracle.com/a/otn/docs/X11M_2U_ZU2XL_EcoDesignReportingInfo.pdf) | ZU2XL | X11M Server, all models as defined in the DoC | 1400 W |

Each table names **Oracle America Inc.** as manufacturer. That is the named legal
manufacturer; it does not independently identify the physical factory or ODM.
The printed manufacturer value, including its trailing ellipsis, is preserved
alongside the normalized company name. The X11M PDF carries a confidentiality
marking despite being served at this public Oracle URL; the archive preserves
that marking without making a broader distribution-rights claim.

`ZU2XL` is explicitly associated with two products. It therefore cannot identify
X10M versus X11M on its own. These relationships remain separate source records,
not aliases or merged marketing-model identities. The qualified product scope is
retained; the referenced DoC's complete variant list has not been imported here.

## Search and use

```sh
python3 -m devicehunt search WH1CL --vendor Oracle --role manufacturer
python3 -m devicehunt lookup ZU2XL
python3 -m devicehunt specs ZU2XL --exact-model
python3 -m devicehunt import oracle-ecodesign data/raw/oracle-ecodesign-bundle.json
```

```sql
SELECT d.model AS compliance_code, f.value AS product_scope, r.source_url
FROM records r JOIN devices d ON d.id=r.device_id
JOIN facts f ON f.record_id=r.id
WHERE r.source_id GLOB 'oracle-ecodesign-*' AND f.name='product_scope'
ORDER BY d.model,r.source_id;
```

The archive makes every page searchable, while the structured importer is limited
to reviewed page 1 fields. PSU rated output is not measured wall consumption or
whole-server maximum consumption. Configuration-specific idle measurements remain
in the document archive rather than becoming generalized power specifications.
The source's manufacture-year fields (2020, 2020, 2022 and 2024) are not manufacture
dates for an individual auction unit. No current support, software entitlement,
operator, deployment or auction availability is inferred.

`data/exports/oracle-ecodesign-model-relationships.json` retains all four source
relationships, with page references and PDF hashes. `ecodesign_product_scope` is
an informational identifier kind, excluded from inventory identity matching and
auction-title aliases. A search result for X10M may lead to the ZU2XL evidence,
but that alone does not pass an exact X10M inventory expectation.

## Validation

Every PDF/text hash, page count, document heading, manufacturer field, qualified
product scope, compliance code and reviewed field set is validated before any of
the four source snapshots is replaced. Three regression tests verify shared-code
relationships, no false alias/baseline pass, changed manufacturer/scope rejection,
later-dependency failure preservation and repeat-import stability. All 238 tests
pass. No new device identity is needed: the three compliance codes were already
present through BSMI. These four records strengthen their evidence.

The owned-device baseline remains 10/10 searchable and 8/10 corroborated. Exact
searches for the two outstanding Edge-Core labels still yield seller evidence or
complete suffixed variants, not the missing independent exact-label proof.
