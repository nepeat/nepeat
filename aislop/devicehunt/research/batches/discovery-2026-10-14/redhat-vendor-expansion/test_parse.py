import copy,importlib.util,json,unittest
from pathlib import Path
HERE=Path(__file__).resolve().parent
spec=importlib.util.spec_from_file_location('redhat_bulk_parse',HERE/'parse.py');m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
class ListingTests(unittest.TestCase):
 def setUp(self):
  self.partner='Exact Vendor';self.source={'id':'s','snapshot':'fixture.json','original_sha256':'a'*64}
  self.row={'id':'001','partnerName':self.partner,'documentKind':'CertifiedHardware','certified_category':'System','system_types':['Server'],'model':'X-1 Rev B','partnerProductName':'Other Brand X-1 Rev B','view_uri':'https://catalog.redhat.com/hardware/system/detail/001','make':'Other Brand','certified_versions':['RHEL 8','RHEL 9']}
  self.data={'response':{'start':0,'numFound':1,'docs':[self.row]}}
 def parse(self):return m.parse_listing(self.data,self.partner,self.source)
 def test_identity_role_and_disparate_make_stay_literal(self):
  rows,c=self.parse();r=rows[0];self.assertEqual((r['brand'],r['model'],r['title']),(self.partner,'X-1 Rev B','Other Brand X-1 Rev B'));self.assertEqual(r['affiliations'],[[self.partner,'listing_holder']]);self.assertIn(['redhat_cert_make_field_as_printed','Other Brand'],r['facts']);self.assertEqual(r['key'],'001');self.assertTrue(c['complete_query'])
 def test_desktop_and_cloud_are_excluded_edge_retained(self):
  self.row['system_types']=['Desktop/Workstation'];self.assertEqual(self.parse()[0],[])
  self.row['system_types']=['Desktop/Workstation','Edge System'];self.assertEqual(self.parse()[0][0]['kind'],'edge system')
  self.row['certified_category']='Cloud Instance Type';self.assertEqual(self.parse()[0],[])
 def test_duplicate_certificate_fails_not_collapses(self):
  self.data['response']['docs'].append(copy.deepcopy(self.row));self.data['response']['numFound']=2
  with self.assertRaisesRegex(ValueError,'Duplicate'):self.parse()
 def test_partner_mismatch_fails(self):
  self.row['partnerName']='Different Vendor'
  with self.assertRaisesRegex(ValueError,'mismatch'):self.parse()
 def test_partial_query_is_explicit(self):
  self.data['response']['numFound']=2000;rows,c=self.parse();self.assertEqual(len(rows),1);self.assertFalse(c['complete_query']);self.assertEqual(c['numFound'],2000)
 def test_missing_identity_or_wrong_url_fails(self):
  self.row['model']=''
  with self.assertRaisesRegex(ValueError,'Missing'):self.parse()
  self.row['model']='X';self.row['view_uri']='https://example.invalid/'
  with self.assertRaisesRegex(ValueError,'Unexpected'):self.parse()
 def test_real_captures_replay_exactly(self):
  candidate,coverage=m.build();self.assertEqual(candidate,json.loads((HERE/'candidate.json').read_text()));self.assertEqual(coverage,json.loads((HERE/'coverage.json').read_text()));self.assertEqual(sum(x['captured'] for x in coverage),245);self.assertEqual(len(candidate['devices']),242);self.assertTrue(all(c['complete_query'] for c in coverage))
if __name__=='__main__':unittest.main()
