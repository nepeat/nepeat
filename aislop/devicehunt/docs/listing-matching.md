# Match an auction title locally

`match-listing` compares supplied titles with the identities and reviewed
identifiers already in your SQLite catalog. It reads the database without
modifying it, contacting a marketplace, or importing seller text as evidence.

```sh
python3 -m devicehunt match-listing --title 'Oracle E5-2C server for parts'
python3 -m devicehunt match-listing --title 'Edge-Core AZ4444-52X switch' --format json
python3 -m devicehunt match-listing --csv my-listings.csv --format json > matches.json
python3 -m devicehunt match-listing --csv my-listings.csv --vendor Celestica --format csv > matches.csv
```

Input CSV needs a `title` column. Use `--title-column NAME` for another header.
JSON retains all original columns, such as your item URL and seller notes,
alongside each result. Those extra columns are context only: only the selected
title column is matched. UTF-8 with or without a BOM is accepted. Limits are
5,000 input records, a 20 MB file, and 4,096 characters per title. Duplicate or
missing headers, malformed row widths, and empty titles produce an error.

## Reading results

- `model_mention`: a complete stored model occurs in the title.
- `identifier_mention`: a complete explicit alias, model-search identifier,
  manufacturer-model field, or part number occurs in the title.
- `variant_lead`: a hyphen- or plus-delimited prefix relationship, such as bare
  `AZ4444-52X` versus stored `AZ4444-52X-DC-10`. This does not establish equivalence.

For `Unattributed accessory` identities whose kind contains the word `FRU` or
`FRUs`, model codes with a separate short revision also produce variant leads:
`YM-2651Y` finds the stored `YM-2651Y B01R` and `YM-2651Y, B01R` records, and
`YM-2651Y-B01R` finds those same revision spellings. The rule requires a
hyphenated alphanumeric base code and a 1–8 character alphanumeric revision.
Bare codes can lead to several revisions; a hyphenated revision must match the
printed revision. Exact stored spellings still produce complete mentions.
These leads retain separate identities and source records, carry no specifications,
and establish neither revision equivalence nor fitment. Aliases and other kinds
do not receive this extra matching rule.

Suffix leads also work for spaced names: `Wedge 100-32X-UNKNOWN` can point to
`Wedge 100-32X`, while `Wedge 100` can point to the suffixed commercial model.
The full phrase must match and only dash or plus suffixes of the final code are compared.
`SV7220G3-V` and `SV7220G3-V+` remain distinct; an unknown extension such as
`SV7220G3-V+EXTRA` produces variant leads rather than borrowing either model's facts.
Spaces are preserved: `Wedge 100-32X` is not made an alias of `WEDGE100-32X`.
`related_title_tokens` contains the complete normalized phrase for these leads.
With `--with-specs`, variant leads always have an empty specifications list.

Exact model mentions sort first, followed by other identifier mentions, then
variant leads. These labels describe text matching, not confidence that the
item for sale is that hardware. No candidate is automatically selected as the
correct identity. Different brands, revisions and suffixes remain separate.
If one alias belongs to several identities, all remain candidates.

JSON shows every matching identifier and its own source, source snapshot hash
and retrieval timestamp when available, evidence tier, record ID, source URL,
and company relationships. The `--vendor` filter requires that company's
affiliation on the **matching record**; another record cannot lend its vendor
or stronger evidence tier to a seller-only alias. Compact table/CSV output lists
sources for each candidate's best match type; JSON has the full breakdown.

The command reports per-title candidate totals, counts by best match type, and
truncation. `--limit 20` is the default per-title limit; `--limit -1` shows all
candidates and `--limit 0` gives counts only. Unmatched CSV rows remain in the
output. `variant_leads_only` distinguishes a family lead from a complete mention.

Words such as `replacement`, `PSU`, `for parts` and `untested` are exposed as
`context_terms`, not used as an automatic condition or accessory classifier.
For example, a replacement PSU listing can mention a supported switch without
selling the switch itself. The matcher does not verify photos, completeness,
authenticity, live availability, firmware access, licensing or past ownership.

Matching ignores case, collapses repeated whitespace, and normalizes Unicode
dash typography. It retains ordinary hyphens and underscores, enforces model
boundaries, and does not use edit-distance guesses or remove suffixes. Terms
normally have at least four characters and a letter; all-numeric and shorter
names are omitted to reduce incidental matches. Codenames such as Minipack can match.
`--include-short-models` additionally permits three-character alphanumeric names
containing both a letter and a digit, such as `AF1`. This option does not enable
pure words, pure numbers or one- or two-character names. JSON reports whether this option
was enabled. These short mentions can be ambiguous; vendor filters still apply
to the matching evidence record, without establishing what an auction is selling.

```sh
python3 -m devicehunt match-listing --title 'AWS Snowcone AF1' --vendor 'Amazon Data Services' --include-short-models
```

`--include-numeric-identifiers` separately enables catalog models and the same
eligible identity fields containing **6–20 ASCII digits**, optionally separated
by hyphens. This covers Oracle marketing parts such as `7103791` and numeric
ordering codes while keeping small numbers such as `2026`, `48` and `100`
excluded. It preserves leading zeros and all hyphens: `07103791` is not
`7103791`, and `371432502` is not `371-4325-02`. Added suffixes remain variant
leads. Vendor filtering still requires the matching record's own affiliation.
Known numeric strings can occur incidentally in titles, so matches remain text
mentions rather than identification of the item being sold.

```sh
python3 -m devicehunt match-listing --title 'Oracle 7103791 for parts' --vendor Oracle --include-numeric-identifiers
python3 -m devicehunt match-listing --csv my-listings.csv --include-numeric-identifiers --format json
```

Numeric-only matching is off by default and independent of the short-model
option. JSON, table and CSV results report both options, including unmatched
rows. Numbers below six digits or above twenty digits remain excluded by this
option; use literal `lookup` or SQL for them. Source-scoped
`oracle_mfg_part_number` mappings are still discovery fields, not eligible
identity aliases; this option does not promote them into listing matches.

Facts, compatible-product mentions, firmware build targets and certificate
numbers are not automatically treated as product identities. The command does
not search raw registry rows; use `lookup` for those.

## Software-reference identities

Identities that describe software files rather than sellable hardware (brands
`Meta OpenBMC platform`, `OpenBMC machine`, `OpenBMC entity-manager config`,
`Linux BMC device tree`, `ONIE machine`, `SONiC platform`, `ONL platform`,
`Stratum platform`, `DENT platform`, `coreboot mainboard`, or any kind containing
software/firmware/BMC/device tree/config; see `SOFTWARE_REFERENCE_BRANDS` in
`devicehunt/listings.py`) are excluded by default, so words such as `wedge`,
`fuji` or `yosemite` do not match them. `--include-software-references`
re-includes them with match type `software_reference_mention`, which sorts after
`variant_lead` and so never displaces hardware under `--limit`. The flag adds
`software_references_included` and a count key to JSON; a title matching only
such rows has status `software_references_only`. The kind words (notably `config`, `bmc`) match as
substrings, so a future hardware kind containing them would be hidden by default. `lookup` and `search` are unchanged.

## Specifications with listing matches

Use `--with-specs` to include available power, cooling, physical and software-
installation statements alongside each exact model or identifier mention:

```sh
python3 -m devicehunt match-listing --title 'Celestica R4023-F9102-A0 empty chassis' --with-specs --format json
python3 -m devicehunt match-listing --csv my-listings.csv --vendor Celestica --with-specs --format csv > matches-with-specs.csv
```

The specification must belong to the same source record as the matching model,
alias or part number. For example, the reviewed `R4023-F9102-A0` ordering-code
record contributes its F2B configuration without borrowing the B2F statement
from `R4023-F9003-A0`. Searching just `DS4000` can show multiple configurations;
each keeps its own record ID and `record_part_numbers`, so those alternatives
do not become a description of a single auction unit.

JSON adds a `specifications` list and `record_part_numbers` to each evidence
match. Statements keep their category and original wording, including qualifiers.
The enclosing match retains source URL, tier, company roles and snapshot metadata.
Conflicting sources and revisions stay separate; a seller's statement retains
its marketplace tier. `variant_lead` matches always have an empty specifications
list, even when their candidate model has known facts. Exact matches with no
categorized facts also stay empty instead of borrowing another record's facts.
An empty list means unknown in that record, not a compatibility determination.

Table/CSV output adds the candidate revision and a `specifications_json` column.
It contains record-scoped statements with their source metadata, affiliations,
part-number scope and matching identifiers. Multiple identifiers from the same
record are grouped without duplicating its statements. An unmatched title or
counts-only result keeps the same output columns with an empty JSON list.
Existing output remains unchanged when this option is omitted.

The command remains offline and read-only. Specification text does not prove
what the seller is supplying, installed components, working condition, or
firmware entitlement. `empty chassis`, `replacement`, `untested` and similar
title words remain visible context rather than automatic product classifications.

`data/exports/listing-specifications-examples.json` and `.csv` demonstrate eight
reviewed Celestica chassis ordering codes and three storage models using
illustrative titles. These are not live listings. The corresponding audit
checks all eight full-code records, unknown suffixes, six distinct DS4000
configurations, and unchanged database bytes.

## Included examples

`data/examples/listing-titles.csv` contains one previously cached seller heading
and five explicitly illustrative titles. It is **not a live offer list** and is
not an evidence source or a rebuild input. Its output is saved at
`data/exports/listing-matches-example.json` and `.csv`.

The 7760 example retains seller-only evidence. Bare AZ4444 produces variant
leads. The Celestica full ordering code and Oracle E5-2C produce complete text
matches. An accessory example retains its context and an unknown-title example
remains unmatched. These examples do not change inventory validation.

`data/examples/numeric-listing-titles.csv` adds three synthetic Oracle titles:
one complete numeric identifier, one suffix variant, and one leading-zero
near-match. Its JSON and CSV outputs are saved under
`data/exports/numeric-listing-matches-example.*`. They are examples, not offers.
