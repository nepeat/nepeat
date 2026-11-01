# milestone-0 independent review

date: 2026-10-04. reviewer: `/root/archive_pilot_review`, GPT-6.1 Sol.
result: **pass for the bounded offline inventory/design milestone**. no blocking
findings or requested fixes. coordinator retains acceptance authority.

## checks

- read project instructions, goal/progress, workflow/evidence policy, pilot note,
  boundary, inventory/helper, relevant archive-plan sections and existing
  `reference_documents.prepare` / `documents.dependency` / `read_pages` code.
- ran `PYTHONPATH=. python3 -B research/batches/artifact-archive-pilot-2026-10-04/pin_inventory.py --check`:
  exit 0; 47 documents, 94 dependency paths/unique dependency artifacts,
  3,589 pages, 3,575 pages with text and six supporting provenance files.
  full preparation and live reference PDF/page projection checks passed.
- independently hashed all 103 pinned paths (manifest, complete bundle, live
  checkpoint, 94 dependencies and six provenance inputs); matched recorded hashes
  and repeated hashes after review with identical results. live database matches
  PROGRESS SHA-256 `a098c390431efd77d25f162acb039d2ddd938ff5effeffd7e93fa9b0bc34ff16`.
  inventory digest matches boundary:
  `d7be503676cb832824d4e0fd33e5390417cc7af55c476027563d75391c11b327`.
- independently compared all 47 preserved entries and snapshot JSON-pointer keys
  with the selected bundle, PDF/text hash associations and proposed derivation
  edges. acquisition times, actual request/final URLs, collector/tool versions
  and missing legacy metadata remain unknown rather than fabricated.
- read saved Ingrasys provenance, capture ledger and handoff; independently
  reproduced the reconstructed PDF hash from the original malformed body span
  `[4128654,8355366)` plus hex `460d0a`. both body locations remain distinct;
  the derivative has no downloaded-original capture artifact assignment. all ten
  saved embedded-text pages remain blank. saved HTTP 200 is not promoted into a
  valid-original or completeness claim.
- checked local links in the pilot note, boundary and PROGRESS; all resolve after
  this assigned report is created. PROGRESS correctly distinguishes the planning
  pilot from pending SmartNIC/QCT integration and the accepted live checkpoint.
- inspected the helper's read-only execution path: `--check` does not open an
  output writer, SQL uses `mode=ro` and only SELECT, and preparation only reads
  inputs. default creation uses exclusive `x` mode. in-memory fault injection
  of a missing pinned dependency raised `FileNotFoundError`; the same read-only
  SQLite URI mode rejected a nonexistent database without creating a file.

## evidence and authority

the boundary keeps immutable bytes/receipts as evidence authority, a future
isolated registry as an acquisition index, frozen reviewed releases as the
lineage-import input and `data/devices.sqlite` as accepted hardware authority.
stable claim revisions and explicit projection mapping are required later;
inventory keys are correctly limited to this snapshot. candidate/search content
does not become hardware evidence. QCT mirror attribution, source roles,
unknowns and legacy saved-output reuse limits are preserved. no archive schema,
collector, migration, release or evidence lookup implementation is claimed.

## limits and handoff

no network, imports, extraction, package installation, catalog mutations, export
refresh, full tests, rebuild, FTS/integrity audit or baseline rerun performed.
historical visual reviews were read as provenance; pages were not newly rendered
or visually re-reviewed. hash preservation applies to the 103 pinned inputs;
unrelated originals/exports were not independently hashed. missing-input checks
used in-memory fault injection and a nonexistent read-only database URI, without
moving or modifying originals. only this report was written.

coordinator may replace pending-review wording after accepting this report;
milestone 1 still requires its own bounded assignment and path ownership.
