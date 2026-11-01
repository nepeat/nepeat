"""Selected public mirror table extracts; kept separate from original HTML indexes."""
import datetime as dt
import json
import re
from pathlib import Path

from .documents import dependency
from .fcc_index import GRANTEES

BASE = 'https://fccid.io'
METHOD = 'reviewed factual web-tool table extract; not original HTML'


def parse_capture(data):
    if not isinstance(data, dict):
        raise ValueError('Reviewed FCC capture must be an object')
    code = data.get('grantee_code')
    if (data.get('schema_version') != 1 or not isinstance(code, str) or code not in GRANTEES
            or data.get('grantee_name') != GRANTEES[code]
            or data.get('source_url') != BASE + '/' + code
            or data.get('capture_method') != METHOD
            or data.get('coverage') != 'selected visible table rows; historical completeness unverified'):
        raise ValueError('Unrecognized reviewed FCC index scope')
    reviewed = data.get('reviewed_on')
    if not isinstance(reviewed, str) or not re.fullmatch(r'\d{4}-\d{2}-\d{2}', reviewed):
        raise ValueError('Reviewed FCC capture date missing')
    dt.date.fromisoformat(reviewed)
    lines = data.get('table_lines')
    if (not isinstance(lines, list) or not lines or len(lines) % 3
            or any(not isinstance(line, str) or not line.strip() or '\n' in line or '\r' in line for line in lines)):
        raise ValueError('Malformed reviewed FCC index table')
    rows = []
    for n in range(0, len(lines), 3):
        fid, description, purpose = lines[n:n+3]
        if not re.fullmatch(re.escape(code) + r'[A-Z0-9-]+', fid):
            raise ValueError('Reviewed FCC ID does not match its grantee')
        match = re.fullmatch(r'(\d{4}-\d{2}-\d{2}) (.+)', description)
        if not match:
            raise ValueError('Reviewed FCC displayed date/description missing')
        date, description = match.groups(); dt.date.fromisoformat(date)
        rows.append(dict(fcc_id=fid, grantee_code=code, grantee_name=GRANTEES[code],
                         listed_date=date, product_description=description,
                         equipment_class='', application_purpose=purpose,
                         source_url=BASE+'/'+fid, page_url=BASE+'/'+code))
    if (type(data.get('reviewed_rows')) is not int or len({r['fcc_id'] for r in rows}) != len(rows)
            or len(rows) != data['reviewed_rows']):
        raise ValueError('Reviewed FCC row count or unique IDs changed')
    return code, rows


def import_bundle(c, path):
    path = Path(path).resolve(); data = json.loads(path.read_text())
    if data.get('schema_version') != 1 or not isinstance(data.get('captures'), list) or not data['captures']:
        raise ValueError('Invalid reviewed FCC index bundle')
    ready = [parse_capture(json.loads(dependency(path.parent, entry))) for entry in data['captures']]
    if len({code for code,_ in ready}) != len(ready):
        raise ValueError('Repeated reviewed FCC grantee capture')
    for code, rows in ready:
        sid = 'fcc-index-reviewed-' + code.lower()
        c.source(sid, BASE+'/'+code, 'regulatory_mirror', 'Selected factual table fields; mirror source rights retained',
                 METHOD + '. Selected company-table rows only; no complete FCC history inferred. '
                 'Displayed dates and product descriptions are mirror claims; the date is not '
                 'independently verified as an application or grant date. Equipment class was '
                 'not in the captured company table and is blank. FCC product-code suffixes are '
                 'not model names. No catalog identities, manufacturer, operator, application '
                 'or exhibit records created. Original HTML mirror captures remain separate.', path)
        c.db.execute('DELETE FROM fcc_id_index WHERE source_id=?', (sid,))
        c.db.executemany('INSERT INTO fcc_id_index VALUES (?,?,?,?,?,?,?,?,?,?)',
                        [(sid,*[r[k] for k in ('fcc_id','grantee_code','grantee_name','listed_date',
                          'product_description','equipment_class','application_purpose','source_url','page_url')]) for r in rows])
        c.db.execute('UPDATE sources SET records_seen=? WHERE id=?', (len(rows),sid))
    c.db.commit()
    return {'sources':len(ready), 'index_rows':sum(len(rows) for _,rows in ready)}
