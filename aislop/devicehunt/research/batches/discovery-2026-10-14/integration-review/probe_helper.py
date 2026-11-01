"""Isolated probes of helper functions/SQL; never invokes stage.main()."""
import ast
import hashlib
import json
from pathlib import Path
import shutil
import sqlite3
import tempfile
OUT=Path(__file__).resolve().parent
SRC=OUT.parent/'integration-breadth/stage.py'

def main():
    code=SRC.read_text();(OUT/'stage-reviewed.py').write_text(code)
    tree=ast.parse(code);functions=[n for n in tree.body if isinstance(n,ast.FunctionDef) and n.name in {'sha','clone','checked_path'}]
    ns={'Path':Path,'shutil':shutil,'hashlib':hashlib};exec(compile(ast.Module(body=functions,type_ignores=[]),str(SRC),'exec'),ns)
    with tempfile.TemporaryDirectory(dir=OUT) as tmp:
        p=Path(tmp);src=p/'source.json';dst=p/'destination.json';src.write_text('accepted bytes')
        accepted=ns['sha'](src)
        # Intervening source mutation after caller's acceptance check, before clone.
        src.write_text('unreviewed bytes');ns['clone'](src,dst)
        copied_unaccepted=ns['sha'](dst)!=accepted
        try:ns['checked_path'](p,'../escape')
        except AssertionError:traversal_rejected=True
        else:traversal_rejected=False
        c=sqlite3.connect(':memory:');c.execute('ATTACH DATABASE ? AS old',(':memory:',))
        for schema in ['main','old']:
            c.execute('CREATE TABLE '+schema+'.records (id TEXT, observed_at TEXT, fetched_at TEXT, cache_path TEXT)')
        c.execute('INSERT INTO old.records VALUES (?,?,?,?)',('old-id','accepted-time','accepted-fetch','accepted-path'))
        c.execute('INSERT INTO main.records VALUES (?,?,?,?)',('old-id','CHANGED','CHANGED','CHANGED'))
        columns=[r[1] for r in c.execute('PRAGMA main.table_info("records")') if r[1] not in ('observed_at','fetched_at','cache_path')]
        fields=','.join('"'+col+'"' for col in columns)
        query=f'SELECT {fields},count(*) AS multiplicity FROM {{}}."records" GROUP BY {fields}'
        helper_missing=c.execute('SELECT count(*) FROM ('+query.format('old')+' EXCEPT '+query.format('main')+')').fetchone()[0]
        full_missing=c.execute('SELECT count(*) FROM (SELECT * FROM old.records EXCEPT SELECT * FROM main.records)').fetchone()[0];c.close()
    report={'reviewed_helper_sha256':hashlib.sha256(code.encode()).hexdigest(),'stage_main_invoked':False,
            'shared_paths_written':[],'network_requests':0,'checked_path_rejects_parent_escape':traversal_rejected,
            'clone_copied_changed_source_without_accepted_hash_check':copied_unaccepted,
            'old_row_changed_only_in_excluded_columns':{'helper_missing_rows':helper_missing,'full_missing_rows':full_missing}}
    (OUT/'probes.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
if __name__=='__main__':main()
