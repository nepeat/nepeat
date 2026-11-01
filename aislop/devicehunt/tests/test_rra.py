import copy
import html
import json
import tempfile
import unittest
from pathlib import Path

from devicehunt import rra, lookup
from devicehunt.catalog import Catalog, digest


class RRATests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(); self.root = Path(self.tmp.name)
        self.c = Catalog(self.root/'db.sqlite')
        self.rows = [dict(zip((k for k,_ in rra.FIELDS), values)) for values in [
            ['Different Brand', 'Switch', 'MAIN-10', 'DERIVED-10, DERIVED-11',
             'R-R-ZZR-MAIN10', 'Celestica Inc.', '태국,중국', '2020-05-19', ''],
            ['Celestica', 'Router', 'UNMAPPED-10', '', 'TE-A35/K016-04-0186',
             'Other Company, Celestica Inc.', '중국', '2004-06-22', 'notice']]]
        self.csv = ('\n\n' + ', '.join(rra.HEADER) + '\n\n' +
            '1,Different Brand,Switch,MAIN-10,태국,중국,2020-05-19,적합등록,R-R-ZZR-MAIN10\n\n' +
            '2,Celestica,Router,UNMAPPED-10,중국,2004-06-22,적합등록,TE-A35/K016-04-0186\n').encode('cp949')

    def tearDown(self):
        self.c.db.close(); self.tmp.cleanup()

    def save(self):
        def entry(name, body):
            (self.root/name).write_bytes(body)
            return dict(path=name, sha256=digest(body))
        details = []
        for n,row in enumerate(self.rows):
            body = '<table>' + ''.join('<tr><th>'+label+'</th><td>'+html.escape(row[key])+'</td></tr>'
                                       for key,label in rra.FIELDS) + '</table>'
            details.append(dict(certificate=row['certificate_number'],
                                url=rra.DETAIL+rra.quote(row['certificate_number'],safe=''),
                                **entry(str(n)+'.html',body.encode())))
        data = dict(schema_version=1, complete=True, form_url=rra.FORM, csv_url=rra.CSV_URL,
            query=dict(category='',fromdate='19900101',todate='20260926',firm='',equip='',model_no='',app_no='',maker='Celestica',nation=''),
            form=entry('form.html',b'<input name="maker"><script>"/ko/license/csv_down.do"</script>'),
            csv_before=entry('before.csv',self.csv),csv_after=entry('after.csv',self.csv),
            expected_rows=2,details=details)
        p=self.root/'bundle.json';p.write_text(json.dumps(data));return p

    def test_original_fields_raw_csv_and_composite_companies_keep_distinct_scope(self):
        result=rra.import_bundle(self.c,self.save())
        self.assertEqual(result,dict(registry_rows=2,model_assertions=3))
        raw=list(self.c.db.execute('SELECT * FROM rra_equipment ORDER BY row_number'))
        self.assertEqual(len(json.loads(raw[0]['cells_json'])),9)
        self.assertEqual(raw[0]['country_raw'],'태국,중국')
        self.assertEqual(raw[1]['manufacturer'],'Other Company, Celestica Inc.')
        self.assertEqual({r[0] for r in self.c.db.execute('SELECT model FROM devices')},
                         {'MAIN-10','DERIVED-10','DERIVED-11'})
        self.assertEqual({r[0] for r in self.c.db.execute('SELECT kind FROM identifiers')},{'rra_certificate_id'})
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM hyperscaler_evidence').fetchone()[0],0)
        result=lookup.run(self.c.db,'UNMAPPED-10')
        section=next(s for s in result['datasets'] if s['dataset']=='rra')
        self.assertEqual(section['matches'],1)
        self.assertEqual(section['sources'][0]['tier'],'regulatory')
        self.assertEqual(next(s for s in result['datasets'] if s['dataset']=='catalog')['matches'],0)

    def test_incomplete_changed_wrong_certificate_and_hash_fail_before_replacement(self):
        rra.import_bundle(self.c,self.save())
        original=[tuple(r) for r in self.c.db.execute('SELECT * FROM records')]
        for change in ['incomplete','detail_missing','bad_url','changed_csv','bad_hash','query','order']:
            p=self.save();d=json.loads(p.read_text())
            if change=='incomplete':d['complete']=False
            if change=='detail_missing':d['details'].pop()
            if change=='bad_url':d['details'][1]['url']='https://example.org'
            if change=='changed_csv':
                body=self.csv+b'\n';(self.root/'after.csv').write_bytes(body);d['csv_after']['sha256']=digest(body)
            if change=='bad_hash':(self.root/'1.html').write_text('different')
            if change=='query':d['query']['fromdate']='20260230'
            if change=='order':d['details'].reverse()
            p.write_text(json.dumps(d))
            with self.assertRaises(ValueError):rra.import_bundle(self.c,p)
            self.assertEqual([tuple(r) for r in self.c.db.execute('SELECT * FROM records')],original)

    def test_exact_labelled_table_required_and_ambiguous_models_remain_raw(self):
        p=self.save();body=(self.root/'0.html').read_bytes()
        with self.assertRaises(ValueError):rra.parse_detail(body,'other-certificate')
        with self.assertRaises(ValueError):rra.parse_detail(body+body,'R-R-ZZR-MAIN10')
        with self.assertRaises(ValueError):rra.parse_detail(body.replace('제조자'.encode(),'상호'.encode()),'R-R-ZZR-MAIN10')
        row=copy.deepcopy(self.rows[0]);row['main_model']='ONE / TWO';row['derived_models']='THREE (rev 2), FOUR-10'
        self.assertEqual(rra.models(row),[('derived','FOUR-10')])
        with self.assertRaises(ValueError):rra.csv_rows(self.csv.replace(b'2,Celestica',b'3,Celestica'))

    def test_reimport_does_not_duplicate_and_only_replaces_its_query(self):
        p=self.save();rra.import_bundle(self.c,p);rra.import_bundle(self.c,p)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM rra_equipment').fetchone()[0],2)
        self.c.source('other','https://example.org','vendor','fixture','fixture')
        self.c.add('other','x','https://example.org','Oracle','KEPT-1','kept','server',{})
        self.c.finish('other',1)
        rra.import_bundle(self.c,p)
        self.assertEqual(self.c.db.execute("SELECT count(*) FROM records WHERE source_id='other'").fetchone()[0],1)

    def test_underscore_certificate_keeps_literal_identity_and_encoded_detail_url(self):
        self.rows[0]['certificate_number']='R-R-amz-SAT1A_C2'
        self.csv=self.csv.replace(b'R-R-ZZR-MAIN10',b'R-R-amz-SAT1A_C2')
        p=self.save();rra.import_bundle(self.c,p)
        self.assertEqual(self.c.db.execute('SELECT certificate_number FROM rra_equipment WHERE row_number=1').fetchone()[0],
                         'R-R-amz-SAT1A_C2')
        self.assertEqual(self.c.db.execute("SELECT value FROM identifiers WHERE value LIKE '%SAT1A%' LIMIT 1").fetchone()[0],
                         'R-R-amz-SAT1A_C2')

    def test_korean_company_text_cannot_disappear_into_a_target_match(self):
        self.rows[0]['applicant']='다른 회사 (Oracle America, Inc.)'
        self.rows[1]['manufacturer']='다른 회사 (Celestica Inc.)'
        result=rra.import_bundle(self.c,self.save())
        self.assertEqual(result['model_assertions'],3)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM rra_equipment').fetchone()[0],2)
        self.assertEqual(self.c.db.execute("SELECT count(*) FROM affiliations WHERE target='Oracle'").fetchone()[0],0)
        self.assertEqual(self.c.db.execute("SELECT count(*) FROM affiliations WHERE company=? AND target IS NULL",
                         (self.rows[0]['applicant'],)).fetchone()[0],3)
        self.assertEqual(rra.company_target('CELESTICA (THAILAND) LIMITED.'),'Celestica')
        self.assertIsNone(rra.company_target('주식회사 트렌토 시스템즈 (Edgecore Networks Corporation)'))

    def test_main_lists_keep_literal_labels_roles_and_evidence_without_aliases(self):
        self.rows[0]['main_model'] = 'Sun Fire 280R,350R, SUN FIRE 280R'
        self.rows[0]['derived_models'] = '350r, NEXT-10'
        self.rows[1]['main_model'] = 'NO-10, NO-11'  # Composite manufacturer stays unmapped.
        result = rra.import_bundle(self.c, self.save())
        self.assertEqual(result['model_assertions'], 3)
        self.assertEqual({r[0] for r in self.c.db.execute('SELECT model FROM devices')},
                         {'Sun Fire 280R', '350R', 'NEXT-10'})
        claims = list(self.c.db.execute("SELECT d.model,f.value FROM devices d JOIN records r "
            "ON r.device_id=d.id JOIN facts f ON f.record_id=r.id WHERE f.name='rra_model_role'"))
        self.assertEqual({tuple(r) for r in claims},
                         {('Sun Fire 280R', 'main'), ('350R', 'main'), ('NEXT-10', 'derived')})
        raw = self.c.db.execute('SELECT main_model FROM rra_equipment WHERE row_number=1').fetchone()[0]
        self.assertEqual(raw, self.rows[0]['main_model'])
        self.assertEqual(self.c.db.execute("SELECT count(*) FROM facts WHERE name='rra_main_model' "
                         "AND value=?", (raw,)).fetchone()[0], 3)
        self.assertEqual({r[0] for r in self.c.db.execute('SELECT kind FROM identifiers')},
                         {'rra_certificate_id'})
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM hyperscaler_evidence').fetchone()[0], 0)
        # Withdrawal removes a list entry and its search hit, preserving other sources.
        self.c.source('other', 'https://example.org', 'vendor', 'fixture', 'fixture')
        self.c.add('other', 'x', 'https://example.org', 'Oracle', 'KEEP-10', 'kept', 'server', {})
        self.c.finish('other', 1)
        self.rows[0]['main_model'] = 'Sun Fire 280R'
        self.rows[0]['derived_models'] = 'NEXT-10'
        rra.import_bundle(self.c, self.save())
        self.assertEqual({r[0] for r in self.c.db.execute('SELECT model FROM devices')},
                         {'Sun Fire 280R', 'NEXT-10', 'KEEP-10'})
        self.assertEqual(self.c.search('350R'), [])

    def test_annotated_or_incomplete_main_lists_are_not_partially_promoted(self):
        row = copy.deepcopy(self.rows[0]); row['derived_models'] = 'DERIVED-10'
        for main in ['MAIN-10,OTHER (rev 2)', 'MAIN-10,OTHER/ALT', 'MAIN-10,',
                     ',MAIN-10', 'MAIN-10,,OTHER', 'MAIN-10,null', 'MAIN-10,한글',
                     'HDX900A,HDX9001:ISDN', 'MAIN-10,OTHER*']:
            row['main_model'] = main
            with self.subTest(main=main):
                self.assertEqual(rra.models(row), [('derived', 'DERIVED-10')])
        row['main_model'] = 'MTYA-R,CP3060-RTM'
        self.assertEqual(rra.models(row), [('main', 'MTYA-R'), ('main', 'CP3060-RTM'),
                                           ('derived', 'DERIVED-10')])
