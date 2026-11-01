"""Reconstruct preimages by removing exactly the two additive patches.

No original file copies were retained. Recorded pre-change hashes authenticate
these reconstructions before any diff is emitted. Code/test files are read only.
"""
import difflib
import hashlib
import json
from pathlib import Path

root = Path(__file__).resolve().parents[4]
out = Path(__file__).resolve().parent
before_hashes = dict(line.split(maxsplit=1)[::-1] for line in
                     (out / 'pre-code-sha256.txt').read_text().splitlines())
after_hashes = dict(line.split(maxsplit=1)[::-1] for line in
                    (out / 'post-code-sha256.txt').read_text().splitlines())
cuts = {
    'devicehunt/buying.py': ('# Oracle\'s named SM variants stay separately labeled;', 'LIMITS = ['),
    'tests/test_buying.py': ('    def test_oracle_variants_preserve_labels_and_processor_wattage_scope(self):',
                           '    def test_matched_identity_with_no_practical_facts_still_reports_gaps(self):'),
}
report = {'method': 'reconstructed by inverting exactly the additive mapping and three-test edits; no retained original copies',
          'files': [], 'added_line_whitespace_errors': []}
diffs = []
for name, (start, end) in cuts.items():
    data = (root / name).read_bytes()
    after = data.decode()
    assert after.count(start) == after.count(end) == 1
    a, b = after.index(start), after.index(end)
    before = after[:a] + after[b:]
    pre_hash = hashlib.sha256(before.encode()).hexdigest()
    post_hash = hashlib.sha256(data).hexdigest()
    assert pre_hash == before_hashes[name], (name, pre_hash)
    assert post_hash == after_hashes[name], (name, post_hash)
    preimage = out / ('reconstructed-before-' + Path(name).name)
    preimage.write_bytes(before.encode())
    diff = ''.join(difflib.unified_diff(before.splitlines(True), after.splitlines(True),
                                       fromfile='before/' + name, tofile='after/' + name))
    assert diff
    diffs.append(diff)
    for line_number, line in enumerate(after[a:b].splitlines(), 1):
        if line != line.rstrip() or '\t' in line:
            report['added_line_whitespace_errors'].append({'file': name, 'added_block_line': line_number, 'line': line})
    report['files'].append({'file': name, 'preimage_sha256': pre_hash, 'preimage_verified': True,
                            'post_sha256': post_hash, 'post_unchanged': True,
                            'added_lines_checked': len(after[a:b].splitlines())})
assert not report['added_line_whitespace_errors']
diff_bytes = ''.join(diffs).encode()
(out / 'code.diff').write_bytes(diff_bytes)
report['diff_bytes'] = len(diff_bytes)
report['diff_sha256'] = hashlib.sha256(diff_bytes).hexdigest()
(out / 'diff-verification.json').write_text(json.dumps(report, indent=2) + '\n')
print(json.dumps(report, indent=2))
