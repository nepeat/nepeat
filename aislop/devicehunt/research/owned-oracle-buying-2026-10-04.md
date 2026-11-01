# oracle owned-model buying evidence — 2026-10-04

Import follow-up: the saved Broadcom BIOS tables are now SQL-queryable, including
the three rows each for E4-2C, E5-2C and X10-2C. The X7 VPAT's official content
provenance is now recovered: a new Oracle capture is byte-identical to the old
file. Its 25 scoped facts are now imported through the
[primary-source batch](networking-primary-batch-2026-10-04.md). The
[server-evidence milestone](server-evidence-batch-2026-10-04.md) imports sixteen
saved NIST E4-2C facts as an additional extraction of the same capture, preserving
software-module test scope. The old VPAT capture's historical request URL/time
remain unknown. See the
[accepted import batch](owned-buying-import-batch-2026-10-04.md) and
[PROGRESS](../PROGRESS.md) for current verification. The review below preserves
the earlier saved-evidence stage and its original findings. Use the full
`buying-report 'Oracle Server X7-2C'` query to include the recovered VPAT;
short aliases remain record-local. Use `buying-report 'Oracle Server E4-2C'` for
the NIST detail; tested configurations establish neither current Windows support
nor the installed hardware or software entitlement of an auction unit.

## Earlier saved-evidence review

The original finite review of E4-2C, E5-2C, X7-2C and X10-2C made
no acquisition, import or database change. This is an evidence review, not proof
that a particular liquidated unit boots or carries software entitlement.

four [structured findings](batches/owned-buying-2026-10-04/oracle/) contain exact
catalog identities, source/record ids, capture hashes, URLs where recorded,
field references, imported facts, newly reviewed facts, exclusions and unknowns.
[verification](batches/owned-buying-2026-10-04/oracle/verification.json) confirms
registered capture hashes, Broadcom detail/raw-record equality and unchanged
database SHA-256. [review helper](batches/owned-buying-2026-10-04/oracle/review_saved.py)
uses read-only SQLite and writes only this worker's artifact directory.

| exact baseline | saved evidence and buying implication |
| --- | --- |
| E4-2C | imported Broadcom 53494 identifies a two-socket rackmount AMD EPYC 7003/7003X compatibility configuration. its CPU note directs SD/USB boot users to KB 85685; it does not state E5/X10's hard prohibition. newly reviewed BIOS rows name AMI 78014900 UEFI Mode. NIST 4506 explicitly lists Windows Server 2022 on E4-2C with AMD EPYC 7J13, with PAA and without PAA (single-user mode), as tested software-module configurations; these details are not structured imported facts. |
| E5-2C | imported Broadcom 57328 identifies a two-socket rackmount AMD EPYC 9004 configuration. its CPU note says boot from SD/USB flash media is not supported, within this compatibility evidence's scope. newly reviewed BIOS rows name AMI 83002900 UEFI Mode. the benchmark record has no registered saved capture path and is excluded from reviewed positive claims. |
| X7-2C | imported Oracle OSS heading lists four ILOM source-code archive links: 4.0.0.21, 4.0.0.30, 4.0.2.20.b and 4.0.2.29. these are source archives, not installable firmware. RRA pairs derived model Oracle Server X7-2c with main model X72c2U; the code's spelling alone is not a chassis-height specification. a saved but unimported Oracle accessibility report dated 23-Aug-2017 explicitly distinguishes SM 1U and SM 2U configurations. |
| X10-2C | imported Oracle licensing page says it may be included in Private Cloud Appliance. Broadcom 57094 describes two-socket rackmount Intel Xeon Platinum 8400 compatibility evidence and the same SD/USB prohibition as E5. newly reviewed BIOS rows name AMI 79006400 UEFI Mode. licensing notices identify edk2-stable202208/BSD-2-Clause-Patent and links for technologies that may be included, not secondhand entitlement. |

Broadcom's BIOS tables retain all three certification rows per model and their
certIds. unfiltered BIOS rows and release lists do not establish pairings.
CPU-series maximum cores/socket is not the installed CPU configuration. the
saved guide's ESXi 8.0 U3/9.0/9.1 lists are snapshot statements; neither current
compatibility nor access to any named BIOS is asserted.

X7's accessibility capture describes 24 DDR4 DIMM slots, Pilot4 service processor
and management ports in both configurations, with different front PCIe slot and
drive-bay arrangements. literal variant sections and the dependent-product list
are preserved in its findings. processor 150W / 205W / 165W statements are not
PSU ratings or measured draw. source wording about memory channels is preserved
without correction. At this earlier review, missing original-URL metadata kept
the facts out of import. The later byte-identical Oracle capture resolves content
provenance, as linked above; it does not reconstruct the old request's URL/time.
The dated dependent products are not a general OS compatibility or entitlement table.

no exact reviewed source resolves PSU models/input voltage/connectors, fan
requirements/airflow/noise, rails/depth, standalone operation, owned-unit
credentials/locks or current firmware download/resale entitlement. absence in
this saved corpus is a research gap, not proof that operation is impossible.
E4/E5 1U/2U/Enclave/Blockstore identities, X7-2, E5-2L and unmapped EcoDesign codes
remain separate; no specifications transfer to the four baseline identifiers.

finite next work: reuse the recovered X7 VPAT capture; seek at most
one exact-model service/installation manual or label-backed part-number mapping
per model, one BIOS/ILOM release/readme plus public download-access evidence per
model, and one official applicable secondhand licensing/support-transfer policy.
check source-specific failure reports before any future acquisition; use no gated
serial/warranty requests and do not retry denied captures.
