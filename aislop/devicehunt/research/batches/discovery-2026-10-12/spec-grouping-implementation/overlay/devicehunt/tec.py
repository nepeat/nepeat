"""TEC table: preserve declared OEM, applicant, model and certificate columns."""
import datetime as dt
from pathlib import Path

from .catalog import target_for
from .importers import Page

URL = 'https://www.tec.gov.in/equipment-certified-under-mtcte'
HEADER = ['S.No.', 'Company Name', 'Product Name', 'Product Variant', 'OEM Name',
          'Product Model Name', 'Product Model No.', 'Category', 'Certificate Type',
          'Issue Date', 'Valid Upto', 'Certificate ID']


def date_cell(value):
    # Hidden sort key and displayed date must agree when both are present.
    parts = value.split()
    if len(parts) not in (1, 2):
        raise ValueError('Unexpected TEC date: ' + value)
    date = dt.datetime.strptime(parts[-1], '%d/%m/%Y').date()
    if len(parts) == 2 and parts[0] != date.strftime('%Y%m%d'):
        raise ValueError('Conflicting TEC sort/display dates')
    return date.isoformat()


def parse_rows(body):
    rows = Page(body).rows
    if not rows or rows[0] != HEADER or any(row == HEADER for row in rows[1:]):
        raise ValueError('TEC table header changed or is ambiguous')
    for number, row in enumerate(rows[1:], 1):
        if len(row) != len(HEADER) or row[0] != str(number):
            raise ValueError('TEC rows are malformed, missing, or out of sequence')
    if len(rows) == 1:
        raise ValueError('TEC snapshot contains no equipment rows')
    return rows[1:]


def import_table(c, path):
    rows = parse_rows(Path(path).read_text())
    entries, matched = [], 0
    for row in rows:
        applicant, oem = row[1], row[4]
        target = target_for(oem) or target_for(applicant)
        if target is None:
            continue
        if not row[5] or not row[11]:
            raise ValueError('Target TEC record has no primary model/certificate ID')
        issue, expiry = date_cell(row[9]), date_cell(row[10])
        matched += 1
        # Explicit comma-separated entries are separate models, never aliases.
        seen = set()
        for column in (5, 6):
            for model in (m.strip() for m in row[column].split(',')):
                if not model or model in seen:
                    continue
                seen.add(model)
                entries.append((row, target, model, column, issue, expiry))
    if not entries:
        raise ValueError('TEC snapshot contains no target equipment')
    c.source('tec-mtcte', URL, 'regulatory', 'TEC public factual equipment table; source rights retained',
             'All rows of one cached HTML table scanned. OEM Name is retained as a declared-OEM relationship; '
             'Company Name is the applicant. Product Model No. entries become separate identities, not aliases. '
             'Certificate dates are historical table assertions, not current validity or support deadlines. '
             'This is a historical cached table, not a complete current TEC register.', path)
    for row, target, model, column, issue, expiry in entries:
        c.add('tec-mtcte', row[0] + '|' + model, URL, target, model, target + ' ' + model,
              row[2].lower(), {'cells': dict(zip(HEADER, row)), 'model_column': HEADER[column]},
              [(row[4], 'declared_oem'), (row[1], 'regulatory_applicant')], [('tec_certificate_id', row[11])],
              [('product_variant', row[3]), ('model_column', HEADER[column]),
               ('certificate_category', row[7]), ('certificate_type', row[8]),
               ('certificate_issue_date', issue), ('certificate_valid_upto', expiry),
               ('registry_row', row[0]), ('certificate_date_scope', 'Historical register assertion; not current status or vendor support deadline')])
    c.finish('tec-mtcte', len(rows))
    return {'scanned': len(rows), 'target_rows': matched, 'model_assertions': len(entries)}
