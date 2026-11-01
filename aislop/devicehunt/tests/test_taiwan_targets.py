import csv
import io
import json
import tempfile
import unittest
import xml.etree.ElementTree as ET
from pathlib import Path

from devicehunt import bsmi, ncc, bsmi_additional, vendor_inventory, taiwan_targets as t
from devicehunt.catalog import Catalog, digest


class TaiwanTargetTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(); self.root = Path(self.temp.name)
        self.c = Catalog(self.root / 'db.sqlite')
        self.bsmi_rows = [dict(vendor_code='001', certificate_number='B1', applicant='美商甲骨文有限公司台灣分公司',
                               expiry_raw='2020/01/01', product_name='Server', primary_model='E2ES2U (Ampere CPU)', status_raw='Expired')]
        self.ncc_rows = []
        for brand, model, applicant, manufacturer in [
            ('Intel', '9260NGW', '美商甲骨文有限公司台灣分公司', ''),
            ('Aruba', 'AP-52-W', 'Other Applicant', '智邦科技股份有限公司'),
            ('Other', 'DONT-IMPORT', '智邦科技股份有限公司 subsidiary', ''),
            ('Other', 'MULTI-COMPANY', '', '智邦科技股份有限公司/Other Company'),
            ('Other', '(申請者申請保密)', '智邦科技股份有限公司', ''),
            ('Other', '', '智邦科技股份有限公司', '')]:
            r = dict.fromkeys([k for k, _ in ncc.FIELDS], '')
            r.update(brand=brand, model=model, applicant=applicant, manufacturer=manufacturer, certificate_number=model)
            self.ncc_rows.append(r)
        self.pages = {
            'Oracle': '<li><h4>美商甲骨文有限公司台灣分公司 – 台北辦事處 辦公室</h4><ul><li>Oracle Taiwan LLC, Taiwan Branch</li></ul></li>',
            'Accton': '<p>智邦科技股份有限公司 (Accton)</p>'}

    def tearDown(self):
        self.c.db.close(); self.temp.cleanup()

    def save(self):
        data = dict(schema_version=1, companies={}, bsmi_summary=bsmi.summary(self.bsmi_rows),
                    ncc_summary=ncc.summary(self.ncc_rows), expected=t.summary(t.select_rows(self.bsmi_rows,self.ncc_rows)))
        def dep(name, body, url):
            (self.root/name).write_bytes(body)
            return dict(path=name, sha256=digest(body), url=url)
        for target,url in t.MAPPINGS.values():
            if target not in self.pages: continue
            data['companies'][target] = dep(target+'.html', self.pages[target].encode(), url)
        root = ET.Element('bsmi')
        for r in self.bsmi_rows:
            element = ET.SubElement(root,'row')
            for k,label in bsmi.FIELDS: ET.SubElement(element,label).text = r[k]
        data['bsmi'] = dep('bsmi.xml',ET.tostring(root,encoding='utf-8'),bsmi.XML_URL)
        stream = io.StringIO();writer = csv.writer(stream);writer.writerow([label for _,label in ncc.FIELDS])
        writer.writerows([[r[k] for k,_ in ncc.FIELDS] for r in self.ncc_rows])
        data['ncc'] = dep('ncc.csv',stream.getvalue().encode(),ncc.CSV_URL)
        if 'Edge-Core' in self.pages:
            data['schema_version'] = 2
            root = ET.Element('bsmi')
            for r in self.product_rows:
                element = ET.SubElement(root,'row')
                for k,label in bsmi_additional.DATASETS['162936']['fields']:
                    ET.SubElement(element,label).text = r[k]
            data['bsmi_product'] = dep('product.xml',ET.tostring(root,encoding='utf-8'),bsmi_additional.urls('162936')[1])
            data['bsmi_product_summary'] = bsmi_additional.summary(self.product_rows,'162936')
            data['expected'] = t.summary(t.select_rows(self.bsmi_rows,self.ncc_rows,self.product_rows))
        path = self.root/'bundle.json';path.write_text(json.dumps(data));return path

    def test_roles_brands_and_unsplit_models_not_operator_or_factory(self):
        counts = t.import_bundle(self.c,self.save());self.assertEqual(counts['assertions'],3)
        self.assertEqual({r['model'] for r in self.c.search(target='Oracle')},{'E2ES2U (Ampere CPU)','9260NGW'})
        self.assertEqual(self.c.search('9260NGW')[0]['brand'],'Intel')
        self.assertEqual(self.c.search('AP-52-W')[0]['brand'],'Aruba')
        self.assertEqual(len(self.c.search(target='Accton',role='registry_manufacturer')),1)
        self.assertEqual(self.c.search(target='Oracle',role='registry_manufacturer'),[])
        self.assertEqual(self.c.search(target='Accton',role='regulatory_applicant'),[])
        self.assertEqual(self.c.search(hyperscaler=True),[])
        self.assertEqual(self.c.search(role='manufacturer'),[])
        self.assertEqual({r[0] for r in self.c.db.execute('SELECT kind FROM identifiers')},
                         {'ncc_certificate_number','bsmi_certificate_number'})
        self.assertFalse(self.c.db.execute("SELECT 1 FROM devices WHERE model='E2ES2U'").fetchone())
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM bsmi_certificates').fetchone()[0],0)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM ncc_equipment').fetchone()[0],0)

    def test_exact_matching_company_evidence_and_all_dependencies_before_replacement(self):
        path=self.save();t.import_bundle(self.c,path)
        for key in ['Oracle','Accton']:
            old=self.pages[key];self.pages[key]='<script>'+old+'</script>'
            with self.assertRaises(ValueError):t.import_bundle(self.c,self.save())
            self.pages[key]=old
            self.assertEqual(self.c.db.execute('SELECT count(*) FROM records').fetchone()[0],3)
        path=self.save();(self.root/'ncc.csv').write_text('changed')
        with self.assertRaisesRegex(ValueError,'checksum'):t.import_bundle(self.c,path)
        path=self.save();data=json.loads(path.read_text());data['expected']['assertions']=4;path.write_text(json.dumps(data))
        with self.assertRaisesRegex(ValueError,'counts'):t.import_bundle(self.c,path)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM records').fetchone()[0],3)

    def test_idempotent_refresh_and_no_global_alias_side_effects(self):
        from devicehunt.catalog import target_for
        path=self.save();t.import_bundle(self.c,path);t.import_bundle(self.c,path)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM records').fetchone()[0],3)
        self.ncc_rows=self.ncc_rows[:1];t.import_bundle(self.c,self.save())
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM records').fetchone()[0],2)
        self.assertEqual(self.c.search(target='Accton'),[])
        self.assertIsNone(target_for('智邦科技股份有限公司'))
        self.assertIsNone(target_for('美商甲骨文有限公司台灣分公司'))

    def edgecore_fixture(self):
        self.pages['Edge-Core'] = '<table>' + ''.join('<tr><td>'+k+'</td><td>'+v+'</td></tr>' for k,v in [
            ('廠商編號','A0902'),('廠商名稱 (中文)','鈺登科技股份有限公司'),
            ('廠商名稱 (英文)','Edgecore Networks Corporation'),('營利事業 統一編號','25063543')]) + '</table>'
        fields=bsmi_additional.DATASETS['162936']['fields']
        row=dict.fromkeys([k for k,_ in fields],'')
        row.update(certificate_number='EC001',applicant='鈺登科技股份有限公司',primary_model='ECS-001-AC',product_name='Switch')
        self.product_rows=[row]
        for brand,model,applicant,manufacturer in [
            ('SMC','SMC-LABEL','鈺登科技股份有限公司','智邦科技股份有限公司'),
            ('Edgecore','EC-MFG','Other Applicant','鈺登科技股份有限公司'),
            ('Other','EXCLUDE-SUBSIDIARY','鈺登科技股份有限公司 subsidiary',''),
            ('Other','EXCLUDE-MULTI','','鈺登科技股份有限公司/Other')]:
            row=dict.fromkeys([k for k,_ in ncc.FIELDS],'')
            row.update(brand=brand,model=model,applicant=applicant,manufacturer=manufacturer,certificate_number=model)
            self.ncc_rows.append(row)

    def test_edgecore_government_name_mapping_and_three_registry_roles(self):
        self.edgecore_fixture();counts=t.import_bundle(self.c,self.save())
        self.assertEqual(counts['assertions'],6)
        self.assertEqual(self.c.search('SMC-LABEL')[0]['brand'],'SMC')
        self.assertEqual({r['model'] for r in self.c.search(target='Edge-Core',role='regulatory_applicant')},
                         {'SMC-LABEL','ECS-001-AC'})
        self.assertEqual({r['model'] for r in self.c.search(target='Accton',role='registry_manufacturer')},
                         {'AP-52-W','SMC-LABEL'})
        inventory=vendor_inventory.run(self.c.db,'Edge-Core',limit=-1)
        self.assertEqual({r['model'] for r in inventory['rows']},{'EC-MFG'})
        self.assertEqual(self.c.search(hyperscaler=True),[])
        self.assertEqual(self.c.search(role='listed_factory'),[])
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM bsmi_product_certificates').fetchone()[0],0)
        t.import_bundle(self.c,self.save())
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM records').fetchone()[0],6)
        from devicehunt.catalog import target_for
        self.assertIsNone(target_for('鈺登科技股份有限公司'))

    def test_edgecore_name_pair_cannot_be_joined_across_companies_or_unchecked_dependency(self):
        self.edgecore_fixture();t.import_bundle(self.c,self.save())
        original=self.pages['Edge-Core']
        for bad in [original.replace('25063543','00000000'),
                    original.replace('Edgecore Networks Corporation','Other Corporation'),
                    original.replace('<tr><td>廠商名稱 (英文)','</table><table><tr><td>廠商名稱 (英文)'),
                    '<script>'+original+'</script>']:
            self.pages['Edge-Core']=bad
            with self.assertRaises(ValueError):t.import_bundle(self.c,self.save())
            self.assertEqual(self.c.db.execute('SELECT count(*) FROM records').fetchone()[0],6)
        self.pages['Edge-Core']=original;path=self.save()
        (self.root/'product.xml').write_text('changed')
        with self.assertRaisesRegex(ValueError,'checksum'):t.import_bundle(self.c,path)
        self.assertEqual(self.c.db.execute('SELECT count(*) FROM records').fetchone()[0],6)

    def test_version_one_keeps_only_its_verified_company_scope(self):
        self.edgecore_fixture();path=self.save();data=json.loads(path.read_text())
        data['schema_version']=1;data['companies'].pop('Edge-Core')
        data.pop('bsmi_product');data.pop('bsmi_product_summary')
        legacy={name:item for name,item in t.MAPPINGS.items() if item[0] in {'Accton','Oracle'}}
        data['expected']=t.summary(t.select_rows(self.bsmi_rows,self.ncc_rows,mappings=legacy))
        path.write_text(json.dumps(data));result=t.import_bundle(self.c,path)
        self.assertEqual(result['assertions'],4)  # SMC row still qualifies through its Accton manufacturer.
        self.assertEqual(self.c.search(target='Edge-Core',role='regulatory_applicant'),[])
        self.assertEqual(self.c.search(target='Edge-Core',role='registry_manufacturer'),[])
        self.assertFalse(self.c.db.execute("SELECT 1 FROM sources WHERE id='bsmi-product-reviewed-targets'").fetchone())
