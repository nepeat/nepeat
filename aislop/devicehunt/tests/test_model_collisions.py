import contextlib
import io
import json
import sqlite3
import tempfile
import unittest
from pathlib import Path

from devicehunt import model_collisions as mc
from devicehunt.__main__ import main
from devicehunt.catalog import Catalog


def build(path):
    c = Catalog(path)
    db = c.db
    db.execute("INSERT INTO sources(id,url,tier,license,coverage) VALUES('s1','u','vendor','l','c'),('s2','u','community','l','c')")
    devs = [('d1', 'Quanta', 'QuantaGrid D43K-1U', 'server'), ('d2', 'Quanta Cloud Technology', 'QuantaGrid  d43k-1u', 'server'),
            ('d3', 'Inspur', 'NF5180A7', 'server'), ('d4', 'Inspur Corporation', 'Inspur NF5180A7', 'server'),
            ('d5', 'Accton', 'AS7726-32X', 'switch'), ('d6', 'Accton', 'AS7726-32X-O-AC-F', 'switch'),
            ('d7', 'Oracle', 'SPARC S7-2', 'server'), ('d8', 'Oracle', 'Oracle SPARC S7-2', 'server'),
            ('d9', 'Foo', 'X100', 'network switch software platform definition'), ('d10', 'Bar', 'X100', 'switch'),
            ('d11', 'Foo', 'Y200', 'Fan Module FRU'), ('d12', 'Bar', 'Y200', 'switch'),
            ('d13', 'Edge-Core', 'EAP102', 'access point'), ('d14', 'Accton', 'EAP102', 'access point'),
            ('d15', 'Oracle', 'Oracle', 'server'),
            ('d16', 'Edge-Core', 'EAP1', 'access point'), ('d17', 'Edge-corE', 'EAP1', 'access point')]
    for i, b, m, k in devs:
        db.execute("INSERT INTO devices(id,brand,model,title,kind) VALUES(?,?,?,?,?)", (i, b, m, b + ' ' + m, k))
        db.execute("INSERT INTO records VALUES(?,?,?,?,?,?,?)", ('r' + i, i, 's1', i, 'u', 't', '{}'))
    db.execute("INSERT INTO records VALUES('r1b','d1','s2','k','u','t','{}')")
    db.execute("INSERT INTO affiliations VALUES('rd1','Quanta Computer','Quanta','brand'),('rd13','Edge-Core','Edge-Core','brand')")
    db.commit()
    db.close()


class ModelCollisionTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.path = Path(self.tmp.name) / 'c.sqlite'
        build(self.path)
        self.db = sqlite3.connect(self.path.resolve().as_uri() + '?mode=ro', uri=True)
        self.db.row_factory = sqlite3.Row

    def tearDown(self):
        self.db.close()
        self.tmp.cleanup()

    def groups(self, **kw):
        r = mc.run(self.db, limit=-1, **kw)
        return {g['key']: g for g in r['groups']}

    def test_normalisation_is_conservative(self):
        self.assertEqual(mc.stripped_key('Oracle', ' Oracle   SPARC S7-2'), ('sparc s7-2', True))
        self.assertEqual(mc.stripped_key('Oracle', 'Oracle'), ('oracle', False))
        self.assertEqual(mc.stripped_key('Inspur Corporation', 'Inspur NF1'), ('nf1', True))
        self.assertEqual(mc.stripped_key('Inspur', 'Inspurious NF1'), ('inspurious nf1', False))

    def test_reasons_and_members(self):
        g = self.groups()
        self.assertEqual(g['quantagrid d43k-1u']['reason'], 'same_model_different_brand')
        m = g['quantagrid d43k-1u']['members'][0]
        self.assertEqual((m['id'], m['source_count'], m['source_tiers']), ('d1', 2, ['community', 'vendor']))
        self.assertEqual(m['affiliations'], [{'company': 'Quanta Computer', 'role': 'brand'}])
        self.assertEqual(g['nf5180a7']['reason'], 'brand_token_prefix_different_brand')
        self.assertEqual(g['sparc s7-2']['reason'], 'brand_token_prefix')
        self.assertTrue(any(x['brand_token_stripped'] for x in g['sparc s7-2']['members']))

    def test_suffixes_are_not_collisions(self):
        keys = self.groups()
        self.assertNotIn('as7726-32x', keys)
        self.assertFalse(any('as7726' in k for k in keys))

    def test_kind_exclusion_and_all_kinds(self):
        self.assertNotIn('x100', self.groups())
        self.assertIn('y200', self.groups())  # accessory-style kinds are not excluded
        self.assertIn('x100', self.groups(all_kinds=True))
        for k in ('BMC firmware platform', 'software reference', 'BMC device tree source', 'software configuration'):
            self.assertTrue(mc.kind_excluded(k))
        self.assertFalse(mc.kind_excluded('server'))

    def test_same_brand_same_plain_key_is_not_a_group(self):
        self.assertNotIn('eap1', self.groups())

    def test_order_deterministic(self):
        a = mc.run(self.db, limit=-1)
        self.assertEqual(a, mc.run(self.db, limit=-1))
        keys = [(-g['size'], g['key']) for g in a['groups']]
        self.assertEqual(keys, sorted(keys))


    def test_flags_and_min_key_length(self):
        g = self.groups()
        self.assertTrue(g['y200']['short_key'])
        self.assertFalse(g['quantagrid d43k-1u']['short_key'])
        self.assertTrue(g['y200']['kinds_differ'])
        self.assertTrue(g['eap102']['kinds_differ'] is False)
        self.assertEqual(set(self.groups(min_key_length=6)), set(g) - {'y200'})
        with self.assertRaises(ValueError):
            mc.run(self.db, min_key_length=-1)

    def test_target_filter(self):
        g = self.groups(target='Edge-Core')
        self.assertEqual(set(g), {'eap102'})
        self.assertEqual(set(self.groups(target='Quanta')), {'quantagrid d43k-1u'})
        with self.assertRaises(ValueError):
            mc.run(self.db, target='Nobody')

    def test_limit_and_counts(self):
        r = mc.run(self.db, limit=1)
        self.assertEqual((r['shown'], r['truncated'], r['groups_total']), (1, True, 5))
        self.assertEqual(sum(r['counts_by_reason'].values()), 5)
        self.assertEqual(mc.run(self.db, limit=0)['groups'], [])
        with self.assertRaises(ValueError):
            mc.run(self.db, limit=-2)

    def test_cli_readonly_json_and_table(self):
        before = self.path.read_bytes()
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            self.assertEqual(main(['--db', str(self.path), 'model-collisions', '--format', 'json', '--limit', '-1']), 0)
        data = json.loads(out.getvalue())
        self.assertEqual(data['groups_total'], 5)
        self.assertIn('does not merge', data['note'])
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            main(['--db', str(self.path), 'model-collisions', '--target', 'Quanta'])
        self.assertIn('[same_model_different_brand]', out.getvalue())
        self.assertEqual(before, self.path.read_bytes())

    def test_missing_database_fails_without_creating(self):
        missing = Path(self.tmp.name) / 'nope.sqlite'
        with contextlib.redirect_stderr(io.StringIO()):
            self.assertNotEqual(main(['--db', str(missing), 'model-collisions']), 0)
        self.assertFalse(missing.exists())


if __name__ == '__main__':
    unittest.main()
