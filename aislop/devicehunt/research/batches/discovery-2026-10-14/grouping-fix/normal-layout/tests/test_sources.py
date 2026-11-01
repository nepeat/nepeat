import copy
import json
import tempfile
import unittest
from pathlib import Path

from devicehunt import importers, onie
from devicehunt.catalog import Catalog


class SourceTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.c = Catalog(self.root / 'devices.sqlite')

    def tearDown(self):
        self.c.db.close()
        self.temp.cleanup()

    def bundle(self, rows):
        tree, files = [], []
        for platform, body in rows:
            path = 'accton/' + platform + '/machine.make'
            sha = onie.blob_sha(body.encode())
            tree.append({'path': path, 'sha': sha, 'type': 'blob', 'mode': '100755'})
            files.append({'path': path, 'git_blob_sha': sha, 'text': body})
        return {'revision': onie.REVISION, 'tree': {'truncated': False, 'tree': tree}, 'files': files}

    def import_bundle(self, bundle):
        path = self.root / 'bundle.json'
        path.write_text(json.dumps(bundle))
        return onie.import_bundle(self.c, path)

    def test_onie_models_require_comment_and_directory_agreement(self):
        self.import_bundle(self.bundle([
            ('accton_as5812_54x', '# Makefile fragment for Accton AS5812_54X\n'),
            ('accton_as9817', '# Accton AS9726_32D\n'),
            ('accton_bacon', '# Accton Bacon\n'),
        ]))
        models = {r[0] for r in self.c.db.execute('SELECT model FROM devices')}
        self.assertEqual(models, {'AS5812-54X', 'onie:accton_as9817', 'onie:accton_bacon'})
        self.assertEqual(self.c.search(role='manufacturer'), [])
        self.assertEqual(self.c.search(hyperscaler=True), [])

    def test_make_content_is_not_evaluated(self):
        marker = self.root / 'executed'
        body = '# Accton AS5812_54X\nONIE_ARCH ?= x86_64\n'
        body += 'UEFI_ENABLE = $(shell touch ' + str(marker) + ')\n'
        self.import_bundle(self.bundle([('accton_as5812_54x', body)]))
        facts = dict(self.c.db.execute('SELECT name,value FROM facts'))
        self.assertEqual(facts['build_onie_arch'], 'x86_64')
        self.assertNotIn('build_uefi_enable', facts)
        self.assertFalse(marker.exists())

    def test_bad_bundles_preserve_previous_source(self):
        good = self.bundle([('accton_as5812_54x', '# Accton AS5812_54X\n')])
        self.import_bundle(good)
        mutations = []
        bad = copy.deepcopy(good); bad['files'] = []; mutations.append(bad)
        bad = copy.deepcopy(good); bad['files'] *= 2; mutations.append(bad)
        bad = copy.deepcopy(good); bad['tree']['truncated'] = True; mutations.append(bad)
        bad = copy.deepcopy(good); bad['files'][0]['text'] += 'tampered'; mutations.append(bad)
        bad = copy.deepcopy(good); bad['tree']['tree'][0]['mode'] = '120000'; mutations.append(bad)
        for bad in mutations:
            with self.assertRaises(ValueError):
                self.import_bundle(bad)
            self.assertEqual(len(self.c.search('AS5812')), 1)
        self.import_bundle(good)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM records').fetchone()[0], 1)

    def test_vendor_hardware_filters_exclude_navigation_and_software(self):
        fixtures = {
            'celestica-storage': ('SC6100', ['SC6100 - Newest Addition', 'SC6100'], ['Storage', 'SD6300 firmware']),
            'oracle-storage': ('Oracle Storage Drive Enclosure DE3-24C', ['Oracle Storage Drive Enclosure DE3-24C'], ['Oracle Solaris', 'Storage']),
            'oracle-ethernet': ('Oracle Switch ES2-72', ['Oracle Switch ES2-72'], ['Cisco MDS Switch', 'Ethernet Switches']),
            'oracle-unified-storage': ('Sun Storage 7110 Unified Storage System', ['Sun Storage 7110 Unified Storage System Documentation'], ['Software Release 2.0', 'Documentation']),
        }
        for sid, (model, included, excluded) in fixtures.items():
            with self.subTest(source=sid):
                path = self.root / (sid + '.html')
                path.write_text(''.join('<a href="product">' + text + '</a>' for text in included + excluded))
                self.assertEqual(importers.hardware_index(self.c, path, sid), 1)
                rows = list(self.c.db.execute('SELECT d.model FROM devices d JOIN records r ON r.device_id=d.id WHERE r.source_id=?', (sid,)))
                self.assertEqual([r[0] for r in rows], [model])

    def oracle_table(self, rows):
        path = self.root / 'oracle-eol.html'
        path.write_text('<table><tr><th>Product</th><th>Documentation</th>'
                        '<th>Next-Generation Product</th></tr>' + rows + '</table>')
        return path

    def test_oracle_retired_models_keep_cell_provenance(self):
        path = self.oracle_table(
            '<tr><td colspan="3">Servers</td></tr>'
            '<tr><td>Sun Netra CP2060/CP2080</td>'
            '<td><a href="/old">Documentation</a></td>'
            '<td><a href="/new">Documentation</a> SPARC T8</td></tr>'
            '<tr><td>Fujitsu M10-1 Server</td><td>N/A</td><td></td></tr>')
        result = importers.hardware_index(self.c, path, 'oracle-eol')
        self.assertEqual(result, {'scanned': 2, 'imported': 2, 'excluded': ['Fujitsu M10-1 Server']})
        self.assertEqual({r[0] for r in self.c.db.execute('SELECT model FROM devices')},
                         {'Sun Netra CP2060', 'Sun Netra CP2080'})
        docs = list(self.c.db.execute("SELECT value FROM facts WHERE name='documentation_url'"))
        self.assertEqual([r[0] for r in docs], ['https://www.oracle.com/old'] * 2)
        ids = [r[0] for r in self.c.db.execute('SELECT value FROM identifiers')]
        self.assertFalse(any('T8' in value for value in ids))
        self.assertEqual(self.c.search(role='manufacturer'), [])
        self.assertEqual(self.c.search(hyperscaler=True), [])

    def test_oracle_explicit_aliases_and_part_numbers_are_searchable(self):
        path = self.oracle_table(
            '<tr><td>32-Port Ethernet Switch Assembly (PUE2 - 4199516 - 7358381)</td>'
            '<td>N/A</td><td>Oracle Switch ES2-72</td></tr>'
            '<tr><td>Sun Fire V20z Server</td><td></td><td></td></tr>')
        for _ in range(2):
            importers.hardware_index(self.c, path, 'oracle-eol')
        self.assertEqual(len(self.c.search('4199516')), 1)
        self.assertEqual(len(self.c.search('PUE2')), 1)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM records').fetchone()[0], 2)
        self.assertIn('Sun Fire V20z', {r[0] for r in self.c.db.execute('SELECT model FROM devices')})

    def test_oracle_unreviewed_rows_leave_previous_source_intact(self):
        good = '<tr><td>Sun Fire V40z Server</td><td>N/A</td><td></td></tr>'
        path = self.oracle_table(good)
        importers.hardware_index(self.c, path, 'oracle-eol')
        bad_rows = [
            '<tr><td>Sun Fire V40z/V99z Server</td><td>N/A</td><td></td></tr>',
            '<tr><td>Unrecognized hardware</td><td>N/A</td><td></td></tr>',
            '<tr><td>Sun Fire V40z</td><td>N/A</td></tr>',
        ]
        for bad in bad_rows:
            with self.assertRaises(ValueError):
                importers.hardware_index(self.c, self.oracle_table(good + bad), 'oracle-eol')
            self.assertEqual(len(self.c.search('V40z')), 1)
        path.write_text('<html>Temporary service failure</html>')
        with self.assertRaises(ValueError):
            importers.hardware_index(self.c, path, 'oracle-eol')
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM records').fetchone()[0], 1)


if __name__ == '__main__':
    unittest.main()
