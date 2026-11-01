# artifact archive and evidence pipeline handoff

current implementation status: bounded milestones 0–6 are delivered in the
[implementation batch](../research/archive-implementation-2026-10-04.md).
[PROGRESS](../PROGRESS.md) owns accepted state; [usage](archive-storage.md) describes
the implemented commands and limits. the text below preserves the original
planning handoff, including options that were not adopted.

prepared 2026-10-04 for the next agent working in
`/Users/nep/nocloud/git/nepeat/aislop/fcc_shenanigans`.

**purpose and status**

the user wants a durable way to collect from multiple upstream databases,
download artifacts once where practical, retain their metadata, and reference
saved artifacts for future extraction and interpretation. this document captures
the discussion and proposes an implementation plan. the architecture below is a
recommendation, not an implemented system or a claim that the user has selected
every schema detail. this turn delivers the handoff; it does not start migration.

recommended direction: retain SQLite for the structured hardware catalog, add
immutable files identified by SHA-256, and introduce explicit capture history,
versioned derivations and links from reviewed claims to exact evidence. use
small incremental jobs so capture, extraction and interpretation can be repeated
independently. evaluate optional off-the-shelf interfaces after the archive
contract works locally.

read [GOAL](../GOAL.md), [PROGRESS](../PROGRESS.md), [AGENTS](../AGENTS.md),
[workflow](agent-workflow.md) and [evidence policy](evidence-policy.md) first.
PROGRESS owns changing counts, the active complete PDF bundle, current manifest
and integration assignments. at this handoff's initial read it described a
SmartNIC/QCT milestone awaiting exclusive integration. re-read it before work;
do not reuse the older catalog counts from the chat or assume integration has
remained pending. this plan does not supersede another batch's ownership.

**1. project and conversation summary**

the project catalogs liquidated enterprise, networking and server hardware.
exact model/order codes, company roles, practical specifications and provenance
matter. targets include Accton, Edge-Core, Celestica, Oracle, Amazon Data Services,
Inventec, Hyve, Foxconn, Wiwynn, TD SYNNEX / SYNNEX, Quanta, ByteDance and Inspur.
hyperscaler connections are optional evidence context.

the user first asked whether a document database or better schema could preserve
downloaded artifacts and metadata for reuse. the discussion then established the
intended flow: aggregate upstream evidence, extract its contents, and transform
it into our own searchable interpretations while retaining what each source
actually said. our interpretations must remain traceable and revisable.

an example: a regulatory applicant, a vendor manual's compatibility statement,
and a seller's previous-operator claim may all concern the same model. retain
three statements with their distinct roles and evidence classes. joining them
must not manufacture a claim that the applicant built that auction unit for the
reported operator. identical PDFs found on several websites are shared bytes,
not automatically independent corroboration.

the desired capability is to answer new questions from already captured files,
even when the original extraction did not anticipate those questions. an archive
can contain useful unparsed documents and failed extraction attempts without
adding any device assertion.

**2. what already exists and where to inspect it**

these findings were checked against local code on 2026-10-04. line numbers can
move; use the named functions and tables.

| location | current behavior and implication |
| --- | --- |
| [main fetch function](../devicehunt/__main__.py), `fetch` | reuses an existing destination unless refresh is requested; downloads through a `.part` file and replaces the destination. reuse is filename-based, and refresh is not an append-only version history. |
| [catalog schema](../devicehunt/catalog.py), `sources`, `documents`, `document_pages`, `document_search` | source metadata, PDF hashes, page text and FTS already exist. there is no general artifact/capture/derivation registry in the inspected schema. |
| [catalog source method](../devicehunt/catalog.py), `Catalog.source` | upserts current source metadata; derives `fetched_at` from input-file mtime, which can be a bundle's timestamp rather than an original network capture time. |
| [PDF import and search](../devicehunt/documents.py), `dependency`, `read_pages` | validates sibling paths and hashes; binds complete page-text JSON to a PDF hash and a fixed extraction-method string. tool version/configuration and extraction-run identity are absent. |
| [reference import](../devicehunt/reference_documents.py), `prepare`, `import_bundle` | validates a complete collection before replacing document rows. duplicate PDF hashes within a collection are rejected. historical originals remain on disk by workflow, but extraction history is not represented as versioned runs in SQL. |
| [offline rebuild](../devicehunt/rebuild.py), `read_manifest`, and [manifest](../data/import-manifest.json) | validates saved input hashes and reconstructs the catalog. it imports saved PDF text; it does not rerun PDF extraction. |
| [sitemap collector](../research/fetch_fcc_sitemaps.py) | verifies successful cached hashes and preserves known failures without retrying. |
| [historical details collector](../research/fetch_fcc_details.py) | skips existing HTML, but rebuilds its report and can retry entries with no saved file. it is not a safe general retry policy under current project rules. do not run it as part of this planning work. |

the improvement is to unify these existing foundations. there is no need to
discard current sources, curated facts, PDF page search or offline rebuilds.

**3. proposed processing stages**

| stage | durable output | what can trigger another run |
| --- | --- | --- |
| discover | upstream record keys, URLs, link context, coverage and checkpoint metadata | a bounded new source set or an explicit source refresh |
| capture | original bytes and acquisition receipts, including failures | missing capture or an allowed refresh |
| extract | versioned text, rows, tables, OCR or rendered pages | changed input bytes or extraction recipe |
| normalize | original values plus normalized fields and explicit rules | changed extracted content or normalization rules |
| reconcile | candidate entity links and reviewed match decisions | new identifiers, relationships or matching rules |
| interpret | source statements and reviewed claims with evidence references | new evidence or interpretation/review changes |
| publish locally | reproducible catalog, FTS, lookups and reports | a new accepted evidence release |

discovery can precede capture of linked artifacts. extraction outputs can become
inputs to later derivations. preserve that dependency graph explicitly. replacing
a parser should not require new acquisition; correcting a company relationship
should not require OCR. partial success must be retained and distinguishable
from complete source coverage.

**4. storage and authority**

use `data/artifacts/sha256/ab/cd/<full-sha256>` for new immutable byte objects.
the exact two-level layout is proposed. keep original filenames, URLs and declared
content types in metadata. preserve the exact stored byte representation and
record whether it is a response payload, decoded body, browser rendering, or
derived output; never silently normalize an original before hashing it.

| layer | proposed responsibility |
| --- | --- |
| existing `sources/` and raw files | preserved originals and historical provenance; remain untouched |
| immutable artifact files and versioned receipt/manifests | durable archive evidence from which indexes and lineage can be reconstructed |
| archive registry SQLite database | indexed capture/derivation lookup; initially an isolated pilot database, later a separately owned acquisition registry |
| job queue/checkpoints | operational scheduling and leases; job completion alone does not establish reviewed evidence |
| `data/devices.sqlite` | authoritative accepted hardware catalog, with accepted lineage imported through the exclusive integration workflow |
| FTS and exports | derived views of explicitly selected accepted evidence |

recommended separation: collectors write to a dedicated archive registry and
immutable files; they do not write directly into the live hardware catalog.
freeze accepted archive metadata into a hashed release manifest for integration.
the catalog must rebuild from that frozen release without consulting a changing
queue, mutable "latest" pointer or network service.

before implementation, settle which accepted lineage tables belong in the
catalog versus the archive registry. this is a design decision, not permission
to create a second competing hardware catalog. keep catalog schema changes in
`devicehunt/catalog.py` and update rebuild/count/comparison routing. references
across databases need explicit validation because ordinary SQLite foreign keys
do not enforce cross-database relationships.

store receipts and manifests atomically and version them. a database row pointing
at missing bytes is an error. an interrupted write may leave an unreferenced
object; report/reconcile it without deleting historical data. checksums establish
byte identity, not source authenticity or factual correctness. backup both bytes
and metadata, and verify restoration together.

**5. proposed logical schema**

this is an implementation contract to refine into migrations, not executable SQL.
add foreign keys, indexes, state checks and schema versions with the implementation.

| table or entity | principal fields and rules |
| --- | --- |
| `artifacts` | full `sha256` primary key, byte count, detected media type; one logical object per exact byte sequence |
| `artifact_locations` | artifact hash, relative path/storage key, verification state; supports registering existing locations before moving anything |
| `resources` | source namespace, upstream key, original URL and conservative request identity; a resource can have many captures |
| `discoveries` | resource, discovering capture/run, link/row selector and upstream metadata; preserve multiple discovery paths |
| `capture_attempts` | stable ID, resource/request fingerprint, requested/final URL, acquisition method, collector version, actual start/end time when known, status/error, response metadata and prior validated capture reference |
| `capture_artifacts` | capture ID, artifact hash, role and ordinal; multiple assets per capture and multiple captures per artifact |
| `derivation_recipes` | kind, extractor/code version or code hash, dependency versions, canonical configuration hash and output schema version |
| `derivation_runs` | run ID, recipe, input fingerprint, status/error, times and logs; preserve failed and repeated attempts |
| `derivation_artifacts` | run ID, artifact hash, input/output role and ordinal; supports multiple inputs/outputs |
| `document_versions` and `extracted_pages` | logical document identity, original artifact, derivation run and page ordinal/status/text reference; keep PDF page index separate from printed page labels |
| `source_statements`, `review_decisions`, `evidence_links` | stable statement/claim identity, exact raw value and optional normalized value, source role, review state/history, capture, run, artifact, locator and supporting excerpt |
| `jobs` and `source_policies` | bounded scope, checkpoint, dependency IDs, lease, retry budget, host limits, denied-resource decisions and policy provenance |

separate globally intrinsic metadata, such as byte count, from capture-specific
metadata such as server headers, filenames, licensing statements and source URLs.
the same blob can be served under different names and declared content types.

do not deduplicate records or source statements merely because bytes deduplicate.
preserve duplicate upstream rows with their source keys/ordinals. do not infer
entity equality from a filename, title, approximate model match or shared PDF.
keep manufacturer, applicant, grantee, brand, factory, platform and operator roles
separate, together with chassis/node/component identities and conflicting facts.

claim identities must survive source replacement. current `facts` have a composite
key and source records can be deleted/recreated during import; a new evidence
link must not depend on unstable SQLite rowids. define a stable claim key or
revision record and an explicit mapping into the current catalog projection.
retain review history and superseded statements even when the latest projection
changes. newly derived interpretations start as candidates, not accepted facts.

**6. acquisition and reuse behavior**

1. consult saved source failures and the shared resource/request registry before
   making a request. build request identity from method, exact URL, body hash and
   representation-affecting context. retain original URLs; do not blindly strip
   query parameters or merge signed/distinct requests.
2. default to reusing a verified saved capture. a local reuse event must not be
   labeled as a new network observation or refresh its original capture time.
3. for an allowed refresh, use recorded HTTP validators where applicable. a 304
   refers to the previously validated representation and stores no invented body.
   the same URL returning changed bytes creates a new artifact/version.
4. stream downloads into a unique temporary file, enforce limits, calculate
   SHA-256, and publish complete bytes atomically. retain unexpected/error bodies
   with their actual status when available; a 200 login/error page is not a
   successful PDF acquisition merely because the requested URL ended in `.pdf`.
5. coordinate requests with a lease per request identity and one registry writer
   or short serialized transactions. do not hold SQLite transactions across network
   calls. a crashed lease must be recoverable without treating partial bytes as valid.
6. preserve denied/unavailable/empty/invalid/timeout/throttled outcomes distinctly.
   known 402/403 restrictions remain blocked under project policy; no alternate
   route or generic retry loop. stop and checkpoint on throttling. bounded retry
   of transient failures belongs to source policy and must honor current rules.
7. capture pagination/cursors and observed totals where supplied. distinguish a
   completed bounded request set from a claim of complete upstream coverage.

"download once" means avoid repeat acquisition when an eligible capture is
already known, and store identical bytes once in the new object store. an
unfamiliar URL may still require a first download before its hash is known.
mutable upstreams require deliberate freshness checks. do not promise that
content hashing eliminates all network duplication.

HTML/JSON/PDF payloads should be retained before parser-specific interpretation.
browser screenshots, rendered DOM and WARC/WACZ captures have distinct roles;
a screenshot does not replace the response data needed to reparse a source.
page replay quality must be checked if a browser archive tool is adopted.

**7. derivation reuse and evidence inspection**

compute a recipe/input fingerprint from input hashes with their roles/order,
extractor identity/version, canonical configuration and relevant code/dependency
versions. reuse a successful compatible output only after verifying its artifacts.
recipe identity is separate from run identity: failures, explicit reruns and
nondeterministic outputs must remain representable without uniqueness conflicts.

for model-assisted extraction, retain model identifier, prompt/template version,
parameters, input context hashes and the actual response. replay should load
that response; rerunning a hosted model is not guaranteed to reproduce it. no
remote model service is required for the initial pilot.

keep embedded text and OCR distinguishable. an empty text extraction does not
prove the page has no information; diagrams and mixed image/text pages can need
visual review. a search hit does not establish device identity or company role.

evidence locators should support PDF page/table/row/bounding box, HTML element
and quoted context, JSON pointer, CSV row/column, or archive response identifier.
locators are anchored to a particular artifact/run, never only a live URL. record
coordinate conventions where needed. derived text and normalized units are
separate from printed spelling, suffixes, raw units and dates.

the eventual `show evidence` interface should resolve an accepted claim to its
original local artifact and selector, show the source capture and extraction
recipe, and explain review status and conflicts. this command name is conceptual;
it does not exist yet. exact identifiers and structured filters should remain
usable alongside full-text search.

**8. off-the-shelf options and recommendation**

the primary documentation below was consulted during this conversation on
2026-10-04. reassess versions and behavior before adoption; none was installed
or benchmarked for this project. the fit judgments are recommendations.

| option | relevant capability | recommended role |
| --- | --- | --- |
| [Paperless-ngx](https://docs.paperless-ngx.com/usage/) and its [API](https://docs.paperless-ngx.com/api/) | document search, OCR, tags, custom fields and file versions | optional reading/review UI. retain independent immutable originals; some PDF editing actions change its original file. do not assume its duplicate policy enforces our artifact contract. |
| [ArchiveBox](https://github.com/ArchiveBox/ArchiveBox/) | multiple web capture formats including HTML, screenshots, PDF and WARC | optional web capture adapter. explicitly bound enabled capture methods and disable external publication/submission features. |
| [Browsertrix Crawler](https://crawler.docs.browsertrix.com/user-guide/) and [archive replay](https://docs.browsertrix.com/user-guide/archived-items/) | browser captures and portable WACZ archives | evaluate the standalone crawler for interaction-dependent sources before considering the full platform. verify replay against the captured pages. |
| [Scrapy file pipelines](https://docs.scrapy.org/en/latest/topics/media-pipeline.html) | collection orchestration, file download metadata and reuse | possible collector framework; default URL-based paths and expiration behavior still need adaptation to immutable content hashes and our failure policy. |
| [MongoDB GridFS](https://www.mongodb.com/docs/manual/core/gridfs/) | binary files and flexible metadata | viable storage alternative, but it still needs provenance, capture and derivation schemas. no demonstrated reason to migrate the current catalog. |

start with SQLite and local files using available dependencies. a remote object
store, distributed scheduler, semantic search or document UI can be added behind
the same contract when measured needs justify them. a database product alone
does not resolve evidence semantics or entity matching.

**9. implementation milestones and acceptance**

each milestone needs a fresh assignment, finite input set, one editor and a clear
stop condition. implementation should follow the user's next work assignment;
this document is not an instruction to start an unbounded crawl.

| milestone | implementation scope | acceptance and stopping condition |
| --- | --- | --- |
| 0 — freeze a pilot | re-read active ownership; select the current complete reference-PDF bundle and its declared dependencies; record file hashes, known provenance and unknowns | a finite hashed inventory and approved worker path ownership; no network, live mutations or invented metadata |
| 1 — register saved artifacts | implement hash-addressed storage/registration and capture metadata ingestion in an isolated pilot; preserve existing paths and sibling bundles | repeated registration is idempotent; identical bytes share one logical artifact; corrupt/missing inputs fail visibly; all originals unchanged |
| 2 — version saved derivations | register existing page-text outputs, then support one local extraction recipe with explicit version/configuration | output binds to exact inputs; repeated compatible work reuses output; changed recipe creates a new run; two downstream consumers reuse one original offline |
| 3 — integrate one collector | adapt one currently relevant source collector to shared capture/reuse/policy contracts | mocked tests cover cache, refresh, errors, leases and interruption; any live smoke test is separately bounded by source policy and current authorization |
| 4 — add reviewed lineage | implement stable statement/claim IDs, selectors and review mapping for a small representative fact set | every selected claim resolves to an original and locator; conflicting roles remain separate; unreviewed extraction adds no accepted hardware claim |
| 5 — accept a catalog release | freeze archive inputs; update schema/importer/rebuild/comparison code; build candidate and separate clean rebuild; integrate exclusively after QA | affected/full tests, preservation, integrity, foreign keys, FTS and offline equality pass; archive restores; exports and handoff reflect accepted state |
| 6 — expand deliberately | add another upstream format/source and optionally evaluate one UI/capture tool | demonstrate adapter reuse and measure requests/bytes/jobs avoided; stop at the named finite source set |

milestones 1–2 can use all entries in the pinned complete reference collection
without converting every historical capture. add synthetic fixture cases for two
URLs sharing bytes and one URL changing bytes. fixture statements must never
enter live evidence. existing capture times or extractor versions that cannot be
recovered remain null/unknown, with separate registration time and an explicit
legacy-provenance label; do not infer them from current environment or file mtime.

initial module names might be `devicehunt/artifacts.py` and
`devicehunt/derivations.py`, with matching tests. these are proposed paths, not
existing commands. split reusable storage/recipe logic from source-specific
adapters; avoid introducing a workflow server just to coordinate the first pilot.

**10. migration and reproducibility rules**

register existing originals in place before attempting storage consolidation.
preserve historical files, bundles and database snapshots; physical deduplication
of old files is outside this plan. continue to provide byte-identical sibling
dependencies to current importers. do not loosen `dependency()` path validation
or replace the complete PDF collection with a pilot subset.

a future hash resolver must receive explicit tests for traversal, symlink escapes,
missing/corrupt objects and complete-collection semantics before replacing the
current contract. avoid mutable hardlinks that let a legacy writer alter a
supposedly immutable object; use copies or a demonstrated immutability mechanism.

freeze capture and derivation IDs/timestamps in immutable input metadata. offline
rebuilds import those values rather than generating new acquisition times. any
operational timestamps excluded from logical comparison must be identified and
justified. include new accepted lineage tables in both rebuild and comparison.

[compare_rebuild.py](../scripts/compare_rebuild.py) currently uses an explicit
table list, excludes selected metadata columns, and checks differences with SQL
`EXCEPT` plus counts. those checks alone do not prove duplicate multiplicities
for every arbitrary table. preserve the project's additional duplicate-sensitive
audits and strengthen comparison for new tables; do not cite an unmodified
comparison as proof of all new archive metadata.

rebuild from a hashed accepted archive manifest even if the live acquisition
registry has newer pending captures. if multi-input dependencies or registered
legacy files are unavailable, fail rather than silently refetch or produce a
partial accepted catalog. retained originals and the prior live checkpoint are
the recovery path until the new release passes acceptance.

**11. verification matrix for implementation**

| case | expected outcome |
| --- | --- |
| same URL/input registered twice | stable logical registration; no redundant fetch/extraction; genuine later attempts retain distinct receipts |
| two source URLs, identical bytes | one artifact, separate captures/provenance; no false corroboration |
| one URL, changed bytes | two immutable versions and explicit history |
| 304 / local cache reuse | prior artifact reused; network validation and local reuse remain distinct |
| denied / throttled / unavailable / confirmed empty | distinct persisted states; denied sources are not retried; throttling checkpoints |
| HTML error body returned for PDF URL | retained with actual metadata, excluded as a valid PDF input |
| crash during fetch or metadata commit | no partial valid artifact; restart reconciles staged/orphan state safely |
| concurrent same-resource requests | lease or serialized scheduling prevents duplicate work where possible; no corrupt object or lost receipt |
| changed extractor/configuration | new recipe/run; old outputs retained; only affected descendants become stale |
| duplicate raw rows / conflicting claims | duplicates and conflicts survive extraction, normalization and rebuild |
| legacy metadata missing | explicit unknowns; no fabricated capture time or tool version |
| bad path / wrong hash / missing artifact | validation fails before accepted-source replacement |
| read-only query or missing database | no file/database mutations; absent DB does not become an empty DB |
| candidate versus offline rebuild | accepted records and lineage equal, including duplicate multiplicities and logical FTS contents |
| backup restored into a new location | artifact hashes, metadata links and representative evidence lookups resolve offline |

tests and offline rebuilds must perform no network I/O. use mocked responses and
temporary directories for acquisition behavior. run affected tests during each
bounded change; full suite for schema/importer changes and before combined
code/data promotion. preserve the owned 10/10 baseline's nine-document/one-label
distinction and the separate unresolved ByteDance examples; do not fabricate hits.

use the runtime documented in [README](../README.md). if system Python is blocked,
the currently documented bundled executable is
`/Users/nep/.cache/codex-runtimes/codex-primary-runtime/dependencies/python/bin/python3`.
verify it exists; do not install packages or change Xcode/system configuration.

existing verification commands, after selecting unique candidate paths and a
stable input manifest, include:

```sh
python3 -m unittest discover -s tests
python3 -m devicehunt --db data/rebuilt-artifact-pilot-UNIQUE.sqlite rebuild --manifest data/import-manifest.json
python3 -m devicehunt --db data/rebuilt-artifact-pilot-UNIQUE.sqlite validate-inventory
PYTHONPATH=. python3 scripts/compare_rebuild.py data/rebuilt-artifact-pilot-UNIQUE.sqlite --database data/candidate-artifact-pilot-UNIQUE.sqlite --output-prefix artifact-pilot-UNIQUE
```

these are templates, not commands to run during handoff preparation. replace
`UNIQUE` and select the reviewed frozen manifest appropriate to the candidate.
the comparison writes audit files under `data/exports`; only its assigned owner
should run it. never compare a new candidate against a moving live checkpoint.

**12. delegation, constraints and immediate next action**

once implementation is assigned, use GPT-6.1 Sol workers here, or the verified
latest Sonnet on a Claude host per AGENTS. a useful sequence is an artifact
engineer, an independent adapter/fixture engineer with disjoint files, then an
independent reviewer. assign shared catalog/schema/manifest work to one integrator.
the coordinator owns prioritization and acceptance; workers do not delegate
further without an explicit assignment.

keep all code, data and research in this workspace. use public/free sources.
no seller/provider contact, purchases, paid data, publication or push follows
from this plan. respect known access restrictions; no alternate-route retries.
do not delete history, change validation expectations, or collect every discovery
URL. documentation must remain current without duplicating changing counts.

the next bounded action is milestone 0: pin the complete reference collection,
map its existing PDF/text dependencies into proposed artifact/capture/derivation
records, list unknown legacy metadata, and finalize the archive/catalog authority
boundary. this can be done entirely offline and does not require selecting a
document-management product.

handoff prompt for the next agent:

> read `AGENTS.md`, `GOAL.md`, `PROGRESS.md`, `docs/agent-workflow.md`,
> `docs/evidence-policy.md`, and `docs/artifact-archive-plan.md`. continue within
> the user's assigned scope. the proposed design preserves original artifacts,
> separate capture events, versioned reusable derivations and reviewed evidence
> links while retaining SQLite as the accepted hardware catalog. start by checking
> active integration ownership and pinning a finite offline reference-PDF pilot.
> preserve complete sibling bundles and all existing evidence. report concrete
> files, checks, unknowns and the next bounded step; do not claim unimplemented
> stages are complete.

planning ownership and review status are recorded in the
[handoff batch note](../research/artifact-archive-handoff-batch-2026-10-04.md).
