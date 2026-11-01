import json
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from devicehunt import rra
from devicehunt.catalog import digest
from research import capture_rra_query as capture


class RRACaptureTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory(); self.root=Path(self.tmp.name)
        self.form=self.root/'form.html'
        self.form.write_text('<input name="maker"><script>"/ko/license/csv_down.do"</script>')
        self.before=self.root/'before.csv'
        self.before.write_bytes((','.join(rra.HEADER)+'\n1,Oracle,Server,MODEL-1,미국,2020-01-01,적합등록,R-R-SUN-MODEL1\n').encode('cp949'))
        self.values=['Oracle','Server','MODEL-1','','R-R-SUN-MODEL1','Oracle America, Inc.','미국','2020-01-01','']

    def tearDown(self):
        self.tmp.cleanup()

    def run_capture(self, name, after=None, fail=False):
        out=self.root/name
        def details(args,check):
            self.assertTrue(check)
            self.assertEqual(args[1:3],['-m','research.capture_rra_details'])
            html='<table>'+''.join('<tr><th>'+label+'</th><td>'+v+'</td></tr>'
                for (_,label),v in zip(rra.FIELDS,self.values))+'</table>'
            p=out/'detail-001.html';p.write_text(html)
            (out/'capture-progress.json').write_text(json.dumps(dict(complete=True,entries=[
                dict(path=p.name,sha256=digest(p.read_bytes()),certificate='R-R-SUN-MODEL1',url=rra.DETAIL+'R-R-SUN-MODEL1') ])))
        def download(url,path,query):
            self.assertEqual(url,rra.CSV_URL)
            self.assertEqual(query['maker'],'Oracle')
            if fail:raise subprocess.CalledProcessError(22,['curl'])
            path.write_bytes(self.before.read_bytes() if after is None else after)
        argv=['capture','Oracle',str(out),'--end','20260926','--form',str(self.form),'--before',str(self.before)]
        with patch('sys.argv',argv),patch.object(capture.subprocess,'run',side_effect=details),patch.object(capture,'download',side_effect=download):
            capture.main()
        return out

    def test_only_matching_complete_capture_publishes_bundle(self):
        out=self.run_capture('complete')
        self.assertTrue((out/'bundle.json').exists())
        self.assertFalse((out/'candidate.json').exists())
        _,rows=rra.validate(out/'bundle.json')
        self.assertEqual(len(rows),1)
        with self.assertRaises(ValueError):self.run_capture('changed',after=self.before.read_bytes()+b'\n')
        self.assertFalse((self.root/'changed'/'bundle.json').exists())

    def test_network_failure_retains_partial_capture_without_publication_or_retry(self):
        with self.assertRaises(subprocess.CalledProcessError):self.run_capture('failed',fail=True)
        self.assertTrue((self.root/'failed'/'before.csv').exists())
        self.assertFalse((self.root/'failed'/'bundle.json').exists())
        with self.assertRaises(FileExistsError):self.run_capture('failed')
