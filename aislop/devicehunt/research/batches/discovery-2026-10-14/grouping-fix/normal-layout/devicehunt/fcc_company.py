"""Reviewed FCC.report company-table rows, not complete authorization records."""
import datetime as dt
import json
import re
from pathlib import Path

from .documents import dependency

SOURCE = 'fcc-company-accton'
URL = 'https://fcc.report/company/Accton-Technology-Corp'
REVISION = '20260803-seo-feeds-ranges-dates-bing-verify'
COMPANY = 'Accton Technology Corp'
METHOD = 'reviewed factual web-tool company-table extract; not original HTML'
AMAZON_URL = 'https://fcc.report/company/amazon-data-services-inc-2'
PROFILES = {
    URL: dict(source=SOURCE, company=COMPANY, frn='0005033246', code='HED', pages=3),
    AMAZON_URL: dict(source='fcc-company-amazon', company='Amazon Data Services, Inc.',
                     frn='0029198181', code='2AVPL', pages=1),
}
SCOPE = (METHOD + '. All rows visible across the three captured company pages, not a '
         'complete FCC history or original HTML archive. Original page download returned '
         'HTTP 403; web-tool captures may have different cache ages. Dates, Model cells, '
         'purposes and frequency strings remain mirror claims; duplicate rows are retained. '
         'Company FRN is page-level metadata, not assigned to each filing. The discovery '
         'index selects the first displayed row per literal FCC ID; all rows are preserved '
         'in fcc_company_rows. Model cells are not verified hardware identities or aliases. '
         'No detailed application IDs, grants, manufacturer, operator or PDF assertions.')


def page_url(page):
    return URL if page == 1 else URL + '?__fccreport_origin_revision=' + REVISION + '&fcc_page=' + str(page)


def validate(path):
    path = Path(path).resolve(); bundle = json.loads(path.read_text())
    url = bundle.get('source_url')
    profile = PROFILES.get(url) if isinstance(url, str) else None
    if (bundle.get('schema_version') != 1 or profile is None
            or not isinstance(bundle.get('pages'), list) or len(bundle['pages']) != profile['pages']):
        raise ValueError('Incomplete or unrecognized reviewed company capture')
    ready = []
    for number, entry in enumerate(bundle['pages'], 1):
        data = json.loads(dependency(path.parent, entry))
        if (data.get('schema_version') != 1 or data.get('capture_method') != METHOD
                or data.get('company') != profile['company'] or data.get('company_frn') != profile['frn']
                or type(data.get('page')) is not int or data['page'] != number
                or data.get('total_pages') != profile['pages']
                or data.get('page_url') != (page_url(number) if url == URL else url)):
            raise ValueError('Company identity, pagination or capture method changed')
        dt.date.fromisoformat(data['reviewed_on'])
        dt.datetime.strptime(data['reported_refresh'], '%Y-%m-%d %H:%M:%S')
        rows = data.get('rows')
        if (not isinstance(rows, list) or not rows or len(rows) > 100
                or (number < profile['pages'] and len(rows) != 100)):
            raise ValueError('Truncated or oversized company page')
        last_line = -1
        for row in rows:
            if not isinstance(row, dict):
                raise ValueError('Invalid company row')
            for key in ('listed_date','fcc_id','model_cell','purpose','frequency'):
                if not isinstance(row.get(key), str) or (key != 'frequency' and not row[key].strip()):
                    raise ValueError('Missing company row field: ' + key)
            dt.date.fromisoformat(row['listed_date'])
            if not re.fullmatch(re.escape(profile['code']) + r'[A-Z0-9-]+', row['fcc_id']):
                raise ValueError('Unexpected FCC ID/grantee')
            if type(row.get('web_text_line')) is not int or row['web_text_line'] <= last_line:
                raise ValueError('Missing or reordered source line')
            last_line = row['web_text_line']
        ready.append(data)
    if type(bundle.get('row_count')) is not int or sum(len(p['rows']) for p in ready) != bundle['row_count']:
        raise ValueError('Company capture row count differs')
    if len({p['reported_refresh'] for p in ready}) != 1:
        raise ValueError('Company pages report different refresh timestamps')
    return ready


def import_bundle(c, path):
    pages = validate(path)
    url = pages[0]['page_url']; profile = PROFILES[url]; sid = profile['source']
    scope = SCOPE
    if url == AMAZON_URL:
        scope = SCOPE.replace('three captured company pages', 'one captured company page').replace(
            'Original page download returned HTTP 403; web-tool captures may have different cache ages.',
            'Original HTML download was not attempted; this is a selected web-tool text capture. '
            'Only the FCCID identity with FRN 0029198181 is included. Separately listed '
            'same-name ULS identities and their licenses are not device records.')
    c.source(sid, url, 'regulatory_mirror', 'Factual mirror fields; source rights retained', scope, path)
    c.db.execute('DELETE FROM fcc_company_rows WHERE source_id=?', (sid,))
    c.db.execute('DELETE FROM fcc_id_index WHERE source_id=?', (sid,))
    count = 0; first = {}
    for page in pages:
        for local_number, row in enumerate(page['rows'], 1):
            count += 1
            c.db.execute('INSERT INTO fcc_company_rows VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)',
                (sid,count,page['page'],local_number,profile['company'],page['company_frn'],row['fcc_id'],
                 row['listed_date'],row['model_cell'],row['purpose'],row['frequency'],
                 page['page_url'],row['web_text_line']))
            first.setdefault(row['fcc_id'], (page, row))
    for fid, (page, row) in first.items():
        c.db.execute('INSERT INTO fcc_id_index VALUES (?,?,?,?,?,?,?,?,?,?)',
            (sid,fid,profile['code'],profile['company'],row['listed_date'],'Unverified Model column: '+row['model_cell'],
             '',row['purpose'],page['page_url'],page['page_url']))
    c.db.execute('UPDATE sources SET records_seen=? WHERE id=?', (count,sid))
    c.db.commit()
    return dict(company_rows=count,distinct_fcc_ids=len(first),catalog_assertions=0)
