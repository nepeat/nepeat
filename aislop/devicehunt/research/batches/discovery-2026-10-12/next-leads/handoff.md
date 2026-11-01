# next original-vendor leads — offline plan, 2026-10-12

owner `/root/psu`. **five actionable leads; zero network requests**. recommend
priorities 1–3 first: SD6200 datasheet, WEDGE100-32X R04, and two 7420 service
pages. these are links extracted from immutable saved original vendor HTML,
not guessed filenames or a repeat of failed acquisition. current availability
and the presence of the sought facts are unknown.

`plan.json` contains exact captured/resolved URLs, source-link provenance,
coverage gaps, limits and stop rules. `local-proof.json` binds the input hashes
and read-only catalog fact queries. linked target documents have no exact URL
match in current sources or the three relevant vendor request ledgers; no
corresponding local capture was identified in the coverage inspected. this
bounded check does not claim exhaustive uniqueness over every historical file.

| priority | exact scope / fetch | existing coverage and useful gap |
| --- | --- | --- |
| 1 | **Celestica SD6200**: [linked datasheet](https://www.celestica.com/uploadedFiles/Site/our-expertise/hardware-platform-solutions/storage/storage-list/cls_datasheet_SD6200.pdf) | current portfolio/product facts establish 4U, 90 HDD, SAS-4 and source manufacturer statement. seek input/PSU, cooling, depth/weight and host/cable constraints. no categorized power/cooling or dimensional facts found. |
| 2 | **Edge-Core WEDGE100-32X**: [R04 download landing](https://www.edge-core.com/download/wedge-100-32x-r04/), then its one published PDF link | current product HTML has 1RU, redundant AC or 12 V DC options, 4+1 fans, front(port)-to-back(power), ONIE. seek complete ordering/FRU codes, precise input conditions, depth/rails and revision-scoped distinctions. |
| 3 | **Oracle 7420**, service-platform name: [internal components](https://docs.oracle.com/en/storage/zfs-storage/zfs-appliance/os8-8-x/service-manual/sun-zfs-storage-7420-internal-components.html), [attached storage](https://docs.oracle.com/en/storage/zfs-storage/zfs-appliance/os8-8-x/service-manual/sun-zfs-storage-7420-attached-storage.html) | existing captured PCIe page provides component marketing/manufacturing mappings. seek chassis PSU/fan/service-part facts and documented shelf/controller/cable limits. current `ZFS STORAGE 7420` and `S7420` namespaces remain separate; review identity routing before any candidate. |
| 4 | **Edge-Core MINIPACK AS8000 OPEN MODULAR PLATFORM**: [R02 quick-start landing](https://www.edge-core.com/download/as8000-quick-start-guide-r02/), then its one published PDF link | current broad AC/fan/ONIE facts and separate Minipack-AS8000 order/component retirement entries lack operating ratings. seek connectors/input, module requirements, rack/rail clearance and service restrictions. an empty chassis or SCM is not a complete working system. |
| 5 | **Celestica SC6100**: [linked datasheet](https://www.celestica.com/uploadedFiles/Site/our-expertise/hardware-platform-solutions/storage/storage-list/cls_datasheet_SC6100.pdf) | current 2U, 24 SSD, HA, PCIe Gen5 descriptions and source manufacturer statement. seek precise input/PSU, cooling, dimensions and controller/drive conditions. installed drives remain unknown. |

saved link bases: `data/raw/celestica-storage-manufacturer/{sd6200,sc6100}.html`,
`data/raw/edgecore/product-{wedge100-32x,minipack-as8000-open-modular-platform}.html`,
and `data/raw/oracle-zfs-components/sun-zfs-storage-7420.html`. exact link
presence was parsed/checked locally. the two Edge-Core saved links include old
`refresh` tokens; use the canonical landing and only its newly published link,
not token guessing, a private endpoint or an alternate copy.

## finite fetch budget and stopping point

first wave priorities 1–3: **at most ten total HTTP requests**, zero searches,
zero remote screenshots. optional full five-lead set: **at most fourteen total
HTTP requests** = eight document requests, up to three redirects, and up to
three robots/policy requests if required. Edge-Core PDF redirects count within
these caps. unused requests are not an invitation to widen the source set.
all target links come from saved pages, so product/index recaptures add no value.

save original bytes, requested/final URLs, UTC timestamps, status/outcomes,
SHA-256 and exact request inputs. use the reviewed local extractor if PDFs are
successfully captured; at most six local page renders for targeted table/identity
QA. select finite useful facts, or report that a requested field is absent.
preserve suffixes, full order codes, revisions, grouped options and contradictions.
no decoder-by-convention, cross-platform fitment, current support/stock,
factory inference from vendor branding, or auction-unit configuration follows.

stop the affected source on 402/403, login requirement or robots prohibition;
no retry/alternate host/copy/route. stop the whole fetch run on 429 and
checkpoint. record a 404/timeout once and stop that URL rather than guessing
revisions. stop after this source set and a small evidence/negative handoff.

## excluded or already covered routes

- Accton SAF6101I product/shop returned 402 and its July-2022 datasheet 403;
  the Joy announcement/filing and Accton corporate-name PDF also had denied
  original retrievals. no reattempt or alternate copy is proposed. a new
  independent public Accton technical link would justify a future plan.
- Accton/Cisco MX100 certificate and Aruba legacy interoperability PDF were
  denied; Celestica UL pages, historical corporate exhibit, and the Cisco
  factory certificate route also contain known denials. they do not belong
  in this practical-doc wave.
- Edge-Core's four legacy `_upload` 404s remain excluded: DCS203 R09,
  DCS510/AS9716 R09, AS7312 R02 and AS4630-54NPE R04. current successfully
  captured AS7712/5812/7326/7726/7816/4630 PDFs are already covered.
- Celestica DS4000 guide/eight chassis SKUs, 2025 catalog and networking index
  are already captured. no duplicate capture or universal SKU/airflow rule.
- exact owned Oracle E4/E5/X7/plain-X10 manuals were searched in the prior
  finite pass without a new qualifying document. X10 GPU L40S/PCA guide and
  firmware/support policies are captured; MOS firmware/product-note links
  remain gated. the new 7420 service pages do not resolve those owned gaps.

stop condition met: small prioritized plan with saved original link provenance,
coverage and request caps. only `next-leads/` was written for planning; no
fetch/import, catalog, manifest, archive, export or current-doc edit occurred.
the separately authorized PSU review hash correction is recorded in its review.
