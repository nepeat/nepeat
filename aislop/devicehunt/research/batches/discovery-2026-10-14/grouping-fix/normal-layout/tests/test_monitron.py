import json
import tempfile
import unittest
from pathlib import Path

from devicehunt import monitron as m
from devicehunt import hyperscaler_inventory
from devicehunt.catalog import Catalog, digest


class MonitronTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.c = Catalog(self.root / 'db.sqlite')

    def tearDown(self):
        self.c.db.close()
        self.temp.cleanup()

    def bundle(self):
        def entry(p): return {'path': p.name, 'sha256': digest(p.read_bytes())}
        index = '<h1>Amazon Monitron Safety Guide</h1>'
        b = {'schema_version': 1, 'index_url': m.INDEX_URL, 'history_url': m.HISTORY_URL, 'guides': {}}
        for name, (url, models, pn, role, kind) in m.GUIDES.items():
            if role == 'manufacturer':
                label = ', '.join(models) + (' Ex' if name == 'ex-sensor' else '') + ' Sensor Safety Manual'
                field = ('TE1Ax9z Note: x and z define the product variant configuration' if name == 'ex-sensor'
                         else 'TE1A001, TE1A105 FCC ID: 2AVPL-TE1A001 IC: 25869-TE1A001')
                text = 'Amazon Monitron Manufacturer Amazon Data Services, Inc., Address Model ' + field + ' End'
            else:
                label = 'EU / English'
                index += '<p>Amazon Monitron ' + kind.replace('gateway', 'Gateway') + ' (' + models[0] + ') Safety and Compliance Information:</p>'
                text = ('Hereby, Amazon Data Services, Inc. declares that the radio equipment type Amazon Monitron Gateway '
                        + models[0] + ' is in compliance with Directive 2014/53/EU.')
            index += '<a href="' + url + '">' + label + '</a>'
            pdf = self.root / (name + '.pdf'); pdf.write_bytes(b'%PDF-1.4\nTest fixture only')
            side = self.root / (name + '.json')
            pages = [{'page': n, 'text': text if n == pn else ''} for n in range(1, pn + 1)]
            side.write_text(json.dumps({'pdf_sha256': digest(pdf.read_bytes()),
                                       'extraction_method': 'pypdf embedded text; no OCR', 'pages': pages}))
            b['guides'][name] = {'url': url, 'pdf': entry(pdf), 'text': entry(side), 'page_count': pn}
        p = self.root / 'index.html'; p.write_text(index); b['index'] = entry(p)
        p = self.root / 'history.html'; p.write_text('<h1>Document history for the Amazon Monitron User Guide</h1>'
            '<table><tr><td>Notice</td><td>' + m.NOTICE + ' See our blog.</td><td>October 31, 2024</td></tr></table>')
        b['history'] = entry(p)
        p = self.root / 'bundle.json'; p.write_text(json.dumps(b)); return p

    def change_text(self, path, name, before, after):
        b = json.loads(path.read_text()); entry = b['guides'][name]['text']; p = self.root / entry['path']
        p.write_text(p.read_text().replace(before, after)); entry['sha256'] = digest(p.read_bytes()); path.write_text(json.dumps(b))

    def test_exact_models_roles_shared_ids_and_service_context(self):
        p = self.bundle()
        for _ in range(2): self.assertEqual(m.import_bundle(self.c, p), {'sources': 5, 'models': 5, 'model_assertions': 10})
        self.assertEqual({d['model'] for d in self.c.search(target='Amazon Data Services', role='manufacturer')},
                         {'TE1A001', 'TE1A105', 'TE1A195'})
        self.assertEqual({d['model'] for d in self.c.search(target='Amazon Data Services', role='compliance_declarant')},
                         {'TG1A001', 'TG1A002'})
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM devices').fetchone()[0], 5)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM records').fetchone()[0], 10)
        self.assertEqual({d['model'] for d in self.c.search(hyperscaler=True)},
                         {'TE1A001','TE1A105','TE1A195','TG1A001','TG1A002'})
        links=hyperscaler_inventory.run(self.c.db,vendor='Amazon Data Services',limit=-1)
        self.assertEqual(links['assertions'],5)
        self.assertEqual(links['counts_by_relationship'],{'documented_product':5})
        self.assertTrue(all(r['operator']=='AWS' and 'not data-center deployment' in r['detail'] for r in links['rows']))
        self.assertEqual({r['model'] for r in hyperscaler_inventory.run(
            self.c.db,vendor='Amazon Data Services',role='manufacturer')['rows']},
            {'TE1A001','TE1A105','TE1A195'})
        self.assertEqual({r['model'] for r in hyperscaler_inventory.run(
            self.c.db,vendor='Amazon Data Services',role='compliance_declarant')['rows']},
            {'TG1A001','TG1A002'})
        self.assertEqual(hyperscaler_inventory.run(self.c.db,relationship='documented_platform_deployment')['assertions'],0)
        self.assertEqual(self.c.db.execute("SELECT count(*) FROM hyperscaler_evidence h JOIN records r ON r.id=h.record_id WHERE r.source_id='aws-monitron-service-history'").fetchone()[0],0)
        self.assertEqual(self.c.db.execute("SELECT count(*) FROM affiliations a JOIN records r ON r.id=a.record_id WHERE r.source_id='aws-monitron-service-history' AND a.target IS NOT NULL").fetchone()[0], 0)
        ids = [tuple(r) for r in self.c.db.execute('SELECT kind,value FROM identifiers')]
        self.assertEqual(ids.count(('fcc_id', '2AVPL-TE1A001')), 2)
        self.assertFalse(any(kind == 'alias' for kind, _ in ids))
        models = {r[0] for r in self.c.db.execute('SELECT model FROM devices')}
        self.assertNotIn('TE1Ax9z', models)
        self.assertEqual(self.c.db.execute("SELECT count(*) FROM facts WHERE name='service_notice_date' AND value='2024-10-31'").fetchone()[0], 5)

    def test_wrong_manufacturer_or_gateway_model_rejected_before_replacement(self):
        for name, before, after in [('sensor', 'Amazon Data Services, Inc.', 'Amazon Web Services, Inc.'),
                                    ('wifi-eu', 'TG1A001', 'TG1A002'),
                                    ('ex-sensor', 'variant configuration', 'unrelated text')]:
            p = self.bundle(); m.import_bundle(self.c, p)
            snapshot = [tuple(r) for r in self.c.db.execute('SELECT * FROM sources ORDER BY id')]
            self.change_text(p, name, before, after)
            with self.assertRaises(ValueError): m.import_bundle(self.c, p)
            self.assertEqual(snapshot, [tuple(r) for r in self.c.db.execute('SELECT * FROM sources ORDER BY id')])

    def test_wrong_index_mapping_future_notice_or_duplicate_notice_rejected(self):
        for key, before, after in [('index', 'TE1A195 Ex Sensor', 'TE1A999 Ex Sensor'),
                                   ('history', 'October 31, 2024', 'October 1, 2024'),
                                   ('history', m.NOTICE, 'Amazon Monitron will no longer be open to new customers.')]:
            p = self.bundle(); b = json.loads(p.read_text()); f = self.root / b[key]['path']
            f.write_text(f.read_text().replace(before, after)); b[key]['sha256'] = digest(f.read_bytes()); p.write_text(json.dumps(b))
            with self.assertRaises(ValueError): m.prepare(p)
        p = self.bundle(); b = json.loads(p.read_text()); f = self.root / b['history']['path']
        f.write_text(f.read_text() * 2); b['history']['sha256'] = digest(f.read_bytes()); p.write_text(json.dumps(b))
        with self.assertRaises(ValueError): m.prepare(p)

    def test_checksum_pdf_association_and_outside_path_rejected(self):
        p = self.bundle(); b = json.loads(p.read_text()); b['guides']['ethernet-eu']['pdf']['sha256'] = '0' * 64
        p.write_text(json.dumps(b))
        with self.assertRaisesRegex(ValueError, 'checksum'): m.prepare(p)
        p = self.bundle(); self.change_text(p, 'sensor', digest(b'%PDF-1.4\nTest fixture only'), '0' * 64)
        with self.assertRaisesRegex(ValueError, 'association'): m.prepare(p)
        p = self.bundle(); b = json.loads(p.read_text()); b['history']['path'] = '../outside.html'; p.write_text(json.dumps(b))
        with self.assertRaises(ValueError): m.prepare(p)
