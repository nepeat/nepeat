# independent batch review — 2026-10-04

reviewer: `batch_review`, independent of both authors and engineering. scope completed here: inspur nic curation and plus-suffix matcher. no network, import, full-suite run, rebuild or shared-state mutation. only this report was written.

## acceptance findings

no substantive evidence or matcher finding in the nic/matcher scope. engineering logs were inspected directly (ordinary `rg --files` hides ignored `.log` artifacts): focused 23 tests in 1.540s, full python 411 tests in 18.350s, node 6 passed / 0 failed. all agree with handoff claims.

one source-scope finding in wiwynn curation was identified and corrected: the ST7000G2 HTML record's `storage` fact includes “page 1 specifies 12Gb SAS/6Gb SATA”. `data/raw/nics-wiwynn-hyve-2026-10-04/wiwynn-st7000g2.html` supports 72 3.5-inch bays and SAS12G expander, but lacks that 12Gb SAS/6Gb SATA prose. the wording exists on the ST PDF p1 and SV7000G2 HTML, not this HTML source. remove the speed clause from this HTML record; retain it in its existing PDF-backed records. otherwise record-scoped listings/specifications can credit the HTML capture with another source's assertion. author removed the speed clause; reviewer rechecked the exact final fact against the HTML capture and confirms this finding resolved.

## inspur nic evidence checked

read `data/curated-inspur-nics-2026-10-04.json` and `research/inspur-nics-2026-10-04.md`; independently inspected all three `data/exports/nics-wiwynn-hyve-visual-review/inspur-nics-{14,15,16}.png` source-page renders.

- pdf p14 / printed p7: storage rows X710_2Port_LC_PCIe and 82599_2Port_LC_PCIe; management/business rows Inspur_Vostok_I350_1G_RJ_PCIEx4-G3_2, I350_4Port_RJ45_PCIe and 82599_2Port_LC_PCIe. exact names, 10G versus 1G groups and printed remarks agree.
- pdf p15 / printed p8: X710_2Port_LC_PCIe continuation, Inspur_Andes-M6_X710_10G_LC_OCP3x8_2, NIC_10Gbps_2Port_LC_Inspur_X710_PCIe_MM, Inspur_Sbt_X722_10G_LC_PCIEx8-G3_2 and E810_2Port_LC_PCIe. wrapped `_2` / `_MM` endings, continuing inspur vendor and 10G cells, and distinct E810 25G group agree. the x710 remark is the two-line Intel / +HG+ARM statement. the last three remarks are visibly blank and remain blank, without borrowing their neighbor's value.
- pdf p16 / printed p9: no further inspur rows. third-party rows, including the separate intel E810 spelling, are excluded correctly.
- p14 warning excludes intel X722 10-Gigabit; p15 lists the specific sbt x722 row. json preserves both statements and unresolved scope, with no unqualified compatibility.
- all ten records have only `Inspur / compatibility_table_vendor`; identity namespace is explicitly qualified, and no manufacturer/operator/controller specification/alias is inferred.

independent local json/hash checks: 8 unique exact models, 10 records, 3 blank remarks. original pdf sha256 `9f340f567f5603150a9c82781e1fc34656c3f2177e68b3f259ba842a32d16150`; curated sha256 `8f3da5d987e699fbce8888c9015061431340fd6da7831f37b31c639be12d4e51`. both match the author note.

## matcher checked

inspected complete `devicehunt/listings.py`, relevant tests in `tests/test_listings.py`, and `docs/listing-matching.md`. exact boundary excludes attached word/dash/plus extensions. V and V+ exact matches stay distinct in both directions; unknown plus extensions remain leads. phrase leads require the whole phrase and vary only the final code, without introducing whitespace-free aliases. vendor filters and seller tiers remain matching-record scoped. specifications are drawn only from exact matching records; variant leads get empty lists. record part numbers remain explicitly identified record context.

flat `[-+][\\w+-]*` phrase extension avoids the previous nested suffix segmentation. meaningful regression covers a 4096-character plus run, mixed suffixes, phrase boundaries, exact seller alias/context and 4097 rejection. no runtime threshold or exhaustive catalog performance guarantee is inferred.

independent checks with bundled python: `python3 -m unittest discover -s tests -p test_listings.py` passed 16 tests in 0.565s; `python3 -m unittest discover -s tests -p test_listing_specifications.py` passed 7 tests in 0.235s. no full suite was run by this reviewer. system git cannot run because of the existing xcode license state; no system change was made.

## wiwynn/hyve evidence checked

the json and note appeared during review, so the finite evidence review was completed. read the whole curated json and `research/wiwynn-hyve-2026-10-04.md`; inspected all eight existing PDF renders and the visible text from all four original HTML captures using the standard-library HTML parser. pypdf separately confirmed the JBOF copyright/dated parent footer, which the existing image render crops. no source acquisition or render mutation was performed.

verified nine original file hashes independently against the nine curated source entries: all match. 11 identities / 15 records / 9 sources. no invented aliases/order codes, manufacturer or operator roles appear. HTML and PDF facts stay record-scoped apart from the single ST HTML speed-clause finding above.

- SV7100G2 PDF pp1–2 supports 4U16N, tray versus node scope, 48 total / 3 per node M.2, 80kg chassis, per-node memory and 12.5V bus bar. HTML contains the different 1OU/one-third-width row plus chassis row; scopes remain qualified.
- SV7220G3 PDF pp1–2: visually confirmed the plus in the middle column. V four bays/two x16 HHHL; V+ two bays/one x16 plus two x8 HHHL; S one 3.5-inch bay/two x16 FHHL. common cells, half-width versus three-node wording, physical/power and historical OS rows agree. HTML 100GbE/four-node OCP3.0, management changes and SV7200G3 download label remain conflicts/context without variant copying or alias invention.
- ST7000G2 PDF pp1–2: SV7000G2 and -L server columns remain separate from grey ST enclosure cells; server-only CPU/memory/M.2/IOM/mezzanine data does not reach enclosure identity. management/OS merged cells have appropriate server scope. 185x537 PDF versus 190x538 HTML and ST1004/SV940 handle depths remain separate. ST-to-SV conversion is a relationship, not alias.
- Hercules PDF p1: heading/table support the 5U 112-drive storage identity; erroneous Orion GPU prose stays a conflict. HS9124 is an explicitly named separate Hyve-designed board; chassis specs do not reach it. Hercules is not Hercules9120. memory capacity ambiguity, R45 typo, slash PSU options and May2026 parent role stay explicit.
- JBOF PDF p1: named 2U 24-drive enclosure; BlueField-3, PCIe expansion and backplane statements agree. literal `11-220VAC`, table optional TPM versus feature TPM claim, May2026 footer and revision are retained without electrical correction or manufacture/operator inference.

the evidence source note's paths and hashes agree with the reviewed captures. final acceptance: all requested evidence/code review checks pass; no unresolved findings. correction recheck confirmed ST HTML bays only, SV7000G2 HTML storage speed wording explicitly sourced to its HTML prose, SV7100G2 HTML network wording explicitly sourced to its own feature prose, and unchanged PDF storage speed facts. all nine original hashes still match; 259 facts, 15 records, 11 identities remain.

accepted final curated sha256: `378a37307403fe9046f9cb078ef319ba2cb77824bccf9087eb0a4bd5bbe2a2aa` for wiwynn/hyve, `8f3da5d987e699fbce8888c9015061431340fd6da7831f37b31c639be12d4e51` for inspur nics. combined-batch integration/rebuild remains the coordinator's responsibility.
