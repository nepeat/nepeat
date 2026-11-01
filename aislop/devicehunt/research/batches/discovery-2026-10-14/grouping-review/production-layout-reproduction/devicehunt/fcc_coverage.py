"""Read-only collection gaps among locally known target-associated FCC IDs."""
from .catalog import target_for

GRANTEES = {'HED': 'Accton', 'YZK': 'Edge-Core', 'A4H': 'Oracle',
            '2AVPL': 'Amazon Data Services', 'MRF': 'Celestica', 'NVN': 'Celestica',
            'DGI': 'Inventec', '2AUSZ': 'Hyve', 'HFS': 'Quanta', 'PZ5': 'Quanta'}
TABLES = {'index': 'fcc_id_index', 'detail': 'fcc_detail_pages',
          'application': 'fcc_applications', 'pdf': 'documents'}
SCOPE = ('Collection coverage of locally known target-associated FCC IDs only; not a complete '
         'FCC filing inventory. Catalog associations use explicit fcc_id identifiers and target '
         'roles on the same source record. Grantee associations use stored reviewed grantee codes. '
         'Neither proves manufacture or model equivalence. IDs are compared case-insensitively '
         'with outer whitespace trimmed; all punctuation and suffixes remain significant. '
         'Missing means no matching local record, not that a filing or public PDF does not exist. '
         'A cached index, detail page or PDF does not establish a complete application or exhibit '
         'archive. Stored cache metadata is reported without checking files or making network requests.')


def identity(value):
    return str(value).strip().casefold()


def run(db, vendor=None, missing=None, limit=100):
    target = target_for(vendor) if vendor is not None else None
    if vendor is not None and target is None:
        raise ValueError('Unknown target vendor: ' + str(vendor))
    if missing is not None and missing not in TABLES:
        raise ValueError('Unknown FCC collection stage')
    if type(limit) is not int or limit < -1:
        raise ValueError('Limit must be -1 or nonnegative')
    own_transaction = not db.in_transaction
    if own_transaction:
        db.execute('BEGIN')
    try:
        return _run(db, target, missing, limit)
    finally:
        if own_transaction:
            db.rollback()


def _run(db, target, missing, limit):
    tables = {r[0] for r in db.execute("SELECT name FROM sqlite_master WHERE type='table'")}
    loaded = {stage: table in tables for stage, table in TABLES.items()}
    catalog_loaded = {'identifiers', 'records', 'devices', 'affiliations', 'sources'} <= tables
    rows = {}

    def entry(value):
        key = identity(value)
        if not key:
            return None
        item = rows.setdefault(key, dict(fcc_id=key.upper(), printed_ids=set(), targets=set(),
            catalog_claims=[], index=[], detail=[], application=[], pdf=[]))
        item['printed_ids'].add(value)
        return item

    if catalog_loaded:
        sql = ('SELECT i.value,d.brand,d.model,d.revision,r.id AS record_id,r.source_id,'
               'r.source_url,s.tier,a.company,a.target,a.role FROM identifiers i '
               'JOIN records r ON r.id=i.record_id JOIN devices d ON d.id=r.device_id '
               'JOIN sources s ON s.id=r.source_id JOIN affiliations a ON a.record_id=r.id '
               "WHERE i.kind='fcc_id' AND a.target IS NOT NULL")
        args = []
        if target:
            sql += ' AND a.target=?'; args.append(target)
        for r in db.execute(sql + ' ORDER BY i.value,r.id,a.target,a.role,a.company', args):
            claim = dict(r); item = entry(claim['value'])
            if item is not None:
                item['targets'].add(claim['target']); item['catalog_claims'].append(claim)

    # Seed additional discovery IDs from explicit grantee fields, never a guessed
    # prefix of an arbitrary model or identifier. Unavailable details remain evidence.
    for stage in ('index', 'detail'):
        if not loaded[stage]:
            continue
        for r in db.execute('SELECT * FROM ' + TABLES[stage] + ' ORDER BY source_id,fcc_id'):
            associated = GRANTEES.get(r['grantee_code'])
            if associated and (target is None or target == associated):
                item = entry(r['fcc_id'])
                if item is not None:
                    item['targets'].add(associated)

    # Application records may be present without a duplicate identifier row.
    if loaded['application'] and {'affiliations', 'records'} <= tables:
        sql = ('SELECT f.fcc_id,a.target FROM fcc_applications f '
               'JOIN records r ON r.id=f.record_id JOIN affiliations a ON a.record_id=r.id '
               'WHERE a.target IS NOT NULL')
        args = []
        if target:
            sql += ' AND a.target=?'; args.append(target)
        for r in db.execute(sql, args):
            item = entry(r['fcc_id'])
            if item is not None:
                item['targets'].add(r['target'])

    # Decorate each known ID with all locally stored stage records. Preserve
    # duplicate mirror claims and failure statuses instead of collapsing them.
    for stage, table in TABLES.items():
        if not loaded[stage]:
            continue
        for r in db.execute('SELECT * FROM ' + table + ' ORDER BY fcc_id'):
            item = rows.get(identity(r['fcc_id']))
            if item is not None:
                item['printed_ids'].add(r['fcc_id'])
                item[stage].append(dict(r))

    ordered = []
    for key in sorted(rows):
        item = rows[key]
        item['printed_ids'] = sorted(item['printed_ids'])
        item['targets'] = sorted(item['targets'])
        counts = {stage: len(item[stage]) for stage in TABLES}
        counts['detail'] = sum(r['status'] == 'captured' for r in item['detail'])
        item['collection'] = {stage: ('not_loaded' if not loaded[stage] else
            'collected' if counts[stage] else 'not_collected') for stage in TABLES}
        item['counts'] = dict(index_entries=len(item['index']), detail_attempts=len(item['detail']),
            captured_detail_pages=counts['detail'], application_records=len(item['application']),
            pdf_documents=len(item['pdf']), catalog_claims=len(item['catalog_claims']))
        ordered.append(item)
    summary = dict(known_fcc_ids=len(ordered), stages={stage: {
        state: sum(r['collection'][stage] == state for r in ordered)
        for state in ('collected', 'not_collected', 'not_loaded')} for stage in TABLES})
    selected = [r for r in ordered if missing is None or r['collection'][missing] == 'not_collected']
    shown = selected if limit == -1 else selected[:limit]
    return dict(scope=SCOPE, vendor=target, missing=missing,
        datasets={**{s: 'loaded' if v else 'not_loaded' for s,v in loaded.items()},
                  'catalog': 'loaded' if catalog_loaded else 'not_loaded'},
        summary=summary, matches=len(selected), shown=len(shown), truncated=len(shown)<len(selected), rows=shown)


def flat_rows(result):
    return [dict(fcc_id=r['fcc_id'], targets=', '.join(r['targets']),
                 **r['counts'], **{stage+'_state': state for stage,state in r['collection'].items()})
            for r in result['rows']]
