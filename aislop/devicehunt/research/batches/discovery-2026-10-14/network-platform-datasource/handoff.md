# network platform datasource — review handoff

updated 2026-10-15 UTC. author `/root/network_bulk_full`; root owns acceptance,
shared integration and current project documentation. this supersedes the work
state in [the ownership-transfer checkpoint](resume-full-access.md), preserving it
as history. actual permissions here: danger-full-access, network enabled,
approval never; no escalation flags used.

outcome: **BISDN implementation complete; Cumulus matrix unavailable.** no
production code, database, manifest, exports or shared source collection changed.
no active tool handles remain. no installs, publication or further delegation.

## delivered

- [hcl.py](hcl.py): deterministic offline HTML-table reader, strict BISDN adapter,
  source-scoped curated-candidate generator and read-only novelty comparison.
  preserves row order, duplicates, exact complete cells, row/download links,
  qualifiers, printed headings and line/table/row references. changed headers,
  missing cells, mismatched attributes/links, spanning cells, nested/truncated
  tables and missing matrices fail closed. heading contexts can preserve current
  and retired sections for a future source adapter; no Cumulus rows are invented.
- [capture.py](capture.py): separate bounded acquisition helper, inherited and
  improved. ten **actual HTTP exchanges** are counted, retaining all failed
  attempts in the ledger. any response still counts if its body fails/oversizes.
  existing capture filenames cannot be overwritten after an interrupted attempt.
  host allowlist, robots gates, denial stops, no automatic redirects/retries,
  hashes and 8MB cap remain. support.cumulusnetworks.com was explicitly added
  solely for the literal captured newer-list link, then stopped on DNS failure.
- [bisdn-matrix.json](bisdn-matrix.json): the complete captured 11 rows, all
  vendors included: Celestica 1, Delta 2, Edgecore 8. printed release is **6.0.0**.
  the fixture is a literal table slice from immutable body SHA256
  `281d4717ae1003e3843ab4bd0eb1ccc0564f5bc729d1ab9d4b1095a6fda1635f`.
- [candidate.json](candidate.json): **1 source / 11 listing records / 77 facts /
  11 source-specific identifiers / zero company affiliations**. brand is
  `BISDN Linux HCL`; kind is
  `network OS compatibility listing (software reference)`. each exact whole
  platform cell remains one listing; no parenthetical aliases, physical hardware
  identity claims or manufacturer roles are inferred. source ID is
  `bisdn-linux-hcl-2026-10-14`; snapshot is the unchanged saved HTML body.
- [novelty.json](novelty.json): complete exact-string model/identifier comparison
  against read-only live schema29 database SHA256
  `0879a77f5d2cdf4edfdce3a88ab03034504c02f99b4645537f43af5db5c32853`,
  verified unchanged. five printed platforms have exact matches (four separate
  AS4610 variants and AS7726-32X). six whole cells have no exact match: Questone 2
  D3030 (Questone 2A), AG5648, AG7648, EPS201 (AS4630-54TE), EPS202
  (AS4630-54PE), DCS201 (AS5835-54X). absence of a whole grouped cell does not
  establish a new hardware identity. lowercase-only leads are separately
  reported; no substring/alias expansion or identity merge.
- [source-status.json](source-status.json), [validation.json](validation.json),
  [test-results.txt](test-results.txt), [tests](tests/), [validate.py](validate.py)
  and [SHA256SUMS](SHA256SUMS) bind outcomes and reproducible review inputs.

## acquisition and limits

[requests.jsonl](requests.jsonl) retains **8 attempts, 4 actual HTTP200 exchanges,
4 DNS failures**. six of six web queries were used by the previous owner; no new
searches occurred here. six of ten HTTP exchanges remain unused. raw bodies,
headers and original receipts were not modified. all eight receipts and eight
body/header files were hash-verified.

Cumulus's captured public page is a Vue shell, not the actual matrix. its literal
API hostname failed DNS in the prior worker. the literal newer Cumulus4 support
article's robots hostname also failed external DNS here (seq8); no article or API
request was made, no embedded frontend authorization header was used, and no
retry followed that failure. current and retired row counts are **unavailable**,
not zero. no denied route, mirror, alternate API or per-device crawl was pursued.

BISDN download URLs were captured as links only; no images/firmware were fetched.
ASIC, architecture and release are table-row statements, not installed OS,
current support, entitlement, unit configuration, physical manufacture, stock or
safe flashing guarantees. no retirements are inferred from absence. existing
ONL/SONiC/ONIE/PICOS/DENT sources were not duplicated. vendor/ODM owners were
informed of the scope; their primary catalogs remain separate.

## checks and reproduction

**21 offline unittest cases passed**, including real capture/fixture comparison,
complete suffix/qualifier retention, duplicate rows, heading contexts, changed
schemas, hash validation, exact-versus-substring novelty, missing DB rejection,
robots denial, immutable collision recovery and actual-HTTP budget accounting.
network acquisition is mocked in tests; no network tests ran. four isolated
integration cases prove every candidate listing is recognized by unchanged
production `is_software_reference`, excluded from default `match-listing`, shown
only as `software_reference_mention` with explicit opt-in, and given no shopping
links. the kind's explicit `software reference` text uses the existing rule;
neither a brand-list change nor other routing edit is required. the previous
bare `network OS compatibility listing` kind failed that classification check
and was corrected before handoff.

`prepare_curated` passed. [validate.py](validate.py) imported and replaced this
source twice into a new temporary isolated schema29 catalog, each reporting 11
records; final counts are exactly 1 source / 11 records / 77 facts / 11 identifiers
/ 0 affiliations. integrity `ok`, foreign-key violations zero, Questone FTS hit1.
the scratch database was temporary; live input was read-only. full production
suite/rebuild has not run because no production importer/schema was changed or
candidate promoted. review acceptance must precede any integration.

from project root:

```sh
python3 -m unittest discover -s research/batches/discovery-2026-10-14/network-platform-datasource/tests -v
PYTHONPATH=. python3 research/batches/discovery-2026-10-14/network-platform-datasource/validate.py
```

to regenerate the three deterministic outputs (overwrites only these owned
generated files; requires the same stable catalog hash for identical novelty):

```sh
python3 research/batches/discovery-2026-10-14/network-platform-datasource/hcl.py --database data/devices.sqlite --output research/batches/discovery-2026-10-14/network-platform-datasource
```

## proposed integration

root should independently review the complete table against all 11 parsed rows,
candidate identity semantics and source-specific code/tests. if accepted, use
the existing curated importer with the unchanged HTML capture as its snapshot;
no new production importer or schema is necessary. any manifest registration,
new candidate/rebuild paths, combined suite, preservation checks, promotion and
exports belong to root's exclusive sequential integration workflow.

the hardware identity fields intentionally describe a compatibility listing.
mapping table labels to hardware brands/aliases or company roles requires a
separate reviewed source and is outside this proposal. no further Cumulus DNS
retry or broader acquisition is a required next step; only a genuinely new
official bulk matrix source would justify reopening that unavailable family.
