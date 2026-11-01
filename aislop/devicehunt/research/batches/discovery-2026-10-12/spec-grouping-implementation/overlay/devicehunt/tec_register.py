"""All rows of the cached TEC table, independently of target model assertions."""
import json
from pathlib import Path

from .tec import URL, parse_rows, date_cell

SOURCE = 'tec-register'
SCOPE = ('All rows of one historical cached TEC MTCTE HTML table, not the current worldwide or Indian inventory. '
         'Company Name is applicant and OEM Name is declared OEM; blanks remain unknown. '
         'Primary and additional model cells are preserved separately, without inferred aliases. '
         'Repeated certificate IDs remain separate rows. Invalid date cells are retained with NULL parsed dates '
         'and explicit review status. Historical date assertions do not establish current approval validity or support.')


def register_rows(body):
    result = []
    for cells in parse_rows(body):
        dates, issues = [], []
        for index, label in ((9, 'issue_date'), (10, 'valid_upto')):
            try:
                dates.append(date_cell(cells[index]))
            except ValueError:
                dates.append(None)
                issues.append(label + ': unparseable or conflicting date cell')
        result.append((SOURCE, int(cells[0]), *cells[1:], *dates,
                       'review_required' if issues else 'parsed', '; '.join(issues),
                       json.dumps(cells, ensure_ascii=False), URL))
    return result


def import_table(c, path):
    rows = register_rows(Path(path).read_text())
    c.source(SOURCE, URL, 'regulatory', 'TEC public factual equipment table; source rights retained', SCOPE, path)
    c.db.execute('DELETE FROM tec_equipment WHERE source_id=?', (SOURCE,))
    c.db.executemany('INSERT INTO tec_equipment VALUES (' + ','.join('?' for _ in range(19)) + ')', rows)
    c.finish(SOURCE, len(rows))
    return {'rows': len(rows), 'review_required': sum(r[15] == 'review_required' for r in rows)}
