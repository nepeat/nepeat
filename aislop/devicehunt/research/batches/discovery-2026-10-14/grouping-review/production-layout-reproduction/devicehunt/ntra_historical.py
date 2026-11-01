"""December 2022 NTRA rows; keep the combined BRAND & MODEL cell unsplit."""
import json
from pathlib import Path

from .catalog import digest
from .documents import dependency

SOURCE = 'ntra-register-2022-12'
URL = 'https://www.tra.gov.eg/wp-content/uploads/2022/12/Type-Approved-List-December-2022-%D9%90.pdf'
PDF_SHA256 = 'da373d0fdd2f804f10e14570771896e4fbd4aa6ef582aa740cb06b7cc276dc3c'
PAGE_COUNT = 1218
ROW_COUNT = 29208
HEADER = ['No', 'TYPE', 'BRAND & MODEL']


def parse_pages(data, expected_pages=PAGE_COUNT, expected_rows=ROW_COUNT):
    pages = data.get('pages')
    if not isinstance(pages, list) or len(pages) != expected_pages:
        raise ValueError('Incomplete historical NTRA page set')
    rows = []
    for number, page in enumerate(pages, 1):
        tables = page.get('tables')
        if page.get('page') != number or not isinstance(tables, list) or len(tables) != 1 or not tables[0]:
            raise ValueError('Missing or reordered historical NTRA page table')
        for position, cells in enumerate(tables[0], 1):
            if not isinstance(cells, list) or len(cells) != 3 or any(x is not None and not isinstance(x, str) for x in cells):
                raise ValueError('Historical NTRA row must have three cells')
            clean = [' '.join((x or '').split()) for x in cells]
            if number == 1 and position == 1:
                if clean != HEADER:
                    raise ValueError('Historical NTRA column header changed')
                continue
            if not clean[0].isascii() or not clean[0].isdigit() or int(clean[0]) != len(rows) + 1:
                raise ValueError('Historical NTRA sequence is missing, duplicated or reordered')
            readable = all(clean) and not any('(cid:' in x for x in clean)
            # The historical source supplies one combined cell. Splitting at a
            # space would invent brand/model boundaries for multiword brands.
            rows.append((number, 1, position, clean[0], clean[1] if readable else None,
                         None, None, None, 'combined_brand_model' if readable else 'review_required',
                         json.dumps(cells, ensure_ascii=False), URL + '#page=' + str(number)))
    if len(rows) != expected_rows:
        raise ValueError('Historical NTRA row count changed')
    return rows


def summary(rows):
    return {'rows': len(rows), 'combined_brand_model': sum(r[8] == 'combined_brand_model' for r in rows),
            'review_required': sum(r[8] == 'review_required' for r in rows)}


def extract(pdf_path, tables_path):
    """Reproduce the reviewed positional capture locally; validate before writing."""
    pdf_path = Path(pdf_path)
    if digest(pdf_path.read_bytes()) != PDF_SHA256:
        raise ValueError('Historical NTRA PDF differs from reviewed snapshot')
    import pdfplumber
    pages = []
    with pdfplumber.open(pdf_path) as pdf:
        if len(pdf.pages) != PAGE_COUNT:
            raise ValueError('Unexpected historical NTRA page count')
        for number, page in enumerate(pdf.pages, 1):
            pages.append({'page': number, 'tables': page.extract_tables()})
            page.close()
    data = {'pages': pages}
    parse_pages(data, PAGE_COUNT, ROW_COUNT)
    Path(tables_path).write_text(json.dumps(data, ensure_ascii=False) + '\n')


def import_bundle(c, path):
    path = Path(path).resolve()
    data = json.loads(path.read_text())
    if (data.get('schema_version') != 1 or data.get('source_url') != URL
            or data.get('capture_format') != 'pdfplumber_positional_tables'):
        raise ValueError('Unrecognized historical NTRA bundle')
    pdf = dependency(path.parent, data['pdf'])
    if digest(pdf) != PDF_SHA256 or not pdf.startswith(b'%PDF-'):
        raise ValueError('Historical NTRA PDF differs from reviewed snapshot')
    rows = parse_pages(json.loads(dependency(path.parent, data['tables'])), PAGE_COUNT, ROW_COUNT)
    counts = summary(rows)
    if counts != data.get('expected'):
        raise ValueError('Historical NTRA extraction counts differ')
    c.source(SOURCE, URL, 'regulatory', 'NTRA public factual equipment register; source rights retained',
             'December 4, 2022 historical register. All 1,218 pages and 29,208 numbered rows. '
             'BRAND & MODEL is a combined source cell retained in cells_json; brand, model and linked_models '
             'are NULL rather than inferred. No catalog identities, approval IDs, current approval status, '
             'physical manufacturer or operator attribution. Sequence numbers are document row numbers. '
             'Different snapshots remain separate observations, not additive unique-device counts.', path)
    c.db.execute('DELETE FROM ntra_equipment WHERE source_id=?', (SOURCE,))
    c.db.executemany('INSERT INTO ntra_equipment VALUES (?,?,?,?,?,?,?,?,?,?,?,?)', [(SOURCE, *r) for r in rows])
    c.finish(SOURCE, len(rows))
    return {'pages': PAGE_COUNT, **counts}
