"""Read-only target inventories with manufacturer, factory and other roles separate."""
from .catalog import TARGETS, target_for
from .validation import EXTERNAL_TIERS

MANUFACTURER_ROLES = frozenset({'manufacturer', 'registry_manufacturer', 'declared_manufacturer', 'declared_oem'})
FACTORY_ROLES = frozenset({'listed_factory'})
SCOPE = ('Collected catalog evidence only, not a complete historical inventory or an independent factory audit. '
         'Manufacturer scope includes named manufacturer, regulatory manufacturer and declared OEM claims '
         'on imported external snapshots. Legal manufacturer need not be the physical factory. Factory '
         'listings are separate. Other roles, seller claims and unhashed evidence are available with scope all. '
         'Each target and role must occur on the same evidence record; brand labels alone never qualify. '
         'No unit provenance, current certification, availability or software entitlement is inferred.')


def evidence_class(role):
    if role in MANUFACTURER_ROLES:
        return 'manufacturer_claim'
    if role in FACTORY_ROLES:
        return 'factory_listing'
    return 'other_association'


def snapshot_eligible(source):
    """Stored import metadata eligibility, shared by CLI inventory and HTML."""
    return bool(source['tier'] in EXTERNAL_TIERS and source['sha256'] and source['cache_path'])


def run(db, vendor=None, scope='manufacturer', limit=100):
    target = target_for(vendor) if vendor is not None else None
    if vendor is not None and target is None:
        raise ValueError('Unknown target vendor: ' + str(vendor))
    if scope not in {'manufacturer', 'factory', 'all'}:
        raise ValueError('Inventory scope must be manufacturer, factory or all')
    if type(limit) is not int or limit < -1:
        raise ValueError('Limit must be -1 or nonnegative')
    own_transaction = not db.in_transaction
    if own_transaction:
        db.execute('BEGIN')
    try:
        return _run(db, target, scope, limit)
    finally:
        if own_transaction:
            db.rollback()


def _run(db, target, scope, limit):
    sql = ('SELECT d.id AS device_id,d.brand,d.model,d.revision,d.kind,a.target,a.company,a.role,'
           'r.id AS record_id,r.source_id,r.source_url,s.tier,s.sha256,s.cache_path,s.coverage '
           'FROM affiliations a JOIN records r ON r.id=a.record_id '
           'JOIN devices d ON d.id=r.device_id JOIN sources s ON s.id=r.source_id '
           'WHERE a.target IS NOT NULL')
    args = []
    if target:
        sql += ' AND a.target=?'; args.append(target)
    sql += ' ORDER BY a.target,d.brand,d.model,d.revision,d.id,r.source_id,r.id,a.role,a.company'
    groups = {}
    for row in db.execute(sql, args):
        eligible = snapshot_eligible(row)
        category = evidence_class(row['role'])
        if scope != 'all' and (not eligible or category != {'manufacturer':'manufacturer_claim','factory':'factory_listing'}[scope]):
            continue
        key = (row['target'], row['device_id'])
        if key not in groups:
            groups[key] = {k:row[k] for k in ('target','device_id','brand','model','revision','kind')}
            groups[key]['evidence'] = []
        groups[key]['evidence'].append({**{k:row[k] for k in ('record_id','source_id','source_url','tier','company','role','coverage')},
            'source_sha256':row['sha256'], 'source_snapshot_eligible':eligible, 'evidence_class':category})
    rows = list(groups.values())
    counts = []
    for name in ([target] if target else TARGETS):
        items = [r for r in rows if r['target'] == name]
        evidence = [e for r in items for e in r['evidence']]
        counts.append({'target':name, 'identities':len(items), 'source_records':len({e['record_id'] for e in evidence}),
                       'sources':len({e['source_id'] for e in evidence}), 'role_assertions':len(evidence)})
    shown = rows if limit == -1 else rows[:limit]
    return {'scope_description':SCOPE, 'scope':scope, 'vendor':target,
            'manufacturer_roles':sorted(MANUFACTURER_ROLES), 'factory_roles':sorted(FACTORY_ROLES),
            'identity_target_pairs':len(rows), 'unique_identities':len({r['device_id'] for r in rows}),
            'shown':len(shown), 'truncated':len(shown)<len(rows), 'counts_by_target':counts, 'rows':shown}


def flat_rows(result):
    """One CSV row per matching role assertion, not one row per unique device."""
    return [{**{k:r[k] for k in ('target','device_id','brand','model','revision')},
             'inventory_scope':result['scope'], **e} for r in result['rows'] for e in r['evidence']]
