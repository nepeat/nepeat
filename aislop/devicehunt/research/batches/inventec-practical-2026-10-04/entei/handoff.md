# entei practical evidence handoff

outcome: named-source task complete; import proposal pending independent acceptance.
no shared database, manifest, exports, code or current docs changed. no PDF proposal.

- candidate: `candidate-curated.json`, SHA-256 `f7eb3c11e98967b448b80b145f0b66f6135224d021a54d60901d4b560162e5eb`; one exact Inventec Entei record, 20 facts including 14 practical statements; unique source `inventec-entei-announcement-specs-2020`.
- quote/line provenance: `evidence.json`, SHA-256 `426b5d37dce003eb0613ee57b1ead7a6f7c83e5712fe8df08233703846259a7b`. every claim binds original URL, saved capture SHA and literal HTML line text.
- accepted evidence capture: `../../../../data/raw/inventec-practical-2026-10-04/entei/entei-announcement-2020.html`, SHA-256 `f7e99a943c6eedd406fff2d2b87a1573f9fbeb56fc166f40e7f4796f1f2e9396`; HTTP 200 primary Inventec article `/cn/news/新闻与公告/2020/29`.
- source printed date: 14 August 2020 (HTML 315, 321); capture 2026-10-05T00:03:32.241613+00:00, equivalent to 2026-10-04 local. article announcement date is not a separately verified commercial shipping date.

supported practical fields: 2U; dual second-generation Intel Xeon Scalable; twelve DIMMs/up to 768GB using 64GB DIMMs; DDR4; Intel Optane persistent memory up to 1TB as stated; Intel C621; 24x3.5-inch plus 4x2.5-inch hot-swap drives; rear NVMe caching; two M.2 through interposer; storage mezzanine-card options without named cards; drawer serviceability; printed three PCIe x16 lanes/up to four cards via risers; OCP 2.0 NIC mezzanine option 10G–40G optical or Base-T as stated. see HTML lines 321, 330, 333–334, 337, 340. preserve PCIe and NIC wording without silently resolving their meaning.

roles: this proposal attaches brand/platform_vendor only. existing dated manufacturer source stays separate. no model alias, specific factory, operator, hyperscaler deployment or auction-unit configuration inferred. original manufacturer article (saved, not refetched) corroborates Entei's Intel Purley/2U24Bay/dual-socket wording, but no second proposal repeats those facts.

reserved capture: coordinator authorized the exact article link `/en/product/Storage/2U%2024Bay/Entei`; it redirected to generic `/en/product` with HTTP 200 and zero Entei text. saved `entei-product.html` SHA-256 `3176d41b82bb0f5cfe38a8123c65b8858b9fe8a724b058077690d895357af1d3` is excluded from model evidence. final response is a generic index, not an empty Entei specification and not proof that no Entei page ever existed. no retry/alternate language/path attempted.

remaining gaps: PSU watts/input/connector/redundancy; fan/cooling/noise; rack depth/weight/rails; BMC/management interfaces; OS and firmware/download availability. bounded inspected-corpus gaps, not worldwide absence.

checks: JSON parses; reviewed source and importer-compatible repeated fact names; snapshot path resolves from actual candidate directory to captured article; all practical claims have exact matching cleaned line quotes containing Entei; redirect page lacks Entei; no imports performed. snapshot path is relative to this candidate's location: integrator must rebase if relocating the JSON. source hashes and literal excerpts should be reviewed independently before acceptance.

budget: one of three permitted search queries; two source captures, with second explicitly allocated from coordinator's remaining batch budget. actual query/outcome and saved-input list in `queries-outcomes.json`; both curl reports retain original/final URLs, times, bytes and SHA-256. stopped after named source review and capture budget. next bounded step is independent candidate review; further collection needs a distinct primary exact-model lead.

coordinator follow-up: practical fields use recognized cpu/memory/memory_as_printed/storage/expansion names where applicable. the two PCIe sentences are separate expansion facts preserved literally; duplicate fact names are intentional source statements.
