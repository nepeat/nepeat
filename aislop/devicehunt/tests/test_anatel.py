import csv
import io
import json
import tempfile
import unittest
from pathlib import Path
from zipfile import ZIP_DEFLATED, ZipFile

from devicehunt import anatel as a
from devicehunt.catalog import Catalog, digest


class AnatelTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.c = Catalog(self.root / 'db.sqlite')
        def row(manufacturer, model):
            values = {k: '' for k, _ in a.FIELDS}
            values.update(manufacturer=manufacturer, model=model, approval_number='000010100001',
                          applicant='Local Applicant Ltd', applicant_cnpj='00123456789012',
                          product_type='Switch', certificate_status='Suspenso',
                          commercial_name='Marketing label, not an alias', approval_date='01/02/2001')
            return [values[k] for k, _ in a.FIELDS]
        edge = row('EdgeCore Networks Corporation', 'AZ4444-52X-DC-10')
        self.rows = [edge, edge, row('Amazon Data Services, Inc', 'AZ9074-64X-DC-42'),
                     row('Celestica Technology Consultancy (Shanghai) Co.,', 'CS8200-32X-UN-00'),
                     row('Amazon Technologies Inc.', 'Other model'), row('Unrelated', '')]

    def tearDown(self):
        self.c.db.close()
        self.temp.cleanup()

    def archive(self, rows=None, headers=None):
        text = io.StringIO(newline='')
        writer = csv.writer(text, delimiter=';')
        writer.writerow(headers if headers is not None else [v for _, v in a.FIELDS])
        writer.writerows(self.rows if rows is None else rows)
        p = self.root / 'before.zip'
        with ZipFile(p, 'w', ZIP_DEFLATED) as archive:
            archive.writestr(a.MEMBER, text.getvalue().encode('utf-8-sig'))
        return p

    def save(self):
        before = self.archive()
        after = self.root / 'after.zip'; after.write_bytes(before.read_bytes())
        index = self.root / 'index.html'; index.write_text('<a href="' + a.URL + '">CSV ZIP</a>')
        bundle = {'schema_version': 1, 'source_url': a.URL, 'rows': len(self.rows), 'files': {}}
        for key, p, url in [('before', before, a.URL), ('after', after, a.URL), ('index', index, a.PAGE)]:
            bundle['files'][key] = {'path': p.name, 'url': url, 'sha256': digest(p.read_bytes())}
        path = self.root / 'bundle.json'; path.write_text(json.dumps(bundle))
        return path

    def test_complete_register_and_target_roles_preserve_source_limits(self):
        path = self.save()
        for _ in range(2):
            self.assertEqual(a.import_bundle(self.c, path), {'registry_rows': 6, 'model_assertions': 4})
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM anatel_equipment').fetchone()[0], 6)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM devices').fetchone()[0], 3)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM records').fetchone()[0], 4)
        self.assertEqual(self.c.search(role='manufacturer'), [])
        self.assertEqual(self.c.search(hyperscaler=True), [])
        models = {r[0] for r in self.c.db.execute('SELECT model FROM devices')}
        self.assertNotIn('AZ4444-52X', models)
        self.assertNotIn('Other model', models)
        self.assertNotIn('Marketing label, not an alias', models)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM identifiers WHERE kind="alias"').fetchone()[0], 0)
        self.assertEqual(len(self.c.search(target='Celestica')), 1)
        r = self.c.db.execute('SELECT approval_number,applicant_cnpj FROM anatel_equipment LIMIT 1').fetchone()
        self.assertEqual(tuple(r), ('000010100001', '00123456789012'))

    def test_search_includes_duplicate_and_non_target_rows_and_literal_text(self):
        a.import_bundle(self.c, self.save())
        self.assertEqual(len(a.search(self.c.db, 'az4444')), 2)
        self.assertEqual(len(a.search(self.c.db, 'Unrelated')), 1)
        self.assertEqual(len(a.search(self.c.db, 'Local Applicant')), 6)
        self.assertEqual(a.search(self.c.db, '%'), [])
        self.assertEqual(len(a.search(self.c.db, limit=0)), 0)
        with self.assertRaises(ValueError): a.search(self.c.db, limit=-2)

    def test_bad_bundle_never_replaces_valid_register(self):
        a.import_bundle(self.c, self.save())
        for mutation in ['hash', 'count', 'path', 'url', 'different_after']:
            p = self.save(); data = json.loads(p.read_text())
            if mutation == 'hash': data['files']['before']['sha256'] = '0' * 64
            elif mutation == 'count': data['rows'] = 5
            elif mutation == 'path': data['files']['before']['path'] = '../outside.zip'
            elif mutation == 'url': data['files']['index']['url'] = 'https://unrelated.example/'
            else:
                after = self.root / 'after.zip'; after.write_bytes(b'different')
                data['files']['after']['sha256'] = digest(after.read_bytes())
            p.write_text(json.dumps(data))
            with self.assertRaises(ValueError): a.import_bundle(self.c, p)
            self.assertEqual(self.c.db.execute('SELECT count(*) FROM anatel_equipment').fetchone()[0], 6)

    def test_csv_layout_and_archive_corruption_fail(self):
        for rows, headers in [(self.rows, ['wrong header']), ([self.rows[0][:-1]], None), ([], None)]:
            with self.assertRaises(ValueError): a.parse_archive(self.archive(rows, headers))
        p = self.root / 'invalid.zip'; p.write_bytes(b'<html>Error</html>')
        with self.assertRaisesRegex(ValueError, 'Invalid Anatel'): a.parse_archive(p)

    def test_collect_checks_both_archives_and_source_index(self):
        body = self.archive().read_bytes()
        calls = []
        def fetch(url, path, refresh, max_bytes):
            calls.append(url)
            path.write_bytes(body if url == a.URL else ('<a href="' + a.URL + '">download</a>').encode())
            return path
        path = a.collect(self.root, fetch)
        self.assertEqual(len(a.validate_bundle(path)), 6)
        self.assertEqual(calls, [a.URL, a.PAGE, a.URL])
        self.assertEqual(a.collect(self.root, fetch), path)
        self.assertEqual(len(calls), 3)
