"""Source-scoped hyperscaler relationships, without promoting lineage to deployment."""
import json

from .catalog import target_for
from .vendor_inventory import snapshot_eligible

SCOPE = ('Explicit imported hyperscaler relationship records only. Deployment, design lineage, '
         'announcements, product families and regulatory associations remain distinct. Default results '
         'require eligible external-source snapshot metadata; this does not reverify cached bytes. '
         'Vendor and role filters must match the same affiliation on the relationship record. '
         'No company mapping is inferred from brand labels or other records. Dates and statements '
         'are historical source claims, not proof of current use or an auction unit\'s previous owner. '
         'Unverified records can be included explicitly. A missing result is a collection gap, not '
         'proof that a platform was never used by a hyperscaler.')


def run(db, vendor=None, role=None, operator=None, relationship=None,
        include_unverified=False, limit=100):
    target = target_for(vendor) if vendor is not None else None
    if vendor is not None and target is None:
        raise ValueError('Unknown target vendor: ' + str(vendor))
    if type(limit) is not int or limit < -1:
        raise ValueError('Limit must be -1 or nonnegative')
    if type(include_unverified) is not bool:
        raise ValueError('include_unverified must be boolean')
    for label, value in [('role', role), ('operator', operator), ('relationship', relationship)]:
        if value is not None and (not isinstance(value, str) or not value.strip()):
            raise ValueError(label + ' must be a nonempty string')
    role = role.strip() if role is not None else None
    operator = operator.strip() if operator is not None else None
    relationship = relationship.strip() if relationship is not None else None
    own_transaction = not db.in_transaction
    if own_transaction:
        db.execute('BEGIN')
    try:
        return _run(db, target, role, operator, relationship, include_unverified, limit)
    finally:
        if own_transaction:
            db.rollback()


def _run(db, target, role, operator, relationship, include_unverified, limit):
    affiliations, dates = {}, {}
    for row in db.execute('SELECT record_id,company,target,role FROM affiliations '
                          'ORDER BY record_id,company,role'):
        affiliations.setdefault(row['record_id'], []).append(
            {key: row[key] for key in ('company', 'target', 'role')})
    for row in db.execute("SELECT record_id,name,value FROM facts WHERE name IN "
                          "('published_date','published_timestamp') ORDER BY record_id,name,value"):
        dates.setdefault(row['record_id'], []).append(dict(field=row['name'], value=row['value']))
    rows = []
    for row in db.execute(
        'SELECT d.id AS device_id,d.brand,d.model,d.revision,r.id AS record_id,'
        'r.source_id,r.source_url,h.operator,h.relationship,h.detail,s.tier,s.sha256,'
        's.cache_path,s.coverage,s.upstream_revision,s.fetched_at '
        'FROM hyperscaler_evidence h JOIN records r ON r.id=h.record_id '
        'JOIN devices d ON d.id=r.device_id JOIN sources s ON s.id=r.source_id '
        'ORDER BY h.operator,h.relationship,d.brand,d.model,d.revision,d.id,r.source_id,r.id'):
        if operator is not None and row['operator'].casefold() != operator.casefold():
            continue
        if relationship is not None and row['relationship'] != relationship:
            continue
        eligible = snapshot_eligible(row)
        if not include_unverified and not eligible:
            continue
        aff = affiliations.get(row['record_id'], [])
        matched = [a for a in aff if (target is None or a['target'] == target)
                   and (role is None or a['role'] == role)]
        if (target is not None or role is not None) and not matched:
            continue
        rows.append({**{key: row[key] for key in (
            'device_id','brand','model','revision','record_id','source_id','source_url',
            'operator','relationship','detail','tier','coverage')},
            'source_sha256': row['sha256'], 'source_revision': row['upstream_revision'],
            'source_fetched_at': row['fetched_at'], 'source_snapshot_eligible': eligible,
            'affiliations': aff, 'matched_affiliations': matched,
            'source_dates': dates.get(row['record_id'], [])})
    by_relationship, by_operator = {}, {}
    for row in rows:
        by_relationship[row['relationship']] = by_relationship.get(row['relationship'], 0) + 1
        by_operator[row['operator']] = by_operator.get(row['operator'], 0) + 1
    shown = rows if limit == -1 else rows[:limit]
    return dict(scope_description=SCOPE, vendor=target, role=role, operator=operator,
                relationship=relationship, include_unverified=include_unverified,
                assertions=len(rows), unique_identities=len({r['device_id'] for r in rows}),
                counts_by_relationship=dict(sorted(by_relationship.items())),
                counts_by_operator=dict(sorted(by_operator.items())), shown=len(shown),
                truncated=len(shown)<len(rows), rows=shown)


def flat_rows(result):
    """One CSV/table row per relationship assertion, with source roles intact."""
    return [{**{k: v for k, v in row.items() if k not in (
                 'affiliations', 'matched_affiliations', 'source_dates')},
             'affiliations_json': json.dumps(row['affiliations'], ensure_ascii=False),
             'matched_affiliations_json': json.dumps(row['matched_affiliations'], ensure_ascii=False),
             'source_dates_json': json.dumps(row['source_dates'], ensure_ascii=False)}
            for row in result['rows']]
