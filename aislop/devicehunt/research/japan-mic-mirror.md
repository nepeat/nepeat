# Japanese radio-equipment approval queries

Current combined capture: **346 rows from 12 name queries**, with 191 distinct
approval-number strings and 131 model-field strings. The additional-name review
below expands the original six-query snapshot without changing its results.

The [Japanese government's API catalog](https://api-catalog.e-gov.go.jp/info/ja/apicatalog/view/47)
lists a radio-equipment approval search API with JSON, XML and CSV output. The
linked official guide returned HTTP 403 in this investigation. It was not
retried or used to guess API parameters, and no official API data was imported.

The independent [Lang-ship approval mirror](https://giteki.lang-ship.com/) is
accessible. Its [operator's explanation](https://lang-ship.com/blog/work/giteki-web-api/)
describes fetching yearly JSON into SQLite and applying fullwidth-to-halfwidth
text normalization. The data here is therefore explicitly `regulatory_mirror`
evidence, not original MIC data or a verified complete Japanese database.

## Captured queries, 2026-09-27

The public form's `name` field was used with all other filters empty. Each query
was downloaded twice. Parsed ordered rows, including duplicates, agree between
the two captures. Original HTML and hashes are retained under
`data/raw/japan-mic/`; the checked import input is `bundle.json`.

| Company-name query | Returned rows |
|---|---:|
| Accton | 177 |
| Edgecore | 21 |
| Edge-Core | 0 |
| Celestica | 0 |
| Oracle | 1 |
| Amazon Data Services | 0 |

All queries are below the page's displayed 2,000-row limit. No independent total
is supplied, so this verifies the returned tables, not historical completeness,
all company spellings, Japanese-language names or every approval scheme. The
zero-result captures are retained as query results, not proof of no approvals.

The 199 rows contain 146 distinct approval-number strings and 86 distinct
model-field strings. Repeated radio classes and exact duplicate rows remain
separate. Neither figure counts unique physical devices. The Accton substring
query also returns Accton Wireless Broadband Corp.; no parent-company identity
is inferred from that spelling.

The Oracle result names `Bump Bar K3-10W, Bump Bar K3-20W`; other useful labels
include `ML2.5-60-BF-18`, EAP104 and grouped EAP112 variants. Grouped strings
remain whole source fields rather than silently generated aliases or models.

## SQL and local search

```sh
python3 -m devicehunt import japan-mic data/raw/japan-mic/bundle.json
python3 -m devicehunt lookup 'Bump Bar K3-10W'
python3 -m devicehunt lookup 'ML2.5-60-BF-18'
python3 -m devicehunt sql "SELECT registered_name,model_field,approval_number,approval_date_raw,equipment_class FROM japan_equipment WHERE registered_name LIKE '%Oracle%'"
```

`japan_equipment` preserves the six displayed fields as approval type, approval
number, raw approval date, registered person/entity name, radio-equipment class,
and model/type-name field. Each row also has a source query URL, detail link,
row number and JSON array of the six normalized display cells. Original spacing
and markup remain available in the HTML snapshots. Detail links are references;
their destination pages have not been downloaded.

The Japanese `氏名又は名称` label is retained as `registered_name`. It is not
automatically interpreted as manufacturer, factory, brand or operator. This
import creates no catalog identities or manufacturer assertions and does not
change the owned-hardware validation result. Dates are source strings, not
claims of present approval validity. The mirror's home page contains some
future-dated entries; those unrelated latest-entry rows are not imported, and
no attempt is made to repair or interpret its date fields.

The combined query rows are exported as `data/exports/japan-equipment.csv` and
`.json`. The unified `lookup` command adds a Japanese dataset section and
includes the source scopes, including the zero-result queries.

## Additional company-name queries

A separate `name_variants` collection in
`data/raw/japan-mic-name-review/bundle.json` contains two matching captures for
each of six additional queries. It is imported with the same `japan-mic`
command. Refreshing this collection replaces only its sources, keeping the
original six queries intact. Lookup now shows all 12 query scopes, including
the additional zero-result sources.

| Query | Returned rows |
|---|---:|
| Amazon | 143 |
| オラクル | 0 |
| セレスティカ | 0 |
| エッジコア | 0 |
| 智邦 | 0 |
| アクトン | 4 |

The broad Amazon query has no exact Amazon Data Services legal-name result.
It returns Amazon.com Services LLC (including another capitalization), Amazon
com Services, Inc, Amazon Robotics/LLC, and the unrelated
Amazonen-Werke H. Dreyer GmbH & Co. KG. These names remain separate raw strings.
They do not become Amazon Data Services devices or receive a hyperscaler role.
This result covers that substring query only, not every possible ADS spelling.

The `アクトン` query returns four rows under `アクトンテクノロジィ株式会社`:
AD11A-1 with NYCA0053 and GZCA0036, and AD11C-1 with NYCA0054 and GZCA0037.
Each displays 2000-10-03. The two different radio-class rows per model are kept.
No English legal-entity alias, parent-company mapping or manufacturer claim is
inferred from the Japanese name. These exact labels are now available in SQL
and `lookup AD11A-1` / `lookup AD11C-1`.

Supplement-only exports are `data/exports/japan-name-review.csv` and `.json`.
The existing grouped-model and zero-result limitations still apply. An added
regression test verifies that base and supplemental collections coexist, a
collection cannot be relabeled to overwrite another query set, zero-result
scopes survive refresh and Japanese names do not create target affiliations.

## Validation and rebuild

The importer checks source URLs, query-form filters, headers, table shape,
certificate links, company-query association, the display cap, checksums and
agreement between both captures before replacing any source. A changed or
capped result fails validation. Only the selected collection's six sources are replaced.
The catalog schema advances to version 25 for the new raw table; read-only
commands do not migrate an older database and report the absent dataset as
not loaded.

Tests cover duplicate/grouped rows, Unicode preservation, zero queries, changed
captures, malformed headers/filters/links, hash/path/count guards, display-cap
rejection, scoped refresh and local lookup. A live audit re-parses both captures
and checks every stored cell and detail URL. Device/record/affiliation tables
are unchanged. Audit files include `japan-import-audit.json`,
`japan-sql-lookup-audit.json`, and the full rebuild comparison in `data/exports/`.
