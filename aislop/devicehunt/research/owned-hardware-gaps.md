# Owned hardware: identity coverage and buying gaps

Reviewed 2026-10-04. The ten entries in
[the owned baseline](../validation/owned-hardware.json) supply validation queries,
not source evidence. All ten have identity support: **nine saved document
snapshots and one reviewed label photo**. Current verification belongs in
[PROGRESS](../PROGRESS.md); the earlier investigation is preserved in
[history](../docs/history/2026-10-04-before-owned-buying/owned-hardware-gaps.md).

## Exact identifiers and evidence

| Owner identifier | Supporting source and scope |
| --- | --- |
| CS8200-32X-UN-00 | UL E362364 model extract; TEC certificate 379401371 names Celestica Thailand Limited as declared OEM. |
| CS8210-32x-un-00 | UL E362364 includes the complete UN-00 model. The DC-11 variant remains separate. |
| DMC-12-48R-AS0 | NTRA page 189, row 11955 names EDGE-CORE. The separate `48V` register label does not specify this unit's input. |
| 7760-32X-A-AC-F | Reviewed user-supplied photo pairs Model No. with Supplier Sku AS7760-32X; photograph-only identity evidence, separate from seller text. |
| AZ4444-52X-DC-10 | Anatel certificate 225812303459 names the full model and EdgeCore manufacturer. Owner clarified this full suffix; the shorter earlier query is not a separate baseline. |
| AS7772-32X | NTRA page 189, row 11963 names EDGE-CORE. Other regulatory rows remain source-specific. |
| E4-2C | NIST CMVP 4506 names an Oracle server in a tested software-module configuration; Broadcom adds separate compatibility evidence. |
| E5-2C | Saved Broadcom search/detail record 57328 names Oracle Server E5-2c and Oracle partner. |
| X7-2C | Saved Oracle OSS index explicitly heads Oracle Server X7-2C and its source archives. |
| X10-2C | Saved Oracle licensing documentation explicitly names the hardware component; Broadcom adds separate compatibility evidence. |

Company roles stay on their source records. Identity support does not establish
AWS deployment, current support, electrical configuration, standalone operation
or the contents/condition of an auction unit. Preserve case as printed while using
case-insensitive exact queries. The corrected AZ4444 name and all suffixes remain
intact; no validation criterion was relaxed.

## Practical follow-up

The saved-evidence reviews are split into
[six networking devices](owned-networking-buying-2026-10-04.md) and
[four Oracle devices](owned-oracle-buying-2026-10-04.md). They distinguish already
imported facts, newly reviewed facts awaiting import/provenance, source conflicts
and missing buying information. No additional 7760 photograph is required for the
resolved identity mapping. See [the label review](edgecore-7760-label.md).

Known denied captures remain denied: direct UL pages and the AS7710/AS7760/AS7761
quick-start PDF returned 403. The Master MT-3593/2015 certificate original also
returned 403; see [that lead](as7760-certificate-lead.md). Read source-specific
fetch reports before acquisition; no alternate-route retry of the same denied
item. Registry expiry dates are historical assertions, not support deadlines.

The separate [ByteDance examples](../validation/vendor-examples.json) remain
an affiliation gap and are not owned-device expectations. A matching Accton model
does not establish ByteDance provenance. See [the source review](bytedance-inspur-2026-10-04.md).
