# independent owned-buying review — 2026-10-04

## stage 1: oracle evidence

outcome: one correction needed before accepting the Oracle handoff. reviewed only
the stable Oracle researcher note, four findings files and helper against saved
captures and read-only catalog records. engineering and networking remain outside
this stage.

### actionable finding

- **[P2] retain NIST's single-user-mode restriction.**
  `oracle/e4-2c-findings.json`'s newly reviewed NIST statement ends at
  `without PAA`, and `oracle/review_saved.py` constructs that shorter text and
  merely checks it as a substring. The saved `data/raw/nist-4506.html:584`
  actually says `Windows Server 2022 running on Oracle E4-2C server with AMD EPYC
  7J13 without PAA (single-user mode)`. Omitting the qualification broadens the
  tested configuration. Preserve the entire list-item text in the helper and
  regenerated findings, and include the restriction in the note's NIST summary.
  The with-PAA list item at line 583 has no such qualification.

### checks and accepted boundaries

- independently matched all four exact identity rows, record ownership/source
  keys and imported fact lists to the live read-only catalog.
- independently verified all 11 record capture references and additional
  positive-statement capture hashes; Broadcom detail JSON equals the corresponding
  catalog raw detail, and all three BIOS rows per model match saved fieldValues.
- read Broadcom 53494/57328/57094's actual Model Details notes: E4 redirects SD/USB
  considerations to KB 85685; E5 and X10 explicitly say SD/USB flash-media boot is
  unsupported in this compatibility scope. No firmware entitlement or whole-unit
  bootability is established. BIOS/release pairings remain uninferred.
- inspected Oracle OSS X7-2C heading and four adjacent archive links in
  `data/raw/oracle-oss-x86.html:2406`; the handoff correctly limits these to
  source-code archive listings, separate from installable firmware.
- inspected RRA `detail-031.html`: X72c2U is the main model; Oracle Server X7-2c
  is a derived model alongside a different OCI E1-2c identity. The handoff neither
  merges these models nor treats the regulatory spelling as chassis dimensions.
- inspected saved X7 VPAT product descriptions: separate SM 1U/SM 2U layouts,
  23-Aug-2017 date, CPU watt statements and dated dependent products are faithfully
  retained. Original URL remains absent in the findings; provenance-incomplete
  material is explicitly excluded from import readiness. No alias, entitlement
  or general OS-compatibility inference is made.
- inspected X10 licensing capture: edk2-stable202208 and BSD-2-Clause-Patent
  appear in notices; the handoff correctly avoids download/resale entitlement.
  E5 benchmark data without a registered saved capture is excluded from reviewed
  positive claims. Nearby suffix/variant identities receive no borrowed facts.

live database SHA-256 independently checked after review:
`ea7d121849c34cac801c178d48be80ea5f3af09958b526228b6b6f536d0662d8`.
no network, imports, package installation or full-suite execution. Only this
review report was written. Recheck the NIST correction before stage-1 acceptance.

### stage-1 correction recheck

accepted: the corrected helper compares exact relevant NIST list items rather
than matching a shorter substring. Corrected E4 findings preserve `(single-user
mode)`, and the Oracle note repeats the restriction. Independently compared the
two findings statements to complete saved HTML list-item text and verified the
coordinator's three expected corrected-file hashes. Stage 1 is now accepted.

## stage 2: networking, report code and supporting documents

outcome: code and networking accepted; one documentation correction pending.

- **[P2] use the actual literal model in the direct SQL example.**
  `docs/buying-reports.md` filters `device_facts.model = 'E5-2C'`; the catalog's
  literal model is `Oracle Server E5-2c`. Read-only execution confirms the shown
  predicate returns zero facts, while the full literal name returns ten. Change
  this SQL example to the full model name. The report CLI accepts the short
  record-local alias correctly; direct SQL against this view does not expand it.

Reviewed `devicehunt/buying.py`, its CLI route and six affected tests. Exact
casefold comparison retains suffixes/plus signs, alias/part-number matching selects
only qualifying records, vendor filtering applies to the same record, conflicts
and source context stay visible, and unmapped categories describe mapping gaps.
Marketplace-only categories keep seller-only status; mixed categories retain
per-statement seller labels. The CLI opens a read-only URI; absent databases fail
without creation. All six affected tests passed independently.

Reviewed all 22 networking record raw fields/imported facts against the live
read-only catalog, and independently hashed 34 capture references. Directly
viewed original 7760 pixels: the adjacent AS7760-32X label carries the stated
100-240~ marking, separate from measured draw/PSU inventory; photo firmware and
full-code mapping stay unit observations. Saved seller Details contains all three
proposed statements; these remain historical ecrunch seller claims and do not
become claims about the owner's unit or the eBay photograph. Saved RRA
`data/raw/rra-celestica-official/detail-032.html:135` contains the FSJ021/Acbel
statement in its other cell; association is not promoted to an exclusive PSU
requirement. Separate 48V/-12V/full-code variants receive no borrowed facts.

Reviewed coordinator `generate_reports.py`: complete baseline supplies query
parameters only, subprocess CLI queries are read-only, output creation is
exclusive, existing snapshots are preserved, and before/after database hashes
are checked. Reviewed rewritten `research/owned-hardware-gaps.md`: document/photo
distinction, exact identifiers, ByteDance gap and denied-source boundaries remain
explicit, with preserved history linked. No actionable issue in those files.

Database SHA remains the stage-1 checkpoint. No full-suite run or shared-state
write by this reviewer. Final PROGRESS/docs acceptance awaits its separate stage.

### stage-2 correction recheck

accepted: the documented SQL now uses `Oracle Server E5-2c`. Executed the exact
SQL extracted from the corrected document against read-only live SQLite and
confirmed ten rows. Scoped `git diff --check` could not execute because the
system Git launcher requires Xcode license acceptance; no system change was made.
Code and both evidence
handoffs are accepted for the coordinator's remaining verification/documentation
work; neither stage has an unresolved actionable finding.
