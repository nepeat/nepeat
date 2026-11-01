"""Offline fixture and full-capture acceptance checks for the private bulk dataset."""
import json
from pathlib import Path
import unittest
import tempfile
from extract_bulk import Document,extract_tables,extract_cards,extract_amd,extract,discovery_rows,sha,source_records
from compare_novelty import compare,proposal
BASE=Path(__file__).resolve().parent

class BulkTests(unittest.TestCase):
    def test_full_suffix_blanks_duplicates_and_nested_blocks(self):
        d=Document((BASE/'fixtures/tables.html').read_bytes());t=extract_tables(d)[0]
        self.assertEqual(t['table_id'],'systems');self.assertEqual(len(t['rows']),6)
        self.assertEqual([r['cells'][1]['text'] for r in t['rows'][1:4]],['SV7220G3-V+','SV7220G3-V','SV7220G3-V+'])
        self.assertEqual(t['rows'][1]['cells'][2]['text'],'')
        self.assertEqual(t['rows'][2]['cells'][2]['text'],'T4 L4')
        self.assertEqual(t['rows'][1]['cells'][1]['links'][0]['attributes']['href'],'/SV7220G3-V%2B')
    def test_span_and_footnote_identity_never_silently_forward_filled(self):
        t=extract_tables(Document((BASE/'fixtures/tables.html').read_bytes()))
        result={'sources':[{'source':{'dataset':'test','url':'fixture','path':'fixture','sha256':'fixture'},'tables':t}]}
        rows=discovery_rows(result)
        self.assertTrue(rows[3]['has_spans']);self.assertTrue(rows[3]['has_identity_superscripts'])
        self.assertIsNone(rows[4]['partner'])
        nov=compare(rows,[],[]);p=proposal(rows,nov)
        self.assertEqual(len(p['held_rows']),1)
        self.assertNotIn('D75E-4U3',[c['printed_model'] for c in p['candidates']])
    def test_cards_family_full_code_per_node_and_unicode_byte_offsets(self):
        raw=(BASE/'fixtures/cards.html').read_bytes();doc=Document(raw);cards=extract_cards(doc)
        self.assertEqual(len(cards),2);self.assertEqual(cards[0]['family_heading'],'family B (2)')
        self.assertEqual(cards[0]['model'],'B8056G68AE12HR-2T-TU-HE')
        self.assertEqual(cards[0]['fields'][0]['value']['text'],'2 / per node')
        self.assertEqual(cards[0]['fields'][1]['value']['text'],'')
        proof=cards[0]['provenance'];self.assertEqual(sha(raw[proof['byte_start']:proof['byte_end']]),proof['fragment_sha256'])
        self.assertGreater(proof['byte_start'],proof['char_start'])
    def test_amd_keeps_centered_partner_and_continuation_separate(self):
        side={'pdf_page_count':1,'pages':[{'page':1,'text':'       PARTNER             PRODUCT              CPU\n                           AS -2115HV-TNRT       2\n       SUPERMICRO\n                           (water cooled)\nSOLUTION CATALOG: x\n'}]}
        amd=extract_amd(side);lines=amd['pages'][0]['tables'][0]['lines']
        self.assertEqual(lines[0]['product_fragment'],'AS -2115HV-TNRT')
        self.assertEqual(lines[1]['partner_column_raw'].strip(),'SUPERMICRO')
        self.assertIsNone(lines[1]['product_fragment']);self.assertEqual(lines[2]['product_fragment'],'(water cooled)')
        self.assertIn('not inferred',amd['partner_mapping'])
    def test_amd_rejects_missing_or_reordered_pages(self):
        for side in [{'pdf_page_count':2,'pages':[{'page':1,'text':''}]},{'pdf_page_count':1,'pages':[{'page':2,'text':''}]}]:
            with self.assertRaises(ValueError):extract_amd(side)
    def test_exact_brand_and_cross_namespace_hits_are_distinct(self):
        rows=[{'dataset':'x','source_url':'x','locator':{},'model':'MODEL+','partner':p,'identity_scope':'literal'} for p in ['A','B',None]]
        devices=[{'id':'a','brand':'A','model':'MODEL+','revision':''}]
        detail=compare(rows,devices,[])
        self.assertEqual(detail[0]['status'],'exact_printed_partner_and_model')
        self.assertEqual(detail[1]['status'],'same_literal_model_other_or_unknown_partner')
        self.assertEqual(detail[2]['status'],'same_literal_model_other_or_unknown_partner')
        rows[0]['model']='model+';self.assertEqual(compare(rows,devices,[])[0]['status'],'no_literal_model_or_identifier_hit')
    def test_identifier_only_does_not_create_exact_pair(self):
        r={'dataset':'x','source_url':'x','locator':{},'model':'SKU-01','partner':'A','identity_scope':'literal'}
        hit=compare([r],[],[{'value':'SKU-01','device_id':'b','brand':'B'}])[0]
        self.assertEqual(hit['status'],'literal_identifier_hit_only');self.assertEqual(hit['exact_partner_model_hits'],[])
    def test_complete_capture_count_content_routing_and_determinism(self):
        d=extract(BASE);self.assertEqual(d,extract(BASE));rows=discovery_rows(d)
        self.assertEqual(len(d['sources']),6);self.assertEqual(len(rows),1120)
        self.assertEqual(rows,json.loads((BASE/'discovery-rows.json').read_text()))
        self.assertEqual(sum(len(s.get('tables',[])) for s in d['sources']),21)
        ngc=next(s for s in d['sources'] if s['source']['dataset']=='nvidia-ngc-ready')
        self.assertEqual(ngc['source']['format'],'html');self.assertEqual(ngc['source']['path'],'redirect-nvidia-ngc-ready.pdf')
        self.assertEqual(len(next(s for s in d['sources'] if 'pages' in s)['pages']),6)
        self.assertEqual(sum(len(s.get('cards',[])) for s in d['sources']),38)
        self.assertTrue(any(r['model']=='B8056G68CE12HR-2T-TU-HE' for r in rows))
        self.assertTrue(any(r['model']=='Miramar -ICEMan' for r in rows))
        self.assertTrue(any(r['model']=='Miramar - ICEMan' for r in rows))
        self.assertEqual(d,json.loads((BASE/'bulk-extracted.json').read_text()))
    def test_receipt_hash_validation_and_redirect_exclusion(self):
        with tempfile.TemporaryDirectory(dir=BASE) as tmp:
            p=Path(tmp);body=b'<html></html>';headers=b'HTTP/2 200\ncontent-type: text/html\n'
            (p/'body.pdf').write_bytes(body);(p/'headers').write_bytes(headers)
            record={'is_policy':False,'status':200,'body_path':'body.pdf','body_sha256':sha(body),
                    'header_path':'headers','header_sha256':sha(headers),'dataset':'fixture','url':'https://example.invalid/x',
                    'finished_at_utc':'fixture','sequence':1}
            redirect={'is_policy':False,'status':301}
            (p/'requests.jsonl').write_text(json.dumps(record)+'\n'+json.dumps(redirect)+'\n')
            self.assertEqual(source_records(p)[0]['format'],'html')
            self.assertEqual(len(source_records(p)),1)
            (p/'body.pdf').write_bytes(body+b'changed')
            with self.assertRaisesRegex(ValueError,'capture hash mismatch'):source_records(p)
            (p/'body.pdf').write_bytes(body);(p/'headers').write_bytes(b'changed')
            with self.assertRaisesRegex(ValueError,'header hash mismatch'):source_records(p)
    def test_superscript_without_span_is_held_from_proposal(self):
        d=Document(b'<html><table><tr><th>Partner</th><th>System</th></tr><tr><td>QCT</td><td>D75E-4U<sup>3</sup></td></tr></table></html>')
        result={'sources':[{'source':{'dataset':'x','url':'x','path':'x','sha256':'x'},'tables':extract_tables(d)}]}
        rows=discovery_rows(result);proposed=proposal(rows,compare(rows,[],[]))
        self.assertEqual(proposed['candidates'],[])
        self.assertIn('superscript',proposed['held_rows'][0]['reason'])
    def test_every_html_fragment_hash_resolves_to_original_bytes(self):
        d=extract(BASE)
        def check(obj,raw):
            if isinstance(obj,dict):
                if {'byte_start','byte_end','fragment_sha256'}<=obj.keys():
                    self.assertEqual(sha(raw[obj['byte_start']:obj['byte_end']]),obj['fragment_sha256'])
                for v in obj.values():check(v,raw)
            elif isinstance(obj,list):
                for v in obj:check(v,raw)
        for s in d['sources']:
            if s['source']['format']=='html':check(s,(BASE/s['source']['path']).read_bytes())

if __name__=='__main__':unittest.main(verbosity=2)
