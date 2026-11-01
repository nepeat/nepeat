# listing_softref independent review

Verdict: ACCEPT with notes (no must-fix).

Reviewed: handoff, `is_software_reference` / `SOFTWARE_REFERENCE_BRANDS` / `include_software_references` path in
`devicehunt/listings.py`, flag in `__main__.py`, `tests/test_listing_software_refs.py`, docs section. Live catalog
`data/devices.sqlite` (5,107 identities, read-only). Probes and mutants were run on copies in the scratchpad.

## (1) Coverage of the live catalog

478 identities are hidden (155 in the pre-batch catalog). The distinct hidden (brand, kind) pairs are:

| brand | kind | n | judgement |
|---|---|---|---|
| DENT platform | network switch software platform definition | 10 | software, correct |
| Linux BMC device tree | BMC device tree source | 12 | software, correct |
| Meta OpenBMC platform | BMC firmware platform definition (software reference) | 19 | software, correct |
| ONIE machine | network switch ONIE machine definition (software reference) | 61 | software, correct |
| ONL platform | network switch software platform definition | 91 | software, correct |
| OpenBMC entity-manager config | software configuration (BMC hardware detection) | 183 | software, correct |
| OpenBMC machine | BMC firmware machine definition | 14 | software, correct |
| SONiC platform | network switch software platform definition | 63 | software, correct |
| Stratum platform | network switch software platform config | 15 | software, correct |
| coreboot mainboard | host firmware mainboard definition | 8 | software, correct |
| (empty brand) | networking software artifact reference (`AS7772-32X`, a DTB gist) | 1 | software, correct |
| Oracle | virtual machine configuration (`1-Chip VM with SPARC M7`, SPEC CPU result) | 1 | not sellable hardware, correct |

- No hardware identity is hidden. Every visible row from the `project`, `community`, `benchmark` and
  `primary_document_mirror` sources has a hardware kind (server, switch, network platform, FRU).
- The kind rule hides exactly 2 identities that the brand list misses. The brand list alone covers all the others, so
  the two rules overlap entirely on the other 476.
- No software-derived brand is missed. I checked for firmware, ONIE, linux-hwmon, u-boot and BIOS brands and found none.
- Residual (note): 33 visible `onie:<vendor>_<board>` models have brand Accton or Celestica
  and kind `network platform`. They come from the ONIE source but hold hardware-vendor identities. They are
  single tokens with a colon or underscore, so a real listing title cannot match them. They were also visible before the
  batch, so there is no behavior change. Only `lookup` and `search` can see them. Optional cleanup, not required here.
- Latent risk (as the handoff says): the substring `config` or `bmc` in a future hardware kind would hide it,
  and this is the same trade-off `model-collisions` makes. A kind such as "Configurable switch" would be hidden. Substring
  matching is crude, but nothing in the current catalog is affected.

## (2) Default output against the pre-batch catalog

I ran the default flags, `--limit -1`, with `--db data/devices-before-discovery-2026-10-11.sqlite` against the live db,
comparing the (brand, model, best_match) row sets, the status and the JSON key set.

| title | result |
|---|---|
| Edge-Core 7760-32X (owned) | identical, `variant_leads_only`, 1 row |
| Oracle X10-2C (owned) | identical, 1 `model_mention` |
| Celestica DS4000 (owned) | identical, 1 `model_mention` |
| Accton AS7712-32X 32x40G switch | identical, 10 rows |
| Facebook Minipack switch | identical, 2 rows |
| Quanta T7032-IX1 | identical, no match |
| Oracle Sun Server X4-2 Fuji Transformers Zaius Mori Yosemite Wedge Elbert | identical, the single Oracle hardware row; all 7 software rows are gone |
| Wedge 100BF-32QS | identical, no match |
| Celestica DX010 Seastone | identical, 2 rows |
| Inspur NF5280M6 2U server | 1 row became 3; the new rows are the hardware identities `IEIT SYSTEMS Co.,Ltd.` and `Inspur Corporation`. This is genuine new hardware and agrees with the worker's note. |

- The JSON key set is equal in every case. The default output has no `software_references_included` and no
  `software_reference_mention` count key.
- I also used 400 random identities from the pre-batch catalog (brand + model) as titles. The hardware candidates were
  byte-identical with the flag off and on, so the flag never changes hardware rows. The software rows that turned up
  with the flag on were the expected ONL and SONiC rows.
- I did not diff against the pre-batch listings.py code, because the package is untracked and there is no old copy. The
  evidence is that the hidden set in the pre-batch db is only ONL, SONiC and the gist, and those have long
  model strings that cannot match titles. The six-title and ten-title comparisons agree with that.

## (3) Flag on

- The F3 title with `--limit 3` gives `Oracle Sun Server X4-2` first, then the software rows. The counts are
  `model_mention` 1 and `software_reference_mention` 7, matching 8.
- `Zaius Power9 Barreleye server` returns `software_references_only` with 1 `software_reference_mention` row. Without the
  flag the status is `no_catalog_mention`.
- A software row is never labelled `model_mention`. The kind is overridden for every match type, including variant leads
  on software rows. Sort rank is 3, after `variant_lead`. The `software_references_included` key appears only with the flag.
- `--vendor` already drops software rows because they have no affiliations. This matches buyer-audit F24.

## (4) Tests and mutants

`python3 -m unittest tests.test_listing_software_refs tests.test_listings tests.test_listing_specifications` gave
28 tests, OK.

| mutant | result |
|---|---|
| include software rows by default | killed (3 failures) |
| software rank -1 (sorts first) | killed (1 failure) |
| drop the kind rule | killed (3 failures) |
| status `software_references_only` changed to `no_catalog_mention` | killed |
| count key always present | killed |
| remove the `coreboot mainboard` brand from the set | survived |

## Findings

Must-fix: none.

Should-fix:
- Brand-list coverage is untested. Only `OpenBMC machine` and `Meta OpenBMC platform` are exercised in the tests, and
  in the live catalog every brand-listed identity is also caught by its kind. The surviving mutant shows that removing a brand
  entry passes the tests, because every live brand is also caught by its kind. Add a parametrized test over
  `SOFTWARE_REFERENCE_BRANDS` with an empty kind, and assert `is_software_reference('Accton', 'switch')` is False.
  This is a one-minute fix.

Notes:
- The `onie:<vendor>_<board>` hardware-branded models above are an unhandled corner. They are harmless for
  `match-listing` and are only seen by `lookup` and `search`.
- The kind substrings `config` and `bmc` are broad (see the latent risk above).
- The `--include-software-references` flag help text and the docs section are accurate.
