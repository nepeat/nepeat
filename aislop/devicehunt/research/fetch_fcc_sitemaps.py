"""Register/reuse a finite saved FCC sitemap collection; offline by default."""
import argparse
import hashlib
import json
import re
import sys
import uuid
import xml.etree.ElementTree as ET
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from devicehunt.artifacts import Archive, _safe
from devicehunt.capture import Collector, Policy

ROOT = Path(__file__).resolve().parents[1] / 'data/raw/fcc-sitemaps'
NS = '{http://www.sitemaps.org/schemas/sitemap/0.9}'


def validate_xml(body):
    if ET.fromstring(body).tag != NS + 'urlset':
        raise ValueError('Expected sitemap URL set')


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--saved-root', type=Path, default=ROOT)
    parser.add_argument('--archive', type=Path, required=True)
    parser.add_argument('--report-dir', type=Path, required=True)
    parser.add_argument('--network-budget', type=int, default=0)
    args = parser.parse_args(argv)
    if args.network_budget < 0: parser.error('budget must be nonnegative')
    args.saved_root = _safe(args.saved_root.absolute(), args.saved_root.absolute())
    _safe(args.saved_root, 'fetch-report.json')
    _safe(args.saved_root, 'collection-report.json')
    initial = json.loads((args.saved_root / 'fetch-report.json').read_text())
    previous = json.loads((args.saved_root / 'collection-report.json').read_text())['entries']
    index = next(r for r in initial if r['url'] == 'https://fccid.co/sitemap.xml')
    index_path = _safe(args.saved_root, index['path'])
    body = index_path.read_bytes()
    if hashlib.sha256(body).hexdigest() != index['sha256']:
        raise ValueError('index checksum mismatch')
    tree = ET.fromstring(body)
    if tree.tag != NS + 'sitemapindex': raise ValueError('Expected sitemap index')
    urls = [n.find(NS + 'loc').text for n in tree]
    if len(urls) != len(set(urls)) or not all(re.fullmatch(
            r'https://fccid\.co/sitemaps/(main|applications-\d{4}|companies|equipment-classes)\.xml', u) for u in urls):
        raise ValueError('Unexpected advertised sitemap URL')
    known = {r['url']: r for r in initial + previous}
    failures = tuple(r for r in known.values() if r.get('status') != 200)
    # Validate every saved input before publishing any collection result.
    for row in initial + previous:
        if row.get('status') == 200:
            path = _safe(args.saved_root, row['path'])
            if path.parent.resolve() != args.saved_root.resolve(): raise ValueError('not sibling capture')
            if hashlib.sha256(path.read_bytes()).hexdigest() != row['sha256']:
                raise ValueError('saved capture checksum mismatch')
            if row['url'] in urls:
                validate_xml(path.read_bytes())
            if row.get('final_url', row['url']) != row['url']:
                raise ValueError('unexpected saved redirect')
    args.archive = _safe(args.archive.absolute(),args.archive.absolute())
    # New-only initialization must never reset an existing or damaged archive.
    archive = Archive(args.archive, workspace=Path(__file__).resolve().parents[1],
                      create=not args.archive.exists())
    collector = Collector(archive, Policy(frozenset(urls), budget=args.network_budget,
                                         prior_failures=failures))
    saved_results = {}
    for report_name, rows in [('fetch-report.json', initial), ('collection-report.json', previous)]:
        report_hash = hashlib.sha256((args.saved_root / report_name).read_bytes()).hexdigest()
        for ordinal, row in enumerate(rows):
            saved_results[row['url']] = collector.register_saved(row,
                args.saved_root / row['path'] if row.get('status') == 200 else None,
                locator={'report': report_name, 'report_sha256': report_hash, 'ordinal': ordinal})
    entries = []
    for url in urls:
        if url in known:
            result = saved_results[url]
        else:
            result = collector.capture(url, validator=validate_xml)
        entries.append(result)
        if result['outcome'] == 'throttled': break
    args.report_dir.mkdir(parents=True, exist_ok=True)
    report = args.report_dir / ('collection-' + uuid.uuid4().hex + '.json')
    with report.open('x') as handle:
        json.dump({'schema_version': 2, 'index': index, 'entries': entries,
                   'coverage': 'finite advertised URL set; saved observations are not freshness checks',
                   'checkpoint': {'completed': len(entries), 'advertised': len(urls),
                                  'stopped': collector.stopped}}, handle, indent=2)
        handle.write('\n')
    archive.db.close()
    print(report)
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
