import json
import tempfile
import unittest
from pathlib import Path

from devicehunt import fcc_details as f, lookup
from devicehunt.catalog import Catalog, digest


def html(fid='HEDTEST0', company='Accton Technology Corp', text='Model EXAMPLE-1 radio antenna', truncated=True):
    return ('<h1>'+fid+' Fixture equipment</h1><div><h2>Application Details</h2><dl>'
            '<div><dt>Company</dt><dd>'+company+'</dd></div>'
            '<div><dt>Equipment Class</dt><dd>DTS</dd></div>'
            '<div><dt>Application Purpose</dt><dd>Original Equipment</dd></div>'
            '<div><dt>Date of Grant</dt><dd>Aug 10, 2020</dd></div></dl></div>'
            '<section><h3>Test Report</h3><a title="Open report" href="/api/files/s2/fccids/'+fid+'/files/Att_2.911(d)(5)&(ii)">PDF</a></section>'
            '<section id="document-text"><details><summary>Test Report</summary><div>'
            '<p class="whitespace-pre-wrap">'+text+'</p>'
            + ('<p>Text truncated - open the document above for the full version.</p>' if truncated else '')
            + '</div></details></section>').encode()


class FccDetailTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory(); self.root=Path(self.tmp.name)
        self.c=Catalog(self.root/'db.sqlite')
        self.c.source('fcc-index-hed',f.BASE,'regulatory_mirror','test','test')
        for fid in ('HEDTEST0','HEDTEST1'):
            self.c.db.execute('INSERT INTO fcc_id_index VALUES (?,?,?,?,?,?,?,?,?,?)',
                ('fcc-index-hed',fid,'HED','Accton Technology Corp','Aug 10, 2020','Fixture','DTS',
                 'Original Equipment',f.BASE+'/fcc/'+fid,f.BASE+'/company/HED'))
        self.c.db.commit()

    def tearDown(self):
        self.c.db.close(); self.tmp.cleanup()

    def bundle(self, text='Model EXAMPLE-1 radio antenna'):
        entries=[]
        for n,fid in enumerate(('HEDTEST0','HEDTEST1')):
            e=dict(fcc_id=fid,grantee_code='HED',grantee_name='Accton Technology Corp',source_url=f.BASE+'/fcc/'+fid)
            if n:
                e.update(status='unavailable',issue='HTTP 403; not bypassed')
            else:
                p=self.root/(fid+'.html'); p.write_bytes(html(fid,text=text))
                e.update(status='captured',page={'path':p.name,'sha256':digest(p.read_bytes())})
            entries.append(e)
        p=self.root/'bundle.json'; p.write_text(json.dumps({'schema_version':1,'mirror':f.BASE,'entries':entries})); return p

    def test_metadata_excerpts_links_and_unavailable_remain_separate(self):
        p=self.bundle()
        for _ in range(2): self.assertEqual(f.import_bundle(self.c,p),
            {'attempted':2,'captured':1,'unavailable':1,'links':1,'excerpts':1,'truncated_excerpts':1})
        self.assertEqual(self.c.search(),[])
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM fcc_applications').fetchone()[0],0)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM documents').fetchone()[0],0)
        row=self.c.db.execute('SELECT * FROM fcc_detail_pages WHERE fcc_id="HEDTEST0"').fetchone()
        self.assertEqual(json.loads(row['fields_json'])['Date of Grant'],'Aug 10, 2020')
        sections={s['dataset']:s for s in lookup.run(self.c.db,'antenna')['datasets']}
        self.assertEqual(sections['fcc_excerpts']['matches'],1)
        self.assertEqual(sections['fcc_excerpts']['rows'][0]['truncated'],1)
        self.assertIn('antenna',sections['fcc_excerpts']['rows'][0]['excerpt'])
        self.assertNotIn('text',sections['fcc_excerpts']['rows'][0])
        self.assertEqual(sections['catalog']['matches'],0)
        missing=next(s for s in lookup.run(self.c.db,'HEDTEST1')['datasets'] if s['dataset']=='fcc_details')
        self.assertEqual(missing['rows'][0]['status'],'unavailable')

    def test_refresh_removes_old_child_text_and_preserves_duplicates_by_ordinal(self):
        f.import_bundle(self.c,self.bundle())
        f.import_bundle(self.c,self.bundle('new text'))
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM fcc_text_excerpts').fetchone()[0],1)
        self.assertEqual(self.c.db.execute('SELECT text FROM fcc_text_excerpts').fetchone()[0],'new text')
        body=html(); start=body.index(b'<details>'); end=body.index(b'</details>')+len(b'</details>')
        parsed=f.parse_page(body[:end]+body[start:end]+body[end:],'HEDTEST0','Accton Technology Corp')
        self.assertEqual(len(parsed['excerpts']),2)  # Same category is not a document ID.
        self.assertFalse(f.parse_page(html(truncated=False),'HEDTEST0','Accton Technology Corp')['excerpts'][0]['truncated'])

    def test_hash_or_scope_failure_precedes_source_replacement(self):
        p=self.bundle();f.import_bundle(self.c,p)
        snapshot=[tuple(r) for r in self.c.db.execute('SELECT * FROM sources ORDER BY id')]
        b=json.loads(p.read_text()); b['entries'][0]['page']['sha256']='0'*64;p.write_text(json.dumps(b))
        with self.assertRaisesRegex(ValueError,'checksum'):f.import_bundle(self.c,p)
        self.assertEqual(snapshot,[tuple(r) for r in self.c.db.execute('SELECT * FROM sources ORDER BY id')])
        b['entries'].pop();p.write_text(json.dumps(b))
        with self.assertRaisesRegex(ValueError,'scope'):f.import_bundle(self.c,p)
        p=self.bundle();b=json.loads(p.read_text());b['entries'][0]['grantee_name']='Wrong Company';p.write_text(json.dumps(b))
        with self.assertRaisesRegex(ValueError,'mismatch'):f.import_bundle(self.c,p)

    def test_identity_company_duplicate_fields_and_truncation_changes_rejected(self):
        for old,new in [(b'<h1>HEDTEST0',b'<h1>HEDOTHER'),
                        (b'Accton Technology Corp',b'Wrong Company'),
                        (b'Text truncated - open the document above for the full version.',b'Unexpected truncation rule'),
                        (b'</dl>',b'<div><dt>Company</dt><dd>Accton Technology Corp</dd></div></dl>')]:
            with self.assertRaises(ValueError): f.parse_page(html().replace(old,new),'HEDTEST0','Accton Technology Corp')
        with self.assertRaises(ValueError):f.parse_page(html().replace(b'/api/files/s2/',b'/api/files/../'),'HEDTEST0','Accton Technology Corp')

    def test_snippets_follow_literal_unicode_match_and_limits(self):
        text='unrelated '*90+'Straße antenna '+'tail '*300
        f.import_bundle(self.c,self.bundle(text))
        s=next(s for s in lookup.run(self.c.db,'STRASSE')['datasets'] if s['dataset']=='fcc_excerpts')
        self.assertIn('Straße',s['rows'][0]['excerpt']); self.assertLess(len(s['rows'][0]['excerpt']),510)
        self.assertIn('…',s['rows'][0]['excerpt'])
        s=next(s for s in lookup.run(self.c.db,'antenna',0)['datasets'] if s['dataset']=='fcc_excerpts')
        self.assertEqual((s['matches'],s['shown']),(1,0))

    def test_detail_import_and_collector_exclude_other_mirror_supplements(self):
        self.c.source('fcc-index-reviewed-mrf','https://fccid.io/MRF','regulatory_mirror','fixture','partial reviewed extract')
        for fid in ('MRF13316C','HEDTEST0'):
            self.c.db.execute('INSERT INTO fcc_id_index VALUES (?,?,?,?,?,?,?,?,?,?)',
                ('fcc-index-reviewed-mrf',fid,'MRF','Celestica Incorporated','1997-05-16',
                 'Fixture','','Class II Permissive Change','https://fccid.io/'+fid,'https://fccid.io/MRF'))
        self.c.db.commit()
        self.assertEqual({r['fcc_id'] for r in f.detail_index_rows(self.c.db)},{'HEDTEST0','HEDTEST1'})
        self.assertTrue(all(r['source_url'].startswith(f.BASE+'/fcc/') for r in f.detail_index_rows(self.c.db)))
        self.assertEqual(f.import_bundle(self.c,self.bundle())['attempted'],2)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM fcc_id_index').fetchone()[0],4)
