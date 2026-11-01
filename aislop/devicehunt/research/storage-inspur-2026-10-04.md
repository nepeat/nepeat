# Inspur support discovery and Inventec storage — 2026-10-04

The new Inspur support feed is raw discovery data, separate from model assertions.
The batch also adds Inventec Metagross and specifications for the already-present
Inventec E850G4 and Inspur NF5280M5. All original captures and fetch outcomes are
under `data/raw/storage-inspur-2026-10-04/`.

## Public support feed

The original [Inspur download page](https://www.inspur.com/eportal/ui?pageId=2317460)
references a public `getAllProductName` JSON request in its ordinary page script.
One read of that exact endpoint returned **1,064 rows / 1,055 distinct printed
names**. The captured page and JSON are hash-linked in `inspur-support-bundle.json`.
No serial-number lookup, warranty/configuration query, login, activation workflow,
form submission or contact action was performed.

The response mixes hardware, software, groups and third-party parts. Examples:
`NF5280M5`, `AS13000-M1`, `i24&NS5162M5`, `GPU Tesla-T4`, `InCloud OS`, and menu
category names. Parent `115` has 148 rows; parent `211` has 91. Those numeric IDs
are preserved as source IDs, without assigning unverified category labels.
The `typeTreeLeaf` field is `1` even for some category-like entries; it is not a
reliable independent hardware classifier. Nine repeated names remain separate.

Schema 28 adds `inspur_support_entries`, with row number, source product/parent
IDs, exact name, nullable level, leaf flag, complete raw JSON and original URL.
`inspur-support` validates the two input hashes, page-to-endpoint relationship,
row structure/count and unique product IDs before replacing only its own rows.
It creates **zero device, brand, manufacturer or operator assertions**. `lookup`
searches the raw table with literal Unicode-insensitive matching. A feed result
does not establish current download availability or complete historical coverage.

```sh
python3 -m devicehunt lookup 'i24&NS5162M5' --format json
python3 -m devicehunt lookup AS13000-M1 --format json
python3 -m devicehunt sql "SELECT product_id,parent_id,name FROM inspur_support_entries WHERE parent_id='115' ORDER BY name"
```

The raw rows are available through SQL/lookup. The HTML model inventory continues
to show separately reviewed model assertions; it does not display these 1,064
mixed support entries as 1,064 verified Inspur devices.

## Selected platform evidence

[Inventec Metagross's original page](https://ebg.inventec.com/en/product/Storage/2U%2042%20Bay/Metagross)
names a 2U enclosure for 42 hot-swappable 3.5-inch SATA drives. Selected facts
preserve D 925 × W 447 × H 86.8 mm, two 800 W supplies with 1+1 redundancy,
and four dual-rotor fans with N+1 redundancy. Input voltage is not specified.
This is a brand/platform association, not an explicit manufacturing claim.

The [original E850G4 brief](https://ebg.inventec.com/en/tool-download/product_files/file/80?note=)
adds page-specific specs: 2U/450 mm depth, second-generation Xeon Scalable,
12 DIMMs and 800/1300 W CRPS options. The 2021 copyright and PDF metadata are
not selected as a product release date. Its platform assertions supplement the
separate 2020 manufacturer announcement; roles are not copied between sources.
Page 2 was visually reviewed. OTII design compatibility does not establish a
particular telecom or hyperscaler deployment.

The [original NF5280M5 white paper](https://en.inspur.com/eportal/fileDir/defaultCurSite/resource/cms/2020/04/2020040710453910867.pdf)
is revision 5.0, released 2019-12-22, with 47 PDF pages. Selected specifications
retain dimensional distinctions with/without handles, memory options and BMC.
Its platinum PSU section reduces the 1300/1600/2000 W options to 1000 W at rated
110 VAC. Some GPU configurations do not support 1+1 redundancy. These are
historical module/configuration conditions, not an installed unit's configuration.

Other power lines contain apparent source mistakes: repeated 230 VAC entries,
550 W DC values under higher titanium ratings, a `130W` row with 1300 W outputs,
and the heading `AC -48V`. The original text stays unchanged, with a structured
source-error note. No correction, interchangeability claim or unit wiring advice
is inferred. PDF pages 21, 22, 35 and 36 were visually checked.

Two originals add 49 pages. The full additive document collection is
`data/raw/reference-documents-with-storage-inspur-2026-10-04.json`; the reference
importer replaces its whole collection, so never import a partial two-PDF list.
Reviewed facts are in `data/storage-inspur-facts-2026-10-04.json`.

```sh
python3 -m devicehunt search Metagross --vendor Inventec
python3 -m devicehunt specs Metagross
python3 -m devicehunt specs E850G4
python3 -m devicehunt specs NF5280M5
```

## Remaining work

- Review the support tree's category meanings and exact server/storage identities
  against product/compliance documents before expanding the assertion inventory.
  Preserve compound `i24&NS5162M5` names and third-party component roles.
- The captured Inventec download-center page is only page 1 of 3; it is a future
  document-discovery source, with no inferred model coverage from unseen pages.
- ByteDance B4020/B4010 still lack qualifying primary association evidence. Search
  previews from trade-data providers remain unverified; the known Eximpedia 402
  was not retried. No models were imported just to satisfy validation.

See PROGRESS.md and the dated storage-inspur audits for the live/rebuild counts,
validation and export checks. No manufacturer or hyperscaler assertions are
added by this batch; complete historical coverage remains open.
