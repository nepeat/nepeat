"""Create a production-applicable patch and a separate normal-layout proof tree."""
import difflib
import hashlib
import json
from pathlib import Path
import shutil

OWN = Path(__file__).resolve().parent
ROOT = OWN.parents[3]
assert Path.cwd() == OWN
hashes = json.loads((OWN / 'source-hashes.json').read_text())
assert all(hashlib.sha256((ROOT / name).read_bytes()).hexdigest() == value
           for name, value in hashes.items())
paths = {'devicehunt/specifications.py': OWN / 'overlay/devicehunt/specifications.py',
         'devicehunt/__main__.py': OWN / 'overlay/devicehunt/__main__.py',
         'tests/test_specification_grouping.py': OWN / 'tests/test_specification_grouping.py',
         'tests/fixtures/specification-grouping-defaults.json': OWN / 'tests/fixtures/specification-grouping-defaults.json'}
parts = []
for name, path in paths.items():
    baseline = ROOT / name
    original = baseline.read_text().splitlines(keepends=True) if baseline.exists() else []
    parts.extend(difflib.unified_diff(original, path.read_text().splitlines(keepends=True),
                                      fromfile='a/' + name if baseline.exists() else '/dev/null', tofile='b/' + name))
destination = OWN / 'implementation.patch'
assert not destination.exists(), 'do not silently replace a reviewable patch'
destination.write_text(''.join(parts))
normal = OWN / 'normal-layout'
assert not normal.exists()
for name in hashes:
    target = normal / name
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(ROOT / name, target)
(OWN / 'patch-files.json').write_text(json.dumps({
    'patch_sha256': hashlib.sha256(destination.read_bytes()).hexdigest(),
    'production_patch_files': {name: {'before_sha256': hashes.get(name),
                                     'after_sha256': hashlib.sha256(path.read_bytes()).hexdigest()}
                               for name, path in paths.items()},
    'baseline_files_copied_to_normal_layout': len(hashes)}, indent=2) + '\n')
print(destination)
