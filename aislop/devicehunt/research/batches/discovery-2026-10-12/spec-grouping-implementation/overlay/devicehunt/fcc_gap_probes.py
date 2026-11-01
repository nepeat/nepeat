"""Preserve explicit mirror not-found responses without inventing filing data."""
import datetime as dt
import json
import re
from pathlib import Path

from .documents import dependency
from .fcc_index import BASE, document

SOURCE = 'fcc-gap-probes'
METHOD = 'original HTML from selected public FCCID.co detail URLs'
TITLE = 'Not Found | FCC ID Database'
SCOPE = ('Selected detail-URL probes, independent of the company-index collector. '
         'Original HTML explicitly says not found in this mirror; this does not establish '
         'that the FCC identifier, filing or device does not exist. No company, grantee, '
         'model, application, manufacturer or exhibit claim is inferred. These rows record '
         'unavailable captures only. Existing indexed detail captures remain separate.')


def import_bundle(c, path):
    path = Path(path).resolve()
    data = json.loads(path.read_text())
    if (data.get('schema_version') != 1 or data.get('capture_method') != METHOD
            or not isinstance(data.get('entries'), list) or not data['entries']):
        raise ValueError('Invalid FCC gap-probe capture')
    if not isinstance(data.get('reviewed_on'), str):
        raise ValueError('Missing probe review date')
    dt.date.fromisoformat(data['reviewed_on'])
    seen = set()
    ready = []
    for entry in data['entries']:
        if not isinstance(entry, dict):
            raise ValueError('FCC probe entry must be an object')
        fid = entry.get('fcc_id')
        if (not isinstance(fid, str) or not re.fullmatch(r'[A-Z0-9][A-Z0-9-]{3,31}', fid)
                or fid in seen or entry.get('url') != BASE+'/fcc/'+fid
                or entry.get('path') != fid+'.html'
                or entry.get('state') != 'not_found_in_mirror' or entry.get('html_title') != TITLE):
            raise ValueError('Invalid or duplicate FCC probe identity')
        seen.add(fid)
        root = document(dependency(path.parent, entry).decode('utf-8'))
        if ([n.text() for n in root.walk('title')] != [TITLE] or list(root.walk('h1'))
                or any(n.text() == 'Application Details' for n in root.walk('h2'))
                or any(n.attrs.get('href','').startswith('/api/files/') for n in root.walk('a'))):
            raise ValueError('FCC probe does not unambiguously report not found')
        ready.append(entry)
    # Validate every response and hash before replacing this source's rows.
    c.source(SOURCE, BASE, 'regulatory_mirror', 'Public mirror response; source rights retained', SCOPE, path)
    c.db.execute('DELETE FROM fcc_detail_pages WHERE source_id=?', (SOURCE,))
    for entry in ready:
        fields = dict(probe_state=entry['state'], html_title=entry['html_title'],
                      reviewed_on=data['reviewed_on'], html_sha256=entry['sha256'], html_path=entry['path'])
        c.db.execute('INSERT INTO fcc_detail_pages VALUES (?,?,?,?,?,?,?,?,?)',
            (SOURCE,entry['fcc_id'],'','','',json.dumps(fields,sort_keys=True),entry['url'],
             'unavailable','Explicit not-found HTML from mirror; FCC existence unverified'))
    c.db.execute('UPDATE sources SET records_seen=? WHERE id=?', (len(ready),SOURCE))
    c.db.commit()
    return dict(probes=len(ready), captured_details=0, unavailable=len(ready))
