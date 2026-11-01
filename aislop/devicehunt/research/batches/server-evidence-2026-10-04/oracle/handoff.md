# nist 4506 saved-source review

outcome: complete; one saved source reviewed offline, no requests, source edits,
imports, code changes or shared writes. author: Oracle research worker.

- `review.json` retains actual live source metadata, all four original records
  including exact raw_json, facts, roles and identifiers; all fifteen literal
  tested-platform list rows with ordinal/raw HTML; and both original validation
  history rows. `review.py` reproduces this review using read-only SQLite.
- `candidate-curated.json` proposes one additive source record and sixteen facts
  on the existing `Oracle Server E4-2C` identity. Its snapshot path is deliberately
  `raw/nist-4506.html`, relative to the eventual `data/merged.json` (or another
  curated input directly in `data/`), not the research artifact location. Do not
  import this research-path file directly without arranging that relative path.
- original capture SHA-256:
  `47bf103fab0e61eb214ac5ad17bae892951663a320d2bed427e8bf22e5455064`.
  URL and fetched timestamp are recorded verbatim in review.json. No raw copy made.

## supported detail

The current `nist-4506` source already has four records. Its E4-2C raw_json already
preserves these two literal statements; the structured facts currently contain
only certificate 4506 and the tested-software-module scope warning:

1. Windows Server 2022 running on Oracle E4-2C server with AMD EPYC 7J13 with PAA
2. Windows Server 2022 running on Oracle E4-2C server with AMD EPYC 7J13 without PAA (single-user mode)

The proposed facts expose those two rows without dropping the PAA/single-user
modifiers. They add the Oracle OpenSSL FIPS Provider module's software type,
FIPS 140-2 standard, version 3.0.15, saved Historical status, sunsetting reason,
FIPS-mode/key-strength caveat, literal module vendor and validation history:
`5/3/2023 | Initial | Acumen Security`; `3/26/2025 | Update | Lightship Security, Inc.`.
These are module-level statements; neither history date is assigned to a specific
platform row. PAA is preserved literally without inferring its expansion.

Precise locators in the immutable HTML: Details panel `#module-name` and
`#module-standard`; sibling labeled rows Status, Historical Reason, Caveat and
Module Type; Tested Configuration(s) list items 14 and 15 (lines 583–584);
Software Versions label/value (lines 685–689); Vendor panel Oracle Corporation
link; `#validation-history-table` tbody rows 1 and 2 (lines 742–751).
There is no tested-configuration table ID; the page uses an unlabeled-id `ul`.

## preservation choice

Recommended: keep existing `nist-4506` importer/job and all four records unchanged;
add distinct source id `nist-4506-e4-2c-reviewed-detail-2026-10-04` with the same
original capture URL/hash. The new source is an explicitly labeled extraction of
the same evidence, not independent corroboration. Exact existing Oracle brand,
model, title and kind should reuse identity `b5decfa477ec728d6252`. A new
source/record is necessary under the current curated replacement behavior if all
old rows, including raw_json, must remain exactly intact: curated import serializes
the curated device object as raw_json, whereas the existing raw_json is solely
`{"tested_configurations": [...]}`. Replacing the original source would also risk
its other three records if omitted. The proposal avoids reimporting identities,
certificate and existing scope facts; the repeated tested_platform_vendor role
only scopes this new record and establishes no new company relationship.

Alternative: a separately reviewed importer enhancement could add facts to the
existing source while passing exactly unchanged original raw_json for every old
record. That requires coordinator-owned code/tests and preservation proof; this
research did not implement it. A curated same-source replacement is not acceptable
for strict old-row preservation, even if it carries all four identities.

## exclusions and checks

This evidence does not establish general or current Windows/OS support,
installed CPUs/OS, safe standalone boot, auction-unit hardware certification,
manufacture, hyperscaler deployment, firmware access, or transferable support/
software entitlement. Other platform rows stay context only; no ILOM processor
or SPARC/X9/A1 configurations transfer to E4-2C. Oracle Corporation is the software
module vendor; retain Oracle's existing tested_platform_vendor role, not an OEM role.
Saved Historical status is a snapshot fact, not a live recheck.

Checks passed: registered capture hash matches; original source has four records;
all fifteen original platform rows preserved; both E4 literal rows equal original
record raw_json; exact identity matched; database SHA unchanged before/after review.
No importer/tests were changed or run. Candidate JSON parses and has sixteen facts.

Next bounded step: independent review of these three artifact files against the
saved HTML and actual live rows, then coordinator acceptance of the preservation
choice. One designated integrator may merge the candidate into a data-root input,
rebuild and prove zero new identities, one new source/record, sixteen added facts,
all original rows intact, source/candidate/rebuild equality and unchanged PDF data.
No additional source hunt is required for this finished one-source review.
