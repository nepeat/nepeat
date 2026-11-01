ACCEPT with notes: SmartNIC/QCT candidate may be promoted. No blocking defect; five low/informational notes below.

# independent candidate review (Claude Sonnet 5.5, 2026-10-05)

Read-only review. Opened every SQLite file with mode=ro. Did not run stage.py or promote.py. I reran
audit.py: it passed and its candidate-verification.json output is identical in content to the prior
run (it was only rewritten; mtime changed, content equal). Reviewed the full integration dir, including
promote.py, which was not on my list but is the next step.

## verdict basis (all checked independently of audit.py)

1. Staged manifest `data/import-manifest-smartnic-qct-2026-10-05.json`
   - Live 121 jobs vs staged 122. The only changed index is 51 (reference-documents), path
     `raw/reference-documents-with-inventec-practical-2026-10-04.json` -> `raw/reference-documents-with-smartnic-qct-2026-10-05.json`.
     One appended job: `curated` / `smartnic-qct-facts-2026-10-04.json`.
   - All 2,255 prior file hashes retained unchanged. 25 new hashed inputs (2,280 total) = merged curated,
     new bundle, 6 siblings, 17 accepted `data/` inputs. I recomputed all 2,280 SHA-256 values: 0 mismatches.
   - `expected`: devices 4091, records 5323, documents 61, document_pages 3716. The sources map gains exactly
     7 ids (3 reference-pdf-*, 4 curated), each records_seen=1. Delta equals acceptance expected_delta.
     No `expected` key was dropped.
   - Live manifest sha 27ff5fc4... is unchanged and equals preflight. Live DB sha 62ab16eb... is unchanged.
     No standard export changed: all 974 preflight hashes match; only the two new
     `smartnic-qct-2026-10-05-*.json` files were added. No -wal/-shm files.
2. Reference bundle
   - 50 entries; entries[:47] are `==` the real old bundle file (not just the integration copy);
     schema_version and collection are unchanged. The 3 new ids are unique.
   - The six new `data/raw/` siblings are bare filenames beside the bundle, byte-identical to the originals under
     `data/raw/smartnic-qct-2026-10-04/{smartnic,t42s}/`. Entry sha256 values match the files.
     page_count 2/2/49 = 53 pages.
   - New documents rows: page text equals the .pages.json text page by page; pdf_sha256 equals the entry.
3. Candidate fidelity
   - Merged curated JSON == the three accepted inputs, with only snapshot rebased (checked by recomputing).
     4 records = 17+17+11+11 = 56 facts. Deltas old->candidate: affiliations +8 (QCT/Inventec brand +
     platform_vendor only), facts +56, sources +7, devices +1, records +4. No identifiers, hyperscaler rows or
     fcc rows were added.
   - English IPU is a new device `Inventec / FPGA IPU C5020X` (id dbeb0ade...) with no identifier link to the
     SmartNIC. Its source-qualified `tpm_table_row_as_printed` = `TPM 75W ` (trailing space kept) and is not in
     `specifications.CATEGORIES`, so it never surfaces as a power statement.
   - Chinese `power_statement_as_printed` is literal `TDP 75W`. The Chinese English-governs limitation is kept
     in `source_language_scope_as_printed` and in the source coverage and bundle scope.
   - No QCT->Quanta alias. The existing `qct-quanta-portfolio-2020` Quanta
     `historical_subsidiary_portfolio` row on T42S is pre-existing and unchanged. No manufacturer role was added
     to any new record. The existing SmartNIC `Inventec Corporation / manufacturer` comes from the old
     `inventec-dongguan-manufacturer-2020` source and is not inherited by the IPU.
   - `C5000X` appears in exactly one fact (SmartNIC `specification_scope`, the no-alias disclaimer), in no
     identifier and in 0 document pages. `lookup C5000X` returns that SmartNIC row with empty
     identity_text_matches, so it is visible as a text hit only.
   - Spot checks against the .pages.json text:
     - IPU p2 `TPM 75W`, model name, memory, `Stratix 10 DX ... golden finger`.
     - CN p2 `TDP 75W`, the memory block, and the 以英文版本为准 limitation.
     - QCT p43 T42S-2U block (per-node qualifiers on CPU/memory/network/expansion; storage options have none).
     - All English facts and 6 of 12 Chinese facts (as_printed names) are literal substrings of page text after
       whitespace folding. Source SHA-256 and QxVDI capture sha (32fcf545...) match the files.
4. Preservation old live -> candidate (my own queries)
   - Every sources column is identical for 243 sources. 47 reference-pdf-* sources change exactly
     {sha256, cache_path, fetched_at}; the audit asserts this and the allowed set is the 47 old bundle ids.
   - Old documents rows are identical. Old multiset rows are preserved in all 52 tables, including
     observed_at and document_pages. Only 2 old devices have changed search text:
     the SmartNIC C5020X and T42S-2U (new facts); the IPU is the only new device.
   - Owned 10/10 (identical results/search_summary), ByteDance 0/2, hyperscaler 22 unchanged,
     manufacturer 2,142/2,141 unchanged, all-target 4,227/4,089 -> 4,228/4,090. Schema (sqlite_master) is
     byte-identical across live, candidate and rebuild. user_version is 29 for all three.
5. Rebuild
   - rebuild.log has 122 job lines + summary (counts 4091/5323/297, verified_files 2280), so it ran from the
     staged manifest. compare-rebuild.log passes with `--database` = candidate (the validation JSON records
     reference_database = candidate).
   - The audit table list is complete: scripts/compare_rebuild.py's list + `inspur_support_entries` + all 9
     lineage TABLES = 52. Every base table in sqlite_master is covered; the 10 uncovered names are only FTS5
     shadow tables of `search_index`/`document_search`, which are compared through the virtual tables (rowid
     included). comparison.json has 52 entries, all only_original = only_rebuilt = 0. Integrity_check and
     foreign_key_check are ok on candidate and rebuild.
6. Code state
   - acceptance.json code_sha256 is stale for 5 devicehunt files, 4 tests and compare_rebuild.py. These come
     from the archive/schema-29 base. lookup.py, refresh_exports.py, reference_documents.py and
     the lookup tests are unchanged. The last code edit (tests/test_campaign.py, 02:35:13) precedes the
     start of the passing 556-test run (ended 02:42:11 after 394 s, so started ~02:35:36). Stage and rebuild ran on that
     same code.

## findings

F1 - low - promote.py review gate is first-line only. `promote.py` accepts the review if
`'BLOCK' not in review.split('\n',3)[0].upper() and 'ACCEPT' in review.upper()`. A BLOCK verdict on any
line after the first would still pass, and any text containing "accept" passes. Scenario: a reviewer who
titles the file "# candidate review" and puts "BLOCK" on line 2 would not stop promotion. This file
puts the verdict alone on line 1 and avoids the word BLOCK there, so the gate reads it correctly. Fix before the
next batch: parse an explicit `VERDICT:` token or require the first line to match
`^(ACCEPT|ACCEPT with notes|BLOCK)\b`.

F2 - low - requirement not asserted by audit.py: "unfiltered full T42S shows new facts, Quanta filter stays
source-local". The audit only checks single vs batch lookup equality for the probes (no content assertion). I
verified manually with `specifications.statement_rows` on read-only connections. T42S-2U power statements for
`--vendor QCT` and unfiltered are old `qct-wssd-models` + new `qct-t42s-qxvdi-solution-specifications-2020`.
`--vendor Quanta` returns none on both old and candidate, because the new sources carry no Quanta target
affiliation. So the requirement holds, but it rests on this review rather than on the audit proof.

F3 - low/informational - the audit's TPM check is not an assertion: `tpm` is computed and recorded
but never asserted. It is still covered, since every fact is asserted equal to the merged curated literal and
I verified the merged literal myself (see 3). Same for the Chinese limitation and absence of C5000X elsewhere.

F4 - low - a stray proposal-time sentence is now permanent catalog text. The `qct-product-portfolio-v14-2023`
bundle scope ends "Not a complete bundle replacement." and is copied into `sources.note` for
`reference-pdf-qct-product-portfolio-v14-2023`. The bundle is now complete, so the phrase is misleading. It does
not change evidence. If it is to be fixed, that needs a new bundle, a new manifest hash and a rebuild, so
I recommend recording it and fixing it at the next bundle revision rather than blocking this promotion.

F5 - informational - fact wording is not always page-literal. The T42S portfolio facts use `2.5-inch`
for the printed `2.5"` and join "Network Controller"/"Expansion Slot" lines with explicit "per node". Six of the Chinese
SmartNIC facts (form_factor, processor, storage, network, host_interface, software) are English paraphrases tagged
"(Chinese reference specification)"; the names without `_as_printed` mark them as such. The source gate accepted
this, and I found no mistranslation (checked against the CN page text). Contract wording "exact literals"
applies only to the `*_as_printed` facts.

F6 - informational/process - after promotion, work remains that promote.py does not do:
- `data/exports/*` (974 files, including the 98 lookup reports) and docs (PROGRESS/README) still describe the
  old live DB; run the accepted refresh_exports workflow before closing the milestone.
- `data/exports/smartnic-qct-2026-10-05-validation.json` names the candidate path as its reference DB. That path
  disappears after promote.py `os.replace`s it, so the proof file should be read with that in mind.
- promote.py replaces the DB and then the manifest as two separate `os.replace` calls; a crash between them
  leaves new DB + old manifest. promotion-intent.json and the backups cover recovery. The `xb` backups and
  the intent file make the script non-rerunnable by design.
- Also note a later C2 live-archive upgrade by the coordinator uses `data/archive/`, not the catalog DB. I
  found no dependency of lineage/evidence on `sources.sha256` of reference sources (lineage keys on archive
  receipts), so the 47 sha256/cache_path changes do not affect `devicehunt evidence`.

## stage.py: silent mutation review

- Every output goes through `fresh()` (open 'xb') or `dump()` into the integration dir. The only
  existing-file write is `dump` of preflight/stage-result/old-complete-bundle in the batch's own dir.
  It never opens `data/devices.sqlite` for write: the candidate is a byte copy (hash-asserted equal to live),
  imports use `Catalog(CANDIDATE)`, and the live DB, live manifest and 974 exports are confirmed unchanged afterwards
  (hashes, above). Sibling copies refuse to overwrite a different existing file.
- It does not verify `acceptance['code_sha256']` (only `files` and live sha); see section 6 - code hashes
  changed legitimately, but nothing records the rebinding. The audit proof does not carry current code hashes either.
  Informational.
- `old-complete-bundle.json` is a re-serialised copy used by the audit for the allowed id set. I compared against the
  real old bundle file and they agree, so there is no defect.

## acceptance.json requirements

| requirement | status |
| --- | --- |
| 120/121 jobs except reference path; append curated; all prior hashes | verified (122 jobs, 2,255 retained) |
| duplicate-sensitive table comparisons, preservation, FTS, integrity, FKs | verified (52 tables, superset of the frozen 43) |
| owned 10/10, ByteDance 0/2, manufacturer 2142/2141, all 4228/4090, hyperscaler 22 | verified |
| old facts/timestamps/roles/pages preserved; IPU distinct; TPM unclassified; CN English-governs | verified |
| QCT roles not global Quanta alias; Quanta filter source-local | verified here (F2), not in audit.py |
| lookup legacy vs batch equality for new models and probes | verified (40 probes, equality only) |
| stop before promotion; root owns docs | respected |
| test_policy (reuse only while hashes unchanged) | code hashes changed (archive base), so a fresh 556-test run replaced it; adequate |
