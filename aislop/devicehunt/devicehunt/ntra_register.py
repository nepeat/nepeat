"""Full NTRA positional register, separate from target-brand catalog evidence."""
import json
from pathlib import Path

from .catalog import digest
from .ntra import URL, PDF_SHA256

SOURCE = 'ntra-register-2026-01'
PAGE_COUNT = 760
LABELS = ['(No)', '(Product Description)', '(Brand)', '(Model)', '(linked models)']


def parse_pages(data, expected_pages=PAGE_COUNT):
    pages = data.get('pages')
    if not isinstance(pages, list) or len(pages) != expected_pages:
        raise ValueError('Incomplete NTRA page set')
    rows = []
    for number, page in enumerate(pages, 1):
        tables = page.get('tables')
        if page.get('page') != number or not isinstance(tables, list) or len(tables) != 1 or not tables[0]:
            raise ValueError('Missing, unordered or unexpected NTRA page tables')
        for position, cells in enumerate(tables[0], 1):
            if not isinstance(cells, list) or len(cells) != 5 or any(x is not None and not isinstance(x, str) for x in cells):
                raise ValueError('NTRA row has unexpected columns')
            if number == 1 and position == 1:
                if not all(label in (cell or '') for label, cell in zip(LABELS, cells)):
                    raise ValueError('NTRA column header changed')
                continue
            clean = [' '.join((x or '').split()) for x in cells]
            valid = clean[0].isascii() and clean[0].isdigit() and all(clean[1:4]) and not any('(cid:' in x for x in clean)
            fields = clean if valid else [None] * 5
            rows.append((number, 1, position, *fields, 'identified' if valid else 'review_required',
                         json.dumps(cells, ensure_ascii=False), URL + '#page=' + str(number)))
    return rows


def summary(rows):
    numbers = [json.loads(row[9])[0] for row in rows]
    numeric = [int(n.strip()) for n in numbers if isinstance(n, str) and n.strip().isascii() and n.strip().isdigit()]
    # These are PDF sequence numbers, never approval IDs or device counts.
    return {'rows': len(rows), 'identified': sum(row[8] == 'identified' for row in rows),
            'review_required': sum(row[8] == 'review_required' for row in rows),
            'sequence_is_contiguous': numeric == list(range(1, len(rows) + 1))}


def extract(pdf_path, tables_path):
    pdf_path = Path(pdf_path)
    if digest(pdf_path.read_bytes()) != PDF_SHA256:
        raise ValueError('NTRA PDF differs from reviewed snapshot')
    import pdfplumber
    pages = []
    with pdfplumber.open(pdf_path) as pdf:
        if len(pdf.pages) != PAGE_COUNT:
            raise ValueError('Unexpected NTRA page count')
        for n, page in enumerate(pdf.pages, 1):
            pages.append({'page': n, 'tables': page.extract_tables()})
            page.close()
    data = {'pages': pages}
    parse_pages(data)
    Path(tables_path).write_text(json.dumps(data, ensure_ascii=False))


def import_bundle(c, path):
    path = Path(path)
    data = json.loads(path.read_text())
    if (data.get('schema_version') != 1 or data.get('source_url') != URL
            or data.get('capture_format') != 'pdfplumber_positional_tables'):
        raise ValueError('Unrecognized NTRA register bundle')
    content = {}
    for key in ('pdf', 'tables'):
        entry = data[key]
        name = entry['path']
        if not isinstance(name, str) or Path(name).name != name or name in ('.', '..'):
            raise ValueError('NTRA dependency must be a sibling filename')
        file = (path.parent / name).resolve()
        if file.parent != path.parent.resolve():
            raise ValueError('NTRA dependency escapes bundle')
        content[key] = file.read_bytes()
        if digest(content[key]) != entry['sha256']:
            raise ValueError('NTRA checksum mismatch: ' + key)
    if data['pdf']['sha256'] != PDF_SHA256:
        raise ValueError('NTRA PDF differs from reviewed snapshot')
    rows = parse_pages(json.loads(content['tables']), expected_pages=PAGE_COUNT)
    counts = summary(rows)
    if counts != data.get('expected'):
        raise ValueError('NTRA extraction counts differ')
    c.source(SOURCE, URL, 'regulatory', 'NTRA factual public equipment register; source rights retained',
             'All 760 pages of the cached PDF extracted by table columns, including non-target brands. '
             'Incomplete or undecodable rows retain raw cells for review without asserting named fields. '
             'Brand is not physical manufacturer. Linked models and grouped cells are verbatim, not aliases. '
             'AWS labels do not alone establish the Amazon Data Services legal entity or unit provenance. '
             'Sequence numbers are not approval IDs. English/Arabic printed dates disagree; no current validity inferred.', path)
    c.db.execute('DELETE FROM ntra_equipment WHERE source_id=?', (SOURCE,))
    c.db.executemany('INSERT INTO ntra_equipment VALUES (?,?,?,?,?,?,?,?,?,?,?,?)',
                    [(SOURCE, *row) for row in rows])
    c.finish(SOURCE, len(rows))
    return {'pages': PAGE_COUNT, **counts}


def search(db, query='', limit=100):
    from .lookup import cells_text, contains
    if type(limit) is not int or limit < -1:
        raise ValueError('Limit must be -1 or nonnegative')
    db.create_function('dh_cells', 1, cells_text, deterministic=True)
    db.create_function('dh_contains', 2, contains, deterministic=True)
    return [dict(r) for r in db.execute(
        'SELECT * FROM ntra_equipment WHERE dh_contains(dh_cells(cells_json), ?) '
        'ORDER BY source_id,page,table_number,row_number LIMIT ?', (query.strip(), limit))]
