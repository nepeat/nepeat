"""Official Korean approval detail snapshots; raw company cells stay unsplit."""
import csv
import datetime as dt
import hashlib
import io
import json
import re
from pathlib import Path
from urllib.parse import quote

from .catalog import digest, normalize, target_for
from .documents import dependency
from .fcc_index import document
from . import rra_company_names

FORM = 'https://www.rra.go.kr/ko/license/A_c_search_en.do'
CSV_URL = 'https://www.rra.go.kr/ko/license/csv_down.do'
RESULT_URL = 'https://www.rra.go.kr/ko/license/A_c_search_view_en.do'
DETAIL = 'https://www.rra.go.kr/ko/license/A_b_popup_keyno.do?key_no='
HEADER = ['번호', '상호', '기자재명칭', '모델명', '제조국가', '인증 연월일', '인증 상태', '인증번호']
RESULT_HEADER = ['Applicant', 'Equipment Name', 'Model number', 'Country of Origin', 'Date', 'Status']
EMPTY_MESSAGE = '해당하는 정보가 없습니다.'
FIELDS = [('applicant', '상호'), ('equipment_name', '기기명칭'), ('main_model', '모델명'),
          ('derived_models', '파생모델명'), ('certificate_number', '인증번호'),
          ('manufacturer', '제조자'), ('country_raw', '제조국가'),
          ('approval_date_raw', '인증연월일'), ('other_raw', '기타')]


def company_target(name, reviewed=None):
    # All currently reviewed aliases are ASCII. Do not discard Korean company
    # names around a parenthesized English target and manufacture an exact match.
    if any(char.isalnum() and not char.isascii() for char in name):
        return None
    return target_for(name) or (reviewed or {}).get(normalize(name), {}).get('target')


def composite_claims(root, data, ready):
    """{certificate: reviewed composite entry} for rows whose manufacturer cell an entry names exactly.

    Guard: certificate + exact cell text hash + detail page hash must all match; an entry that
    matches no row of this capture is refused, so drift is loud rather than silently dropped.
    """
    composites = rra_company_names.load_composites(root, data.get('company_evidence'))
    if not composites:
        return {}
    reviewed = rra_company_names.load(root, data.get('company_evidence'))
    claims = {}
    for _, entry, row in ready:
        item = composites.get((row['certificate_number'], digest(row['manufacturer'].encode())))
        if item is None:
            continue
        if (entry['sha256'] != item['detail_sha256'] or entry['url'] != item['detail_url']
                or row['manufacturer'] != item['cell']['text'] or item['query_maker'] != data['query']['maker']
                or company_target(row['manufacturer'], reviewed)):
            raise ValueError('Composite manufacturer cell evidence does not match its detail page')
        raw = item['cell']['raw_html_cell_bytes']
        body = dependency(root, entry)  # checked against the pinned detail hash
        if hashlib.sha256(body[raw['offset']:raw['offset'] + raw['length']]).hexdigest() != raw['sha256']:
            raise ValueError('Composite manufacturer cell raw bytes differ from the saved detail page')
        claims[row['certificate_number']] = item
    if len(claims) != len(composites):
        raise ValueError('Composite manufacturer cell evidence matched no row of this capture')
    return claims


def claim_target(row, reviewed, claims):
    """Whole-cell target, else the target of this row's reviewed composite entry."""
    target = company_target(row['manufacturer'], reviewed)
    claim = claims.get(row['certificate_number'])
    if target or not claim:
        return target
    return next((p['target'] for p in claim['parts'] if p['target']), None)  # first mapped part


def csv_rows(body, allow_empty=False):
    rows = [r for r in csv.reader(io.StringIO(body.decode('cp949')), strict=True) if r]
    if not rows or [c.strip() for c in rows[0]] != HEADER:
        raise ValueError('RRA CSV header changed')
    rows = rows[1:]
    for n, row in enumerate(rows, 1):
        if (len(row) < 8 or row[0] != str(n)
                or not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9/_-]+', row[-1])):
            raise ValueError('RRA CSV numbering or certificate field changed')
    if (not rows and not allow_empty) or len({r[-1] for r in rows}) != len(rows):
        raise ValueError('Empty or duplicate RRA CSV certificates')
    return rows


def validate_empty_result(body):
    """Require the observed official no-results table, not just an empty CSV."""
    root = document(body.decode('cp949'))
    tables = list(root.walk('table'))
    if ([n.text() for n in root.walk('title')] != ['Communications Conformity Assessment Center']
            or [n.text() for n in root.walk('h5')] != ['Search Results'] or len(tables) != 1):
        raise ValueError('Unrecognized RRA empty-result page')
    table = tables[0]
    bodies = list(table.walk('tbody'))
    if [n.text() for n in table.walk('th')] != RESULT_HEADER or len(bodies) != 1:
        raise ValueError('RRA empty-result table headings changed')
    rows = list(bodies[0].walk('tr'))
    if len(rows) != 1:
        raise ValueError('RRA empty-result table contains unexpected rows')
    cells = [n for n in rows[0].children if getattr(n, 'tag', '') in ('th', 'td')]
    if (len(cells) != 1 or cells[0].tag != 'td' or cells[0].attrs.get('colspan') != '7'
            or cells[0].text() != EMPTY_MESSAGE):
        raise ValueError('RRA page does not explicitly report no matching information')


def parse_detail(body, certificate):
    root = document(body.decode('utf-8-sig'))
    candidates = []
    for table in root.walk('table'):
        rows = []
        for row in table.walk('tr'):
            cells = [n.text() for n in row.children if getattr(n, 'tag', '') in ('th', 'td')]
            rows.append(cells)
        if len(rows) == len(FIELDS) and all(len(r) == 2 for r in rows):
            if [r[0] for r in rows] == [label for _, label in FIELDS]:
                candidates.append(dict(zip((key for key, _ in FIELDS), (r[1] for r in rows))))
    if len(candidates) != 1:
        raise ValueError('RRA detail must contain one complete labelled identity table')
    result = candidates[0]
    if result['certificate_number'] != certificate:
        raise ValueError('RRA detail certificate differs from CSV or URL')
    return result


def models(row):
    """Read literal model entries without propagating prefixes or making aliases."""
    # Older approvals put multiple names in the labelled main-model cell.
    # Accept a main list only when every entry is simple; annotations can scope
    # the entire list, so do not salvage parts of an ambiguous main cell.
    main = [value.strip() for value in row['main_model'].split(',')]
    if not all(value.lower() != 'null' and re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9._ -]*', value)
               for value in main):
        main = []
    result = []
    for role, values in [('main', main),
                         ('derived', row['derived_models'].split(','))]:
        for value in values:
            value = value.strip()
            if (value and value.lower() != 'null'
                    and re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9._ -]*', value)
                    and value.casefold() not in {m.casefold() for _, m in result}):
                result.append((role, value))
    return result


def validate(path):
    path = Path(path).resolve(); data = json.loads(path.read_text())
    if (data.get('schema_version') != 1 or data.get('form_url') != FORM
            or data.get('csv_url') != CSV_URL or data.get('complete') is not True):
        raise ValueError('Incomplete or unrecognized official RRA bundle')
    query = data.get('query')
    if (not isinstance(query, dict) or set(query) != {'category', 'fromdate', 'todate', 'firm', 'equip', 'model_no', 'app_no', 'maker', 'nation'}
            or any(not isinstance(v, str) for v in query.values())
            or not query['maker'] or any(query[k] for k in ('category','firm','equip','model_no','app_no','nation'))):
        raise ValueError('Unsupported RRA query scope')
    for name in ('fromdate', 'todate'):
        if not re.fullmatch(r'\d{8}', query[name]):
            raise ValueError('Invalid RRA query dates')
        dt.datetime.strptime(query[name], '%Y%m%d')
    if query['fromdate'] > query['todate']:
        raise ValueError('Reversed RRA query dates')
    rra_company_names.load(path.parent, data.get('company_evidence'))
    form = dependency(path.parent, data['form']).decode('cp949')
    if '/ko/license/csv_down.do' not in form or 'name="maker"' not in form:
        raise ValueError('RRA form/export association changed')
    if 'first_result_page' in data:
        dependency(path.parent, data['first_result_page'])
    before = dependency(path.parent, data['csv_before'])
    after = dependency(path.parent, data['csv_after'])
    rows = csv_rows(before, allow_empty=True)
    comparison = data.get('csv_comparison', 'byte_identical')
    if comparison == 'byte_identical':
        if before != after:
            raise ValueError('RRA CSV changed during capture')
    elif comparison == 'exact_rows_ignore_order':
        later = csv_rows(after, allow_empty=True)
        # Only the generated row number and ordering may differ. Every original
        # cell, including unparsed comma fragments, must agree per certificate.
        if {r[-1]: r[1:] for r in rows} != {r[-1]: r[1:] for r in later}:
            raise ValueError('RRA CSV certificate rows changed during capture')
    else:
        raise ValueError('Unsupported RRA CSV comparison mode')
    if not rows:
        empty = data.get('empty_result')
        if not isinstance(empty, dict) or empty.get('url') != RESULT_URL:
            raise ValueError('Empty RRA CSV requires an original no-results page')
        validate_empty_result(dependency(path.parent, empty))
    elif 'empty_result' in data:
        raise ValueError('Nonempty RRA capture cannot claim an empty result')
    entries = data.get('details')
    if (not isinstance(entries, list) or len(entries) != len(rows)
            or type(data.get('expected_rows')) is not int or data['expected_rows'] != len(rows)):
        raise ValueError('RRA detail set does not cover the CSV')
    ready = []
    for row, entry in zip(rows, entries):
        certificate = row[-1]
        if entry.get('certificate') != certificate or entry.get('url') != DETAIL + quote(certificate, safe=''):
            raise ValueError('RRA detail URL/order differs from CSV')
        parsed = parse_detail(dependency(path.parent, entry), certificate)
        if query['maker'].casefold() not in parsed['manufacturer'].casefold():
            raise ValueError('RRA detail manufacturer does not match the captured query')
        ready.append((row, entry, parsed))
    composite_claims(path.parent, data, ready)  # refuse entries that no longer match their row
    return data, ready


def import_bundle(c, path):
    data, ready = validate(path)
    reviewed = rra_company_names.load(Path(path).resolve().parent, data.get('company_evidence'))
    claims = composite_claims(Path(path).resolve().parent, data, ready)
    sid = 'rra-official-query-' + digest(json.dumps(data['query'], sort_keys=True).encode())[:12]
    c.source(sid, FORM, 'regulatory', 'RRA public factual approval data; source rights retained',
             'Official manufacturer search ' + json.dumps(data['query'], ensure_ascii=False, sort_keys=True) +
             ('. All numbered rows in the two identical CSV exports have checked original detail HTML. '
              if data.get('csv_comparison', 'byte_identical') == 'byte_identical' else
              '. Both CSV exports contain identical certificate-keyed field values, allowing only '
              'generated row numbers/order to differ. Before-export order and both original hashes '
              'are preserved; every row has checked original detail HTML. ') +
             'Completeness is limited to this returned query, not all historical Korean approvals. '
             'CSV intermediate cells contain unquoted commas and remain uninterpreted. Labelled detail '
             'fields supply identities. Composite company cells stay unsplit' + (' except reviewed certificate entries' if claims else '') + '; only exact known target '
             'manufacturer cells create catalog claims. Catalog brand is a manufacturer namespace. '
             'Simple comma-separated main and derived model entries remain distinct literal identities, '
             'not aliases or proven distinct physical products. Main lists containing annotations or '
             'empty entries stay raw in full. No prefix/suffix is propagated between entries. Raw dates, statuses '
             'and countries do not prove present validity, a factory or operator deployment.' +
             rra_company_names.coverage(reviewed) +
             rra_company_names.composite_coverage(rra_company_names.load_composites(
                 Path(path).resolve().parent, data.get('company_evidence'))) +
             (' This query returned zero rows: a separately hashed original result page '
              'explicitly reports no matching information. This is a dated query-level '
              'observation, not company-wide absence or proof of database completeness.'
              if not ready else ''), path)
    c.db.execute('DELETE FROM rra_equipment WHERE source_id=?', (sid,))
    assertions = 0
    claimed_keys = {}
    for n, (cells, entry, row) in enumerate(ready, 1):
        c.db.execute('INSERT INTO rra_equipment VALUES (' + ','.join('?' for _ in range(14)) + ')',
                     (sid, n, *[row[k] for k,_ in FIELDS], json.dumps(cells, ensure_ascii=False), entry['url'], entry['sha256']))
        target = claim_target(row, reviewed, claims)
        if target is None:
            continue
        claim = claims.get(row['certificate_number'])
        company_facts = []
        for field in ('manufacturer', 'applicant'):
            evidence = reviewed.get(normalize(row[field])) if company_target(row[field], reviewed) else None
            if evidence:
                company_facts.extend((field + '_company_' + key, value) for key, value in evidence.items())
        manufacturers = [(row['manufacturer'], 'registry_manufacturer')]
        if claim:
            # Parts replace the whole cell as affiliations; the unlabelled parenthetical is a fact only.
            manufacturers = [(p['text'], p['role']) for p in claim['parts'] if p['role']]
            company_facts += [('rra_manufacturer_as_printed', row['manufacturer']),
                             ('manufacturer_cell_basis', 'reviewed certificate-scoped exact cell; ' + claim['evidence_basis']),
                             ('manufacturer_cell_unlabelled_text', claim['unlabelled_parenthetical']['text']),
                             ('manufacturer_cell_review', json.dumps(
                                 dict(cell_text_sha256=claim['cell']['text_sha256'], detail_sha256=claim['detail_sha256'],
                                      evidence_sha256=claim['source_sha256'], reviewed_on=claim['reviewed_on'],
                                      limits=claim['limits']), ensure_ascii=False, sort_keys=True))]
            company_facts += [('manufacturer_cell_part', json.dumps(
                dict(text=p['text'], role=p['role'], target=p['target'], name_evidence=p['name_evidence'],
                     name_basis=p['name_basis'], company_evidence=reviewed.get(normalize(p['text']))),
                ensure_ascii=False, sort_keys=True)) for p in claim['parts']]
        for role, model in models(row):
            c.add(sid, row['certificate_number'] + ':' + model, entry['url'], target, model,
                  target + ' ' + model, row['equipment_name'], row,
                  manufacturers + [(row['applicant'], 'regulatory_applicant')],
                  [('rra_certificate_id', row['certificate_number'])],
                  [('rra_model_role', role), ('rra_main_model', row['main_model']),
                   ('country_raw', row['country_raw']), ('approval_date_raw', row['approval_date_raw']),
                   ('other_raw', row['other_raw']), ('detail_sha256', entry['sha256']),
                   *company_facts])
            assertions += 1
            if claim:
                claimed_keys.setdefault(row['certificate_number'], []).append(row['certificate_number'] + ':' + model)
    # Catalog.add applies general normalization. Preserve the raw affiliation
    # while enforcing the same whole-cell match for applicants in this source.
    companies = {row[key] for _, _, row in ready for key in ('manufacturer', 'applicant')}
    for company in companies:
        c.db.execute('UPDATE affiliations SET target=? WHERE company=? AND record_id IN '
                     '(SELECT id FROM records WHERE source_id=?)',
                     (company_target(company, reviewed), company, sid))
    # After the whole-cell loop above: that loop would otherwise reset a part named like another row's cell.
    for certificate, claim in claims.items():
        for part in (p for p in claim['parts'] if p['role']):
            for key in claimed_keys[certificate]:
                c.db.execute('UPDATE affiliations SET target=? WHERE company=? AND role=? AND record_id IN '
                             '(SELECT id FROM records WHERE source_id=? AND source_key=?)',
                             (part['target'], part['text'], part['role'], sid, key))
    c.finish(sid, len(ready))
    return dict(registry_rows=len(ready), model_assertions=assertions)
