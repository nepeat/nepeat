import contextlib
import io
import json
import sqlite3
import tarfile
import tempfile
import unittest
from pathlib import Path

from devicehunt.catalog import Catalog, target_for
from devicehunt import importers
from devicehunt.__main__ import main, report


class CatalogTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.path = self.root/'catalog.sqlite'
        self.c = Catalog(self.path)

    def tearDown(self):
        self.c.db.close()
        self.temp.cleanup()

    def seed(self, sid='test', model='AS5812-54X', brand='Accton', fact='Trident 2'):
        self.c.source(sid,'https://example.org','test','test','fixture')
        did = self.c.add(sid,model,'https://example.org/'+model,brand,model,brand+' '+model,'switch',{},
                         [(brand,'manufacturer')],facts=[('asic',fact)])
        self.c.finish(sid,1)
        return did

    def test_entity_matching_does_not_include_consumer_amazon_or_substrings(self):
        self.assertEqual(target_for('Edge-corE'),'Edge-Core')
        self.assertEqual(target_for('Amazon Data Services, Inc.'),'Amazon Data Services')
        for name in ('Amazon','Amazon.com Services LLC','Amazon Technologies Inc.','Not Accton','Oracleish'):
            self.assertIsNone(target_for(name))

    def test_odm_names_do_not_collapse_unreviewed_group_entities(self):
        expected = {'INVENTEC CORPORATION': 'Inventec', 'Hyve Solutions Corporation': 'Hyve',
                    'Foxconn': 'Foxconn', 'Wiwynn Corporation': 'Wiwynn',
                    'TD SYNNEX Corporation': 'TD SYNNEX', 'SYNNEX Corporation': 'TD SYNNEX',
                    'Quanta Computer Inc.': 'Quanta'}
        for name, target in expected.items():
            self.assertEqual(target_for(name), target)
        for name in ('Synnex Technology International Corp', 'SYNNEX Japan Corporation',
                     'Quantam Telematics Limited', 'SEQUANTA', 'Quanta Storage Inc.',
                     'Quanta Cloud Technology', 'Foxconn Interconnect Technology Limited Taiwan Branch',
                     'Ingrasys', 'Hon Hai Precision Industry Co., Ltd.', 'Wistron',
                     'Inventec Appliances Corp.', 'Hyve Group', 'Hyve / TD SYNNEX'):
            self.assertIsNone(target_for(name), name)

    def test_hyve_manufacturer_and_td_synnex_parent_roles_stay_separate(self):
        self.c.source('hyve', 'https://example.org', 'vendor', 'fixture', 'fixture')
        self.c.add('hyve', 'platform', 'https://example.org', 'Hyve', 'Fixture', 'Fixture', 'server', {},
                   [('Hyve Solutions Corporation', 'manufacturer'),
                    ('TD SYNNEX Corporation', 'historical_subsidiary_portfolio')])
        self.c.finish('hyve', 1)
        self.assertEqual(len(self.c.search(target='Hyve', role='manufacturer')), 1)
        self.assertEqual(len(self.c.search(target='SYNNEX', role='historical_subsidiary_portfolio')), 1)
        self.assertEqual(self.c.search(target='TD SYNNEX', role='manufacturer'), [])

    def test_bytedance_and_inspur_keep_exact_entities_and_roles(self):
        self.assertEqual(target_for('BYTEDANCE'), 'ByteDance')
        self.assertEqual(target_for('INSPUR'), 'Inspur')
        self.assertEqual(target_for('Inspur Electronic Information Industry Co., Ltd.'), 'Inspur')
        for name in ('ByteDance System Sdn Bhd', 'TikTok', 'Pico', 'Not ByteDance',
                     'Inspur Software Group Ltd.', 'Inspur Electric Information Industry Co, Ltd.',
                     'Qingdao LG Inspur digital Communications Co., LTD', 'IEIT Systems'):
            self.assertIsNone(target_for(name), name)
        self.c.source('joint', 'https://example.org', 'vendor', 'fixture', 'fixture')
        self.c.add('joint', 'switch', 'https://example.org', 'ByteDance', 'B4010', 'fixture', 'switch', {},
                   [('ByteDance', 'brand'), ('Accton Technology Corporation', 'applicant')])
        self.c.finish('joint', 1)
        self.assertEqual(len(self.c.search('B4010', target='ByteDance')), 1)
        self.assertEqual(self.c.search(target='ByteDance', role='manufacturer'), [])
        self.assertEqual(self.c.search(target='Accton', role='manufacturer'), [])

    def test_vendor_example_does_not_pass_from_other_company_model(self):
        from devicehunt import validation
        snapshot = self.root/'certificate.txt'
        snapshot.write_text('fixture model evidence')
        inventory = self.root/'examples.json'
        inventory.write_text(json.dumps({'schema_version': 1, 'devices': [
            {'identifier': 'B4010', 'vendor': 'ByteDance'},
            {'identifier': 'B4020', 'vendor': 'ByteDance'}]}))
        self.c.source('certificate', 'https://example.org', 'vendor', 'fixture', 'fixture', snapshot)
        self.c.add('certificate', 'B4010', 'https://example.org', 'N/A', 'B4010', 'fixture', 'switch', {},
                   [('Accton Technology Corporation', 'applicant')])
        self.c.finish('certificate', 1)
        result = validation.run(self.c.db, inventory)
        self.assertEqual(result['summary'], {'total': 2, 'hits': 0, 'gaps': 2})
        self.assertEqual(result['results'][0]['leads'][0]['reason'], 'expected vendor not evidenced on this record')
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM devices').fetchone()[0], 1)

    def test_ntra_screening_includes_expanded_targets_but_parsing_stays_exact(self):
        from devicehunt import ntra
        from devicehunt.catalog import TARGETS
        for name in TARGETS:
            self.assertIsNotNone(ntra.PAGE_TARGET.search('header ' + name + ' footer'), name)
        self.assertIsNotNone(ntra.PAGE_TARGET.search('EDGECORE'))
        rows = ntra.target_rows([(1, [[['1', 'SERVER', 'INSPUR', 'SA5212M5-A', ''],
            ['2', 'SWITCH', 'BYTEDANCE', 'B4020', ''],
            ['3', 'SWITCH', 'NOT BYTEDANCE', 'B4010', '']]])])
        self.assertEqual([(r['target'], r['cells'][3]) for r in rows],
                         [('Inspur', 'SA5212M5-A'), ('ByteDance', 'B4020')])

    def test_report_keeps_all_requested_targets_visible_when_catalog_is_empty(self):
        from devicehunt.catalog import TARGETS
        path = self.root/'report.html'
        report(self.c, path)
        body = path.read_text()
        for target in TARGETS:
            self.assertIn('<option>' + target + '</option>', body)
        self.assertNotIn('__TARGET_OPTIONS__', body)
        self.assertIn('<p>' + ' · '.join(TARGETS) + '<br>', body)
        self.assertNotIn('__TARGET_NAMES__', body)

    def test_onl_new_sections_keep_full_models_and_raw_cells(self):
        path = self.root/'onl.md'
        path.write_text('Quanta\n------\n|Device|Ports|CPU|Forwarding|\n|---|---|---|---|\n'
                        '|QuantaMesh T7032-IX1B|32x100G|Intel C2558|BCM56960|\n'
                        'Inventec\n------\n|D7032Q28B|32x100G|Intel|on|\n'
                        'Other\n------\n|Unrelated|32x100G|Intel|BCM|')
        self.assertEqual(importers.onl(self.c, path), 2)
        self.assertEqual([r['model'] for r in self.c.search(target='Quanta')], ['QuantaMesh T7032-IX1B'])
        row = self.c.search(target='Inventec')[0]
        self.assertIn(('asic', 'on'), [(f['name'], f['value']) for f in self.c.detail(row['id'])['facts']])
        self.assertEqual(self.c.search(target='Quanta', role='manufacturer'), [])

    def test_model_punctuation_is_searchable_not_fts_syntax(self):
        self.seed()
        self.assertEqual(len(self.c.search('AS5812-54X')),1)
        self.assertEqual(len(self.c.search('trident',target='accton')),1)
        self.assertEqual(len(self.c.search('AS5812-54T')),0)
        self.assertEqual(self.c.search('---'),[])

    def test_source_conflicts_kept_without_duplicate_identity(self):
        did=self.seed('a',fact='Trident 2')
        self.seed('b',fact='Trident 2+')
        self.assertEqual(len(self.c.search()),1)
        self.assertEqual({f['value'] for f in self.c.detail(did)['facts']},{'Trident 2','Trident 2+'})
        self.assertEqual(len(self.c.detail(did)['records']),2)

    def test_partial_model_tokens_combine_with_fulltext_and_filters(self):
        self.seed('a',model='AS7760-32X',brand='Edge-Core',fact='specialasic chipset')
        self.seed('b',model='CS7760-48X',brand='Celestica',fact='server chipset')
        self.seed('c',model='AS7770-32X',brand='Edge-Core')
        self.assertEqual({r['model'] for r in self.c.search('7760')},{'AS7760-32X','CS7760-48X'})
        self.assertEqual([r['model'] for r in self.c.search('7760 32X')],['AS7760-32X'])
        self.assertEqual([r['model'] for r in self.c.search('7760_32X')],['AS7760-32X'])
        self.assertEqual([r['model'] for r in self.c.search('specialasic 7760')],['AS7760-32X'])
        self.assertEqual(self.c.search('7760 99X'),[])
        self.assertEqual([r['model'] for r in self.c.search('7760',target='Edge-Core',role='manufacturer')],['AS7760-32X'])
        self.assertEqual(self.c.search('7760',target='Edge-Core',role='fcc_grantee'),[])
        self.assertEqual(self.c.search('7760',hyperscaler=True),[])
        self.assertEqual(len(self.c.search('7760',limit=1)),1)
        self.assertEqual(self.c.search('7760',limit=0),[])
        self.assertEqual(self.c.search('7760 OR 7770'),[])  # OR is a word, not an operator.

    def test_fragment_scope_is_identity_fields_not_arbitrary_fact_substrings(self):
        self.c.source('one','https://example.org','vendor','test','fixture')
        self.c.add('one','a','https://example.org','Accton','MODEL-A','MODEL-A','switch',{},
            [('Accton','manufacturer')], identifiers=[('manufacturer_model','ZZ8200-48T'),('part_number','ABC12345')])
        self.c.add('one','b','https://example.org','Accton','MODEL-B','MODEL-B','switch',{},
            [('Accton','manufacturer')], facts=[('compatibility','ZZ8200-48T')],
            identifiers=[('onie_platform','YY9999-platform')])
        self.c.add('one','c','https://example.org','Accton','AAÄBC77','AAÄBC77','switch',{})
        self.c.finish('one',3)
        self.assertEqual([r['model'] for r in self.c.search('8200')],['MODEL-A'])
        self.assertEqual([r['model'] for r in self.c.search('12345')],['MODEL-A'])
        self.assertEqual(self.c.search('9999'),[])
        self.assertEqual([r['model'] for r in self.c.search('äbc77')],['AAÄBC77'])
        self.assertEqual(self.c.search('20'),[])  # Two-character interior fragments remain too broad.
        self.assertEqual({r['model'] for r in self.c.search('ZZ8200')},{'MODEL-A','MODEL-B'})
        self.assertEqual(self.c.search('8200-48T-A-AC-F'),[])

    def test_fragment_search_is_readonly_and_preserves_transactions(self):
        self.seed(model='AS7760-32X')
        before=self.path.read_bytes()
        reader=Catalog(self.path,read_only=True)
        try:
            self.assertTrue(reader.db.in_transaction)
            self.assertEqual(len(reader.search('7760')),1)
            self.assertTrue(reader.db.in_transaction)
        finally:
            reader.db.close()
        self.assertEqual(self.path.read_bytes(),before)
        self.c.db.execute('BEGIN')
        self.c.search('7760')
        self.assertTrue(self.c.db.in_transaction)
        self.c.db.rollback()

    def test_speed_search_finds_compact_port_specifications(self):
        self.seed(fact='32x100G + 2x10G')
        self.assertEqual(len(self.c.search('100G')),1)
        self.assertEqual(len(self.c.search('10G')),1)
        self.assertEqual(self.c.search('400G'),[])

    def test_search_index_is_independent_of_source_and_fact_insertion_order(self):
        other = Catalog(self.root/'other.sqlite')
        try:
            for catalog, reverse in [(self.c, False), (other, True)]:
                sources = ['a', 'b']
                if reverse: sources.reverse()
                for sid in sources:
                    catalog.source(sid, 'https://example.org', 'test', 'test', 'fixture')
                    facts = [('asic', sid + ' chipset'), ('ports', '32x100G + 2x10G')]
                    if reverse: facts.reverse()
                    catalog.add(sid, 'one', 'https://example.org', 'Accton', 'Shared',
                                'Shared', 'switch', {}, [('Accton', 'platform_vendor')], facts=facts)
                    catalog.add(sid, 'two', 'https://example.org', 'Accton', sid,
                                sid, 'switch', {}, facts=facts)
                    catalog.finish(sid, 2)
            query = 'SELECT rowid,device_id,text FROM search_index ORDER BY rowid'
            self.assertEqual([tuple(r) for r in self.c.db.execute(query)],
                             [tuple(r) for r in other.db.execute(query)])
            self.assertEqual(self.c.search('100G'), other.search('100G'))
        finally:
            other.db.close()

    def test_distinct_brand_suffix_revision_not_merged(self):
        self.seed('a')
        self.seed('b',model='AS5812-54T')
        self.seed('c',brand='Another brand')
        self.assertEqual(len(self.c.search()),3)

    def test_refresh_removes_stale_rows_and_is_idempotent(self):
        self.seed()
        self.seed()
        self.assertEqual(len(self.c.search()),1)
        self.seed(model='NEW-MODEL')
        self.assertEqual(self.c.search('AS5812'),[])
        self.assertEqual(len(self.c.search('NEW-MODEL')),1)

    def test_failed_import_rolls_back_previous_snapshot(self):
        self.seed('sonic')
        bad=self.root/'bad.html'
        bad.write_text('<html>upstream unavailable</html>')
        with self.assertRaises(ValueError):
            importers.sonic(self.c,bad)
        self.c.db.rollback()
        self.assertEqual(len(self.c.search('AS5812')),1)

    def test_fcc_grantee_does_not_become_manufacturer(self):
        self.c.source('fcc','https://example.org','mirror','facts','fixture')
        self.c.add('fcc','1','https://example.org','Amazon Data Services','CUBSCOUT','CUBSCOUT','server',{},
                   [('Amazon Data Services, Inc.','fcc_grantee')])
        self.c.finish('fcc',1)
        self.assertEqual(len(self.c.search(target='Amazon Data Services')),1)
        self.assertEqual(self.c.search(target='Amazon Data Services',role='manufacturer'),[])
        self.assertEqual(self.c.search(hyperscaler=True),[])

    def test_devicecode_scans_oem_and_does_not_extract_archive_paths(self):
        path=self.root/'source.tar.gz'
        rows=[('Brand X Router',{'title':'Brand X Router','brand':'Brand X','model':{'model':'RX'},
                              'manufacturer':{'name':'Accton'},'device_types':['router']}),
              ('Amazon Echo',{'title':'Amazon Echo','brand':'Amazon','model':{'model':'Echo'},'manufacturer':{}})]
        with tarfile.open(path,'w:gz') as tar:
            for name,row in rows:
                body=json.dumps(row).encode()
                member=tarfile.TarInfo('repo/squashed/devices/'+name+'.json')
                member.size=len(body)
                tar.addfile(member,io.BytesIO(body))
            body=b'not a device'
            member=tarfile.TarInfo('../../outside.txt');member.size=len(body)
            tar.addfile(member,io.BytesIO(body))
        self.assertEqual(importers.devicecode(self.c,path),{'scanned':2,'matched':1})
        self.assertEqual(len(self.c.search(target='Accton',role='manufacturer')),1)
        self.assertFalse((self.root/'outside.txt').exists())

    def test_onl_setext_sections_do_not_leak_to_other_vendors(self):
        path=self.root/'onl.md'
        path.write_text('Accton/Edge-Core\n------\n|Device|Ports|CPU|Forwarding|\n|---|---|---|---|\n|A1|32x100G|Atom|TH|\n\nDell\n----\n|D1|32x100G|Atom|TH|\n\nCelestica\n----\n|C1|32x100G|Atom|TH|')
        self.assertEqual(importers.onl(self.c,path),2)
        self.assertEqual(self.c.search('D1'),[])
        self.assertEqual(len(self.c.search(target='Edge-Core')),1)

    def test_oracle_navigation_is_not_a_device(self):
        path=self.root/'oracle.html'
        path.write_text('<a href="netra.html">Netra Servers</a><a href="x">Netra Server X5-2</a><a href="f">Fujitsu M10 Servers</a>')
        self.assertEqual(importers.oracle(self.c,path,'oracle-netra'),1)

    def test_sql_interface_denies_writes_attach_and_pragma(self):
        self.seed()
        for query in ('DELETE FROM devices',"ATTACH DATABASE ':memory:' AS extra",'PRAGMA user_version=8'):
            with contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(main(['--db',str(self.path),'sql',query]),1)
        self.assertEqual(len(self.c.search()),1)
        with contextlib.redirect_stdout(io.StringIO()) as output:
            self.assertEqual(main(['--db',str(self.path),'sql','SELECT count(*) AS n FROM devices']),0)
            self.assertIn('1',output.getvalue())
        with contextlib.redirect_stdout(io.StringIO()) as output:
            self.assertEqual(main(['--db',str(self.path),'sql',"SELECT count(*) FROM search_index WHERE search_index MATCH 'Trident'"]),0)
            self.assertIn('1',output.getvalue())

    def test_report_escapes_embedded_script_payload(self):
        self.seed(model='</script><script>alert(1)</script>')
        path=self.root/'report.html'
        report(self.c,path)
        text=path.read_text()
        self.assertNotIn('</script><script>alert(1)',text)
        self.assertIn('\\u003c/script\\u003e',text)


if __name__=='__main__':
    unittest.main()
