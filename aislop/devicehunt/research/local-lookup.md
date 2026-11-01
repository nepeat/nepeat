# One local lookup across cached sources

`python3 -m devicehunt lookup QUERY` searches twenty-five datasets in a read-only SQLite
transaction. It performs no network access, imports, schema migration or file
creation. The existing `search` command remains the catalog's prefix-token full
text search; `lookup` uses a Unicode case-insensitive literal substring across
the catalog text and each registry's relevant fields. Leading/trailing query
whitespace is trimmed. Punctuation, suffixes and internal whitespace are retained.

| Dataset | Search scope | Result unit |
|---|---|---|
| Catalog | Model, title, brand, identifiers, facts, affiliations and indexed exhibit titles | Catalog identity |
| Ghana NCA | Approval number, manufacturer, brand, product type, model and visible cells; detail links retained separately | Public equipment-table row |
| Nigeria NCC | Applicant, certificate holder, manufacturer, equipment, models and original cells; damaged text flagged | Published equipment-register row |
| ISED REL query exports | Company, HVIN, PMN, HMN, FVIN, certificate and all 44 original CSV cells; exact HTML-match status retained | Canadian query-export radio configuration row |
| ENERGY STAR servers | Product ID, brand, partner, model names/numbers and all original fields including configuration power measurements | Published server qualification record |
| ENERGY STAR server archives | Brand, partner, model names/numbers and resolved workbook fields with configuration subheaders | Historical qualification row |
| ENERGY STAR network/storage | Product ID, brand, partner, model names/numbers and all original fields across five current/historical lists | Network/storage qualification row |
| FCC details | FCC ID, grantee, description and verbatim application display fields | Mirror detail page attempt |
| FCC excerpts | FCC ID, grantee, category and mirror-extracted text; truncation retained | Mirror text excerpt |
| PDF documents | FCC ID where assigned, document ID, title, category and embedded page text across vendor, regulator and FCC archives | Cached PDF page |
| FCC IDs | FCC ID, grantee code/name, product description, equipment class and purpose | Selected mirror index entry |
| RRA | Main/derived models, certificate number, applicant, manufacturer, equipment and remarks | Official labelled approval detail |
| NCC | Certificate IDs, equipment, brand, model, applicant, manufacturer and testing fields | Published CSV row |
| BSMI | Vendor code, certificate number, applicant, product name and primary model | Published XML row |
| BSMI product certificates | Certificate, applicant, product and primary model | Published XML row |
| BSMI voluntary certificates | Certificate, applicant, product and primary model | Published XML row |
| BSMI declarations | Registration number, company, business number, product and primary model | Published XML row |
| PCI | Numeric vendor/device/subsystem/class identifiers and registered names | Community PCI ID entry |
| IEEE | Registry, assignment prefix, registered organization and address | Address assignment row |
| NTRA | Product, brand, model, linked models, sequence number and decoded raw cells | PDF table row |
| TEC | Applicant, declared OEM, primary/additional models, certificate ID, product and variant | Historical HTML table row |
| Anatel | Model, commercial name, manufacturer, applicant, approval and certificate numbers | Published register row |
| ICASA | Labelled fields and decoded raw table cells | PDF table row |
| Bahrain | Model, manufacturer, marketing name, description, reference and remarks | Published register row |
| FCC grantees | Company name and grantee code | Historical company registration |

ISED and UL target entries are included in the catalog. This command does not
search unimported files or claim coverage of entire live regulatory databases.
The separate ISED section searches complete saved company-query CSVs, including
non-target companies and unmatched rows. It also reports verified zero-query
sources without fabricating raw rows.
Company/role distinctions from the existing catalog remain intact. It does not
merge identities across databases or turn an ICASA model-string match into an
inferred manufacturer. Different dataset counts must not be summed as devices.

Each dataset reports stored row count, total matches, returned rows, truncation,
and loaded/not-loaded state. `--limit 20` is the default per-dataset cap; use
`--limit 0` for counts and `--limit -1` for all matches. A missing imported dataset
is distinct from a loaded dataset with zero query matches. An absent database
produces an error without creating one. Counts, rows and source metadata share
the same read transaction.

JSON output includes source scope, timestamps, source URLs and catalogue
affiliation roles. Catalog `identity_text_matches` means whole-string equality
with a model or reviewed identifier kind, ignoring case; it is not a source
credibility, expected-vendor or corroboration check. Listing document references
and fact-only matches are excluded from that field. Seller assertions remain
labelled `marketplace`, even when their model text matches exactly.

ICASA `review_required` entries keep NULL model fields and decoded `raw_cells`.
TEC preserves blank OEM/model fields and repeated certificate IDs. Invalid dates
have NULL parsed values, verbatim raw cells, and an explicit review status.
NTRA does the same for incomplete or undecodable rows. JSON null cells become
empty search text, while returned raw cells retain null values. Older databases
without a registry table report that dataset as not loaded; lookup never adds
the missing table. Importing the source adds its schema normally.
Duplicate Anatel rows remain separate and count separately. `%`, `_`, quotes and
SQL-looking text are literal query content, never wildcard or SQL syntax. JSON
encoding characters are not treated as raw ICASA cell content.

Verification includes cross-source fixtures, duplicate preservation, Unicode
case folding, punctuation, empty-source states, count limits, pending caller
transactions, exact text versus factual mentions, read-only file checks and
missing-database behavior. Production examples are exported as
`data/exports/lookup-az4444.json` and `data/exports/lookup-7760.json`.

Three additional BSMI tables are now searched independently: `bsmi_product_certificates`,
`bsmi_voluntary_certificates` and `bsmi_declarations`. Together they retain 20,179
raw rows, including duplicates and blanks. Applicant/registered company is not
a manufacturer assertion. See [source scope and audit](bsmi-additional-registers.md).

Official Korean RRA manufacturer queries now retain 1,081 rows covering 1,080
distinct certificates across all five targets and legacy Sun. Composite company fields remain
unsplit and original detail URLs are available. See
[source scope and CSV limitations](korean-rra-targets.md).
The zero-row `Edge Core` query is retained in source coverage, with original
no-results evidence, rather than as a fabricated device row. See
[alternate spellings and zero-result handling](rra-edge-core-spellings.md).

## Catalog search performance

The catalog part of unified lookup now collects matching text-index device IDs
once per SQL statement instead of rescanning the unindexed FTS device-ID column for each
catalog identity. Literal Unicode case folding, punctuation, suffixes, limits,
counts and identity-versus-fact distinctions are unchanged. Duplicate index
entries do not multiply catalog rows, stale IDs cannot manufacture devices, and
direct model/title matches still work if an index entry is missing.

On the 3,300-identity local snapshot, complete 19-dataset lookups measured
7.8010 to 2.6696 seconds for CSS-24H2L and 5.7905 to 2.4968 seconds for Oracle.
The full JSON outputs agreed before and after. The benchmark alternated execution
order across the two queries; these are observed wall times, not timing guarantees.
Other raw-register scans still contribute most of the remaining work.
See `data/exports/lookup-membership-benchmark.json`. No database migration or
additional cached index is needed for this read-only query change.

## Registry scan performance

Lookup now evaluates its search predicate once per dataset row, instead of
separately filtering for the total and the displayed results. Positive limits
use a SQLite window count before pagination; count-only requests fetch no rows,
and unlimited requests count the returned rows. Source metadata, raw duplicates,
ordering, Unicode matching and evidence distinctions keep their existing meaning.
No database migration or additional persistent index is required.

On the 3,528-identity snapshot, three alternating before/after runs gave median
complete 25-dataset lookup times of 2.6914 to 1.4746 seconds for Oracle, and
2.1704 to 1.6321 seconds for the broad literal query Inc. These are observed
local wall times, not performance guarantees. Counts-only queries already used
one filtered scan, so this change is not expected to speed those up. Window
counting can materialize matching rows in SQLite; broad-query performance still
depends on the result size and local resources.

The full-output comparison audit is `data/exports/lookup-window-audit.json`.
Regression tests also count predicate evaluations across positive, zero and
unlimited limits, including no-match queries, to detect duplicate scans without
relying on unstable timing assertions.
