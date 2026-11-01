"""Archive reviewed local reference PDFs without creating device assertions."""
import json
import re
from pathlib import Path
from urllib.parse import urlsplit

from .catalog import digest
from .documents import dependency, read_capture, reindex

COLLECTION = 'public-device-references-v1'
PREFIX = 'reference-pdf-'


def prepare(path):
    path = Path(path).resolve()
    data = json.loads(path.read_text())
    entries = data.get('documents')
    if (data.get('schema_version') != 1 or data.get('collection') != COLLECTION
            or not isinstance(entries, list) or not entries or len(entries) > 1000):
        raise ValueError('Invalid reference document collection')
    seen = {key: set() for key in ('id', 'url', 'pdf', 'sha256')}
    ready = []
    for entry in entries:
        if not isinstance(entry, dict) or not re.fullmatch(r'[a-z0-9]+(?:-[a-z0-9]+)*', entry.get('id', '')):
            raise ValueError('Invalid reference document identifier')
        url = urlsplit(entry.get('url', ''))
        if url.scheme != 'https' or not url.netloc or url.username or url.password or url.fragment:
            raise ValueError('Reference source requires an original HTTPS URL without a fragment')
        if entry.get('tier') not in ('vendor', 'regulatory', 'certification_body'):
            raise ValueError('Unsupported reference provenance tier')
        for key in ('title', 'category', 'scope'):
            if not isinstance(entry.get(key), str) or not entry[key].strip() or len(entry[key]) > 4000:
                raise ValueError('Missing or oversized reference metadata: ' + key)
        pdf = dependency(path.parent, entry['pdf'])
        if not pdf.startswith(b'%PDF-'):
            raise ValueError('Reference document is not a PDF')
        pages, metadata = read_capture(dependency(path.parent, entry['text']), digest(pdf))
        if type(entry.get('page_count')) is not int or entry['page_count'] != len(pages):
            raise ValueError('Reference PDF page count changed')
        values = {'id': entry['id'], 'url': entry['url'], 'pdf': entry['pdf']['path'], 'sha256': digest(pdf)}
        for key, value in values.items():
            if value in seen[key]:
                raise ValueError('Duplicate reference document ' + key)
            seen[key].add(value)
        ready.append((dict(entry, extraction_method=metadata['extraction_method']), pages))
    return path, ready


def import_bundle(c, path):
    path, ready = prepare(path)  # Complete collection checked before removing old evidence.
    current = {PREFIX + entry['id'] for entry, _ in ready}
    old = {r[0] for r in c.db.execute('SELECT id FROM sources WHERE id GLOB ?', (PREFIX + '*',))}
    for sid in sorted(old | current):
        c.db.execute('DELETE FROM documents WHERE source_id=?', (sid,))
        if sid in old - current:
            c.db.execute('DELETE FROM sources WHERE id=?', (sid,))
    for entry, pages in ready:
        sid = PREFIX + entry['id']
        c.source(sid, entry['url'], entry['tier'], 'Original document rights retained; local research archive',
                 entry['scope'] + ' Embedded page text only; no OCR. Image content may be absent even '
                 'on pages with some text. Search hits do not create identities, aliases, manufacturer '
                 'or deployment assertions. This source is a document archive, not a new retrieval.', path)
        key = digest((sid + ':' + entry['id']).encode())[:24]
        with_text = sum(bool(p['text'].strip()) for p in pages)
        # An empty FCC field explicitly means this archive does not assign an FCC ID.
        c.db.execute('INSERT INTO documents VALUES (?,?,?,?,?,?,?,?,?,?,?,?)',
                     (key, sid, '', entry['id'], entry['title'], entry['category'], entry['url'],
                      entry['pdf']['path'], entry['pdf']['sha256'], len(pages), with_text,
                      entry['extraction_method']))
        c.db.executemany('INSERT INTO document_pages VALUES (?,?,?,?)',
                        [(key, p['page'], p['text'], 'embedded_text' if p['text'].strip() else 'no_embedded_text') for p in pages])
        c.db.execute('UPDATE sources SET records_seen=1 WHERE id=?', (sid,))
    reindex(c.db)
    c.db.commit()
    return {'documents': len(ready), 'pages': sum(len(pages) for _, pages in ready),
            'pages_with_text': sum(bool(p['text'].strip()) for _, pages in ready for p in pages)}
