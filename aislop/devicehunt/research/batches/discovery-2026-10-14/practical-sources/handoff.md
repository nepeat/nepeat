# practical-source acquisition and candidate

owner `/root/practical_sources`; batch 2026-10-14 local, captures 2026-10-15 UTC.
**complete finite source set; local candidate awaits independent review.** only
`research/batches/discovery-2026-10-14/practical-sources/` was intentionally written.
shared code/catalog/manifest/export inputs were read-only; no import, package
installation, source expansion or subagent work occurred.

the coordinator separately authorized execution of [the r3 plan](../../discovery-2026-10-12/next-leads-r3/plan.json)
and later explicitly requested a curated candidate in this directory. that later
brief supersedes the original plan's discovery-only/no-candidate boundary; it
does not authorize this worker to import. [input pins](input-pins.json) preserve
the plan hash, verified parent-capture hashes and read instruction hashes.

## captures and limits

[requests.jsonl](requests.jsonl) retains every invocation URL/argv, UTC interval,
outcome, policy, manual redirect parent, body/header filenames and SHA-256.
**six actual HTTP exchanges**, cap eight: two fresh host policies, three exact
planned routes and one manual same-origin SD4100 redirect. standalone source
counts including applicable policy: SD4100 **3/3**, ZS3-2 **2/3**, ZS3-4 **2/3**;
the shared Oracle policy counts once overall. all fresh policies allowed the
exact paths for `devicehunt-public-research/1.0`, with no crawl delay.

the first sandbox invocation failed DNS (`curl6`, HTTP000, no response body or
actual exchange). its receipt/empty header is preserved. per tool requirements,
the same policy request was executed once with approved network escalation and
new immutable output names. all later capture commands used that approved
outside-sandbox capability. there was no remote retry, denial, timeout, throttle,
login/registration, alternate host/route, search, linked-resource crawl or remote
screenshot. seven invocations therefore represent six HTTP exchanges. no more
requests are authorized by unused budget: all three sources are processed.

| source / scope | outcome | capture SHA-256 |
| --- | --- | --- |
| `https://www.celestica.com/hps/sd4100` | 302, empty body; one literal same-origin Location followed after offline novelty/scope check | body `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `https://www.celestica.com/uploadedFiles/Site/our-expertise/hardware-platform-solutions/storage/storage-expansion/cls_branded_datasheet_SD4100.pdf` | 200; complete two-page SD4100 datasheet | `7d8694a1b311ff65ca654c41d3c5da7d85661c7911a3b49b3e115eaec9d82385` |
| `https://docs.oracle.com/en/storage/zfs-storage/zfs-appliance/os8-8-x/service-manual/oracle-zfs-storage-zs3-2-internal-components.html` | 200; complete saved article/table markup | `cafa86fa34e14d2586d77e34b27a5eb2258670d59a54755f487670195f1513f1` |
| `https://docs.oracle.com/en/storage/zfs-storage/zfs-appliance/os8-8-x/service-manual/oracle-zfs-storage-zs3-4-internal-components.html` | 200; complete saved article/table markup | `9b4cbdcbc1aac1f5b6e360b5779fb1cfdd9280869833604785a623e5ff36e6f4` |

[redirect check](sd4100-redirect-check.json) records the literal Location and
bounded exact filename/raw metadata and read-only accepted catalog checks;
none matched the new target. this is not a global novelty/coverage claim.
[capture report](capture-report.json) and [artifact manifest](artifact-manifest.json)
retain concise outcomes and local file hashes. source bytes and original
headers remain immutable; generated proposal/check files are separate.

## extraction and reviewed source scopes

the reviewed unchanged `scripts/extract_pdf_pages.py` produced the complete
[SD4100 sidecar](sd4100-pages.json), validated by `documents.read_capture`.
method **Poppler pdftotext -layout embedded text; no OCR**, installed tool pair
26.06.0; sidecar retains exact commands, options, pdfinfo, versions and hashes.
both pages were rendered at150dpi and visually inspected: [p1](sd4100-render-page1.png),
[p2](sd4100-render-page2.png). **two local renders**, cap six; no further render,
crop or OCR. [render provenance](render-provenance.json) pins PNG/source hashes.
two-column extraction interleaves prose near PSU/approvals/order text; separate
literal quote fragments plus visual layout preserve their correct associations.

Oracle HTML was reviewed against original article/table markup with the offline
[article extractor](observe.py). complete article blocks, raw source line
locators and all **seven** literal tables remain in `zs3-2-article.json` and
`zs3-4-article.json`, with readable companion text. remote figures/assets and
linked specification/software pages were not fetched.

[findings](findings.json) contains52 selected observations:26 SD4100,9 ZS3-2
and17 ZS3-4, plus all7 literal HTML tables. these discovery observations are
distinct from the subsequently requested curated proposal.

**SD4100**, PDF p1:2U/19-inch rack fitment, up to12 3.5-inch drives, SAS-4
uplink/SAS-3 or SATA-3 drives,13W maximum per drive bay at35°C. two hot-swappable
ESMs; four24G SAS-4 mini-SAS HD ports and one UART via USB-C in the ESM feature
block. **two800W CRPS PSUs**, source Lot-9 claim, printed **`100-127,200-240V AC input`**;
**three40mm dual-rotor fans per ESM**, not per chassis. height87mm(2U),
width446mm, depth545mm. operating5–35°C versus non-operating-40–60°C;
humidity8–80%RH without an added non-condensing qualifier; altitude clause
0–950m with1°C derating per300m above950m; sound power at23+/-2°C<7.6Bel.
no chassis weight, AC frequency/current, exact PSU/fan FRU or measured appliance
draw appears.800W is the printed PSU rating, not actual chassis consumption.

**R4086-F9001-01**, PDF p2:the SD4100 Order Information block explicitly lists
JBOD system(chassis,PSUs,SAS-4 I/O modules,fan modules), **12x drive carriers**,
**6x dummy drive carriers**, rail kit set. retain both carrier rows literally;
do not infer18 drives or installed drives. the exact order identity receives
only its own p2 order code/bundle and explicit SD4100 heading context; p1 family
ratings are not copied to it. printed footer **121224** and copyright2024
remain literal metadata, without decoding into a publication/manufacturing date.

**Oracle ZFS Storage ZS3-2**, saved HTML:motherboard/SP uses host3.3V standby
whenever AC arrives, including system-off state; PDB distributes main12V,
supplies connect directly to PDB, and top-cover interlock is explicit.
three risers/two PCIe CRUs per riser, sixteen DDR3 LV DIMM slots, identical DIMMs
required in all sockets, eight-disk backplane. Table11-4 title is literally
“Controller CPU Descriptions” despite DIMM-slot content:256GB row and second
capacity **`.`** remain separate literal rows; no missing capacity is repaired.
Table11-5 parent service-reference rows are:

| Component | Description | FRU/CRU | Part Number |
| --- | --- | --- | --- |
| CPU | Intel E5-2658, 2.1G, 8-core | FRU | 7019701 |
| Memory | DIMM, 16GB,DDR3,1600,2Rx4,1.35V | CRU | 7041603 |
| Memory | DIMM, 32GB,DDR3,1066,4Rx4,1.35V | CRU | 7055964 |

these exact codes remain parent facts, not controller identifiers or standalone
component identities. FRUs/internal cables are not customer-serviceable and
require trained Oracle technicians per the page. numbered legend9 Fan modules
and12 Power supplies are labels, not counts. no PSU wattage/model, AC rating,
fan-module quantity/airflow or chassis dimensions/weight appears in this article.

**Oracle ZFS Storage ZS3-4**, saved HTML:full printed service-platform name
remains separate from registry **ZS3-4**, pending any independently justified
future routing. host3.3V standby and main12V distribution remain internal rails.
PSUs connect through PSU backplane to PDB; vertical PDB links the power/fan/
drive/I/O boards. PSU0/1 are literal identifiers; **two fan power boards** are
explicit FRUs, not a fan-module count. six-drive backplane, mirrored boot drives
in slots0/1; read cache in controller slots2–5 or DE2-24 shelf, **never both
controller and shelf simultaneously**.16GB DDR3 on all eight risers supports
the source-stated1TB; required/optional grouped slots and identical-DIMM rule
remain intact. fan-fault LED conditions and possible overtemperature LED retain
their conditional language. no PSU wattage/model/AC rating, fan-module quantity/
airflow or chassis dimensions/weight is supplied. ZS3-2 direct PDB topology and
ZS3-4 PSU-backplane topology are separate model statements, not a contradiction
to normalize into a common architecture.

## curated proposal and checks

[candidate.json](candidate.json) SHA-256
`a431ea99bb69582ace0bebbeda90b2804959e3ffa91731707ae2eadc63568385`:
**3 sources /4 records /60 facts**, each with an evidence entry linking the exact
source hash, quoted text/cells, PDF page or original HTML lines and scope note.

- Celestica / `SD4100`:26 source-local facts.
- Oracle / `Oracle ZFS Storage ZS3-2`:14 facts, including5 literal table rows.
- Oracle / `Oracle ZFS Storage ZS3-4`:17 facts; full name distinct from `ZS3-4`.
- Celestica / `R4086-F9001-01`:3 own-order-block facts, sole new part-number
  identifier; no inherited p1 specifications.

only `documentation_vendor` affiliations are proposed. no manufacturer/factory/
deployment role, alias, unit-fitment inference, regulatory-certification claim
or current support/stock entitlement is introduced. two exact identities already
exist under the printed namespaces; full ZS3-4 service name and exact order code
are distinct proposed names. root owns final routing/acceptance.

[checks.json](checks.json) **passed**:all body/header hashes, fresh robots exact
paths, literal same-origin redirect, complete source-bound PDF sidecar, two render
hashes, reproducible HTML blocks/tables, all60 evidence locators, order p2 scope,
empty Oracle parent identifier lists and `prepare_curated` preflight. the checker
performed zero HTTP or database writes. no importer/schema/production change was
made, so no full suite or database rebuild was run by this worker.

**next bounded step:**independent coordinator/reviewer checks this exact candidate
and the two-page reference-document dependency; after acceptance the sole root
integrator may stage a fresh candidate/bundle preserving the complete existing
reference collection, then apply the normal offline rebuild/preservation gates.
no additional collection is needed to review these inputs. known denied Hyve,
Accton/UL/Edge-Core routes remain excluded; unused HTTP budget is closed.
