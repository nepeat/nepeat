import tempfile
import unittest
from pathlib import Path

from devicehunt.catalog import Catalog
from devicehunt import oracle_evidence as oracle


class OracleEvidenceTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.c = Catalog(self.root / 'catalog.sqlite')

    def tearDown(self):
        self.c.db.close()
        self.temp.cleanup()

    def load(self, sid, body):
        path = self.root / (sid + '.html')
        path.write_text(body)
        return oracle.import_page(self.c, path, sid)

    def test_source_headings_preserve_suffixes_and_link_ownership(self):
        body = 'Source Code for Open Source Software - X86 Hardware Systems'
        body += '<section><h5>Oracle Server X7-2C</h5><a href="//download.oracle.com/x7">ILOM</a></section>'
        body += '<section><h5>Oracle Exadata X10M Server E5-2L</h5></section>'
        body += '<section><h5>1.0</h5><a href="//download.oracle.com/other">Other</a></section>'
        body += '<section><h5>EDK II</h5><a href="//download.oracle.com/software">Software</a></section>'
        entries = oracle.oss_entries(body)
        self.assertEqual(len(entries), 2)
        self.assertEqual(entries['Oracle Exadata X10M Server E5-2L']['links'], set())
        self.assertEqual(entries['Oracle Server X7-2C']['links'], {('ILOM', 'https://download.oracle.com/x7')})
        self.load('oracle-oss', body)
        self.assertEqual(len(self.c.search('X7-2C')), 1)
        self.assertEqual(self.c.search('E5-2C'), [])
        self.assertEqual(self.c.search(hyperscaler=True), [])
        self.assertEqual(self.c.search(role='manufacturer'), [])
        with self.assertRaises(ValueError):
            self.load('oracle-oss', '<h5>Oracle Server X7-2C</h5>')
        self.assertEqual(len(self.c.search('X7-2C')), 1)

    def test_license_requires_body_statement_and_handles_inline_markup(self):
        heading = '<h2>Oracle Server X10-2c Licensing Information</h2>'
        statement = '<p>The Oracle Server X10-2c is a hardware component that can be included in <span>Oracle Private Cloud Appliance</span>.</p>'
        self.assertEqual(self.load('oracle-x10-license', heading + statement), 1)
        self.assertEqual(len(self.c.search('X10-2C')), 1)
        with self.assertRaises(ValueError):
            self.load('oracle-x10-license', heading)
        self.assertEqual(len(self.c.search('X10-2C')), 1)

    def test_nist_only_reads_tested_configuration_scope(self):
        body = 'Certificate #4506 <p> running on Oracle X99-2C Server </p>'
        body += 'Tested Configuration(s)</span><ul><li>Oracle Linux running on Oracle E4-2C Server (AMD)</li>'
        body += '<li>Windows running on Oracle E4-2C Server (AMD)</li></ul>'
        self.assertEqual(self.load('nist-4506', body), 1)
        self.assertEqual(len(self.c.search('E4-2C')), 1)
        self.assertEqual(self.c.search('X99-2C'), [])
        raw = self.c.db.execute('SELECT raw_json FROM records').fetchone()[0]
        self.assertIn('Windows', raw)
        self.assertIn('Oracle Linux', raw)
        self.assertEqual(self.c.search(role='manufacturer'), [])


if __name__ == '__main__':
    unittest.main()
