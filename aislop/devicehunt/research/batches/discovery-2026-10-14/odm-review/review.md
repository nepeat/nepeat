# independent ODM dataset review and curation

reviewer/curator `/root/network_bulk_full`, 2026-10-15 UTC. **accepted for the
bounded HTML source selection and conservative source-scoped roles below.** no
blocking extraction/identity findings in the 154 proposed pairs / 214 references.
AMD remains raw and is not accepted for identities or company mappings. root must
independently review this newly authored candidate before integration.

only this new `odm-review/` directory was written. source captures, author outputs,
production code, database, manifest and exports were read-only. no network,
installation, delegation, publication or external action occurred. no tool handles
remain. root owns current documentation and integration.

## independent evidence checks

[review_build.py](review_build.py) uses a separate standard-library event-buffer
HTML parser; it does **not** import the author's DOM/extractor or novelty code.
all 20 original acquisition body/header files were SHA256-verified, including the
empty 301 body and AMD PDF. only the five accepted HTML bodies and their headers
were copied here, byte-identically, into [raw](raw/). the historical filename
`redirect-nvidia-ngc-ready.pdf` contains HTML; the empty original
`nvidia-ngc-ready.pdf` is not an evidence document.

all **154 exact whole company/model pairs and all 214 source references** were
checked mechanically against freshly parsed HTML. identity cells, field headers,
raw decoded cell text, whitespace-only display normalization, row/card boundaries,
MiTAC family/field values and byte-fragment hashes agree with the author's output.
the independently parsed complete target-company selection contains exactly the
proposed references; none was dropped or silently added. selected source rows
have no rowspan/colspan or identity-cell superscripts. missing/continued company
cells are never forward-filled; grouped/parenthetical names remain whole.

[review-checks.json](review-checks.json) binds the proposal and discovery hashes:

- proposal `97c0deda94f1a928f64b066a97717e7bbcdaae96fdea58e8e474cff3434991de`
- discovery `5a8bc70209bb58ec2b6be5b1c36c0597495c2b4ef0e1b80275ec1a2b9d37f4a7`

the independently inspected source formats and limits include:

| format | inspected source example / consequence |
| --- | --- |
| NVIDIA five-column reference tables | `table-hgx-ref-2-8-9-400` binds Inventec `P8000G6` to its own tested GPU and east-west/north-south fields; no transfer to a different reference configuration |
| NVIDIA four-column inference/Arm/workstation tables | header-specific meanings retained; `Tested` and `Supported` remain literal labels; nonidentity `*` marker on GH200 text stays printed, without invented interpretation |
| NVIDIA three-column system tables | `table-data-center-servers` retains Wistron `Miramar -ICEMan`; five-column reference table separately prints `Miramar - ICEMan`; neither spelling is repaired or aliased |
| NVIDIA workstations / edge tables | all **27 selected blank cells** remain empty strings in whole-row facts; blank networking/edge qualifiers do not mean unsupported/absent hardware |
| historical NGC four-column server and three-column edge tables | literal `Manufacturer` header retained; role downgraded to source listing association; article's V100/T4 condition on AI Enterprise Compatible statements retained in source scope |
| MiTAC cards with three, four or five fields | exact visible h3 configuration heading has a matching literal image alt; dt/dd order and boundaries independently checked; family heading is context, not an alias |
| MiTAC multi-node cards | `C2810Z5D2E3XD`: CPUs `1 / per node`, Form Factor `2OU`, Storage Bays `14 / per node`; qualifiers and units unaltered |
| MiTAC absent fields | `D50DNP1MHCPAC`, `D50DNP1MHCPLC`, `D50DNP1MHEVAC`, `B8016H68D5` have three fields and no storage-bay field; no blank/inferred value inserted |
| MiTAC long suffixes | `B8056G68CE12HR-2T-TU-HE` and `B8261T85E8HR-2T-N-HE` remain complete configuration identities, separate from family headings |

the source snapshots, row-level byte offsets and fragment hashes remain the
authority for full context. visible HTML cards are the selected MiTAC scope;
hydration-only products or other pages are not claimed covered. AMD's centered
partner names, continuation fragments and multiline models require PDF visual
review before curation; its 142 fragments remain excluded.

## novelty verified independently

[validate_candidate.py](validate_candidate.py) recomputed **all 1,120 discovery
row/fragment novelty statuses**, complete literal-model hits, exact company/model
hits and identifier-hit tuples against a stable read-only schema29 transaction.
all agree with the author: 15 raw row hits for exact printed company/model,
35 same literal model hits in another/unknown company namespace, and 1,070 with
no literal model/identifier hit. all six exact source URLs are absent. this is
literal-string discovery comparison, not a count of new proven devices or a
claim that equivalent URL/brand spellings are absent.

among the accepted 154 pairs, **9** have exact live matches and **145** do not.
the actual Catalog identity algorithm also reuses exactly those 9 identities:
no additional case/brand-normalized collision occurs. QCT/Quanta and other
corporate/brand relationships were not mapped. live file SHA before/after is
`0879a77f5d2cdf4edfdce3a88ab03034504c02f99b4645537f43af5db5c32853`.
[candidate-validation.json](candidate-validation.json) retains every pair result.

## importable candidate and role decisions

[candidate.json](candidate.json) contains **5 sources / 214 records / 154 exact
company-model identities / 2,255 facts / 214 affiliations / 0 identifiers / 0
hyperscaler claims**. its SHA256 is
`f4bb69c73b809f5d35da8e4de58b6b933d3024136f4ed18228f473db1befcc71`.

- NVIDIA current: **137 `program_partner` records**. the source explicitly labels
  the first column `Partner`; the company string is preserved as the identity
  namespace and source-local program association, with no manufacture/brand-role
  or corporate-alias assertion.
- NVIDIA historical NGC: **39 `listing_holder` records**. the source prints
  `Manufacturer`, retained in raw row/header facts. the affiliation here states
  that the named company is the one listed in that row; physical manufacture
  remains unasserted. the article's conditional V100/T4 statement is retained.
- MiTAC: **38 `documentation_vendor` records** from the three MiTAC Computing
  product indexes. exact card models use the MiTAC identity namespace; publisher
  association is established by the page title/host. no manufacturer or factory
  role, actual chassis/node relationship, installed quantities or alias is added.

each original row or card has its own source-local key and whole raw row/card
JSON fact, exact headings/fields, fragment hash and locator. 60 additional
references beyond the 154 pairs become **separate records**, not a merged
evidence statement. tested/supported cells and blanks stay attached to their
original table row; no raw source text is rewritten to reconcile differences.
the existing curated importer can consume this candidate; no production module,
schema or alias routing change is proposed.

## validation and reproduction

**10 independent offline tests pass**, covering deterministic complete candidate
generation, duplicate rows, plus suffixes, blanks, no partner forward-fill,
span/identity-footnote refusal, retained nonidentity markers, malformed tables,
capture corruption, omitted/altered references, per-node/absent card fields and
conservative roles. [test-results.txt](test-results.txt) records the actual run.

`prepare_curated` validates all five snapshots and 214 rows. isolated import and
source replacement both report 214 records; final temporary schema29 catalog is
5 sources / 154 identities / 214 records / 2,255 facts / 214 affiliations, no
identifiers/hyperscaler rows. integrity `ok`, FK violations zero, FTS sample hits4.
this is an empty isolated-catalog proof, **not** combined preservation/rebuild or
production promotion. no full production suite was run for this private artifact.

from project root:

```sh
python3 -B research/batches/discovery-2026-10-14/odm-review/review_build.py
python3 -B -m unittest discover -s research/batches/discovery-2026-10-14/odm-review -p test_review.py -v
PYTHONPATH=. python3 -B research/batches/discovery-2026-10-14/odm-review/validate_candidate.py
```

the generator uses only [selection.json](selection.json) and the copied immutable
captures, reproducing [accepted-rows.json](accepted-rows.json) and candidate JSON.
the optional `--audit-source` path repeats the independent author-output audit;
novelty validation reads the original discovery/novelty artifacts and pinned live
catalog. [SHA256SUMS](SHA256SUMS) covers all non-transient review artifacts.

next finite step: root independently checks this candidate and conservative roles,
then performs any accepted integration exclusively and sequentially, with the
required combined tests, old-row preservation, fresh offline rebuild, exports and
documentation update. no further acquisition or AMD mapping is required for this
accepted HTML milestone.
