# energystar_diff handoff (2026-10-11, lead-only, no import)

## Fetched
9 requests total, all HTTP 200, no retries, 3 s spacing, UA `devicehunt-research/0.1 (personal hardware research; ...)`.
robots.txt saved and obeyed (Crawl-delay 1; only /browse, /api/odata, etc. disallowed; /api/views and
/api/v3/views/*/export.csv are allowed). Same URLs as the pinned bundle.json files
(`https://data.energystar.gov/api/views/<id>` and `.../api/v3/views/<id>/export.csv?accessType=DOWNLOAD`).

Captures in `data/raw/discovery-2026-10-11-energystar-check/` with URL/status/UTC/sha256/bytes in `capture-manifest.json`:
robots.txt; servers (qifb-fcj2) metadata+csv; storage-block (3uec-2gqf) metadata+csv;
storage-file (put7-uu67) metadata+csv; network (n8cx-m62r) metadata+csv.
Each dataset's metadata was fetched once (not before/after like the pinned bundles), so no
before/after stability check on the fresh set. Scripts: `diff.py`, `fetch.sh` (this dir).

## Diff vs pinned snapshots (offline, keyed on first column ID, exact-string cell compare)
Headers identical for all four datasets.

| dataset | pinned | fresh | added | removed | changed |
|---|---:|---:|---:|---:|---:|
| Enterprise Servers | 495 | 496 | 1 | 0 | 1 |
| Storage block | 83 | 84 | 1 | 0 | 0 |
| Storage file | 35 | 36 | 1 | 0 | 0 |
| Network | 96 | 96 | 0 | 0 | 0 |

Metadata rowsUpdatedAt: servers 2026-09-26 13:16Z -> 2026-10-10 13:16Z; block 09-26 -> 2026-10-10 13:22Z.
(viewLastModified unchanged.) Looks like a periodic refresh.

Added rows:
- Servers 4693354: Dell Inc. / DELL, model name E103S, model number PowerEdge XR8720t, multi-node,
  PSU 1,800 W (all three configs), SERT idle 164.5 / 205.8 / 216.2 W (low/typical/high). No certification
  date column exists in this dataset. Not a target vendor.
- Storage block 4711794: NetApp EF80. Not a target.
- Storage file 4,710,094 (note thousands-comma in ID as printed): Huawei OceanStor Pacific 9520-G2. Not a target.

Changed row (servers 3543476, DELL PowerEdge R770, existing ID): processor/low-end config re-reported
(Xeon 6505P -> 6503P, 12 -> 8 cores, memory hynix -> Micron, many SERT scores, idle 194.90 -> 196.50)
and Markets gained "Taiwan". Not a target vendor.

## Target vendors
Regex over partner+brand: Quanta/QCT, Inspur/IEIT, Inventec, Wiwynn, Foxconn/Hon Hai/Fii/Fulian, Hyve, Celestica,
Oracle, Amazon, Accton/Edgecore, SYNNEX. Result: **3 rows pinned, 3 rows fresh in servers (the same Oracle
X9-2L, X10M, X11M); 0 in storage and network, before and after.** No new or changed target-vendor models.

## Recommendation
A refresh is not worth doing for target hunting: zero target-vendor additions or changes, and the only
new rows are Dell, NetApp and Huawei. Optional, low priority: fold into a later batch if the project wants
a currency bump (one Dell multi-node XR8720t row is mildly relevant as a hyperscale-adjacent chassis; one re-reported existing row).
If refreshed, use the existing adapter with new pinned dirs and before/after metadata captures; do not overwrite
`data/raw/energystar-*`. Re-check at the next quarterly cadence.
