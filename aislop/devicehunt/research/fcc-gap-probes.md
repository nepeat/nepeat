# FCC IDs missing from the mirror

On 2026-09-27, three IDs from the local `fcc-coverage` missing-index report were
checked at the mirror's normal public detail URLs:

- [HEDML2560](https://fccid.co/fcc/HEDML2560)
- [HEDML60LW](https://fccid.co/fcc/HEDML60LW)
- [HEDMR3202A](https://fccid.co/fcc/HEDMR3202A)

Each returned original HTML with title `Not Found | FCC ID Database`, no device
heading, no application-details section and no exhibit links. The captures are
preserved under `data/raw/fcc-gap-probes/`, with URLs and SHA-256 hashes in
`review.json`. These are missing entries in this particular mirror, not proof
that the FCC IDs or devices do not exist. Existing catalog evidence for all
three identifiers remains intact.

The new `fcc-gap-probes` importer verifies every original page and hash before
replacing its own source rows. A wrong title, contradictory device content,
duplicate ID, unexpected URL, path escape or corrupt checksum prevents import.
It creates only `unavailable` rows in `fcc_detail_pages`, with source ID
`fcc-gap-probes`. Grantee/company fields remain blank because the response
doesn't identify a grantee. No device, model, manufacturer, index, application,
exhibit link or text-excerpt record is inferred from the failure.

```sh
python3 -m devicehunt import fcc-gap-probes data/raw/fcc-gap-probes/review.json
python3 -m devicehunt lookup HEDML2560
python3 -m devicehunt fcc-coverage --vendor Accton --missing detail --format json
python3 -m devicehunt sql "SELECT fcc_id,status,issue,fields_json FROM fcc_detail_pages WHERE source_id='fcc-gap-probes' ORDER BY fcc_id"
```

The raw detail table now has 174 rows: 171 captured pages plus these three
unavailable probes. The coverage report shows one failed attempt for each
tested ID while correctly leaving its detail stage `not_collected`. The known
identifier total and all collected-stage totals are unchanged. A later separate
successful capture can coexist with this dated negative result; a negative
probe never overwrites another source's success.

The supplemental importer is separate from the existing company-index detail
collector, which still validates against its own complete cached index scope.
An offline rebuild reproduces the probes from the original HTML without making
network requests. The same-source refresh drops withdrawn probes, while other
sources and device assertions remain untouched.

The associated Accton China legal-name investigation remains unresolved. An
original 2018 consolidated-report download at
`https://www.accton.com/wp-content/uploads/2019/05/2018-Consolidated-Report.pdf`
returned HTTP 403 and was stopped without retry or an alternate download. It
was not archived or used to add a company mapping. The Anatel OS-LS-6248 record
still retains its original unmapped manufacturer name in the raw register.
