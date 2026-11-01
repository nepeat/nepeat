# SPEC continuation candidate — accepted revision3 rules

owner `/root/bytedance`; 2026-10-12 PDT / 2026-10-13 UTC. **staged candidate,
independent review required; not imported.** Only this new directory written.
Production code remains strictly frozen; no code/test/shared catalog/manifest/
archive/export/current-document edit, prepared database, network/install or delegation.

[Candidate](candidate.json), [builder](build_candidate.py),
[field mapping](field-mapping.json), [read-only routing](routing.json),
[copy provenance](provenance.json), [checks](checks.json),
[offline checker](check_candidate.py).
Inputs: [accepted discovery review](../review/spec-continuation-review.md),
[frozen discovery handoff](../spec-continuation/handoff.md),
[observations](../spec-continuation/observations.json) and
[request ledger](../spec-continuation/requests.jsonl).
Historical wave9/10 handoffs, revision3 review and builders were read; historical
scripts with stale scratch paths were never executed.

## payload and field mapping

Exactly **18 sources / 18 source-scoped records / 366 facts**, one record per
captured result: six SPECjbb2015 HTML pages with27 facts each (162), and twelve
CPU2017 text pages with17 each (204). There are18 `spec_system_name` identifiers
and18 affiliations. Source IDs retain the accepted `spec-<result stem>` convention;
record keys retain each complete printed system name. Snapshot copies preserve
all18 raw HTML/text bodies and original mtimes; captured UTC dates are explicit.

Every record preserves its complete reviewed raw observation and index row.
Every fact has a quote, source URL/hash and 1-based line or table/cell/label
locator. **348 source-content quotes** re-resolve to copied captures; the18
`spec_result_url` facts explicitly cite saved request input URL/sequence rather
than pretend the URL is document content. No duplicate source/record/fact key
is present. Raw original strings, wraps, conditions and unavailable fields remain.

Rules match previously accepted revision3:

- CPU Hardware Vendor comes from the printed system-title block, split only by
  the exact saved-index hw_model suffix; jbb uses `Hardware hw_1` Vendor/Name.
  Every printed Vendor here is `Inspur Corporation`, which has no `target_for`
  alias hit. Brand stays exactly `Inspur Corporation`; it is not mapped to Inspur.
- All18 Test Sponsor strings equal printed Vendor, so the old affiliation rule
  yields `brand`. A differing sponsor would yield `reported_system_vendor`;
  Tested by remains a fact. No manufacturer/factory/OEM/operator/deployment claim.
- Twelve CPU names remove only their trailing parenthesized processor annotation
  under the accepted CPU-word rule. All six jbb names stay unchanged. The printed
  `Inspur` model prefix, case and base suffix remain; full printed name is in the
  record key, identifier, `spec_system_name` fact, raw observation and coverage.
- Vendor-attested/SPEC-published tier stays `vendor`, with upstream rights retained.
  Kind is `server`; this set has no cloud-instance or virtual-machine result.

## routing and case boundary

Timestamped `mode=ro` observations of both authoritative live and staged wave3
catalogs find **12 distinct catalog identity IDs: eight existing, four proposed
new**. All18 proposed source IDs were absent in both. Catalog hashes at these
observations are recorded, not permanent future baselines:

- live: `53a9a0eabaef5656b38ad922f96d963b76839eeda2da3b28a18b22359d857dee`;
- staged wave3: `4ae2cd0d6a614d582cf6fc47e6ba0e022521ac2b5e73bb268fcfa0dcb87c03ea`.

Four new exact namespace/model pairs are `Inspur Corporation` with printed
`Inspur NF5170M4`, `Inspur NF5180M4`, `Inspur NF5270M4`, `Inspur NF5488M5`.
The remaining14 records route to eight existing IDs. Near matches under other
legal/brand strings or without the printed prefix are not bridged.

Two existing model displays use lowercase `Inspur i24M6` / `Inspur i24LM6`, while
these three result records print uppercase `Inspur I24M6` / `Inspur I24LM6`.
The coordinator explicitly confirmed the established catalog casefold identity
rule: these share existing IDs `710919c49765e744012c` / `2efcb0bdbfed7895e91c`;
each new record keeps its printed uppercase model/full name/raw evidence, and
existing first-writer identity display remains unchanged. This is a recorded
case-only collision, not a new alias or identity bridge.

The first research-only builder attempt halted on an overly strict display-case
assertion before any candidate/provenance/routing JSON was created. Only18 exact
snapshot copies existed. [First source revision](build_candidate.py.first-attempt),
[traceback](first-attempt.log) and [provenance note](first-attempt-note.txt) preserve
that outcome; source revision is reconstructed from the precise guard/
anchor correction and traceback is transcribed from the recorded tool result.
Corrected builder follows the already established casefold rule. No production
change or snapshot replacement was needed.

## source-scoped hardware and benchmark limits

All six jbb pages contain only hw_1; no hw_2 field transfers occur. Five NF test
configurations print `1 x 550W`, TS860G3 prints `4 x 2000W`; original 1U/2U/8U
form factors and node/CPU/memory fields remain literal. These are historical
submitted configurations, not universal ratings, exact PSU models, redundancy,
measured draw, summed available capacity or auction-unit inventory. Null source
fields stay raw and are omitted from selected facts under the old rule.

Each jbb score retains the full `SPECjbb2015-MultiJVM max-jOPS` or critical-jOPS
label, plus `spec_jbb_mode`. Modes are not cross-comparable. CPU rate/speed result
facts retain their distinct printed metric names and values. All benchmark
scores are historical run results, not hardware specifications. Software/JVM/
firmware/tuning/topology/RT curves and individual benchmark rows are not hardware
facts selected here.

None of the twelve CPU captures contains `POWER-RELEVANT HARDWARE`, so **no CPU
power-supply fact** is added. This does not mean the system lacks a PSU or other
power-related tuning prose. Availability dates and result dates are historical
source statements, not current support/stock, manufacture or physical-unit dates.

## reproducibility and checks

From project root, existing runtime only:

```sh
python3 -B research/batches/discovery-2026-10-12/spec-continuation-facts/build_candidate.py --check
```

The builder's `--check` is read-only: byte-compares candidate/mapping/provenance
and copies, rechecks source pins and current source-ID absence/namespace routing,
then verifies neither queried catalog changed during observation. It does not
import data, create a database or rerun the network collector. Existing differing
outputs fail rather than overwrite. The offline checker additionally re-resolves
all fact evidence, verifies all47 frozen discovery artifact hashes, uniqueness,
full names, role/rule boundaries, case-only collisions and snapshot copies.
Its saved checks show staged bytes unchanged under `--check`.

Candidate SHA-256: `580ff0e2aa3f46b996fc28612fe8d0489de9a8ca8fdd649948dd202ffc8b8608`.
Builder SHA-256: `e74fe084c4f52966ab824274534e19ebc83eee1e8e0a212670217158748e0566`.
All local artifact hashes are in `SHA256SUMS`; frozen production hashes recorded
in checks still equal accepted curated-code review pins.

Stop condition met: finite18-page candidate, source-scoped evidence, read-only
routing and checks. Independent candidate review precedes any coordinator-only
integration. Full-suite/rebuild work runs separately; no import acceptance,
catalog count change or speed claim is implied.
