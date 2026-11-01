import contextlib
import csv
import io
import json
import tempfile
import unittest
from pathlib import Path

from devicehunt import ieee, lookup
from devicehunt.catalog import Catalog, digest
from devicehunt.__main__ import main


class IeeeTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.path = self.root / 'catalog.sqlite'
        self.c = Catalog(self.path)
        self.entries = {
            'MA-L': [('001122', 'Fixture parent'), ('080030', 'Fixture A'), ('080030', 'Fixture B'),
                     ('AABBCC', 'Fixture historical local-bit allocation')],
            'MA-M': [('0011223', 'Fixture medium')],
            'MA-S': [('001122345', 'Fixture small'), ('001122346', 'Private')],
            'IAB': [('0050C2001', 'Fixture Straße_100%')],
        }
        self.bundle = ieee.collect(self.root, self.fetch)

    def tearDown(self):
        self.c.db.close()
        self.tmp.cleanup()

    def fetch(self, url, path, refresh, max_bytes):
        path = Path(path)
        if url == ieee.URL:
            path.write_text(''.join('<a href="' + u.replace('https:', 'http:') + '">CSV</a>'
                                    for _, u in ieee.REGISTRIES.values()))
        else:
            registry = next(k for k, (_, u) in ieee.REGISTRIES.items() if u == url)
            with path.open('w', newline='') as stream:
                writer = csv.writer(stream)
                writer.writerow(ieee.FIELDS)
                for assignment, name in self.entries[registry]:
                    writer.writerow([registry, assignment, name, ' Verbatim\naddress, with comma '])
        return path

    def test_full_import_idempotent_preserves_duplicates_and_no_device_claim(self):
        for _ in range(2):
            result = ieee.import_bundle(self.c, self.bundle)
        self.assertEqual(result['rows'], 8)
        self.assertEqual(self.c.search(), [])
        rows = ieee.mac_lookup(self.c.db, '08:00:30:00:00:01')['matches']
        self.assertEqual([r['organization_name'] for r in rows], ['Fixture A', 'Fixture B'])
        self.assertEqual(rows[0]['organization_address'], ' Verbatim\naddress, with comma ')

    def test_longest_prefix_private_and_unassigned(self):
        ieee.import_bundle(self.c, self.bundle)
        for mac, name, bits in [('001122999999', 'Fixture parent', 24),
                                ('001122399999', 'Fixture medium', 28),
                                ('001122345999', 'Fixture small', 36),
                                ('001122346999', 'Private', 36),
                                ('0050C2001001', 'Fixture Straße_100%', 36)]:
            result = ieee.mac_lookup(self.c.db, mac)
            self.assertEqual((result['status'], result['prefix_bits']), ('matched', bits))
            self.assertEqual([r['organization_name'] for r in result['matches']], [name])
        result = ieee.mac_lookup(self.c.db, '00:00:01:00:00:01')
        self.assertEqual((result['status'], result['state']), ('no_public_match', 'loaded'))

    def test_mac_formats_strict_no_partial_or_eui64_guessing(self):
        for value in ['001122345abc', '00:11:22:34:5a:bc', '00-11-22-34-5A-BC', ' 0011.2234.5abc ']:
            self.assertEqual(ieee.normalize_mac(value), '001122345ABC')
        for value in ['', '001122', '00:11:22-34:5a:bc', '00 11 22 34 5a bc',
                      '0:11:22:34:5a:bc', '001122FFFE345ABC', 'fe80::211:22ff:fe34:5abc',
                      '00:11:22:34:5a:bg', None]:
            with self.assertRaises(ValueError): ieee.normalize_mac(value)

    def test_local_group_zero_and_broadcast_do_not_infer_assignee(self):
        ieee.import_bundle(self.c, self.bundle)
        for mac, classification in [('AABBCC000001', 'locally_administered'),
                                    ('011122000001', 'multicast'), ('FFFFFFFFFFFF', 'broadcast'),
                                    ('000000000000', 'unspecified')]:
            result = ieee.mac_lookup(self.c.db, mac)
            self.assertEqual(result['classification'], classification)
            self.assertEqual((result['status'], result['matches']), ('not_attributable', []))
        section = next(s for s in lookup.run(self.c.db, 'AABBCC')['datasets'] if s['dataset'] == 'ieee')
        self.assertEqual(section['matches'], 1)  # Raw historical assignment remains accessible.

    def test_malformed_csv_fails_even_if_rehashed(self):
        for body in [b'wrong,header\n', ','.join(ieee.FIELDS).encode(),
                     b'Registry,Assignment,Organization Name,Organization Address\nMA-L,001122,Name\n',
                     b'Registry,Assignment,Organization Name,Organization Address\nMA-S,001122345,Name,Address\n',
                     b'Registry,Assignment,Organization Name,Organization Address\nMA-L,00112Z,Name,Address\n',
                     b'Registry,Assignment,Organization Name,Organization Address\nMA-L,001122,"unfinished']:
            with self.assertRaises(ValueError): ieee.parse_csv(body, 'MA-L')

    def test_invalid_bundle_leaves_previous_import_intact(self):
        ieee.import_bundle(self.c, self.bundle)
        original = self.bundle.read_text()
        changes = [lambda d: d['files']['MA-L'].update(sha256='0' * 64),
                   lambda d: d['files']['MA-L'].update(path='../outside.csv'),
                   lambda d: d['files']['MA-L'].update(url='https://example.com'),
                   lambda d: d['files'].pop('IAB_check'),
                   lambda d: d['expected_rows'].update({'MA-L': 5})]
        for mutate in changes:
            data = json.loads(original); mutate(data); self.bundle.write_text(json.dumps(data))
            with self.assertRaises((ValueError, KeyError)): ieee.import_bundle(self.c, self.bundle)
            self.assertEqual(self.c.db.execute('SELECT count(*) FROM ieee_assignments').fetchone()[0], 8)
        data = json.loads(original)
        check = self.bundle.parent / data['files']['MA-L_check']['path']
        check.write_bytes(check.read_bytes().replace(b'Fixture parent', b'Changed parent'))
        data['files']['MA-L_check']['sha256'] = digest(check.read_bytes())
        self.bundle.write_text(json.dumps(data))
        with self.assertRaisesRegex(ValueError, 'between downloads'): ieee.import_bundle(self.c, self.bundle)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM ieee_assignments').fetchone()[0], 8)

    def test_literal_lookup_and_no_repeat_network_on_valid_cache(self):
        ieee.import_bundle(self.c, self.bundle)
        def no_network(*args, **kwargs): raise AssertionError('Unexpected download')
        self.assertEqual(ieee.collect(self.root, no_network), self.bundle)
        result = lookup.run(self.c.db, 'STRASSE_100%')
        section = next(s for s in result['datasets'] if s['dataset'] == 'ieee')
        self.assertEqual((section['matches'], section['unit']), (1, 'address assignment row'))
        self.assertEqual(section['rows'][0]['registry'], 'IAB')

    def test_cli_readonly_missing_tables_and_caller_transaction(self):
        ieee.import_bundle(self.c, self.bundle)
        before = digest(self.path.read_bytes())
        with contextlib.redirect_stdout(io.StringIO()) as out:
            self.assertEqual(main(['--db', str(self.path), 'mac', '00:11:22:34:50:01', '--format', 'json']), 0)
        self.assertEqual(json.loads(out.getvalue())['prefix_bits'], 36)
        self.assertEqual(before, digest(self.path.read_bytes()))
        self.c.db.execute("UPDATE ieee_assignments SET organization_name='Pending' WHERE registry='MA-S'")
        ieee.mac_lookup(self.c.db, '001122345001')
        self.assertTrue(self.c.db.in_transaction)
        self.assertEqual(self.c.db.execute("SELECT organization_name FROM ieee_assignments WHERE registry='MA-S'").fetchone()[0], 'Pending')
        self.c.db.rollback()
        self.c.db.execute('DROP TABLE ieee_assignments'); self.c.db.commit()
        before = digest(self.path.read_bytes())
        with contextlib.redirect_stdout(io.StringIO()) as out:
            self.assertEqual(main(['--db', str(self.path), 'mac', '001122345001', '--format', 'json']), 0)
        self.assertEqual(json.loads(out.getvalue())['state'], 'not_loaded')
        self.assertEqual(before, digest(self.path.read_bytes()))
        missing = self.root / 'missing.sqlite'
        with contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(main(['--db', str(missing), 'mac', '001122345001']), 1)
        self.assertFalse(missing.exists())
