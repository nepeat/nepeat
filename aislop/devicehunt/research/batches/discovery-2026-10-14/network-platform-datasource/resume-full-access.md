# network-platform datasource — exclusive-ownership transfer checkpoint

root requested immediate stop/replacement because this worker retained the old workspace-write/network-restricted environment. no new remote calls after that request. owner was `/root/grouping_review`; all datasource writes are confined to this directory. **collection is partial; parser/tests/candidate generator have not been implemented.** no production/shared data edits, imports or schema changes occurred.

## actual collection and budgets

six of six allowed web search queries used, in four tool calls:

1. `site.docs.nvidia.com networking ethernet software cumulus linux hardware compatibility list supported hardware retired`
2. `site:nvidia.com/en-us/networking/ethernet-switching/hcl/ "Retired"`
3. `site:nvidia.com "Hardware Compatibility List" "Cumulus" "AS7326"`
4. `Cumulus Linux hardware compatibility list 4 switches Edgecore Celestica retired`
5. `site:docs.nvidia.com/networking-ethernet-software "Hardware Compatibility List" "retired"`
6. `site:docs.nvidia.com/networking-ethernet-software "Hardware Compatibility List" "Hardware" "Model"`

do not start further web searches without root expanding the cap. queries5/6 were unhelpfully dominated by other NVIDIA products. search results are in tool history rather than saved files; never treat snippets as a complete matrix.

[requests.jsonl](requests.jsonl) records seven acquisition attempts: **four actual HTTP exchanges, all200; three DNS failures with no HTTP response**. against the assigned ten-HTTP-exchange cap, six exchanges remain. `capture.py` currently enforces the stricter ten-total-attempt cap, so it would allow only three further attempts until intentionally adapted by the new owner. do not silently discard ledger entries. there were **no401/402/403/429 responses or robots denials**, no redirects and no retries of denied routes. no API content request was made.

| seq | request/result | saved body |
| --- | --- | --- |
|1|ordinary NVIDIA robots: sandbox DNS failure|receipt `raw/01-nvidia-policy.json`; no body|
|2|NVIDIA robots, escalated:200|`raw/02-nvidia-policy.body`, SHA256 `5069f0e6713ea599a59d48d5fb160ac859af14dcce9ec2a87fb9f554fe0f7125`|
|3|`https://www.nvidia.com/en-eu/networking/ethernet-switching/hcl/`,200, allowed by seq2 policy|`raw/03-cumulus-hcl.body`,512465bytes, SHA256 `e4216c1b2b979e23d6f95f9a57fda0d76abd2327e6a7984bfcb562467bc899df`|
|4|BISDN robots, escalated:200|`raw/04-bisdn-policy.body`, SHA256 `8ccb27dae29b2cf821c3fd5b163bbccef4b7df3ea08569ba8dcba3804868a39e`|
|5|`https://www.bisdn.de/en/linux/compatibility/`,200, allowed by seq4 policy|`raw/05-bisdn-hcl.body`,12602bytes, SHA256 `281d4717ae1003e3843ab4bd0eb1ccc0564f5bc729d1ab9d4b1095a6fda1635f`|
|6|ordinary `https://platforms.cumulusnetworks.com/robots.txt`:sandbox DNS failure|receipt `raw/06-cumulus-api-policy.json`; no body|
|7|same policy URL escalated:external DNS `[Errno -2] Name or service not known`|receipt `raw/07-cumulus-api-policy.json`; no body|

every successful capture has sibling `.headers` and `.json` receipts with timestamps, URL, hashes, policy context and no-auto-redirect metadata. original bytes must stay immutable. no final whole-directory SHA256SUMS has been generated.

two bulk source families were attempted (assigned cap two): Cumulus and BISDN. only BISDN currently has a captured complete actual matrix. the Cumulus page is a Vue shell with no actual platform rows. **unavailable is not an empty HCL.** its frontend literally references `https://platforms.cumulusnetworks.com/api/v1/hcl-platforms/` with `?limit=1000`; the hostname failed DNS even outside the sandbox. it also embeds a frontend Authorization header; this worker did not use it or request API data. no further domain retry or individual-device requests are justified by this checkpoint.

the captured Cumulus page explicitly links `https://support.cumulusnetworks.com/hc/en-us/articles/360038231814` for the newer Cumulus Linux4 list. that exact link is a possible bounded next route to a static official matrix, subject to policy, the remaining HTTP budget and root's judgment. it is not yet fetched. the requested current/retired sections have **not** been obtained or parsed; do not claim otherwise.

## source audit and coordination

read project AGENTS, evidence policy, workflow/current handoff earlier in this worker, `research/odm-target-registry.md`, `research/batches/discovery-2026-10-11/nos2/handoff.md`, PICOS and OcNOS notes/collectors. existing ONL/SONiC/ONIE/PICOS and DENT/Stratum definitions are extensively captured/imported. do not duplicate those source datasets. source-list entries are in `data/import-manifest.json`; collectors in `devicehunt/importers.py`, `devicehunt/onie.py`, `devicehunt/picos.py`, `devicehunt/ocnos.py` are useful patterns. Cumulus/BISDN full HCLs were not found among those existing datasource imports.

`/root/wave5_review` confirmed no overlap: it owns vendor-primary Celestica sitemap/resource graphs, saved Edge-Core product-download graphs, Oracle indexes and possible Accton sitemap work, not network-OS HCLs. root was told BISDN is the optional distinct second dataset.

BISDN's captured static table contains **eleven raw rows** with columns `Vendor`, `Platform`, `ASIC`, `Architecture`, `Release`; three printed vendor labels are Celestica, Delta, Edgecore. the actual capture prints release6.0.0 (older search snippet5.6.4 is not authoritative). platform strings include `Questone 2 D3030 (Questone 2A)`, `AG5648`, `AG7648`, four separate AS4610 variants, `EPS201 (AS4630-54TE)`, `EPS202 (AS4630-54PE)`, `DCS201 (AS5835-54X)`, `AS7726-32X`. retain each complete cell and row/download URL; do not silently expand parenthetical/grouped text into aliases or merge hardware identities. reported ASIC/architecture/release are row-scoped compatibility facts; no physical manufacturer, installed OS, present support guarantee or licence-transfer claim follows.

## code/state and next work

[capture.py](capture.py) is the only implementation so far. standard-library acquisition is isolated from future deterministic parsing; explicit host allowlist, robots gates,8MB cap,45s timeout, no automatic redirects/retries, response/hash receipts and denial stop are implemented. it has not been unit tested. its allowed hosts are `www.nvidia.com`, `www.bisdn.de`, `platforms.cumulusnetworks.com`; a new hostname needs an explicit reviewed allowlist addition. `tests/fixtures/` and `tmp/` exist but are empty. no parsed dataset, novelty report or curated candidate exists yet.

**no live tool handles remain.** all capture calls finished synchronously. the one pre-existing grouping suite75634 was polled once to terminal per root instruction: exit0,49passed,49.763s. its already-owned old handoff was updated as expressly requested; no further grouping work is needed here.

next useful local step: implement an offline parser for the complete saved BISDN table, with exact headers, complete row/cell validation, duplicate/qualifier retention, row-local links and fixture tests. preserve all eleven rows across all vendors, then generate discovery/candidate output and exact-model/identifier novelty against a read-only live catalog (bind its observed hash; current promotion state may have changed). avoid treating substring matches as identity equivalence. separately decide whether the remaining HTTP allowance can resolve the **literal** newer Cumulus HCL link; otherwise finish with Cumulus unavailable and a reusable BISDN matrix datasource rather than inventing results. production wiring stays a proposal until root assigns it.

stop/transfer is complete. this worker will make no further writes until reassigned.
