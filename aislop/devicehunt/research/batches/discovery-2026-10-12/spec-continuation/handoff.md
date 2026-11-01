# finite SPEC continuation

owner `/root/bytedance`; completed 2026-10-12 PDT / 2026-10-13 UTC.
**18 planned result pages captured, research only; no candidate/import or identity/role decision.**
Only this new directory contains outputs. No shared catalog, manifest, archive,
exports, production code or current documentation changed; no delegation/install.

[Bound plan](plan.json), [input and historical-failure provenance](inputs.json),
[request ledger](requests.jsonl), [outcomes](capture-outcomes.json),
[observations](observations.json), [checks](checks.json),
[finite collector](capture.py), [offline observation helper](observe.py).
Parent plan: [selection](../spec-next-plan/plan.json) and
[pure selection code](../spec-next-plan/plan.py).

## acquisition and policy

Exactly **19 actual HTTP exchanges**, within cap20: fresh robots first, six named
SPECjbb2015 HTML results and twelve CPU2017 text results. Every response200; no
redirect, retry, login marker, denial, timeout or throttle. No searches/screenshots,
CGI or alternate URL. Plain historical user agent:
`devicehunt-research/1.0 (non-commercial hardware identification research; low-rate)`.

Fresh [robots.txt](robots.txt) SHA-256
`cf5af82736d8ff7fe0ae89bda583603c005d6be47afa739ba93b2cc01795d4c8`
equals the prior wave10 bytes. It retains crawl-delay10 and `/cgi-bin/` denial.
Every exact result path was checked before request and allowed under the declared
agent/wildcard policy. The shortest recorded interval from previous completion
to next request start was **10.000170751009136 seconds**. Request ledger records
input argv/UA, UTC start/end, actual status, completion gap, policy hash,
unfollowed Location, response-header/body paths and hashes, bytes and stderr.
All bodies and raw response headers are retained; none was overwritten.

All604 historical ledger entries were read before collection. Prior failure
records remain explicit: two SPECpower redirect guesses and the known missing
`jbb2015-20150904-00007.txt`404. None of the18 selected exact URLs had a prior
request. The collector checks prior failures before each exchange, disables
automatic redirects/retries and refuses an existing run ledger. An affected
source would stop on failure/robots denial;429 would stop the entire run. No
such stop occurred, and the one spare HTTP allowance remains unused.

## bounded observations and limits

The six formerly remaining jbb HTML sources are:
`jbb2015-20151123-00036`, `-20160118-00041`, `-20160321-00065`,
`-20160321-00066`, `-20160321-00067`, `-20160516-00095`.
Each has one `Hardware hw_1` block; no `hw_2` block was found in this finite set.
The observer keeps every found hw_N block separate and would never attach
hw_2 fields to hw_1. Printed Vendor is `Inspur Corporation`; Name fields retain
`Inspur NF5280M4`, `Inspur NF5180M4`, `Inspur NF5270M4`, `Inspur NF5170M4`
and `Inspur TS860G3` spellings, including the repeated NF5280M4 results.

Five jbb rows print `Power Supply Quantity and Rating (W): 1 x 550W`;
TS860G3 prints `4 x 2000W`. Form Factor is2U for NF5280M4/NF5270M4,
1U for NF5180M4/NF5170M4 and8U for TS860G3. These describe historical submitted
benchmark configurations, not universal model ratings, redundancy, exact PSU
models, summed available output or an auction unit. No PSU-model detail was
supplied by a selected Power Supply Details field. CPU/memory/node fields,
availability and source `None` omissions remain with raw cell locators. Header
max-jOPS/critical-jOPS remain throughput for the printed SPECjbb2015 mode,
not hardware capability; modes are not cross-comparable.

The twelve selected CPU results span `cpu2017-20220325-31246` through
`cpu2017-20211206-30303`, exactly as selected from the saved index's newest
twelve of244 remaining distinct sponsor/full-system names. They retain complete
printed system header lines and all selected HARDWARE fields, including original
CPU suffixes, wraps and duplicate labels. **None has a POWER-RELEVANT HARDWARE
section**. No hardware-vendor splitting, suffix stripping, brand canonicalization,
model bridge or role decision was made. Raw index rows remain bound to each page.
This finite acquisition leaves232 of that plan's244 CPU names beyond this batch;
it is not an exhaustive current SPEC inventory.

`observations.json` contains18 pages with original body SHA, URL and capture UTC,
raw index row, header text and hardware evidence. The saved stdlib jbb cell parser
normalizes cell whitespace only; original HTML remains unchanged. CPU raw evidence
preserves lines and 1-based locations. Software/JVM/tuning tables and benchmark
scores were not selected as hardware-capability facts. Original SPEC/submitter
rights remain upstream. Current support, manufacturer/factory/operator or physical
unit provenance is not established.

## verification and next step

Checks passed for all19 response body/header hashes and body sizes; exact18-source
set; all completion gaps; per-path policy pins; plan/index/parser/old-ledger hashes;
and **354 raw evidence fields/header locators** against the unchanged captures.
Live catalog still has accepted SHA
`53a9a0eabaef5656b38ad922f96d963b76839eeda2da3b28a18b22359d857dee`;
manifest still `6ab359e22929bdb6a55e47c2a816c2c78fc3e5d0f90c8b252129f8fafea3aac5`.
All local artifact hashes are in `SHA256SUMS`; request/observation/helper hashes
are also in checks. No import, live query mutation or historical script with stale
scratch paths was run. The original source handoffs were read as historical context.

Stop condition met: all18 sources,19 exchanges, finite saved observations and
checks. Coordinator owns independent research review, any later exact identity/
role routing and candidate decisions. Do not rerun the network collector or expand
to the remaining232 names without a new bounded assignment.
