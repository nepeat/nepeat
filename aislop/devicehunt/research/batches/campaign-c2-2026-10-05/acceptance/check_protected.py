"""Read-only recheck of the c1 protected-file proof; writes only --output.

Live archive entries are compared only when --archive-membership is given,
because an accepted registry upgrade intentionally adds index/authority files.
"""
import argparse
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
BEFORE = ROOT / 'research/batches/campaign-c1-2026-10-04/acceptance/protected-before.json'


def sha256(path):
    h = hashlib.sha256()
    with open(path, 'rb') as handle:
        for block in iter(lambda: handle.read(1 << 20), b''):
            h.update(block)
    return h.hexdigest()


def check(entries):
    bad = []
    for entry in entries:
        path = ROOT / entry['path']
        if not path.is_file():
            bad.append(dict(path=entry['path'], error='missing'))
        elif path.stat().st_size != entry['byte_count'] or sha256(path) != entry['sha256']:
            bad.append(dict(path=entry['path'], error='changed'))
    return bad


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--output', type=Path)
    parser.add_argument('--archive-membership', action='store_true')
    args = parser.parse_args()
    before = json.loads(BEFORE.read_text())
    entries = list(before['c0_pinned_source_and_checkpoint_files'])
    bundle = before['complete_reference_bundle']
    entries.append(dict(path=bundle['path'], byte_count=bundle['byte_count'], sha256=bundle['sha256']))
    archive = before['live_archive_files']
    result = dict(protected_entries=len(entries), protected_failures=check(entries))
    # Receipts and objects are immutable even across an accepted upgrade.
    immutable = [e for e in archive if e['path'].startswith(('data/archive/receipts/', 'data/archive/objects/'))]
    result.update(immutable_archive_entries=len(immutable), immutable_archive_failures=check(immutable))
    if args.archive_membership:
        result['archive_membership_failures'] = check(archive)
    result['ok'] = not any(v for k, v in result.items() if k.endswith('failures'))
    text = json.dumps(result, indent=2, sort_keys=True) + '\n'
    if args.output:
        args.output.write_text(text)
    print(text, end='')
    return 0 if result['ok'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
