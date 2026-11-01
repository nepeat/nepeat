# staged vendor-discovery candidate — awaiting independent acceptance

stage complete; no live promotion or general export refresh. coordinator owns docs.

- candidate: `data/vendor-discovery-candidate-2026-10-04.sqlite`, sha256
  `5b1317f554968fae6c20a4dc0cd2d4d8524ee1c5e60a662675e36a763203a904`.
- clean rebuild: `data/rebuilt-with-vendor-discovery-2026-10-04.sqlite`, sha256
  `37b3a85cb76086583f1e27c2a209ff01e9f60c6f7d5d6b28a21772a65cb37ee5`.
- staged manifest sha256
  `e9d921abac21693c185d8f33eb74333a93bbecf2d88f045e8eb5b479892a1662`;
  117 jobs / 2,178 hashed inputs. manifest now describes staged candidate; live
  remains previous checkpoint until explicitly authorized promotion.
- live remains sha256
  `1e7a877f46b0140c62993de0ebc48b0378773db30c0e889cdfb67244371d7074`.
- counts: 4,083 identities / 5,300 records / 44,728 facts / 281 sources;
  56 archive PDFs / 3,651 pages. additions: 0 identities, 2 records, 38 facts,
  3 sources, 1 PDF / 2 pages. complete reference bundle 45 entries, old 44 unchanged.
- preservation includes exact old records/observed_at/raw_json/roles/facts/raw rows;
  only old 44 reference-source bundle hash/cache/fetched metadata refreshed.
  document FTS preserves old page/text multiplicities and matches every page row;
  device FTS updates only two existing identities and matches full reconstruction.
- existing affected tests: 19 pass (6 reference-documents / 6 rebuild / 7 buying).
  both baseline validations remain owned 9 documents + 1 label and ByteDance 0/2.
  all 43 logical tables equal between candidate/rebuild; supplemental Counters
  preserve duplicate multiplicities, including both FTS contents. main compare
  separately checks deterministic FTS rowids.
- exact unfiltered QCT buying CLI reports retain all 38 accepted context facts;
  `--vendor Quanta` excludes new QCT records. absent/unknown buying categories
  remain explicit; management/environment and other unmapped facts stay in context.
  D51B archived page search verified; readonly CLI/audits leave candidate unchanged.
- all 7 accepted input and 162 code/test hashes rechecked unchanged. original QCT
  captures, bare pages list, historical manifest hashes/jobs and old live preserved.
  before backups use `before-vendor-discovery-2026-10-04` suffix.

proofs beside this handoff: `stage-verification.json`, `candidate-preservation.json`,
`candidate-rebuild-counter-equality.json`, `candidate-practical-proof.json`,
`candidate-buying-reports.json`, affected-test logs, clean-rebuild/compare logs.
main compare artifacts: `data/exports/vendor-discovery-2026-10-04-{comparison,validation}.json`.
reference derivative and metadata: `d51b-embedded-text-wrapper.json` and
`proposed-d51b-reference-entry.json`; complete new bundle and byte-identical sibling
copies are under `data/raw/`. mirror authorship/hosting and garbled embedded text
are explicit; full page 2 specifications were visually reviewed; no OCR.

request coordinator independent candidate acceptance, then separate explicit
promotion/export assignment. no remaining child process: stage session 11909 exited0.
`git diff --check` could not run because host Git requires Xcode license agreement;
no system configuration change attempted. this is not an import/test failure.
