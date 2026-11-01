"""Bounded offline reviewer probes; outputs only this review directory and temp catalogs."""
from contextlib import ExitStack
import hashlib
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[4]
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(ROOT))
sys.dont_write_bytecode = True
from devicehunt.catalog import Catalog
from devicehunt.importers import curated, prepare_curated


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def load_old(label, filename):
    source = ROOT / 'research/batches/discovery-2026-10-12/curated-import-fix' / filename
    spec = importlib.util.spec_from_file_location('devicehunt.' + label, source)
    if spec is None:
        from importlib.machinery import SourceFileLoader
        spec = importlib.util.spec_from_loader('devicehunt.' + label, SourceFileLoader('devicehunt.' + label, str(source)))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def logical(c):
    tables = ('sources', 'devices', 'records', 'facts', 'identifiers', 'affiliations', 'hyperscaler_evidence', 'search_index')
    return {t: sorted([tuple(r) for r in c.db.execute('SELECT * FROM ' + t)], key=repr) for t in tables}


def main():
    report = {'scope': 'offline preparation + two tiny old/new catalogs; no shared catalog opens or imports'}
    with ExitStack() as guards:
        for name in ('socket.socket.connect', 'socket.create_connection', 'subprocess.run', 'subprocess.Popen'):
            guards.enter_context(patch(name, side_effect=AssertionError('offline reviewer probe')))
        jobs = {}
        totals = {}
        for filename in ('data/import-manifest.json', 'data/import-manifest-candidate-wave3-2026-10-12.json'):
            path = ROOT / filename
            held = path.read_bytes()
            manifest = json.loads(held)
            selected = [j for j in manifest['imports'] if j['source'] == 'curated']
            for job in selected:
                p = ROOT / 'data' / job['path']
                assert sha(p) == manifest['files'][job['path']]
                if job['path'] not in jobs:
                    prepared = prepare_curated(p)
                    jobs[job['path']] = (len(prepared), sum(len(item[3]) for item in prepared))
            assert path.read_bytes() == held
            totals[filename] = {'sha256': hashlib.sha256(held).hexdigest(), 'jobs': len(selected),
                                'source_occurrences': sum(jobs[j['path']][0] for j in selected),
                                'selected_rows': sum(jobs[j['path']][1] for j in selected)}
        report['pure_preflight'] = totals
        oldcatalog = load_old('_review_original_catalog', 'catalog.py.before')
        oldimporters = load_old('_review_original_importers', 'importers.py.before')
        with tempfile.TemporaryDirectory() as directory:
            p = Path(directory)
            new = Catalog(p / 'new.sqlite')
            old = oldcatalog.Catalog(p / 'old.sqlite')
            guards.callback(new.db.close)
            guards.callback(old.db.close)
            source = dict(id='one', url='u', tier='vendor', license='l', coverage='c')
            seedrow = dict(source='one', key='seed', brand='None', model='Existing', title='original', kind='fixture', affiliations=[])
            file = p / 'input.json'
            guards.enter_context(patch('devicehunt.catalog.now', return_value='fixed'))
            guards.enter_context(patch.object(oldcatalog, 'now', return_value='fixed'))
            file.write_text(json.dumps({'sources': [source], 'devices': [seedrow]}))
            curated(new, file)
            oldimporters.curated(old, file)
            incoming = dict(seedrow, key='new', brand=None, title=None, kind=None,
                            affiliations=[['company', None], [[], {'ignored': True}]],
                            identifiers=[[None, 'value'], [{'ignored': True}, False]],
                            facts=[[None, 'value'], [{'ignored': True}, None]],
                            hyperscaler=[[None, 'relationship', 'detail']], revision={'ignored': True})
            file.write_text(json.dumps({'sources': [source], 'devices': [incoming, {'source': {'unused': True}}]}))
            assert curated(new, file) == oldimporters.curated(old, file) == 1
            assert logical(new) == logical(old)
            report['nullable_insert_ignore_and_skipped_tuple_fields_equal_original'] = True
            snapshot = p / 'capture.bin'
            snapshot.write_bytes(b'trusted bytes')
            file.write_text(json.dumps({'sources': [dict(source, snapshot='capture.bin', sha256='untrusted')], 'devices': []}))
            actual_read = Path.read_bytes
            read_count = []
            def counted(path):
                if path == snapshot:
                    read_count.append(path)
                return actual_read(path)
            with patch.object(Path, 'read_bytes', counted):
                curated(new, file)
            assert len(read_count) == 1
            assert new.db.execute("SELECT sha256 FROM sources WHERE id='one'").fetchone()[0] == hashlib.sha256(b'trusted bytes').hexdigest()
            report['end_to_end_snapshot_read_count'] = len(read_count)
        report['frozen_files_after'] = {name: sha(ROOT / name) for name in ('devicehunt/importers.py', 'devicehunt/catalog.py', 'tests/test_curated_import.py')}
        report['PASS'] = True
    with (HERE / 'curated-fix-independent-probes.json').open('x') as handle:
        json.dump(report, handle, indent=2)
        handle.write('\n')
    print(json.dumps(report, indent=2))


if __name__ == '__main__':
    main()
