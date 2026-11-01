# SUN-6000 scoped company evidence (proposal A, Kladno-only) — phase 1 design (rev 2)

worker `sun6000_a` (Claude Sonnet 5.5), 2026-10-11. Phase 1 only: nothing under `devicehunt/`, `tests/`, `data/`
was written. Everything below is read-only analysis plus a scratchpad prototype whose outputs are saved here.
User decision 2026-10-11: "A: scoped company evidence".

**Rev 2 (2026-10-11, after the independent design review, ACCEPT with notes):** per the coordinator, the design is now
**Kladno-only**: `Celestica Kladno s.r.o.` is the one mapped part (`registry_manufacturer` -> Celestica);
`Celestica de Monterrey S.A. de C.V.` is unmapped literal context (`role: null`, no target, no affiliation), exactly like
`manufacturer_list_context` for SUN-X6200-Series / SUN-T6300. This narrows what the user approved; the user is to be told
and may override (an override is the one-line change in the wrapper: Monterrey part -> mapped, plus the loader's
`name_evidence: certificate_scoped_only` allowance, which rev 2 removes). Review items M1, S1-S5 are folded in below; the
drafts, prototype outputs and numbers are regenerated (nothing is copied from rev 1).

Artifacts in this directory (proof paths):

| file | what |
| --- | --- |
| `rra-composite-manufacturer-cells-draft.json` | the evidence entry (wrapper), sha256 `6daf8c28…54b7`; draft, not under `data/` |
| `bundle-composite-cells-draft.json` | the Celestica bundle with only its `company_evidence` block repointed (one 2-line diff vs `bundle.json`) |
| `prototype.diff` | the proposed code change to `devicehunt/rra.py`, `rra_company_names.py`, `campaign_sources.py` (regenerated for rev 2) |
| `prototype-projection-diff.txt` | `review_queue` candidate-vs-live projection diff of the Celestica unit (records, facts, coverage) |
| `prototype-catalog-effect.txt` | before/after counts on a scratch copy of the live catalog (copy deleted) |
| `prototype-guard-checks.txt` | guard, Oracle-unchanged, old-bundle-unchanged and negative checks |
| `prototype-*.py` | the scripts that produced them (they import a patched copy of the package in the session scratchpad) |

## 0. Findings that change or sharpen the brief (read first)

1. **Kladno IS in a saved reviewed extract; Monterrey is in none.** The brief says both factory names are absent from the
   saved Exhibit 21.1 extract. That holds for the 2025 extract (`celestica-subsidiaries-2025.json`, five names) and for
   Monterrey in every extract. But `celestica-archived-21.1-reviewed.json` (period "not established from reviewed
   excerpt") lists `Celestica Kladno, s.r.o.`, and the Celestica unit already maps `Celestica Kladno s.r.o.` to Celestica
   through it (`rra_celestica_historical.EXHIBITS['archived-21.1']`). The entry therefore labels the two parts
   differently (`name_evidence`: `base` for Kladno, `unmapped_context` for Monterrey), and the loader checks the
   label against the base mapping in both directions (a context part must NOT already be mappable). The evidence-basis statement is written to say exactly this.
2. **Existing precedent maps only Kladno for the same two-name list.** `research/rra-manufacturer-lists.md` says
   "a Celestica Kladno claim does not also assert an unreviewed mapping for the adjacent Celestica de Monterrey name";
   for `SUN-X6200-Series` and `SUN-T6300` (same list, no parenthetical) the accepted `rra-reviewed-manufacturer-lists`
   source records Kladno only. Decision A as originally stated gave SUN-6000 both parts; the review recommended, and the coordinator chose, Kladno-only so
   the three byte-identical two-name lists are treated alike (also: the same capture leaves `Celestica Czech Republic, s.r.o.`
   (SUN-445) and other Celestica-named cells unmapped, so name+query alone has never been an accepted basis).
3. **`target_for('(Oracle America, Inc.)')` is `Oracle`.** The global alias rule would turn the parenthetical into an
   Oracle affiliation if it ever reached `Catalog.add` as a company. The design keeps it out of affiliations entirely and
   records it only as a literal fact (`manufacturer_cell_unlabelled_text`).
4. **The saved cell is whitespace-collapsed by the parser.** In `detail-090.html` the cell is
   `\r\n\t…Celestica de Monterrey S.A. de C.V.,Celestica Kladno s.r.o.\r\n\t…(Oracle America, Inc.)\r\n\t…`; the importer holds
   `Celestica de Monterrey S.A. de C.V.,Celestica Kladno s.r.o. (Oracle America, Inc.)` (single space). The guard keys on
   what the importer actually holds (text sha256 `4b68ec1b…23d5e`) and pins the raw HTML cell bytes (offset 3897, length
   110, sha256 `4d03ef3a…083c`), which `rra.composite_claims` now re-verifies against the pinned detail page at load time (review S1). The detail-page sha (`3c01977f…870b`) binds those raw bytes.
   The brief's "raw cell sha" is therefore carried as the raw-slice hash plus the page hash, with the parser-text hash as
   the runtime key. This is a stated substitution, not a silent one.

## 1. Units and files

**Units.** Verified in the live catalog and saved bundles: SUN-6000 is `rra_equipment` row 90 of
`rra-official-query-41e4ca912b33` (Celestica query, `data/raw/rra-celestica-official/detail-090.html`) and row 215 of
`rra-official-query-73009f906e4e` (Oracle query, `data/raw/rra-oracle-official/detail-215.html`). The two HTML files are
byte-identical (4,483 bytes, `3c01977f…`), and the manufacturer cells are identical. The Oracle query returned it
because its substring match hits the parenthetical `(Oracle America, Inc.)` (and the applicant); the Celestica query
returned it because the cell names Celestica. Two other certificates have the same two-name list without the
parenthetical (`SUN-X6200-Series`, `SUN-T6300`) and `EXR-A350-2` lists Kladno alone; none of them change.

- **Changed unit: Celestica `rra-official-query-41e4ca912b33`** (projection gains 5 records).
- **Oracle `rra-official-query-73009f906e4e`: no claims, projection unchanged** (see section 2.3).
- The other 10 units: projection unchanged, but their jobs are re-created (the adapter/runner code hash is in every recipe).

**Files that change (recommended layout: sibling bundle, old `bundle.json` untouched).**

| file | change | owner/phase |
| --- | --- | --- |
| `devicehunt/rra_company_names.py` | `kind: rra-composite-manufacturer-cells` wrapper: `load` delegates to its base, new `load_composites`, `composite_coverage` | engineering, phase 2 |
| `devicehunt/rra.py` | `composite_claims`, `claim_target`; `validate` runs the guard; `import_bundle` uses claims | engineering, phase 2 |
| `devicehunt/campaign_sources.py` | `RraSavedAdapter.records` uses `rra.claim_target` for `eligible_manufacturer` / `projected_source_keys` | engineering, phase 2 |
| `devicehunt/campaign.py` | `DEFAULT_DEFINITION` / `DEFAULT_INVENTORY` repointed to the new re-pin dir, at promotion only (section 3) | integrator |
| `tests/test_rra_composite_cells.py` (new), `tests/test_campaign_rra_live.py` (one new test) | section 4 | engineering |
| `data/raw/rra-celestica-official/rra-composite-manufacturer-cells-2026-10-11.json` (new) | the evidence entry | integrator |
| `data/raw/rra-celestica-official/bundle-composite-cells.json` (new) | byte copy of `bundle.json` except `company_evidence` -> wrapper | integrator |
| `data/import-manifest.json` | Celestica `rra` job path -> `bundle-composite-cells.json`; `files` + new bundle, wrapper, release, coverage manifest; `campaign_gate`; `expected.devices` 5107->5112, `expected.records` 7225->7230 (`sources[...41e4ca912b33]` stays 151) | integrator |
| `data/devices.sqlite` | +5 devices / records, section 4 | integrator |
| new re-pin dir `research/batches/campaign-c3c-2026-10-11/inventory/` (name for the coordinator to confirm) incl. a batch-local `pin_inventory.py` | new definition + inventory | integrator |
| `data/coverage/rra-ised-saved-2026-10-04/coverage-<new>.json`, `data/campaign-reviews/<new>/`, `data/campaign-releases/campaign-release-<new>.json` | new c3b artifacts | integrator / reviewer |
| docs: `research/rra-celestica-company-names.md` (or a new note), `research/korean-rra-targets.md`, `docs/vendor-inventory.md`, PROGRESS, README c3b paragraph | wording + totals | editor |

**Why a sibling bundle instead of editing `bundle.json` in place (the `bundle-before-historical-companies.json` precedent):**
an in-place edit changes the sha of `data/raw/rra-celestica-official/bundle.json` (`6aacf7a8…`), which is
(a) pinned by `data/raw/rra-manufacturer-lists.json` `captures[0]` (so `rra_manufacturer_lists.capture_path` would refuse it
until re-hashed, the manifest hash of that file would change, and the 37 Celestica rows in the un-gated
`rra-reviewed-manufacturer-lists` would show `records_changed` because their `manufacturer_list_observation` facts embed
`capture_sha256`), (b) the file `tests/test_campaign_rra_live.py::test_celestica_carried_evidence_restores_the_pinned_eligible_rows`
reads against the c0 pins (105 / 149 assertions) and an evidence-file list, so it would break, and (c) read by
`live_rra.py rebundle` as `carry_from`. The sibling keeps (a)-(c) untouched: prototype-verified that the old `bundle.json`
still equals its pin (`matches_pinned`) and `rra_manufacturer_lists.prepare` still returns 22 pairs / 45 assertions.
Cost of the sibling: the unit manifest path moves, so the c3b queue uses its moved-bundle path (baseline job = old
`bundle.json` from the accepted release, `candidate_job` = the sibling; this is the `MovedBundleBaselineTests` behaviour).
If the coordinator prefers the in-place precedent, add: re-hash `rra-manufacturer-lists.json` captures + manifest entry,
expect the 37 `records_changed` in that source, update the carry test to the new pins, and keep a byte copy
`bundle-before-composite-cells-2026-10-11.json`.

## 2. The evidence entry

### 2.1 Shape (`rra-composite-manufacturer-cells-draft.json`)

A wrapper file declared as the bundle's `company_evidence` (so the existing declaration, hash check and `carry_from`
machinery apply with no new field). It has `kind: rra-composite-manufacturer-cells`, `base` = the existing
`celestica-historical-companies.json` (path + sha256 `ab28b04a…5c5d`), `reviewed_on`, `decision`, `scope`, and `entries[]`.
`campaign_rra_live._load_carried_evidence` already follows `base`, so the wrapper, the historical bundle and the three
exhibit extracts are carried byte-identically with no change to `campaign_rra_live.py`.

Constraint found in the pinner: `pin_inventory.declared_entries` treats any JSON object with a `path` key as a
dependency (sibling, hashed). The wrapper therefore uses `path` only for `base`; the detail page is named by
`detail_file`/`detail_sha256`, never `path`.

Entry for `SUN-6000`:

| field | value |
| --- | --- |
| `certificate_number` | `SUN-6000` |
| `query_maker` | `Celestica` (guard: refused under any other query) |
| `detail_url` / `detail_sha256` / `detail_file` | `…A_b_popup_keyno.do?key_no=SUN-6000` / `3c01977f51bc25c4865c1c7e59c4fe7024d6e7cb4c8a46f20ceee5aeedfa870b` / `detail-090.html` |
| `cell.text` | `Celestica de Monterrey S.A. de C.V.,Celestica Kladno s.r.o. (Oracle America, Inc.)` |
| `cell.text_sha256` (runtime key with the certificate) | `4b68ec1b447e01f9b15bf3de88a0ea2d724a1693b0272097559c7541964e23d5` |
| `cell.raw_html_cell_bytes` (context, checkable against the saved page) | offset 3897, length 110, sha256 `4d03ef3a337c713042920acc67d223e21dd3a55ff298a644de20b0c317ed083c` |
| `separator` / `reconstruction` | `,`; parts joined by `,` + one space + parenthetical must equal `cell.text` exactly |
| `parts[0]` (cell order) | `Celestica de Monterrey S.A. de C.V.` -> `role: null`, `target: null`, `name_evidence: unmapped_context`: literal context in a fact only, no affiliation |
| `parts[1]` | `Celestica Kladno s.r.o.` -> role `registry_manufacturer`, target `Celestica`, `name_evidence: base` (archived Exhibit 21.1 extract, `Celestica Kladno, s.r.o.`, period not established) |
| `unlabelled_parenthetical` | `(Oracle America, Inc.)`, `role: null`: not a role, not an affiliation, not brand, not a third manufacturer |
| `evidence_basis` | the certificate was returned by the regulator's own Celestica manufacturer query; the cell names two companies; only Kladno has a saved reviewed extract (archived 21.1, period unestablished) and is mapped; Monterrey is in none and stays unmapped context; the mapped role is `registry_manufacturer` |
| `limits` | no role/affiliation for the Monterrey name; Celestica-brand `Sun Blade 6000 Chassis` is distinct from the Oracle-brand identity of that name; no factory / per-factory country / sole-manufacturer / unit-provenance claim (one country field, 멕시코, is not split); the Oracle-query occurrence gets no claim; derived model names stay RRA-asserted for this certificate only; not merged with the Oracle-documented "Sun Blade 6000 Modular System" identity |
| `live_reobservation` (informational) | cycle `2026-10-10d` re-fetched the same bytes: Celestica-query receipt `71049355-03cf-48e9-b529-75402eb5bfd2`, Oracle-query receipt `e11177f0-15b3-4615-98cd-ec61bdd51507` (context only, not corroboration) |

### 2.2 Guard (exact match, no pattern splitting)

An entry applies only when all hold: certificate equal; `sha256(parsed manufacturer text)` equals `cell.text_sha256` and the text
equals `cell.text`; `detail_sha256` and `detail_url` equal the capture entry; `query_maker` equals the bundle's query
maker; the whole cell does not already resolve through `company_target` (no double meaning); the raw HTML cell slice (offset/length/sha256) re-hashes from the pinned detail page. Load-time: parts reproduce
the cell exactly, mapped parts: role `registry_manufacturer`, target `Celestica`, `name_evidence: base` and the base must map the name; context parts: `role`/`target` null, `name_evidence: unmapped_context`, name not mappable by the base; at least one mapped part; no duplicate entry; `base` must be the historical Celestica bundle (no wrapper
nesting). **An entry that matches no row of the capture it is declared with is refused** (`rra.validate`), so a changed
cell or removed certificate fails loudly (a live bundle then ends as `failed` with the candidate retained, and a HTTP-free
rebundle with updated evidence is possible) instead of silently losing or keeping stale evidence. Reviewer may prefer a
softer "report only" mode; recommended strict. Prototype negative checks: wrapper under the Oracle query, an edited
part text, and an entry keyed to a certificate whose cell differs are all refused.

### 2.3 The Oracle-query occurrence

Treated as **raw-only, no claims**. Mechanism: the Oracle bundle declares no `company_evidence`, and the entry is
`query_maker`-scoped, so the Oracle capture's SUN-6000 row 215 keeps `eligible_manufacturer: false` and no records.
Reasons: the evidence basis is "returned by the Celestica manufacturer query"; the Oracle query matched on the
parenthetical; a second set of five identical records under `rra-official-query-73009f906e4e` would double
`role_assertions` and read as corroboration of one regulator page (the queue already flags the pair with
`cross_query_certificate_overlap`, `independent_corroboration: false`). Prototype: the saved Oracle bundle is
`matches_pinned` against the c3b pins with the patched code. The overlap conflict stays on both queue items (the
Celestica item now through its records as well as its raw row).

## 3. Code change (smallest) and its effect

`prototype.diff` (against the live files as of this session; the three files were unchanged since 2026-09-27 / 2026-10-05):

1. `rra_company_names.load`: if `kind == rra-composite-manufacturer-cells`, return `load(root, base)` after checking the base is the
   historical Celestica bundle. All existing callers (`rra.validate`, `import_bundle`, `campaign_sources`,
   `rra_manufacturer_lists`) therefore keep getting the same whole-company mapping dict, so nothing else in the loading
   path changes. New `load_composites(root, entry) -> {(certificate, text_sha256): entry}` (returns `{}` for any other
   evidence kind) and `composite_coverage`.
2. `rra.composite_claims(root, data, ready)` (guard above, called from `validate`) and
   `rra.claim_target(row, reviewed, claims)` (`company_target(cell)` else the target of the **first part that has a target**; review M1 — with Monterrey first, `parts[0]` has none).
3. `rra.import_bundle`: for a claimed row the affiliations are the mapped part(s) (Kladno, `registry_manufacturer`) plus the applicant; the context part is a fact only;
   facts: `rra_manufacturer_as_printed`, `manufacturer_cell_basis`, `manufacturer_cell_unlabelled_text`,
   `manufacturer_cell_review` (cell hash, detail hash, evidence-file hash, `reviewed_on`, limits), one
   `manufacturer_cell_part` per part, mapped or context (role, target, `name_evidence`, `name_basis`, reused company evidence when present);
   plus the existing facts. Targets for the mapped parts are set by an exact `source_key` `UPDATE` placed **after** the existing
   whole-cell `UPDATE` loop (that loop would otherwise reset a part that equals another row's cell). Source coverage text
   gains one explicit exception sentence; the base sentence "Composite company cells stay unsplit" is made conditional
   (`… except reviewed certificate entries` only when claims exist) so the other 11 units' `sources.coverage` text, and therefore
   their projection hashes, are byte-identical (prototype: Oracle `matches_pinned`).
4. `campaign_sources.RraSavedAdapter.records`: `eligible_manufacturer` / `projected_source_keys` via `rra.claim_target`.
   The occurrence `logical_sha256` (original row bytes) is unchanged; only `stats`, `projected_source_keys` and
   `eligible_manufacturer` change, and those live in `pinned_values` (not in the candidate-bundle JSON).
5. `research/batches/campaign-pilot-2026-10-04/inventory/pin_inventory.py` duplicates the eligibility logic in `replay()`;
   the batch-local copy must call `rra.claim_target` / `rra.composite_claims` rather than re-implement it.

Prototype results for the Celestica unit (candidate vs live projection, `review_queue.diff`): `records_added: 5`,
`records_unchanged: 149`, removed/changed/moved 0, raw rows identical, `source_fields_changed: coverage, sha256` only.
Adapter stats: `eligible_manufacturer_rows` 105 -> 106, `model_assertions` 149 -> 154, `validated_original_records_sha256`
unchanged (`140dcdd9…`), SUN-6000 `projected_source_keys` = `SUN-6000:6000`, `:Netra 6000 AC`, `:Netra 6000 Chassis`,
`:Oracle Sun Blade 6000 Chassis M2`, `:Sun Blade 6000 Chassis`. Effect on bundles: the candidate-bundle JSON hashes only
outcome, scope, occurrence ids, `occurrences_sha256`, `record_sets`, coverage, and the unit revision. For Celestica the
**unit revision changes** (pinned files change), so `candidate_sha256` and all `occ-…` ids change; for the other 11 units the unit revision and
candidate JSON are unchanged, only receipt/recipe ids change (code hash in every recipe).
`check_candidate_bundle` re-derives with the same code, so it passes only if the code is final before `campaign run`.

## 4. Predicted catalog effect and tests

Measured on a scratch copy of the live catalog after importing the sibling bundle with the prototype code
(`prototype-catalog-effect.txt`):

| metric | before | after |
| --- | --- | --- |
| devices / records / identifiers | 5,107 / 7,225 / 6,833 | 5,112 / 7,230 / 6,838 (+5 each) |
| affiliations / facts | 10,557 / 74,074 | 10,567 / 74,129 (+10 = Kladno + applicant per record; +55 = 11 per record) |
| `rra_equipment` rows, `records_seen` of the Celestica source | 1,081, 151 | unchanged |
| **Celestica manufacturer scope** | 193 identities, 239 source records, 241 role assertions | **198 (+5), 244 (+5), 246 (+5)** |
| manufacturer scope, all targets | 2,142 pairs / 2,141 identities | 2,147 / 2,146 |
| scope `all`, all targets | 4,581 pairs / 4,443 identities | 4,591 / 4,448 |
| `all`: Celestica identities / role assertions | 374 / 592 | 379 / 597 |
| `all`: Oracle identities (applicant role only) | 1,281 | 1,286 |
| Oracle manufacturer scope | 859 | 859 (unchanged) |

N = **5** Celestica manufacturer identities: brand `Celestica` models `6000`, `Netra 6000 AC`, `Netra 6000 Chassis`,
`Oracle Sun Blade 6000 Chassis M2`, `Sun Blade 6000 Chassis`; none of the five device ids exists today. The count is insensitive to the Monterrey decision; role assertions are +5 (Kladno only). Needs updating: `docs/vendor-inventory.md`
totals/table text, PROGRESS table, exports/lookups/collection reports. Docs must also say plainly (review S5/N2) that Celestica-brand `Sun Blade 6000 Chassis` (RRA-asserted derived name) and Oracle-brand `Sun Blade 6000 Chassis` (retired hardware product) are distinct, unmerged identities, that the bare model `6000` is the RRA's literal string and collision-prone in lookups, and refresh the `research/rra-manufacturer-lists.md` note (its Kladno-only rule stays true).

**Tests to add** (`tests/test_rra_composite_cells.py`, new): loader positive; each load-time refusal (parts do not reproduce
cell, hash mismatch, role not allowed, target not Celestica, mislabelled `name_evidence` both ways, duplicate entry, nested wrapper, wrong base kind,
non-object); guard refusals (wrong `query_maker`, detail sha mismatch, certificate present but cell changed, entry matching no row,
cell that already maps by `company_target`, raw HTML slice offset/length/sha differing from the saved page (review S1), context part that the base can already map, entry mapping no part, `claim_target` with the unmapped part first); import: exactly five keys for SUN-6000, affiliations are exactly Kladno (registry_manufacturer) + applicant, **no** Monterrey affiliation and **no** `(Oracle America, Inc.)` company row (guards finding 3), the Monterrey text present as a context fact, targets set per part including after the whole-cell loop,
facts present, source coverage names the exception, other certificates and the same code without a wrapper unchanged, re-import replaces cleanly;
adapter: 106 / 154 and `projected_source_keys`; the saved Oracle bundle and old Celestica `bundle.json` still equal their pins;
`rra_manufacturer_lists.prepare` still 22 / 45. In `tests/test_campaign_rra_live.py` add one carry test: `carry_from` the sibling bundle copies
wrapper + base + exhibits byte-identically and `compare_live` against the new pins is `matches_pinned`. Existing
`test_celestica_carried_evidence_restores_the_pinned_eligible_rows` stays valid with the sibling layout (it reads the c0 pins and the untouched
`bundle.json`; it would break under an in-place edit). `test_projection_canary_is_frozen` is unaffected: `review_queue.projection()` is untouched.
`AcceptedManifestGateTests` is addressed in section 5.

## 5. The c3b path, command by command

Preconditions: coordinator message "cycle 2 done"; no other writer on `devicehunt/campaign*.py`, `capture.py`, `artifacts.py`, the RRA importer.
Reviewer separate from the author (procedural rule). All steps offline; zero HTTP.

**Ordering constraints I verified in code**
- `live_rra.py rebundle` requires `acquisition.unit_revision == unit.revision` of the *current default definition* and uses `ROOT/unit.manifest`
  as `carry_from`. Any cycle-2 Celestica/Oracle bundle is stamped with the **old** revision, so rebundle it **before** re-pinning (with the old
  definition) or it is refused afterwards. Branch 1 (expected): cycle 2 byte-identical (`compare_live` -> `matches_pinned`): no rebundle, nothing to adopt; SUN-6000 work is independent.
  Branch 2: cycle 2 changed the Celestica or Oracle unit: that change needs its own c3b round (new path, `candidate_job`); finish it first, or fold the composite wrapper in by
  rebundling with the sibling bundle as `carry_from` (needs a `--carry-from` option in the research driver, which is not under `devicehunt/`).
- `pin_inventory.py build` reads `data/import-manifest.json` by constant and `closure()` raises `accepted manifest disagrees` unless the manifest already hashes
  the new bundle and wrapper; the live manifest still carries the old `campaign_gate` (which `gate_inputs` would refuse for a swapped job). Do not write an ungated
  manifest to the live path. **Procedural gap:** use a batch-local copy of the script with `MANIFEST` and `RELATIVE_OUTPUT` pointing at a gate-less candidate manifest
  (`data/import-manifest-pinbase-sun6000-2026-10-11.json`: live manifest minus `campaign_gate`, job swapped, new files hashed). It is for `pin_inventory.py` only: it must never be passed as `--accepted-manifest` or to `rebuild`, must be named in the batch handoff, and is deleted or clearly marked superseded after the pin (review S2). Its `REFERENCES` self-pin path moves too.
- `AcceptedManifestGateTests` asserts `release.definition_sha256 == load_definition(DEFAULT_*)`. If `DEFAULT_*` is repointed before the release is promoted it fails.
  The `campaign`, `coverage` and `review_queue queue` CLIs all accept `--definition`/`--inventory`, so repoint `DEFAULT_*` **at promotion, together with the manifest**.
  This removes c3b's inverted-order caveat: the guarded suite can be green **before** promotion and the gate class re-run after.

**Commands**

0. **Dry run (review S3):** before opening the batch, with the new code and the old (c3b) `DEFAULT_*` still in place, run the targeted list below once and report it green (recipe code hashes change while the default inventory is still c3b; `test_campaign`, `test_coverage`, `test_review_queue` read the default inventory).
1. Code and tests (phase 2): apply `prototype.diff`; add the tests; wording edits. Targeted:
   `python3 -m unittest tests.test_rra_composite_cells tests.test_rra tests.test_rra_company_names tests.test_rra_celestica_historical tests.test_rra_manufacturer_lists tests.test_rra_sun tests.test_campaign tests.test_campaign_rra_live tests.test_review_queue`.
2. Data: create the wrapper from the draft (byte copy) and the bundle sibling (`bundle.json` with only the `company_evidence` block replaced; expected diff is exactly the 2 lines in `bundle-composite-cells-draft.json`). `python3 -c` validation: `rra.validate(bundle-composite-cells.json)`.
3. Pin base manifest, then re-pin:
   `python3 research/batches/campaign-c3c-2026-10-11/inventory/pin_inventory.py build --root . --output research/batches/campaign-c3c-2026-10-11/inventory`
   then `… verify`. Expect 12 units; the only unit change is Celestica (dependency_paths 160 -> 161: `-bundle.json +bundle-composite-cells.json +wrapper`; unique source paths 1,121 -> 1,122, so `max_referenced_source_paths` 1,122);
   Celestica expectations 106 / 154 with the five keys on ordinal 90; all other units' expectations byte-equal to c3b; `definition_version` stays 1 (so unchanged units keep their unit revisions); the definition hash changes (code-reference hashes).
4. Plan, run, report (explicit flags, defaults untouched):
   `python3 -m devicehunt.campaign --definition D --inventory I plan` (36 jobs unplanned: recipes changed for all 12 units);
   `python3 -m devicehunt.campaign --definition D --inventory I run --offline`; `… report`. Expect 12/12 validated, 2 explicit zero, all `matches_pinned` (Celestica against the new pins), small archive growth (c3b was 1.39 MB; cap 64 MiB).
5. Freeze: `python3 -m devicehunt.coverage --definition D --inventory I freeze RUN_ID --output data/coverage/rra-ised-saved-2026-10-04` then `… validate PATH`.
6. Queue (with baseline jobs, built against the gated live manifest):
   `python3 -m devicehunt.review_queue queue COVERAGE.json --output data/campaign-reviews/<coverage id> --definition D --inventory I --accepted-manifest data/import-manifest.json`.
   Expect 12 items: 11 `no_changes` (Oracle included, with its existing overlap conflict); Celestica: `records_added: 5`, source `coverage` + `sha256` changed, baseline job = `raw/rra-celestica-official/bundle.json` with the 160 old inputs from the accepted release,
   candidate job = the sibling, conflicts include `cross_query_certificate_overlap` for SUN-6000. The queue also re-derives each bundle with the final code.
7. Independent decisions: `python3 -m devicehunt.review_queue decide ITEM accepted|rejected --reviewer NAME --reason WHY --queue data/campaign-reviews/<coverage id>` for **all 12** items (c3b precedent; reviewer is not the author). The Celestica reason must state the **Kladno-only judgement** and the numbers (5 new Celestica identities, role assertions +5, Monterrey unmapped context, Oracle-query row raw-only; review S4); the deciding reviewer is not this author. A rejection leaves Celestica at its baseline entry (`candidate_job` forbidden by the gate) and nothing else changes.
8. Release: `python3 -m devicehunt.review_queue release data/campaign-reviews/<coverage id> --coverage-manifest COVERAGE.json` then `check-release`.
9. Manifest: byte copy `data/import-manifest-before-sun6000-2026-10-11.json`; candidate manifest = job swap, new files hashed (bundle, wrapper, release, coverage manifest), `campaign_gate` -> new release + coverage, `expected.devices/records` +5. `rebuild.read_manifest` / `gate_inputs` check inputs before any import.
10. Candidate catalog: integrator-style staging (`research/batches/discovery-2026-10-11/integration/integrate.py` pattern): backup-copy the live catalog to a new path, `import_source(c, 'rra', bundle-composite-cells.json)`, audit that every live row survives except the 5 additions.
11. Clean gated rebuild (this runs `gate_inputs` before and `gate_projections` after all imports):
    `python3 -m devicehunt --db data/rebuilt-sun6000-2026-10-11.sqlite rebuild --manifest data/import-manifest-candidate-sun6000-2026-10-11.json`;
    `PYTHONPATH=. python3 scripts/compare_rebuild.py data/rebuilt-sun6000-2026-10-11.sqlite --database <candidate catalog> --output-prefix sun6000-2026-10-11`;
    `python3 -m devicehunt --db <candidate> validate-inventory`. Expect equality in all tables/indexes + baseline. Repeat c3b's check that the gated rebuild also passes with `data/archive` and the review queue moved away (rebuild never opens them; review item 4).
12. Guarded suite **before** promotion (should be fully green): `FCC_C1_ACCEPTANCE_LABEL=sun6000-pre python3 research/batches/campaign-c2-2026-10-05/acceptance/run_offline_suite.py`.
13. Promote (integrator): backup `data/devices-before-sun6000-2026-10-11.sqlite`; replace `data/devices.sqlite` and `data/import-manifest.json` with the verified candidates; repoint `DEFAULT_DEFINITION`/`DEFAULT_INVENTORY` in `devicehunt/campaign.py`.
**Rollback (review S2):** a failure after step 13 restores, together, `data/devices-before-sun6000-2026-10-11.sqlite` -> `data/devices.sqlite`, `data/import-manifest-before-sun6000-2026-10-11.json` -> `data/import-manifest.json`, and the previous `DEFAULT_*` constants in `devicehunt/campaign.py`; none of the three is restored alone.
14. After promotion: `python3 -m unittest tests.test_review_queue.AcceptedManifestGateTests` (+ `tests.test_campaign tests.test_coverage` which read the default inventory), full guarded suite again, refresh exports/lookups/collection reports, PROGRESS/README/docs (vendor-inventory totals), `python3 research/…/pin_inventory.py verify` noting the pinned checkpoint hashes.

## 6. Risks and what the independent reviewer must judge

Evidence-policy fit (the reviewer's main task; `docs/evidence-policy.md`: "An exact company-name association is not proof of physical manufacture. Review dated entity relationships before mapping aliases or parent/subsidiary portfolios."):

1. **Monterrey is not mapped (rev 2).** No saved extract relates "Celestica de Monterrey S.A. de C.V." to Celestica; mapping it on name + query hit alone would be a basis the project refuses elsewhere (e.g. `Celestica Czech Republic, s.r.o.` on SUN-445, `Celestica Philippines, Inc.`, `Celestica Corp.`) and would treat three byte-identical two-name lists differently. Kladno rests on the existing archived-21.1 extract (period unestablished, a reviewed text extract). The user chose proposal A with both parts; the narrowing is the coordinator's, to be reported to the user, who may override. Counts do not discriminate (Celestica +5 identities either way; role assertions +5 vs +10); the reviewer should judge only whether Kladno-only is the right reading and whether a note to the user suffices.
2. **Legal manufacturer is not the factory**, and one country field (Mexico) covers two named factories (Kladno is a Czech city; the page does not say so). The draft makes no per-factory claim; facts carry `country_raw` literally.
3. **Four derived names from one 2007 certificate** (`Netra 6000 AC`, `Netra 6000 Chassis`, `Oracle Sun Blade 6000 Chassis M2`, `Sun Blade 6000 Chassis`) gain Celestica manufacturer identities; Oracle's public docs (separate source, never merged) show chassis variants through 2013. Names stay RRA-asserted, identical in the existing parser.
4. **Not independent corroboration**: Oracle-query row 215 is the same page; raw-only by design. The `live_reobservation` receipts are re-fetches of the same page.
5. **Parenthetical misreading**: literal fact only (so is the Monterrey context); a test pins that no Oracle affiliation is created from it (the global alias would otherwise map it).
6. **Exception to "composite companies are not split"**: limited to hash-pinned, certificate-scoped entries; the guard is exact; docs and the source coverage statement name the exception. Strict "entry must match a row" is a design choice that makes a stale entry fail a future refresh bundle (recoverable offline).
7. **Reviewed text extracts**, not original HTML/PDF, underlie the Kladno mapping (existing limitation, unchanged).
8. **Process risks**: code must be final before `campaign run`; re-pin before cycle-2 rebundle is refused; the gate-less pin-base manifest is a procedural step outside the checked tooling (batch-local script copy); all 12 queue items need decisions (11 are `no_changes`); a leftover live-bundle `-carried` directory does not carry the wrapper (those pre-date it).
9. **Follow-up (not done, outside the decision)**: whether `SUN-X6200-Series`/`SUN-T6300` should get the same scoped treatment for Monterrey, and whether EXR-A350-2 (Kladno alone) already behaves consistently.

## 7. Phase-1 hygiene

No file under `devicehunt/`, `tests/` or `data/` was modified; the scratch copy of the package, the patched code, bundle sibling and the 396 MB live-catalog copy lived in the session scratchpad (catalog copy deleted; the live catalog was opened read-only / via the backup API). Waiting for the coordinator's "cycle 2 done" and phase 2 go-ahead.
