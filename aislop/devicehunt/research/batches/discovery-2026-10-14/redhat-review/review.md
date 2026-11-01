# independent red hat expansion review

**accept the exact candidate for staging through the existing curated importer.**
No blocking findings in this bounded snapshot. Reviewer `/root/odm_bulk_full`,
independent of root's collection/parser/candidate authorship. Accepted input:

`../redhat-vendor-expansion/candidate.json`

SHA-256: `c286acae001d79fcad0efadd9400e625a929eae88e16ed6f56a223fcfb0a2a17`.

This accepts source-scoped certification-listing records, not manufacturer,
deployment, support, product-brand or assembled-chassis claims. The nine literal
listing holders remain separate namespaces, with `listing_holder` affiliations
only and no target aliases. Normal sequential integration/rebuild/preservation
checks remain the integrator's responsibility. This review did not promote data.

## independent evidence checks

[Audit code](audit.py) reconstructs every candidate row independently from raw
JSON and compares the complete ordered device dictionaries, including all facts,
identifiers, source URL, source key, affiliation and original model/title strings.
[Results and pinned input hashes](audit-results.json) record all checks and test
logs. All source inputs remained unchanged during review.

- All ten receipts are HTTP 200 / curl exit 0 with matching body bytes and
  SHA-256. Saved headers also report 200 and are independently hash-pinned here.
  No redirects, failures, retries, alternate routes or detail-page requests occur
  in the recorded source set. Collector code uses ordinary unauthenticated GET,
  a fixed user agent, no automatic redirects and no retry option.
- Saved robots policy allows every requested route. It was acquired first; code
  checks `can_fetch` before partner queries and sleeps at least two seconds.
  The minimum gap between finished receipts is 2.295123 seconds. No crawl delay
  is specified. Public Solr response parameters explicitly report unauthenticated
  and unentitled status and retain upstream filters excluding private/hidden rows.
- The selection basis hash matches the prior saved partner-name facet, and all
  nine selected names/counts match that artifact. No prior partner-query snapshot
  was reacquired. This review makes no global archive-completeness claim.
- All nine Solr responses have status 0, no partial-results flag, start 0 and
  `numFound == len(docs) < 200`. URL query parameters and echoed response filters
  match each exact literal partner. All 245 IDs are globally distinct in this
  batch. Thus completeness is established for these nine filtered snapshots only.
- Exactly 242 rows explicitly list System + Server/Edge System. The three omitted
  rows are Desktop/Workstation only: IDs 318717, 3445 and 280857. All remain in raw
  captures. Independent reconstruction matches 242 candidate records, 3,388 facts,
  nine sources, 484 identifiers and 242 affiliations exactly.

## identities and role boundaries

The accepted convention agrees with the prior reviewed Red Hat non-Oracle model
rule: keep the separate `model` field and preserve full `partnerProductName` as
both title and identifier/fact. No make-derived aliases or corporate mappings are
added. Snapshot path/hash plus zero-based row ordinal establish full raw-row
provenance; source metadata resolves to the same snapshot bytes in `prepare_curated`.

There are 223 accepted rows where literal `make` differs from `partnerName`,
including capitalization/product-line variants. Important examples checked:

- Gigabyte holder, `Penguin Computing, Inc.` make: Altus XE1112, Relion XE1112 and
  Relion XE2112; Gigabyte holder, `Format sp. z o.o.` make: FormatServer THOR E12.
- TYAN Computer Corporation holder, MiTAC make: Tiogapass. MiTAC Computing
  Technology Corp holder remains separate from TYAN; neither becomes an alias.
- ASRockRack holder has eleven literal `Motherboard` makes; Gigabyte has
  `Server Motherboard` for GA-6UASL3. Their `server` kind follows Red Hat's explicit
  System/Server classification. These entries do not establish that an auction
  item is an assembled server/chassis. Raw make and system-type facts remain visible.
- ID 5277 preserves model `EPC612D4I ` and title `ASRockRack EPC612D4I ` with their
  trailing spaces. The existing catalog identity function trims for its identity
  hash; the original string/facts remain preserved and no collision occurs here.

All 242 candidate identity hashes are distinct under the actual production
identity function, and none collides with live identities. No candidate source ID
or catalog identifier already exists live. The author novelty report's zero hits
is therefore independently confirmed using actual production identity hashes,
not just its case-insensitive SQL approximation. All new affiliation targets are
NULL; `listing_holder` is the only new role. Hyperscaler assertions stay empty.

## executable validation

Project direnv Python 3.14.7; no installs or network. Independently ran all seven
author parser tests and all 16 production curated-import tests: **23 passed**.
Tests meaningfully cover exact identity/role separation, unlike make strings,
wrong partners, missing identity/bad URLs, duplicate IDs, explicit partial-query
coverage, excluded desktops versus retained edge systems, and exact real-capture
replay. Production importer tests exercise atomic failures and legacy semantics.

An independent read-only SQLite backup of the live database was imported using
unchanged `prepare_curated` / `curated`, in review-owned
`candidate-review.sqlite`. First import returned 242; repeat import returned 242
with unchanged counts. Scratch results:

| item | before | after |
| --- | ---: | ---: |
| devices | 5,157 | 5,399 |
| records | 7,302 | 7,544 |
| sources | 2,121 | 2,130 |
| facts | 74,837 | 78,225 |
| identifiers | 6,891 | 7,375 |
| affiliations | 10,640 | 10,882 |
| hyperscaler evidence | 22 | 22 |

All old logical rows are preserved across all 50 non-search tables (zero missing
rows in each). Integrity is `ok`; foreign-key violations zero. Reimport preserves
counts. Live database SHA-256 before/after remains:

`0879a77f5d2cdf4edfdce3a88ab03034504c02f99b4645537f43af5db5c32853`.

Only this `redhat-review/` directory was written. No shared code, source captures,
catalog, manifest or exports were changed. No live handles remain.

## nonblocking helper hardening

For later collections, `parse_listing` should explicitly reject Solr nonzero
`responseHeader.status` or `partialResults: true`, and validate list/object field
types before accepting them. It currently checks row counts/start/type/partner
but does not enforce those extra envelope flags. The independent audit verified
that every bound capture has status 0, no partial flag and the expected types,
so this does not block the exact accepted candidate. A future changed capture
requires fresh validation rather than inheriting this acceptance.
