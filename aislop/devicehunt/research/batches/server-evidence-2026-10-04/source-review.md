# independent source review — server evidence 2026-10-04

status: oracle and ingrasys inputs pass independent source review for coordinator
consideration; integration preservation checks remain required. reviewer owns this file. no network,
imports, shared-state changes or delegated work occurred.

## oracle — pass

Reviewed `oracle/handoff.md`, `oracle/review.json`, `oracle/candidate-curated.json`,
immutable `data/raw/nist-4506.html`, live SQLite through `mode=ro`, and current
curated-import identity/replacement behavior. Evidence proof:
[`review/oracle-proof.json`](review/oracle-proof.json).

All sixteen facts are supported source-scoped statements or explicit provenance
and limitation text. The two E4-2C tested configurations exactly preserve both
literal rows: Windows Server 2022 / AMD EPYC 7J13 with PAA, and without PAA
(single-user mode). Software module name/type, FIPS 140-2, version 3.0.15,
Historical snapshot status, sunsetting reason, caveat, vendor literal and both
history rows match saved HTML. History dates remain module events; no platform
assignment, current support, installed hardware, entitlement, manufacture or
operator assertion is introduced. PAA remains unexpanded.

The current four `nist-4506` records and their raw JSON, facts, affiliations and
identifiers exactly equal the supplied review snapshot. The original capture hash
matches the source registry and candidate provenance. Independently computing the
current identity digest gives existing `b5decfa477ec728d6252`; zero new identities
are expected. Exact Oracle tested_platform_vendor role is preserved.

An additive extraction source is justified: curated import replaces a source's
records and serializes the complete curated object as raw_json, so replacing the
original would violate strict original-row preservation. The new source plainly
labels reused evidence; it is not independent corroboration. Expected addition:
one source, one record, sixteen facts. Integration must still prove these deltas
and old-row preservation. `raw/nist-4506.html` requires the assembled curated
input under `data/`; do not import the research-path file directly.

Reviewed source hash:
`47bf103fab0e61eb214ac5ad17bae892951663a320d2bed427e8bf22e5455064`.
Reviewed candidate hash:
`9bc1b31ff468ba41e0d10be4e04fe38a07f43553b8125ff13e18552bcc7153df`.
Live hash at review:
`7019d5858161a9cf71b8f8040e013fc23124a87e65e8343892d3265ac6320fd2`.

## ingrasys — pass with explicit recovery and identity limits

Reviewed completed handoff, candidate, portfolio and single-document proposals;
all ten rendered page PNGs were independently viewed, and every proposed product
specification checked against pages 3–10. All 124 curated vendor facts match their
model page or give explicit provenance/limitations. Units, printed FHAHL, SKU 3
CXL memory, SKU storage/expansion boundaries, fifth/fourth-generation memory
speeds and DC-SCM versions remain intact. Pages 7–10 contain no storage facts;
none is invented. Pages 1/2 stay generic context and do not transfer specs.

Independently verified original/fresh response byte equality, exact first-span
prefix, exact derivative equality to `[4128654,8355366)` plus only `F\r\n`,
and strict pypdf ten-page parsing with blank embedded text. Derivative is
reconstructed page evidence, not a valid complete original download. Source,
derivative, text, corporate, candidate and page hashes are bound in
[`review/ingrasys-proof.json`](review/ingrasys-proof.json). The PDF proposal's PDF
and text hashes match actual artifacts; it must only append to the full preserved
collection with byte-identical sibling files.

All eight headings explicitly end in A/I. **Six identities are new:** SV1120A/I,
SV1121A/I and SV2121A/I; only SV2221A/I currently exist in live data. Unsuffixed
SV1120/SV1121/SV2121/SV2221 retain their separate discovery identities and gaps.
Expected vendor additions: one source, eight records, 124 facts, six identities.
No aliases or inferred manufacture are present. Footer years on model pages
3–7 are 2023 and 8–10 are 2025; page 2 is 2022. No release date or platform
generation is derived from index date or document metadata.

The separate six-record parent proposal adds 72 facts under one new source.
Saved about.html line 805 explicitly says Foxconn Technology Group subsidiary;
its hash matches. Portfolio inference is labeled cross-source, retains both
source URLs/hashes, capture observation date and derivative/page provenance.
Corporate text does not name models. Parent-company portfolio role establishes
neither physical manufacturer nor deployment. Existing SV2221A/I portfolio
records are not duplicated. Root expressly authorized this finite extension.

Current SV2221A HTML liquid-cooling statement and recovered page 9 air/liquid
options must both remain source-local; existing SV2221A/I facts are recorded in
proof for integration preservation. Live database hash remains the Oracle review
hash. No acquisition, imports or shared-state edits occurred during QA.

Source gate: pass. Integration must independently prove all original records,
record timestamps, raw facts and roles unchanged. All old source fields remain
exact except sha256, cache_path and fetched_at on exactly the 45 existing reference
source rows whose source points to the complete bundle: appending the accepted
one-document entry necessarily updates these bundle-derived fields. No other
source-metadata exception is permitted. Preserve the full bundle's 45 original
entries verbatim and all old PDF/page bytes, appending one accepted entry only.
This review accepts recovered evidence within its stated limits; it does not
resolve the unsuffixed specification gap.
