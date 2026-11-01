# Joy Technology and the historical Accton portfolio

The cached official Anatel register identifies **Joy Technology (Shenzhen)
Co., Ltd.** as manufacturer of **OmniSwitch 6855-24**, approval
`005331103709`, CSV record 21008. The raw approval date is `13/10/2017`.
The applicant is `ALCATEL-LUCENT BRASIL TELECOMUNICACOES LTDA.`. The complete
published archive and its two-download integrity checks remain unchanged.

Accton's [September 26, 2024 announcement](https://www.accton.com/accton-technology-announces-strategic-move-with-digital-china-group-to-reshape-market-presence-in-china/)
names Joy as a subsidiary in a planned transaction. A later
[consolidated financial filing, note 32](https://www.accton.com/wp-content/uploads/2025/10/20251028075444.pdf)
reports disposal of its 100% interest in Joy, with completion and control
transferred on April 30, 2025. These are dated corporate facts, not evidence
that Accton owned Joy when the 2017 approval was issued.

Both corporate statements were available in public web-search text. Requests
for the original HTML and PDF returned HTTP 402 and were not retried. The
saved company evidence is a selected factual extract, explicitly not original
document bytes. The review log also records an unavailable 2023 annual report.

The new record preserves Joy as the manufacturer and identity namespace, with
no parent target assigned to that manufacturer role. Accton has a separate
`historical_subsidiary_portfolio` affiliation. This makes the device discoverable
under Accton in all-association searches without adding it to Accton's direct
manufacturer inventory. There is no inferred retail-brand equivalence,
model alias, factory identity, hyperscaler deployment, current subsidiary
status, certification validity, or individual auction-unit history.

## Search locally

```sh
python3 -m devicehunt search --vendor Accton --role historical_subsidiary_portfolio
python3 -m devicehunt lookup 'OmniSwitch 6855-24'
python3 -m devicehunt vendor-inventory Accton --scope all --limit -1
```

```sql
SELECT d.model, a.company, a.role, r.source_url
FROM records r
JOIN devices d ON d.id=r.device_id
JOIN affiliations a ON a.record_id=r.id
WHERE r.source_id='anatel-accton-historical-portfolio'
ORDER BY a.role;
```

## Reproducibility and limits

`anatel-accton-portfolio` validates the hashed registry bundle, complete Anatel
archive and dated company evidence before replacing its own source. It selects
only the reviewed whole Joy manufacturer name (case-insensitive, outer-space
trimmed), retains duplicate approval rows independently, and excludes unrelated
or composite company names. The raw Anatel source is not rewritten. Reimporting
a changed snapshot withdraws absent claims.

The import manifest includes the new bundle, corporate extract and review log.
Tests cover role isolation, dated facts, duplicate rows, refresh withdrawal,
composite-name exclusion and rejection of invalid evidence before replacement.

Anatel record 11896 names `Accton Technology (China) CO. LTD`, model
`OS-LS-6248`, approval `001960905130`, raw date `06/10/2017`. This record remains
available in raw-register searches. The corporate documents use differing
Accton China / Accton Technology legal names; this update does not equate them.
That company mapping still needs unambiguous evidence.
