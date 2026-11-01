import json
import tempfile
import unittest
from pathlib import Path

from devicehunt import oracle_ecodesign as o, validation
from devicehunt.catalog import Catalog, digest


class OracleEcoDesignTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.root = Path(self.temp.name)
        self.c = Catalog(self.root/'db.sqlite'); self.pages = {}
        for key, (_, product, code, _) in o.DOCUMENTS.items():
            self.pages[key] = ('Technical Documentation for EU regulation 2019/424 '
                'Product Type Rack-mount Server Name/Address of Manufacturer Oracle America Inc …. '
                'Product Model Number Compliance Model Number ' + product +
                ' Server, all models as defined in the DoC ' + code +
                ' Year of Manufacture 2020 PSU Efficiency 10% Load: 90% '
                'Power Supply rated power output 1200W Idle State Power 100 Watts')

    def tearDown(self):
        self.c.db.close(); self.temp.cleanup()

    def save(self):
        data = dict(schema_version=1,documents={})
        for key,(url,_,_,count) in o.DOCUMENTS.items():
            pdf=self.root/(key+'.pdf');pdf.write_bytes(b'%PDF-1.7\nfixture '+key.encode())
            txt=self.root/(key+'.json');txt.write_text(json.dumps(dict(pdf_sha256=digest(pdf.read_bytes()), extraction_method='pypdf embedded text; no OCR',
                pages=[{'page':n,'text':self.pages[key] if n==1 else 'fixture'} for n in range(1,count+1)])))
            data['documents'][key] = dict(url=url,pdf=dict(path=pdf.name,sha256=digest(pdf.read_bytes())),
                text=dict(path=txt.name,sha256=digest(txt.read_bytes())),expected=o.parse_page(self.pages[key]))
        path=self.root/'bundle.json';path.write_text(json.dumps(data));return path

    def test_shared_code_keeps_separate_product_scopes_and_no_alias_pass(self):
        self.assertEqual(o.import_bundle(self.c,self.save()),{'sources':4,'assertions':4,'compliance_codes':3})
        self.assertEqual({r['model'] for r in self.c.search(target='Oracle',role='manufacturer')},{'WH1CL','WH2CL','ZU2XL'})
        did=self.c.search('ZU2XL')[0]['id'];detail=self.c.detail(did)
        self.assertEqual({f['value'] for f in detail['facts'] if f['name']=='product_model'},{'X10M','X11M'})
        self.assertEqual({i['kind'] for i in detail['identifiers']},{'ecodesign_product_scope'})
        self.assertEqual(self.c.search(hyperscaler=True),[])
        inv=self.root/'inventory.json';inv.write_text(json.dumps({'schema_version':1,'devices':[{'vendor':'Oracle','identifier':'X10M'}]}))
        self.assertEqual(validation.run(self.c.db,inv)['summary']['hits'],0)
        self.assertIn('not measured wall draw',next(f['value'] for f in detail['facts'] if f['name']=='power_description'))

    def test_scope_changes_and_bad_later_dependency_preserve_all_prior_sources(self):
        path=self.save();o.import_bundle(self.c,path)
        self.pages['x11m']=self.pages['x11m'].replace('ZU2XL','DIFFERENT')
        with self.assertRaisesRegex(ValueError,'reviewed fields'):o.import_bundle(self.c,self.save())
        self.pages['x11m']=self.pages['x11m'].replace('DIFFERENT','ZU2XL')
        path=self.save();(self.root/'x11m.pdf').write_bytes(b'changed')
        with self.assertRaisesRegex(ValueError,'checksum'):o.import_bundle(self.c,path)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM records').fetchone()[0],4)
        self.assertEqual({f['value'] for f in self.c.detail(self.c.search('ZU2XL')[0]['id'])['facts'] if f['name']=='compliance_model'},{'ZU2XL'})

    def test_table_ambiguity_and_manufacturer_changes_rejected_and_reimport_stable(self):
        text=self.pages['x9-2']
        for changed in [text.replace('Oracle America Inc ….','Different Company'),
                        text.replace('all models as defined in the DoC','selected models'),text+' '+text,
                        text.replace('Compliance Model Number','Other Model Number')]:
            with self.assertRaises(ValueError):o.parse_page(changed)
        path=self.save();o.import_bundle(self.c,path);o.import_bundle(self.c,path)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM devices').fetchone()[0],3)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM records').fetchone()[0],4)
