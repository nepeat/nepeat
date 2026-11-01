"""Prepare a new complete label input without touching capture or catalog state."""
import copy
import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
OLD = ROOT / 'data/label-evidence.json'
NEW = ROOT / 'data/label-evidence-with-input-marking-2026-10-04.json'
IMAGE = ROOT / 'data/raw/edgecore-7760-label.png'
ADDED = [
    ['label_observed_input_voltage_range', '100-240~, 50 - 60Hz, 6A Max'],
    ['label_observed_input_voltage_range_reference',
     'Adjacent compliance label in the same photograph, Model AS7760-32X, Input Voltage Range line.'],
]
SCOPE = (' Input marking 100-240~, 50 - 60Hz, 6A Max is transcribed from the '
         'adjacent AS7760-32X compliance label in the same photograph and applies '
         'only to the photographed marking. It does not establish measured draw, '
         'installed PSU inventory or wattage, or a universal input requirement '
         'for this ordering code or model family.')

def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

original_hash = sha(OLD)
old = json.loads(OLD.read_text())
assert len(old['sources']) == len(old['devices']) == 1
assert old['sources'][0]['id'] == old['devices'][0]['source'] == 'edgecore-7760-label'
assert old['sources'][0]['snapshot'] == 'raw/edgecore-7760-label.png'
assert sha(IMAGE) == dict(old['devices'][0]['facts'])['image_sha256']
new = copy.deepcopy(old)
new['sources'][0]['coverage'] += SCOPE
new['devices'][0]['facts'].extend(ADDED)
with NEW.open('x') as handle:
    handle.write(json.dumps(new, indent=2, ensure_ascii=False) + '\n')
actual = json.loads(NEW.read_text())
assert actual == new
assert actual['devices'][0]['facts'][:-len(ADDED)] == old['devices'][0]['facts']
restored = copy.deepcopy(actual)
restored['sources'][0]['coverage'] = old['sources'][0]['coverage']
restored['devices'][0]['facts'] = old['devices'][0]['facts']
assert restored == old  # Every original field/assertion preserved.
assert sha(OLD) == original_hash
assert sha(IMAGE) == dict(old['devices'][0]['facts'])['image_sha256']
assert set(actual) == {'sources', 'devices'}
for device in actual['devices']:
    assert all(key in device for key in ('source', 'key', 'brand', 'model', 'title', 'kind', 'affiliations'))
    for name in ('affiliations', 'identifiers', 'facts'):
        assert all(isinstance(row, list) and len(row) == 2 and
                   all(isinstance(value, str) for value in row) for row in device[name])
assert (NEW.parent / actual['sources'][0]['snapshot']).resolve() == IMAGE.resolve()
proof = {
    'original_input_sha256': original_hash,
    'new_input_sha256': sha(NEW),
    'image_sha256': sha(IMAGE),
    'sources': 1, 'devices': 1,
    'previous_fact_count': len(old['devices'][0]['facts']),
    'new_fact_count': len(actual['devices'][0]['facts']),
    'added_facts': ADDED,
    'coverage_append': SCOPE,
    'checks': {'full_original_assertion_preservation': True,
               'original_input_unchanged': True, 'image_hash_matches_prior_fact': True,
               'json_roundtrip_and_importer_shape': True, 'snapshot_resolves_beside_new_input': True},
    'visual_review': 'Original image viewed independently with view_image on 2026-10-04; adjacent Model AS7760-32X / Input Voltage Range line. No serial/MAC transcription.',
}
with Path(__file__).with_name('proof.json').open('x') as handle:
    handle.write(json.dumps(proof, indent=2) + '\n')
print(json.dumps(proof, indent=2))
