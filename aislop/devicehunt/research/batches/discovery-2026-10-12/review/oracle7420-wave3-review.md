# independent Oracle 7420 wave-3 discovery review

reviewer `/root/psu`; author `/root/handoff_audit`; 2026-10-12 local date.

**ACCEPT for bounded discovery**. all **12/12** claims checked against the two
original HTML captures; no correction required. no candidate, identity-routing
decision or live promotion is accepted by this review.

reviewed `original-vendor-wave3/oracle7420/{handoff.md,findings.json,checks.json,
request-ledger.json,fetch-report.json,fetch.py,text-extraction.json}` plus all
three raw responses and the saved original link basis in `next-leads/plan.json`.
findings SHA-256:
`fda5b86d5e017b2b71fd520f9a16b9e17942819b4e6e6b3e1659c886efa62a13`.
request-ledger SHA-256:
`1ea0b9c90d16cecd0c7e06501cba6563e5ad11f846a73d1b1ea787544d2029d6`.

## request and provenance checks

exactly **three** GET exchanges: Oracle robots plus the two assigned HTML pages.
all statuses200, requested/final URLs identical, no Location or redirect,
no retry/search/screenshot/image/other-page request recorded. the fetch helper
disables automatic redirects and refuses repeat execution after a ledger exists.
finite page list and stop logic agree with the saved successful ledger.

all three raw byte counts and SHA-256 values independently agree with ledger
and handoff; UTC request/completion timestamps are ordered. independently
parsed saved robots with the declared user agent
`devicehunt/1.0 (bounded public documentation research)`: both paths permitted,
no crawl delay. recorded policy decisions agree. no denied/throttled or
login-required page was substituted for content.

| raw file | bytes | independently verified SHA-256 |
| --- | --- | --- |
| internal-components.html | 20,339 | `9bbfce09f2b51a8e0bad158e764bf307e53cb5b2c84b258b6c82ff903eb80a83` |
| attached-storage.html | 5,682 | `f3772553049f0e1a6671cb0579043be928550976129b2ea8e104f1ca3c4e6642` |
| robots.txt | 158,708 | `40bfc5a2ef02ae1a03b2805ba1416ff9da1d74367f3dc8f0acaa7ac16de0e9c0` |

immutable original PCIe-page basis SHA-256 independently verified:
`d0e6fb8ab728d0a12fa1cd8b6e77d4bebda4df121fa0b7da06ab040d8a95361a`.
both exact sibling hrefs occur in that HTML; URL resolution equals the two
requested service URLs in the ledger. no guessed filename or broader crawl.

## complete claim checks

independently parsed article paragraphs and section IDs from original HTML.
all supplied source hashes, section IDs, paragraph ordinals and source text
agree; whitespace adjoining closing links/punctuation is insignificant in
the HTMLParser aid. manually checked every value against those original
paragraphs and table markup:

| claim | original article paragraphs | finding |
| --- | --- | --- |
| component_legend | internal3–13 | all11 labels match. original table summary explicitly calls this a figure legend; indices3/10 do not count PSUs/fans. |
| fru_service_restriction | internal14 | trained Oracle service-technician restriction retained. |
| service_processor_power | internal15 | SP3.3V standby/AC-present/off-state condition retained. |
| power_distribution_topology | internal16 | FRU/main12V, Vertical PDB, bus bar/ribbon, top-cover interlock and supply→backplane→PDB connections accurate. |
| vertical_pdb_service_name | internal17 | Paddle Card/FRU and named interconnections accurate; no order-code identity invented. |
| power_supply_connection_positions | internal18 | supplies0/1 are named positions; no rating/model/redundancy inferred. |
| fan_power_boards | internal19 | explicitly two **boards**, plus power/LED/I2C functions; no fan-module count inferred. |
| drive_backplane_service_name | internal20 | six-drive FRU/circuit functions accurate; no installed-drive count inferred. |
| m2_system_drive_and_cache_rule | internal21 | exact M2 scope, mirrored slots0/1, up to four cache SSDs slots2–5 in order; per-controller clustered SSD counts may vary. |
| 7420_system_drive_and_cache_rule | internal22 | original7420 scope retains matching SSD counts between clustered controllers; not reconciled with M2 rule. |
| fan_fault_indicators | internal37,39–42 | all named fault LEDs retained; Overtemp **might** light, not guaranteed. |
| attached_storage_chain_and_shelf_limits | attached1 | one–six chains, up to six HDD-only/SSD-capable shelves per chain in any order; no invented shelf/cable/port/topology claim. |

article title/section identify **Sun ZFS Storage 7420**, in the named OS8.8.x
service manual. the separately named M2 paragraph retains its own variant
scope. author proposes no route to existing `ZFS STORAGE 7420` or `S7420`
identities, no equivalence alias, component identity or company role.
source statements about FRUs are documented service names and functions,
not component manufacturer/part-number evidence or unit-specific fitment.

absence statements are bounded to the two captured pages: no exact PSU/fan
part code, PSU wattage/input rating/connector, airflow direction or safe
standalone wiring procedure found. following cabling/shelf pages and embedded
images were not acquired or visually reviewed; no finding needs their content.
no current support/entitlement, installed configuration, certification or
auction-unit provenance is asserted.

this review used saved files only; no network, import, code tests, candidate,
database/manifest/archive/export write or delegation. only this review file
was written. stop condition met: twelve claims, provenance and finite request
set accepted; candidate construction remains a separate coordinator decision.
