"""Offline transport guard and fresh-link protocol fixtures; no network."""
from pathlib import Path
from unittest import mock
from urllib.robotparser import RobotFileParser
import json,tempfile,unittest
import collect


class Response:
 def __init__(self,status=403,body=b'denied'):
  self.status=status;self.headers={};self.body=body
 def __enter__(self):return self
 def __exit__(self,*a):return None
 def read(self,limit):return self.body
 def geturl(self):return 'https://www.edge-core.com/robots.txt'


class CollectorTests(unittest.TestCase):
 def fixture(self,root):
  (root/'prior-url-inventory.json').write_text(json.dumps(dict(edgecore_urls=[])))
  (root/'prior-pdf-inventory.json').write_text(json.dumps(dict(local_pdfs=[],catalog_reference_documents=[])))
  (root/'selection-plan.json').write_text('{}')
  return collect.Collector(dict(selections=[]))

 def test_denial_saved_once_and_source_cannot_continue(self):
  with tempfile.TemporaryDirectory() as tmp,mock.patch.object(collect,'HERE',Path(tmp)):
   c=self.fixture(Path(tmp))
   with mock.patch.object(collect,'build_opener') as opener:
    opener.return_value.open.return_value=Response()
    with self.assertRaises(collect.Stop):c.fetch('https://www.edge-core.com/robots.txt','robots.txt','policy',{})
    self.assertEqual(opener.return_value.open.call_count,1)
    receipt=json.loads((Path(tmp)/'robots.txt.receipt.json').read_text())
    self.assertEqual(receipt['status'],403)
    self.assertEqual((Path(tmp)/'robots.txt').read_bytes(),b'denied')
    with self.assertRaises(collect.Stop):c.fetch('https://www.edge-core.com/another','next','policy',{})
    self.assertEqual(opener.return_value.open.call_count,1)

 def test_host_cap_prior_url_and_policy_fail_before_transport(self):
  with tempfile.TemporaryDirectory() as tmp,mock.patch.object(collect,'HERE',Path(tmp)),mock.patch.object(collect,'build_opener') as opener:
   c=self.fixture(Path(tmp))
   for url in ['https://elsewhere.test/a','http://www.edge-core.com/a','https://www.edge-core.com/a#x']:
    with self.assertRaises(collect.Stop):c.fetch(url,'x','policy',{})
   with self.assertRaises(collect.Stop):c.fetch('https://www.edge-core.com/a','x','landing',{})
   c.robot=RobotFileParser();c.robot.parse(['User-agent: *','Disallow: /denied'])
   with self.assertRaises(collect.Stop):c.fetch('https://www.edge-core.com/denied','x','landing',{})
   c.prior_keys.add('https://www.edge-core.com/old/')
   with self.assertRaises(collect.Stop):c.fetch('https://www.edge-core.com/old/?refresh=new','x','landing',{})
   c.entries=[dict(url=str(i)) for i in range(28)]
   with self.assertRaises(collect.Stop):c.fetch('https://www.edge-core.com/new','x','landing',{})
   opener.assert_not_called()

 def test_fresh_link_and_one_published_redirect_never_use_stale_token(self):
  with tempfile.TemporaryDirectory() as tmp,mock.patch.object(collect,'HERE',Path(tmp)):
   c=self.fixture(Path(tmp));c.results=[]
   base='https://www.edge-core.com/download/test-ds-r01/'
   fresh=base+'?wpdmdl=123&refresh=fresh'
   pdf='https://www.edge-core.com/wp-content/uploads/2026/01/test.pdf'
   c.plan['selections']=[dict(selection_number=1,slug='test-ds-r01',printed_document_heading='TEST Datasheet R01',landing_url=base,source_rows=[dict(wpdmdl_values=['123'],url=base+'?wpdmdl=123&refresh=stale')])]
   calls=[]
   def fetch(url,name,purpose,basis):
    calls.append(url)
    if purpose=='policy':return dict(status=200,sha256='policy'),b'User-agent: *\nDisallow:\n'
    if purpose=='landing':return dict(status=200,sha256='landing',body_path=name),('<h1 class="entry-title">TEST Datasheet R01</h1><a class="wpdm-download-link" href="'+fresh.replace('&','&amp;')+'">Download</a>').encode()
    if purpose=='published_download':return dict(status=302,url=fresh,headers={'Location':pdf},sha256='redirect',body_path=name),b''
    return dict(status=200,url=pdf,body_path=name),b'%PDF-fixture'
   with mock.patch.object(c,'fetch',side_effect=fetch):c.collect()
   self.assertEqual(calls,['https://www.edge-core.com/robots.txt',base,fresh,pdf])
   self.assertEqual(c.results[0]['outcome'],'new_pdf')
   self.assertEqual((Path(tmp)/'test-ds-r01.pdf').read_bytes(),b'%PDF-fixture')

 def test_rerun_cannot_retry_existing_reservation_ledger(self):
  with tempfile.TemporaryDirectory() as tmp,mock.patch.object(collect,'HERE',Path(tmp)),mock.patch.object(collect,'build_opener') as opener:
   root=Path(tmp);c=self.fixture(root)
   (root/'requests.jsonl').write_text('{}\n')
   with self.assertRaises(collect.Stop):c.run()
   opener.assert_not_called()


if __name__=='__main__':unittest.main()
