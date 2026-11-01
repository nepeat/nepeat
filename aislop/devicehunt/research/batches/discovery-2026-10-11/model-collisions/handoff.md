# model-collisions handoff (2026-10-11)

New read-only report: `python3 -m devicehunt model-collisions [--db PATH] [--format table|json] [--target NAME] [--limit N] [--all-kinds]`.
Code `devicehunt/model_collisions.py`, wiring in `devicehunt/__main__.py`, tests `tests/test_model_collisions.py` (8 tests, OK), README subsection "model collision review leads".

## dataset note

REVIEW LEAD LIST ONLY. It does not merge, alias or assert that any two identities are the same product. It writes nothing (read-only URI connection; missing DB fails). Collisions can be real relabels (SMC/Accton) or unrelated products sharing a short code.

## normalisation

1. casefold, collapse whitespace (plain key).
2. optionally strip one leading token equal to the member's own brand (whole casefolded brand, or the first word of a multi-word brand, e.g. `Inspur Corporation` -> `inspur`), remainder non-empty (stripped key).
Never stripped: suffixes, hyphens, revision letters, parenthesised groups. `AS7726-32X` vs `AS7726-32X-O-AC-F` is not a collision (tested).
Group = same stripped key, at least 2 distinct (brand, model) pairs. Reasons: `same_model_different_brand`, `brand_token_prefix` (one brand, differs only by prefix), `brand_token_prefix_different_brand` (both). Same brand + same plain key is dropped.
Skipped kinds unless `--all-kinds`: casefolded kind containing `software`, `firmware`, `bmc`, `device tree`, `config`. (`Unattributed accessory` is a brand, not a kind, and is not excluded: Edge-Core vs Unattributed accessory FAN/PSU strings are intended leads.)

## run against data/devices-candidate-discovery-2026-10-11-b.sqlite (5015 devices; 445 skipped by kind)

351 groups: same_model_different_brand 326, brand_token_prefix 18, brand_token_prefix_different_brand 7. Brands per group: 321 groups of 2, 12 of 3, 18 single-brand. 85 groups include a `registered equipment` or `radio equipment` member. Sizes are small (max 5).

20 largest groups (size, reason, key, brands):
1. 5 same_model_different_brand `smc7004vwbr`: Accton, SMC, `SMC/SMC7004VWBR`
2. 4 prefix_diff_brand `nf5280a7`: IEIT Systems Co., Ltd.; Inspur; Inspur Electronic ... (IEI) (`NF5280A7` and `Inspur NF5280A7`)
3. 3 same_model `ap-5131`: Accton, Symbol (x2)
4. 3 same_model `eap102`: Accton, Edge-Core, Edge-corE
5. 3 prefix_diff_brand `nf5180a7`: IEIT, Inspur Electronic ... (IEI) x2 (bare and `Inspur NF5180A7`)
6. 3 prefix_diff_brand `nf5180m6`: IEIT SYSTEMS Co.,Ltd.; Inspur; Inspur Corporation (`Inspur NF5180M6`)
7. 3 same_model `nf5180m7`: IEIT, Inspur, Inspur Electronic ... (IEI)
8. 3 same_model `nf5280m7`: IEIT, Inspur, Inspur Electronic ... (IEI)
9. 3 prefix_diff_brand `nf8480m6`: IEIT SYSTEMS, Inspur, Inspur Corporation
10-12. 3 same_model `smc2555w-ag`, `smc2804wbr`, `smc7004awbr`: Accton, SMC (x2)
13-15. 3 same_model `sp-w2-ac1200`, `sp-w2m-ac1200`, `sp-w2m-ac1200-poe`: Accton, Edge-Core, IgniteNet
16-17. 3 same_model `wlan access point 2220` / `2221`: Accton, Nortel (misspelt `NORTL NETWORKS` / `NORTEL NETWORKS` brand variants)
18-20. 2 same_model `3crshpw796`, `3crwe154g72`, `3crwe41196`: 3Com, Accton (first of many 2-member groups)

Beyond the top 20, the notable families:
- Quanta / QCT / Quanta Cloud Technology: 16 groups where the same `QuantaGrid`/`QuantaPlex` model is held under `Quanta` and either `QCT` (kind `server platform`) or `Quanta Cloud Technology`. Matches spec-review N1 (convention: QCT never mapped to Quanta).
- Inspur / IEIT / IEI / Inspur Corporation: the groups above plus `nf5180m5`, `nf5280m5` (`Inspur` vs `Inspur Corporation` + `Inspur ` prefix; spec-review N2, redhat-cert S3).
- Oracle: all 18 `brand_token_prefix` groups (`SPARC S7-2` vs `Oracle SPARC S7-2`, `Sun Fire X4270 M2` etc.), all under one brand `Oracle`; plus `x4470` (Oracle `X4470` vs brand `Sun Fire` `Sun Fire X4470`).
- Accton versus OEM relabel brands (SMC, 3Com, Symbol, Nortel, IgniteNet, Edge-Core): expected ODM/OEM sharing; the Accton-branded member is typically the regulatory-register identity.

## mis-merge risk observations (nothing was merged)

- Short or opaque codes: `a5qb` (Hyve `server` vs Quanta `wireless device server`) and `a5wf` (Amazon Data Services vs Quanta) look like FCC-ID-style codes, and the kinds disagree. Treat as likely different things until checked.
- Generic names (`wlan access point 2220`, `database appliance`) are product-class labels, weak evidence of identity.
- Brand strings containing the model (`SMC/SMC7004VWBR`, `Sun Fire`) are as-printed oddities; they are not mapped.
- Oracle same-brand prefix pairs and Inspur prefix pairs are the highest-confidence relabel/format duplicates, but also differing printed names; any merge needs a reviewed alias decision (IEIT is deliberately never mapped to Inspur).

## limits

- Only a token equal to the member's own brand is stripped. Bare `D51B-2U` versus `QuantaGrid D51B-2U` (redhat-cert S1 family-prefix split) is NOT detected, since `QuantaGrid` is not the brand. A family-token pass would need a reviewed list.
- No minimum key length: very short codes collide across unrelated brands.
- Affiliations are the distinct (company, role) pairs over all records of the identity; source tiers are the tiers of those records' sources.

## verification

`python3 -m unittest tests.test_model_collisions`: Ran 8 tests, OK. Full suite not run (coordinator). data/ not written; DBs opened with `mode=ro`.

## revision after review (M1, S1-S3)

- M1: the `Unattributed accessory` exclusion was removed (it is a brand, never a kind); code, note, `--all-kinds` help, README and this handoff no longer claim it. Group counts on candidate b are unchanged (351 = 326 / 18 / 7; 445 skipped by kind).
- S1: each group has `short_key` (key under 6 chars) and `kinds_differ`; `--min-key-length N` (default 0) optionally drops short keys. Candidate b: 9 short-key groups (`a5qb`, `a5wf`, `ax411`, `ip200`, `ip250`, `lr11b`, `om1p`, `vg007`, `x4470`), 266 groups with differing kinds, 8 groups both short and kind-differing (weakest leads). Nothing is dropped by default.
- S2: README documents `--target` semantics (any member's brand resolves to the target or any member has an affiliation with it; all members shown).
- S3: tests added for same-brand same-plain-key (no group), deterministic ordering, flags and `--min-key-length`. `python3 -m unittest tests.test_model_collisions`: Ran 11 tests, OK.
