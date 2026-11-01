# QCT D52BQ-2U CCL — independent discovery review

reviewer `/root/handoff_audit`, 2026-10-13 05:08 UTC. **ACCEPT discovery; no correction needed.** root-authored handoff SHA `896c61de2dd8cbc3593a99bc048fa97224a64fdb3dc6350a72ac8f8665254123`. this accepts immutable acquisition and the eight selected PSU rows only; no candidate/import, component identity, manufacturer role or bare/prefixed model bridge is accepted.

verified all twelve SHA256SUMS entries, the saved product-page basis hash `d5b0fd941c02b83842157ce5e9a4ad8e188fe5abe4bd6e9e07a57fe927061455` and literal compatibility href. original HTML SHA `9c377013ee3bde686ae7372eb45aeadef00361b1ebe1f81a67bd872b5379d8b8`; observations SHA `437bebc720b1e71bf74646166b245d8addc33c43feeb390975c5fc3e98d2d2e9`. [independent check script](qct-compatibility-review-check.py) and [results](qct-compatibility-review-checks.json) retain the audit evidence. acquisition/observation helpers were read but not rerun.

the ledger contains exactly two HTTP GETs, fresh robots then `https://www.qct.io/microsite/ccl/D52BQ-2U.html`, both200 with one matching raw status header each. body lengths and body/header hashes match; no Location, curl error, login marker, auto-redirect or retry. argv specifies zero retries and no redirect-following option. fresh wildcard robots permits this HTML path with no declared delay; policy hash `3cae06309b0823f56f465ac59f249a15844f17ba45496e0f90f35cbcde81dae3` matches ledger/result. PDF/download/internal-registration restrictions remain outside scope; no additional fetch is implied by allowed HTML. prior D51B and T41S404 routes and their captures were not retried or used as alternate copies of this item.

a separate stdlib HTML parser exactly reproduces **15 tables /892 rows**, including row/cell tags, every original attribute list, text fragment, nested element and source start-line. source bytes were decoded directly: universal newline conversion would otherwise alter the regional note's retained CRLF fragment. the selected table is **12 (1-based), PSU CCL**, rows3–10. all eight rows have exactly these nine headers and correctly aligned cells:

`Power Capacity | Type | Efficiency | Dimension(mm) | 240VDC | Cold redundancy | QuantaPN | VendorPN | Note`

| QuantaPN | VendorPN | capacity / efficiency | 240VDC |
| --- | --- | --- | --- |
| AF800C00002 | MC800B6-3-4R-02 | 800W / Titanium | Yes |
| AF800C00030 | MPS8146 | 800W / Titanium | Yes |
| AF800B00009 | MC800B4-3-4R-02 | 800W / Platinum | Yes |
| AF800B00019 | MC800B4-3-5R-02 | 800W / Platinum | Yes |
| AFC00F00019 | MC1200B4-3-4R-02 | 1200W / Platinum | Yes |
| AFC00B00032 | PS-2122-7Q | 1200W / Platinum | Yes |
| AFB00E00003 | FSF048-930G | 1100W / Gold | Yes |
| AFB00E00005 | FSF048-931G | 1100W / Gold | No |

every row separately prints Type `Redundant`, Dimension(mm) `86*196*40` and Cold redundancy `Yes`. preserve that dimension order without assigning unprinted axes; redundancy labels establish no installed PSU count or summed capacity. `240VDC` is its literal Yes/No column, not an AC/DC input range, connector/pinout, safe wiring or standalone-operability rating. both part-number columns remain separate paired source values; suffixes and their associations cannot be inferred outside these rows.

all selected rows use `form01`, with cells `style4`; the exact styles and footnote “*Grayed-out items: to be EOL soon or already EOL” are retained. CSS colors/style classes do not establish current EOL, availability or certification. FSF048-930G alone carries the historical USA/EU regulatory statement and scheduled end-Q2-2018 Taiwan/China/Russia implementation; the 2025 printed date does not establish that schedule's completion or current compliance. blank Note cells stay blank.

the page prints **Quanta Cloud Technology**, **D52BQ-2U** and publication **2025-06-27**. its CCL disclaimer describes a reference compatibility list for channel partners/system integrators, based on QCT testing, while withholding a third-party component quality/reliability guarantee and stating it does not replace the full hardware certification program. keep that qualification with any later fact. none of the other fourteen component/OS tables was selected as new evidence by this review.

three bounded read-only catalog query cases confirm the exact CCL source URL is absent and two full parent identities remain separate: `QCT / QuantaGrid D52BQ-2U` id`f65a474568c484317fc5` and `Quanta / QuantaGrid D52BQ-2U` id`0f3c305cd28ca9b6f7f3`. the QCT identity already has the exact source product URL that publishes this CCL href; the Quanta identity has separate regulatory/benchmark/certification records. current DB SHA `fc88dca7e8b6f23aa8078134be767c22fed902303c2880ee923ad1d55952e3bf` stayed unchanged during checks; this is a dated checkpoint while root integrates other batches.

candidate boundary recommendation, subject to separate routing/candidate review: one source snapshot and eight **literal CCL row facts on the existing QCT parent** can be useful, with the saved full-name product link explicitly explaining the bare table heading and the disclaimer/date/raw headers/styles/notes retained. use a source documentation/compatibility association only if the catalog role contract permits it. do not create standalone PSUs or new aliases, route to the separate Quanta identity automatically, infer manufacturer from MC/MPS/PS/FSF strings, or treat QuantaPN as VendorPN/model equivalence. full AC/input/output/pinout/airflow specifications would require independent original evidence. no candidate was built here.

only reviewer artifacts were written. no network, import, production/shared-data edit or delegation occurred. finite stop condition met; root owns subsequent candidate scope and acceptance.
