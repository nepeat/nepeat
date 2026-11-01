"""Import reviewed public Broadcom server compatibility API captures."""
import json
import re
from pathlib import Path
from urllib.parse import urlencode

from .catalog import digest, target_for
from .oracle_evidence import plain

SOURCE = 'broadcom-servers'
BASE = 'https://compatibilityguide.broadcom.com/compguide/programs/'
PAGE = 'https://compatibilityguide.broadcom.com/search?program=server&persona=live'
QUERIES = ['Accton', 'Edgecore', 'Edge-Core', 'Edge Core', 'Celestica', 'Oracle', 'Amazon Data Services']


def search_url(query):
    return BASE + 'viewSearchResults?' + urlencode(
        {'limit': 100, 'page': 1, 'keyword': query, 'programIdList': 'server'})


def detail_url(uid):
    return BASE + 'viewDetails?' + urlencode({'programId': 'server', 'id': uid, 'filterBy': ''})


def search_rows(data):
    if not isinstance(data, dict) or type(data.get('totalCount')) is not int or not isinstance(data.get('results'), list):
        raise ValueError('Invalid Broadcom search result')
    if data['totalCount'] != len(data['results']):
        raise ValueError('Incomplete Broadcom query; capture all pages before importing')
    rows = {}
    for row in data['results']:
        if (not isinstance(row, dict) or type(row.get('uuid')) is not int or row['uuid'] <= 0
                or row.get('programId') != 'server' or row.get('programName') != 'Systems / Servers'
                or any(not isinstance(row.get(k), str) or not row[k].strip() for k in ['title1', 'title2', 'description2'])
                or row['title1'] != row['description2']
                or not isinstance(row.get('description1'), list)
                or not row['description1']
                or any(not isinstance(v, str) or not v.strip() for v in row['description1'])):
            raise ValueError('Broadcom server identity fields changed')
        if row['uuid'] in rows:
            raise ValueError('Duplicate Broadcom record identifier')
        rows[row['uuid']] = row
    return rows


def detail_fields(data, row):
    if not isinstance(data, dict) or data.get('programId') != 'server' or not isinstance(data.get('details'), list):
        raise ValueError('Invalid Broadcom detail record')
    bios_certification_rows(data)  # Validate all records, even non-target partners.
    fields = {}
    for section in data['details']:
        if section.get('title') != 'Model Details':
            continue
        for sub in section.get('subsections', []):
            if sub.get('type') != 'kv':
                continue
            for field in sub.get('fields', []):
                key, values = field.get('displayKey'), field.get('fieldValues')
                if not isinstance(key, str) or key in fields or not isinstance(values, list):
                    raise ValueError('Ambiguous Broadcom detail field')
                if any(type(v) not in (str, int, float) for v in values):
                    raise ValueError('Non-scalar Broadcom detail field')
                fields[key] = values
    if fields.get('model') != [row['title2']] or fields.get('partner') != [row['title1']]:
        raise ValueError('Broadcom search/detail identity mismatch')
    return fields


def bios_certification_rows(data):
    """Literal rows from one unfiltered BIOS table; None means no table captured.

    Array position preserves row order/duplicates independently of certId. Do not
    zip these rows with the separate release filter or infer a firmware choice.
    """
    tables = []
    for section in data['details']:
        if not isinstance(section, dict) or not isinstance(section.get('subsections', []), list):
            raise ValueError('Invalid Broadcom detail section')
        for sub in section.get('subsections', []):
            if not isinstance(sub, dict):
                raise ValueError('Invalid Broadcom detail subsection')
            if section.get('title') != 'Certification Details' or sub.get('type') != 'table':
                continue
            headers = sub.get('fieldHeaders')
            if not isinstance(headers, list) or any(not isinstance(h, dict) for h in headers):
                raise ValueError('Invalid Broadcom certification table headers')
            bios_headers = [h for h in headers if h.get('displayKey') == 'bios' or h.get('field') == 'certDetails.BIOS']
            if not bios_headers:
                # A row carrying BIOS with a missing/changed header is malformed,
                # not evidence that a BIOS table is absent.
                values = sub.get('fieldValues', [])
                if isinstance(values, list) and any(isinstance(v, dict) and 'bios' in v for v in values):
                    raise ValueError('Broadcom BIOS table header changed')
                continue
            if len(bios_headers) != 1 or bios_headers[0].get('displayKey') != 'bios':
                raise ValueError('Ambiguous Broadcom BIOS table header')
            values = sub.get('fieldValues')
            if not isinstance(values, list):
                raise ValueError('Invalid Broadcom BIOS table rows')
            for value in values:
                if (not isinstance(value, dict) or any(k not in value for k in ('bios', 'certId'))
                        or any(value[k] is not None and type(value[k]) not in (str, int, float)
                               for k in ('bios', 'certId'))):
                    raise ValueError('Invalid Broadcom BIOS certification row')
            tables.append(values)
    if len(tables) > 1:
        raise ValueError('Ambiguous Broadcom BIOS tables')
    if tables:
        # Reject non-JSON numeric literals before c.source replaces any evidence.
        json.dumps(tables[0], ensure_ascii=False, allow_nan=False)
    return tables[0] if tables else None


def validate_bundle(path):
    path = Path(path)
    bundle = json.loads(path.read_text())
    if (bundle.get('schema_version') != 1 or bundle.get('persona') != 'live'
            or bundle.get('queries') != QUERIES or not isinstance(bundle.get('responses'), dict)):
        raise ValueError('Unrecognized Broadcom capture bundle')
    responses = bundle['responses']
    used = set()

    def read(key, url, method):
        entry = responses.get(key, {})
        name = entry.get('path')
        if (not isinstance(name, str) or Path(name).name != name
                or entry.get('url') != url or entry.get('method') != method):
            raise ValueError('Invalid Broadcom response reference: ' + key)
        file = (path.parent / name).resolve()
        if file.parent != path.parent.resolve():
            raise ValueError('Broadcom response escapes bundle directory')
        body = file.read_bytes()
        if digest(body) != entry.get('sha256'):
            raise ValueError('Broadcom response checksum mismatch: ' + key)
        response = json.loads(body)
        if response.get('success') is not True:
            raise ValueError('Broadcom API reported failure')
        used.add(key)
        return response.get('data')

    rows = {}
    for query in QUERIES:
        before = search_rows(read('before:' + query, search_url(query), 'GET'))
        after = search_rows(read('after:' + query, search_url(query), 'GET'))
        if before != after:
            raise ValueError('Broadcom query changed during capture')
        for uid, row in before.items():
            if uid in rows and rows[uid] != row:
                raise ValueError('Conflicting Broadcom query results')
            rows[uid] = row
    records = []
    for uid, row in sorted(rows.items()):
        detail = read('detail:' + str(uid), detail_url(uid), 'POST')
        fields = detail_fields(detail, row)
        records.append((row, detail, fields))
    if used != set(responses):
        raise ValueError('Unexpected Broadcom response entries')
    if not any(target_for(row['title1']) for row, _, _ in records):
        raise ValueError('Broadcom capture contains no exact target partners')
    return records


def import_bundle(c, path):
    records = validate_bundle(path)  # Validate every dependency before replacing prior evidence.
    c.source(SOURCE, PAGE, 'vendor', 'Extracted factual compatibility records; Broadcom source rights retained',
             'Seven target-name public server queries, each checked against its API total and repeated after detail capture. '
             'Exact target partner matches only; this is the live server-guide query scope, not all historical hardware or all programs. '
             'Compatibility entries describe submitted configurations, not factory/OEM, current firmware access, entitlement, '
             'or configuration of a particular used unit. Search/detail model and partner must agree. '
             'No release-to-BIOS relationship is inferred from the unfiltered detail tables.', path)
    count = 0
    for row, detail, fields in records:
        target = target_for(row['title1'])
        if target is None:
            continue
        model = row['title2']
        ids = [('broadcom_server_id', str(row['uuid']))]
        short = re.fullmatch(r'Oracle Server ([a-z0-9-]+)', model, re.I)
        if target == 'Oracle' and short:
            ids.append(('model_search', short[1]))
        facts = [('compatibility_releases', ', '.join(row['description1'])),
                 ('evidence_scope', 'Submitted server compatibility configurations; not physical OEM, unit specifications, firmware access or software entitlement')]
        names = {'cpuSeries': 'compatibility_cpu_series', 'systemType': 'compatibility_system_type',
                 'numberOfSockets': 'compatibility_sockets', 'maxCpuSeriesCoressocket': 'compatibility_cpu_series_max_cores_per_socket',
                 'notes': 'compatibility_notes'}
        for key, name in names.items():
            facts.extend((name, plain(str(v))) for v in fields.get(key, []) if str(v).strip())
        bios_rows = bios_certification_rows(detail)
        if bios_rows is not None:
            facts.append(('compatibility_bios_certification_rows', json.dumps(bios_rows, ensure_ascii=False, allow_nan=False)))
        url = 'https://compatibilityguide.broadcom.com/detail?' + urlencode(
            {'program': 'server', 'productId': row['uuid'], 'persona': 'live'})
        c.add(SOURCE, str(row['uuid']), url, target, model, model, 'server platform',
              {'search': row, 'detail': detail}, [(row['title1'], 'compatibility_partner')], ids, facts)
        count += 1
    c.finish(SOURCE, len(records))
    return {'query_records': len(records), 'imported': count}
