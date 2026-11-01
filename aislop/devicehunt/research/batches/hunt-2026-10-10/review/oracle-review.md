# oracle_practical review (independent, Claude Sonnet 5.5) - 2026-10-10

Verdict: ACCEPT with notes (one must-fix on snapshot placement is an integrator action, not a content defect).

Method: sha256/bytes of all 7 ledger entries vs files; extracted PDF text with pypdfium2 (`python3 -I`) and HTML text; checked every one of the 15 fact rows' literal quote on its cited page/capture; checked coverage text of the 5 sources against the captured policy PDF/FAQ/release notes; read importers.curated(); 1 re-fetch (concept-servers.html, sha256 identical to ledger).

## Verified
- Ledger: 7/7 sha256 and byte counts match the files; all HTTP 200; handoff hash list matches files (incl. facts JSON 1625d5c8..., ledger 6be23f49...). The 2 extra on-disk files (ledger, facts JSON) are not ledgered, as stated.
- Facts: 15/15 quotes present verbatim on the cited PDF page (p.4, 69, 69, 70, 70, 76, 76, 76, 78, 77, 74, 74) or in the cited HTML (concept page quote needs tag-stripping: `<span>NVIDIA L40S GPU</span>s`). Each fact value equals its evidence quote. "Page N of 12 = PDF page - 68" holds (p.69 = "Page 1 of 12").
- Identity: only `X10-2c GPU L40S Compute Server` is used (model/title/key); plain `X10-2C`, E4-2C, E5-2C, X7-2C are never asserted; every source coverage disclaims transfer. Existing DB already has separate identities `Oracle Server X10-2c`, `... X10-2c 3U`, `... 3U GPUv4 Control Head`; the new model casefolds to a distinct device id, so no accidental merge.
- Policy PDF p.7 "Unsupported Hardware Systems" wording is exactly as the coverage paraphrases (entitlement, parts transfer, replacement media). The FAQ sentence appears twice as claimed. Release notes (F74805-44) contain no X10-2c/E4/E5/X7 string and no per-server BIOS/ILOM version, as claimed. Source ids are unique vs the DB; `documentation_vendor` is an existing role.
- Absence claim (no resale/second-hand/ownership-transfer clause) holds in my own search of all 33 pages (hits only for technical-contact transfer, title to returned parts, hardware relocation, reinstatement "not purchased at the time you acquired").

## Findings
1. MUST-FIX (integrator placement) - `oracle-practical-facts-2026-10-10.json` snapshots are `raw/hunt-2026-10-10-oracle/<file>`. `importers.curated()` resolves `Path(path).parent / snapshot`. Left in `data/raw/hunt-2026-10-10-oracle/` it resolves to `data/raw/hunt-2026-10-10-oracle/raw/hunt-2026-10-10-oracle/...` and `read_bytes()` raises FileNotFoundError. Required: copy the JSON byte-identically to `data/` (e.g. `data/oracle-practical-facts-2026-10-10.json`, manifest path with no `raw/` dir prefix), then all 5 snapshots resolve to existing files. Do not edit the JSON in place (its sha256 would diverge from the handoff).
2. SHOULD-FIX - `psu_redundancy_as_printed`: quote `provide fully redundant "N+N" power` drops the preceding qualifier; page 76 reads "The PSUs are designed to be hot-pluggable, and in most cases, provide fully redundant "N+N" power". Stored without "in most cases" it reads stronger than printed. Extend the quote/value to include "in most cases,".
3. SHOULD-FIX - policy coverage text says a text search for "second-hand/resale/used found none". "used" does occur (p.2, 4, 9, 20: "maltreated or used in a manner...", "unused support"); none concern resale. Reword to "no resale/second-hand/ownership-transfer clause found; unrelated uses of 'used' excluded". The absence conclusion itself is correct and properly disclaimed (not permission/prohibition).
4. NOTE - `airflow_as_printed` holds only the failed-PSU-slot sentence (p.77); the name could be read as chassis airflow direction. Coverage correctly says no airflow direction; consider a more literal name (e.g. psu_failed_slot_airflow_note).
5. NOTE - `chassis_height_as_printed` on the concept page stores the whole sentence (3 RU + Xeon Platinum 8480+ + four L40S GPUs + 48GB), so CPU/GPU spec rides under a height label; concept sentence also continues "and 1466 peak FP8 TFLOPS" (omitted, fine). Rating-vs-measured: PSU "1400W" and "200-240 VAC" are printed ratings and are labelled as_printed; no measured claims.
6. NOTE - back-panel facts keep one callout each (`Power supply (PS 0)`, `AC power input connector (PS 3)`, `High speed fan module 2 (FM 2)`); the handoff prose "PS0-PS3 / FM0-FM2" is the aggregate of these pages, not a stored fact. Acceptable; the integrator should not describe the DB as storing four PSUs as one fact.
7. NOTE - HTML PSU duplicate is correctly labelled "not independent corroboration"; its visible text names no model (slug only); coverage says so. Handoff decisions 1-3 remain open; do not apply PSU/voltage facts to owned `X10-2C`.

## Fix verification (after author fixes; curated sha256 06b99208...)
- Facts JSON sha256 = 06b99208256c5618d01a47b19808c5160288eee6db722ea251d62e76da37f051; ledger (6be23f49...) and all 7 captures unchanged and still match.
- Finding 1 (snapshots): now bare filenames; all 5 resolve relative to the JSON's own directory when it stays in `data/raw/hunt-2026-10-10-oracle/`, and each file's sha256 equals its ledger hash. Integrator manifest path: `raw/hunt-2026-10-10-oracle/oracle-practical-facts-2026-10-10.json` (do not copy to `data/`; a copy would break the snapshots). Fixed.
- Finding 2: `psu_redundancy_as_printed` = `in most cases, provide fully redundant "N+N" power`, found on p.76. Fixed.
- Finding 3: policy coverage now says only that no resale/secondhand/ownership-transfer clause was found on the captured pages, as absence not permission/prohibition. Fixed.
- Findings 4-5: renamed `failed_psu_slot_airflow_note_as_printed`; concept sentence split into `chassis_height_as_printed` ("3 RU server"), `processor_as_printed`, `gpu_as_printed`. Fixed.
- Re-check: 17/17 quotes found on the cited PDF page / HTML text; every fact value equals its evidence quote; evidence hashes equal the ledger.
- Temp-catalog import (scratch sqlite, `importers.curated`): 5 sources, 1 device, 3 records, 17 facts, integrity ok, foreign_key_check empty, every source sha256 equals the file's sha256.

Updated verdict: ACCEPT. Open items are the author's decisions (is Oracle X10-2c GPU = owned X10-2C; device-less policy sources), not defects.
