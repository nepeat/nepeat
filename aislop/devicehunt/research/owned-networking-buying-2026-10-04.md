# six owned networking identifiers — saved buying evidence

Import follow-up: the photographed input marking and its location reference are
now in the catalog. The three seller statements remain unimported. See the
[accepted import batch](owned-buying-import-batch-2026-10-04.md) and
[PROGRESS](../PROGRESS.md) for current verification. The review below records
the earlier saved-evidence stage and its original findings.

A [later primary-source pass](networking-primary-batch-2026-10-04.md) found no
qualifying exact-model installation/power manual in its finite search. It did
capture a public AS7772-32X device-tree gist: reviewed community declarations,
not physical configuration, supported installation instructions or owned-unit proof.
Those declarations are now imported as a separate unbranded software-reference
identity; the practical-document gaps below remain. Use an unfiltered
`buying-report 'AS7772-32X'` to include that community source.

reviewed 2026-10-04; complete for this finite saved-source review. no network,
imports or catalog changes. [machine-readable findings](batches/owned-buying-2026-10-04/networking/findings.json)
retain 22 exact identity records, per-record roles/aliases, source URLs, capture
hashes, field references, reviewed material, imported facts and evidence gaps.
[review script](batches/owned-buying-2026-10-04/networking/review_saved.py) uses
read-only SQLite and the standard library; image review is a separate visual step.

| owned identifier | useful exact saved evidence | practical limits |
| --- | --- | --- |
| CS8200-32X-UN-00 | official RRA detail-032's 기타 cell says `Switching Power Supply - FSJ021 / Acbel Polytech Inc.(R-R-API-FSJ021)`; already imported as `other_raw` | an approval-record association, not a required/installed PSU inventory; no input, wattage, connector, airflow or boot information |
| CS8210-32x-un-00 | UL structured extract lists printed `CS8210-32X-UN-00`; matching is case-insensitive only | capture supplies identity/listing holder, not power/cooling/software; `CS8210-32X-DC-11` stays separate |
| DMC-12-48R-AS0 | NTRA row 11955 and RRA detail-024 name exact model; RRA calls it a console terminal server | no voltage or chassis requirements in these exact records; `DMC-12-48R-AS0 48V` is separately printed and excluded |
| 7760-32X-A-AC-F | same photographed label pairs full Model No. with Supplier Sku `AS7760-32X`; F/W `V2015.02.00.30`, H/W `R011`, Rev. `R20` already imported | pictured-unit observations only; no boot/recovery instructions, image access or transferable license |
| AZ4444-52X-DC-10 | TEC/UL/RRA Accton relationships and Anatel EdgeCore manufacturer statement retain separate records | no input/connector/cooling/software requirement established; do not decode `DC-10` or lend other AZ4444 variants' specs |
| AS7772-32X | exact NTRA/TEC/UL/Anatel and RRA derived-model entries | RRA co-listing with AS7762-32X or AS7712-32X is not a specifications/firmware alias; `AS7772-32X-12V` stays separate |

## four fields identified during the saved-evidence review

1. the existing [7760 photo](../data/raw/edgecore-7760-label.png), directly inspected,
   has an adjacent compliance label whose Model is `AS7760-32X` and whose
   `Input Voltage Range` reads `100-240~, 50 - 60Hz, 6A Max`. keep this on the
   physical-label record as `label_observed_input_voltage_range`. it describes
   the photographed marking; it is not measured consumption or every unit's PSU.
2. saved [ecrunch seller HTML](../data/raw/ecrunch-7760.html), product Details:
   `I powered this on as shown in the pictures.` proposed `seller_power_on_claim`.
3. same Details: `Red handles on power supplies and fans.` proposed
   `seller_component_handle_claim`. color does not establish airflow direction.
4. same Details: `Switch hardware is guaranteed; however, as this switch is
   re-programmable, software is not guaranteed.` proposed
   `seller_software_guarantee_statement`.

seller claims belong only to historical source `ecrunch-I610661`, record
`b6dab80755ae5603580a01a1`, tier `marketplace`. they do not establish current
availability, independent functional verification or the state of either the
owner's unit or the different user-attributed eBay photograph. serial/MAC were
not transcribed. no seller return/warranty policy is promoted into a universal
hardware or software claim.

## method, gaps and next bounded work

checked the original saved RRA HTML labelled cells, TEC HTML table rows, Anatel
ZIP/CSV rows, Bahrain API row, immutable UL structured web extracts, saved NTRA
embedded-text page 189, seller Details paragraph, and original label pixels.
UL is an extract, not original HTML; its original download's recorded 403 stays
unavailable. exact source/capture provenance and review method are in the JSON.

also inspected existing UL Celestica platform-section text, ICASA pages 153/224,
and historical NTRA pages 391/460. these add identity context only, no positive
practical requirement for these six. separately printed 48V/-12V variants stay
excluded, even when a substring matches. PICOS's saved table does not name any
of these exact six identifiers; no compatible NOS is inferred from nearby models.

all six still lack source-backed chassis/mounting requirements, operational
cooling requirements, boot/recovery procedures, exact NOS installation guidance,
image-download access and licensing/transfer terms in this reviewed finite set.
absence is a collected-evidence gap, not proof of unsupported hardware,
inability to operate, or a safety conclusion. regulatory validity dates are not
software support or license dates.

next finite leads are one exact full-code hardware/installation or support
document per unresolved model, as listed in the findings JSON; do not initiate a
broad crawl. three seller fields remain candidates after the label follow-up.
this original review made no manifest, bundle, database or export changes;
the subsequent label import is linked above.
