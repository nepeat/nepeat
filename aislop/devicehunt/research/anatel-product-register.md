# Anatel published product register

Captured 2026-09-26 from the official
[product consultation page](https://informacoes.anatel.gov.br/paineis/certificacao-de-produtos/consulta-de-produtos)
and its linked
[bulk ZIP](https://www.anatel.gov.br/dadosabertos/paineis_de_dados/certificacao_de_produtos/produtos_certificados.zip).
Two downloads have identical SHA-256
`91f09ca0795d0833f2749dbcb6d3a1569fe2bc2c14c5cb1d7a8482833b33157f`.
The 7,888,200-byte archive contains one UTF-8 BOM, semicolon-separated CSV,
`Produtos_Homologados_Anatel.csv` (53,653,965 bytes). The raw index, both archives
and checked bundle are included in the offline rebuild manifest.

All **190,852** data records are retained in SQL, including **1,178** exact
duplicate records and **37** empty-model records. There are **87,949** distinct
approval-number strings, including one empty value. Neither duplicate counts nor
approval IDs are used as counts of unique physical designs. There is no
independent server-side record total: coverage is the complete published file,
not a verified copy of every record in the regulator's underlying system.

Exact target manufacturer matches contribute 235 assertions:

| Target | Source rows |
|---|---:|
| Edge-Core | 102 |
| Accton | 63 |
| Celestica | 34 |
| Amazon Data Services | 20 |
| Oracle | 16 |

The literal `Celestica Technology Consultancy (Shanghai) Co.,` string is added
to the explicit Celestica name mapping. Its original truncated spelling is
retained; no missing legal suffix is invented. `Accton Technology (China) CO. LTD`
and `Compaq (Celestica International)` remain available in the full register but
are not silently assigned to a target entity.

## Findings

The register explicitly names EdgeCore Networks Corporation as manufacturer for
AZ4444-52X-AC-00 (079542403459), AZ4444-52X-DC-10 (225812303459) and
AZ4444-52X-DC-42 (054422403459). This adds vendor evidence absent from the earlier
Accton-only variant records. The bare AZ4444-52X baseline remains unresolved;
suffixes are not discarded. AS7760-32X is listed under EdgeCore, but the full
7760-32X-A-AC-F order code is not established.

Amazon Data Services manufacturer rows include AZ9074-64X-DC-42,
AZ9075-64X-DC-42, AZ9274-64X-DC-42, AZ9275-64X-DC-42, NAF20_L1 and
CMWA6400, alongside TE1A001. These are registry manufacturer claims, not factory
attribution or evidence of deployment/ownership of individual auction units.
Commercial-name cells contain additional variants but remain unparsed facts.

CS8200-32X-UN-00, DMC-12-48R-AS0 and AS7772-32X gain additional corroboration.
The inventory remains **10/10 searchable, 8/10 corroborated**.

## SQL fields

`anatel_equipment` stores the source ID, a one-based logical data-record number
(excluding the CSV header), and all 21 source columns. No rows are collapsed.
`devicehunt/anatel.py: FIELDS` is the exact Portuguese-to-SQL column mapping.
Approval/certificate dates and statuses, manufacturer country and flags remain
raw source strings; they are not interpreted as present approval validity,
fabrication location, support dates or hardware activity. Applicant identity is
separate from the declared registry manufacturer. CNPJ values are text so their
leading zeroes survive.

```sql
SELECT model, manufacturer, approval_number, certificate_number,
       certificate_status, application_status, certificate_expiry
FROM anatel_equipment
WHERE model LIKE 'AZ4444%' ORDER BY model;
```

Use `python3 -m devicehunt anatel QUERY` for case-insensitive literal searches
over manufacturer, applicant, model, commercial name and approval number.
Percent/underscore characters are literal. Default limit is 100; `--limit -1`
returns every matching row. Broader target-company search uses the catalog's
existing `search` command and includes all imported manufacturer assertions.
