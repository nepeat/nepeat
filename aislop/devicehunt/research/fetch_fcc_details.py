"""Cache detail pages from the already reviewed FCC index; sequential, resumable."""
import datetime
import json
import sqlite3
import time
from pathlib import Path
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen

from devicehunt.catalog import digest
from devicehunt.fcc_index import BASE, detail_index_rows

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'data/raw/fcc-details'
OUT.mkdir(parents=True, exist_ok=True)
with sqlite3.connect((ROOT/'data/devices.sqlite').as_uri()+'?mode=ro',uri=True) as db:
    db.row_factory=sqlite3.Row
    rows=detail_index_rows(db)
report={'schema_version':1,'index_scope':str(len(rows))+' entries in reviewed incomplete FCCID.co company index', 'entries':[]}
for n,row in enumerate(rows,1):
    fid=row['fcc_id']; url=row['source_url']
    assert url==BASE+'/fcc/'+fid and '/' not in fid
    path=OUT/(fid+'.html'); result=dict(row,path=path.name)
    if path.exists():
        result.update(status='cached',sha256=digest(path.read_bytes()))
    else:
        try:
            with urlopen(Request(url,headers={'User-Agent':'DeviceHunt/0.1 local public-index research'}),timeout=25) as r:
                if r.geturl()!=url or 'text/html' not in r.headers.get('Content-Type',''):
                    raise ValueError('Unexpected redirect or content type')
                body=r.read()
            path.write_bytes(body)
            result.update(status='downloaded',sha256=digest(body))
        except (HTTPError,URLError,TimeoutError,ValueError) as e:
            result.update(status='unavailable',error=str(e))
        time.sleep(0.5)
    result['checked_at']=datetime.datetime.now(datetime.timezone.utc).isoformat()
    report['entries'].append(result)
    (OUT/'fetch-report.json').write_text(json.dumps(report,indent=2)+'\n')
    print(n,len(rows),fid,result['status'],flush=True)
