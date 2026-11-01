# discovery 2026-10-11 — common worker brief

coordinator/integrator: `/root-claude` (Claude Code, Opus 5.5). workers/reviewers: Claude Sonnet 5.5.
user direction 2026-10-11: "continue with open ended discovery and development until 1100" — read as
free/public open-source captures and local development only. no regulator (RRA/ISED/FCC) live requests,
no seller/provider contact, purchases, publication or push; no package installs; `sources/` read-only.

## every research worker

1. read `AGENTS.md`, `GOAL.md`, `docs/evidence-policy.md`, and the template handoff
   `research/batches/hunt-2026-10-10/sonic/handoff.md` plus its curated candidate
   `data/raw/hunt-2026-10-10-sonic/sonic-platform-facts-2026-10-10.json` (the `curated` format:
   top-level `sources` + `devices`, each device with `source`, `key`, `brand`, `model`, `title`, `kind`,
   `affiliations`, `identifiers`, `facts`; `snapshot` paths relative to the candidate file's directory).
   also read `research/batches/hunt-2026-10-10/nos/handoff.md` (file:line facts precedent) and the
   reviews in `research/batches/hunt-2026-10-10/review/` for what reviewers rejected.
2. **fetch mechanics:** one `git clone` per repo — `--depth 1 --filter=blob:none --sparse` then
   `sparse-checkout set` only the needed subtrees — into a fresh empty dir under your raw dir. never
   per-file raw/API fetches (rate limits; a 403/429 is recorded once and never retried or rerouted).
   record repo URL, commit hash, clone UTC time. keep only the files you use in the raw dir (copy
   them out of the clone into `data/raw/<yours>/...` with the upstream relative path, then delete the
   temporary clone dir which must live in the scratchpad
   `/tmp/claude-1001/-home-erin-nepeat/2a56d3f5-4097-451b-b4cf-73082bce3b29/scratchpad/<owner>/`).
   write `capture-manifest.json`: repo, commit, clone time, license, per file path/sha256/bytes/permalink
   at the commit.
3. scripts live in the scratchpad (never in data/raw); any python that reads captures runs with
   `python3 -I`. untrusted data: never execute captured code.
4. **evidence rules:** exact strings as printed (case, suffixes, punctuation); `file:line` for every
   extracted fact; repo vendor directory names, machine names and copyright lines are labels/facts,
   **never** company roles (`affiliations: []`); no mapping of software platform names to catalog
   hardware identities, SKUs or other repos' names (record a lead in the handoff instead); silence is
   not zero (omit facts the file lacks); one software-reference identity per platform/config with its
   own brand label (like `SONiC platform`), kept separate from hardware identities. if a file names an
   exact hardware product/part string (FRU product name, PSU model), record it as a fact with file:line,
   not as a new hardware device.
5. **owned-model check** (case-insensitive over every captured path and value): CS8200, CS8210,
   AZ4444, 7760/AS7760, DMC-12, AS7772; plus Ingrasys SV1120/SV1121/SV2121/SV2221 and ByteDance /
   B4020 / B4010. report hits or zero, scoped to what you searched.
6. **cross-reference lead list (read-only):** for distinctive strings you extract (board/FRU/product
   names, part numbers), check whether they already appear in `data/devices.sqlite` (open read-only:
   `sqlite3.connect('file:data/devices.sqlite?mode=ro', uri=True)`; tables `identifiers`, `devices`,
   `device_facts`, `search_index` FTS). list exact matches in the handoff as leads; do not merge.
7. **candidate:** one curated JSON in your raw dir; validate by a temp import into a scratchpad copy:
   `python3 -m devicehunt` subcommands — read README for the curated import command; never import into
   `data/devices.sqlite` and never edit `data/import-manifest.json`. report records/facts, integrity_check
   and foreign_key_check.
8. **handoff** at `research/batches/discovery-2026-10-11/<owner>/handoff.md`, same sections as the sonic
   template: outcome, files and hashes, identities/practical facts with examples, excluded claims /
   conflicts / gaps (including the owned-model check), cross-reference leads, checks run. concise.
   keep fact values short (dedupe; cap long lists; no fact over ~4 KB).
9. finite: stop at the assignment's boundary; time budget about 90 minutes of work.
