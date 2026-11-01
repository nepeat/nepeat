"""Offline PDF page search and FCC exhibit import; never implicit model evidence."""
import json
import re
from pathlib import Path

from .catalog import digest
from .fcc_index import BASE, document


def index_documents(body, fcc_id):
    root = document(body.decode('utf-8'))
    headings = list(root.walk('h1'))
    if len(headings) != 1 or not re.match(re.escape(fcc_id) + r'(?:\s|$)', headings[0].text()):
        raise ValueError('FCC document index identity changed')
    rows = []
    for a in root.walk('a'):
        href = a.attrs.get('href', '')
        if not href.startswith('/api/files/'):
            continue
        # This archive format currently supports numeric document IDs exposed
        # by the mirror's s1 file links, not inferred IDs for other URL forms.
        match = re.fullmatch(r'/api/files/s1/([a-z0-9_]+)_(\d{7})_([0-9a-f]+)\.pdf', href)
        if not match or not match.group(1).startswith(fcc_id.lower().replace('-', '_') + '_'):
            raise ValueError('Unrecognized FCC PDF link')
        parent = a.parent
        category = None
        while parent:
            headings = [n.text() for n in parent.children if hasattr(n, 'tag') and n.tag in ('h2', 'h3', 'h4')]
            if len(headings) == 1:
                category = headings[0]
                break
            parent = parent.parent
        title = a.attrs.get('title', '')
        if not category or not title.startswith('Open '):
            raise ValueError('Missing FCC exhibit category or title')
        rows.append({'document_id': match.group(2), 'title': title[5:], 'category': category,
                     'source_url': BASE + href, 'pdf_file': href.rsplit('/', 1)[1]})
    if not rows or len({r['document_id'] for r in rows}) != len(rows):
        raise ValueError('Empty or duplicated FCC document index')
    return rows


def dependency(root, entry):
    name = entry['path']
    if not isinstance(name, str) or Path(name).name != name or name in ('', '.', '..'):
        raise ValueError('Document dependency must be a sibling filename')
    p = (root / name).resolve()
    if p.parent != root.resolve():
        raise ValueError('Document dependency escapes bundle')
    body = p.read_bytes()
    if digest(body) != entry['sha256']:
        raise ValueError('Document checksum mismatch: ' + name)
    return body


PYPDF_METHOD = 'pypdf embedded text; no OCR'
POPPLER_METHOD = 'Poppler pdftotext -layout embedded text; no OCR'
POPPLER_OPTIONS = {
    'layout': True, 'encoding': 'UTF-8', 'eol': 'unix',
    'remove_hyphens': 'none', 'page_selection': 'individual', 'page_breaks': False,
}


def read_capture(body, pdf_sha256):
    """Validate offline capture provenance; never invoke an extractor."""
    data = json.loads(body)
    if not isinstance(data, dict):
        raise ValueError('Invalid PDF text capture or source association')
    pages = data.get('pages')
    method = data.get('extraction_method')
    if (not isinstance(pdf_sha256, str) or not re.fullmatch(r'[0-9a-f]{64}', pdf_sha256)
            or data.get('pdf_sha256') != pdf_sha256
            or method not in (PYPDF_METHOD, POPPLER_METHOD)
            or not isinstance(pages, list) or not pages):
        raise ValueError('Invalid PDF text capture or source association')
    for n, p in enumerate(pages, 1):
        if (not isinstance(p, dict) or type(p.get('page')) is not int
                or p['page'] != n or not isinstance(p.get('text'), str)):
            raise ValueError('Missing, reordered or malformed PDF text page')
    metadata = {'extraction_method': method}
    if method == POPPLER_METHOD:
        version = data.get('extractor_version')
        options = data.get('extraction_options')
        if (not isinstance(version, str) or not re.fullmatch(r'\d+\.\d+(?:\.\d+)?', version)
                or data.get('pdfinfo_version') != version
                or type(data.get('pdf_page_count')) is not int
                or data['pdf_page_count'] != len(pages)
                or not isinstance(options, dict) or options != POPPLER_OPTIONS
                or any(type(options.get(key)) is not type(value)
                       for key, value in POPPLER_OPTIONS.items())):
            raise ValueError('Invalid Poppler PDF extraction metadata')
        metadata.update(extractor_version=version, pdfinfo_version=version,
                        pdf_page_count=len(pages), extraction_options=dict(options))
    return pages, metadata


def read_pages(body, pdf_sha256):
    """Preserve the historical page-list API for source parsers and lineage."""
    return read_capture(body, pdf_sha256)[0]


def reindex(db):
    """Stable row IDs regardless of which document collection was refreshed last."""
    db.execute('DELETE FROM document_search')
    db.execute('INSERT INTO document_search SELECT document_key,page_number,text '
               'FROM document_pages ORDER BY document_key,page_number')


def import_bundle(c, path):
    path = Path(path).resolve()
    data = json.loads(path.read_text())
    fid = data.get('fcc_id', '')
    if (data.get('schema_version') != 1 or not re.fullmatch(r'[A-Z0-9][A-Z0-9-]{3,31}', fid)
            or data.get('index_url') != BASE + '/fcc/' + fid):
        raise ValueError('Unrecognized FCC PDF archive')
    index = index_documents(dependency(path.parent, data['index']), fid)
    entries = data['documents']
    if set(entries) != {r['document_id'] for r in index}:
        raise ValueError('FCC archive does not match all public index links')
    ready = []
    for item in index:
        entry = entries[item['document_id']]
        if entry['pdf']['path'] != item['pdf_file']:
            raise ValueError('PDF filename does not match index link')
        pdf = dependency(path.parent, entry['pdf'])
        if not pdf.startswith(b'%PDF-'):
            raise ValueError('Cached exhibit is not a PDF')
        pages, metadata = read_capture(dependency(path.parent, entry['text']), digest(pdf))
        if type(entry['page_count']) is not int or len(pages) != entry['page_count']:
            raise ValueError('PDF page count differs from extraction capture')
        ready.append((item, dict(entry, extraction_method=metadata['extraction_method']), pages))
    sid = 'fcc-documents-' + fid.lower()
    c.source(sid, data['index_url'], 'primary_document_mirror',
             'Public exhibit copies; original document rights retained',
             'All publicly linked PDFs on this cached mirror detail page, not all FCC filings or restricted exhibits. '
             'Embedded text only, no OCR; images and diagrams require reading the local PDF. '
             'Document text is discovery evidence and does not establish model aliases, factories or deployment. '
             'No inferred grant dates from mirror display dates.', path)
    c.db.execute('DELETE FROM document_search WHERE document_key IN (SELECT document_key FROM documents WHERE source_id=?)', (sid,))
    c.db.execute('DELETE FROM documents WHERE source_id=?', (sid,))
    for item, entry, pages in ready:
        key = digest((sid + ':' + item['document_id']).encode())[:24]
        with_text = sum(bool(p['text'].strip()) for p in pages)
        c.db.execute('INSERT INTO documents VALUES (?,?,?,?,?,?,?,?,?,?,?,?)',
                     (key, sid, fid, item['document_id'], item['title'], item['category'], item['source_url'],
                      item['pdf_file'], entry['pdf']['sha256'], len(pages), with_text, entry['extraction_method']))
        for p in pages:
            status = 'embedded_text' if p['text'].strip() else 'no_embedded_text'
            c.db.execute('INSERT INTO document_pages VALUES (?,?,?,?)', (key, p['page'], p['text'], status))
    reindex(c.db)
    c.db.execute('UPDATE sources SET records_seen=? WHERE id=?', (len(ready), sid))
    c.db.commit()
    return {'documents': len(ready), 'pages': sum(len(p) for _, _, p in ready),
            'pages_with_text': sum(bool(x['text'].strip()) for _, _, p in ready for x in p)}


def search(db, query='', fcc_id=None, limit=20, source=None):
    if type(limit) is not int or limit < -1:
        raise ValueError('Limit must be -1 or nonnegative')
    own_transaction = not db.in_transaction
    if own_transaction:
        db.execute('BEGIN')
    try:
        if not db.execute("SELECT 1 FROM sqlite_master WHERE type='table' AND name='documents'").fetchone():
            return {'state': 'not_loaded', 'matches': 0, 'rows': []}
        clauses, args = [], []
        if fcc_id is not None:
            clauses.append('d.fcc_id=?'); args.append(fcc_id.strip().upper())
        if source is not None:
            clauses.append('d.source_id=?'); args.append(source)
        tokens = re.findall(r'\w+', query)
        if query.strip() and not tokens:
            clauses.append('0')
        if tokens:
            clauses.append('document_search MATCH ?')
            args.append(' AND '.join('"' + t + '"*' for t in tokens))
        sql = (' FROM document_search JOIN documents d ON d.document_key=document_search.document_key '
               'JOIN document_pages p ON p.document_key=d.document_key AND p.page_number=document_search.page_number '
               'JOIN sources s ON s.id=d.source_id')
        if clauses: sql += ' WHERE ' + ' AND '.join(clauses)
        total = db.execute('SELECT count(*)' + sql, args).fetchone()[0]
        rows = [dict(r) for r in db.execute(
            'SELECT d.*,p.page_number,p.text_status,s.cache_path,'
            "snippet(document_search,2,'[',']',' … ',36) AS excerpt" + sql +
            ' ORDER BY d.source_id,d.document_id,p.page_number LIMIT ?', args + [limit])]
        for r in rows:
            r['local_pdf'] = str((Path(r.pop('cache_path')).resolve().parent / r['pdf_file']).resolve())
        loaded = db.execute("SELECT 1 FROM sources WHERE id LIKE 'fcc-documents-%' "
                            "OR id LIKE 'reference-pdf-%' LIMIT 1").fetchone()
        return {'state': 'loaded' if loaded else 'not_loaded', 'query': query, 'fcc_id': fcc_id,
                'source': source,
                'matches': total, 'shown': len(rows), 'truncated': total > len(rows), 'rows': rows,
                'scope': 'Page text matches, not independent model corroboration. No OCR; image content can be missing '
                         'even on pages with some text. Blank-text pages remain in the archive. '
                         'An empty query lists all pages including pages without embedded text.'}
    finally:
        if own_transaction:
            db.rollback()
