# independent SD6200 / Wedge facts candidate review

reviewer `/root/psu`; candidate author `/root/handoff_audit`; 2026-10-12.
reviewer collected the raw sources but did not author this curated candidate.

**ACCEPT for candidate integration**; all **27/27 facts** checked, no correction
required. review is bound to frozen candidate SHA-256
`0fbf35eb2c9e35364ca915cdba6c4622ae6cd7ae037e9955376576d38d7d03ab`.
ordinary isolated candidate/rebuild/preservation and complete-collection checks
remain integrator responsibilities; this review is not live promotion.

## bindings, shape and exact identities

read candidate, author handoff/builder/checks/provenance/catalog-context,
accepted raw discovery review/findings, original PDFs and complete sidecars,
saved request ledger and current exact catalog records. all four copied PDF/
sidecar artifacts independently match original bytes, provenance hashes and
sizes. both source URLs/capture timestamps equal their successful request
entries; source `original_sha256` matches the raw PDF.

all snapshot/text-sidecar paths resolve inside this self-contained candidate
directory, and each PDF shares its snapshot directory with its sidecar.
`read_capture` independently validates both bound sidecars: **two plus three
complete, consecutively numbered pages**, exact Poppler method/version/options
equal source extraction metadata. no extraction, render or builder was rerun.
original PDFs remain unchanged; no text replacement or pypdf label.

two unique source ids, two records, one per source; matching source/key/model
bindings, **27 facts**, all with explicit PDF-page pointers. empty identifiers,
no hyperscaler list/claim, and only documentation_vendor affiliations.
curated importer format and `Catalog.add` were inspected without invocation;
existing identity ids are descriptive metadata, while exact namespace/model
strings determine actual identity routing. current readonly queries independently
confirm both anchors and absent proposed source ids:

| source / exact namespace-model | existing device id | facts |
| --- | --- | ---: |
| celestica-sd6200-datasheet-2026-10-12 / Celestica / SD6200 | `d1daa8e8c3ad705b1118` | 11 |
| edgecore-wedge100-32x-r04-2026-10-12 / Edge-Core / Wedge 100-32X | `9287d01bdc56adda4517` | 16 |

spaced **Wedge 100-32X** matches the PDF heading and existing Meta-source record
`d40677e0d94ccff9926bfe95`. joined **WEDGE100-32X** is separately present as
device `2098884f461aca65bec3`, product-index record `74fed6b3c0e8221f2e7b8f77`;
no joined alias, role transfer, new SKU identity or model equivalence is proposed.
`Catalog.add` inserts existing devices with `INSERT OR IGNORE`, so the neutral
new record title does not overwrite the existing identity or Meta evidence.

## complete fact assessment

independently compared all nineteen discovery-observation values to candidate
facts, parsed all six serialized ordering rows, and checked the two additional
context/conflict statements against the original extracted source paragraphs.
also independently re-viewed all four saved renders for this candidate review:
SD6200p1–2, Wedgep2–3. the Wedgep1 AC/12V sentence in the conflict fact matches
the complete saved embedded text; page1 was not visually rendered in this pass.

| fact group | count | independent finding |
| --- | ---: | --- |
| SD6200 heading, drives, ESM configuration/interfaces | 4 | exact identity and alternatives match; 13W drive maximum retains35°C; ports remain per ESM, no installed-drive/configuration inference. |
| SD6200 PSU/input, fans, dimensions, environment, sound, revision/conflict | 7 | 1300W, 2+2(200–240V)/3+1(100–240V), separated100–127/200–240VAC47–63Hz retained; five80mm fans, depth1070mm **w/CMA**, conditions/altitude derating and sound **power**<7.6Bel at23±2°C correct. DS6200 footer versus SD6200 headings remains conflict, not alias. |
| Wedge nineteen-observation subset: power, dimensions, cooling, environment, software, revision | 8 | AC or **-54VDC** supplies, separately positive40–72VDC input and12V option not normalized; typical436W not maximum/PSU rating. W×D×H, weight, fans/airflow and environment correct. software loaded/compatibility facts retain historical document context and unknown publication date. |
| Wedge literal ordering rows | 6 | all full codes/descriptions match visiblep3; classification expressly unresolved. 12V short description does not classify the complete code as a standalone PSU FRU. |
| Wedge power-option conflict and company context | 2 | sourcep1 AC/12V wording preserved besidep2 AC/-54V and separate12V option. generic Accton/Edgecore company prose stays a literal context fact, no new per-model manufacturer/factory affiliation. |

six literal rows are `Wedge100-32X-O-12V-F`, exact US/EU/UK/JP
`Wedge100-32X-O-AC-F-*` codes and `ORSA-1U`; the wildcard here summarizes
the four reviewed full strings only. candidate contains each full string,
not a grouped/invented code. all remain facts, with **zero identifiers or aliases**.
no per-SKU PSU/input/airflow allocation, general suffix decoder or installed
cord/adapter/component conclusion is added.

power/cooling/physical categories retain printed conditions; ESM interfaces and
configuration, revision/conflict, ordering and generic-company facts remain
named literals. software category covers explicit loaded-software statements,
with compatibility claims kept historical and no current-download/entitlement
promise. copyright2016/2024 and document tokens are not auction manufacture
dates; uploaded URL dates are not publication dates.

affiliations are exclusively **Celestica Inc. / documentation_vendor** and
**Edgecore Networks Corporation / documentation_vendor**. existing manufacturer
records remain independent; neither namespace/role nor generic about-company
text automatically supplies a manufacturer, factory or deployment assertion.

## completion and limits

no unsupported weight/airflow/PSU model or wiring facts supplied for SD6200;
no exact Wedge PSU FRU model/electrical connector or current software support
conclusion. no current certification, availability, individual-unit provenance,
installed components or safe standalone operation follows.

current catalog-anchor checks and the author's timestamped baseline are
point-in-time observations: concurrent coordinator promotion can change later
state. sole integrator must recheck against its current accepted database and
manifest. review used saved files and readonly SQLite only; no network, imports,
tests that write databases, builder execution, extra renders, candidate/code or
shared-state edits. only this review file was written. finite27-fact stop met.
