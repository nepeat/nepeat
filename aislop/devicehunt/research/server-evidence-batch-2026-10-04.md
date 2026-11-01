# Server evidence — 2026-10-04

Status: complete and promoted; source/code/candidate, export and independent
[documentation review](batches/server-evidence-2026-10-04/docs-review.md) passed.
The overall goal remains active. Coordinator owns current documentation.
Workers use GPT-6.1 Sol with fresh briefs; no further delegation.

| Worker | Task and stop condition | Exclusive writable paths |
| --- | --- | --- |
| Ingrasys researcher | Inspect existing malformed server PDF first; recover useful original evidence for SV1120/SV1121/SV2121/SV2221 with at most eight queries/four new source captures | `data/raw/server-evidence-ingrasys-2026-10-04/`; `research/batches/server-evidence-2026-10-04/ingrasys/` |
| Oracle researcher | Review saved NIST 4506 detail, preserve tested software-module scope and compare existing records; no network | `research/batches/server-evidence-2026-10-04/oracle/` |
| Buying-report engineer | Expose explicit CMVP tested-configuration facts with full module context; meaningful focused/full tests | `devicehunt/buying.py`; `tests/test_buying.py`; this batch's `engineering/` |
| Independent reviewer | Check source claims, restored-PDF provenance and bounded code change; later candidate/docs gates when assigned | This batch's `source-review.md`, `code-review.md` and `review/` |
| Integrator | Merge accepted inputs, preserve the full PDF bundle, stage candidate/rebuild and stop for QA | New batch data/bundle/candidate/rebuild, import manifest and batch integration/audit files; live/current exports unchanged until promotion |

The previous turn made verified progress; live SHA and source/code checkpoint are
in `batches/server-evidence-2026-10-04/before.json`. Shared state stayed read-only
during research; accepted source/code inputs are now frozen in `acceptance.json`.
One exclusive integrator staged the candidate after source/code acceptance, then
promoted it only after independent candidate QA and the coordinator's acceptance.
The manifest and live database now describe the same accepted checkpoint.
Do not rerun historical mutation helpers. The bounded engineering change is frozen.

The unsuffixed Ingrasys names and SV2221A/I were already imported. The prior
handoff linked an outdated next-step paragraph; this batch targets the remaining
malformed server-catalog gap, without refetching the denied Foxconn press release.
New source bytes and derivatives must remain separately hashed and fully explained;
partial recovery is never a complete document claim. NIST-tested configurations
are not general OS support, installed hardware or transferable software entitlement.

The recovered catalog pages explicitly name eight A/I variants, not the unsuffixed
models. The source task includes those eight precise vendor records and, if proven,
separate dated portfolio records for six newly identified variants. The unsuffixed
specification gap remains; no extra acquisition is implied by this scope clarification.

## Accepted results

- [Ingrasys source handoff](batches/server-evidence-2026-10-04/ingrasys/handoff.md):
  eight exact vendor records (124 facts), with six separate dated Foxconn portfolio
  records (72 facts). SV1120A/I, SV1121A/I and SV2121A/I add six identities;
  SV2221A/I enrich existing identities while retaining their other sources' claims.
  No new physical-manufacturer or deployment assertion is made.
- The original response remains immutable and malformed. A documented second-span
  extraction plus three terminal EOF bytes produces a ten-page derivative; strict
  parsing and independent visual review support the page facts. It remains explicitly
  reconstructed. All pages are image-only; no OCR or embedded-text facts are claimed.
- [Oracle handoff](batches/server-evidence-2026-10-04/oracle/handoff.md): sixteen
  facts expose the saved NIST E4-2C environments with PAA/single-user qualifiers
  and module context. This is another extraction of the same capture, not independent
  corroboration. All four original NIST records remain intact.
- [Code review](batches/server-evidence-2026-10-04/code-review.md): one explicit
  report-field classification, with 11 affected and all 428 tests passing.

[Source review](batches/server-evidence-2026-10-04/source-review.md) and
[candidate review](batches/server-evidence-2026-10-04/candidate-review.md) passed.
Old evidence stays exact except the 45 existing archive sources' bundle hash/path/
fetch metadata. All 43 logical tables match with duplicate-sensitive comparisons
and both FTS indexes reconstruct. Only the new document's text pointer changes
from the source proposal: an [authorized envelope](batches/server-evidence-2026-10-04/text-envelope-authorization.json)
adds required PDF-hash/extraction metadata to the unchanged ten page objects.
The rejected initial preparation and original text remain preserved.

Current totals, export acceptance and next work live in [PROGRESS](../PROGRESS.md).
[Final verification](batches/server-evidence-2026-10-04/integration/final-verification.json)
binds the promoted candidate, refreshed exports and unchanged read-only checks.
Batch mutation helpers are historical after execution and must not be rerun.
