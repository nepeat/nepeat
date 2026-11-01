"""Selected Japanese approval queries from the Lang-ship public mirror."""
import json
import re
from pathlib import Path
from urllib.parse import urlencode

from .documents import dependency
from .fcc_index import Tree

BASE = 'https://giteki.lang-ship.com/'
QUERIES = {'accton': 'Accton', 'edgecore': 'Edgecore', 'edge-core': 'Edge-Core',
           'celestica': 'Celestica', 'oracle': 'Oracle', 'amazon': 'Amazon Data Services'}
NAME_VARIANTS = {'amazon-broad': 'Amazon', 'oracle-ja': 'オラクル', 'celestica-ja': 'セレスティカ',
                 'edgecore-ja': 'エッジコア', 'accton-zh': '智邦', 'accton-ja': 'アクトン'}
COLLECTIONS = {'base': QUERIES, 'name_variants': NAME_VARIANTS}
HEADERS = ['技術基準適合証明等の種類', '番号', '年月日', '氏名又は名称', '特定無線設備の種別', '型式又は名称']
FILTERS = ['techCode','number','date','name','radioEquipmentCode','typeName',
           'elecWave','spuriousRules','bodySar','note','organName','fullText']
PREFIX = 'japan-mic-mirror-'
SCOPE = ('Selected company-name query results from the independent Lang-ship mirror, not a direct '
         'MIC retrieval or complete Japanese approval database. Two captures agree on displayed rows. '
         'The page caps output at 2000 rows; these queries are below that cap, but no independent total '
         'is supplied. The operator describes upstream fullwidth-to-halfwidth normalization. '
         'Name of person/entity is stored as registered_name, not inferred manufacturer or factory. '
         'Grouped model labels, dates, duplicate rows and radio classes remain mirror claims. '
         'No present validity, brand, FCC equivalence, operator or catalog identity is inferred.')


def url(query):
    return BASE+'search.php?'+urlencode({'name': query})


def parse(body, query):
    text = body.decode('utf-8'); root = Tree(text).root
    if [n.text() for n in root.walk('title')] != ['技適検索']:
        raise ValueError('Japanese mirror title changed')
    forms = list(root.walk('form'))
    if len(forms) != 1 or forms[0].attrs.get('method','').upper() != 'GET':
        raise ValueError('Japanese query form missing')
    inputs = [n.attrs for n in forms[0].walk('input') if n.attrs.get('name')]
    values = {n['name']: n.get('value','') for n in inputs}
    if (len(inputs) != len(FILTERS) or set(values) != set(FILTERS)
            or values['name'] != query or any(values[k] for k in FILTERS if k != 'name')):
        raise ValueError('Japanese query filters changed')
    tables = []
    for table in root.walk('table'):
        rows = list(table.walk('tr'))
        if rows and [n.text() for n in rows[0].walk('th')] == HEADERS:
            tables.append((table,rows))
    if (len(tables) != 1 or len(list(tables[0][0].walk('table'))) != 1
            or len(re.findall(r'<table\b',text,re.I)) != len(re.findall(r'</table\s*>',text,re.I))
            or '※最大2000件までの表示です' not in root.text()):
        raise ValueError('Japanese result table missing, truncated or ambiguous')
    result = []
    for tr in tables[0][1][1:]:
        cells = [n for n in tr.children if getattr(n,'tag','') in ('th','td')]
        if len(cells) != 6 or any(n.tag != 'td' for n in cells):
            raise ValueError('Japanese result row width changed')
        values = [n.text() for n in cells]
        anchors = list(cells[1].walk('a'))
        if (not values[1] or query.casefold() not in values[3].casefold()
                or not re.fullmatch(r'[A-Za-z0-9-]+',values[1])
                or len(anchors) != 1 or anchors[0].attrs.get('href') != values[1]):
            raise ValueError('Japanese result identity/query association changed')
        result.append(dict(cells=values,detail_url=BASE+values[1]))
    if len(result) >= 2000:
        raise ValueError('Japanese mirror output reaches the display cap; narrow the query')
    return result


def prepare(path):
    path=Path(path).resolve(); data=json.loads(path.read_text())
    entries=data.get('queries')
    queries=COLLECTIONS.get(data.get('collection','base'))
    if (data.get('schema_version') != 1 or data.get('mirror') != BASE
            or queries is None or not isinstance(entries,list) or len(entries)!=len(queries)):
        raise ValueError('Invalid Japanese mirror bundle')
    ready={}
    for item in entries:
        key=item.get('key')
        if (key not in queries or key in ready or item.get('query')!=queries[key]
                or item.get('url')!=url(queries[key])):
            raise ValueError('Invalid or duplicate Japanese company query')
        before=parse(dependency(path.parent,item['before']),queries[key])
        after=parse(dependency(path.parent,item['after']),queries[key])
        if before!=after or type(item.get('rows')) is not int or len(before)!=item['rows']:
            raise ValueError('Japanese query row count/content changed between captures')
        ready[key]=(item,before)
    return path,ready


def import_bundle(c,path):
    path,ready=prepare(path)
    counts={}
    for key,(item,rows) in ready.items():
        sid=PREFIX+key
        c.source(sid,item['url'],'regulatory_mirror','Lang-ship mirror; original source rights retained',
                 SCOPE+' Query name='+item['query']+'; returned rows='+str(len(rows))+'.',path)
        c.db.execute('DELETE FROM japan_equipment WHERE source_id=?',(sid,))
        for number,row in enumerate(rows,1):
            c.db.execute('INSERT INTO japan_equipment VALUES (?,?,?,?,?,?,?,?,?,?,?)',
                (sid,number,*row['cells'],row['detail_url'],json.dumps(row['cells'],ensure_ascii=False),item['url']))
        c.db.execute('UPDATE sources SET records_seen=? WHERE id=?',(len(rows),sid))
        counts[key]=len(rows)
    c.db.commit()
    return dict(query_rows=counts,rows=sum(counts.values()),model_assertions=0)
