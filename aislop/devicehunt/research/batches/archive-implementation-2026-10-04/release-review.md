# independent archive release review

reviewer: storage_engine, 2026-10-04. completed review of coordinator
`devicehunt/archive.py` reference registration and frozen release generation.
No coordinator source files or shared data were changed. outcome: accepted after mandatory-reference correction and affected checks.
Full real-collection integration remains coordinator-owned.

## corrected blocker

`receipt_references()` originally retained artifact SHA fields only when already
present in the registry-derived release hash set. That silently loses a core
input/output dependency if its registry record is absent, even with its durable
receipt intact. `freeze()` audit examines indexed locations and therefore does
not necessarily detect a missing registry row. This must reject missing **core**
references, while leaving historical nested annotations distinct:

- artifact-registration: payload.sha256;
- saved-derivation/derivation-run: inputs and outputs SHA values;
- legacy-capture: top-level artifact_sha256 and bundle_sha256;
- reference-document: artifact_sha256/text_sha256 and entry PDF/text SHA values;
- reconstruction-lineage: artifact_sha256/original_sha256;
- capture_result: top-level artifact_sha256/response_sha256 when present.

Historical nested report/header/provenance hashes can denote external information
that was never saved, and code/config hashes identify implementation rather than
captured artifact bytes. Those must not be indiscriminately made required object
dependencies. Durable receipt parent references should remain strict.

## positive inspection

Registration validates the complete pinned finite inventory's bytes, lengths and
hashes before registration. Reference importer prepares the full collection and
compares every entry against inventory order; original malformed PDF response,
reconstruction and saved text extraction are registered distinctly. Unknown
historical acquisition/recipe metadata stays unknown. Deterministic receipt IDs
make repeat registration idempotent without conflating later attempts.

The frozen release embeds every receipt wrapper and explicit dependency arrays
inside a canonical content-hashed body. Accepted lineage import/resolution has no
runtime dependence on mutable archive registry after publication. Supplemental
out-of-data-root evidence is copied into the object store with its original
registration intact. Artifact saved paths remain hash/length validated by the
lineage preparer before exclusive publication of a temporary sibling. Release
publication refuses overwrite and flushes file plus directory metadata. Candidate
extraction creates no hardware facts merely by entering the release.

## recheck and actual validation

Coordinator corrected mandatory artifact references by receipt kind. Missing core
registration/derivation/capture/reference/reconstruction references now fail before
publication; unknown nested historical annotations remain optional. Receipt parent
references are still validated strictly.

`python3 -m unittest tests.test_archive_release tests.test_lineage -q`: **10 passed**
(3 release, 7 lineage). Release tests establish frozen contents remain independent
of later registry additions, exclusive no-overwrite publication, erased registry
artifact failure without publishing, idempotent complete-reference registration,
unknown acquisition metadata remaining unknown and preflight pin corruption
rejecting registration without adding receipts.

Independent temporary field-contract probes rejected missing core saved output,
capture output and reference text hashes; unknown historical/header SHA annotations
were permitted without inventing saved object dependencies.

Review limits: synthetic one-document complete collection exercises registration;
real pinned 47-document collection and special malformed-original reconstruction
need coordinator integration/closure proof. The frozen release content hash binds
full receipt wrappers and explicit refs, so subsequent registry mutations cannot
alter an accepted release. Registry corruption can cause refusal and requires
reindex recovery rather than silent lineage loss.
