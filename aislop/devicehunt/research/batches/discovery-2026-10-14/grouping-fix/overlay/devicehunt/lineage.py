"""Frozen, offline evidence releases; accepted lineage never reads a mutable queue."""
import hashlib
import json
import re
from pathlib import Path

TABLES = ('archive_releases', 'archive_artifacts', 'archive_release_artifacts',
          'archive_receipts', 'archive_release_receipts', 'evidence_statements',
          'evidence_reviews', 'evidence_links', 'archive_release_statements')


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=False)


def identity(value):
    return hashlib.sha256(canonical(value).encode()).hexdigest()


def checksum(path):
    result = hashlib.sha256()
    with Path(path).open('rb') as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b''):
            result.update(chunk)
    return result.hexdigest()


def require(condition, message):
    if not condition:
        raise ValueError(message)


def local(root, name):
    require(isinstance(name, str) and name and not Path(name).is_absolute()
            and '..' not in Path(name).parts, 'Invalid evidence path')
    root = Path(root).resolve()
    target = root / name
    require(not any(p.is_symlink() for p in [target, *target.parents] if p != root),
            'Evidence symlinks are not allowed')
    require(target.resolve().is_relative_to(root), 'Evidence escapes data root')
    return target


def indexed(rows, key):
    require(isinstance(rows, list), 'Release array required')
    result = {}
    for row in rows:
        require(isinstance(row, dict) and isinstance(row.get(key), str) and row[key],
                'Invalid release identity')
        require(row[key] not in result, 'Duplicate release identity: ' + row[key])
        result[row[key]] = row
    return result


def projection_exists(db, statement):
    p = statement['projection']
    base = ' FROM records r JOIN %s v ON v.record_id=r.id WHERE r.source_id=? AND r.source_key=?'
    args = [statement['source_id'], statement['source_key']]
    if statement['projection_kind'] == 'fact':
        sql = 'SELECT 1' + (base % 'facts') + ' AND v.name=? AND v.value=?'
        args += [p['name'], p['value']]
    else:
        sql = 'SELECT 1' + (base % 'affiliations') + ' AND v.company=? AND v.role=?'
        args += [p['company'], p['role']]
    return db.execute(sql, args).fetchone() is not None


def prepare(path, db=None):
    """Validate closure, saved bytes, selectors and review mapping before SQL writes."""
    path = Path(path).resolve()
    data = json.loads(path.read_bytes())
    require(data.get('schema_version') == 1, 'Unsupported evidence release')
    release_id = data.get('release_id')
    unsigned = {k: v for k, v in data.items() if k != 'release_id'}
    require(release_id == identity(unsigned), 'Evidence release identity mismatch')
    artifacts = indexed(data['artifacts'], 'sha256')
    require(bool(artifacts), 'Empty evidence release')
    for sha, obj in artifacts.items():
        require(re.fullmatch('[0-9a-f]{64}', sha) and type(obj['byte_count']) is int,
                'Invalid artifact identity')
        require(isinstance(obj['paths'], list) and obj['paths'], 'Missing artifact locations')
        for name in obj['paths']:
            p = local(path.parent, name)
            require(p.stat().st_size == obj['byte_count'] and checksum(p) == sha,
                    'Evidence artifact checksum mismatch: ' + name)
    receipts = indexed(data['receipts'], 'id')
    for receipt in receipts.values():
        require(isinstance(receipt['kind'], str) and isinstance(receipt['payload'], dict),
                'Invalid receipt')
        require(set(receipt['artifact_refs']) <= artifacts.keys(), 'Missing receipt artifact')
        require(set(receipt['receipt_refs']) <= receipts.keys(), 'Missing receipt dependency')
    visited, visiting = set(), set()
    def visit(rid):
        require(rid not in visiting, 'Cyclic receipt lineage')
        if rid in visited:
            return
        visiting.add(rid)
        for child in receipts[rid]['receipt_refs']:
            visit(child)
        visiting.remove(rid)
        visited.add(rid)
    for rid in receipts:
        visit(rid)
    statements = indexed(data['statements'], 'statement_id')
    reviews = indexed(data['reviews'], 'review_id')
    links = indexed(data['links'], 'link_id')
    for statement in statements.values():
        require(statement['statement_id'] == identity({k: v for k, v in statement.items()
                    if k not in ('statement_id', 'accepted_review_id')}), 'Statement revision mismatch')
        require(statement['projection_kind'] in ('fact', 'affiliation'), 'Invalid projection kind')
        for name in ('claim_id', 'source_id', 'source_key', 'predicate', 'raw_value', 'role'):
            require(isinstance(statement[name], str) and statement[name], 'Missing statement field')
        review = reviews.get(statement['accepted_review_id'])
        require(review and review['statement_id'] == statement['statement_id'] and
                review['decision'] == 'accepted', 'Statement is not explicitly accepted')
        require(any(x['statement_id'] == statement['statement_id'] for x in links.values()),
                'Accepted statement has no evidence')
        if db is not None:
            require(projection_exists(db, statement), 'Accepted projection not present in catalog')
    for review in reviews.values():
        require(review['statement_id'] in statements and review['decision'] in
                ('accepted', 'rejected', 'superseded') and review['reviewer'] and review['reason'],
                'Invalid review decision')
    for link in links.values():
        require(link['statement_id'] in statements and link['artifact_sha256'] in artifacts
                and link['receipt_id'] in receipts, 'Dangling evidence link')
        locator = link['locator']
        require(locator['kind'] in ('pdf_page', 'json_pointer', 'text_lines'), 'Invalid selector kind')
        closure_hashes, closure_seen = set(), set()
        def evidence_closure(rid):
            if rid in closure_seen:
                return
            closure_seen.add(rid)
            closure_hashes.update(receipts[rid]['artifact_refs'])
            for parent in receipts[rid]['receipt_refs']:
                evidence_closure(parent)
        evidence_closure(link['receipt_id'])
        require(link['artifact_sha256'] in closure_hashes, 'Evidence is unrelated to receipt lineage')
        artifact = artifacts[link['artifact_sha256']]
        original = local(path.parent, artifact['paths'][0])
        if locator['kind'] == 'pdf_page':
            text_sha = locator['text_sha256']
            require(text_sha in artifacts, 'Missing page text artifact')
            require(text_sha in closure_hashes, 'Page text is unrelated to receipt lineage')
            from .documents import read_pages
            require(original.read_bytes().startswith(b'%PDF-'), 'PDF selector requires PDF bytes')
            text_path = local(path.parent, artifacts[text_sha]['paths'][0])
            pages = read_pages(text_path.read_bytes(), link['artifact_sha256'])
            page = locator['page']
            require(type(page) is int and 1 <= page <= len(pages), 'Invalid PDF page ordinal')
            selected = pages[page - 1]['text']
        elif locator['kind'] == 'json_pointer':
            selected = json.loads(original.read_bytes())
            pointer = locator['pointer']
            require(isinstance(pointer, str) and (not pointer or pointer.startswith('/')),
                    'Invalid JSON pointer')
            for segment in pointer.split('/')[1:] if pointer else []:
                require(not re.search(r'~(?![01])', segment), 'Invalid JSON pointer escape')
                key = segment.replace('~1', '/').replace('~0', '~')
                if isinstance(selected, list):
                    require(re.fullmatch(r'0|[1-9][0-9]*', key), 'Invalid JSON array index')
                    selected = selected[int(key)]
                else:
                    selected = selected[key]
            selected = selected if isinstance(selected, str) else canonical(selected)
        else:
            lines = original.read_text().splitlines()
            start, end = locator['start'], locator['end']
            require(type(start) is int and type(end) is int and 1 <= start <= end <= len(lines),
                    'Invalid line selector')
            selected = '\n'.join(lines[start - 1:end])
        # Whitespace can differ between visual transcription and embedded extraction;
        # original quote stays literal. No fuzzy identifier or semantic matching.
        require(isinstance(link['excerpt'], str) and bool(link['excerpt'].strip()) and
                ' '.join(link['excerpt'].split()) in ' '.join(selected.split()),
                'Evidence excerpt absent from selected saved text')
    return data


def insert_immutable(db, table, values):
    columns = [r[1] for r in db.execute('PRAGMA table_info(' + table + ')')]
    # Every table has a declared primary key, including membership tables.
    info = list(db.execute('PRAGMA table_info(' + table + ')'))
    keys = [i for i, r in enumerate(info) if r[5]]
    where = ' AND '.join(columns[i] + '=?' for i in keys)
    existing = db.execute('SELECT * FROM ' + table + ' WHERE ' + where,
                          [values[i] for i in keys]).fetchone()
    if existing is not None:
        require(tuple(existing) == tuple(values), 'Immutable lineage collision in ' + table)
    else:
        db.execute('INSERT INTO ' + table + ' VALUES (' + ','.join('?' for _ in values) + ')', values)


def import_bundle(c, path):
    data = prepare(path, c.db)
    rid = data['release_id']
    db = c.db
    db.execute('SAVEPOINT evidence_release')
    try:
        insert_immutable(db, 'archive_releases', (rid, checksum(path), canonical(data)))
        for obj in data['artifacts']:
            insert_immutable(db, 'archive_artifacts', (obj['sha256'], obj['byte_count']))
            for name in obj['paths']:
                insert_immutable(db, 'archive_release_artifacts', (rid, obj['sha256'], name))
        for receipt in data['receipts']:
            insert_immutable(db, 'archive_receipts', (receipt['id'], receipt['kind'],
                             receipt.get('created_at'), canonical(receipt)))
            insert_immutable(db, 'archive_release_receipts', (rid, receipt['id']))
        for s in data['statements']:
            insert_immutable(db, 'evidence_statements', tuple(s[k] for k in
                ('statement_id', 'claim_id', 'source_id', 'source_key', 'predicate',
                 'raw_value', 'normalized_value', 'role', 'projection_kind')) +
                (canonical(s['projection']),))
        for review in data['reviews']:
            insert_immutable(db, 'evidence_reviews', tuple(review[k] for k in
                ('review_id', 'statement_id', 'decision', 'reviewer', 'reviewed_at', 'reason')))
        for link in data['links']:
            insert_immutable(db, 'evidence_links', tuple(link[k] for k in
                ('link_id', 'statement_id', 'artifact_sha256', 'receipt_id')) +
                (canonical(link['locator']), link['excerpt']))
        for s in data['statements']:
            insert_immutable(db, 'archive_release_statements',
                             (rid, s['statement_id'], s['accepted_review_id']))
        db.execute('RELEASE evidence_release')
    except BaseException:
        db.execute('ROLLBACK TO evidence_release')
        db.execute('RELEASE evidence_release')
        raise
    return {'release_id': rid, 'artifacts': len(data['artifacts']),
            'receipts': len(data['receipts']), 'statements': len(data['statements'])}


def show(db, query, data_root):
    """Resolve all matching claim revisions and evidence from accepted SQL only."""
    rows = db.execute('SELECT * FROM evidence_statements WHERE claim_id=? OR statement_id=? '
                      'OR source_key=? ORDER BY claim_id,statement_id', (query, query, query))
    results = []
    for row in rows:
        statement = dict(row)
        statement['projection'] = json.loads(statement.pop('projection_json'))
        statement['projection_present'] = projection_exists(db, statement)
        statement['reviews'] = [dict(r) for r in db.execute(
            'SELECT * FROM evidence_reviews WHERE statement_id=? ORDER BY review_id',
            (statement['statement_id'],))]
        statement['evidence'] = []
        for link in db.execute('SELECT * FROM evidence_links WHERE statement_id=? ORDER BY link_id',
                               (statement['statement_id'],)):
            item = dict(link)
            item['locator'] = json.loads(item.pop('locator_json'))
            names = [r[0] for r in db.execute('SELECT DISTINCT path FROM archive_release_artifacts '
                                             'WHERE sha256=? ORDER BY path', (item['artifact_sha256'],))]
            item['local_artifacts'] = []
            for name in names:
                p = local(data_root, name)
                require(checksum(p) == item['artifact_sha256'], 'Evidence artifact checksum mismatch')
                item['local_artifacts'].append(str(p))
            closure = {}
            def load(rid):
                if rid in closure:
                    return
                r = db.execute('SELECT payload_json FROM archive_receipts WHERE receipt_id=?', (rid,)).fetchone()
                require(r is not None, 'Missing accepted receipt')
                receipt = json.loads(r[0])
                closure[rid] = receipt
                for dep in receipt['receipt_refs']:
                    load(dep)
            load(item['receipt_id'])
            item['lineage'] = list(closure.values())
            required = {item['artifact_sha256']}
            required.update(sha for receipt in closure.values() for sha in receipt['artifact_refs'])
            if item['locator']['kind'] == 'pdf_page':
                required.add(item['locator']['text_sha256'])
            item['verified_lineage_artifacts'] = []
            for sha in sorted(required):
                locations = list(db.execute('SELECT DISTINCT path FROM archive_release_artifacts '
                                            'WHERE sha256=? ORDER BY path', (sha,)))
                require(bool(locations), 'Missing accepted artifact locations')
                for name, in locations:
                    require(checksum(local(data_root, name)) == sha,
                            'Evidence lineage artifact checksum mismatch')
                item['verified_lineage_artifacts'].append(sha)
            statement['evidence'].append(item)
        results.append(statement)
    return {'query': query, 'statements': results,
            'scope': 'Accepted source-local revisions; conflicts remain separate. '
                     'No auction configuration, manufacture or deployment inferred.'}
