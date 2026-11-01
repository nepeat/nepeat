# ongoing device-hunting campaign plan

Prepared 2026-10-04 for the user's Fable + Astra planning assignment.
Status: synthesized draft awaiting Fable's final cross-review. This is a plan, not
implemented campaign functionality or authorization for an unbounded crawl.
[Current accepted state](../PROGRESS.md) and [planning/review record](../research/campaign-planning-2026-10-04.md).

## objective and boundary

Build a repeatable, supervised campaign that turns explicitly scoped public
sources into useful datacenter-hardware leads and reviewed catalog evidence.
Measure coverage of named queries, snapshots and date ranges. Do not promise
all worldwide regulator or shipment information, or count downloaded rows as
identified hardware. Keep the existing SQLite catalog and immutable archive.

The archive pilot established storage, capture and release primitives. It did
not migrate all collectors, build a scheduler, implement trade ingestion, or
provide reviewed lineage for the whole catalog. Existing source importers remain
valuable and keep their source-specific validation rules.

The first campaign uses saved **Korean RRA manufacturer queries and Canadian
ISED company queries**, following Fable's recommendation. RRA already has bounded
date queries, detail validation and before/after export comparison; ISED tests
a different, session-based flow. `fccid.co` stays a regression fixture: it is a
third-party discovery mirror, not a first-party FCC feed or proof of full FCC
coverage. Neither pilot source's saved query scope represents its full database.
Trade feasibility runs as a separate branch; no qualifying source is an allowed
outcome. Vendor manuals and compatibility lists remain important hardware
evidence and become the next adapter family after the regulator pilot.

## minimal architecture

| authority | contents |
| --- | --- |
| versioned campaign definitions | explicit source/query scope, input pins, adapter/recipe versions, request/byte/time/item limits, refresh policy and exclusions |
| one operational SQLite database | durable runs, jobs, cursors, budgets/reservations, leases, stops and resume decisions; no accepted hardware facts |
| existing immutable archive and reconstructible index | original bytes, source occurrences, acquisition/derivation receipts, candidate output artifacts; optimized indexed lookup |
| frozen accepted manifests and catalog | selected reviewed inputs and lineage imported exclusively; independent of current job state |

Evolve or explicitly migrate `capture-leases.sqlite` into the operational store;
do not maintain competing lease authorities or create a database per campaign.
Use a local run-to-completion CLI first. Proposed commands are `campaign plan`,
`run --offline`, `status`, `resume` and `report`; these do not exist yet.
No daemon, distributed queue, hosted model, document UI or new database product
is needed for the pilot. A periodic invocation can follow only after repeated
runs are safe and source freshness policies are configured.

The adapter contract must return explicit request/work descriptors, saved input
references, checkpoint state, response validation results, raw row occurrences,
coverage observations and a complete candidate-bundle manifest. All requests
pass through the shared capture policy. Adapters retain format-specific parsing
and decide whether a representation is reusable or session-dependent. A cursor
is namespaced by query, snapshot/session, adapter version and relevant config.
An incompatible resume fails visibly instead of guessing.

Keep discovery, extraction, candidate linkage, review and catalog integration
separate. A changed parser may reuse original bytes; a reviewed claim may change
without recapture. Use actual upstream keys when available, preserve duplicate
rows with occurrence IDs, and never manufacture an upstream key from a title.

## correctness rules to settle before expansion

- **budget and crashes:** reserve a request in a short transaction before HTTP;
  share run/host limits across processes and restarts. Distinguish planned,
  reserved, started, completed and uncertain attempts. A crash after dispatch
  can leave an unknown outcome; do not automatically refund its budget or claim
  exactly-once acquisition. Do not hold SQLite transactions over network I/O.
  Host leases alone cannot enforce a run-wide quota across different hosts;
  admission/reservation must be serialized at the campaign limit as well.
- **index recovery:** replace writable-open full reindex and repeated receipt
  scans with indexed queries, but retain immutable receipt authority. Define
  publication order, pending writes and replay so a crash between receipt and
  index commit cannot hide a denial or completed capture. Start with one writer;
  explicit full audit/reindex remains available and read-only commands stay so.
  Receipt-file count equality is not a sufficient consistency protocol and
  counting all files is itself a scan. Specify recoverable writer state instead.
- **access state:** persisted 402/403 restrictions stay blocked. Throttling stops
  and checkpoints the source. Resume needs a recorded policy decision, eligible
  time and remaining budget; restarting a process is not a reset. Do not convert
  historical unavailable results into an automatic retry queue.
- **stateful requests:** ISED form execution and export preparation cannot use
  ordinary cross-session cache reuse. Cookies remain memory-only; new receipt
  metadata must not store credentials/session cookies. Retain exact saved bodies
  under the existing evidence policy. A fresh session can replay allowed setup
  steps within budget; old tokens do not resume a live workflow. Validate hosts,
  redirect decisions, form actions and discovered links before dispatch.
- **freshness:** distinguish capture time, validation time, local reuse time and
  upstream effective date. Freshness is source-specific. An unchanged download
  is a new observation only if a request occurred; a 304 needs its prior verified
  representation. Absence in a partial page set cannot mean source deletion.
  RRA's final CSV is a deliberate second observation: ordinary cache reuse of
  the before-export must not manufacture a stability proof. Preserve the bundle's
  declared byte-identical or exact-certificate-fields/ignore-order comparison.
  Query stability is not completeness, and an approval-date window is not a
  change feed. Later campaigns need separately bounded historical revalidation
  for amendments/backfills; new-date windows alone cannot establish freshness.
- **identity and release:** snapshot row positions must not be durable claim
  keys. Reordering, amendments, repeated certificates and conflicting roles
  require explicit candidate mappings. Keep old reviewed statements and decisions.
  Complete source replacement still requires complete validated bundles.
- **source and job versions:** pin policy, adapter, query and recipe identities
  to each run. Separate reusable job identity from distinct execution attempts.
  Bounded cancellation/stop controls must survive restart; a new run cannot
  evade an existing source restriction. Treat clock changes conservatively for
  leases, due times and resume eligibility.

## milestones and acceptance

Each milestone starts with one named owner, explicit writable paths and frozen
inputs. No implementation is started by this document. Network stays disabled
for tests and the initial campaign rehearsal.

### c0 — pin the pilot and source contract

Coordinator owns a new `research/batches/campaign-pilot-<unique>/` inventory and
versioned campaign definition. Pin the current accepted manifest/release and all
dependencies of its exact `source == "rra"` and `source == "ised"` jobs. Inspection
for this plan found seven RRA jobs and five ISED jobs; c0 must re-read the manifest
and record any intervening changes rather than assume these counts are current.
RRA inputs are `bundle.json` in these `data/raw/` directories:

- `rra-celestica-official`
- `rra-amazon-data-services-official`
- `rra-edgecore-official`
- `rra-accton-official`
- `rra-oracle-official`
- `rra-edge-core-hyphen-official`
- `rra-edge-core-space-official`

Pin exact query fields from those bundles, their company-evidence dependencies,
before/after CSVs, detail pages and any explicit empty-result proof. Do not infer
additional company spellings or dates. The ISED inputs are:

- `data/raw/ised-amazon-export/capture.json`
- `data/raw/ised-accton-paged/capture.json`
- `data/raw/ised-edgecore-paged/capture.json`
- `data/raw/ised-oracle/capture-paged.json`
- `data/raw/ised-celestica/capture-paged.json`

Read source notes/failure reports before defining scope. Record hashes, duplicate
source occurrences, source ownership, unknown metadata, query coverage and the
current adapter outputs. Define source/job state transitions and report fields.
The accepted import manifest remains unchanged; the pilot inventory is separate.

Acceptance: every pinned file resolves and hashes correctly, missing dependencies
fail, source scope is finite and replay expectations derive from saved inputs.
Stop at the pinned set. The existing finite FCC sitemap archive/test inputs remain
regression coverage; they do not authorize following device links.

### c1 — make archive/capture state safe for repeated runs

One engineer owns `devicehunt/artifacts.py`, `capture.py`, the new operational
state module and affected tests. Add indexed resource/capture/derivation lookups,
durable reservations and recoverable writer state. Retain current archive paths
and immutable receipts. A separate reviewer checks migration and crash behavior.

Acceptance: injected crashes before/after object publication, receipt publication,
index commit, request dispatch and checkpoint commit; restart reconciles safely.
Two competing runners cannot exceed the shared quota or both own a live request
lease. Corrupt bytes fail; denied requests never reach transport; read-only
inspection does not create a missing DB. Benchmark fixed synthetic receipt sets
at two sizes and record lookup/open work, bytes read and timings. Ordinary open
and point lookup must not perform a whole-archive scan; full audit is explicit.
Synthetic fixtures stay outside evidence and the accepted manifest.

### c2 — run the saved campaign with bounded, persistent jobs

After c1's API is accepted, one engineer owns `devicehunt/campaign.py` (proposed),
its CLI routing and tests. Adapter engineers may work independently on an RRA
adapter and an ISED saved-capture bridge with disjoint files. Contract
changes return to the state owner; workers do not patch shared modules together.

Implement plan/run/status/resume/report with durable checkpoint transitions and
the adapter contract. Begin with offline registration and replay. A separate
bounded ISED transport assignment adds the stateful request contract and mocked
form/export flow before any live ISED refresh is considered ready. RRA transport
must also be routed through capture rather than invoking the existing curl
helpers unchanged. Expand the exact allowed detail-URL set only from validated
CSV rows within the frozen source/host policy and a predeclared item/request cap;
unknown row counts are not permission to increase the budget. Empty queries need
their separate result-page proof. A partial detail set cannot publish a bundle.

Acceptance: two runs against identical saved input produce identical logical
outputs, no new captures or extraction runs where compatible results exist,
and no duplicate logical jobs/rows. Additional operational run/reuse events are
allowed and remain distinct from source observations. Interrupt after one work
unit and resume without losing or duplicating raw occurrences. Fixtures exercise
reordered pages, changed totals, stale tokens, malformed CSV, explicit zero,
HTML error pages, blocked/throttled outcomes and budget exhaustion.

The report separates planned/observed/validated/failed/blocked work, coverage
denominators (or unknown), changed/unchanged records, reused/new bytes, physical
storage growth and unresolved exact-model/document leads. Passing a finite job
set does not mean full upstream coverage. Blocked/unavailable terminal jobs count
as accounted-for work, not successfully observed source coverage. Stop after the second offline run and
one fault/recovery rehearsal; do not schedule continuous collection yet.

### c3 — make candidate review and selected releases repeatable

One integration engineer owns archive release selection, lineage/catalog schema
changes if needed, importer/rebuild/count/comparison routing and related tests.
Select explicit candidate and coverage roots (including zero-result and failed
units without statements), compute and validate their required artifact/
receipt closure, and freeze a self-contained release. Unrelated pending inputs
must neither enter the release nor prevent freezing a valid selected closure.
Preserve accepted prior releases and complete source/PDF collection semantics.

Implement a small local JSON/CLI review queue, not a review website. It should
show the source literal, original locator, changed fields, proposed identity and
role, conflicts and reviewer decision. Prefer exact identifier candidates;
approximate name matches remain suggestions. Map snapshot rows to stable claims
explicitly; do not reuse a positional source key as proof of identity.
Existing importers can project whole sources into facts. Therefore either review
and accept their complete candidate projection, or implement a deterministic,
tested selection of accepted claims. A review queue that does not actually gate
the importer is insufficient; rejected/unreviewed candidates must stay out.

Acceptance: choose a finite representative existing evidence set during c0,
including ISED repeated certificates/HTML–CSV discrepancies and existing PDF
lineage, plus RRA query stability, explicit zero and cross-query certificate
overlap. Different spellings can already contain the same certificate: preserve
those observations without counting independent corroboration. Reordering cannot retarget accepted claims; conflicting roles and
duplicate occurrences survive. Synthetic additions, amendments and removals
exercise review behavior but never enter live evidence. An unchanged real source
may yield no new accepted claims and still pass. Missing selected dependencies
fail before source replacement; an unrelated damaged pending object does not
poison selection. Rebuild from frozen inputs after moving the mutable registry
out of reach. Run full tests, preservation, baseline, integrity/FK/FTS and
duplicate-sensitive comparison before an exclusive candidate promotion.
The current lineage validator requires referenced facts in the candidate DB:
stage source imports into an isolated candidate first, review the proposed
projection, freeze accepted lineage, then rebuild/compare before promotion.
Creating the isolated proposal is not accepting it into the live catalog.

### t1 — decide whether a useful trade adapter is feasible

This research branch can proceed after c0 without blocking the regulator pilot.
First inspect saved restrictions. Choose at most three distinct, previously
unblocked official/public candidate datasets, record their documentation URLs
and set a request/byte budget before external research. Verify current access,
cost, allowed use, geographic/time coverage, row granularity, identifiers,
updates and practical model-level usefulness. Do not retry the denied Eximpedia
item through another route; the paid Datadory path remains closed.

Deliver a source matrix and a recommendation: qualifying row-level source,
aggregate-only context, or no qualifying source. No availability claim or source
selection has been verified during this planning pass. Search snippets are leads.

Only a qualifying result starts t2: pin one finite source/date/query set, then
build an adapter preserving original rows, party roles as printed, dates,
commodity classifications, quantities/units and literal product descriptions.
Separate shipment IDs/line items from company and device identities. A consignee
does not establish operator/manufacturer, and an aggregate trade category does
not establish a model. Stop after one tested replay and review of that finite set.

### c4 — demonstrate a supervised recurring campaign

After c1–c3 and source-specific transport checks pass, configure one finite RRA
refresh window, with explicit requests/bytes/time limits and saved
policy decisions. The coordinator determines whether existing user authorization
covers that concrete scope; ask only for genuinely missing authorization.

Run a bounded refresh, review its coverage/failure report, then a second eligible
cycle. ISED remains offline until its session contract passes the separate mocked
and bounded-live gates. Verify budget persistence, no forbidden retries, controlled storage growth
and a reproducible candidate delta or documented no-change result. Current source
access may prevent a live cycle; report offline-ready versus live-validated
separately. Only then add periodic invocation. No daemon retry loop or autoaccept.

Pick the next finite source family using exact-hardware yield and coverage gaps:
an original-vendor manual/support source is preferred over indiscriminate radio
record accumulation. A trade adapter is optional until t1 qualifies a source.

## storage, sequencing and stopping

Retain the current object layout and register existing files in place. Do not
copy the catalog for every job or freeze the whole growing archive for each
release. Keep shared immutable artifacts and small manifests; measure new bytes,
receipt/index growth and total allocation per run. Pin capacity limits during c0
from available disk and stop cleanly before exhaustion. Never delete raw evidence
or history to satisfy a cap. Candidate plus clean rebuild space is needed only
for an integration; archive superseded snapshots through the existing verified
history workflow under exclusive ownership.

Back up operational state as well as archive receipts/bytes and accepted inputs.
After restoration, reconcile uncertain jobs and stops before enabling requests;
do not lose spent quotas and restart collection automatically. Test restoration
offline at a new root. An on-disk restore test is not an off-machine backup.

Recommended order: **c0 → c1 → c2 → c3 → c4**, with t1 independent after c0 and
t2 conditional. Source discovery is bounded by the current assignment; additional
URLs become proposed work. SmartNIC/QCT integration remains a separate pending
batch and must not compete for the manifest/catalog integration lock.

The next assignment is **c0 plus the c1 design/test contract**, entirely offline:
pin the saved inputs, define durable state and recovery semantics, and submit the
contract for independent review before c1 implementation. This avoids building a
scheduler around today's instance-local budget and session-agnostic cache.
