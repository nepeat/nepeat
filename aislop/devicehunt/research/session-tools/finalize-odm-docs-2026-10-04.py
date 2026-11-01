from pathlib import Path
import json,hashlib
from devicehunt.catalog import Catalog,TARGETS
from devicehunt import validation,vendor_inventory
root=Path('/Users/nep/nocloud/git/nepeat/aislop/fcc_shenanigans');out=root/'data/exports';promotion=json.loads((out/'odm-promotion-2026-10-04.json').read_text());sha=hashlib.sha256((root/'data/devices.sqlite').read_bytes()).hexdigest();assert sha==promotion['after_sha256']
c=Catalog(root/'data/devices.sqlite',read_only=True);examples=[]
for query,target in [('D7032Q28B','Inventec'),('ATLANTA13','Hyve'),('CF-EDGE-COMPUTE','Hyve'),('Hornet16F','Wiwynn'),('QuantaMesh T7032-IX1B','Quanta'),('Hercules 9120','Hyve'),('Hercules 9120','TD SYNNEX'),('EP100G2','Wiwynn')]:
 rows=c.search(query,target=target);assert rows,(query,target)
 examples.append(dict(query=query,target=target,models=[dict(r) for r in rows]))
assert c.search('Hercules 9120',target='TD SYNNEX',role='manufacturer')==[]
base=validation.run(c.db,root/'validation/owned-hardware.json');assert base['summary']==dict(total=10,hits=10,gaps=0)
c.db.close();assert sha==hashlib.sha256((root/'data/devices.sqlite').read_bytes()).hexdigest()
(out/'odm-model-examples-2026-10-04.json').write_text(json.dumps(dict(database_sha256=sha,read_only=True,examples=examples),indent=2)+'\n')
p=root/'PROGRESS.md';s=p.read_text();s=s.replace('## ODM implementation checkpoint — 2026-10-04 (verification in progress)','## ODM expansion verified and promoted — 2026-10-04',1)
s=s.replace('A fresh candidate preserves every previous assertion/raw row and adds 304','The promoted catalog preserves every previous assertion/raw row and adds 304',1)
s=s.replace('''395 tests pass; the candidate retains 10/10 owned baseline hits (9 documents +
1 label photo). Candidate preservation audit:
`data/exports/odm-candidate-review-2026-10-04.json`. A second full rebuild is
running against the reviewed 104-job / 2,050-file manifest before promotion.
The live catalog remains the prior snapshot until that check finishes.''','''395 tests pass; the live catalog retains 10/10 owned baseline hits (9 documents +
1 label photo). A second full rebuild matched all 42 checked tables/FTS indexes
and baseline results against the reviewed 104-job / 2,050-file manifest. The
previous database is preserved at `data/devices-before-odm-2026-10-04.sqlite`.

Current database SHA-256: `'''+sha+'''`.
Verification: `odm-candidate-review-2026-10-04.json`,
`odm-verified-rebuild-2026-10-04-comparison.json`, `odm-promotion-2026-10-04.json`
and `tests-odm-expansion.log` under `data/exports/`. The latest clean rebuild is
`data/rebuilt-with-odm-targets-2026-10-04.sqlite`. Historical handoff hashes below
refer to the prior snapshot.

Try `vendor-inventory Hyve --limit -1`, `search ATLANTA13 --vendor Hyve`,
`search Hornet16F --vendor Wiwynn`, `search D7032Q28B --vendor Inventec`, or
`vendor-inventory 'TD SYNNEX' --scope all`. Complete example commands need the
`python3 -m devicehunt` prefix. Snapshot-qualified manufacturer totals across
all targets are 2,134 target/identity pairs / 2,133 identities; all associations
are 3,971 pairs / 3,837 identities. Original five target inventories are unchanged.
Hyve's stored ATLANTA model fields retain their `/ Server` suffix text verbatim.

FCC coverage now counts 417 known target-associated strings: index 210, detail
171, application 2, FCC-ID-linked PDF 1. Missing at those stages: 207 / 246 /
415 / 416. The global sitemap contains 306 of the 417 strings and lacks 111.
The PDF archive remains 36 files / 3,371 pages, including non-FCC references.
Source failures remain recorded; the SEC merger exhibit's original download
returned 403 and must not be retried through another route.

Next priorities:
1. Review/capture more original model-specific Hyve, Wiwynn, Inventec and Quanta
   server/storage evidence and narrow the datacenter buying shortlist. Manufacturer
   totals include consumer devices and community claims; do not count them as
   known hyperscaler deployments.
2. Resolve Foxconn/Hon Hai/FIT/Ingrasys and QCT/Quanta corporate relationships with
   dated evidence and explicit roles. The existing CubScout FIT manufacturer claim
   remains separate and unassigned to the Foxconn target; no blanket alias was added.
3. Expand regulator queries and review additional exact company/Chinese legal names
   for all new vendors, then import source-qualified model records. NCC/BSMI raw
   snapshots have not yet received a comprehensive new-vendor mapping pass.
4. Use the refreshed per-vendor FCC gap reports for bounded filing/exhibit collection,
   preserving known denials and incomplete-mirror limits. Continue original-company
   coverage and exact ordering-code research alongside these new vendors.

The full goal remains active; complete historical device/FCC coverage is unproven.''',1)
s=s.replace('## Resumed and expanded 2026-10-04','## Initial resumption snapshot — before the expanded import',1);p.write_text(s)
p=root/'GOAL.md';s=p.read_text();needle='Status: resumed by the user\'s explicit request. This replaces the prior\nfive-vendor research scope and the September handoff\'s instruction to wait.';s=s.replace(needle,needle+'''

Implementation checkpoint 2026-10-04: all eleven filters now work. The verified
catalog has 3,837 identities, 5,007 source records and 223 sources; 395 tests and
the ten-device baseline pass. A clean rebuild matched 42 tables/indexes. See
PROGRESS.md for current evidence counts, caveats and remaining research priorities.
The objective below remains active; this checkpoint does not establish completeness.''',1);p.write_text(s)
p=root/'README.md';s=p.read_text();s=s.replace('''These are research targets; reviewed CLI vendor mappings and imported coverage
are being expanded separately. Generic `lookup` already accepts any search text.''','''All eleven vendor filters are now available. The verified catalog contains
3,837 identities and 5,007 source records, including 304 identities added by the
expanded source review. Use `vendor-inventory Hyve --limit -1`,
`search Hornet16F --vendor Wiwynn`, or `vendor-inventory 'TD SYNNEX' --scope all`
after the `python3 -m devicehunt` prefix. TD SYNNEX's current association is a
dated parent-company role, not a manufacturer claim. See
[the exact target mappings and exclusions](research/odm-target-registry.md).''',1);p.write_text(s)
p=root/'research/fcc-source-coverage.md';s=p.read_text();s=s.replace('# FCC source coverage\n','# FCC source coverage\n\nCurrent expanded-target snapshot (2026-10-04): 417 known associated FCC strings;\n210 have index records, 171 have detail captures, 2 have application records and\n1 has an FCC-ID-linked PDF. The global sitemap contains 306 of these strings.\nSee `data/exports/fcc-coverage-all.json` and `fcc-sitemap-summary.json`.\nThe following dated entries retain prior-scope collection history.\n',1);p.write_text(s)
p=root/'research/odm-target-registry.md';s=p.read_text();s=s.replace('''expanded imports are staged in a fresh candidate database; only a reviewed,
verified rebuild should replace the current database. Recheck the latest dated
progress note for final promoted counts and test results.''','''expanded imports preserved all original assertions/raw rows and were promoted
after a second clean rebuild matched all 42 checked tables/indexes. 395 tests
pass; the ten-device baseline remains 9 document hits plus one label-photo hit.
Current counts and remaining gaps are in PROGRESS.md.''');p.write_text(s)
print('Final scope documentation and live CLI examples verified.')
