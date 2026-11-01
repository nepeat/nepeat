"""Record the promoted milestone and exact remaining gaps in handoff documents."""
import json
from pathlib import Path
from devicehunt.rebuild import file_sha256
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'data/exports'
p=json.loads((OUT/'bytedance-inspur-promotion-2026-10-04.json').read_text())
assert file_sha256(ROOT/'data/devices.sqlite')==p['database_sha256']
progress=ROOT/'PROGRESS.md';old=progress.read_text()
section='''## ByteDance / Inspur expansion verified — 2026-10-04

The active project scope now has **thirteen target groups**, adding ByteDance and
Inspur to the previous eleven. GOAL.md and AGENTS.md contain the expanded scope.
The app goal remains active; its older objective text may still name eleven
groups because the available goal tool edits status only. Do not complete or
recreate the unfinished goal to change its wording; follow the latest user scope.

Current live catalog: **3,892 identities / 5,070 source records / 231 sources**,
schema 27. It includes **13 Inspur identities** across four sources, three with
regulator manufacturer claims. ByteDance has **zero qualifying catalog identities**
so far. The examples `BYTEDANCE B4020` and `BYTEDANCE B4010` are in
`validation/vendor-examples.json`: **0/2 supported; both explicit coverage gaps**.
These are not asserted as owned devices. The owned baseline remains **10/10**
(9 documents + 1 label photo).

An original Accton-hosted BTL PDF names B4010 with brand N/A and Accton as
applicant. Its separate catalog record provides a model lead, without claiming
ByteDance manufacture, brand or deployment. No FCC ID is assigned. The original
34-page Inspur i24 white paper supports the exact legal-name mapping and a
platform record. See [the source review and exclusions](research/bytedance-inspur-2026-10-04.md).

Two five-vendor assumptions were fixed: the NTRA PDF page screen and the offline
HTML vendor selector now follow the target registry. NTRA records rose from 119
to 169: +17 Hyve, +20 Foxconn, +9 Quanta, +4 Inspur brand/model assertions.
Including the other new sources, this pass adds 53 identities / 60 records.
All original source assertions, raw rows and identity keys survive. Seven Quanta
display titles/categories now come from the earlier NTRA source in rebuild order;
those changes are individually recorded in the preservation audit. Manufacturer
counts for the original five targets are unchanged.

Manufacturer inventory: 2,137 target/identity pairs, 2,136 distinct identities.
All associations: 4,027 pairs, 3,892 identities. Hyperscaler assertions and FCC
discovery/filing claims are separate from these totals. The PDF archive now has
**40 PDFs / 3,410 pages**, including vendor/reference documents.

The interrupted prior milestone was completed first: original Wiwynn SV7100G4 /
SV7100G4-C and Hyve Jade datasheets were imported, preserving chassis/blade and
parent-company distinctions. See [that model review](research/odm-models-2026-10-04.md).

Verification: **399 Python tests + 6 report-script tests**; fresh 106-job rebuild
from 2,071 hashed inputs matched all 42 tables/FTS indexes and the owned baseline.
The report's generated controls include all thirteen targets, including zero-hit
ByteDance. Browser visual QA was unavailable: automated file:// navigation was
blocked by browser policy, with no workaround attempted. DOM-script and generated
HTML checks remain the UI validation evidence.

Current database SHA-256: `HASH`.
Clean rebuild: `data/rebuilt-with-bytedance-inspur-2026-10-04.sqlite`.
Previous live snapshot: `data/devices-before-bytedance-inspur-2026-10-04.sqlite`.
Proof under `data/exports/`: `bytedance-inspur-review-2026-10-04.json`,
`bytedance-inspur-rebuild-2026-10-04-comparison.json`,
`bytedance-inspur-promotion-2026-10-04.json`, `vendor-example-validation.json`,
`latest-export-audit.json`, and the dated test logs.

Try these with the `python3 -m devicehunt` prefix (use the bundled runtime below):
`vendor-inventory Inspur --scope all --limit -1`, `search B4010`,
`documents B4010`, `search Jade --vendor Hyve`, and
`validate-inventory --inventory validation/vendor-examples.json`.
The example check intentionally exits 2 for its two unresolved gaps.

Next bounded work:
1. Find primary ByteDance-labelled/compliance/product evidence for B4020/B4010;
   keep Accton applicant, OEM, product brand and operator claims separate.
2. Pursue the captured Ingrasys DOCS model groups, older liquidation-friendly
   Hyve/Wiwynn/Inspur platforms, and Inventec/Quanta server/storage identifiers.
3. Review additional NCC/BSMI company mappings and original-vendor FCC/order-code
   gaps. Exact name filtering excludes unreviewed subsidiaries/composite cells.

New unavailable sources: Inspur about page returned 403; Eximpedia company page
returned 402. Preserve those outcomes and do not retry through alternate routes.
Trade-data preview labels remain discovery leads; no paid acquisition is authorized.
The full goal remains active; complete historical coverage is unproven.

'''.replace('HASH',p['database_sha256'])
assert '## ByteDance / Inspur expansion verified' not in old
progress.write_text(old.replace('# device hunting — active continuation\n\n','# device hunting — active continuation\n\n'+section,1))
goal=ROOT/'GOAL.md';text=goal.read_text()
start=text.index('Implementation checkpoint 2026-10-04:');end=text.index('\nBuild a local SQL-queryable',start)
text=text[:start]+'''Implementation checkpoint 2026-10-04: all thirteen target filters work in the
CLI and generated HTML. The verified catalog has 3,892 identities, 5,070 source
records and 231 sources; 399 Python tests plus 6 report-script tests pass. The
owned baseline is 10/10. ByteDance B4020/B4010 examples remain 0/2 external-evidence
hits and explicit research gaps. A clean rebuild matched 42 tables/indexes.
See PROGRESS.md for current evidence counts and priorities. This is an active
collection goal; the checkpoint does not establish completeness.
'''+text[end:]
goal.write_text(text)
readme=ROOT/'README.md';text=readme.read_text();start=text.index('Research resumed');end=text.index('\nStart with [PROGRESS.md]',start)
text=text[:start]+'''Research resumed 2026-10-04. The active targets are Accton, Edge-Core, Celestica,
Oracle, Amazon Data Services, Inventec, Hyve, Foxconn, Wiwynn, TD SYNNEX / SYNNEX,
Quanta, ByteDance and Inspur, with other ODMs retained as evidence-backed leads.
See [the expanded goal](GOAL.md) and [current next steps](PROGRESS.md).

All thirteen filters work in the CLI and offline HTML catalog. Current totals:
3,892 identities / 5,070 source records. Inspur has 13 source-qualified identities;
ByteDance B4020/B4010 remain explicit evidence gaps. A B4010 compliance statement
names Accton as applicant, with brand N/A; it does not establish ByteDance or
manufacturer attribution. See [the new source review](research/bytedance-inspur-2026-10-04.md).

Use `vendor-inventory Inspur --scope all --limit -1`, `documents B4010`, or
`validate-inventory --inventory validation/vendor-examples.json` after the
`python3 -m devicehunt` prefix. The examples are separate from the ten owned
devices and are never imported as evidence. [Exact mappings and exclusions](research/odm-target-registry.md)
preserve brand, applicant, manufacturer, parent and operator roles.
'''+text[end:];readme.write_text(text)
compare=ROOT/'scripts/compare_rebuild.py';text=compare.read_text().replace('data/rebuilt-with-odm-targets-2026-10-04.sqlite','data/rebuilt-with-bytedance-inspur-2026-10-04.sqlite');compare.write_text(text)
helpers=ROOT/'research/session-tools/README.md';text=helpers.read_text();import re
text=re.sub(r'^\d+ transient research/import/audit scripts',str(len(list(helpers.parent.glob('*.py'))))+' transient research/import/audit scripts',text,flags=re.M);helpers.write_text(text)
print('Updated current handoff, goal, README, rebuild default and helper inventory.')
