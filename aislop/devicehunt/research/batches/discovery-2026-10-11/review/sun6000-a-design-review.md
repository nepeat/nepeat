# SUN-6000 "A: scoped company evidence" design review (independent, Claude Sonnet 5.5) - 2026-10-11

Verdict: ACCEPT with notes. One must-fix: change the Monterrey part to unmapped context (Kladno-only), then re-run the prototype and re-hash.

Method (all read-only, no network, nothing written under data/ or devicehunt/): read AGENTS.md, evidence-policy, the SUN-6000 handoff and 2026-10-10 review, the full design.md, both drafts, prototype.diff and the prototype outputs; re-hashed `detail-090.html` (3c01977f...870b ok), the raw cell slice (offset 3897, 110 bytes, sha 4d03ef3a...083c ok, CRLF/tab layout as described), the parser-text cell (4b68ec1b...23d5 ok) and the wrapper draft (92f4a598...1c47 = bundle draft's `company_evidence` sha; the bundle draft differs from live `bundle.json` in exactly that one key); confirmed live `bundle.json` sha 6aacf7a8... equals `data/raw/rra-manufacturer-lists.json` `captures[0]`; queried the live catalog read-only (via `mode=ro`); checked `target_for()`, `review_queue.build_queue/gate_inputs`, `MovedBundleBaselineTests`, `DEFAULT_*`.

## 1. Evidence policy: Monterrey (explicit recommendation)

**Recommendation: Kladno-only (A-narrow). Do not map Celestica de Monterrey S.A. de C.V. to Celestica.** Neither-mapped is not needed: Kladno has the same standing as the accepted precedent.

Why Monterrey does not meet the project's rules:
- evidence-policy: "Review dated entity relationships before mapping aliases or parent/subsidiary portfolios", and an exact company-name association is not proof of manufacture. Monterrey is a separate legal entity; no saved extract (2025 Exhibit 21.1, SEC 8.1, archived 21.1) relates it to Celestica. I grepped `data/raw/rra-celestica-official`, `rra-manufacturer-lists.json` and `research/*.md`: "Monterrey" occurs only in the three RRA detail pages and in docs that say it is unmapped. Name contains "Celestica" plus "returned by the Celestica query" is a substring match, not an entity-relationship review.
- The project already refuses this exact basis elsewhere. In the same Celestica query capture (live `rra_equipment`, source `rra-official-query-41e4ca912b33`) these Celestica-named manufacturers are returned by the same regulator query yet carry no Celestica target: `Celestica Czech Republic, s.r.o..` (SUN-445), `CELESTICA ELECTRONICS (S) PTE LTD`, `Celestica Philippines, Inc.`, `Celestica Corp.`, `Celestica Electronics (M) Sdn Bhd` (inside a Keysight parenthetical). Mapping Monterrey on name+query alone would be the first time that basis is accepted, and would make SUN-445's Czech entity indefensibly inconsistent with it. The mapped Celestica names (Dongguan, Suzhou, Shanghai, Thailand, Kladno, Inc., LLC) all rest on a saved reviewed extract.
- Precedent: `rra-reviewed-manufacturer-lists` for SUN-X6200-Series and SUN-T6300 (same two-name list) maps Kladno only; Monterrey is kept as `manufacturer_list_context` and `rra_manufacturer_as_printed`, not as an affiliation (verified in the live DB for X6200_Series). `research/rra-manufacturer-lists.md` states this as a rule. Mapping both here creates two different treatments of byte-identical two-name lists on three certificates of the same 2007 date.
- Cost of narrowing is nil for the headline: five Celestica identities either way (the five device ids do not exist today: 0 Celestica-brand rows; Oracle `Sun Blade 6000 Chassis` is a different brand row). Only role assertions differ (+5 instead of +10).
- Kladno's own basis is thin (archived extract, period "not established", reviewed web-search extract not original filing) but it is the existing accepted standard and the unit already maps it; this design adds no new weakness there.

Caveat for the coordinator: the handoff's proposal A text (which the user chose) lists Monterrey as `registry_manufacturer`/Celestica, and listed the Monterrey absence as risk (1). Kladno-only is a narrowing of what the user approved, so tell the user in one line and let them override; I do not think an override is warranted. The design's own "A-narrow" option and its loader allowance are the right shape.

## 2. Role and claims: correct

- `registry_manufacturer` only, applicant stays `regulatory_applicant`, no factory/country/sole-manufacturer/unit-provenance claim, one country field (멕시코) not split per part, derived names RRA-asserted, not merged with Oracle "Sun Blade 6000 Modular System" or Oracle `Sun Blade 6000 Chassis`. All correct against the policy and my 2026-10-10 finding 3.
- Parenthetical: confirmed `target_for('(Oracle America, Inc.)')` is `Oracle`, so the global alias WOULD create an Oracle company affiliation if it reached `Catalog.add`. Keeping it out of affiliations (literal `manufacturer_cell_unlabelled_text` fact only) is right, and the planned test that no such company row exists is essential.
- Under the narrow variant the Monterrey text should likewise be a fact only (as `manufacturer_list_context` does), not an affiliation with a null target, so the affiliation set is Kladno + applicant.

## 3. Guards: sound, with two gaps

Verified in `prototype.diff` and `prototype-guard-checks.txt`: key is (certificate, sha256 of parsed cell text); additionally detail sha, detail url, `query_maker`, exact text equality, and "cell does not already resolve through `company_target`" must hold; load-time exact reconstruction (parts + `,` + space + parenthetical == cell), role set `{registry_manufacturer}`, target Celestica, name_evidence label checked against the base in both directions, duplicate and nested-wrapper refusals; an entry matching no row is refused. Prototype negatives (wrapper under Oracle query, edited part, entry for a changed cell) refuse; Oracle bundle and old `bundle.json` stay `matches_pinned`; manufacturer-lists summary stays 22/45 (Accton 6, Celestica 37, Oracle 2). No pattern splitting anywhere.

Gaps:
- (should-fix) `cell.raw_html_cell_bytes` (offset/length/sha) is never checked by the loader or planned tests; it is only prose context. I verified it is correct today, but add a test that slices `detail-090.html` and compares, or drop the field, so a reviewer-facing claim cannot rot unnoticed.
- (must-fix, narrow variant) `claim_target` returns `claim['parts'][0]['target']`. With Monterrey first and unmapped, `parts[0]` has no target and the row becomes ineligible. Use the first part that has a target (or require `parts` ordering). Also `load_composites` currently requires every part to be role `registry_manufacturer`/target Celestica; the context part needs an explicit `role: null`, `name_evidence: "unmapped_context"` form with its own checks (no target allowed, must not be mappable by base).

## 4. Layout: sibling bundle is the right call

Verified reasons: `bundle.json` sha is pinned in `rra-manufacturer-lists.json` `captures[0]` (so editing in place would cascade to the 37 Celestica rows of the un-gated `rra-reviewed-manufacturer-lists`, whose facts embed `capture_sha256`), by the c0-pin carry test, and used as `carry_from` by `live_rra.py rebundle`. Cost (moved job path) is exactly what `MovedBundleBaselineTests` exercises: baseline job = old path with the accepted release's old inputs, `candidate_job` = new path; `build_queue` takes the prior accepted release's job/inputs for units with a gate. Release `candidate_job` is only a gate-forbidden marker when not accepted. The alternative (in-place with re-hash and `bundle-before-composite-cells` copy) is strictly more blast radius. Accept sibling. Note: if the narrow variant is adopted, the wrapper sha, bundle sibling, pins, release and manifest hashes all change; nothing should be copied from the current draft by hand.

The wrapper-as-`company_evidence` approach reuses the declared-file hash check and `carry_from` machinery; `path` appears only in `base` (pinner treats `path` keys as dependencies) which I agree is necessary and clever. One watch item: the loader now returns `load(root, base)` for wrappers, so every existing caller sees the identical dict - good - but any future code that reads `data['company_evidence']['sha256']` as "the historical bundle hash" would now see the wrapper hash.

## 5. The 14-command c3b path: ordering correct; add four items

Ordering checks out against code: rebundle (cycle-2) must precede the re-pin because `live_rra.py rebundle` compares `acquisition.unit_revision` to the current default definition; `pin_inventory build` reads the manifest by constant and fails unless the manifest already hashes the new files, hence the batch-local gate-less pin-base manifest; the `AcceptedManifestGateTests` assertion `release.definition_sha256 == load_definition(DEFAULT_*)` is why `DEFAULT_*` repoint must be simultaneous with promotion (step 13), and `--definition/--inventory` flags are accepted by `campaign`, `coverage`, `review_queue` (checked: `campaign.py` and `coverage.py` argparse, `review_queue` paths). Sequence plan -> run -> report -> freeze -> queue -> decide all 12 -> release/check-release -> candidate manifest -> candidate catalog -> gated rebuild + compare -> guarded suite -> promote -> post-promotion suite is complete for a moved-bundle unit.

Should-fix additions:
1. Rollback line: state that failure after step 13 restores `data/devices-before-sun6000-2026-10-11.sqlite`, `data/import-manifest-before-sun6000-2026-10-11.json` and the `DEFAULT_*` constants (the three move together).
2. Step 3's gate-less pin-base manifest and the candidate manifests are extra files under `data/`; require they are listed in the batch handoff and not left looking like live manifests (the gate-less one must never be passed as `--accepted-manifest`).
3. Step 1 changes code hashes (recipe `code_sha256`) while `DEFAULT_*` still points at c3b until step 13: step 1's targeted tests (`test_campaign`, `test_review_queue`, `test_coverage` read the default inventory) must be run and reported green in that mixed state, not assumed; the design asserts the suite is green before promotion but only prototyped the later steps. Run it once as a dry run before opening the batch.
4. Step 11 (gated rebuild with archive/queue not present) should explicitly repeat c3b's "rebuild passes with archive and queue moved away" check.

Also: the cycle-2 coordination is a real dependency; the live RRA refresh now running means step 4 (`run --offline`) must not start until the coordinator says so (agreed in preconditions).

## 6. Predicted catalog effect: plausible

Re-measured the "before" column read-only against the live DB: devices 5,107 / records 7,225 / affiliations 10,557 / facts 74,074; Celestica manufacturer scope 193/239/241; manufacturer scope all targets 2,142 pairs / 2,141 identities; scope `all` 4,581/4,443; Celestica `all` 374/534/592; Oracle `all` 1,281/1,617/2,460; Oracle manufacturer 859 - all equal the design's before values. After values are internally consistent: +5 devices/records/identifiers; Celestica 193->198, records 239->244, role assertions 241->251 (two Celestica roles per record, i.e. both parts); manufacturer pairs/ids +5; `all` pairs +10 (5 Celestica + 5 Oracle applicant) with identities +5 (the five new device ids); affiliations +15 = 3 per record; facts +55 = 11 per record (6 plain + `rra_manufacturer_as_printed`, basis, unlabelled text, review, 2 parts, minus the empty `other_raw`... consistent with a 12-fact set less one empty); Oracle manufacturer scope unchanged (applicant role only), `rra_equipment` rows and `records_seen` unchanged. The "5 identities" claim is robust to the Monterrey decision.

Numbers to re-derive for the Kladno-only variant (do not copy the table): Celestica role assertions 241->246 (+5), affiliations +10 (Kladno + applicant per record), facts about +50 (one fewer `manufacturer_cell_part`-style fact per record; recount from a re-run, I did not run the patched code), identities/records/devices unchanged at +5.

## Findings summary

Must-fix
- M1: Drop the Monterrey -> Celestica mapping (Kladno-only; Monterrey stays literal context in a fact, role null, no affiliation). Implement the loader/`claim_target`/import changes in section 3, update `evidence_basis`/`name_basis`/`limits`/coverage wording and tests, regenerate the wrapper/bundle/pins, re-run the prototype scripts, replace the effect numbers, and tell the user the approved A is narrowed on evidence-policy grounds (one line, user may override).

Should-fix
- S1: test or remove `raw_html_cell_bytes` (unchecked by any code).
- S2: rollback paragraph for the promote step; keep the gate-less pin-base manifest out of any gated path.
- S3: run the targeted test list (step 1) as an actual dry-run in the mixed old-DEFAULT/new-code state before the batch.
- S4: c3b queue decision reason for Celestica must state the Kladno-only judgement and the identity-count/role-assertion numbers; reviewer separate from this author.
- S5: update `research/rra-manufacturer-lists.md` follow-up note (kept true under narrow) and `docs/vendor-inventory.md` totals, and say plainly in exports/lookup docs that Celestica-brand `Sun Blade 6000 Chassis` (RRA-asserted) and Oracle `Sun Blade 6000 Chassis` (retired hardware product) are distinct, unmerged identities.

Notes
- N1: Strict "entry must match a row" makes future cell drift fail a live bundle loudly; recoverable with an offline rebundle; acceptable.
- N2: The bare main model `6000` becomes a Celestica-brand identity; high collision/noise potential in lookups; it is the RRA's literal string, so keep it, but docs should flag it.
- N3: Oracle-query row 215 stays raw-only (no records), keeping the identical page from being double counted as corroboration; agree. The cross-query overlap conflict remains on both queue items.
- N4: Kladno evidence remains a reviewed text extract with unestablished period (pre-existing).
- N5: Prototype was run on a patched copy under the session scratchpad; I did not re-run it (it would not be read-only-safe to reproduce its 396 MB copy here), so after-numbers are checked for consistency, not re-measured.
