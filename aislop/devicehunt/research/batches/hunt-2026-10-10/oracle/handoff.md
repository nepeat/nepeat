# oracle_practical handoff — 2026-10-10

## Outcome
Partial. 13 web queries, 7 captures (all HTTP 200, no denials, no retries, no login). Exact public Oracle service/installation documents were found for NONE of E4-2C, E5-2C, X7-2C or plain X10-2C. The only exact-name service chapter found documents a different identity, `X10-2c GPU L40S Compute Server` (3U), in the Private Cloud Appliance (PCA) 3.0 Hardware Maintenance Guide. Facts are proposed for that identity only; whether it equals the owned `X10-2C` is undecided. The shared policy and firmware-access items were found (they name no target model).

## Files + hashes (data/raw/hunt-2026-10-10-oracle/, ledger: capture-ledger.json)
- `6be23f49ad98046737fce0fcec1a22b3ac6a57f3f70361975d6af0d0b7c4ba71` capture-ledger.json
- `dcf0e34512998674358e5233fd735d9fa1769dd85ebfb4f8e0eb05ad8db1e542` oracle-hardware-systems-support-policies-2026-08-07.pdf
- `06b99208256c5618d01a47b19808c5160288eee6db722ea251d62e76da37f051` oracle-practical-facts-2026-10-10.json
- `c6260aadc98f578f1146da53b9b353150ac664a54ecd0cd3f30fdc659012f416` oracle-system-firmware-faq.html
- `4b4e7ce54d5c4abf925ff582ea16eafc3c084539f8763e78948c552f0f20b582` pca-3.0-concept-servers.html
- `7da847f05b5a0ceec9215581a300c23f26249167ad6d9986320517522dee8b75` pca-3.0-hardware-maintenance-F87242-05.pdf
- `39d4ca52219cac5d48a994ee17f3431afbaa78ed0a1f7a3b11597e7a76116d88` pca-3.0-maintenance-index.html
- `a698f4c679e516b0e28726beeff9b166a2da8e95de2664fa6462a67ee505457e` pca-3.0-release-notes-F74805-44.pdf
- `d548e2183304882a538c21367e2a453bb023fd4244ab3e9eac1a317a7f7f6ebd` x10-2c-pca-replacing-power-supplies.html

Proposed curated input: `data/raw/hunt-2026-10-10-oracle/oracle-practical-facts-2026-10-10.json` (5 sources, 3 device rows for one identity, 17 fact rows, per-fact literal quote + capture sha256 + PDF page in `evidence`). Snapshots are bare capture filenames relative to the JSON's own directory; keep the JSON in this raw dir.

## Supported exact facts (identity `X10-2c GPU L40S Compute Server`, brand Oracle)
Source: `pca-3.0-hardware-maintenance-F87242-05.pdf` (F87242-05, Aug 2025), chapter 6, PDF pages as given (chapter footer "Page N of 12" = PDF page - 68):
- p.4 contents title "Replacing X10-2c GPU L40S Compute Server Components"; p.69 "rear of the 3U server chassis".
- p.69-70 back-panel callouts: four power supplies PS0-PS3 each with an "AC power input connector" (connector type not stated); three high speed fan modules FM0-FM2.
- p.76 PSU: "A271/A271A 1400W power supply", "accepting ranges from 200-240 volts AC (VAC)", "fully redundant N+N" ; p.78 A271 and A271A interchangeable.
- p.77 leave a failed PSU installed "to ensure proper airflow"; p.74 fan swap must finish within 25 seconds; p.74 data cables are at the front.
- HTML `x10-2c-pca-replacing-power-supplies.html`: same PSU text (duplicate, not corroboration; visible text never names the model, URL slug only).
- `pca-3.0-concept-servers.html`: "3 RU server with Intel Xeon Platinum 8480+ ... four NVIDIA L40S GPUs"; it is a GPU expansion-rack node.
Shared (no device rows): Oracle Hardware and Systems Support Policies (07-Aug-2026) PDF p.7 "Unsupported Hardware Systems": no entitlement to download/receive maintenance releases, patches or support for unsupported systems; parts of a covered system not transferable to an unsupported one. Oracle System Firmware FAQ: firmware via My Oracle Support requires "a valid Oracle Support contract or Hardware Warranty" (public statement of the login/contract requirement; no download was attempted).

## Excluded claims, failures, gaps
- No exact Oracle service/installation manual found for E4-2C, E5-2C (searches incl. exact-name and PCA/Cloud@Customer returned nothing; these names appear only in the saved Broadcom/NIST evidence). X7-2C: only X7-2 / X7-2L manuals exist publicly (X7-2 PSU A266 1200 W was seen in search snippets only, never captured; separate identity, not transferred).
- Plain X10-2C: Oracle's licensing page (already imported) calls it "Oracle Server X10-2c"; the maintenance guide's "x10-2c" slug chapter is the GPU L40S server. Not assumed equal; Broadcom 57094 describes Intel Platinum 8400 two-socket, which does not by itself settle it. PSU/voltage facts must not be applied to owned X10-2C until the coordinator decides.
- No rack/rail/depth, connector type, airflow direction, noise, or standalone-boot facts in any capture. PCA servers are rack-integrated; PCA admin credentials/locks are a separate gap.
- No BIOS/ILOM release note for any of the four: the PCA 3.0 release notes (captured, F74805-44) name no per-server firmware versions (bugs/updates are in MOS docs 2906831.1/3013714.1, login-gated, not requested). Maintenance index HTML has no TOC text (JS-rendered).
- The support-policy PDF has no secondhand/resale/transfer-of-ownership clause in the extracted text; that is absence, not permission or prohibition. The component table in the concept page was not used (column association unrecoverable).

## Checks
All curated quotes asserted against extracted text (pypdfium2 from the nix store via PYTHONPATH, `python3 -I`; HTML text whitespace-insensitive). Temp import via `importers.curated` into `scratchpad/oracle/temp.sqlite`: 5 sources, 1 device, 17 fact rows, source sha256 equal ledger sha256. `import_source` needs a registered source key, so it was not used. data/devices.sqlite hash unchanged and no manifest touches. Policy pages' device-less sources carry only coverage text (facts not queryable); decide if a shared-policy fact row is wanted.

## Decisions needed
1. Is Oracle "X10-2c" (PCA) the same product as the owned X10-2C, or is the GPU server distinct? 2. Accept device-less policy sources, or attach download_access_statement to a generic row? 3. Whether to pursue label/part-number mapping for E4/E5 (needs owner photos, not web).

## Next bounded step
Owner labels/photos of E4-2C and E5-2C PSU and chassis (part numbers), then look up those PSU numbers in public Oracle docs; or integrator imports this input after the identity decision.

## Fixes after review
1. Snapshots are now bare filenames beside the JSON (kept in the raw dir); re-imported via `importers.curated` into a temp catalog: 5 sources, 1 device, 17 fact rows, source hashes equal the ledger.
2. `psu_redundancy_as_printed` now quotes "in most cases, provide fully redundant "N+N" power".
3. Policy coverage text no longer claims a "used" search; it says only that no resale, secondhand or ownership-transfer clause was found on the captured pages.
4. Renamed to `failed_psu_slot_airflow_note_as_printed`; the concept-page quote is split into `chassis_height_as_printed` (3 RU), `processor_as_printed` and `gpu_as_printed`.
New curated sha256: 06b99208256c5618d01a47b19808c5160288eee6db722ea251d62e76da37f051. Captures and ledger unchanged.
