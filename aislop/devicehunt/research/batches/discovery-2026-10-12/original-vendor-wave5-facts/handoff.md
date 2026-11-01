# wave5 facts — finite source-scoped candidate

owner `/root/bytedance`, 2026-10-13 UTC / 2026-10-12 PDT. **staged candidate complete;
independent review required; not imported.** Only this directory written. No
temporary/prepared database, import, production/shared-document edit, shared
catalog/manifest/archive/export write, network/install or delegation occurred.

[Candidate](candidate.json), [builder](build_candidate.py), [checker](check_candidate.py),
[checks](checks.json), [mapping](mapping.json), [provenance](provenance.json),
[dated routing](routing.json), [preserved-wave3 routing](wave3-routing.json).
Inputs: [frozen discovery](../original-vendor-wave5/handoff.md),
[accepted review](../review/original-vendor-wave5-review.md),
[accepted boundary recommendation](../review/wave5-candidate-boundary-note.md).

## exact payload

**Three sources /40 records /227 facts /40 distinct namespace-model pairs.**
Five base records carry61 facts derived from all36 reviewed observations;
35 exact ordering-code records carry166 own-row/heading facts. There are35
literal part-number identifiers, no aliases,40 documentation-vendor affiliations
and no manufacturer/factory/deployment/operator/hyperscaler assertions.

| source | records | facts | scope |
| --- | ---: | ---: | --- |
| `oracle-sun-zfs-storage-7420-specifications-2026-10-12` | 1 | 10 | Oracle / Sun ZFS Storage7420 only; no M2 |
| `edgecore-as6700-as6701-qsg-2026-10-12` | 2 | 36 | separate Edge-Core AS6700-32X and AS6701-32X bases; eighteen explicitly joint observations on each |
| `edgecore-as5710-as5712-ds-r09-2026-10-12` | 37 | 181 | separate Edge-Core AS5710/AS5712 bases (7/8 facts), plus35 exact hardware codes |

Record keys/model strings are exact printed base names or full ordering codes;
all source-row titles are plain brand + model. Oracle affiliation is
`Oracle Corporation / documentation_vendor`; Edge-Core affiliation is
`Edgecore Networks Corporation / documentation_vendor`. Generic Accton context
remains explicitly generic source prose and yields no per-model role.

## conditions and exclusions preserved

Oracle retains literal16.36kg/96lbs conflict, 3U/dimensions, CPU-conditioned
100–127VAC versus200–240VAC, frequency/current and power/heat/VA wording.
The1800W “Max power consumption” label retains the qualifying maximum-PSU-rating
note, not actual appliance draw. Heat/VA list evidence also carries the mandatory
note. No M2, registry-code transfer, PSU part/count/redundancy or unit configuration.

AS6700 and AS6701 receive only statements explicitly in their joint guide scope.
Wildcard D*/O patterns remain literal source-local historical software context,
not expanded SKUs. Airflow/module matching, one/two AC modules, configured weight,
operating/storage limits, consumption versus input ratings and installer/service
conditions stay intact; no DC transfer or current NOS entitlement.

AS5710 and AS5712 receive joint family statements only where explicitly headed
for both. The reviewed CPU/memory block is split into exact named left-column
groups: P2041/1.5GHz and its memory/storage on AS5710; Atom C2538/2.4GHz and its
memory/storage on AS5712. The AS5712-only historical Cumulus/PicOS/SnapRoute/
Pluribus observation is absent from AS5710 facts. Configured8.5kg weight, optics-
excluded282W maximum, AC/DC polarity/current conditions, AC efficiency, separate
12V input and operating/storage ranges remain conditional family statements.
R06 PDF Title metadata versus printed R09 footer/landing remains unresolved;
metadata/upload dates do not establish revision history, publication or unit dates.

The35 hardware ordering records are **24 full5710/5712 switch configurations**,
**five PSU/input-unit FRUs** and **six rack/adapter/cable accessories**. They get
only literal row, their own explicit CPU/PSU/airflow/region/description cells and,
for switch rows, the literal Base Model heading. No family size, weight, maximum
consumption or software compatibility is copied by prefix. Blank cells stay absent
in facts and intact in raw rows. No AS prefix or general F/B/AC/DC/O decoder.
`PSU-12V-400-1` is an input-unit FRU with no400W output claim. Rack/cable fields
remain source-listed, condition-specific context; no universal physical fitment.

Three rows remain raw exclusions: service pattern `57xx-54X-SVC-RTF` and the two
embedded FAN versus rendered AN spellings. They are neither hardware identities
nor aliases. Shared raw exclusion metadata does not attribute AS5710 service to
AS5712. Existing Unattributed accessory identities remain separate, including
the old PSU-12V “Power Supply FRU” kind; no prior record is rewritten.

## evidence, copies and routing

Every fact has source/hash, literal quote and physical PDF page/line or original
HTML locator, original reviewed observation/row, exact selected scope and source
conditions. Base facts use reviewed summaries with page citations; CPU groups
use exact model-specific fragments. Ordering-cell values remain exact, including
embedded airflow ligatures, and the full literal row remains with each record.
Raw original summaries/columns remain unmodified evidence, distinct from selected
per-model fact values.

Five byte-identical copies preserve both raw PDFs, their complete2+4-page Poppler
sidecars and Oracle HTML, with original mtimes/UTC captures. All six PDF pages
were visually reviewed during discovery; render/hash links remain to immutable
originals, including the additional page4 reread. No re-extraction/OCR/image change.
All50 frozen discovery artifacts and pinned review/boundary inputs verify.

Read-only routing finds **five existing +35 new identities**, with all three
source IDs absent. Existing bases are Oracle Sun ZFS Storage7420 and the four
Edge-Core AS6700/AS6701/AS5710/AS5712 names. No other-brand match is joined.
Authoritative live advanced to accepted wave4 during this work: saved live and
staged-wave4 observations at05:07UTC both have SHA
`fc88dca7e8b6f23aa8078134be767c22fed902303c2880ee923ad1d55952e3bf`.
An additional explicit read-only preserved-wave3 checkpoint SHA
`4ae2cd0d6a614d582cf6fc47e6ba0e022521ac2b5e73bb268fcfa0dcb87c03ea`
confirms the same5/35 mapping. Each database stayed byte-identical during its
observation; these are dated checkpoints, not permanent baselines. Existing
identity titles/kinds retain first-writer behavior after any later import.

## verification and handoff

From project root, existing runtime:

```sh
python3 -B research/batches/discovery-2026-10-12/original-vendor-wave5-facts/build_candidate.py --check
```

Read-only `--check` passes with staged bytes unchanged: candidate/mapping/
provenance/copies, source pins, current routing and source-ID absence verify.
The checker validates all227 evidence entries, unique keys/fact pairs, all36
observation coverage, explicit CPU/software split, own-row-only code facts,
three exclusions, six complete pages, five exact copies and50 source artifacts.
No database is created or importer invoked. Differing existing outputs fail
rather than overwrite. The three recorded production utility hashes still match
the accepted frozen code.

Candidate SHA-256: `687da4d58027a507cce2f2f4d31225548d3a57fd73ec6a12be5605c4fe5de86b`.
Builder: `ef5ba67ab4f94d8e65b22b6e7a8e90a2cf43dd147ca860306e57811bb6cbf198`.
Checker: `32d094ebbe96f046c72c7afd490d4d06416afdb956dd88a8f072fd335007203b`.
All local artifact hashes are in `SHA256SUMS`.

Stop condition met. Independent candidate review precedes coordinator-only
integration; any successor reference collection must preserve the complete
accepted collection and add these two whole PDFs with sibling dependencies.
No live count change or import acceptance is implied.
