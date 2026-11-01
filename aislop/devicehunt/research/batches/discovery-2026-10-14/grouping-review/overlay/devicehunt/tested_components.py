"""Search components within explicit tested configurations, never model aliases."""
import datetime as dt
import json

from .catalog import target_for
from .vendor_inventory import snapshot_eligible

SCOPE = ('Components in reviewed tested configurations only. Vendor filters apply to '
         'the parent record association, not the component manufacturer. Quantities '
         'and descriptions belong to the named configuration; no interchangeability, '
         'installed auction-unit configuration or parent/component identity alias is inferred. '
         'Snapshot eligibility is metadata only; capture method and original-document '
         'availability remain separate.')


def parse(value):
    data = json.loads(value)
    if not isinstance(data, dict) or data.get('schema_version') != 1:
        raise ValueError('Invalid tested-configuration format')
    for key in ('report_number', 'issued_on', 'capture_method', 'original_document_status'):
        if not isinstance(data.get(key), str) or not data[key].strip():
            raise ValueError('Missing tested-configuration provenance: ' + key)
    dt.date.fromisoformat(data['issued_on'])
    configs = data.get('configurations')
    if not isinstance(configs, list) or not configs:
        raise ValueError('Missing tested configurations')
    seen = set()
    for config in configs:
        if not isinstance(config, dict):
            raise ValueError('Invalid configuration')
        cid = config.get('configuration_id')
        if not isinstance(cid, str) or not cid.strip() or cid in seen:
            raise ValueError('Invalid or duplicate configuration identifier')
        seen.add(cid)
        if type(config.get('source_page')) is not int or config['source_page'] < 1:
            raise ValueError('Invalid configuration page')
        parts = config.get('components')
        if not isinstance(parts, list) or not parts:
            raise ValueError('Missing configuration components')
        for part in parts:
            if not isinstance(part, dict):
                raise ValueError('Invalid component')
            for key in ('kind', 'manufacturer', 'model', 'description'):
                if not isinstance(part.get(key), str) or not part[key].strip():
                    raise ValueError('Missing component field: ' + key)
            if type(part.get('quantity')) is not int or part['quantity'] < 1:
                raise ValueError('Invalid tested component quantity')
    return data


def run(db, query='', vendor=None, limit=100):
    if not isinstance(query, str) or type(limit) is not int or limit < -1:
        raise ValueError('Query must be text and limit must be -1 or nonnegative')
    target = target_for(vendor) if vendor is not None else None
    if vendor is not None and target is None:
        raise ValueError('Unknown target vendor: ' + str(vendor))
    clause, args = '', ['tested_configurations']
    if target:
        clause = ' AND EXISTS (SELECT 1 FROM affiliations a WHERE a.record_id=r.id AND a.target=?)'
        args.append(target)
    # One SELECT provides a consistent read without changing caller transactions.
    records = db.execute('SELECT r.id AS record_id,d.id AS device_id,d.brand,d.model AS parent_model,'
        'd.revision,r.source_id,r.source_url,s.tier,s.sha256 AS source_sha256,s.cache_path,'
        "s.coverage,f.value,(SELECT group_concat(company || ' (' || role || ')', '; ') "
        'FROM affiliations a WHERE a.record_id=r.id) AS parent_company_roles '
        'FROM facts f JOIN records r ON r.id=f.record_id JOIN devices d ON d.id=r.device_id '
        'JOIN sources s ON s.id=r.source_id WHERE f.name=?' + clause +
        ' ORDER BY d.brand,d.model,d.revision,r.source_id,r.id,f.value', args).fetchall()
    rows = []; needle = query.casefold().strip()
    for record in records:
        base = dict(record); data = parse(base.pop('value'))
        base['source_snapshot_eligible'] = snapshot_eligible(dict(
            tier=base['tier'], sha256=base['source_sha256'], cache_path=base['cache_path']))
        base.update({k:data[k] for k in ('report_number','issued_on','capture_method','original_document_status')})
        for config in data['configurations']:
            for number, part in enumerate(config['components'], 1):
                haystack = ' '.join([base['parent_model'],config['configuration_id'],part['kind'],
                                    part['manufacturer'],part['model'],part['description']]).casefold()
                if needle not in haystack:
                    continue
                rows.append(dict(base, configuration_id=config['configuration_id'],
                    source_page=config['source_page'], component_row=number,
                    component_kind=part['kind'], component_manufacturer=part['manufacturer'],
                    component_model=part['model'], quantity=part['quantity'], description=part['description']))
    shown = rows if limit == -1 else rows[:limit]
    return dict(scope=SCOPE,query=query,vendor=target,matches=len(rows),shown=len(shown),
                truncated=len(shown)<len(rows),rows=shown)
