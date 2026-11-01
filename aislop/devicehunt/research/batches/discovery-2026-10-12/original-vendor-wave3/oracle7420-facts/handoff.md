# Oracle 7420 / 7420 M2 finite candidate

author `/root/psu`; **staged for independent review, not imported**.
candidate SHA-256:
`a82e27385459ddafbe54d510d73959c237bad98b743a7cbd4c97f32032c5b25c`.
only this directory written; no network, import, delegation or shared-state edit.

`candidate.json` follows `devicehunt.importers.curated`; `build_candidate.py`
reproduces it from immutable reviewed findings, exact routing and the two HTML
captures. run `python3 <this-directory>/build_candidate.py --check` for an
offline, no-write byte-reproducibility/source-selector check. this passed.
`provenance.json` binds original/staged snapshots, URLs, capture timestamps,
reviewed findings, routing and candidate hash. `checks.json` records finite
shape/source checks and a timestamped read-only baseline. no import execution
or candidate-catalog preservation result is claimed.

## proposed sources and records

| source | exact Oracle documentation-namespace model | observations |
| --- | --- | ---: |
| `oracle7420-internal-components-2026-10-12` | `Sun ZFS Storage 7420` | 10 |
| same internal source, separate model/key | `Sun ZFS Storage 7420 M2` | 1 |
| `oracle7420-attached-storage-2026-10-12` | `Sun ZFS Storage 7420` | 1 |

totals **two sources / three records / two distinct models / twelve facts**.
only `Oracle / documentation_vendor` affiliation; no manufacturer, brand,
factory, deployment or hyperscaler claim. `brand = Oracle` is a documentation
namespace, consistent with the importer, not an additional company-role claim.
kind `storage controller` is explicitly supported by the source paragraphs.
identifiers and hyperscaler lists are empty; no aliases/equivalences added.

both full printed model names stay distinct from existing Oracle
`ZFS STORAGE 7420` and `S7420`. no registry-code similarity or removal of
Sun creates an identity bridge. routing rationale and exact existing record
ids are in `../oracle7420-routing/handoff.md` / `routing.json`.

## exact reviewed observation set

- plain7420 internal: figure legend; trained-technician FRU service restriction;
  SP3.3V standby/AC-present condition; main12V PDB topology; Vertical PDB/Paddle
  Card connections; power-supply positions0/1; **two fan power boards**;
  six-drive backplane description; original7420 mirrored boot/cache/cluster
  rule; conditional fan-fault/Overtemp indicators.
- M2 only: internal article paragraph21 explicitly names `Sun ZFS Storage
  7420 M2`; mirrored boot drives slots0/1, up to four cache SSDs slots2–5 in
  order, and controller SSD counts **may vary** in a cluster. no other internal
  or attached-storage fact is assigned to M2.
- plain7420 attached: one–six chains, up to six HDD-only or SSD-capable shelves
  per chain, combined in any order. no shelf model/cable/wiring topology inferred.

both drive/cache facts explicitly describe the dated manual's **documented
configuration and cluster rules**, not installed inventory on a liquidated unit.
the original7420 paragraph's controller SSD counts **must match** remains
separate from M2's varying-count rule. no contradiction is reconciled.
legend indices are labels, not quantities: index3 does not mean three supplies,
index10 does not mean ten fans. fan-power-board count is not fan-module count.
power supplies0/1 establish connection positions, not ratings/redundancy.

every fact has original section ID and article-paragraph citation. per-record
`evidence` retains original reviewed observation name, text, selectors and source
hash, so category mapping does not obscure the literal source. power functions
use `power_description`; fan functions/indicators use `cooling_description`;
board/backplane/slot/shelf configuration uses `physical_description`. the
component legend and service restriction remain explicit uncategorized literals;
neither is a software installation or entitlement claim. no additional fact
was created beyond the twelve reviewed observations.

## immutable original captures and checks

| sibling snapshot | bytes | original/staged SHA-256 |
| --- | --- | --- |
| `snapshots/internal-components.html` | 20,339 | `9bbfce09f2b51a8e0bad158e764bf307e53cb5b2c84b258b6c82ff903eb80a83` |
| `snapshots/attached-storage.html` | 5,682 | `f3772553049f0e1a6671cb0579043be928550976129b2ea8e104f1ca3c4e6642` |

source URLs equal the captured Oracle URLs; original completion timestamps are
retained in source metadata and provenance. snapshots are byte-identical copy2
copies, not text substitutes. all twelve selectors/text passages independently
rechecked against original HTML, plus routing/source hashes. exact source ids,
snapshots and source/model keys are unique; no orphan record/source.

baseline observed **2026-10-13T03:50:07.665209+00:00**, readonly connection:
both proposed printed identities absent; original existing namespace identities
present. catalog hash before/after read agrees:
`63d3ac788833473f08c716836b528531a4ff7f0df60f270035071fe36243081e`.
this is a point-in-time observation; concurrent coordinator promotion can change
later state. it is not an integration/preservation acceptance proof.

stop condition met: frozen source-scoped candidate ready for independent review.
no current support, certifications, installed-unit fitment/configuration,
credentials, safe wiring/standalone operation, or current firmware entitlement
is established. sole integrator owns any later candidate import/rebuild,
preservation/baseline checks and promotion; no importer/prepare was invoked here.
