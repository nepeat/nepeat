# Device-hunting goal

Build a local SQL-queryable catalog and offline search tools for finding and
identifying **liquidated enterprise, networking and server hardware** on eBay
and other marketplaces. Prioritize exact model/order codes, sourced company
roles, and practical power, cooling, chassis, compatibility and software facts.
Hyperscaler connections are useful provenance context, not an inclusion requirement.

## Scope

Targets: **Accton, Edge-Core, Celestica, Oracle, Amazon Data Services, Inventec,
Hyve, Foxconn, Wiwynn, TD SYNNEX / SYNNEX, Quanta, ByteDance and Inspur**.
Add other relevant vendors/ODMs through evidence. Keep the original vendors active
as coverage broadens. Include systems, switches, storage, appliances, nodes,
chassis, rack/power equipment and identifiable hardware components.

Use free/public and open-source acquisition. Keep code, data and research here.
Preserve exact names, suffixes, company roles, uncertainty and source provenance.
Discovery rows and marketplace leads are not proof of manufacture or deployment.
Follow [the evidence rules](docs/evidence-policy.md).

## Work in bounded milestones

Each milestone has a finite source set or development task and a clear stopping
condition. Use [the subagent workflow](docs/agent-workflow.md) for independent
research, engineering and review, with one integration owner for shared state.
Worker models: GPT-6.1 Sol on ChatGPT; latest Sonnet on Claude (user's current
baseline: Sonnet 5.5). See the workflow for model selection and handoffs.

A completed milestone has useful local searches, reviewed source-backed facts,
meaningful checks and a reproducible import when data changes. Preserve the
[ten owned-device checks](validation/owned-hardware.json), including their
nine-document/one-label distinction. Keep the separate
[ByteDance B4020/B4010 examples](validation/vendor-examples.json) as explicit
coverage expectations. A missing hit remains a gap; never fabricate evidence.

Complete worldwide device coverage or a full FCC archive is not claimed.
Progress means useful exact identities and specifications, resolved gaps and
reliable local tools; current status and next tasks live in [PROGRESS.md](PROGRESS.md).

## Ongoing documentation is part of the goal

After every milestone or interruption, **update and simplify the documentation**
so a future Astra, Fable or other agent can continue from files alone. Keep this
goal concise, replace stale progress with current verified state, maintain usage
and workflow instructions, and archive detailed history without losing evidence.
Give each fact one authoritative home; link to details instead of duplicating
logs and milestone narratives. Documentation review is a completion requirement,
with a named editor and independent reviewer where workers are available.
