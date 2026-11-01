from pathlib import Path
p=Path('docs/vendor-inventory.md');s=p.read_text()
for old,new in [('389 | 0 | 1086','401 | 0 | 1096'),('1,696 target/identity pairs across 1,695','1,708 target/identity pairs across 1,707'),('3,417 pairs across 3,334','3,427 pairs across 3,344'),('1,435 manufacturer model assertions','1,448 manufacturer model assertions'),('passes 282 tests','passes 285 tests')]:
 assert old in s,old;s=s.replace(old,new)
s=s.replace('The table display gives compact identity rows;', 'The [reviewed Accton Singapore spelling](../research/rra-accton-company-name.md)\nadds 13 manufacturer assertions across 12 identities, using a separately hashed\nhistorical company-name extract with its dated parent relationship retained.\nThe table display gives compact identity rows;')
p.write_text(s)
p=Path('README.md');s=p.read_text();anchor='Separate Edge-Core spellings found seven additional approvals.';assert anchor in s
s=s.replace(anchor,'A [reviewed Accton Singapore legal name](research/rra-accton-company-name.md)\nadds 13 manufacturer assertions across twelve model identities from existing\napprovals; unverified company spellings remain raw searchable evidence.\n'+anchor);p.write_text(s)
with Path('GOAL.md').open('a') as f:f.write('''

## Accton Singapore manufacturer spelling — 2026-09-27 UTC

Reviewed the 50 previously unmapped Accton-query manufacturer rows. A public
search extract of Accton's 2013 consolidated report explicitly names Accton
Technology Corp. Pte Ltd (Singapore), reported under Edgecore Networks. Direct
PDF opening returned 402 and was not retried. The selected factual extract is
hashed separately; it does not claim original PDF capture, visual review, an
exact reporting date, or ownership at historical approval dates. Optional RRA
company evidence now recognizes that whole legal name, scoped to this bundle.
The raw legal manufacturer and dated parent/qualifier interpretation remain
facts. Accton is the named manufacturer namespace; no parent manufacturing,
Edge-Core model alias, SMC expansion, current ownership or deployment is inferred.

Fourteen raw approval rows use that company spelling. Thirteen add model claims
for twelve identities, ten new to the catalog. EN1207F-TX/WOL remains unparsed
but raw searchable; no slash alias was invented. Thirty-six rows still have
unmapped manufacturer names. The audit lists them for further review. Accton's
query now has 176 model assertions; all seven RRA queries have 1,448 assertions.

Current totals: 3,344 identities, 4,388 assertions, 175 sources. Manufacturer
scope has 1,708 target/identity pairs across 1,707 identities; Accton has 401,
backed by 461 records/seven sources. All associations have 3,427 pairs across
3,344 identities; Accton has 1,096. Other target counts are unchanged. The ten
owned labels remain unchanged. See research/rra-accton-company-name.md and
exports rra-accton-company-review.json and accton-manufacturer-additions.csv.

All 285 tests pass. The 74-job manifest now hashes 1,825 files. Rebuild target:
data/rebuilt-with-accton-company.sqlite. New tests verify scoped company mapping,
preserved provenance, withdrawal, composite/unreviewed-name exclusion and
invalid evidence rejection before mutation. Full historical/FCC coverage and
the two exact-label gaps remain unproven; the goal remains active.
''')
