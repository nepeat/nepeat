# production integration contract — proposal, not applied

owner `/root/vendor_bulk_full`; coordinating root is the only possible production
integrator. this batch is ready for independent review. no production module,
schema, catalog, import manifest, reference bundle or export was changed.

## accepted input shape for a future integration

use **inputs-ready.json + discovery-ready.json**. the ready names mean author
validation completed; they do not mean independent acceptance. older inputs and
discovery outputs are superseded prototypes and must not be imported.

- schema version 2; row multiplicities are evidence. stable row key is the
  dataset hash + capture path + original anchor/sitemap ordinal. source line is
  additional HTML location context, not a substitute for the capture hash.
- every input binds a source URL, successful status basis, file and SHA-256.
  preserve the upstream manifest for the 239 preexisting Edge-Core product pages;
  four new index captures bind immutable HTTP receipts. validate every byte before
  starting a transactional replacement for this discovery dataset only.
- source roles are `product_page_link_publisher`, `site_sitemap_publisher` and
  `documentation_index_publisher`. they must never route to manufacturer,
  brand/OEM, deployment, compatibility, current support or per-unit claims.
- store raw URL, decoded original href attribute, raw/normalized anchor label,
  raw/normalized document heading/metadata, language/group heading, ordinal,
  capture path/hash, complete query pairs, `wpdmdl` values, revision strings,
  label/URL disagreement, restriction flags and complete raw row JSON.
- endpoint keys strip query and fragment **for comparison only**. keep every raw
  variant. `wpdmdl` is meaningful; sharing a path cannot establish identical
  documents. a comparison key is not a download instruction or URL alias.
- full-label matches require case-insensitive equality after whitespace
  normalization and retain the printed original. token matches are heuristic
  leads. neither creates a device or an alias; complete suffixes remain distinct.

## concrete schema and local query implementation

`build_discovery_db.py` already materializes the proposed separate discovery
store in **discovery.sqlite**: `datasets`, `captures`, `links`, `link_search`
(FTS5). `links` contains searchable headings, publisher role, raw URL, endpoint,
source location, restriction/disagreement flags and complete raw JSON. captures
have foreign keys; new-path exclusive creation prevents replacement. read-only
search uses SQLite URI `mode=ro` and `query_only`. the private database contains
889 links / 243 captures / one dataset; integrity is `ok`, FK violations zero.

integrate this as a separate discovery command/module if accepted, retaining the
same schema boundaries. do not force unreviewed links into `devices`, `records`,
`affiliations`, `identifiers` or `facts`. a future catalog-backed discovery table
would need its own schema migration, manifest routing, transaction/preservation
checks, full production test suite and clean rebuild; none is claimed here.

from the project root, useful local discovery search:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 research/batches/discovery-2026-10-14/vendor-source-expansion/build_discovery_db.py search --database research/batches/discovery-2026-10-14/vendor-source-expansion/discovery.sqlite '"AS7326-56X"'
```

for a fresh deterministic reconstruction, choose NEW output paths:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 research/batches/discovery-2026-10-14/vendor-source-expansion/offline_parser.py --output /tmp/vendor-discovery-review-2026-10-15.json
PYTHONDONTWRITEBYTECODE=1 python3 research/batches/discovery-2026-10-14/vendor-source-expansion/build_discovery_db.py build --input /tmp/vendor-discovery-review-2026-10-15.json --output /tmp/vendor-discovery-review-2026-10-15.sqlite
```

recheck all existing author artifacts without writing them:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 research/batches/discovery-2026-10-14/vendor-source-expansion/verify_offline.py --check-only
```

comparison uses the live catalog read-only. if the catalog has since changed,
novelty/matches and its recorded hash will change by design; raw extraction must
remain identical. checks.json pins the catalog used for this handoff.

## promotion and acquisition gates

independent review should check the ready config/input hashes, all 889 original
row locations, 615 heading/metadata associations, 40 revision disagreements,
Oracle multilingual sections/shared-family URLs, novelty queries, restriction
quarantine and offline fixture tests. `verify_offline.py` independently recounts
raw links with a separate regex/XML traversal and compares duplicate multiplicity;
this is author automation, not an independent human/agent review.

one Edge-Core row is conservatively quarantined because its URL contains the
known-missing DCS203 R09 revision while its printed heading says R10. other known
AS7710/AS7760/AS7761 denied quick-start/certificate families are explicitly
quarantined by config if encountered (zero matching rows in this finite set).
absence from the catalog is not absence from local historical captures. any
future acquisition needs separate deduplication against historical ledgers,
current robots/pacing, finite targets/budget and inherited denials. this batch
schedules no fetches. do not auto-follow all 548 document/index endpoints.
