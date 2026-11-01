# original-vendor wave 5 — finite discovery

owner `/root/handoff_audit`, completed 2026-10-13 04:53 UTC. **all three assigned sources captured; no candidate or import.** only this new directory was written. parent [plan](../next-leads-r2/plan.json), [plan handoff](../next-leads-r2/handoff.md), source-specific failure notes and eleven relevant prior ledgers were checked before collection. production code and reviewed extraction utility remained unchanged.

[requests.jsonl](requests.jsonl) records every exact argv/input URL, UTC start/end, status, raw response-header/body filenames, bytes/hashes, manual redirect parent, user agent and fresh exact-path policy. [capture-report.json](capture-report.json), [published-downloads.json](published-downloads.json), [checks.json](checks.json), [findings.json](findings.json), [render-provenance.json](render-provenance.json) and [artifact manifest](artifact-manifest.json) retain the proof. acquisition helper [fetch.py](fetch.py) permits one exchange per invocation, disables automatic redirects/retries and rejects repeated URLs. do not rerun it: the finite source set is complete.

## acquisition

**nine actual HTTP exchanges**, within overall cap twelve; seven 200 and two counted 302 responses. standalone counts including applicable policy are Oracle 2/3, AS6700 4/5 and AS5712 4/5, with the shared Edge-Core policy counted once overall. fresh Oracle/Edge-Core robots allowed every exact requested path for `devicehunt-public-research/1.0`; neither declared a crawl delay for that agent. no denial, timeout, throttle, login, alternate copy/host, search or remote screenshot occurred. three unused requests do not authorize expansion.

| sequence | exact request / outcome |
| --- | --- |
| 1 | `https://docs.oracle.com/robots.txt`, 200 |
| 2 | `https://www.edge-core.com/robots.txt`, 200 |
| 3 | `https://docs.oracle.com/en/storage/zfs-storage/zfs-appliance/os8-8-x/service-manual/sun-zfs-storage-7420-specifications.html`, 200 |
| 4 | `https://www.edge-core.com/download/as6700-32x-quick-start-guide/`, 200 |
| 5 | `https://www.edge-core.com/download/as5710_as5712-54x-ds-r09-2/`, 200 |
| 6 | AS6700 landing's sole fresh `?wpdmdl=3288&refresh=6acdb799d770c1791866777` anchor, 302 |
| 7 | exact Location `https://www.edge-core.com/wp-content/uploads/2024/01/AS6700-32X-Quick-Start-Guide.pdf`, 200 |
| 8 | AS5710/AS5712 landing's sole fresh `?wpdmdl=3327&refresh=6acdb79f3e9e61791866783` anchor, 302 |
| 9 | exact Location `https://www.edge-core.com/wp-content/uploads/2024/01/AS5710_AS5712-54X-DS-R09-1.pdf`, 200 |

both zero-byte redirect bodies and all raw headers are retained. fresh link parent bodies and literal hrefs are pinned. no guessed PDF filename or revision was requested; 2024/01 upload paths do not establish document publication dates.

## complete extraction and visual review

the frozen reviewed `scripts/extract_pdf_pages.py` generated new `as6700-pages.json` (2 pages) and `as5712-pages.json` (4 pages), validated with `read_capture`. installed Poppler `pdftotext`/`pdfinfo` 26.06.0 used explicit per-page `-layout -enc UTF-8 -eol unix -remove-hyphens none -nopgbrk`. complete command/version/pdfinfo/warning provenance remains in sidecars. method is **Poppler pdftotext -layout embedded text; no OCR**. original PDF hashes did not change.

all six distinct PDF pages were rendered at 150dpi and inspected: `as6700-1.png`, `as6700-2.png`, `as5712-1.png` through `as5712-4.png`. p4 was additionally rendered/viewed at 240dpi to check a fan-code discrepancy: `as5712-reread4-4.png`. **seven local renders total**, cap eight; no clipping/cropping/OCR or image repair. source/tool/argv/PNG hashes are in render provenance. Oracle HTML was reviewed against original article/table markup with a stdlib text aid, not a screenshot.

## finite useful observations

`findings.json` holds **36 selected observations plus 38 literal ordering rows**, every one with an original HTML block/phrase or PDF page/line locator. `observe.py` rebuilds only these local research artifacts; `check.py` verifies source/sidecar/policy/render hashes and all 74 observation/table locators without requests/imports. discovery rows are not a curated schema or accepted identities.

**Sun ZFS Storage 7420**, service-manual specifications article:

- printed 3U controller, height13.3cm/5.25in, depth70.6cm/27.8in, width43.7cm/17.19in. weight is literally **16.36kg/96lbs**, inconsistent units: preserve both, no correction or conversion.
- input50/60Hz; **100–127VAC for 2 CPUs**, **200–240VAC for 2 or 4 CPUs**; maximum AC RMS12A@100VAC /12A@200VAC.
- Power Dissipation list labels **Max power consumption:1800W**, heat6143BTU/hr and1837VA@240VAC,0.98P.F. the preceding note expressly says dissipation numbers are maximum rated for the **power supply**, not actual appliance power consumption. retain the note with all list entries; do not relabel1800W as measured appliance draw or infer PSU quantity/model/redundancy.
- no M2-named specification, environmental limit, PSU part/order code or service-part table appears. plain printed Sun ZFS Storage 7420 scope does not transfer to M2, old `ZFS STORAGE 7420`/`S7420` or any specific auction controller.

**AS6700-32X / AS6701-32X**, complete joint quick-start:

- p1 joint heading and literal footer **E012014-CS-R01 /150200000827A**. the previously saved AS6700 product page's AS6701 prose remains a separate source conflict; joint documentation does not prove model equivalence.
- p1 locally defines wildcard part patterns **6700/6701-32X-D*-********** (switch software preloaded) and **6700/6701-32X-O-********** (ONIE installer preloaded, no switch software image). preserve wildcards, no expansion into full SKUs and no general suffix decoder/current entitlement claim.
- p1 all installed PSU/fan modules must match F2B or B2F airflow, indicated by module labels; installation passage says **one or two universal AC modules**, up to two PSUs matching fan trays. no DC rating/configuration is inferred from that AC passage. rack kit includes two brackets/eight screws; rack installation requires two people; earth connection must not be removed unless all supply connections are disconnected.
- p2 W×D×H438.4×473×43.4mm (17.26×18.62×1.71in); **8.382kg/18.52lb with two PSUs, five fan trays and two40G modules**. operating0–40°C, storage-40–70°C, humidity5–95% non-condensing. **PSU AC input100–240VAC50–60Hz6.0–3.0A**; switch consumption **375W maximum/267W typical**, not PSU output.
- p2 DCSS console settings and ONIE network-server versus attached-storage installer conditions are historical documented behavior, not a unit's installed firmware or image/license availability. no full chassis/PSU/fan ordering table, PSU output/model or replacement timing is supplied.

**AS5710-54X / AS5712-54X**, complete datasheet **EC-DS-0619-09**, copyright2019:

- saved `pdfinfo` Title metadata reads **AS5710_AS5712-54X ONIE DS R06**, conflicting with the visible R09 footer and fresh R09 landing. designation follows the visible printed footer; metadata stays unresolved, with no inferred revision history or alternate-version request. AS6700 metadata creation2009/modification2016 likewise does not override its literal E012014-CS-R01 footer or establish publication/unit manufacture dates.
- p1 standard19-inch rack or21-inch Open Rack using named adapter. historical AS5712 NOS compatibility retains versions Cumulus r2.5.1+, PicOS r2.6+, Pluribus ONVL2.5+, and SnapRoute; AS5710 does not inherit that list. no current support/download entitlement follows.
- p2 per-model CPU/memory rows remain separate: AS5710 Freescale P2041/1.5GHz versus AS5712 Atom C2538/2.4GHz. shared power block: two redundant/load-sharing/hot-swappable AC or-48VDC PSUs; **90–264VAC50–60Hz; -48 to-72VDC**; **6A@100/120VAC,3A@200/240VAC,10A@-72VDC**; up to93% efficiency explicitly for AC PSUs. **12VDC input option** remains separate. **282W maximum excludes pluggable optics**, not PSU output.
- p2 W×D×H44.3×47.3×4.34cm (17.4×18.6×1.71in),8.5kg/18.74lb **with two PSU modules**; hot-swappable4+1 fans; operating0–40°C, separate storage-40–70°C and humidity5–95% non-condensing.
- p3 has **24 exact full configuration rows**: twelve under each printed AS5710/AS5712 Base Model. row codes begin **5710/5712**, without adding AS; processor, dual AC/48V PSU versus one12V input, port-to-power/power-to-port and region columns stay attached to each literal row. no bare AC code or broader decoder is invented.
- p4 has **14 source rows**: four400W AC/-48V PSU FRUs, **PSU-12V-400-1** described only as a **12VDC power input unit FRU** (400 in the code is not an output rating), two fan rows, literal **57xx-54X-SVC-RTF** service pattern, RKIT-85-6712, ORSA-1U and four CBL-PWR region cable rows. keep blank cells blank. service text explicitly names AS5710; rack-kit model list is only source-listed context. cable numeric ratings/lengths and “only required with spare power supplies” remain row-specific.
- **unresolved p4 source discrepancy:** embedded text spells fan codes `FAN-1U-1x1D-F/B`; both150/240dpi renders visibly show `AN-1U-1x1D-F/B`. findings retain both observations and source methods. neither is resolved into a hardware identity, alias or silently repaired spelling. generic Accton subsidiary/manufacture footer prose remains context only, no per-model manufacturer/factory transfer.

all units, conditions, revisions, family splits, wildcard patterns and contradictory observations remain source-scoped. no aliases, company-role/manufacture/deployment inference, current certification/support/stock, inferred unit fitment or new candidate records were created. original upstream rights remain upstream. stop condition met: three sources, immutable bytes, complete sidecars, seven visual renders and finite findings. coordinator owns independent discovery acceptance and any later candidate/routing brief.
