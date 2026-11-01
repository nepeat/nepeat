# Full local Egypt NTRA equipment register

The cached [NTRA equipment list](https://www.tra.gov.eg/wp-content/uploads/2026/01/Type-Approved-List-3.pdf)
has 760 pages and 47,302 numbered equipment rows. The full positional extraction
now lives in `ntra_equipment`, separate from the existing target-brand device
catalog importer. It includes all brands, not only the five target companies.

## Verification and scope

- Original PDF SHA-256: `72f63661db38c56d1d78fdf97dfe5f8268d5ada85a538c7d6911abe8965bb54a`.
- Exactly 760 ordered pages, with one five-column table per page.
- The first table's columns are checked against No, Product Description, Brand,
  Model and linked models.
- All 47,302 row numbers occur once, in order, from 1 through 47,302. These are
  document sequence numbers, not approval identifiers or unique-device counts.
- 47,290 rows have readable complete identification fields. Twelve rows contain
  undecodable glyph markers and are flagged `review_required`. Raw cells remain
  searchable; named fields are NULL for these rows. No OCR repair is guessed.
- All 119 exact target-brand rows from the earlier importer are present in the
  full extraction. That existing importer and its validation evidence remain
  unchanged.
- Page 65 was rendered and visually checked against the fifteen AWS rows and
  their separate linked-model column.

This establishes coverage of the cached published PDF, not of every record in
NTRA's underlying system or current approval validity. The URL is dated January
2026, while the printed English header says December 1, 2025 and the Arabic
header indicates January 2026. No effective approval date is inferred.

## AWS-labelled rows

Page 65, sequence numbers 4069–4083, lists:

| Model cell | Linked-model cell |
| --- | --- |
| Hopper26SF | |
| AtlantaB1 | Atlanta15 / Atlanta13 |
| Atlanta13 | |
| Atlanta15 | |
| AtlantaB2 | Atlanta16 / Atlanta14 |
| Atlanta14 | |
| Atlanta16 | |
| Atlanta16SF (belongs to Atlanta 1U series) | |
| Atlanta17 | Atlanta17C |
| Atlanta17C | |
| ATLANTA23SFX | |
| ATLANTA23SF | |
| SCOUT01 | |
| SCOUT02 | |
| CubScout | |

Whitespace inside a cell is normalized for named fields; the original extracted
cell text is retained in `cells_json`. Parenthetical qualifications remain in
the model string. Linked models are not aliases. AWS is the register's brand
field: it is not automatically rewritten to Amazon Data Services or used to
claim a specific unit's manufacturing or deployment history.

## Local use

```sh
python3 -m devicehunt ntra Atlanta --limit -1
python3 -m devicehunt lookup Hopper26SF --format json
python3 -m devicehunt ntra --limit -1 --format csv
python3 -m devicehunt import ntra-register data/raw/ntra-register-bundle.json
```

```sql
SELECT brand, model, linked_models, registry_number, source_url
FROM ntra_equipment WHERE brand = 'AWS';

SELECT page, row_number, cells_json, source_url
FROM ntra_equipment WHERE parse_status = 'review_required';
```

`data/raw/ntra-register-bundle.json` checksums both the original PDF and complete
table extraction. Imports validate dependencies, layout and counts before
replacing the register. Rebuilds need only Python's standard library for this
new register; the older NTRA catalog importer still uses PDF packages.

To reproduce the full extraction with the optional PDF packages installed:

```python
from devicehunt.ntra_register import extract
extract('data/raw/ntra-2026-01.pdf', 'data/raw/ntra-all-tables.json')
```

Review any changed extraction before updating bundle or manifest hashes. The
current extractor is pinned to the reviewed original PDF. A new PDF requires
source/layout review rather than silently reusing old assumptions.

Exports: `data/exports/ntra-register.csv`, `data/exports/ntra-aws.csv`, and the
combined example query `data/exports/lookup-atlanta.json`.
