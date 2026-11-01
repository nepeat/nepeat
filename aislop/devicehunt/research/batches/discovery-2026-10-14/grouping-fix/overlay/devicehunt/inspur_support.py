"""Archive the public Inspur support-name feed without inferring device identities."""
import json
from pathlib import Path

from .documents import dependency

SOURCE = 'inspur-support-models'
INDEX_URL = 'https://www.inspur.com/eportal/ui?pageId=2317460'
API_PATH = '/eportal/ui?portal.url=/portlet/type-tree-front!getAllProductName.portlet&pageId=2317460&moduleId=973866bf97a740b5bca6cf179cc2523c'
API_URL = 'https://www.inspur.com' + API_PATH
SCOPE = ('All rows returned by the publicly referenced support-name feed at capture time. '
         'Includes systems, third-party parts, software, groups and duplicate names. '
         'A support-menu entry is not a device identity, manufacturer, brand, operator, '
         'current support promise or proof of complete historical coverage. Parent IDs '
         'and grouped names are retained without inferred category labels or aliases.')


def prepare(path):
    path = Path(path).resolve()
    bundle = json.loads(path.read_text())
    if (not isinstance(bundle, dict) or bundle.get('schema_version') != 1
            or bundle.get('source_url') != API_URL or bundle.get('index_url') != INDEX_URL):
        raise ValueError('Invalid Inspur support capture metadata')
    index = dependency(path.parent, bundle['index']).decode('utf-8')
    if API_PATH not in index:
        raise ValueError('Inspur support feed is not referenced by the captured page')
    rows = json.loads(dependency(path.parent, bundle['products']))
    count = bundle.get('expected_rows')
    if type(count) is not int or count < 1 or not isinstance(rows, list) or len(rows) != count:
        raise ValueError('Inspur support capture count mismatch or empty response')
    ids = set()
    for row in rows:
        if not isinstance(row, dict):
            raise ValueError('Invalid Inspur support row')
        for key in ('typeTreeId', 'typeTreeParentId', 'typeTreeCname', 'typeTreeLeaf'):
            if not isinstance(row.get(key), str) or not row[key].strip():
                raise ValueError('Invalid Inspur support field: ' + key)
        if row.get('typeTreeLevel') is not None and not isinstance(row['typeTreeLevel'], str):
            raise ValueError('Invalid Inspur support level')
        if row['typeTreeId'] in ids:
            raise ValueError('Duplicate Inspur support identifier')
        ids.add(row['typeTreeId'])
    return path, rows


def import_bundle(c, path):
    path, rows = prepare(path)  # Validate both hashes and all rows before replacement.
    with c.db:
        c.source(SOURCE, API_URL, 'vendor', 'Public support-menu data; upstream rights retained',
                 SCOPE, path)
        c.db.execute('DELETE FROM inspur_support_entries WHERE source_id=?', (SOURCE,))
        c.db.executemany('INSERT INTO inspur_support_entries VALUES (?,?,?,?,?,?,?,?,?)', [
            (SOURCE, n, row['typeTreeId'], row['typeTreeParentId'], row['typeTreeCname'],
             row.get('typeTreeLevel'), row['typeTreeLeaf'], json.dumps(row, ensure_ascii=False), API_URL)
            for n, row in enumerate(rows, 1)])
        c.finish(SOURCE, len(rows))
    return dict(support_rows=len(rows), distinct_names=len({r['typeTreeCname'] for r in rows}),
                catalog_assertions=0)
