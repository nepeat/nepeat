# Selected OCP product evidence

Reviewed 2026-09-26. `data/raw/ocp-products-reviewed.json` contains factual fields
and brief paraphrases from twelve official OCP product pages. This is a selected
capture, not a full OCP database mirror. Direct downloads of the product index
and legacy networking wiki returned HTTP 403. Original HTML is not cached.
Each row states whether the evidence came from web-tool search or open output;
search-cached content may lag the live page.

## Imported identities

| OCP product ID | Brand | Model(s) | Source |
| --- | --- | --- | --- |
| 335 | Celestica | DS1000 | [Product](https://www.opencompute.org/products/335/celestica-ds1000-1u-48-port-1gbe-access-switch) |
| 428 | Celestica | EG1050 | [Product](https://www.opencompute.org/products/428/celestica-eg1050-1u-48-port-1gbe-25gbe-poe-edge-gateway-with-wifi-and-5g) |
| 677 | Celestica | ES1000 | [Product](https://www.opencompute.org/products/677/celestica-es1000-1u-24-port-48-port-1gbe-poe-access-switch) |
| 678 | Celestica | ES1010, ES1050 | [Product](https://www.opencompute.org/products/678/celestica-es1010es1050-1u-48-port-1gbe-25gbe-poe-access-switch) |
| 679 | Celestica | ES1500 | [Product](https://www.opencompute.org/products/679/celestica-es1500-1u-82448-port-25gbe-poe-access-switch) |
| 703 | Celestica | DS5000 | [Product](https://www.opencompute.org/products/703/celestica-ds5000-2u-64-port-800gbe-data-center-switch) |
| 96 | Edge-Core | AS7726-32X | [Product](https://www.opencompute.org/products/96/edgecore-networks-as7726-32x-32x100g-open-networking-switch) |
| 97 | Edge-Core | AS7816-64X-B | [Product](https://www.opencompute.org/products/97/edgecore-networks-as7816-64x-100gbe-data-center-switch-torspine-switch-power-to-port) |
| 99 | Edge-Core | AS5915-18X-O | [Product](https://www.opencompute.org/products/99/edgecore-networks-cell-site-gateway-dcsg-as5915-18x) |
| 101 | Edge-Core | AS7316-26XB | [Product](https://www.opencompute.org/products/101/edgecore-networks-cell-site-gateway-dcsg-as7316-26xb) |
| 116 | Edge-Core | AS7326-56X | [Product](https://www.opencompute.org/products/116/edgecore-networks-as7326-56x-25g-top-of-rack-switch) |
| 119 | Edge-Core | AS7816-64X-F | [Product](https://www.opencompute.org/products/119/edgecore-networks-as7816-64x-100gbe-data-center-switch-torspine-switch-port-to-power) |

The importer uses the explicit Model field, preserving the O/F/B suffixes. The
ES1010/ES1050 grouped field expands into two models because both identifiers are
explicit. Other slash-separated text, such as port speeds, is descriptive.

## Buying details and limitations

AS7816-64X-B is paired with power-to-port airflow, and AS7816-64X-F with
port-to-power airflow. No general suffix-decoding rule is inferred. Both pages
link an AS7800-64X contribution; that different design name is retained as a
caveat, not an identity alias or permission to transfer specifications.

Likewise, AS5915-18X-O has an unsuffixed heading, and AS7316-26XB links a
contribution named AS7316-26X. Those names remain distinct. Power, ONIE and
physical details are page-scoped assertions, not inspection of a used unit.

The company named in the product heading supplies the catalog brand. Its
separate listing as a solution provider does not prove physical manufacture.
Other listed resellers are not treated as OEMs. No Meta/AWS deployment is
inferred from participation in OCP, from site footers, or from broad cloud-use
marketing. Contribution links are discovery pointers; the linked design files
have not been downloaded as part of this capture.

Some OCP URLs with unrelated old slugs resolve to the same numeric product ID.
The importer rejects duplicate IDs, avoiding duplicate identities from such
URLs. All capture rows are validated before source replacement.

```sh
python3 -m devicehunt import ocp-capture data/raw/ocp-products-reviewed.json
python3 -m devicehunt specs AS7816-64X-F --exact-model --vendor Edge-Core
python3 -m devicehunt search AS7816 --vendor Edge-Core --role solution_provider
```

```sql
SELECT d.brand, d.model, r.source_url, f.name, f.value
FROM records r JOIN devices d ON d.id = r.device_id
JOIN facts f ON f.record_id = r.id
WHERE r.source_id = 'ocp-products-reviewed'
  AND f.name IN ('cooling_description', 'power_description', 'source_caveat');
```

Neither the full 7760 ordering code nor the bare AZ4444 label appears in these
selected records. The two original validation gaps remain open. Fresh exact-code
searches again found seller references and suffixed AZ4444 variants; no primary
source equivalence was established.
