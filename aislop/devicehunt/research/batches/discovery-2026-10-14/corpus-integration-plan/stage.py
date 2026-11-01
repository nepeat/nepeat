"""Stage the accepted 9-PDF text corpus AFTER breadth promotion; never promote.

Requires a root-authored, externally hash-pinned accepted plan. All destinations
must be new. The nine additions file is never passed to the collection importer.
"""
from __future__ import annotations
import argparse
from contextlib import closing
import copy
import hashlib
import json
from pathlib import Path
import re
import shutil
import sqlite3
import sys

ROOT = Path(__file__).resolve().parents[4]
sys.path.insert(0, str(ROOT))
from devicehunt.catalog import Catalog
from devicehunt import documents, reference_documents, rebuild, review_queue, validation

ACCEPTED_ADDITIONS = 'ac3d89415b6036f0617bc9e6789dddc3a1dce20083e5c52f4998601969229534'
COUNT_TABLES = ('devices','records','facts','sources','documents','document_pages')


def require(condition, message):
    if not condition:
        raise ValueError(message)


def sha(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def checked_path(root, relative):
    require(isinstance(relative, str) and bool(relative), 'path must be a relative string')
    name = Path(relative)
    require(not name.is_absolute() and '..' not in name.parts, 'path must stay within its root')
    path = (root / name).resolve()
    require(path.is_relative_to(root.resolve()) and path != root.resolve(), 'path escapes its root')
    return path


def save_new(path, payload):
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open('x', encoding='utf-8') as stream:
        json.dump(payload, stream, ensure_ascii=False, indent=2)
        stream.write('\n')


def copy_new(source, destination, expected):
    require(sha(source) == expected, 'source changed before copy: ' + str(source))
    destination.parent.mkdir(parents=True, exist_ok=True)
    with source.open('rb') as src, destination.open('xb') as dst:
        shutil.copyfileobj(src, dst)
    require(sha(source) == sha(destination) == expected, 'copy hash mismatch')


def no_sqlite_sidefiles(path):
    require(not any(Path(str(path)+suffix).exists() for suffix in ('-wal','-shm','-journal')),
            'SQLite sidefile present; require quiescent pinned catalog: ' + str(path))


def load_plan(path, expected_hash):
    require(re.fullmatch(r'[a-f0-9]{64}', expected_hash or '') is not None, 'explicit plan SHA256 required')
    require(sha(path) == expected_hash, 'accepted plan hash mismatch')
    plan = json.loads(path.read_text())
    require(plan.get('schema_version') == 1 and plan.get('accepted') is True and
            plan.get('scope') == 'accepted-edgecore-text-corpus-only', 'root-accepted corpus plan required')
    require(plan.get('helper_sha256') == sha(Path(__file__)), 'helper differs from root-reviewed code')
    require(plan['additions']['sha256'] == ACCEPTED_ADDITIONS, 'only the exact nine accepted additions are allowed')
    return plan


def combine_bundles(old, additions):
    require(old.get('schema_version') == 1 and old.get('collection') == reference_documents.COLLECTION,
            'expected complete standard reference collection')
    require(len(old.get('documents', [])) == 61, 'preserve the complete existing 61 entries')
    require(additions.get('schema_version') == 1 and additions.get('artifact_kind') == 'reference-document-additions-only',
            'expected accepted additions-only artifact')
    require(len(additions.get('documents', [])) == 9 and sum(e['page_count'] for e in additions['documents']) == 40,
            'expected exactly nine complete documents / 40 pages')
    combined = copy.deepcopy(old)
    combined['documents'].extend(copy.deepcopy(additions['documents']))
    require(combined['documents'][:61] == old['documents'], 'old entries changed')
    for label, get in [('id',lambda e:e['id']),('url',lambda e:e['url']),
                       ('PDF filename',lambda e:e['pdf']['path']),('PDF SHA256',lambda e:e['pdf']['sha256'])]:
        values = [get(e) for e in combined['documents']]
        require(len(set(values)) == len(values), 'duplicate complete-bundle ' + label)
    all_dependencies = [e[k]['path'] for e in combined['documents'] for k in ('pdf','text')]
    require(len(set(all_dependencies)) == len(all_dependencies), 'colliding sibling dependency names')
    return combined


def plan_manifest(original, old_bundle, new_bundle, data_root, new_inputs):
    result = copy.deepcopy(original)
    jobs = [j for j in result['imports'] if j['source'] == 'reference-documents']
    require(len(jobs) == 1 and jobs[0]['path'] == str(old_bundle.relative_to(data_root)),
            'live manifest must name exactly the pinned complete bundle')
    jobs[0]['path'] = str(new_bundle.relative_to(data_root))
    for path, digest in new_inputs.items():
        key = str(path.relative_to(data_root))
        require(key not in result['files'], 'new manifest input already registered: ' + key)
        result['files'][key] = digest
    return result


def quote(name):
    return '"' + name.replace('"','""') + '"'


def compare_preservation(db, old_reference_ids, old_bundle_hash, new_bundle_hash, new_metadata):
    """All old rows exact; only specified metadata on 61 bundle sources may change.

    Caller attaches the old catalog as old. No blanket timestamp/path exclusions.
    FTS shadows are excluded; device FTS including rowids is compared separately.
    """
    preservation = {}
    tables = [r[0] for r in db.execute("SELECT name FROM old.sqlite_master WHERE type='table'")]
    require(set(tables) == {r[0] for r in db.execute("SELECT name FROM main.sqlite_master WHERE type='table'")},
            'candidate table set changed')
    for table in tables:
        if table == 'sources' or table.startswith(('sqlite_','search_index','document_search')):
            continue
        cols = [r[1] for r in db.execute('PRAGMA old.table_info('+quote(table)+')')]
        require(cols == [r[1] for r in db.execute('PRAGMA main.table_info('+quote(table)+')')], 'schema changed: '+table)
        fields = ','.join(quote(c) for c in cols)
        projected = 'SELECT '+fields+',count(*) AS multiplicity FROM {}.'+quote(table)+' GROUP BY '+fields
        missing = db.execute('SELECT count(*) FROM ('+projected.format('old')+' EXCEPT '+projected.format('main')+')').fetchone()[0]
        require(missing == 0, 'old logical row/multiplicity lost: '+table)
        old_count = db.execute('SELECT count(*) FROM old.'+quote(table)).fetchone()[0]
        new_count = db.execute('SELECT count(*) FROM main.'+quote(table)).fetchone()[0]
        delta = {'documents':9,'document_pages':40}.get(table, 0)
        require(new_count == old_count + delta, 'unexpected row count change: '+table)
        preservation[table] = {'old_rows':old_count,'candidate_rows':new_count,'missing_old_groups':missing}
    source_changes = []
    source_cols = [r[1] for r in db.execute('PRAGMA old.table_info(sources)')]
    require(source_cols == [r[1] for r in db.execute('PRAGMA main.table_info(sources)')], 'source schema changed')
    for record in db.execute('SELECT * FROM old.sources ORDER BY id').fetchall():
        old = dict(zip(source_cols, record))
        candidate = db.execute('SELECT * FROM main.sources WHERE id=?',(old['id'],)).fetchone()
        require(candidate is not None, 'old source removed')
        actual = dict(zip(source_cols, candidate))
        expected = dict(old)
        if old['id'] in old_reference_ids:
            require(old['sha256'] == old_bundle_hash, 'old reference source does not bind old bundle')
            expected.update(sha256=new_bundle_hash, fetched_at=new_metadata.fetched_at, cache_path=new_metadata.cache_path)
            source_changes.append({'id':old['id'],'old_sha256':old_bundle_hash,'new_sha256':new_bundle_hash})
        require(actual == expected, 'unexpected old source metadata/content change: '+old['id'])
    require(len(source_changes) == 61, 'expected source input hash changes for exactly 61 bundle sources')
    require(db.execute('SELECT count(*) FROM main.sources').fetchone()[0] ==
            db.execute('SELECT count(*) FROM old.sources').fetchone()[0]+9, 'unexpected source count delta')
    for prefix in ('main','old'):
        require(db.execute('SELECT count(*) FROM '+prefix+'.search_index').fetchone()[0] ==
                db.execute('SELECT count(*) FROM '+prefix+'.devices').fetchone()[0], 'device FTS count mismatch')
    for left,right in [('old','main'),('main','old')]:
        require(not db.execute('SELECT rowid,* FROM '+left+'.search_index EXCEPT SELECT rowid,* FROM '+right+'.search_index').fetchall(),
                'device FTS content/rowids changed')
    require(db.execute('SELECT count(*) FROM document_search').fetchone()[0] ==
            db.execute('SELECT count(*) FROM document_pages').fetchone()[0], 'document FTS count mismatch')
    require(not db.execute('SELECT document_key,page_number,text FROM document_pages EXCEPT SELECT document_key,page_number,text FROM document_search').fetchall(), 'document pages missing from FTS')
    require(not db.execute('SELECT document_key,page_number,text FROM document_search EXCEPT SELECT document_key,page_number,text FROM document_pages').fetchall(), 'unexpected document FTS text')
    return preservation, source_changes


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--plan',type=Path,required=True)
    parser.add_argument('--plan-sha256',required=True)
    args = parser.parse_args(argv)
    plan_path = args.plan.resolve()
    require(plan_path.is_relative_to(ROOT), 'accepted plan must be in the project')
    plan = load_plan(plan_path,args.plan_sha256)
    data = ROOT/'data'
    live = data/'devices.sqlite'
    live_manifest = data/'import-manifest.json'
    require(set(plan['live_hashes']) == {'devices.sqlite','import-manifest.json'}, 'pin both live files')
    no_sqlite_sidefiles(live)
    before = {p.name:sha(p) for p in (live,live_manifest)}
    require(before == plan['live_hashes'], 'post-breadth live pair changed or not yet promoted')
    bound = {plan_path:args.plan_sha256,Path(__file__).resolve():plan['helper_sha256'],
             live:before[live.name],live_manifest:before[live_manifest.name]}

    def pin(key):
        item = plan[key]
        path = checked_path(ROOT,item['path'])
        require(sha(path) == item['sha256'], 'accepted input hash mismatch: '+key)
        bound[path] = item['sha256']
        return path

    promotion = json.loads(pin('breadth_promotion').read_text())
    require(promotion.get('promoted') is True and promotion.get('new') == before,
            'breadth promotion receipt does not bind the current live pair')
    freeze_path = pin('code_freeze')
    freeze = json.loads(freeze_path.read_text())['files']
    require(isinstance(freeze,dict) and bool(freeze), 'nonempty accepted code freeze required')
    for name,digest in freeze.items():
        path = checked_path(ROOT,name)
        require(sha(path) == digest,'accepted code changed: '+name)
        bound[path] = digest
    old_bundle = pin('old_bundle')
    additions_path = pin('additions')
    pin('review')
    reextraction = json.loads(pin('reextraction').read_text())
    baseline_path = pin('baseline')
    require(reextraction['candidate_sha256'] == ACCEPTED_ADDITIONS and
            reextraction.get('structured_device_facts') == 0, 'review must accept text corpus only')
    old_data = json.loads(old_bundle.read_text())
    additions = json.loads(additions_path.read_text())
    combined = combine_bundles(old_data,additions)
    _,prepared_old = reference_documents.prepare(old_bundle)
    require(len(prepared_old) == 61 and sum(len(pages) for _,pages in prepared_old) == 3730,
            'expected complete 61-entry / 3730-page old collection')
    reviewed_docs = {e['id']:e for e in reextraction['documents']}
    require(set(reviewed_docs) == {e['id'] for e in additions['documents']}, 'review/additions set mismatch')
    for entry in old_data['documents']:
        for field in ('pdf','text'):
            path = checked_path(old_bundle.parent,entry[field]['path'])
            bound[path] = entry[field]['sha256']
    for entry in additions['documents']:
        proof = reviewed_docs[entry['id']]
        require((proof['pdf_sha256'],proof['text_sha256'],proof['independently_reextracted_equal_pages']) ==
                (entry['pdf']['sha256'],entry['text']['sha256'],entry['page_count']), 'reviewed document binding differs')
        pdf = documents.dependency(additions_path.parent,entry['pdf'])
        require(pdf.startswith(b'%PDF-'), 'new input is not a PDF')
        pages,_ = documents.read_capture(documents.dependency(additions_path.parent,entry['text']),entry['pdf']['sha256'])
        require(len(pages) == entry['page_count'], 'new sidecar page mismatch')
        for field in ('pdf','text'):
            bound[checked_path(additions_path.parent,entry[field]['path'])] = entry[field]['sha256']
    # read_manifest binds every existing manifest dependency before any write.
    manifest_root,manifest = rebuild.read_manifest(live_manifest)
    require(manifest_root == data, 'unexpected manifest root')
    for name,digest in manifest['files'].items():
        bound[checked_path(data,name)] = digest
    release = review_queue.gate_inputs(data,manifest)
    paths = {key:checked_path(ROOT,name) for key,name in plan['outputs'].items()}
    require(set(paths) == {'bundle','candidate_database','candidate_manifest','report'}, 'exact new output paths required')
    require(paths['bundle'].parent == old_bundle.parent and old_bundle.parent.is_relative_to(data), 'new complete bundle must be an old-bundle sibling under data')
    require(paths['candidate_database'].parent == paths['candidate_manifest'].parent == data, 'candidate pair must be new siblings in data')
    new_dependencies = [(checked_path(additions_path.parent,e[k]['path']),
                         checked_path(paths['bundle'].parent,e[k]['path']),e[k]['sha256'])
                        for e in additions['documents'] for k in ('pdf','text')]
    destinations = list(paths.values()) + [target for _,target,_ in new_dependencies]
    require(len(set(destinations)) == len(destinations), 'overlapping stage output paths')
    require(all(not p.exists() for p in destinations), 'stage destination exists; choose entirely new paths')
    old_reference_ids = {reference_documents.PREFIX+e['id'] for e in old_data['documents']}
    new_reference_ids = {reference_documents.PREFIX+e['id'] for e in additions['documents']}
    with closing(sqlite3.connect(live.as_uri()+'?mode=ro',uri=True)) as db:
        db.execute('PRAGMA query_only=ON')
        counts_before = {name:db.execute('SELECT count(*) FROM '+name).fetchone()[0] for name in COUNT_TABLES}
        require(counts_before == plan['expected_before'], 'post-breadth counts differ from accepted plan')
        require(counts_before['documents'] == 72 and counts_before['document_pages'] == 3804, 'expected complete pre-corpus document baseline')
        ids = {r[0] for r in db.execute("SELECT id FROM sources WHERE id GLOB 'reference-pdf-*'")}
        require(ids == old_reference_ids and not ids.intersection(new_reference_ids), 'live reference collection differs')
        require(all(db.execute('SELECT count(*) FROM records WHERE source_id=?',(sid,)).fetchone()[0] == 0 for sid in ids), 'document-only sources unexpectedly carry structured records')
        require(validation.run(db,baseline_path)['summary'] == {'total':10,'hits':10,'gaps':0}, 'live owned baseline failed')
        review_queue.gate_projections(db,release)
    require(all(sha(p) == digest for p,digest in bound.items()), 'input changed during preflight')
    no_sqlite_sidefiles(live)
    # First output write occurs here, after all inputs/destinations/gates pass.
    staged = {}
    for original,target,digest in new_dependencies:
        copy_new(original,target,digest)
        staged[target] = digest
    save_new(paths['bundle'],combined)
    staged[paths['bundle']] = sha(paths['bundle'])
    _,prepared = reference_documents.prepare(paths['bundle'])
    require(len(prepared) == 70 and sum(len(pages) for _,pages in prepared) ==
            sum(len(pages) for _,pages in prepared_old)+40, 'complete 70-entry bundle validation failed')
    candidate_manifest = plan_manifest(manifest,old_bundle,paths['bundle'],data,staged)
    review_queue.gate_inputs(data,candidate_manifest)
    with paths['candidate_database'].open('xb'):
        pass
    with closing(sqlite3.connect(live.as_uri()+'?mode=ro',uri=True)) as src, closing(sqlite3.connect(paths['candidate_database'])) as dst:
        src.backup(dst)
    catalog = Catalog(paths['candidate_database'])
    try:
        imported = reference_documents.import_bundle(catalog,paths['bundle'])
    finally:
        catalog.db.close()
    with closing(sqlite3.connect(paths['candidate_database'].as_uri()+'?mode=ro',uri=True)) as db:
        db.execute('PRAGMA query_only=ON')
        db.execute('ATTACH DATABASE ? AS old',(live.as_uri()+'?mode=ro',))
        counts = {name:db.execute('SELECT count(*) FROM '+name).fetchone()[0] for name in COUNT_TABLES}
        expected = {name:count+{'sources':9,'documents':9,'document_pages':40}.get(name,0) for name,count in counts_before.items()}
        require(counts == expected, 'unexpected corpus-only count changes')
        preservation,source_changes = compare_preservation(db,old_reference_ids,sha(old_bundle),sha(paths['bundle']),Catalog.prepare_snapshot(paths['bundle']))
        actual_ids = {r[0] for r in db.execute("SELECT id FROM sources WHERE id GLOB 'reference-pdf-*'")}
        require(actual_ids == old_reference_ids|new_reference_ids, 'reference sources changed beyond accepted additions')
        integrity = [r[0] for r in db.execute('PRAGMA integrity_check')]
        fk = [list(r) for r in db.execute('PRAGMA foreign_key_check')]
        require(integrity == ['ok'] and not fk, 'candidate integrity/FK check failed')
        baseline = validation.run(db,baseline_path)['summary']
        require(baseline == {'total':10,'hits':10,'gaps':0}, 'candidate owned baseline failed')
        review_queue.gate_projections(db,release)
        candidate_manifest['expected'].update({k:counts[k] for k in ('devices','records','documents','document_pages')})
        candidate_manifest['expected']['sources'] = dict(db.execute('SELECT id,records_seen FROM sources'))
    save_new(paths['candidate_manifest'],candidate_manifest)
    rebuild.read_manifest(paths['candidate_manifest'])
    no_sqlite_sidefiles(live)
    require(before == {p.name:sha(p) for p in (live,live_manifest)}, 'live pair changed during stage')
    require(all(sha(p) == digest for p,digest in bound.items()), 'accepted input/review/plan/code changed during stage')
    require(all(sha(p) == digest for p,digest in staged.items()), 'staged input changed')
    report = {'staged':True,'promoted':False,'scope':'complete document text corpus only',
              'acceptance_plan_sha256':args.plan_sha256,'helper_sha256':sha(Path(__file__)),
              'counts_before':counts_before,'counts':counts,'deltas':{k:counts[k]-counts_before[k] for k in counts},
              'import_result':imported,'complete_bundle_entries':70,'complete_bundle_pages':3770,'original_entries_unchanged':True,
              'all_old_logical_rows_preserved':preservation,'preservation_blanket_exclusions':[],
              'expected_old_source_metadata_changes':['sha256','fetched_at','cache_path'],
              'expected_complete_bundle_source_hash_changes':source_changes,
              'old_documents_and_page_content_unchanged':True,'device_fts_including_rowids_unchanged':True,
              'document_fts_equals_complete_document_pages':True,'integrity':integrity,'foreign_keys':fk,
              'baseline':baseline,'campaign_gate':'inputs and projections pass before and after import',
              'live_unchanged':before,'accepted_input_hashes':{str(p.relative_to(ROOT)):h for p,h in bound.items()},
              'new_input_hashes':{str(p.relative_to(data)):h for p,h in staged.items()},
              'candidate_database_sha256':sha(paths['candidate_database']),
              'candidate_manifest_sha256':sha(paths['candidate_manifest']),
              'manifest_jobs':len(candidate_manifest['imports']),'manifest_inputs':len(candidate_manifest['files']),
              'frozen_code_files':len(freeze),'fresh_rebuild_and_combined_acceptance_required':True}
    save_new(paths['report'],report)
    print(json.dumps(report,indent=2))


if __name__ == '__main__':
    main()
