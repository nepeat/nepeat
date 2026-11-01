import json
import subprocess
import unittest
from unittest.mock import patch

from devicehunt import rra, lookup
from devicehunt.catalog import digest
from research import capture_rra_query as capture
from tests import test_rra


def empty_html():
    return ('<html><title>Communications Conformity Assessment Center</title>'
            '<h5>Search Results</h5><table><thead><tr>' +
            ''.join('<th>'+value+'</th>' for value in rra.RESULT_HEADER) +
            '</tr></thead><tbody><tr><td colspan="7">'+rra.EMPTY_MESSAGE+
            '</td></tr></tbody></table></html>').encode('cp949')


class RRAEmptyTests(unittest.TestCase):
    def setUp(self):
        self.fixture=test_rra.RRATests();self.fixture.setUp()
        self.c=self.fixture.c
        self.header=(','.join(rra.HEADER)+'\n').encode('cp949')

    def tearDown(self):
        self.fixture.tearDown()

    def save_empty(self):
        self.fixture.csv=self.header
        path=self.fixture.save();data=json.loads(path.read_text())
        page=self.fixture.root/'empty.html';page.write_bytes(empty_html())
        data.update(expected_rows=0,details=[],empty_result=dict(
            path=page.name,url=rra.RESULT_URL,sha256=digest(page.read_bytes())))
        path.write_text(json.dumps(data));return path

    def test_explicit_empty_refresh_only_withdraws_its_own_query(self):
        rra.import_bundle(self.c,self.fixture.save())
        self.c.source('other','https://example.org','vendor','fixture','fixture')
        self.c.add('other','x','https://example.org','Oracle','KEPT-1','kept','server',{})
        self.c.finish('other',1)
        result=rra.import_bundle(self.c,self.save_empty())
        self.assertEqual(result,dict(registry_rows=0,model_assertions=0))
        self.assertEqual([r[0] for r in self.c.db.execute('SELECT model FROM devices')],['KEPT-1'])
        source=self.c.db.execute("SELECT * FROM sources WHERE id LIKE 'rra-official-%'").fetchone()
        self.assertEqual(source['records_seen'],0)
        self.assertIn('dated query-level observation',source['coverage'])
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM rra_equipment').fetchone()[0],0)
        with self.assertRaises(ValueError):rra.csv_rows(self.header)
        self.assertEqual(rra.csv_rows(self.header,allow_empty=True),[])
        section=next(s for s in lookup.run(self.c.db,'nothing')['datasets'] if s['dataset']=='rra')
        self.assertEqual(section['state'],'loaded')
        self.assertEqual(section['stored_rows'],0)
        self.assertEqual(section['sources'][0]['records_seen'],0)

    def test_zero_query_metadata_is_visible_alongside_nonempty_queries(self):
        rra.import_bundle(self.c,self.fixture.save())
        path=self.save_empty();data=json.loads(path.read_text());data['query']['maker']='Edge Core'
        path.write_text(json.dumps(data));rra.import_bundle(self.c,path)
        section=next(s for s in lookup.run(self.c.db,'MAIN-10')['datasets'] if s['dataset']=='rra')
        self.assertEqual(section['state'],'loaded')
        self.assertEqual(section['stored_rows'],2)
        self.assertEqual(section['matches'],1)
        self.assertEqual(sorted(s['records_seen'] for s in section['sources']),[0,2])

    def test_missing_wrong_or_contradictory_empty_proof_fails_before_replacement(self):
        rra.import_bundle(self.c,self.fixture.save())
        before=[tuple(r) for r in self.c.db.execute('SELECT * FROM records')]
        for change in ('missing','hash','url','header','error_page','extra_row','extra_table','details','count','nonempty'):
            path=self.save_empty();data=json.loads(path.read_text());page=self.fixture.root/'empty.html'
            if change=='missing':del data['empty_result']
            elif change=='hash':page.write_bytes(b'changed')
            elif change=='url':data['empty_result']['url']='https://example.org/no-results'
            elif change=='details':data['details']=[{}]
            elif change=='count':data['expected_rows']=1
            elif change=='nonempty':
                original=test_rra.RRATests();original.setUp()
                try:body=original.csv
                finally:original.tearDown()
                for key in ('csv_before','csv_after'):
                    (path.parent/data[key]['path']).write_bytes(body)
                    data[key]['sha256']=digest(body)
            else:
                body=empty_html()
                if change=='header':body=body.replace(b'Applicant',b'Unknown')
                if change=='error_page':body=('<p>'+rra.EMPTY_MESSAGE+'</p>').encode('cp949')
                if change=='extra_row':body=body.replace(b'</tbody>',b'<tr><td>MODEL-1</td></tr></tbody>')
                if change=='extra_table':body+=b'<table></table>'
                page.write_bytes(body);data['empty_result']['sha256']=digest(body)
            path.write_text(json.dumps(data))
            with self.assertRaises(ValueError,msg=change):rra.import_bundle(self.c,path)
            self.assertEqual([tuple(r) for r in self.c.db.execute('SELECT * FROM records')],before,change)

    def test_collector_confirms_empty_result_and_csv_without_detail_requests(self):
        path=self.fixture.save();data=json.loads(path.read_text())
        before=path.parent/'empty.csv';before.write_bytes(self.header)
        out=path.parent/'capture'
        calls=[]
        def download(url,destination,query):
            calls.append(url);self.assertEqual(query['maker'],'Edge Core')
            destination.write_bytes(empty_html() if url==rra.RESULT_URL else self.header)
        argv=['capture','Edge Core',str(out),'--end','20260926','--form',
              str(path.parent/data['form']['path']),'--before',str(before)]
        with patch('sys.argv',argv),patch.object(capture,'download',side_effect=download),patch.object(capture.subprocess,'run') as details:
            capture.main();details.assert_not_called()
        self.assertEqual(calls,[rra.RESULT_URL,rra.CSV_URL])
        bundle,rows=rra.validate(out/'bundle.json')
        self.assertEqual(rows,[]);self.assertEqual(bundle['expected_rows'],0)

    def test_collector_does_not_publish_header_only_export_when_confirmation_fails(self):
        path=self.fixture.save();data=json.loads(path.read_text())
        before=path.parent/'empty.csv';before.write_bytes(self.header)
        out=path.parent/'failed'
        argv=['capture','Edge Core',str(out),'--end','20260926','--form',
              str(path.parent/data['form']['path']),'--before',str(before)]
        with patch('sys.argv',argv),patch.object(capture,'download',side_effect=subprocess.CalledProcessError(22,['curl'])) as download:
            with self.assertRaises(subprocess.CalledProcessError):capture.main()
            self.assertEqual(download.call_count,1)
        self.assertTrue((out/'before.csv').exists())
        self.assertFalse((out/'bundle.json').exists())
