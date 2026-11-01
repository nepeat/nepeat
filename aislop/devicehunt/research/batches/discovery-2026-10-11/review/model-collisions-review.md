# model-collisions review (2026-10-11)

**Verdict: ACCEPT with notes** (one must-fix: the `Unattributed accessory` exclusion is a dead rule, it names a kind but is a brand).

Scope: handoff, `devicehunt/model_collisions.py`, `__main__.py` wiring, `tests/test_model_collisions.py`, README subsection. Probes and mutants ran on copies in the scratchpad; the repo was not modified except this file.

## What checked out

- Tests: `python3 -m unittest tests.test_model_collisions` -> 8 OK.
- Read-only: CLI opens `?mode=ro`; `run()` wraps in BEGIN/rollback and only SELECTs. md5 of `data/devices.sqlite` and `data/devices-candidate-discovery-2026-10-11-b.sqlite` identical before and after runs; a missing DB fails and is not created (tested).
- `__main__.py`: the directory is untracked in git, so no diff was possible. By inspection the change is wiring only: one import, one `add_parser('model-collisions')` block, one dispatch branch (same `mode=ro` pattern as `fcc-coverage`). The lookup-context edit (table strips `matched_in_detail`, appends `matched_in` last; JSON keys) is still present at lines ~443-449. Nothing else touched.
- Normalisation matches the docstring: casefold plus whitespace collapse, optional one leading own-brand token (whole brand tried first, then first word), non-empty remainder. No suffix, hyphen, revision or paren handling. `Inspur` vs `Inspurious NF1` not stripped (tested: needs a trailing space).
- Dataset note present in output (`note`, table line 1, JSON) and README: "does not merge, alias or assert"; test pins `does not merge`.
- Deterministic: two JSON runs on candidate b are byte-identical. Sort is (-size, key), members (brand, model, id); sets are sorted before emit.
- Candidate b: 351 groups (326/18/7), skipped by kind 445, matches handoff. `data/devices.sqlite`: 282 groups (264/17/1), skipped 444.
- Sanity of groups: `smc7004vwbr` (Accton/SMC x3 incl. brand `SMC/SMC7004VWBR`), `ap-5131` (Accton/Symbol), `eap102` (Accton/Edge-Core/Edge-corE), Inspur/IEIT NF family, Oracle `SPARC S7-2` prefix pairs, `x4470` (Oracle vs brand `Sun Fire`) are all plausible review leads and none overstates.

## Must-fix

**M1. `Unattributed accessory` is a brand, not a kind; the exclusion never fires.** `EXCLUDED_KINDS = {'unattributed accessory'}` is compared to the casefolded `kind`. In the catalog `kind='Unattributed accessory'` has 0 rows in both DBs; the string is the brand of 49 devices in candidate b (40 in devices.sqlite), whose kinds are `Accessories`, `Power Supply FRU`, `Fan Module FRU`, `PSU FRUs`, `FAN FRUs`, `FRU`, `Fan Tray FRU`, etc. (see `research/batches/hunt-2026-10-10/edgecore/handoff.md` and netbox-review S1). The unit test passes only because it fabricates `kind='Unattributed accessory'`. Handoff, README, NOTE text and `--all-kinds` help all repeat the wrong claim. Impact today: none (0 of 351 groups contain such a brand), so no wrong output, but the documented rule is false and untested against reality. Fix: either (a) delete the rule and the claims, or (b) make it a brand exclusion and test with brand. My recommendation is (a)/leave them in: after the netbox S1 convention, `Edge-Core FAN-1U-1x1I-F` vs `Unattributed accessory FAN-1U-1x1I-F` is exactly the sort of lead this report should surface, and excluding the brand would hide it. If kept as a brand rule, say "brand" everywhere and change the test.

## Should-fix

**S1. Weak-key annotation / optional minimum length (item 6).** Candidate b has keys of length 4: `a5qb` (Hyve `server` vs Quanta `wireless device server`), `a5wf` (Amazon Data Services vs Quanta), `om1p` (Accton `radio equipment` vs Open Mesh); 9 keys are <=5 chars. These are FCC-ID-style codes and the member kinds disagree. Do not drop them by default (they are legitimate leads and hiding them is silent loss). Recommend: add `--min-key-length N` (default 0) and a per-group boolean `short_key` (len < 6) so a reviewer can sort/skip; optionally flag `kinds_differ` (set of casefolded kinds >1) as a second weak-evidence hint. Handoff "limits" already admits no minimum; the report should say so in the README too.

**S2. `--target` semantics are undocumented in README/note.** Code: a group is kept if ANY member's brand resolves via `target_for(brand)` to the target OR any member has an affiliation row with `target == wanted`; ALL members of a kept group are then shown, including ones with no tie. Help text says "Require a member tied to this target company", which is right but README/handoff never state it. Checked `--target Inspur` on candidate b: 51 groups, all with an Inspur/IEIT-brand member, so no leakage in practice, but affiliation-only inclusion (e.g. a relabel brand carrying an Accton affiliation) will pull in groups whose brand names never mention the target. One README sentence suffices. `--limit 0` also still reports counts (works, help says so).

**S3. Two mutants survive (item 7).** (a) Removing the "same brand + same plain key -> continue" drop (M7) leaves tests green; add a fixture pair like `('Edge-Core','EAP1')` / `('Edge-corE','EAP1')` (only case/brand-spelling differs, same plain key) and assert no group. Today's Edge-corE/Edge-Core row in the real data would be caught by that. (b) Removing the group sort (M8) leaves tests green; add a determinism test (two runs equal, sizes non-increasing, ties by key). Also the shipped `test_kind_exclusion` should use a real kind list, not the fabricated one (see M1).

## Notes

**N1. First-word brand stripping is safe on this data and acceptable in design.** In candidate b the first-word path fires on only 12 devices: `Inspur Corporation` x7 (`Inspur i24LM6`), `Inspur Electronic ... (IEI)` x2, `Sun Microsystems` x2 (`Sun Fire V490`), `Sun Microsystems, Inc.` x1 (`Sun Netra X4250`). None is generic. Whole-brand stripping dominates (Oracle 232). Reasoning on the two examples asked: `Sun Microsystems` stripping `Sun` from `Sun Fire X4170` yields `fire x4170`, which would only collide with another member whose stripped key is also `fire x4170`; a plain `Sun Fire X4170` under a different brand is NOT stripped (`sun fire x4170`), so the likely true relabel is missed, never falsely merged. `Inspur Power Commercial Systems` stripping `Inspur` makes `Inspur NF5280M6` and bare `NF5280M6` equal, which is the intended lead. Theoretical false-collision risk needs a brand whose first word is generic ("Open Mesh", "Digital Equipment", "Network Appliance", "Super Micro"): `Open Mesh Open 5` -> `5` would match any bare `5`, but that again requires another member with the same remainder, so the failure mode is a noisy lead, not a merge. Because strip applies per member only to its own brand, a brand can never strip another's name. Since every output member carries `brand_token_stripped`, the reviewer can see which path fired. Optional hardening: skip first-word stripping when the first word is under 3 chars or the remainder is under 3 chars (e.g. `HP 1` vs `3Com 1`); cheap, and consistent with S1.

**N2. Brand variants as "different brand".** Brands are compared casefolded only, so `Quanta` vs `Quanta Cloud Technology`, `QCT`, `NORTL NETWORKS` vs `NORTEL NETWORKS` are distinct and surface as same_model_different_brand. That is the intent (no alias mapping) and is stated in the handoff.

**N3. Reason edge case.** A group that mixes one brand's prefix pair with a second brand's bare key is labelled `brand_token_prefix_different_brand`; a group of two brands whose plain keys are equal is `same_model_different_brand` even if a third member of one brand differs only by prefix: impossible by construction because `same_plain` is over all members, so the label is correct.

**N4. Table output lacks a divider for long affiliations** (cosmetic: rows are one long pipe-separated line; fine for grep). JSON schema (`note, all_kinds, target, skipped_by_kind, groups_total, counts_by_reason, shown, truncated, groups[{key, reason, size, members[...]}]`) is reasonable; consider adding `schema_version` if downstream tooling is planned.

**N5. Kind exclusion vs actual kinds otherwise correct.** Excluded kinds present in candidate b are exactly 8 distinct (183 `software configuration (BMC hardware detection)`, 154 `network switch software platform definition`, 61 ONIE machine definition, 19 + 14 BMC firmware, 12 BMC device tree source, 1 `networking software artifact reference`, 1 `virtual machine configuration`; total 445). Substring `config` is broad, but no hardware kind in the catalog contains `config`/`bmc`/`software`/`firmware`/`device tree` (checked all kinds). Kept near-miss kinds `retired hardware product`, `seller-listed hardware`, `documented component option`, FRU kinds are hardware and fine to keep. Substring matching means a future kind like `Configurable Server` would be wrongly skipped; acceptable but worth an exact-phrase list if kinds grow.

## Mutants (on copies)

| mutant | result |
| --- | --- |
| strip hyphens in `_collapse` | killed (2 fail, 1 error) |
| strip any leading token | killed |
| drop kind exclusion entirely | killed |
| keep only the (dead) `unattributed accessory` rule | killed (by the other kind assertions) |
| strip `-o-ac-f` suffix | killed |
| disable first-word brand stripping | killed |
| keep same-brand-same-plain groups | SURVIVES (S3a) |
| remove group sort | SURVIVES (S3b) |

## re-review

**Verdict: ACCEPT.** No must-fix; M1 and S1-S3 are resolved.

- Tests: `python3 -m unittest tests.test_model_collisions` -> 11 OK.
- M1: the `unattributed accessory` kind rule is gone from code, NOTE, docstring and README; the only remaining mention is a test comment that accessory-style kinds are not excluded. Kind rule is now substring words only (software, firmware, bmc, device tree, config), unchanged otherwise.
- S1: each group has `short_key` (key < 6 chars) and `kinds_differ`; `--min-key-length` (default 0) is wired in `__main__.py` and validated (nonnegative int). Nothing is dropped by default. Live DB: 9 short keys (`a5qb a5wf ax411 ip200 ip250 lr11b om1p vg007 x4470`), 266 groups with differing kinds; `--min-key-length 6` -> 342 groups (351 minus the 9).
- S2: README now states the `--target` rule (brand resolves to target, or any member affiliation with that target; all members of a kept group shown).
- S3: new tests kill the two surviving mutants (below).
- Live run: `data/devices.sqlite` (sha ab8ed2e3, now candidate b) gives 351 groups (326/18/7), skipped by kind 445, byte-identical across two JSON runs, md5 unchanged afterwards (read-only).
- Mutants on copies, all now killed: keep same-brand-same-plain groups (5 fail), remove sort (1 fail), drop kind exclusion (4), strip hyphens (2+2), strip any leading token (4+2), disable `short_key` (1), disable `kinds_differ` (1), ignore `min_key_length` (1).
- Remaining notes only: optional hardening from N1 (skip first-word strip for very short words) is not applied; not needed. `kinds_differ` is true for 266 of 351 groups, so it is a weak sort hint rather than a filter, since regulatory kinds are free text in many languages.
