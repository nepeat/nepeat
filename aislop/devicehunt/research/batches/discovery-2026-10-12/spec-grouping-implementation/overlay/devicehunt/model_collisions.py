"""Read-only review lead list of catalog identities whose model strings collide.

Nothing here merges, aliases or asserts equivalence. Two identities that share a
normalised model key may be the same product under different brand strings, or
unrelated products that happen to print the same short code. A human reviews them.

Normalisation (deliberately conservative):
  1. casefold and collapse runs of whitespace to one space (plain key);
  2. optionally strip ONE leading token equal to the member's own brand, casefolded,
     followed by whitespace (stripped key). The token is the whole brand string, or,
     for a multi-word brand, its first word ("Inspur Corporation" -> "inspur"). The
     remainder must be non-empty.
Suffixes, hyphens, punctuation, revision letters and parenthesised groups are never
touched, so AS7726-32X and AS7726-32X-O-AC-F do not collide.

Group = identities sharing a stripped key with at least two distinct (brand, model)
pairs. Reasons:
  same_model_different_brand   >=2 casefolded brands, every member's plain key equal
  brand_token_prefix           one casefolded brand; keys equal only after stripping
  brand_token_prefix_different_brand   both effects together

Kinds skipped unless all_kinds: any kind containing (casefolded) 'software',
'firmware', 'bmc', 'device tree' or 'config'.

Each group also carries weak-evidence flags: short_key (key shorter than 6 characters) and
kinds_differ (members' casefolded kinds are not all equal). Nothing is dropped for them;
min_key_length (default 0) optionally drops groups whose key is shorter than N.
"""
import re

from .catalog import target_for

EXCLUDED_KIND_WORDS = ('software', 'firmware', 'bmc', 'device tree', 'config')
SHORT_KEY = 6
REASONS = ('same_model_different_brand', 'brand_token_prefix', 'brand_token_prefix_different_brand')
NOTE = ('Review lead list only: groups share a conservatively normalised model string '
        '(casefold, collapsed whitespace, optional leading own-brand token). It does not merge, '
        'alias or assert that the identities are the same product, and it changes nothing in the '
        'catalog. Suffixes, hyphens and revision letters are never stripped. Kinds containing '
        'software/firmware/BMC/device tree/config are skipped unless '
        'all kinds are requested.')


def _collapse(text):
    return re.sub(r'\s+', ' ', (text or '').strip()).casefold()


def plain_key(model):
    return _collapse(model)


def stripped_key(brand, model):
    key = _collapse(model)
    b = _collapse(brand)
    prefixes = [b] if b else []
    if ' ' in b:
        prefixes.append(b.split(' ', 1)[0])
    for prefix in prefixes:
        if key.startswith(prefix + ' ') and key[len(prefix) + 1:].strip():
            return key[len(prefix) + 1:], True
    return key, False


def kind_excluded(kind):
    k = _collapse(kind)
    return any(w in k for w in EXCLUDED_KIND_WORDS)


def run(db, target=None, limit=20, all_kinds=False, min_key_length=0, families=None):
    wanted = None
    if target is not None:
        wanted = target_for(target)
        if wanted is None:
            raise ValueError('Unknown target vendor: ' + str(target))
    if type(limit) is not int or limit < -1:
        raise ValueError('Limit must be -1 or nonnegative')
    if type(min_key_length) is not int or min_key_length < 0:
        raise ValueError('Minimum key length must be nonnegative')
    own = not db.in_transaction
    if own:
        db.execute('BEGIN')
    try:
        return _run(db, wanted, limit, all_kinds, min_key_length, families)
    finally:
        if own:
            db.rollback()


def _run(db, wanted, limit, all_kinds, min_key_length=0, families=None):
    members = {}
    skipped = 0
    for r in db.execute('SELECT id,brand,model,kind FROM devices ORDER BY brand,model,id'):
        if not all_kinds and kind_excluded(r['kind']):
            skipped += 1
            continue
        skey, stripped = stripped_key(r['brand'], r['model'])
        if not skey:
            continue
        members[r['id']] = {'id': r['id'], 'brand': r['brand'], 'model': r['model'], 'kind': r['kind'],
                            'plain_key': plain_key(r['model']), 'brand_token_stripped': stripped,
                            '_skey': skey, 'source_count': 0, 'source_tiers': set(), 'affiliations': set(),
                            '_targets': set()}
    for r in db.execute('SELECT r.device_id,COUNT(DISTINCT r.source_id) n FROM records r GROUP BY r.device_id'):
        if r['device_id'] in members:
            members[r['device_id']]['source_count'] = r['n']
    for r in db.execute('SELECT DISTINCT r.device_id,s.tier FROM records r JOIN sources s ON s.id=r.source_id'):
        if r['device_id'] in members:
            members[r['device_id']]['source_tiers'].add(r['tier'])
    for r in db.execute('SELECT DISTINCT r.device_id,a.company,a.role,a.target FROM records r '
                        'JOIN affiliations a ON a.record_id=r.id'):
        m = members.get(r['device_id'])
        if m:
            m['affiliations'].add((r['company'], r['role']))
            if r['target']:
                m['_targets'].add(r['target'])
    by_key = {}
    for m in members.values():
        by_key.setdefault(m['_skey'], []).append(m)
    groups = []
    for key, items in by_key.items():
        if len(key) < min_key_length or len({(m['brand'], m['model']) for m in items}) < 2:
            continue
        if wanted and not any(wanted in m['_targets'] or target_for(m['brand']) == wanted for m in items):
            continue
        brands = {_collapse(m['brand']) for m in items}
        same_plain = len({m['plain_key'] for m in items}) == 1
        if len(brands) > 1:
            reason = 'same_model_different_brand' if same_plain else 'brand_token_prefix_different_brand'
        elif same_plain:
            continue  # same brand, same plain key: differs only by case/whitespace/revision, not a lead
        else:
            reason = 'brand_token_prefix'
        out = []
        for m in sorted(items, key=lambda x: (x['brand'], x['model'], x['id'])):
            out.append({'id': m['id'], 'brand': m['brand'], 'model': m['model'], 'kind': m['kind'],
                        'brand_token_stripped': m['brand_token_stripped'], 'source_count': m['source_count'],
                        'source_tiers': sorted(m['source_tiers']),
                        'affiliations': [{'company': c, 'role': ro} for c, ro in sorted(m['affiliations'])]})
        groups.append({'key': key, 'reason': reason, 'size': len(out), 'short_key': len(key) < SHORT_KEY,
                       'kinds_differ': len({_collapse(m['kind']) for m in items}) > 1, 'members': out})
    groups.sort(key=lambda g: (-g['size'], g['key']))
    counts = {r: sum(1 for g in groups if g['reason'] == r) for r in REASONS}
    if families is not None:
        # Display-only annotation, applied after grouping, sorting and counting: it never
        # changes which groups exist, their members, order or reasons.
        for g in groups:
            ids = [families.family_of(m['brand']) for m in g['members']]
            for m, fid in zip(g['members'], ids):
                m['brand_family'] = fid
            g['family'] = {'ids': sorted({i for i in ids if i}), 'unmapped_members': sum(1 for i in ids if not i),
                           'same_family': bool(ids[0]) and len(set(ids)) == 1}
    shown = groups if limit == -1 else groups[:limit]
    result = _result(wanted, all_kinds, min_key_length, skipped, groups, counts, shown)
    if families is not None:
        result['brand_families'] = families.metadata()
        result['groups_same_family'] = sum(1 for g in groups if g['family']['same_family'])
    return result


def _result(wanted, all_kinds, min_key_length, skipped, groups, counts, shown):
    return {'note': NOTE, 'all_kinds': all_kinds, 'min_key_length': min_key_length, 'target': wanted, 'skipped_by_kind': skipped,
            'groups_total': len(groups), 'counts_by_reason': counts, 'shown': len(shown),
            'truncated': len(shown) < len(groups), 'groups': shown}


def render_table(result):
    lines = [result['note'],
             'groups: %d (%s); skipped by kind: %d' % (
                 result['groups_total'], ', '.join('%s=%d' % kv for kv in result['counts_by_reason'].items()),
                 result['skipped_by_kind'])]
    for g in result['groups']:
        lines.append('')
        fam = g.get('family')
        lines.append('[%s] key=%r members=%d%s%s%s' % (g['reason'], g['key'], g['size'],
                     ' short_key' if g['short_key'] else '', ' kinds_differ' if g['kinds_differ'] else '',
                     (' family=%s%s' % (','.join(fam['ids']), ' same_family' if fam['same_family'] else '')) if fam and fam['ids'] else ''))
        for m in g['members']:
            aff = '; '.join('%s (%s)' % (a['company'], a['role']) for a in m['affiliations'])
            lines.append('  %s | %s | %s | %s | sources=%d tiers=%s | %s' % (
                m['id'], m['brand'], m['model'], m['kind'], m['source_count'], ','.join(m['source_tiers']), aff))
    if 'brand_families' in result:
        bf = result['brand_families']
        lines.append('')
        lines.append('brand families (display only): %s sha256=%s; %d of %d rows pending review; same_family groups: %d' % (
            bf['path'], bf['sha256'][:12], bf['rows_pending_review'], bf['rows'], result['groups_same_family']))
    if result['truncated']:
        lines.append('')
        lines.append('Showing %d of %d groups; use --limit -1 for all.' % (result['shown'], result['groups_total']))
    return '\n'.join(lines)
