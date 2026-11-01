# Saint Vincent NTRC and Mauritius ICTA snapshots

Captured September 27, 2026. The local database now includes **4,985 raw rows**:

| Public source | Rows | Published fields | Coverage check |
|---|---:|---|---|
| [Saint Vincent and the Grenadines NTRC](https://www.ntrc.vc/consumer/type-approval-registry-2/) | 2,896 | Brand, Device, Model, Foreign Certification # | Entire identified HTML table; no independent total or publication date |
| [Mauritius ICTA](https://www.icta.mu/type-approved-list/) | 2,089 | TA number, Type, Make, Model | Public All-rows query returned 2,089; total and unfiltered counts agree, and a second download returned the same ordered rows |

Neither snapshot contains an explicit brand/make match for Accton, Edge-Core,
Celestica, Oracle or Amazon Data Services. NTRC's Amazon F100 Ghost row does not
establish an Amazon Data Services relationship. No manufacturer, target catalog
identity or hyperscaler claim was created. The target inventory and owned-device
baseline are unchanged. These are additional local reference registers, useful
for searching labels and foreign certification references.

`public_equipment_rows` retains each source row, including duplicate and grouped
labels, blank cells and the original four cells in `cells_json`. `brand` and
`make` are separate columns; neither is relabelled manufacturer. Foreign
certification strings are unverified references, not fetched grants. The raw
registers are included in unified `lookup` as `public_registers`, with source
URLs and separate coverage notes. Counts are rows, not unique hardware models.

```sh
python3 -m devicehunt lookup 'F100 Ghost'
python3 -m devicehunt lookup 'TA/2014/0074'
python3 -m devicehunt sql 'SELECT source_id, count(*) AS rows FROM public_equipment_rows GROUP BY source_id'
python3 -m devicehunt sql "SELECT brand, model, foreign_certification FROM public_equipment_rows WHERE foreign_certification LIKE '%HDC%'"
```

The Mauritius page's public table configuration advertises an All option
(`length=-1`) and the read-only `get_wdtable` request for table 57. Both downloads
used no global or column filters. The original page, client script, request body
and responses are retained under `data/raw/mauritius-icta-*`. No table editing,
account, seller contact or private endpoint was used. The downloadable table and
its cached HTML preview differ in whitespace: the API preserves a trailing space
in the type cell for TA/2014/0074. Preview comparison collapses whitespace only;
stored API cells remain verbatim. Full historical coverage and current approval
validity are not established by matching the returned totals.

`devicehunt/public_registers.py` validates source URLs, hashes, table IDs,
headings, row widths, expected counts and the two API responses before replacing
that source's raw rows. It deliberately does not split model lists or infer
catalog identities. Schema version 24 adds the raw table. The two bundles are
included in the offline rebuild manifest; no network access is needed to replay
them. Tests exercise malformed captures, partial/filtered results, hash failures,
whitespace, Unicode/literal search, duplicate preservation, source isolation and
the absence of automatic identity or manufacturer assertions.

The initial NTRC browser-text fetch timed out; a normal download of the same
public URL succeeded. No access denial was bypassed. The Celestica documentation
portal parent page redirected to an upgrade-browser page during this review;
no content from that page was imported.
