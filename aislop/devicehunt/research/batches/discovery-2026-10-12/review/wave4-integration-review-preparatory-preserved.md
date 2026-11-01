# wave4 integration — preparatory independent review

reviewer `/root/psu`, 2026-10-13 UTC. **PENDING — NOT ACCEPTED FOR PROMOTION.** actual stage is accepted separately; final clean rebuild/comparison proofs have not yet been supplied as completed evidence for this review. this pending report must not be bound as a final accepted promotion review.

[actual stage acceptance](wave4-stage-review.md) binds DB `fc88dca7e8b6f23aa8078134be767c22fed902303c2880ee923ad1d55952e3bf` and manifest `43e9fdf88703d979b13eef475410d6f138be90833b1fe67de72bf99abe741562`. independently checked all3970 pins/150jobs,29 new pins,59-document/3724-page complete bundle,392facts/20records,57 allowed old reference-source SHA changes,50-table old-row multiplicities, gates, baseline, literal roles/identifiers/raw objects and existing casefold displays. all209 code/test/dependency files still equal the accepted wave3 freeze. [preparatory helper checker](wave4-integration-preparatory-check.py) and [actual results](wave4-integration-preparatory-check.json) bind inspected helper versions, fixes, query outputs and unchanged pairs.

## helper findings and fixes

both concrete findings were resolved by root before this checkpoint; preserved `.before-review-fixes` copies remain hash-bound in reviewer results.

1. wave4 stage.clone deliberately allows equal preexisting copies to support raw-input retry. promotion inherited that behavior for recovery/pending files. corrected promote.py explicitly requires both backup and both pending paths absent **before its first copy**. staging copy behavior remains unchanged. all four paths were absent at independent check.
2. query checker used `include_source_facts=True`, while the dated six-model export omitted it. corrected refresh_exports.py uses the same option for the dated export. normal curated specification exports retain their default behavior. the exact diff introduces only that dated-query option.

no remaining material helper finding. frozen inspected hashes:

| helper | SHA-256 |
| --- | --- |
| `stage.py` | `988a75c526d66018c086713fe761b604c57653d9dc31e05158ab0c401f866cda` |
| `promote.py` | `f5a984654ebac42887294963a8c9a1dfac79891b9b8197658fe230c622dc818a` |
| `refresh_exports.py` | `e15ee2ea78f24a8eb5c8a2276b435e2ece3cf6853c46079388af943731c27e58` |
| `check_queries.py` | `d272eb808553a635173f8dfdf8503851ab500b7f15dc0f0eec3756176b3aa83c` |

## reviewed promotion/export/query semantics

promotion requires an explicit accepted receipt with exact final review path/hash and22 mandatory named proofs, including independent stage/helper/candidate/code reviews, accepted791-test guarded suite, candidate/rebuilt comparison and validation, frozen code set and stage/query/promotion/export helpers. proof names reject absolute paths, `..` and resolution outside the workspace. all209 current code hashes, exact candidate-derived52 logical table/index keys including both FTS indexes, zero comparison differences/count equality, intended database paths, baseline, candidate pair/live pair hashes and complete manifest inputs must agree. live SQLite journal/WAL/SHM must be absent. pending report bytes cannot satisfy reviewer acceptance even though the root-authored receipt boolean is the operational approval record.

recovery copies and pending pair are fresh, byte-checked, and journal binds old/new/backups/review/suite before two file replacements. two-file publication is not a single transactional pair swap; preserved backups/journal support interrupted-operation recovery. root must keep authority quiescent and verify actual final pair/result after execution. promoter must not execute until final reviewer acceptance and receipt are frozen.

export opens accepted live catalog read-only, creates an exclusive historical directory, preserves seven replaced exports with provenance, refreshes current outputs/catalog HTML and creates a new dated six-model output exclusively. owned10/10 and unchanged DB hash are required. old dated wave3/PSU outputs remain outside the replacement set. roots of current exports and historical copies remain root-owned; this review executed no export helper.

completed query artifact checks all20 new raw records/392facts, literal roles/IDs, no new hyperscaler evidence and existing lowercase `Inspur i24M6`/`Inspur i24LM6` displays. independent read-only reproduction of its six exact-model specification queries agrees: Minipack-AS8000=11, SC6100=11, NF5170M4/NF5180M4/NF5270M4=3each, NF5488M5=0. origins remain explicit curated statement versus source fact. jbb PSU/form-factor source facts retain submitted-configuration scope; zero NF5488M5 categorized rows means its CPU facts are outside the reviewed practical-fact selection, not absent hardware/evidence. buying rows preserve namespaces/source records. future document search outputs remain text hits with provenance, not fitment or identity proof.

## required completed proof follow-up

root must supply the completed new-path clean rebuild and comparator results. final independent review will recheck all209 frozen code hashes and both pairs, all52 logical tables with duplicate multiplicities and both FTS rowids, full baseline equality, integrity/FK, exact comparison/validation paths and completed log/hash bindings. accepted wave3 suite791 result remains applicable only because the complete209-file code set is unchanged; it does not prove the new data rebuild. final receipt should also hash full suite/rebuild/check-query logs, guard runner, final reviewer proof outputs and this report's later **final ACCEPT bytes**, beyond the helper's minimum22 named artifacts.

authority remains wave3 DB `4ae2cd0d6a614d582cf6fc47e6ba0e022521ac2b5e73bb268fcfa0dcb87c03ea` and manifest `9b4d93229e1519770a79f665e02987e9706e74cf86db086599160b905157ffec`; candidate pair unchanged. stage/query completed proofs were inspected. partial/running rebuild or comparator outcomes were intentionally not judged. no promotion-result or export-backup/dated-output path existed at this check.

stop at this bounded preparatory checkpoint and await completed-proof follow-up. only this report and uniquely named reviewer artifacts were written; no network, install, importer, temporary/shared DB write, full suite/rebuild/promotion/export execution, shared code/docs/data/manifest/archive change or delegation.
