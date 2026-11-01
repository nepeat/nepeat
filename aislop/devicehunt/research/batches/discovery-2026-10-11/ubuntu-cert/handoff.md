# ubuntu_certified handoff — 2026-10-11 (Claude Sonnet 5.5)

## Outcome
Canonical's public Ubuntu hardware certification listings (ubuntu.com/certified) captured by plain GET with `curl`; 192 requests of the 200 cap
(1 unsaved robots.txt probe + 191 in the manifest). **No documented public JSON API found**: the listing and detail pages are server-rendered HTML
(the page offers `?format=md` and `llms.txt` links, not used); used HTML parsing of `https://ubuntu.com/certified?q=<term>&limit=200` (all categories) then per-certificate pages.
`robots.txt` of ubuntu.com saved (200; `/certified` allowed, `/search*` and `/devices*` disallowed, Crawl-delay 1; none of those paths fetched).
`certification.canonical.com/robots.txt` returned 302 (recorded, not followed, host not used). ≥2 s between requests, one UA, no login/forms.

Searches (vendor names as `q`): Quanta 42 hits, Inspur 22, IEIT 22 (distinct sets; see below), Inventec 2, Wiwynn 2 — all category Server.
**Zero results** (page says "no certified hardware that matches"): Hyve, Celestica, Oracle, Sun Microsystems, Amazon, Amazon Web Services, AWS, Foxconn, Hon Hai, Hon Hai Precision, Ingrasys,
Accton, Edgecore, Edge-Core, Edge-Core Networks, SYNNEX, Synnex Corporation, Quanta Cloud, Wistron. `QCT` -> 6 Dell "Pro Tower QCT1250/1255" desktops (model-name coincidence, excluded). `EC2` -> 2 Dell desktops (excluded).
So no AWS instance types/Oracle/networking/IoT entries exist under these search terms (a search miss, not proof that no such make exists under another spelling).
Result: 90 Quanta/Inspur/IEIT/Inventec/Wiwynn certificate IDs listed (listing pages hold 98 ids incl. 8 excluded Dell desktops); 89 detail pages captured (HTTP 200) -> 89 sources / 89 devices (74 distinct brand+model devices); 55 "All hardware details" pages captured.

## Files and hashes
- `data/raw/discovery-2026-10-11-ubuntu-cert/` — `ubuntu.com/robots.txt`, `certification.canonical.com/robots.txt` (302, empty), `pages/all-q-*.html` (search listings), `detail/<cert>.html` (90 requested, 89 HTTP 200), `hwdetail/<cert>_<release>.html` (71 requested, 55 HTTP 200).
- `capture-manifest.json` sha256 `8629c9794b0b2b80c01cfb1d487ae951547880a1e7215c511f056de03ad6cde9` (url, HTTP status, UTC, sha256, bytes per file; all 191 verified, 0 mismatches).
- `ubuntu-cert-facts-2026-10-11.json` sha256 `054f90fc22488a966c618acf4e4ee6ad8b43d106d4543118d9a5fe1cd041d6a7` — curated candidate, snapshots relative to its own directory (importable in place).
- Scripts (scratchpad `ubuntu-cert/{fetch,parse,build,validate}.py`, run with `python3 -I`; not in data/raw).

## Identities and facts (revised after review; coordinator brand decision)
One device record per certificate page: source `ubuntu-cert-<cert id>`, key = cert id, kind `server`, identifier `ubuntu_certified_model` = model, model = listing "Configuration Name" exactly (whitespace collapsed as rendered), title = `<make> <model>` as printed.
**Brand/affiliation**: brand `Quanta` for make `QUANTA Computer Inc` and `Inspur` for `Inspur Electronic Information Industry Co., Ltd.` (both are reviewed exact aliases in `catalog.TARGETS`: `quanta computer inc`; `inspur electronic information industry co ltd`, see research/bytedance-inspur-2026-10-04.md "Exact target mapping");
`Wiwynn`, `Inventec` unchanged; `IEIT SYSTEMS Co.,Ltd.` kept as the printed brand (unassigned, null target; relationship to Inspur unresolved, no mapping). Every record has exactly one affiliation `[<make string as printed>, listing_holder]` (UL/ENERGY STAR/BSMI precedent); target resolved by `target_for`
(Quanta 41, Inspur 22, Wiwynn 2, Inventec 2 resolved; IEIT 22 null). Tier `certification_body`; coverage still says the listing is not a manufacturer/ODM/deployment/support claim. Existing `QCT` devices untouched.
Counts: 89 records. Fresh temp import into a copy of the current `data/devices.sqlite` (4650 devices): **+66 devices** (4650 -> 4716): Quanta 35, Inspur 13, IEIT 16, Wiwynn 1 (SV300G3), Inventec 1 (K905G6P). Records merge into existing identities by the exact brand+model rule:
Wiwynn `SV7220G3` and Inventec `K885G7`, plus Inspur `NF5180M5, NF5280M5, NF5468M5, NF8260M5, NF8480M5, NS5162M5` (kind `server` is dropped on merge; facts remain record-scoped). `SV7220G3` merges only with plain `SV7220G3`, not `SV7220G3-V+`.
Unmerged 89 records over 74 distinct brand+model in an empty catalog (standalone import); 66 new + 8 merged = 74.
vendor-inventory (`--limit 0`, read-only copy, before -> after): Quanta manufacturer 41 -> 41, all 73 -> 108; Inspur manufacturer 3 -> 3, all 141 -> 154; Wiwynn manufacturer 2 -> 2, all 23 -> 24; Inventec manufacturer 44 -> 44, all 82 -> 83. Manufacturer scope is unchanged; the new rows appear only in scope `all`.
Facts (`ubuntu_cert_*`, values as printed): `certificate_id`, `make_as_printed`, `model_as_printed`, `listing_category_as_printed`, `page_subtitle_as_printed`, `platform_page`, `releases_as_printed`, `kernel_by_release`, `bios_by_release`, `processor_by_release`, `network_by_release` (deduped, `network_entry_count_by_release` separate), `video_by_release`,
`platform_name_as_printed` (only where it differs from the config name). For the 55 with a hardware-details capture: `hardware_details_release`, `hardware_details_capture`, `hw_processor/network/video/bios/board/raid/bmc_network` and **`hw_testbench_disk/usb/mouse/keyboard/cardreader`** (renamed: peripherals and drives attached/detected during the certification run, not system specification; one release only). The source coverage says the same.
Provenance (S1): each source's coverage names the `pages/all-q-*.html` listing file(s) and sha256 that carry the Vendor / Configuration Name / Category columns (the detail page prints make+model only as the joined title). Coverage also states IEIT vs Inspur strings are separate printed strings with an unresolved relationship (12 model strings appear under both).
Examples: `201709-25792` QuantaGrid D52BV-2U: Ubuntu 18.04 LTS, kernel `4.15.0-34-generic`, `Intel Corp. Intel(R) Xeon(R) Gold 6134 CPU @ 3.20GHz`, Intel X722 10GBASE-T x2, ASPEED; hw details add `Broadcom / LSI MegaRAID SAS-3 3108 [Invader] (1000:005d)` and test drive `Union Memory (Shenzhen) ST8000NM0055-1RM112`.
`202601-38367` QuantaEdge EGX88D-1U: Ubuntu 24.04 LTS, kernel 6.8.0-90-generic, BIOS `American Megatrends International, LLC.: 3A02 (UEFI)`, `INTEL(R) XEON(R) 6776P-B`. Release coverage across certificates: 18.04 x26, 20.04 x30, 22.04 x32, 24.04 x12. "Form factor" is only the model-name suffix and page subtitle.

## Excluded claims, conflicts, gaps
- 504/500 responses were recorded once and never retried or rerouted: detail page `202103-28876` (QuantaGrid D53X-1U, 504; listing-only, **no source/device made**), hardware-details `201906-27124/18.04 LTS` (504), and 14 hardware-details pages with HTTP 500 (201905-27048, 201904-27036, 201904-26997, 201807-26320, 201805-26249, 202601-38367, 202508-37865, 202507-37020, 202506-36848, 202411-36041, 202410-35870, 202403-33893, 202403-33543, 202310-32201, 202310-32148).
  My batch loop did **not** stop at the first 5xx (AGENTS.md asks for stop/checkpoint): it continued through 15 consecutive 500s, after which 200s resumed. The review reads these as per-page server errors, not throttling; all recorded once, nothing re-requested. No 429 seen.
- Hardware-details pages were fetched for 71 certificates (newest first, non-Quanta first) and only for the first release tab; 34 certificates (including the 18 oldest Quanta certs and the 15+1 failures) have no `hw_*` facts: absent, not zero. `hw_*` rows are per one release, not all releases.
- Listing model strings differ in detail: `QuantaGird D43N-3U` (sic, the page's own spelling; double space in listing HTML collapsed), `D52T-1U` (no family prefix), `QuantaPlex T42SP` (no U suffix), `STRATOS S910-X31E` (config name) vs platform `S910-X31E`. IEIT models like `NF5180-M7-A0-R0-00` kept literal.
- Search `q` matches vendor and model text; Quanta search returned only `QUANTA Computer Inc`. Searches not repeated per category (/certified/iot, /socs); the all-category listing returned only category `Server`. No category filtering pages were fetched, so the absence of IoT/SoC entries for the zero-hit vendors is scoped to the `q` terms above.
- Cloud instance types (Amazon/AWS) were not listed. Wistron (a related ODM) was probed only as a zero-hit out-of-scope extra.
- Canonical's page text is listing data; processor/NIC strings are what the certification run reported (some PCI IDs unnamed, e.g. `Intel Corp. 8086:12d3`).
- **Owned-model check** (case-insensitive over every captured path and file content incl. the candidate; CS8200, CS8210, AZ4444, 7760/AS7760, DMC-12, AS7772, SV1120, SV1121, SV2121, SV2221, ByteDance, B4020, B4010): ZERO hits. Scope: these 193 files only; `validation/owned-hardware.json` not used as evidence.

## Cross-reference leads (read-only, `data/devices.sqlite` opened `mode=ro`; exact case-insensitive model equality on devices.model, then identifiers)
Existing devices with the same model string (QCT = lead only; the rest merge by exact brand+model under the revised brands): QuantaPlex T42S-2U, QuantaGrid D52BV-2U, QuantaGrid D52BQ-2U, QuantaGrid D51B-2U (existing brand `QCT`); SV7220G3 (`Wiwynn`);
NF5180M5, NF5280M5, NF8480M5, NS5162M5, NF8260M5, NF5468M5, NF5280M6 (`Inspur`); K885G7 (`Inventec`). Under the revised brands the exact-model matches against `Quanta`/`Inspur`/`Wiwynn`/`Inventec` merge (see Identities); `QCT` models stay separate devices (lead only). Loose lead: `QuantaGrid D53XQ-2U` ~ existing `Quanta D53XQ-2U` (Bahrain TRA, family prefix dropped); no merge. NS5162M5 / SV7220G3 relationships to the evidence policy's separation rules (`i24` vs NS5162M5; SV7220G3 vs G3-V+) are untouched.

## Checks run
- Manifest: 191 files, sha256/bytes re-verified, 0 mismatches; 193 files on disk (+ manifest + candidate as the only unmanifested files).
- Candidate: 89 sources = 89 devices, every snapshot exists, max fact 885 bytes.
- Temp import, fresh catalog: returned 89; sources 89, devices 74, records 89, identifiers 89, facts 1656, affiliations 89, integrity_check ok, foreign_key_check empty. Import into a scratchpad copy of `data/devices.sqlite`: sources 865, records 6014, facts 53506, affiliations 9379, devices +66, integrity ok, FK empty. Max fact 885 bytes. No new network requests in the revision; owned-model check re-run: 0 hits. `data/devices.sqlite` and the import manifest untouched.
- No test suite (no code changed).

## Decisions needed
1. (Resolved by coordinator) brand/affiliation convention above.
2. `Quanta` vs existing `QCT` devices remain separate (no merge).
3. Optionally fetch the remaining ~34 hardware-details pages and the 504 page later (budget used 192/200 here).
