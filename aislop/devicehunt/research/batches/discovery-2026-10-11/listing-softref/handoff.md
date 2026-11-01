# listing_softref handoff

Changes
- `devicehunt/listings.py`: `SOFTWARE_REFERENCE_BRANDS` (frozenset, 10 brands) + `is_software_reference(brand, kind)`;
  kind rule reuses `model_collisions.EXCLUDED_KIND_WORDS` (imported, no duplication, model_collisions.py unedited).
  Matcher kwarg `include_software_references` (default False): software rows are not indexed at all by default.
  When included, every match on them (exact or variant) is `software_reference_mention`, ranked 3 (after variant_lead).
  Status `software_references_only` when only such rows match. New JSON keys appear only with the flag
  (`software_references_included`, count key), so default output shape is unchanged.
- `devicehunt/__main__.py`: `--include-software-references` flag only.
- `tests/test_listing_software_refs.py` (5 tests), `docs/listing-matching.md` section.

Pre-batch comparison (default flags, --limit -1, row sets of brand/model/best_match, before-db vs live db)
- Identical: 'Wedge 100BF-32QS', 'Celestica DX010 Seastone', 'Edgecore AS7726-32X 32x100G switch',
  'Sun Oracle X4-2 server', plus 'Fuji Xerox DocuCentre printer', 'Yosemite sample pack' (all no match, as pre-batch).
- Differs: 'Inspur NF5280M6 2U server': pre-batch 1 (Inspur NF5280M6); live 3 (adds hardware identities
  `IEIT SYSTEMS Co.,Ltd. NF5280M6` and `Inspur Corporation | Inspur NF5280M6`). That is buyer-audit F5 brand
  split, real new hardware catalog content, not software refs; out of scope here.
- Compared candidate rows only, not per-candidate source lists (those legitimately grew).

Notes
- Kind rule also catches `Oracle | virtual machine configuration` and one brand-less "networking software artifact
  reference" identity; both are software-ish. Kind words are broad ('config'): a future hardware kind containing
  "config"/"firmware" would be hidden by default (same trade-off model-collisions already makes).
- Tests: test_listing_software_refs + test_listings + test_listing_specifications = 28 OK.
- Kind words (`config`, `bmc`, etc.) match as substrings; future hardware kinds containing them would be hidden by default (also in docs).
