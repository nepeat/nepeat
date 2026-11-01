"""Bahrain TRA public register: preserve all rows and exact manufacturer claims."""
import datetime as dt
import json
from pathlib import Path
from urllib.parse import quote

from .catalog import digest, target_for

SOURCE = 'bahrain-tra'
PAGE_URL = 'https://etra.tra.org.bh/approved-equipment'
API_URL = 'https://etra.tra.org.bh/api/public/approved-equipment/list'
QUERIES = ['accton', 'celestica', 'oracle', 'amazon data', 'edgecore', 'edge core']
FIELDS = {'authority_ref', 'marketing_name', 'model_number', 'manufacturer',
          'authority_remark', 'device_description', 'expiry', 'device_remark', 'status_code'}
OPTIONAL = {'marketing_name', 'authority_remark', 'device_remark'}


def parse_rows(body):
    rows = json.loads(body)
    if not isinstance(rows, list):
        raise ValueError('Expected Bahrain API result array')
    by_ref = {}
    for row in rows:
        if not isinstance(row, dict) or set(row) != FIELDS:
            raise ValueError('Bahrain register fields changed')
        if any(not isinstance(v, str) and not (k in OPTIONAL and v is None) for k,v in row.items()):
            raise ValueError('Unexpected Bahrain field type')
        if any(not row[k].strip() for k in ['authority_ref','model_number','manufacturer','expiry','status_code']):
            raise ValueError('Missing Bahrain identity/date/status field')
        dt.datetime.strptime(row['expiry'], '%d-%m-%Y')
        ref = row['authority_ref']
        if ref in by_ref:
            raise ValueError('Duplicate Bahrain authority reference: ' + ref)
        by_ref[ref] = row
    return by_ref


def validate_bundle(path):
    path = Path(path)
    data = json.loads(path.read_text())
    if data.get('schema_version') != 1 or data.get('source_url') != API_URL:
        raise ValueError('Unrecognized Bahrain bundle')
    urls = {'full_before': API_URL, 'full_after': API_URL}
    urls.update({q: API_URL + '?search=' + quote(q) for q in QUERIES})
    if set(data.get('responses', {})) != set(urls):
        raise ValueError('Incomplete Bahrain response set')
    parsed = {}
    for key, url in urls.items():
        entry = data['responses'][key]
        name = entry['path']
        if not isinstance(name, str) or Path(name).name != name or entry['url'] != url:
            raise ValueError('Invalid Bahrain response reference')
        file = (path.parent / name).resolve()
        if file.parent != path.parent.resolve():
            raise ValueError('Bahrain file escapes bundle directory')
        body = file.read_bytes()
        if digest(body) != entry['sha256']:
            raise ValueError('Bahrain response checksum mismatch: ' + key)
        parsed[key] = parse_rows(body)
        if len(parsed[key]) != entry['rows']:
            raise ValueError('Bahrain response row count mismatch: ' + key)
    full = parsed['full_before']
    if not full or full != parsed['full_after']:
        raise ValueError('Bahrain full response changed during capture')
    for query in QUERIES:
        for ref,row in parsed[query].items():
            if full.get(ref) != row:
                raise ValueError('Filtered Bahrain response absent/different in full snapshot')
        # At least every exact target-manufacturer substring match must be in
        # that filtered response. Other matches may come from descriptions.
        expected = {ref for ref,row in full.items() if query in row['manufacturer'].casefold()}
        if not expected <= set(parsed[query]):
            raise ValueError('Filtered Bahrain response misses manufacturer matches')
    return list(full.values())


def collect(raw, fetch, refresh=False):
    raw = Path(raw)
    path = raw / 'bahrain-bundle.json'
    if path.exists() and not refresh:
        return path
    jobs = [('full_before', API_URL, 'bahrain-equipment.json')]
    jobs += [(q, API_URL + '?search=' + quote(q), 'bahrain-check-' + q.replace(' ', '-') + '.json') for q in QUERIES]
    jobs += [('full_after', API_URL, 'bahrain-equipment-after.json')]
    responses = {}
    for key,url,name in jobs:
        f = fetch(url, raw/name, True, max_bytes=50_000_000)
        body = f.read_bytes()
        responses[key] = {'url': url, 'path': name, 'sha256': digest(body), 'rows': len(parse_rows(body))}
    tmp = path.with_suffix('.part')
    tmp.write_text(json.dumps({'schema_version':1,'source_url':API_URL,'responses':responses},indent=2)+'\n')
    try:
        validate_bundle(tmp)
        tmp.replace(path)
    finally:
        if tmp.exists():
            tmp.unlink()
    return path


def import_bundle(c, path):
    rows = validate_bundle(path)
    target_rows = [(r,target_for(r['manufacturer'])) for r in rows if target_for(r['manufacturer'])]
    if not target_rows:
        raise ValueError('Bahrain register has no exact target manufacturers')
    c.source(SOURCE, API_URL, 'regulatory', 'Bahrain TRA factual public equipment register; source rights retained',
             'Unfiltered public API response used by the website, checked against a second full response and six filtered queries. '
             'No independent server total is exposed: completeness beyond the returned response is unverified. '
             'Manufacturer is the register claim, not independently verified factory/OEM or deployment evidence. '
             'Expiry and raw status are snapshot assertions, not current validity or support dates. '
             'All rows retained separately; exact target manufacturer rows populate the device catalog.', path)
    c.db.execute('DELETE FROM bahrain_equipment WHERE source_id=?', (SOURCE,))
    c.db.executemany('INSERT INTO bahrain_equipment VALUES (?,?,?,?,?,?,?,?,?,?,?)', [
        (r['authority_ref'],r['manufacturer'],r['model_number'],r['device_description'],
         r['marketing_name'],r['device_remark'],r['authority_remark'],r['expiry'],
         dt.datetime.strptime(r['expiry'],'%d-%m-%Y').date().isoformat(),r['status_code'],SOURCE) for r in rows])
    count = 0
    for r,target in target_rows:
        # Commas explicitly separate complete model labels; never split slash
        # names such as SCOUT01/AWS or discard a voltage/order suffix.
        models = [m.strip() for m in r['model_number'].split(',')]
        if any(not m for m in models) or len(models) != len(set(models)):
            raise ValueError('Ambiguous target model list')
        for model in models:
            facts = [('registry_manufacturer',r['manufacturer']),('raw_model_cell',r['model_number']),
                     ('registry_expiry_date',dt.datetime.strptime(r['expiry'],'%d-%m-%Y').date().isoformat()),
                     ('registry_status_code',r['status_code']),
                     ('evidence_scope','Regulatory manufacturer/model assertion; no factory, physical configuration, current approval or prior ownership inference')]
            facts += [(k,r[k]) for k in ['marketing_name','device_description','device_remark','authority_remark'] if r[k]]
            c.add(SOURCE,r['authority_ref']+'|'+model,PAGE_URL+'?search='+quote(r['authority_ref']),
                  target,model,target+' '+model,r['device_description'].lower() or 'registered equipment',r,
                  [(r['manufacturer'],'registry_manufacturer')],[('bahrain_authority_ref',r['authority_ref'])],facts)
            count += 1
    c.finish(SOURCE,len(rows))
    return {'registry_rows':len(rows),'target_rows':len(target_rows),'model_assertions':count}
