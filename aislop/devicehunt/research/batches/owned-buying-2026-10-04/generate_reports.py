"""Reproduce the ten read-only buying queries; expectations are never evidence."""
import argparse
import hashlib
import json
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[3]


def sha256(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest() if hasattr(hashlib, 'file_digest') else hashlib.sha256(stream.read()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True, help='New output path; existing reports are preserved')
    args = parser.parse_args()
    if args.output.exists():
        parser.error('Output exists; select a new path to preserve the previous snapshot')
    database = ROOT / 'data/devices.sqlite'
    inventory = ROOT / 'validation/owned-hardware.json'
    before = sha256(database)
    reports = []
    for item in json.loads(inventory.read_text())['devices']:
        # The expected validation vendor is query context, not a collection filter:
        # exact Accton/OEM-role records can be useful for an Edge-Core identifier.
        command = ['-m', 'devicehunt', '--db', str(database), 'buying-report',
                   item['identifier'], '--format', 'json']
        result = subprocess.run([sys.executable, *command], cwd=ROOT,
                                text=True, capture_output=True, check=True)
        reports.append({'query': item['identifier'], 'expected_vendor': item['vendor'],
                        'command': [sys.executable, *command], 'report': json.loads(result.stdout)})
    after = sha256(database)
    if after != before:
        raise RuntimeError('Read-only report queries changed the database')
    output = {
        'schema_version': 1,
        'scope': 'Saved-source buying evidence across all exact-record company roles. Baseline entries supply queries and expected-vendor context only, never evidence or claims about owned units. No vendor filter is imposed.',
        'database': 'data/devices.sqlite', 'database_sha256': after,
        'inventory': 'validation/owned-hardware.json', 'inventory_sha256': sha256(inventory),
        'generator': str(Path(__file__).relative_to(ROOT)),
        'read_only_verified': True, 'reports': reports,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open('x') as stream:
        json.dump(output, stream, ensure_ascii=False, indent=2)
        stream.write('\n')
    print(json.dumps({'output': str(args.output), 'reports': len(reports), 'database_sha256': after}))


if __name__ == '__main__':
    main()
