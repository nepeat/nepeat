# lookup match context — handoff

## change
- `devicehunt/lookup.py`: each catalog row gets `matched_in` (string) and `matched_in_detail` (dict, json only), via new `match_context()`.
  Uses the lookup's literal casefold substring test (`contains`) per source for that device: identity (title/brand/model/revision/kind/identifier values),
  fact NAMES whose values contain the query (sorted, first 5 + `(+N more)`, total in `facts[N]`), and counts for affiliations, hyperscaler evidence and
  exhibit titles; `indexed text only` when the FTS hit spans fields. Values are never shown. Catalog dataset scope note extended: software-reference identities
  (kind contains 'software') list strings their files contain; a PSU/FRU/part-string match is not fitment evidence (static dataset note, no brand list).
- `devicehunt/__main__.py` (outside the stated path, minimal): table output strips `matched_in_detail` and appends `matched_in` as the LAST column (after `identity_text_match`).
  JSON: `matched_in`, `matched_in_detail` are new keys after `identity_text_matches`; existing keys unchanged.
- `tests/test_lookup_match_context.py` (new, 5 tests). No existing tests edited; none pinned catalog row columns.

## example
`lookup PFE1100-12-054NA`: Meta OpenBMC platform minipack -> `matched_in = facts[1]: fbobmc_psu_part_string_as_printed`, `identity_text_match False`.

## verification
`unittest tests.test_lookup_match_context tests.test_lookup tests.test_lookup_batch`: 19 tests OK (pre-existing unclosed-db ResourceWarnings only).

## timing (CLI, json, data/devices.sqlite read-only, wall time, one run each)
| query | before | after |
| --- | --- | --- |
| NF5280M7 | 1.44s | 1.46s |
| Minipack | 1.44s | 1.46s |
| PFE1100-12-054NA | 1.45s | 1.44s |
Per-row cost is a few indexed lookups per displayed catalog row (default limit 20; `--limit -1` scales with matches).

## coordinator notes
- `scripts/refresh_exports.py` regenerates lookup-*.json via `run_many` -> `_run`, so refreshed reports gain `matched_in`/`matched_in_detail` and the longer catalog scope text; I did not run it. No export generator/test pins the column set.
- Rule is kind-agnostic in code; the software caveat is a static scope sentence.

## coordinator follow-up (after review ACCEPT with notes)

- review should-fix applied by `/root-claude`: the catalog scope sentence no longer claims software-reference
  kinds contain "software" (OpenBMC machine / Linux BMC device-tree kinds don't); it now names the source
  classes (firmware, BMC, device-tree, ONIE, SONiC/ONL, config definitions). not independently re-reviewed;
  `tests.test_lookup_match_context tests.test_lookup tests.test_lookup_batch` 19 OK.
