# AS8000 / SC6100 r2 — plain hardware titles

owner `/root/bytedance`; **staged title-only successor for future integration**.
Frozen [r1](../as8000-sc6100-facts/handoff.md) is untouched. Its
[accepted fact review](../../review/as8000-sc6100-facts-review.md) still applies;
root independently reviews this finite title refinement.

[Candidate](candidate.json), [builder](build_candidate.py),
[r1/r2 provenance](provenance.json), [checks](checks.json).
Exactly two device-title strings change:

- `Edge-Core Minipack-AS8000 source-scoped document statements` → `Edge-Core Minipack-AS8000`.
- `Celestica SC6100 source-scoped document statements` → `Celestica SC6100`.

No other candidate field changes. Byte comparison also equals r1 after just those
two string replacements. Two sources/two records/26 facts, evidence, identifiers,
roles, exclusions, dates, routing and page boundaries remain identical. All four
PDF/complete-sidecar copies match r1 and original raw captures; full14pages and
six selected visual-page boundaries remain. Every frozen r1 SHA256SUMS entry
still verifies. Existing SC6100 identity title retains first-writer behavior;
the proposed new AS8000 identity gets a plain hardware label.

Reproduce the read-only verification from project root:

```sh
python3 -B research/batches/discovery-2026-10-12/original-vendor-wave4/as8000-sc6100-facts-r2/build_candidate.py --check
```

`--check` passes with no writes. No catalog was opened or prepared, import run,
production/shared state edited or network request made. Only this new r2 directory
contains outputs. Differing existing candidate/copy bytes fail rather than overwrite.

R1 candidate SHA-256: `66fbabc7252652a5748d71e53a45cfd29ecf26d434a77ccedd45e18c2d3aae00`.
R2 candidate: `7c73ce18d7a5812f776b3bde2dbec704a45bed68c7922b3e1e4dad0db2dcaa3e`.
Builder: `24ca383d9b0240f621545d744382fe16bfe54571ede41fd75087ae2f2aec6a25`.
Other local artifact hashes are in `SHA256SUMS`.

Stop condition met. R2 replaces the candidate for future coordinator integration
only; raw sources and accepted r1 evidence remain unchanged. Not imported.
