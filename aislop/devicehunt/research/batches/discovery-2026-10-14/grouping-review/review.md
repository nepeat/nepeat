# independent specification grouping implementation review

reviewer `/root/grouping_review`; completed 2026-10-15 UTC / 2026-10-14 America/Los_Angeles. **bounded review complete; request changes before production acceptance.** this is a review of the frozen private overlay, not authorization to apply it. the grouping collector passed the focused suite and independent evidence checks. one table-rendering gap needs correction, and the private test file needs adaptation before production application. the outstanding CLI help assertion is reproduced and explained; a whitespace-normalized assertion passes without changing application help.

## findings

### R1 — medium: table output hides the raw distinction between separate groups

location: frozen `overlay/devicehunt/__main__.py:103–111`, `write_specification_groups`. its group header excludes evidence, and its member lines omit `raw_fact`. `source_fact_statement` deliberately strips DMI citation paths from the displayed statement. two different DMI raw values on the **same record** therefore produce different strict groups whose entire rendered table blocks are byte-identical. neither raw value is available in the output. retaining both groups in memory/JSON does not make that distinction inspectable in the default grouped table format.

reproduction: [review_checks.py](review_checks.py) adds these two `dmi_chassis_type` facts to one record with URL `https://example.invalid/a/one`:

```text
Rack Mount Chassis (a/0123456789ab:1)
Rack Mount Chassis (b/0123456789ab:2)
```

both display as `chassis type reported by firmware in submitted dmidecode dumps: Rack Mount Chassis`. the strict grouping key correctly preserves two groups. [dmi-raw-evidence.json](dmi-raw-evidence.json) preserves their distinct values; [dmi-table-output.txt](dmi-table-output.txt) contains two identical group/member blocks with neither citation. this is an output-evidence gap, not a grouping merge or data loss in the database/JSON/CSV. it matters to the accepted review requirement that raw values remain available despite display formatting.

recommended exact change: expose the shared raw fact in the table header whenever its value differs from the displayed statement. all members of one group have the same raw name/value by the approved strict key, so it can be printed once. replace the header construction with:

```python
shared = {key: value for key, value in group.items() if key != 'evidence'}
raw_fact = group['evidence'][0]['raw_fact']
if raw_fact['value'] != group['statement']:
    shared['raw_fact'] = raw_fact
print(json.dumps(shared, ensure_ascii=False), file=out)
```

this preserves the existing “shared statement once” behavior for curated facts, whose raw value equals the statement; the category already preserves their raw name. source facts retain both the formatted statement and shared raw name/value. keep each member's provenance lines. add a regression with the same-record DMI pair, requiring two distinguishable table blocks and both original values, while retaining the existing shared-statement-count assertion. this recommendation was not applied or tested as a fix in this review.

### R2 — production integration blocker: the patch's new test module requires the private overlay layout

location: frozen `tests/test_specification_grouping.py:17–31`, and legacy references at lines71,78,80,164. importing this test requires production modules to be beneath `OWN / 'overlay'`, then loads `OWN / 'originals/specifications.py'` and `OWN / 'originals/__main__.py'`. those files/directories are absent from a normal project layout and are not added by `implementation.patch`.

reproduction: [production-layout-reproduction/patch-application.log](production-layout-reproduction/patch-application.log) shows the frozen patch applying cleanly to an isolated copy with normal `devicehunt/` and `tests/` paths. running unittest discovery against that copy exits1 immediately at line19, before executing a grouping test: [production-test-layout.log](production-layout-reproduction/production-test-layout.log). the error points to the copied production-layout `devicehunt/__init__.py`, which correctly is not inside an `overlay/` directory. removing that assertion alone would leave the subsequent `originals/` dependency unresolved.

this is expected infrastructure for a private proof, but means the patch is not a complete production-ready test change. recommended adaptation:

1. keep overlay resolution guards and legacy-module loading in a private review/acceptance runner; remove them and `importlib.util` from the production test module.
2. replace `LEGACY.search` / `LEGACY_CLI` comparisons in `test_unflagged_api_and_all_cli_formats_are_byte_identical` with pinned expected default API rows and serialized JSON/CSV/table output generated from the hash-bound original modules. do not replace historical parity with only a comparison between omitted/False flags on the new implementation. the private legacy comparison may remain an additional pre-application acceptance check.
3. for the curated reconstruction assertion at line164, use the existing unchanged `specifications.curated_statement_rows(self.c)`, or explicit independently specified fixture rows. keep the meaningful raw-evidence, record-local scope, ordering, limit, format and read-only tests.
4. run the ported test file from a normal project layout and the project offline acceptance runner after the coordinator releases the freeze. retain the shared-state/data milestone gate.

### C1 — resolved diagnosis: CLI help assertion depends on argparse wrapping

the original [cli_checks.py](cli_checks.py) fails at line39 exactly as reported; [cli-reproduction.log](cli-reproduction.log) retains that failure. argparse prints the wording as `Limits count\n                        complete groups` at the normal width. the flag, evidence semantics and group-limit explanation are present.

the minimal runner fix is:

```python
assert '--group-identical' in help_out.stdout
assert 'Limits count complete groups' in ' '.join(help_out.stdout.split())
```

only the copied runner was changed, in [cli_checks_normalized.py](cli_checks_normalized.py). it passes: [cli-normalized.log](cli-normalized.log), [cli-checks.json](cli-checks.json). that includes parent/child overlay module resolution, grouped two-source output, unchanged fixture bytes, missing DB failure without creating the file, absent overlay default data and help semantics. [help-width-40.txt](help-width-40.txt), [help-width-80.txt](help-width-80.txt), [help-width-120.txt](help-width-120.txt) all fail the original contiguous phrase check and pass the normalized check. no production help-string change is needed.

## correctness and compatibility assessment

- `search(..., group_identical=False)` retains the original branch, validation, output shape and ordering; the sole CLI branch change selects the specialized renderer only with the flag. generic `write_rows` is unchanged. focused parity covers API plus all three CLI formats; independent parity covers16 query/filter/source-fact combinations.
- grouped mode reads pre-collapse raw facts, selects records by exact model/alias/part-number and their own vendor affiliations before grouping, and keeps same-source duplicate records. no sibling affiliation or identifier is borrowed. alias/part matching retains casefold/outer-trim semantics and literal internal spaces/suffixes.
- the key is exactly `(device_id, category, origin, source_fact_name, statement, raw_value)`. brand/revision namespaces, curated/source origins, numbered configuration names, conflicts and different raw strings stay distinct. no identity, source role, manufacturer or configuration inference is introduced.
- JSON/CSV retain complete raw facts, source keys/URLs/tiers, record-local affiliations and applicable matched part numbers. evidence/record/source counts reflect their stated cardinalities. group construction precedes slicing, so retained groups keep later members. no per-member query is issued; the focused bulk-query test verifies constant query counts as evidence grows. whole-table bulk scans remain a scaling limit, not an observed correctness failure at this bounded review.
- R1 is the remaining evidence-presentation exception. no other grouping/evidence/default regression was found in the inspected code and executed checks.

## checks and provenance

all writes stayed under this review directory. runtime was project `python3` 3.14.7. no network, installations, delegation, imports into shared catalog state, live DB queries, manifest changes, production edits or original-overlay edits occurred. temporary fixture paths were confined here via `TMPDIR`/explicit `dir`; fixtures were removed by their context managers. original dated live-case evidence was inspected rather than rerun against a concurrently changing data milestone.

| check | actual result |
| --- | --- |
| copied original focused runner: 11new +37existing tests | 48passed,0failures/errors/skips,36.549s; no blocked network/process attempts; [proof](focused-tests-result.json), [log](focused-tests.log) |
| independent8scope cases ×2source-fact settings | all16 raw-fact multisets, default API parity, counts, limit1 completeness, JSON/CSV reconstruction and table-member counts pass; [proof](independent-checks.json), [runner](review_checks.py) |
| same-record DMI table distinction | defect reproduced: two distinct raw groups render identically |
| original CLI runner | expected exit1 at help assertion; not counted as passing |
| whitespace-normalized copied CLI runner | exit0; fixture SHA before/after `cb343deea8000f797af9cd8b24b6627d96d8ebeeed644cf2879ab67230f11931` |
| production-layout patch/test smoke | patch applies; unittest import fails at private overlay assertion, as described in R2 |
| frozen inputs | all117 original implementation files unchanged; all116 original `SHA256SUMS` entries verify; all209 frozen production/test files unchanged; [integrity proof](integrity-checks.json), [starting hashes](inputs-before.json) |

selected reviewed SHA-256 values:

| input | SHA-256 |
| --- | --- |
| frozen `implementation.patch` | `16f4d8905ea65830856b4456f7d8e2b7c3bc237d0c054871f37439941ec32068` |
| frozen overlay `specifications.py` | `ba2cf7a77c4e403a3af5f2cb892ef3f5380158f5480e1d655fa3aecde155ba8c` |
| frozen overlay `__main__.py` | `9ede6002c0386a51815c7ed6949d0d57a3d001883edbd533ffd5b230def5deb8` |
| frozen grouping test | `a6aa45f6b24d35b7d3955636afc9765355095568e853495ca1d88c77720bed68` |
| design | `d65c72c95f8655faf7e40b6cff3bf3fcff5bcdfdc41843d99a1dfe661c242097` |

to reproduce from this directory, use `PYTHONDONTWRITEBYTECODE=1 PYTHONPATH="$PWD/overlay" TMPDIR="$PWD/tmp" python3 -B review_checks.py`, or substitute `run_tests.py` / `cli_checks_normalized.py`. the frozen original runner is separately preserved as `cli_checks.py`; running it intentionally reproduces the assertion failure. the integration-layout failure command is `PYTHONDONTWRITEBYTECODE=1 PYTHONPATH="$PWD" python3 -B -m unittest discover -s tests -p test_specification_grouping.py` from `production-layout-reproduction/`.

next bounded step: coordinator assigns R1's renderer/regression and R2's production test adaptation in a fresh private overlay, retains the original frozen artifacts, then independently reviews the resulting patch and documentation. a passing private focused suite is not a full production suite or acceptance to apply. this review stops here.
