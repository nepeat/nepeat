# Inspect hyperscaler relationships locally

`hyperscaler-inventory` shows the reason a model appears in hyperscaler research,
alongside its source, historical date and company roles. It reads existing
catalog records without fetching listings or changing the database.

```sh
python3 -m devicehunt hyperscaler-inventory
python3 -m devicehunt hyperscaler-inventory --relationship documented_platform_deployment
python3 -m devicehunt hyperscaler-inventory --vendor Celestica --role manufacturer
python3 -m devicehunt hyperscaler-inventory --operator 'Facebook / Meta' --format json
python3 -m devicehunt hyperscaler-inventory --include-unverified --limit -1 --format csv
```

## Interpret the relationships

Relationship names are kept verbatim, without upgrading weaker or unknown
categories. The current saved-source results include:

| Relationship | Current assertions | Interpretation |
|---|---:|---|
| `documented_platform_deployment` | 3 | Source reports platform deployment; not the history of an auction unit |
| `documented_design_lineage` | 4 | Design connection; does not establish deployment of that commercial model |
| `announced_platform_deployment` | 1 | Historical plan, not proof the plan was carried out |
| `documented_fabric_role` | 1 | Meta's dated account of Minipack3's role in building its fabrics; not an explicit completed-deployment statement |
| `documented_product_family` | 3 | Product-family connection, not hyperscaler data-center deployment |
| `documented_product` | 6 | AWS Monitron product identity; two sources name TE1A001, and no data-center deployment is asserted |
| `documented_brand` | 1 | CubScout report identifies AWS branding; no deployment or auction-unit history is inferred |

These 19 assertions cover 17 catalog identities. Assertion counts are not
independent votes or counts of distinct physical products.

The three deployment records currently name Accton's Wedge 100 and Celestica's
Wedge 400/Wedge 400C. No additional deployment assertions are inferred from
brand names, a device's other records, raw document search, or owner inventory.
The archived Minipack2 specification remains separate document evidence.

## Filters and evidence boundaries

Default output requires external-source snapshot metadata eligible under the
same rules as `vendor-inventory`. This checks stored tier/hash/path metadata;
it does not reread all files or independently confirm every source assertion.
`--include-unverified` exposes all 21 imported relationship records, including
two uncached claims, retaining their tier and `source_snapshot_eligible` flag.
The same option also permits marketplace claims if added later.

`--vendor` requires a target-company affiliation on the **relationship record**.
`--role` is an exact role string. With both filters, one affiliation must satisfy
both; neither another company on the record nor another record on the device can
lend its manufacturer role. All affiliations remain visible, with matching ones
identified separately in JSON/CSV.

The 2025 Minipack3 fabric-role record names Meta as documentation vendor. Its
existing Celestica identity namespace comes from the separately validated 2024
manufacturer caption, with that source URL and hash recorded as identity context.
The 2025 record therefore appears in the unfiltered and fabric-role reports,
but not under the strict Celestica vendor filter. It does not borrow the 2024
record's manufacturer affiliation. See [the source review](../research/minipack3-fabric-role.md).

For example, the default AWS product-family rows have AWS documentation-vendor
affiliations. They do not acquire an Amazon Data Services legal-entity affiliation
from another record or from the AWS name. The Amazon Data Services-filtered
report instead returns five Monitron guide records that explicitly name that
entity: three sensor manufacturer claims and two gateway compliance-declarant
claims. All five are `documented_product` relationships. An empty deployment-filtered
result is a relationship-coverage gap, not proof that no deployment exists.

The ADS filter additionally returns CubScout's `documented_brand` relationship
with a regulatory-applicant affiliation. Its original report download failed;
the saved source is a labeled factual web-text extract. Snapshot eligibility
does not turn that extract into an archived PDF. Foxconn remains the report's
named manufacturer. See [tested configurations and source limits](tested-components.md).

`--operator` matches the complete stored label after case folding and trimming
outer whitespace. Use `Facebook / Meta`; `Meta` alone is not an inferred alias.
`--relationship` matches the complete exact relationship string. Unknown filter
values return no rows, while unknown relationship categories in the data remain
visible in unfiltered output.

## Output and reproducibility

Each row represents one source relationship assertion. Conflicting records,
company namespaces and hardware revisions remain separate. JSON reports counts
before pagination; `--limit 0` shows counts, and `--limit -1` returns all rows.
JSON/CSV retain record IDs, source URLs, hashes, revisions, dates and roles.
Table output is a compact human-readable view. Dates describe historical sources;
retrieval time is not a deployment date. No result guarantees current use, stock,
firmware access, factory location or unit provenance.

Saved outputs in `data/exports/`:

- `hyperscaler-shortlist.md`: readable default shortlist with source links.
- `hyperscaler-inventory.json` / `.csv`: all eligible relationship records.
- `hyperscaler-inventory-all.json` / `.csv`: also includes unverified records.
- `hyperscaler-deployed-manufacturer.json` / `.csv`: documented deployment and
  literal manufacturer role on the same record.
- `hyperscaler-inventory-<vendor>.json` / `.csv`: each requested target filter.

The live audit checks all 21 rows against `hyperscaler_evidence`, validates company
roles against their source records, and confirms unchanged database bytes and
owned-hardware validation. Six regression tests cover evidence eligibility,
relationship distinctions, revision/date preservation, company/role isolation,
filters/limits, read-only CLI output and caller transactions.
