"""Build a NEW private discovery SQLite index, never the authoritative catalog."""
import argparse
import hashlib
import json
from pathlib import Path
import sqlite3


def create(input_path, output_path):
    body = Path(input_path).read_bytes()
    dataset = json.loads(body)
    dataset_id = hashlib.sha256(body).hexdigest()
    output = Path(output_path)
    # Exclusive creation prevents accidental replacement of any existing DB.
    with output.open('xb'):
        pass
    with sqlite3.connect(output) as db:
        db.execute('PRAGMA foreign_keys=ON')
        db.executescript('''
        CREATE TABLE datasets(id TEXT PRIMARY KEY, schema_version INTEGER NOT NULL, summary_json TEXT NOT NULL);
        CREATE TABLE captures(path TEXT PRIMARY KEY, dataset_id TEXT NOT NULL REFERENCES datasets(id), source TEXT NOT NULL, url TEXT NOT NULL, sha256 TEXT NOT NULL);
        CREATE TABLE links(id INTEGER PRIMARY KEY, dataset_id TEXT NOT NULL REFERENCES datasets(id), capture_path TEXT NOT NULL REFERENCES captures(path), source TEXT NOT NULL, source_role TEXT NOT NULL, source_row INTEGER NOT NULL, source_url TEXT NOT NULL, label TEXT NOT NULL, document_heading TEXT NOT NULL, document_metadata TEXT NOT NULL, section_label TEXT NOT NULL, raw_url TEXT NOT NULL, endpoint TEXT NOT NULL, restricted INTEGER NOT NULL, revision_disagreement INTEGER NOT NULL, evidence_class TEXT NOT NULL CHECK(evidence_class='discovery_only'), raw_json TEXT NOT NULL, UNIQUE(capture_path,source_row));
        CREATE INDEX links_endpoint ON links(endpoint);
        CREATE INDEX links_source ON links(source);
        CREATE VIRTUAL TABLE link_search USING fts5(label,document_heading,document_metadata,section_label,content='links',content_rowid='id');
        ''')
        db.execute('INSERT INTO datasets VALUES(?,?,?)', (dataset_id,dataset['schema_version'],json.dumps(dataset['summary'],sort_keys=True)))
        db.executemany('INSERT INTO captures VALUES(?,?,?,?,?)', [(c['path'],dataset_id,c['source'],c['url'],c['sha256']) for c in dataset['inputs']])
        for row in dataset['rows']:
            if row['evidence_class'] != 'discovery_only' or any(row[k] for k in ['identity_assertion','manufacturer_assertion','deployment_assertion']):
                raise ValueError('Discovery-only boundary violated')
            db.execute('INSERT INTO links(dataset_id,capture_path,source,source_role,source_row,source_url,label,document_heading,document_metadata,section_label,raw_url,endpoint,restricted,revision_disagreement,evidence_class,raw_json) VALUES('+','.join('?'*16)+')',
                       (dataset_id,row['capture_path'],row['source'],row['source_role'],row['row_number'],row['source_url'],row['printed_label'],row.get('document_heading',''),row.get('document_metadata',''),row.get('section_label',''),row['url'],row['endpoint_comparison_key'],bool(row['restrictions']),bool(row.get('revision_label_url_disagreement')),row['evidence_class'],json.dumps(row,sort_keys=True,ensure_ascii=False)))
        db.execute("INSERT INTO link_search(link_search) VALUES('rebuild')")
        if db.execute('PRAGMA integrity_check').fetchone()[0] != 'ok' or db.execute('PRAGMA foreign_key_check').fetchall():
            raise ValueError('Discovery database failed integrity checks')
    return dataset_id


def search(path, query):
    with sqlite3.connect(Path(path).resolve().as_uri()+'?mode=ro',uri=True) as db:
        db.execute('PRAGMA query_only=ON')
        db.row_factory = sqlite3.Row
        return [dict(row) for row in db.execute('SELECT links.id,links.source,links.source_role,links.label,links.document_heading,links.document_metadata,links.section_label,links.raw_url,links.restricted,links.revision_disagreement FROM link_search JOIN links ON links.id=link_search.rowid WHERE link_search MATCH ? ORDER BY links.id', (query,))]


if __name__ == '__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    sub=parser.add_subparsers(dest='command',required=True)
    b=sub.add_parser('build'); b.add_argument('--input',required=True); b.add_argument('--output',required=True)
    s=sub.add_parser('search'); s.add_argument('--database',required=True); s.add_argument('query')
    args=parser.parse_args()
    if args.command=='build':
        print(create(args.input,args.output))
    else:
        print(json.dumps(search(args.database,args.query),indent=2,ensure_ascii=False))
