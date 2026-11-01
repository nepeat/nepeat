# Oracle 7420 service-page discovery — 2026-10-12

author `/root/handoff_audit`; outcome: **complete bounded discovery**, both assigned pages captured. exactly three HTTP GETs: robots policy plus the two named pages. all 200; zero redirects, searches, screenshots, retries or other-page/image requests. robots permitted both named pages and declared no crawl delay. no throttling, denial or login requirement observed. request cap and two-page stop condition met; no follow-on crawl.

only this directory was written. original source basis, failure notes, plan and evidence policy read first; saved PCIe-page hash and both sibling link strings checked. fetch execution is recorded in `fetch.py` (redirects disabled and repeat-run guard); exact method, URL, User-Agent, timeout, UTC request/completion timestamps, status, size/hash and robots decisions are in `request-ledger.json`. original bytes in `robots.txt`, `internal-components.html`, `attached-storage.html`; `fetch-report.json` summarizes outcomes.

## useful source-scoped findings

the printed platform is **Sun ZFS Storage 7420**, in *Oracle ZFS Storage Appliance Customer Service Manual, Release OS8.8.x*. `findings.json` contains 12 finite literal claims with source URL/hash, DOM paragraph ordinals and original source text. `text-extraction.json` is a local HTMLParser article-paragraph aid, not a browser/image review.

- internal-component table lists Motherboard, Low-profile PCIe cards, Power supplies, Power supply backplane, Drive backplane, System lithium battery, CPUs and heatsinks, Memory risers, Fan board, Fan modules, Boot drives and SSDs. numbers 1–11 are figure legend labels: **3 is not three PSUs and 10 is not ten fans**.
- Power Supply Backplane Card explicitly connects the Power Distribution Board to **power supplies 0 and 1**. Power Distribution Board is a FRU distributing main **12V**; its bus-bar/ribbon-cable, Vertical PDB and top-cover interlock relationships are described. SP controller uses the host **3.3V standby rail** whenever AC input is present, even when the system is off. no PSU model/input rating/wattage or redundancy conclusion follows.
- **two fan power boards** are FRUs and carry fan-module power, status LEDs and I2C data; two is a board count, not fan-module count. Vertical PDB Card / Paddle Card and six-drive Drive Backplane are separately named FRUs. the six-drive backplane description does not establish six installed drives.
- the service manual says FRUs should only be replaced by trained Oracle service technicians. fan-fault/status LED behavior is described; no airflow direction or replacement module part code is supplied.
- separate named drive paragraphs stay separate: both describe mirrored boot drives in slots 0/1 and up to four read-optimized cache SSDs in slots 2–5. **Sun ZFS Storage 7420 M2** clustered controllers may have varying SSD counts; the **Sun ZFS Storage 7420** clustered paragraph says both controller SSD counts must match. do not transfer or reconcile these named scopes.
- attached-storage paragraph: **one to six chains**; each contains **up to six HDD-only or SSD-capable disk shelves, combined in any order**. no shelf model/order code, cable part number/length, port limit or wiring topology appears on that page. the linked cabling/shelf pages were not requested.

no identity routing is proposed. existing Oracle `ZFS STORAGE 7420` and `S7420` namespaces remain separate from each other and from the service-platform wording pending review. model-number resemblance is not equivalence; no component identity, manufacturer/factory role, specific-unit fitment/configuration, current support or firmware entitlement is inferred.

## source paths and hashes

all URL prefix references below mean `https://docs.oracle.com/en/storage/zfs-storage/zfs-appliance/os8-8-x/service-manual/`.

| original bytes | exact URL suffix | sha256 |
| --- | --- | --- |
| internal-components.html | sun-zfs-storage-7420-internal-components.html | 9bbfce09f2b51a8e0bad158e764bf307e53cb5b2c84b258b6c82ff903eb80a83 |
| attached-storage.html | sun-zfs-storage-7420-attached-storage.html | f3772553049f0e1a6671cb0579043be928550976129b2ea8e104f1ca3c4e6642 |
| robots.txt | https://docs.oracle.com/robots.txt | 40bfc5a2ef02ae1a03b2805ba1416ff9da1d74367f3dc8f0acaa7ac16de0e9c0 |

checks in `checks.json`: all three raw sizes/hashes match ledger; no requested/final URL difference; exact source section headings retained; article-paragraph selectors checked; input basis SHA matches plan (`d0e6fb8a…`). no code tests or imports are needed for these discovery captures. no live archive/database/manifest/export or existing source bytes were changed.

stop condition met. next bounded step is independent claim/routing review if integration is useful; do not fetch newly noticed links or resolve `7420` namespace equivalence without a new explicit brief.
