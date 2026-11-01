# independent PSU facts candidate review — 2026-10-12

reviewer: `/root/psu`; candidate author: `/root/handoff_audit`.

verdict: **ACCEPT for candidate integration**. all **42/42 facts** reviewed;
no blocking correction found. this accepts the source-scoped candidate, not
live promotion, system fitment, current certification or unit provenance.

reviewed candidate SHA-256:
`2c55934a430e423c861f04e107e121ff801e9a36e32985daa032308eab4ef756`.
final cosmetic revision independently verified: restoring one removed common
coverage prefix on each of six sources reconstructs the previously reviewed
`c5f72b43ab6ddfbb6aedf238db533c742848e80b3f150cb5423bd0c86cbc53af`
byte hash exactly. all device rows, facts, roles and snapshot bindings are unchanged;
ACCEPT is bound to the final hash above.
reviewed six sources/six records/three exact namespace-model identities:
`Bel Fuse Inc. / PFE1100-12-054NA`,
`Bel Fuse Inc. / PFE1500-12-054NAC`,
`FSP / FSP550-29FM`.
no suffix normalization, aliases, extra models or target mappings were added.

## independent source checks

all six staged snapshots are byte-identical to the immutable originals.
each SHA-256 matches candidate `original_sha256`, `provenance.json` and the
original `psu/fetch-report.json`; byte counts, exact original/final URLs and
capture timestamps agree. source ids/snapshots are unique and every source
owns exactly one candidate record. candidate hash matches `checks.json`.

independently inspected the original saved HTML's JSON-LD with stdlib parsing:
the exact hasVariant product has name/SKU/MPN `PFE1100-12-054NA`,
printed brand `Bel Fuse`, and isVariantOf.name `PFE1100-12-054xA Series`.
no legal-company-name/manufacturer inference is taken from that brand literal.

independently viewed all **nine fact-bearing PDF pages** using existing
local PNG renders, and verified their render hashes/source bindings:

- PFE1100 p. 1, 2, 5:
  `psu-facts-review-renders/bel-pfe1100-series-p{1,2,5}.png`.
- PFE1100 p. 9:
  `psu-visual/bel-pfe1100-page9.png`.
- PFE1500 ordering p. 2 and Nemko p. 1:
  `psu-facts-review-renders/bel-pfe1500-series-p2.png`,
  `psu-facts-review-renders/bel-pfe1500-nemko-p1.png`.
- CSA p. 4: `pdf-runtime/bel-pfe1500-csa-page4.png`.
- FSP p. 1, 2:
  `psu-facts-review-renders/fsp550-29fm-p1.png`,
  `psu-visual/fsp550-29fm-page2.png`.

this is visual inspection of the specifically cited pages, not all-pages
visual review or acceptance of a text extractor's output.

## complete fact review

| record | facts | independent finding |
| --- | --- | --- |
| PFE1100 datasheet | 11 | exact NA caption confirmed; 1100 W series class stays distinct from 1080 W/90 A nominal main output. normal standby 3.3 V/5 A and 5 V/3.3 A, 16.5 W alternatives match the table. general 0…45°C condition, input range, qualitative derating, normal option, unlabeled dimension order, interfaces and revision are accurate. |
| PFE1100 HTML | 2 | exact product/group and printed-brand literals match the original JSON-LD. |
| PFE1500 ordering | 4 | exact NAC C16 row, operating AC/DC ranges and output-connector → input-socket airflow confirmed. upper programmable 3.3/5 V group stays separate from lower S412 12 V group. |
| NAC Nemko | 9 | exact model/certificate/date, separately named applicant/manufacturer/factory, rated input/output alternatives, derating caveat and built-in compliance constraint match the visible page. |
| NAC CSA | 7 | exact NAC regional groups and all four input/temperature table rows match; 12 V main and linear derating remain scoped. Canada/US versus other countries and 45°C versus 60°C are kept separate. |
| FSP | 9 | page-1 exact identity, CRPS, power/input/output ratings, explicit L×W×H dimensions, temperature, interface and revision match. page-2 -20FM heading conflict is preserved and its drawing/airflow excluded. |

PFE1100 current-limitation thresholds are not promoted to continuous ratings,
RA curves are not transferred to NA, and no graph points are digitized.
no rail maxima are summed. FSP output table's 100–240 V input scope is not
confused with its wider operating input specification. rated certificate
input and datasheet operating ranges remain distinct.

## importer and role semantics

the current curated entrypoint is `devicehunt.importers.curated`; there is
no separate `curated.py` or pure curated prepare helper. read its source
and `Catalog.add` without invoking an import. the candidate's source/row
fields, two-element fact/affiliation lists and empty identifier/hyperscaler
sets match that format. integration must still run its normal isolated
candidate import/rebuild and preservation checks.

power/cooling/physical statements use the existing curated category names.
environmental working temperature is retained as an explicit physical
operating condition. model/brand/revision/certificate/interface/conflict
facts remain named literals outside those practical categories; no interface
fact is labelled as software installation or entitlement.

only the Nemko record proposes `manufacturer` and `listed_factory`.
the manufacturer is Bel Fuse Inc.; factory is separately
BPS Asia Pacific Electronics (Shenzhen) Co., Ltd. catalog role classification
returns manufacturer_claim/factory_listing respectively. other affiliations
are documentation_vendor context. the `brand` field is an identity namespace,
not an automatic affiliation/manufacturer assertion in `Catalog.add`.
all proposed companies return no current target mapping.

no current support/certification, chassis compatibility, deployment,
auction-unit configuration/provenance or safe standalone operation follows.
the candidate excludes unresolved Murata suffixes and retains source conflicts.

checks were offline/read-only: no network, installations, imports,
catalog/manifest/archive/export writes or code edits. the live catalog and
manifest hashes still match the candidate's accepted baseline. only this
review file was written. corrected hash-bound extraction/complete collection
and normal integration acceptance remain coordinator responsibilities.
