"""Coordinator: create five new captures, preserving the unsupported first pass."""
import datetime
import difflib
import hashlib
import json
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[4]
sys.path.insert(0, str(ROOT))
from scripts.extract_pdf_pages import extract

OUT = Path(__file__).resolve().parent
BATCH = OUT.parent
TOOLS = Path('/nix/store/wb8486c5l68f1shsbc217b7m4hbgrw3s-poppler-utils-26.06.0/bin')


def sha(p):
    return hashlib.sha256(p.read_bytes()).hexdigest()


def main():
    review = BATCH / 'review/poppler-support-review.md'
    if not review.is_file():
        raise SystemExit('Independent code review required before extraction')
    inputs = json.loads((BATCH / 'psu-extraction/extraction-report.json').read_text())['inputs']
    report = {'created_at_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
              'utility_sha256': sha(ROOT / 'scripts/extract_pdf_pages.py'),
              'review_sha256': sha(review), 'documents': []}
    for item in inputs:
        source = BATCH / 'psu' / item['source_file']
        assert sha(source) == item['source_sha256_before']
        dest = OUT / item['envelope_file']
        capture = extract(source, dest, TOOLS)
        previous = BATCH / 'psu-extraction' / item['envelope_file']
        assert sha(previous) == item['envelope_sha256']
        old = json.loads(previous.read_text())
        assert len(capture['pages']) == len(old['pages']) == item['page_count']
        changed = []
        diffs = []
        for a, b in zip(old['pages'], capture['pages']):
            assert a['page'] == b['page']
            if a['text'] != b['text']:
                changed.append(b['page'])
                diffs.extend(difflib.unified_diff(a['text'].splitlines(True), b['text'].splitlines(True),
                             fromfile=f"r1/page-{b['page']}", tofile=f"r2/page-{b['page']}"))
        diff = OUT / (source.stem + '-r1-r2.diff')
        with diff.open('x') as f:
            f.writelines(diffs)
        report['documents'].append({'source': str(source.relative_to(ROOT)), 'pdf_sha256': sha(source),
            'text': dest.name, 'text_sha256': sha(dest), 'pages': len(capture['pages']),
            'method': capture['extraction_method'], 'version': capture['extractor_version'],
            'options': capture['extraction_options'], 'changed_pages_from_r1': changed,
            'diff': diff.name, 'warnings': [x for x in capture['provenance']['commands'][2:] if x['stderr']]})
    report['total_pages'] = sum(x['pages'] for x in report['documents'])
    assert report['total_pages'] == 63
    with (OUT / 'report.json').open('x') as f:
        json.dump(report, f, indent=2)
        f.write('\n')
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
