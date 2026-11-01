"""Extract original ENERGY STAR workbooks without executing formulas/macros.

Optional extraction dependencies: openpyxl 3.1.5 and xlrd 2.0.2.
Catalog import/rebuild uses the hashed extraction and needs neither package.
"""
import datetime
import hashlib
import json
import sys
from pathlib import Path


def extract(path):
    path = Path(path)
    sheets = []
    if path.suffix == '.xlsx':
        import openpyxl
        book = openpyxl.load_workbook(path, read_only=False, data_only=False)
        method = 'openpyxl ' + openpyxl.__version__ + '; stored cell values; formulas rejected'
        for sheet in book:
            rows = []
            for row in sheet:
                values = []
                for cell in row:
                    if cell.data_type in ('f', 'e'):
                        raise ValueError('Formula/error cell is not supported: ' + cell.coordinate)
                    v = cell.value
                    if isinstance(v, (datetime.datetime, datetime.date, datetime.time)):
                        v = {'iso_datetime': v.isoformat()}
                    values.append(v)
                rows.append(values)
            sheets.append(dict(name=sheet.title, rows=rows, merged_ranges=[
                [r.min_row - 1, r.max_row, r.min_col - 1, r.max_col]
                for r in sheet.merged_cells.ranges]))
        book.close()
    elif path.suffix == '.xls':
        import xlrd
        book = xlrd.open_workbook(path, formatting_info=True)
        method = 'xlrd ' + xlrd.__version__ + '; stored cell values; Excel date serials retained'
        for sheet in book.sheets():
            rows = []
            for n in range(sheet.nrows):
                values = []
                for cell in sheet.row(n):
                    if cell.ctype == xlrd.XL_CELL_ERROR:
                        raise ValueError('Error cell is not supported')
                    v = cell.value
                    if cell.ctype == xlrd.XL_CELL_DATE:
                        v = {'excel_serial': v, 'date_mode': book.datemode,
                             'iso_datetime': xlrd.xldate_as_datetime(v, book.datemode).isoformat()}
                    values.append(v)
                rows.append(values)
            sheets.append(dict(name=sheet.name, rows=rows, merged_ranges=sheet.merged_cells))
    else:
        raise ValueError('Expected XLS or XLSX')
    return dict(schema_version=1, workbook_sha256=hashlib.sha256(path.read_bytes()).hexdigest(),
                extraction_method=method, sheets=sheets)


if __name__ == '__main__':
    path = Path(sys.argv[1])
    path.with_suffix('.cells.json').write_text(json.dumps(extract(path), ensure_ascii=False) + '\n')
