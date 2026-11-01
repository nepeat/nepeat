# oracle x7-2c official-content provenance — 2026-10-04

outcome: complete for this one-document task. the saved VPAT is byte-identical to an official Oracle public documentation capture from [t2-6972](https://docs.oracle.com/en/corporate/accessibility/templates/t2-6972.html). the historical request URL and acquisition time for the old file remain unrecorded; this establishes official content provenance rather than inventing historical fetch metadata.

one exact search and one original public GET; HTTP 200 at 2026-10-04T23:00:28.889695+00:00. both captures are 36,063 bytes with SHA-256 `171c447c850db4a57f6f151db20fac52911280ded6d97a9145d33f850ce71fc6`. original `data/raw/oracle-x7-2c-vpat.html` was not changed. new bytes: `data/raw/networking-primary-oracle-2026-10-04/oracle-x7-2c-vpat-t2-6972.html`.

## proposed input and evidence boundaries

[candidate-curated.json](candidate-curated.json) uses the existing curated format, one vendor-tier source, one exact model `Oracle Server X7-2c`, and 25 source-scoped facts. printed report name `Oracle Server X7-2c 1.0` and report date `23-Aug-2017` remain literal facts. report title's `1.0` is not inferred to be an installed firmware release. catalog identity comparison casefolds models; lower-case printed `c` routes to the existing `Oracle Server X7-2C` identity without a new alias. review this routing before integration.

SM 1U and SM 2U descriptions are separate facts and complete literal sections. each lists 24 DDR4 DIMM slots, Pilot4 management and internal USB. the report distinguishes one front x16 low-profile slot/two drive bays in SM 1U from up to five front slots/four drive bays in SM 2U. literal memory-channel wording is retained. processor section text `150W Max` and `205W Max / 165W Max for GPU` is processor-related; no PSU rating, input current, voltage or measured draw is asserted. variant-specific names remain fact scopes, not fabricated aliases or standalone device identities.

Oracle is retained as brand/platform vendor; no physical manufacturer, deployment or owned-unit configuration claim. no X7-2 specs transfer. dependency list describes dated accessibility relationships and establishes no current OS compatibility, installable firmware access or resale entitlement. the report directs Product Notes readers to My Oracle Support; that gated URL was not requested. PSU input/connectors/count, fans/airflow/noise, rails/depth, exact owned-unit variant, credentials and standalone bootability remain gaps.

## checks and next step

[capture ledger](capture-ledger.json), [query ledger](query-ledger.json), [comparison](comparison.json) and [verification](verification.json) record requests and acceptance checks. both byte equality and normalized visible-text equality pass, old hash agrees with prior findings, candidate JSON is valid, snapshot exists and both variant headings are present. snapshot path is relative to the staged candidate; moving the candidate to `data/` requires an integrator to update that path to `raw/networking-primary-oracle-2026-10-04/oracle-x7-2c-vpat-t2-6972.html`.

no database access, imports, code changes, manifest changes or generated exports. coordinator may independently review this candidate, appoint the exclusive integrator and follow the normal staged import/rebuild workflow. stop condition reached: exact official report verified; no further source search needed for provenance.
