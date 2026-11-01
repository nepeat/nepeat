# wave5 and QCT — paused after staging

The user requested graceful spin-down. Root stopped new work; no clean rebuild,
query/inventory checker or promotion was launched. Stage session48379 completed
successfully before the stop request. Do not rerun it or overwrite its outputs.

Candidate database: `data/devices-candidate-wave5-2026-10-12.sqlite`, SHA
`0879a77f5d2cdf4edfdce3a88ab03034504c02f99b4645537f43af5db5c32853`.
Candidate manifest: `data/import-manifest-candidate-wave5-2026-10-12.json`, SHA
`86f6179ff8dd490739936d5ab416e39e606380acf547ac009b1dc5b269dca81b`.
Counts:5,157 identities,7,302 records,74,837 facts,2,121 sources,72 PDFs,3,804pages;
152jobs/3,984pins. `stage-report.json` binds actual outputs and unchanged live pair.
The complete61-entry reference bundle retains all59 old entries; QCT auxiliary
parent HTML is explicitly copied and pinned. Root preservation, integrity/FK,
owned10/10 and campaign input/projection gates pass.

Inputs: independently accepted wave5 candidate687da4d… (three sources,40records,
227facts) and QCT9f2f76… (one source/record,eight facts). Final stage helper
SHAec9ebc10d4f7bebab2875467ff7fdc70ce9fbb94ca02d3f56c6f09734007418e
was independently accepted in `../review/wave5-stage-helper-review.md`.
This does not constitute independent acceptance of actual staged outputs.

Authoritative live pair remains accepted wave4: databasefc88dca7… and
manifest43e9fdf8…, as fully pinned in PROGRESS.md and the stage report. All209
production files remain equal to the accepted791-test freeze. Grouping code is
only in a separate private overlay; it has not been applied to production.

Pending after explicit user resume: verify exact current/staged hashes; run the
prepared read-only query and inventory checkers; independently review actual
stage preservation; rebuild into a new path from the candidate manifest; compare
all52 logical tables/indexes and baseline; obtain final independent integration
acceptance and hash-bound root receipt; only then execute reviewed promotion and
export helpers. No wave5 rebuilt DB, comparison proof, final integration review,
promotion receipt/result, recovery pair or dated export has been created. Never
invent completed proofs from preparation scripts. One root integrator owns all
shared writes. If live/code changed meanwhile, reassess rather than blindly
reusing this checkpoint. Earlier preparation-only note is preserved separately.
