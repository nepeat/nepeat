import json
from pathlib import Path
with Path('GOAL.md').open('a') as out:
 out.write('''

## Amazon UL model names — 2026-09-27 UTC

The public Amazon Data Services UL AZOT7.E515139 search result supplied 74
factual model entries. Direct opening returned 403 and was not retried. The
reviewed search extract preserves duplicates and spelling; original HTML,
visual grouping and index freshness remain unverified. Ten unique placeholder
templates stay unexpanded in the cache. A new ul-amazon-platforms importer
adds 57 exact-name assertions, including 34 new catalog identities. Every
assertion has listing_holder scope only, with zero manufacturer, operator or
cross-brand alias inference. See research/ul-amazon-platforms.md and the
ul-amazon-models JSON/CSV exports and audit.

Current totals: 3,334 identities, 4,375 assertions, 175 sources. Manufacturer
scope remains 1,696 target/identity pairs across 1,695 identities. All associations
have 3,417 pairs across 3,334 identities; Amazon Data Services rises from 176 to
210 association identities while its manufacturer count stays 173. The baseline
remains 10/10 searchable and 8/10 corroborated, including all four Oracle labels.
The full 7760-32X-A-AC-F and bare AZ4444-52X gaps remain open.

All 282 tests pass. The 74-job manifest hashes 1,824 files and rebuilds to
data/rebuilt-with-ul-amazon.sqlite. Tests cover exact names and suffixes,
placeholder exclusion, repeated-entry provenance, role isolation, idempotent
refresh and invalid-capture rejection before replacing evidence.

OpenBMC discovery did not establish new target-maker records: the initially
retrieved upstream recursive tree and a later master commit referenced different
trees. A guard stopped dependent downloads before mixing snapshots. Discovery
files are retained outside the import manifest; no target manufacturing or
negative exhaustive coverage claim was inferred. Any continuation must pin
its tree and source files to the same immutable commit. FCC coverage and the
broader exhaustive goal remain unproven, so the goal stays active.
''')
