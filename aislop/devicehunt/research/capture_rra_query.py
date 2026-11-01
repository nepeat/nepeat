"""Complete a public RRA manufacturer-query capture in a new directory.

Run from the repository root with python -m research.capture_rra_query.
Stops on network errors; partial captures are retained and never published as bundles.
"""
import argparse
import datetime as dt
import json
import re
import shutil
import subprocess
import sys
from pathlib import Path

from devicehunt import rra
from devicehunt.catalog import digest


def download(url, path, query=None):
    args = ['curl', '--silent', '--show-error', '--fail', '--location', '--max-time', '55']
    if query is not None:
        for key, value in query.items():
            args += ['--data-urlencode', key + '=' + value]
    args += [url, '--output', str(path)]
    subprocess.run(args, check=True)


def entry(path):
    return dict(path=path.name, sha256=digest(path.read_bytes()))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('manufacturer')
    parser.add_argument('output', type=Path)
    parser.add_argument('--end', required=True, help='YYYYMMDD')
    parser.add_argument('--start', default='19900101', help='YYYYMMDD')
    parser.add_argument('--form', type=Path, help='Reuse a saved official source form')
    parser.add_argument('--before', type=Path, help='Reuse a CSV already retrieved with these exact query fields')
    parser.add_argument('--empty-result', type=Path, help='Reuse an official no-results page retrieved with these exact query fields')
    parser.add_argument('--allow-row-reordering', action='store_true',
                        help='Require exact certificate-keyed CSV fields, allowing generated row order to differ')
    args = parser.parse_args()
    query = dict(category='', fromdate=args.start, todate=args.end, firm='', equip='',
                 model_no='', app_no='', maker=args.manufacturer, nation='')
    # Validate dates before creating output or issuing any request.
    for date in (args.start, args.end):
        if not re.fullmatch(r'\d{8}', date):
            parser.error('dates must use YYYYMMDD')
        dt.datetime.strptime(date, '%Y%m%d')
    if args.start > args.end or not args.manufacturer.strip():
        parser.error('invalid date range or empty manufacturer')
    root = args.output.resolve(); root.mkdir(parents=True, exist_ok=False)
    (root/'query.json').write_text(json.dumps(query, indent=2)+'\n')
    form = root/'search-form.html'
    if args.form:
        shutil.copyfile(args.form, form)
    else:
        download(rra.FORM, form)
    before = root/'before.csv'
    if args.before:
        shutil.copyfile(args.before, before)
    else:
        download(rra.CSV_URL, before, query)
    rows = rra.csv_rows(before.read_bytes(), allow_empty=True)
    print('Collecting', len(rows), 'certificate details for', args.manufacturer, flush=True)
    empty = None
    if rows:
        if args.empty_result:
            raise ValueError('An empty-result page cannot accompany a nonempty CSV')
        subprocess.run([sys.executable, '-m', 'research.capture_rra_details', str(before), str(root)], check=True)
        capture = json.loads((root/'capture-progress.json').read_text())
    else:
        empty = root/'empty-result.html'
        if args.empty_result:
            shutil.copyfile(args.empty_result, empty)
        else:
            download(rra.RESULT_URL, empty, query)
        rra.validate_empty_result(empty.read_bytes())
        capture = dict(complete=True, entries=[])
        (root/'capture-progress.json').write_text(json.dumps(capture, indent=2)+'\n')
    if capture.get('complete') is not True:
        raise ValueError('Detail collection did not complete')
    after = root/'after.csv'; download(rra.CSV_URL, after, query)
    bundle = dict(schema_version=1, complete=True, form_url=rra.FORM, csv_url=rra.CSV_URL,
                  query=query, form=entry(form), csv_before=entry(before), csv_after=entry(after),
                  expected_rows=len(rows), details=capture['entries'])
    if empty is not None:
        bundle['empty_result'] = dict(url=rra.RESULT_URL, **entry(empty))
    if args.allow_row_reordering:
        bundle['csv_comparison'] = 'exact_rows_ignore_order'
    candidate = root/'candidate.json'; candidate.write_text(json.dumps(bundle, indent=2)+'\n')
    rra.validate(candidate)
    candidate.rename(root/'bundle.json')
    print('Verified bundle:', root/'bundle.json', flush=True)


if __name__ == '__main__':
    main()
