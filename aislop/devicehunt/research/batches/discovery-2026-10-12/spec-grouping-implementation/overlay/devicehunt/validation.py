"""Check owner inventory against external snapshots without importing expectations."""
import json
import re
from pathlib import Path

from .catalog import digest, now, target_for


# Only identity fields count. Facts may name replacements or compatible devices.
IDENTITY_KINDS = {'alias', 'model_search', 'manufacturer_model', 'part_number', 'fcc_id', 'ic_id'}
EXTERNAL_TIERS = {'vendor', 'project', 'community', 'regulatory', 'regulatory_mirror', 'primary_document_mirror', 'certification_body', 'physical_label_photo'}


def identity(value):
    # Case and separator typography may differ, but never remove model prefixes,
    # revisions, or ordering-code suffixes (including leading zeroes).
    return re.sub(r'[-_\s\u2010-\u2015]+', '-', value.strip().casefold())


def load_inventory(path):
    raw = Path(path).read_bytes()
    data = json.loads(raw)
    if data.get('schema_version') != 1 or not isinstance(data.get('devices'), list) or not data['devices']:
        raise ValueError('Inventory must have schema_version 1 and a nonempty devices list')
    seen = set()
    for item in data['devices']:
        if not isinstance(item, dict) or not isinstance(item.get('identifier'), str) or not re.search(r'[a-zA-Z0-9]', item['identifier']):
            raise ValueError('Inventory entries require a hardware identifier')
        if not isinstance(item.get('vendor'), str) or not target_for(item['vendor']):
            raise ValueError('Inventory entry has an unknown target vendor')
        key = (target_for(item['vendor']), identity(item['identifier']))
        if key in seen:
            raise ValueError('Duplicate inventory identifier: ' + item['identifier'])
        seen.add(key)
    return data, digest(raw)


def run(db, inventory_path):
    """Read-only evaluation; no source, device, or search-index writes."""
    inventory, inventory_sha = load_inventory(inventory_path)
    sources = {r['id']: dict(r) for r in db.execute(
        'SELECT id,tier,sha256,cache_path,upstream_revision,coverage FROM sources')}
    eligible = {sid for sid, s in sources.items()
                if s['tier'] in EXTERNAL_TIERS and s['sha256'] and s['cache_path']}
    records = [dict(r) for r in db.execute(
        'SELECT r.id,r.device_id,r.source_id,r.source_url,d.model FROM records r '
        'JOIN devices d ON d.id=r.device_id ORDER BY r.source_id,r.id')]
    fields, targets = {}, {}
    for row in db.execute('SELECT record_id,kind,value FROM identifiers'):
        if row['kind'] in IDENTITY_KINDS or row['kind'] == 'onie_platform':
            fields.setdefault(row['record_id'], []).append((row['kind'], row['value']))
    for row in db.execute('SELECT record_id,target FROM affiliations WHERE target IS NOT NULL'):
        targets.setdefault(row['record_id'], set()).add(row['target'])
    results = []
    for item in inventory['devices']:
        wanted = identity(item['identifier'])
        vendor = target_for(item['vendor'])
        evidence, leads = [], []
        for record in records:
            values = [('model', record['model'])] + fields.get(record['id'], [])
            for kind, value in values:
                normalized = identity(value)
                # Build-target names and family prefixes remain leads. They do
                # not establish the complete chassis/order code in the inventory.
                exact = normalized == wanted and kind != 'onie_platform'
                family = wanted.startswith(normalized + '-') or normalized.startswith(wanted + '-')
                if not exact and not family and not (kind == 'onie_platform' and wanted in normalized):
                    continue
                proof = {'device_id': record['device_id'], 'record_id': record['id'],
                         'source_id': record['source_id'], 'source_url': record['source_url'],
                         'field': kind, 'value': value, 'source_sha256': sources[record['source_id']]['sha256'],
                         'source_tier': sources[record['source_id']]['tier'],
                         'source_coverage': sources[record['source_id']]['coverage']}
                if exact and record['source_id'] in eligible and vendor in targets.get(record['id'], set()):
                    evidence.append(proof)
                else:
                    proof['reason'] = ('incomplete identity or build target' if not exact else
                                       'seller-reported identity; independent corroboration needed' if sources[record['source_id']]['tier'] == 'marketplace' else
                                       'no imported external snapshot' if record['source_id'] not in eligible else
                                       'expected vendor not evidenced on this record')
                    leads.append(proof)
        # Show the actual search behavior too, but never let loose FTS matches
        # (including words scattered across facts) turn into identity evidence.
        tokens = re.findall(r'\w+', item['identifier'])
        query = ' AND '.join('"' + t + '"*' for t in tokens)
        search_hits = [dict(r) for r in db.execute(
            'SELECT d.id,d.brand,d.model FROM devices d WHERE d.id IN '
            '(SELECT device_id FROM search_index WHERE search_index MATCH ?) ORDER BY d.id', (query,))]
        searchable = bool({p['device_id'] for p in evidence} & {r['id'] for r in search_hits})
        gap_type = 'source_coverage' if not evidence else 'search_index' if not searchable else None
        results.append({**item, 'status': 'gap' if gap_type else 'hit', 'gap_type': gap_type,
                        'search_hits': search_hits, 'evidence': evidence, 'leads': leads})
    hits = sum(r['status'] == 'hit' for r in results)
    document_hits = sum(r['status'] == 'hit' and any(
        p['source_tier'] != 'physical_label_photo' for p in r['evidence']) for r in results)
    return {'generated_at': now(), 'inventory_sha256': inventory_sha,
            'policy': 'Complete model/identifier on an imported source snapshot with the expected vendor on the same record. Reviewed physical-label photos qualify separately from seller text or owner assertions; their source tier and capture provenance remain explicit. Case/separators normalized; prefixes and suffixes retained. Owner AWS claims are context, not validated deployment evidence.',
            'eligible_sources': [sources[sid] for sid in sorted(eligible)],
            'search_summary': {'total': len(results), 'queries_with_results': sum(bool(r['search_hits']) for r in results)},
            'summary': {'total': len(results), 'hits': hits, 'gaps': len(results) - hits},
            'evidence_summary': {'document_snapshot_hits': document_hits,
                                 'label_photo_only_hits': hits - document_hits,
                                 'unresolved_identity_gaps': len(results) - hits},
            'results': results}
