"""Import target-brand rows from Egypt NTRA's published equipment table."""
import re
from pathlib import Path

from .catalog import TARGETS, digest, target_for

URL = 'https://www.tra.gov.eg/wp-content/uploads/2026/01/Type-Approved-List-3.pdf'
SOURCE = 'ntra-2026-01'
PDF_SHA256 = '72f63661db38c56d1d78fdf97dfe5f8268d5ada85a538c7d6911abe8965bb54a'
# Text screening selects pages for positional table extraction. It is not used
# to split product/model columns or repair model names.
# Screening must follow the active target registry. Exact company/role matching
# still happens in target_rows; a broad page match never creates an assertion.
PAGE_TARGET = re.compile(r'\b(?:' + '|'.join(
    r'[\W_]*'.join(re.escape(token) for token in re.findall(r'[A-Za-z0-9]+', name))
    for target, aliases in TARGETS.items() for name in [target, *aliases]
) + r')\b', re.I)


def target_rows(pages):
    """Validate and select five-column table rows; preserve linked-model cells."""
    selected, seen = [], set()
    for page, tables in pages:
        for table in tables:
            for row in table:
                cells = [' '.join((cell or '').split()) for cell in row]
                # Header rows are not records. A numbered malformed row is an error.
                if not cells or not cells[0].isdigit():
                    continue
                if len(cells) != 5:
                    raise ValueError('Unexpected NTRA table columns on page ' + str(page))
                number, kind, brand, model, linked = cells
                target = target_for(brand)
                if target is None:
                    continue
                if not kind or not model or number in seen:
                    raise ValueError('Incomplete or duplicate NTRA target row: ' + number)
                seen.add(number)
                selected.append({'page': page, 'cells': cells, 'target': target})
    if not selected:
        raise ValueError('NTRA table contains no exact target-brand rows')
    return selected


def save_rows(c, path, pages, page_count):
    rows = target_rows(pages)  # Reject malformed data before source replacement.
    c.source(SOURCE, URL, 'regulatory', 'NTRA public factual equipment register; source rights retained',
             'All PDF pages screened for target names, then candidate tables parsed by column. '
             'Only exact target-brand matches imported. Linked models are separate facts, not aliases. '
             'Registry brand does not prove physical OEM or AWS use. Snapshot URL is January 2026; '
             'English and Arabic printed dates disagree, so no effective approval date is inferred.', path)
    for row in rows:
        number, kind, brand, model, linked = row['cells']
        facts = [('registry_product_description', kind), ('registry_brand', brand),
                 ('registry_row', number), ('pdf_page', str(row['page']))]
        if linked:
            facts.append(('registry_linked_models', linked))
        # This source's brand is a register classification, not an OEM assertion.
        c.add(SOURCE, number, URL + '#page=' + str(row['page']), row['target'], model,
              brand + ' ' + model, kind.lower(), row, [(row['target'], 'registry_brand')], facts=facts)
    c.finish(SOURCE, len(rows))
    return {'pages_screened': page_count, 'candidate_pages': len(pages), 'imported': len(rows)}


def import_pdf(c, path):
    path = Path(path)
    if digest(path.read_bytes()) != PDF_SHA256:
        raise ValueError('NTRA PDF differs from the reviewed snapshot; review the new file before importing')
    try:
        import pdfplumber
        from pypdf import PdfReader
    except ImportError as exc:
        raise ValueError('NTRA PDF import requires optional pypdf and pdfplumber packages') from exc
    reader = PdfReader(path)
    if not reader.pages:
        raise ValueError('NTRA PDF has no pages')
    candidate_pages = []
    for index, page in enumerate(reader.pages):
        text = page.extract_text()
        if not text or 'Type Approved Equipment List' not in text:
            raise ValueError('NTRA page has missing text/header: ' + str(index + 1))
        if PAGE_TARGET.search(text):
            candidate_pages.append(index)
    tables = []
    with pdfplumber.open(path) as pdf:
        first = pdf.pages[0].extract_tables()
        expected = ['(No)', '(Product Description)', '(Brand)', '(Model)', '(linked models)']
        if not first or len(first[0][0]) != 5 or not all(
                label in (cell or '') for label, cell in zip(expected, first[0][0])):
            raise ValueError('NTRA column headers changed')
        for index in candidate_pages:
            page_tables = pdf.pages[index].extract_tables()
            if not page_tables:
                raise ValueError('No positional table on candidate page ' + str(index + 1))
            tables.append((index + 1, page_tables))
    return save_rows(c, path, tables, len(reader.pages))


def import_saved(c, path):
    """Reproduce target import from complete reviewed positional/text captures.

    The bundle extends the full register bundle with an explicit hashed ``text``
    dependency. All pages and rows validate before any source replacement; original
    PDF bytes remain the source snapshot, preserving historical source metadata.
    """
    import json
    from .documents import dependency, read_pages
    from . import ntra_register
    path = Path(path).absolute()
    if any(p.is_symlink() for p in (path, *path.parents)):
        raise ValueError('NTRA saved bundle symlinks are not allowed')
    data = json.loads(path.read_bytes())
    if (data.get('schema_version') != 1 or data.get('source_url') != URL
            or data.get('capture_format') != 'pdfplumber_positional_tables'):
        raise ValueError('Unrecognized NTRA saved target bundle')
    content = {}
    for key in ('pdf', 'tables', 'text'):
        entry = data.get(key)
        if not isinstance(entry, dict):
            raise ValueError('Missing NTRA saved dependency: ' + key)
        name = entry.get('path')
        if not isinstance(name, str) or Path(name).name != name or name in ('', '.', '..'):
            raise ValueError('NTRA dependency must be a sibling filename')
        if (path.parent / name).is_symlink():
            raise ValueError('NTRA saved dependency symlinks are not allowed')
        content[key] = dependency(path.parent, entry)
    if data['pdf']['sha256'] != PDF_SHA256 or not content['pdf'].startswith(b'%PDF-'):
        raise ValueError('NTRA PDF differs from the reviewed snapshot')
    tables = json.loads(content['tables'])
    all_rows = ntra_register.parse_pages(tables, expected_pages=ntra_register.PAGE_COUNT)
    counts = ntra_register.summary(all_rows)
    if counts != data.get('expected') or not counts['sequence_is_contiguous']:
        raise ValueError('NTRA saved extraction counts/sequence differ')
    pages = read_pages(content['text'], PDF_SHA256)
    if len(pages) != ntra_register.PAGE_COUNT:
        raise ValueError('Incomplete NTRA saved text pages')
    candidates = set()
    for page in pages:
        if not page['text'] or 'Type Approved Equipment List' not in page['text']:
            raise ValueError('NTRA page has missing text/header: ' + str(page['page']))
        if PAGE_TARGET.search(page['text']):
            candidates.add(page['page'])
    all_tables = [(page['page'], page['tables']) for page in tables['pages']]
    screened = [(number, page_tables) for number, page_tables in all_tables if number in candidates]
    if target_rows(screened) != target_rows(all_tables):
        raise ValueError('NTRA saved text screen omits target table rows')
    return save_rows(c, path.parent / data['pdf']['path'], screened, len(pages))
