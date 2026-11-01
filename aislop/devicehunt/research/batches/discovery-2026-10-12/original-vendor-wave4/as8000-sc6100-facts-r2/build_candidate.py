"""Title-only successor to frozen reviewed r1; never import or open a catalog."""
import argparse
import copy
import hashlib
import json
import os
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[4]
R1 = HERE.parent / 'as8000-sc6100-facts'
R1_SHA = '66fbabc7252652a5748d71e53a45cfd29ecf26d434a77ccedd45e18c2d3aae00'


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def encoded(value):
    return (json.dumps(value, ensure_ascii=False, indent=2) + '\n').encode()


def save(path, body, check):
    if check or path.exists():
        assert path.read_bytes() == body, str(path)
    else:
        with path.open('xb') as handle:
            handle.write(body)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true', help='verify without writing')
    args = parser.parse_args()
    assert sha(R1 / 'candidate.json') == R1_SHA
    originals = json.loads((R1 / 'candidate.json').read_text())
    candidate = copy.deepcopy(originals)
    changes = []
    for row in candidate['devices']:
        title = row['brand'] + ' ' + row['model']
        assert row['title'] != title
        changes.append({'source': row['source'], 'key': row['key'], 'old_title': row['title'], 'new_title': title})
        row['title'] = title
    restored = copy.deepcopy(candidate)
    for old, new in zip(originals['devices'], restored['devices'], strict=True):
        new['title'] = old['title']
    assert restored == originals and len(changes) == 2
    assert len(candidate['sources']) == len(candidate['devices']) == 2
    assert sum(len(r['facts']) for r in candidate['devices']) == 26
    copied = []
    for artifact in json.loads((R1 / 'provenance.json').read_text())['artifacts']:
        source = R1 / artifact['snapshot']
        target = HERE / artifact['snapshot']
        assert sha(source) == artifact['sha256']
        if not args.check:
            target.parent.mkdir(parents=True, exist_ok=True)
        save(target, source.read_bytes(), args.check)
        if not args.check:
            original_stat = source.stat()
            os.utime(target, ns=(original_stat.st_atime_ns, original_stat.st_mtime_ns))
        copied.append({'snapshot': artifact['snapshot'], 'r1_path': source.relative_to(ROOT).as_posix(),
                       'original': artifact['original'], 'sha256': artifact['sha256'], 'bytes': source.stat().st_size})
    save(HERE / 'candidate.json', encoded(candidate), args.check)
    provenance = {'r1_candidate_path': (R1 / 'candidate.json').relative_to(ROOT).as_posix(),
                  'r1_candidate_sha256': R1_SHA, 'r2_candidate_sha256': sha(HERE / 'candidate.json'),
                  'changes': changes, 'all_other_fields_equal': True, 'artifacts': copied,
                  'scope': 'Exactly two user-facing title strings; all source/evidence/facts/roles/routing unchanged; raw sources untouched.'}
    save(HERE / 'provenance.json', encoded(provenance), args.check)
    print('PASS: exactly 2 titles changed; 2 sources / 2 records / 26 facts; 4 immutable copies; no import')


if __name__ == '__main__':
    main()
