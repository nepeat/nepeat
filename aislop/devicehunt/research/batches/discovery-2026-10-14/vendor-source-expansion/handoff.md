# bulk vendor datasource handoff

outcome: **finite collection/extraction complete; independent review pending**.
owner/editor `/root/vendor_bulk_full`, replacement for stopped `/root/wave5_review`.
actual developer policy was danger-full-access, network enabled, approval never;
no sandbox override was supplied. only this directory was written. live catalog,
manifest, production code, complete reference bundle and exports remained read-only.

## actual datasets

| source | discovery rows | distinct raw URLs | query-free endpoints | endpoints absent from catalog |
| --- | ---: | ---: | ---: | ---: |
| Edge-Core 239 saved product pages | 615 | 607 | 465 | 465 |
| Oracle legacy x86 index | 18 | 18 | 18 | 18 |
| Oracle older-M/older-T indexes | 67 | 65 | 65 | 57 |
| Celestica sitemap | 189 | 189 | 189 | 187 |
| total | 889 | 879 | 737 | 727 |

**700 document/index discovery rows span 548 endpoint comparison keys; 540 keys
are absent from catalog source/record endpoints.** that is a discovery novelty
metric, not 540 new devices or proof of uncaptured documents. Celestica contributes
six platform portfolio navigation URLs and 183 corporate/other navigation URLs;
none is counted as a hardware identity. 230 of the 239 saved Edge-Core pages have
matching same-host `/download/` links; all 239 page hashes were verified.

ready artifacts:

- [inputs-ready.json](inputs-ready.json), [discovery-ready.json](discovery-ready.json):
  pinned finite config and complete duplicate-preserving row dataset.
- [discovery.sqlite](discovery.sqlite): private SQL/FTS discovery store;
  [integration-contract.md](integration-contract.md) includes working search and
  fresh-build commands, schema boundary and production acceptance requirements.
- [edgecore-document-inventory.json](edgecore-document-inventory.json): 465
  endpoint groups, every raw URL/heading/metadata variant and original row refs.
- [offline_parser.py](offline_parser.py), [build_discovery_db.py](build_discovery_db.py),
  [prepare_inputs.py](prepare_inputs.py), [test_offline_parser.py](test_offline_parser.py)
  and [verify_offline.py](verify_offline.py): reusable standard-library tools.
- [checks.json](checks.json), [tests-final.log](tests-final.log), `SHA256SUMS`:
  actual author validation and frozen artifact hashes.

`collector.py parse` now delegates to the same final parser. capture remains an
explicit-one-URL bounded helper, not a recursive crawler or a production importer.
older `inputs.json`, `inputs-final.json`, `inputs-accepted.json`, `discovery.json`,
`discovery-v2.json`, `discovery-final.json` and `discovery-accepted.json` are
superseded development outputs. the word accepted in those development names
never denotes independent acceptance. use only the **ready** pair above.

## evidence, identities and conflicts

all 615 Edge-Core links retain printed document headings and metadata plus raw
anchor text, exact URLs, `wpdmdl` and refresh parameters, product-page placement,
line/anchor ordinal and capture hash. headings include full codes and revisions,
for example `AGR100 (AS5912-54X) DS R20`. query-free grouping does not merge
identities or assert document equivalence. 40 rows have printed revision tokens
that disagree with URL tokens; both remain visible. e.g. `DCS203 (AS7326-56X)
DS R10` is linked through a `dcs203-as7326-56x-ds-r09` path. that row is
conservatively excluded from future scheduling under inherited missing-route rules.

Oracle legacy x86 preserves 18 full printed names, including `Sun Fire X2100 M2
Server` separately from `Sun Fire X2100 Server`. older-M contributes six rows;
older-T contributes 61, with printed multilingual group headings preserved
(including literal `SPARCT3 ...`). three distinct T8 headings share one source
URL; every row survives. these are documentation-publisher links, including
Fujitsu-labelled documentation, not Oracle-manufacturing statements. 22 total
rows have full printed-label model equality to existing catalog identities
(7 Edge-Core, 15 legacy x86); four rows have complete-label identifier equality.
heuristic tokens remain separate leads, with no suffix expansion or aliases.

## capture provenance and access limits

inherited captures/receipts/first six reservation rows were preserved; new
requests were appended as sequences7–8. [resume-full-access.md](resume-full-access.md)
is the prior interrupted checkpoint. [oracle-recovery-plan.json](oracle-recovery-plan.json)
pins the prior DNS-only failed receipt and saved public SPARC index with both
literal target hrefs; the explicit environment-recovery allowance never bypassed
an HTTP denial.

- [Oracle older M](https://docs.oracle.com/cd/F24632_01/index.html): HTTP200,
  `oracle-older-m-series-full-access.html`, SHA-256
  `918a37ccd45b0d441a930da97609dcd4f7adc157a649546e2d2764235215260f`.
- [Oracle older T](https://docs.oracle.com/cd/F24630_01/index.html): HTTP200,
  `oracle-older-t-series-full-access.html`, SHA-256
  `8226d79a1d89a337993078b5872087dd9d19ffa6ec6d8c6cc7a0be38792ac2cb`.
- the inherited Celestica sitemap and Oracle x86 receipts pin successful200
  bodies. Accton `/sitemap_index.xml` returned404 HTML and is excluded from
  parsing; it is unavailable, not an empty Accton product dataset. no alternate
  sitemap guess was attempted. Celestica compute is a coming-soon form, not an
  inventory; no contact/form submission occurred.

final batch consumption: **3/8 search queries; 8/12 conservative transport
attempts, 7 actual HTTP responses** (one inherited DNS-only failure). only two
HTTP requests were made by this replacement, both200, zero redirects or retries
after recovery; no additional searches. four HTTP attempts and five searches
remain unused. collection stops here. no denied item, alternate copy, paid
source, package install, delegation, publication or seller/provider contact.

## checks and next bounded step

all **8 offline fixture/integration tests pass**. they cover exact suffixes,
revision conflict, duplicate raw URLs versus shared endpoints, contextual
headings, multilingual table scope/tooltip exclusion, hash/status/path failures,
read-only missing-DB behavior, SQL/FTS search and refusal to replace an existing
private DB. raw-link counts were independently recomputed from source bytes,
including duplicate multiplicities; reparse matches the ready JSON exactly.
243 capture hashes remained unchanged. private SQL integrity `ok`, FK violations0;
FTS probe `AS7326-56X` returns3 source-scoped rows and leaves its DB hash unchanged.

live catalog remains SHA-256
`0879a77f5d2cdf4edfdce3a88ab03034504c02f99b4645537f43af5db5c32853`:
5,157 devices / 7,302 records / 2,121 sources / 74,837 facts. manifest remains
`86f6179ff8dd490739936d5ab416e39e606380acf547ac009b1dc5b269dca81b`.
no full production suite was needed or claimed for private research-only code.

root should independently review the ready pair, source-role boundaries,
restriction flags, SQL contract and checks before accepting a separate discovery
index integration. any future document capture is a separate finite acquisition
batch after historical-ledger deduplication; the current graph is not a queue to
crawl. exact hardware facts, Accton bulk product inventory, current support and
manufacturer/deployment claims remain outside this deliverable.
