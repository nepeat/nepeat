import csv
import io
import json
import sqlite3
import tempfile
import unittest
from pathlib import Path

from devicehunt import lookup, ncc
from devicehunt.catalog import Catalog, digest


class NCCTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.c = Catalog(self.root / 'db.sqlite')
        self.rows = []
        for model in [' CS-1, special\nrevision ', '(申請者申請保密)', '']:
            row = dict.fromkeys([key for key, _ in ncc.FIELDS], '')
            row.update(certificate_number='00123', model=model, brand='Other Brand',
                       applicant='申請商', manufacturer='Celestica Limited/Other Company',
                       year_suffix_raw='01', approval_date_raw='20010101',
                       certificate_validity_raw='N', revocation_date_raw='20200102')
            self.rows.append(row)
        self.rows[-1]['manufacturer'] = ''

    def tearDown(self):
        self.c.db.close()
        self.temp.cleanup()

    def body(self):
        stream = io.StringIO(newline='')
        writer = csv.writer(stream)
        writer.writerow([label for _, label in ncc.FIELDS])
        writer.writerows([[r[key] for key, _ in ncc.FIELDS] for r in self.rows])
        return stream.getvalue().encode('utf-8-sig')

    def save(self, body=None):
        meta = dict(dataset_url=ncc.PAGE_URL, csv_url=ncc.CSV_URL,
                    capture_format='factual web-tool metadata extract')
        data = dict(schema_version=1, dataset_url=ncc.PAGE_URL, download_url=ncc.CSV_URL,
                    license_url=ncc.LICENSE_URL, expected=ncc.summary(self.rows))
        for key, content in [('metadata', json.dumps(meta).encode()),
                             ('csv', self.body() if body is None else body)]:
            path = self.root / key
            path.write_bytes(content)
            data[key] = dict(path=path.name, sha256=digest(content))
        path = self.root / 'bundle.json'
        path.write_text(json.dumps(data))
        return path

    def test_raw_roles_dates_confidentiality_duplicates_and_refresh(self):
        path = self.save()
        result = ncc.import_bundle(self.c, path)
        self.assertEqual(result['duplicate_certificate_rows'], 2)
        self.assertEqual(result['confidential_identity_rows'], 1)
        stored = [dict(r) for r in self.c.db.execute('SELECT * FROM ncc_equipment ORDER BY row_number')]
        self.assertEqual([{k:r[k] for k, _ in ncc.FIELDS} for r in stored], self.rows)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM devices').fetchone()[0], 0)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM affiliations').fetchone()[0], 0)
        ncc.import_bundle(self.c, path)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM ncc_equipment').fetchone()[0], 3)
        self.c.source('unrelated', 'https://example.org', 'regulatory', 'test', 'test')
        self.c.db.execute("INSERT INTO ncc_equipment SELECT 'unrelated',row_number," +
                          ','.join(k for k, _ in ncc.FIELDS) + ' FROM ncc_equipment WHERE row_number=1')
        self.c.db.commit()
        self.rows = self.rows[:1]
        ncc.import_bundle(self.c, self.save())
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM ncc_equipment').fetchone()[0], 2)

    def test_bad_inputs_preserve_previous_snapshot(self):
        ncc.import_bundle(self.c, self.save())
        header = self.body().decode('utf-8-sig').splitlines()[0]
        for body in [b'wrong header\n', (header + '\nshort\n').encode(),
                     (header + '\n"unterminated').encode(),
                     self.body().replace(b'00123', b'00\x00123'),
                     (header + '\n').encode()]:
            with self.assertRaises(ValueError):
                ncc.import_bundle(self.c, self.save(body))
            self.assertEqual(self.c.db.execute('SELECT count(*) FROM ncc_equipment').fetchone()[0], 3)
        path = self.save()
        (self.root / 'csv').write_bytes(b'changed')
        with self.assertRaisesRegex(ValueError, 'checksum'):
            ncc.import_bundle(self.c, path)
        path = self.save()
        data = json.loads(path.read_text()); data['expected']['rows'] = 4
        path.write_text(json.dumps(data))
        with self.assertRaisesRegex(ValueError, 'counts'):
            ncc.import_bundle(self.c, path)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM ncc_equipment').fetchone()[0], 3)

    def test_lookup_keeps_registry_evidence_read_only_and_old_schema_missing(self):
        ncc.import_bundle(self.c, self.save())
        before = (self.root / 'db.sqlite').read_bytes()
        with sqlite3.connect((self.root / 'db.sqlite').as_uri() + '?mode=ro', uri=True) as db:
            db.row_factory = sqlite3.Row
            result = lookup.run(db, 'CELESTICA', 1)
            section = next(r for r in result['datasets'] if r['dataset'] == 'ncc')
            self.assertEqual(section['matches'], 2)
            self.assertTrue(section['truncated'])
            self.assertEqual(section['rows'][0]['applicant'], '申請商')
            self.assertEqual(section['rows'][0]['source_url'], ncc.PAGE_URL)
        self.assertEqual(before, (self.root / 'db.sqlite').read_bytes())
        self.c.db.execute('DROP TABLE ncc_equipment'); self.c.db.commit()
        result = lookup.run(self.c.db, 'Celestica')
        self.assertEqual(next(r for r in result['datasets'] if r['dataset'] == 'ncc')['state'], 'not_loaded')
