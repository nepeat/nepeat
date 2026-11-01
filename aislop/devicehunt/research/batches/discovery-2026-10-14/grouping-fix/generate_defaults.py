"""Private provenance tool: capture fixed defaults from hash-bound original code."""
import contextlib
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import tempfile
from unittest.mock import patch

import devicehunt
from devicehunt.catalog import Catalog

OWN = Path(__file__).resolve().parent
assert Path.cwd() == OWN
assert Path(devicehunt.__file__).resolve().is_relative_to(OWN / 'overlay')
hashes = json.loads((OWN / 'source-hashes.json').read_text())

def original(filename):
    path = OWN / 'originals' / filename
    assert hashlib.sha256(path.read_bytes()).hexdigest() == hashes['devicehunt/' + filename]
    spec = importlib.util.spec_from_file_location('devicehunt._baseline_' + filename.replace('.', '_'), path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module

specifications = original('specifications.py')
cli = original('__main__.py')
cli.specifications = specifications
destination = OWN / 'tests/fixtures/specification-grouping-defaults.json'
assert not destination.exists(), 'pinned defaults must not be silently regenerated'
baseline = {'provenance': {'description': 'unchanged production defaults before optional grouping',
                          'modules_sha256': {name: hashes['devicehunt/' + name]
                                             for name in ('specifications.py', '__main__.py')}}, 'cases': []}
with tempfile.TemporaryDirectory(dir=OWN / 'tmp') as temp:
    path = Path(temp) / 'fixture.sqlite'
    c = Catalog(path)
    for sid in ('a', 'b'):
        c.source(sid, 'https://example.invalid/' + sid, 'vendor' if sid != 'b' else 'community', 'fixture', 'fixture')
        c.add(sid, 'one', 'https://example.invalid/' + sid + '/one', 'Celestica', 'DS4000',
              'Celestica DS4000', 'switch', {}, [('Celestica', 'documentation_vendor')],
              [('part_number', 'P-1'), ('alias', 'alias')],
              [('power_description', '2 PSUs'), ('spec_power_supply', '2 x 800 W')])
        c.finish(sid, 1)
    cases = [('DS4000', {}), ('DS4000', {'include_source_facts': True}),
             ('alias', {'exact_model': True, 'include_source_facts': True}),
             ('P-1', {'exact_part_number': True, 'target': 'Celestica', 'include_source_facts': True})]
    with patch('urllib.request.urlopen', side_effect=AssertionError('network forbidden')), \
         patch('socket.create_connection', side_effect=AssertionError('network forbidden')), \
         patch('subprocess.Popen', side_effect=AssertionError('process forbidden')):
        for query, kwargs in cases:
            args = [query, '--limit', '-1']
            for key, value in kwargs.items():
                args.append('--vendor' if key == 'target' else '--' + key.replace('_', '-'))
                if key == 'target':
                    args.append(value)
            case = {'query': query, 'kwargs': kwargs, 'args': args,
                    'api_rows': specifications.search(c, query, limit=-1, **kwargs), 'cli': {}}
            for fmt in ('json', 'csv', 'table'):
                stream = io.StringIO()
                with contextlib.redirect_stdout(stream):
                    assert cli.main(['--db', str(path), 'specs', *args, '--format', fmt]) == 0
                case['cli'][fmt] = stream.getvalue()
            baseline['cases'].append(case)
        baseline['default_api_rows'] = specifications.search(c, 'DS4000')
    c.db.close()
destination.write_text(json.dumps(baseline, ensure_ascii=False, indent=2) + '\n')
print(json.dumps({'path': str(destination), 'sha256': hashlib.sha256(destination.read_bytes()).hexdigest(),
                  'baseline_modules_sha256': baseline['provenance']['modules_sha256'],
                  'cases': len(baseline['cases'])}, indent=2))
