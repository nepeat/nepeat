# label input curation — 2026-10-04

complete; candidate only, awaiting coordinator acceptance and independent review.
the curator independently viewed the original `data/raw/edgecore-7760-label.png`
with `view_image`: the adjacent compliance label prints Model `AS7760-32X` and
Input Voltage Range `100-240~, 50 - 60Hz, 6A Max`.

[candidate input](../../../../data/label-evidence-with-input-marking-2026-10-04.json)
retains the complete original one-source/one-device input and all nine old facts.
it adds `label_observed_input_voltage_range` and the source-location fact
`label_observed_input_voltage_range_reference`, plus an appended coverage scope.
the original coverage is retained verbatim as a prefix. this is only the marking
on the photographed unit; no measured draw, PSU wattage/inventory, universal model
input, suffix-derived specification, seller statement or deployment claim is added.
serial/MAC remain untranscribed.

[proof.json](proof.json) contains original/new input hashes, image hash, exact
additions and checks. [prepare.py](prepare.py) generated the candidate using the
bundled Python standard library, with exclusive creation to preserve snapshots.
it verified all prior source/device assertions, exact fact-list prefix, unchanged
original input/capture, JSON roundtrip, importer field shape and unchanged relative
snapshot resolution beside the candidate. all checks passed. original input hash:
`cb03db8c48a15a857a05dd1803c84e1a3315b472e1d8c6349e38e69216a1d4c9`.
candidate hash: `da337224625fc56240c387d579bbc5e9818edd1524064589384c3f28989c1de9`.
capture hash: `3c85ec72cf8e8c69283c562a87a579a519f2a4a0ae1fa7072bdddac78e8082b5`.

no network, package installation, source mutation, database import, manifest edit
or export refresh occurred. the coordinator owns integration; next step is
independent review of pixels, candidate additions and preservation proof.
