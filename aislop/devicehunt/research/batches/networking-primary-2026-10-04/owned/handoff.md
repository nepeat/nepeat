# owned networking source pass — 2026-10-04

outcome: bounded pass complete; no qualifying vendor installation/service/power
document found. twelve distinct queries exhausted, one new original-HTML fetch.
no denied item retried, no code executed from sources, no shared catalog changed.

all six exact identifiers retain their existing practical gaps: CS8200-32X-UN-00,
CS8210-32x-un-00, DMC-12-48R-AS0, 7760-32X-A-AC-F,
AZ4444-52X-DC-10 and AS7772-32X. this finite search does not establish absence
of documentation or lack of hardware/software support.

new useful artifact: a public nepeat gist explicitly titled `AS7772-32X DTB`,
https://gist.github.com/nepeat/4d80007ef64987497f64c365522531f7 . its saved
HTML contains 2,060 source lines. title gives the exact model link. lines 7–8
print model and compatible `accton,as7772_32x`; line 75 aliases serial0 to
`/soc@ffe000000/serial@11c500`. partition labels at lines 118,123,128,133,
138,143,148,154 are `RCW`, `hw-info`, `onie`, `diag`, `open`, `Fman-FW`,
`uboot-env`, `uboot`. these are declarations in published source text, not
verified installed partitions, a boot procedure, a support promise, safe flashing
guidance, physical configuration or evidence tying the artifact to any owned unit.
no stdout/bootargs/console property was found by literal search of extracted text.
community-tier artifact candidate is supplied separately; coordinator acceptance
and independent review are required before integration.

capture: `data/raw/networking-primary-owned-2026-10-04/as7772-owner-dtb-gist.html`,
HTTP 200, 658,413 bytes, SHA-256
`d7f4d9523f3e253638445f6476ddd0eced3ba83b1db1095d90d45e3a3e49cb7b`.
fetch timestamp/outcome are in the sibling fetch-ledger.json. extraction is
`as7772-dtb-extracted.txt`; it decodes HTML cells without executing code.
search ledger records every query and results, with preserved responses for
search batches 2–4. batch 1 has a contemporaneous summary, not full saved response.

excluded: old/new regulator register repeats, seller listings, paid trade-data
snippets, adjacent variants and unrelated bare-code collisions. a new third-party
CS8200 article and a Chinese owner video are leads only, not vendor manuals.
the known-denied AS7710/AS7760/AS7761 quick-start surfaced again and was not
opened, mirrored or downloaded. independent vendor-authored mirrors were eligible
in principle; none newly qualified in this finite pass.

next bounded step: review artifact-only candidate against saved HTML; if pursuing
practical gaps, scope separate exact-code public vendor requests or software-project
sources. don't restart these twelve queries or reinterpret -DC-11, 48V, -12V,
AS7712/AS7762 or the pictured AS7760 supplier code as specifications aliases.
