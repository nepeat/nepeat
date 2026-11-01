# concrete integration plan — accepted

**accept the exact `integration-breadth/accepted-inputs.json` for root's exclusive
local staging with the accepted helper.** Reviewer `/root/odm_bulk_full`.

Plan SHA-256:
`d486dba2b9de90c32b3a497015e2b5c103de32a10721f483dc0813c4d2e38627`.
Stage helper SHA-256:
`e795eac0ec21a9315adfc13d0f18d45515ffdd9764a558a127747627ad86e6cc`.

[Independent checks](plan-checks.json), [audit code](review_plan.py),
[exact reviewed plan copy](accepted-plan-reviewed.json). The actual preflight
loop from the accepted helper was executed alone, with no staging-write/import/
backup/rebuild statements and without invoking `stage.main()`.

| exact accepted group | sources | records | facts |
| --- | ---: | ---: | ---: |
| redhat | 9 | 242 | 3,388 |
| bisdn | 1 | 11 | 77 |
| odm | 5 | 214 | 2,255 |
| total | 15 | 467 | 5,720 |

All three candidate/review file hashes match the plan; each named independent
review explicitly accepts its exact candidate hash. The Red Hat review is this
reviewer's prior independent source review; BISDN was independently reviewed by
root, ODM by `/root/vendor_bulk_full`. The plan introduces no unreviewed AMD
fragments, source replacement, aliases or manufacturer/deployment assertions.
BISDN uses its accepted software-reference namespace and whole grouped names;
ODMs retain 214 separate row/card references for 154 company/model pairs.

All 15 source IDs are unique/new versus live, all 467 source/key pairs unique,
and all required raw snapshot hashes agree with `prepare_curated` metadata.
No text-sidecar or unpinned extra dependency is present in these inputs. Planned
names are unique safe slugs, destinations remain under `data/raw`, output paths
are absent, and complete real preflight passes without creating them.

Independent production identity-hash calculation predicts **398 added identities
and nine existing identity reuses**. Expected staged counts:

| item | expected |
| --- | ---: |
| devices | 5,555 |
| records | 7,769 |
| sources | 2,136 |
| facts | 80,557 |
| identifiers | 7,386 |
| affiliations | 11,096 |
| hyperscaler evidence | 22 |
| documents / pages | 72 / 3,804 |

The 456 new affiliations are 281 listing-holder, 137 program-partner and 38
documentation-vendor statements; no manufacturer rows or hyperscaler evidence
are added. Exact make/company distinctions and unusual raw strings remain in the
source records/facts. The nine existing identity reuses introduce no unexpected
namespace collision.

Live DB and manifest hashes remain the exact old pair named in the plan:
`0879a77f5d2cdf4edfdce3a88ab03034504c02f99b4645537f43af5db5c32853`
and `86f6179ff8dd490739936d5ab416e39e606380acf547ac009b1dc5b269dca81b`.
All 209 production/test/dependency files match the accepted freeze. Existing
manifest verifies all 3,984 inputs / 152 jobs and its campaign gate. The complete
61-entry reference-PDF bundle remains the existing unchanged job/hash; no PDF
input or bundle replacement is planned.

No staging/candidate database, import, rebuild, promotion, shared write or network
request occurred in this review. Root may now run the accepted staging helper
within the authorized local batch. Staged-result, fresh offline rebuild/equality,
query-check and final promotion acceptance are still pending and must be reviewed
before promotion. This plan acceptance does not constitute promotion acceptance.
