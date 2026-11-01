# Search by part of a model number

The local `search` command now finds interior fragments in device model names
and recorded identity fields. Previously, full-text word-prefix matching found
`7760-32X-A-AC-F` but missed `AS7760-32X` when the query was just `7760`.

```sh
python3 -m devicehunt search 7760
python3 -m devicehunt search '7760 32X' --vendor Edge-Core
python3 -m devicehunt search 7760_32X --vendor Edge-Core --role registry_manufacturer
python3 -m devicehunt search 8200 --vendor Celestica
python3 -m devicehunt search 8210 --vendor Celestica
python3 -m devicehunt search 7772 --vendor Edge-Core
```

Each query term must match. Terms can match the existing full-text word-prefix
index, or—when at least three characters long—occur inside the model or a recorded
identity value. The identity fields are `alias`, `model_search`,
`manufacturer_model`, `part_number`, `fcc_id`, and `ic_id`. Matching fragments
uses Unicode case folding; punctuation and underscores separate query terms.
One- and two-character terms retain word-prefix matching to avoid broad interior
matches. Quoted tokens prevent user punctuation from becoming FTS operators.

Interior matching does not scan arbitrary fact values or ONIE build-target
identifiers. Those remain searchable with the existing word-prefix behavior.
For example, an interior numeric fragment in a compatibility note alone does
not produce this additional identity-field match. Different terms may match
different recorded fields on the same catalog identity, as with full-text search.

Vendor, role and hyperscaler filters apply to the complete combined result.
The vendor and role still have to occur on the same affiliation. Result order
and limits are unchanged; repeated matching records do not duplicate identities.

## Verified local examples

| Query/filter | Previous word-prefix results | Current results |
|---|---:|---:|
| `7760` | 1 | 3 |
| `7760`, Edge-Core | 1 | 2 |
| `7760`, Edge-Core, registry_manufacturer | 0 | 1 |
| `8200`, Celestica | 0 | 3 |
| `8210`, Celestica | 0 | 2 |
| `7772`, Edge-Core | 0 | 2 |

The three `7760` identities are Accton AS7760-32X, Edge-Core AS7760-32X, and
the seller-listed Edge-Core 7760-32X-A-AC-F. They stay separate. A fragment hit
does not establish equivalence, complete ordering-code evidence or deployment.
The owned-hardware validator and marketplace identity matcher are unchanged;
the baseline remains 8/10 corroborated, with the same two gaps.

`lookup` remains a separate literal-substring search across the catalog and all
25 raw datasets. Use `lookup 7760` to include unpromoted register and document
hits. It searches the whole supplied string rather than combining query terms.

For a direct model-field SQL query:

```sql
SELECT brand,model FROM devices
WHERE model LIKE '%7760%'
ORDER BY brand,model;
```

The change requires no migration, import or index rebuild. The database retains
3,522 identities and 4,619 assertions (at that milestone). All 329 tests then passed, including fragment
scope, combined filters, Unicode, short-term behavior, token-operator safety,
limits, suffix preservation, read-only operation and caller transactions.
Thirteen live search cases match the verified offline rebuild, with no previous
result lost in those cases. The slowest measured case in that single audit was
13.006 ms; this is a local measurement, not a performance guarantee.
The database byte hash and all baseline results are unchanged. See
`data/exports/search-fragment-examples.json`, `search-fragment-audit.json`, and
the compact `search-7760.csv` export.
