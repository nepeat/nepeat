"""Read-only model mentions in auction titles; never import seller text as evidence."""
import csv
import json
import re
from pathlib import Path

from .catalog import target_for
from .model_collisions import EXCLUDED_KIND_WORDS
from .specifications import CATEGORIES

KINDS = {'alias', 'model_search', 'manufacturer_model', 'part_number'}
DASHES = str.maketrans({c: '-' for c in '\u2010\u2011\u2012\u2013\u2014\u2015\u2212'})
SCOPE = ('Matches are mentions in supplied listing text, not identification of the item being sold. '
         'Model/alias evidence comes only from the existing catalog. Suffix variants remain leads, '
         'and seller-only evidence remains seller-only. No availability, condition, software entitlement, '
         'OEM or previous ownership is inferred. No network access or catalog writes.')
SPECIFICATION_SCOPE = ('Specifications are original statements from the same record as each exact model or identifier mention. '
                      'Record part numbers retain configuration scope, including when only the family name was mentioned. '
                      'Conflicting records and seller claims stay separate. Variant leads receive no specifications. '
                      'An empty list means no categorized statements on that matching record; it is not proof of compatibility '
                      'or of the configuration, installed parts or power requirements of the auction item.')
# Software-reference identities describe files in OpenBMC, ONL, SONiC, ONIE, coreboot etc., not
# sellable hardware. A device is one when its brand is listed here OR its casefolded kind contains
# a word from model_collisions.EXCLUDED_KIND_WORDS (software/firmware/bmc/device tree/config),
# the same kind rule `model-collisions` uses. Excluded from match-listing unless requested.
SOFTWARE_REFERENCE_BRANDS = frozenset(b.casefold() for b in (
    'Meta OpenBMC platform', 'OpenBMC machine', 'OpenBMC entity-manager config', 'Linux BMC device tree',
    'ONIE machine', 'SONiC platform', 'ONL platform', 'Stratum platform', 'DENT platform',
    'coreboot mainboard'))
SOFTWARE_MATCH = 'software_reference_mention'
FAMILY_SCOPE = ('Display only: brand_family labels a candidate by reviewed brand family; same_family_candidates counts the '
                'OTHER candidates (all, not just shown) in the same family, 0 when none or unmapped. It is a grouping hint, '
                'not a ranking signal, and never an identity or model-equivalence claim.')


def is_software_reference(brand, kind):
    kind = (kind or '').casefold()
    return (brand or '').casefold() in SOFTWARE_REFERENCE_BRANDS or any(w in kind for w in EXCLUDED_KIND_WORDS)


CONTEXT = ['for parts', 'parts only', 'for repair', 'untested', 'as is', 'as-is',
           'compatible with', 'replacement', 'power supply', 'psu', 'fan', 'fans',
           'rack rails', 'heatsink', 'cable', 'cables', 'empty chassis']


def normalized(value):
    return ' '.join(value.translate(DASHES).casefold().split())


def usable(value, include_short_models=False, include_numeric_identifiers=False):
    if len(value) >= 4 and any(c.isalpha() for c in value):
        return True
    if (include_numeric_identifiers and re.fullmatch(r'[0-9]+(?:-[0-9]+)*', value)
            and 6 <= len(value.replace('-', '')) <= 20):
        return True
    return (include_short_models and len(value) == 3 and value.isalnum()
            and any(c.isalpha() for c in value) and any(c.isdigit() for c in value))


def pattern(value):
    # A longer ordering code must not become an exact mention of its prefix.
    # Plus is a model suffix too (for example SV7220G3-V+).
    return re.compile(r'(?<![\w+-])' + re.escape(value) + r'(?![\w+-])')


def accessory_revision_pattern(term):
    """Recognize code + short revision spelling, never an identity alias."""
    parts = re.fullmatch(r'([a-z0-9]+(?:-[a-z0-9]+)+),? ([a-z0-9]{1,8})', term)
    if not parts or not any(c.isdigit() for c in parts[1]) or not any(c.isalpha() for c in parts[1]):
        return None
    base, revision = map(re.escape, parts.groups())
    return re.compile(r'(?<![\w+-])' + base + r'(?:[- ]|, ?)' + revision + r'(?![\w+-])')


class Matcher:
    def __init__(self, db, vendor=None, include_short_models=False, include_numeric_identifiers=False,
                 include_specifications=False, include_software_references=False, families=None):
        self.families = families
        self.include_software_references = include_software_references
        self.include_short_models = include_short_models
        self.include_numeric_identifiers = include_numeric_identifiers
        self.include_specifications = include_specifications
        target = target_for(vendor) if vendor else None
        if vendor and target is None:
            raise ValueError('Unknown target: ' + vendor)
        own_transaction = not db.in_transaction
        if own_transaction:
            db.execute('BEGIN')
        try:
            self.devices = {r['id']: dict(r) for r in db.execute('SELECT * FROM devices')}
            affiliations = {}
            for r in db.execute('SELECT record_id,company,target,role FROM affiliations ORDER BY record_id,company,role'):
                affiliations.setdefault(r['record_id'], []).append(dict(r))
            self.record_specs, self.record_parts = {}, {}
            if include_specifications:
                for r in db.execute('SELECT record_id,name,value FROM facts WHERE name IN (' +
                                    ','.join('?' for _ in CATEGORIES) + ') ORDER BY record_id,name,value', list(CATEGORIES)):
                    self.record_specs.setdefault(r['record_id'], []).append((r['name'], r['value']))
                for r in db.execute("SELECT record_id,value FROM identifiers WHERE kind='part_number' ORDER BY record_id,value"):
                    self.record_parts.setdefault(r['record_id'], []).append(r['value'])
            terms = {}
            for r in db.execute('SELECT r.id,r.device_id,r.source_id,r.source_url,s.tier, '
                                's.sha256 AS source_sha256,s.fetched_at AS source_fetched_at '
                                'FROM records r JOIN sources s ON s.id=r.source_id ORDER BY r.id'):
                evidence = dict(r)
                evidence['record_id'] = evidence.pop('id')
                evidence['affiliations'] = affiliations.get(evidence['record_id'], [])
                if target and not any(a['target'] == target for a in evidence['affiliations']):
                    continue
                device = self.devices[evidence['device_id']]
                evidence['software_reference'] = is_software_reference(device['brand'], device['kind'])
                if evidence['software_reference'] and not self.include_software_references:
                    continue
                values = [('model', device['model'])]
                values.extend((i['kind'], i['value']) for i in db.execute(
                    'SELECT kind,value FROM identifiers WHERE record_id=? ORDER BY kind,value',
                    (evidence['record_id'],)) if i['kind'] in KINDS)
                for kind, value in values:
                    term = normalized(value)
                    if usable(term, self.include_short_models, self.include_numeric_identifiers):
                        terms.setdefault(term, []).append(dict(evidence, identifier_kind=kind, identifier=value))
            self.terms = [(term, pattern(term), evidence) for term, evidence in sorted(terms.items())]
            # Narrow buyer-audit F13 rule: only model fields of unattributed FRUs.
            # Keep this record-scoped even when a server or alias has the same text.
            self.accessory_revision_records = {
                e['record_id'] for entries in terms.values() for e in entries
                if self.devices[e['device_id']]['brand'] == 'Unattributed accessory'
                and re.search(r'\bfrus?\b', (self.devices[e['device_id']]['kind'] or '').casefold())
            }
            self.accessory_variants = {}
            for term, entries in terms.items():
                if any(e['identifier_kind'] == 'model' and e['record_id'] in self.accessory_revision_records
                       for e in entries):
                    revision_pattern = accessory_revision_pattern(term)
                    if revision_pattern:
                        base = term.split(' ', 1)[0].rstrip(',')
                        self.accessory_variants[term] = (pattern(base), revision_pattern)
            # Token-based suffix comparisons cannot see identifiers containing
            # spaces. Preserve the entire phrase and vary only its final code.
            self.phrase_variants = {}
            for term in terms:
                if ' ' not in term or not any(c.isdigit() for c in term):
                    continue
                # One delimiter and a flat suffix alphabet avoid ambiguous
                # nested repetition on long seller-supplied titles.
                longer = re.compile(r'(?<![\w+-])' + re.escape(term) + r'[-+][\w+-]*(?![\w+-])')
                shorter = []
                for index in range(term.rfind(' ') + 1, len(term)):
                    prefix = term[:index]
                    if (term[index] in '-+' and any(c.isdigit() for c in prefix)
                            and usable(prefix, self.include_short_models, self.include_numeric_identifiers)):
                        shorter.append((prefix, pattern(prefix)))
                self.phrase_variants[term] = (longer, shorter)
        finally:
            if own_transaction:
                db.rollback()

    def match(self, title, limit=20):
        if not isinstance(title, str) or not title.strip() or len(title) > 4096:
            raise ValueError('Listing title must contain 1 to 4096 characters')
        if type(limit) is not int or limit < -1:
            raise ValueError('Limit must be -1 or nonnegative')
        text = normalized(title)
        tokens = set(re.findall(r'[\w]+(?:-[\w]+|\+[\w]*)*', text))
        # Prefix-related tokens are leads only. Never strip a known or unknown suffix.
        tokens = {t for t in tokens if usable(t, self.include_short_models, self.include_numeric_identifiers)
                  and any(c.isdigit() for c in t)}
        matches = {}
        for term, regex, evidence in self.terms:
            exact = bool(regex.search(text))
            related = [] if exact else sorted(t for t in tokens if any(
                t.startswith(term + suffix) or term.startswith(t + suffix) for suffix in '-+'))
            if not exact and term in self.phrase_variants:
                longer, shorter = self.phrase_variants[term]
                related = sorted(set(longer.findall(text)) |
                                 {prefix for prefix, prefix_pattern in shorter if prefix_pattern.search(text)})
            accessory_related = []
            if not exact and term in self.accessory_variants:
                accessory_related = sorted({m.group() for p in self.accessory_variants[term]
                                            for m in p.finditer(text)})
            if not exact and not related and not accessory_related:
                continue
            for e in evidence:
                record_related = related
                if accessory_related and e['identifier_kind'] == 'model' and e['record_id'] in self.accessory_revision_records:
                    record_related = sorted(set(related) | set(accessory_related))
                if not exact and not record_related:
                    continue
                kind = ('model_mention' if e['identifier_kind'] == 'model' else 'identifier_mention') if exact else 'variant_lead'
                if e['software_reference']:
                    kind = SOFTWARE_MATCH
                did = e['device_id']
                result = matches.setdefault(did, {'device_id': did, 'brand': self.devices[did]['brand'],
                    'model': self.devices[did]['model'], 'revision': self.devices[did]['revision'], 'matches': []})
                match = dict(e, match_type=kind, related_title_tokens=record_related)
                if self.include_specifications:
                    # A variant lead is not an identity match. Never attach family
                    # or sibling SKU statements to an unmatched ordering code.
                    specs = self.record_specs.get(e['record_id'], ()) if exact else ()
                    match['specifications'] = [{'category': k, 'statement': v} for k, v in specs]
                    match['record_part_numbers'] = list(self.record_parts.get(e['record_id'], ()))
                result['matches'].append(match)
        order = {'model_mention': 0, 'identifier_mention': 1, 'variant_lead': 2, SOFTWARE_MATCH: 3}
        results = []
        for result in matches.values():
            result['matches'].sort(key=lambda m: (order[m['match_type']], m['source_id'], m['record_id'], m['identifier_kind'], m['identifier']))
            result['best_match'] = result['matches'][0]['match_type']
            results.append(result)
        results.sort(key=lambda r: (order[r['best_match']], r['brand'], r['model'], r['revision'], r['device_id']))
        shown = results if limit == -1 else results[:limit]
        if self.families is not None:
            # Display-only hint, computed over ALL candidates (before the limit); no candidate is
            # added, removed or reordered, and best_match/status/candidate_counts are untouched.
            by_family = {}
            for r in results:
                fid = self.families.family_of(r['brand'])
                r['brand_family'] = fid
                if fid:
                    by_family[fid] = by_family.get(fid, 0) + 1
            for r in results:
                r['same_family_candidates'] = by_family[r['brand_family']] - 1 if r['brand_family'] else 0
        counts = {kind: sum(r['best_match'] == kind for r in results) for kind in order
                  if kind != SOFTWARE_MATCH or self.include_software_references}
        result = {'title': title, 'scope': SCOPE, 'context_terms': [v for v in CONTEXT if pattern(v).search(text)],
                'short_identifiers_included': self.include_short_models,
                'numeric_identifiers_included': self.include_numeric_identifiers,
                'matches': len(results), 'shown': len(shown), 'truncated': len(results) > len(shown),
                'candidate_counts': counts,
                'status': ('mentions_found' if counts['model_mention'] or counts['identifier_mention'] else
                           'variant_leads_only' if any(r['best_match'] == 'variant_lead' for r in results) else
                           'software_references_only' if results else 'no_catalog_mention'), 'candidates': shown}
        if self.families is not None:
            result['brand_families'] = dict(self.families.metadata(), scope=FAMILY_SCOPE)
        if self.include_software_references:
            result['software_references_included'] = True
        if self.include_specifications:
            result['specifications_included'] = True
            result['specification_scope'] = SPECIFICATION_SCOPE
        return result


def read_csv(path, title_column='title'):
    path = Path(path)
    if path.stat().st_size > 20_000_000:
        raise ValueError('Listing CSV exceeds 20 MB')
    rows = []
    with path.open(encoding='utf-8-sig', newline='') as stream:
        reader = csv.DictReader(stream)
        if (not reader.fieldnames or len(set(reader.fieldnames)) != len(reader.fieldnames)
                or title_column not in reader.fieldnames):
            raise ValueError('Listing CSV requires unique headers and a ' + title_column + ' column')
        for number, row in enumerate(reader, 1):
            if None in row or any(v is None for v in row.values()):
                raise ValueError('Malformed listing CSV record ' + str(number))
            title = row[title_column]
            if not title.strip() or len(title) > 4096:
                raise ValueError('Invalid title in listing CSV record ' + str(number))
            if number > 5000:
                raise ValueError('Listing CSV exceeds 5000 records')
            rows.append({'input_record': number, 'input': row, 'title': title})
    if not rows:
        raise ValueError('Listing CSV has no records')
    return rows


def flat_rows(result, input_record=1):
    base = {'input_record': input_record, 'title': result['title'], 'context_terms': ', '.join(result['context_terms']),
            'short_identifiers_included': result['short_identifiers_included'],
            'numeric_identifiers_included': result['numeric_identifiers_included'],
            'total_candidates': result['matches'], 'truncated': result['truncated']}
    if not result['candidates']:
        rows = [dict(base, match_type=result['status'], brand='', model='', device_id='', source_ids='', evidence_tiers='')]
    else:
        rows = [dict(base, match_type=r['best_match'], brand=r['brand'], model=r['model'], device_id=r['device_id'],
                 source_ids=', '.join(sorted({m['source_id'] for m in r['matches'] if m['match_type'] == r['best_match']})),
                 evidence_tiers=', '.join(sorted({m['tier'] for m in r['matches'] if m['match_type'] == r['best_match']}))) for r in result['candidates']]
    if 'brand_families' in result:
        for i, row in enumerate(rows):
            candidate = result['candidates'][i] if result['candidates'] else {}
            row['brand_family'] = candidate.get('brand_family') or ''
            row['same_family_candidates'] = candidate.get('same_family_candidates', 0)
    if result.get('specifications_included'):
        for i, row in enumerate(rows):
            row['specifications_included'] = True
            row['revision'] = result['candidates'][i]['revision'] if result['candidates'] else ''
            statements = []
            # A candidate may mention multiple identifiers from the same record.
            # Preserve the matched identifiers without duplicating its statements.
            grouped = {}
            matches = result['candidates'][i]['matches'] if result['candidates'] else ()
            for match in matches:
                if not match['specifications']:
                    continue
                rid = match['record_id']
                if rid not in grouped:
                    grouped[rid] = {k: match[k] for k in ('record_id', 'source_id', 'source_url', 'tier',
                                                         'source_sha256', 'affiliations', 'record_part_numbers', 'specifications')}
                    grouped[rid]['matched_identifiers'] = []
                    statements.append(grouped[rid])
                grouped[rid]['matched_identifiers'].append({k:match[k] for k in ('identifier_kind', 'identifier', 'match_type')})
            row['specifications_json'] = json.dumps(statements, ensure_ascii=False)
    return rows
