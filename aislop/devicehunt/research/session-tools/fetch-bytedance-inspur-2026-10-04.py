"""Bounded original evidence downloads using system TLS; no retries."""
import datetime
import hashlib
import json
from pathlib import Path
import subprocess
ROOT = Path(__file__).resolve().parents[2] / 'data/raw/bytedance-inspur-2026-10-04'
ROOT.mkdir(exist_ok=True)
report = ROOT/'fetch-report.json'
entries = json.loads(report.read_text()) if report.exists() else [dict(
    url='https://inspur.com/en/2822784/index.html', status=403,
    method='web open', note='Known denied company page; do not retry or bypass.',
    checked_at=datetime.datetime.now(datetime.timezone.utc).isoformat())]
seen = {entry['url'] for entry in entries}
for name,url in (
    ('accton-b4010-compliance.pdf','https://www.accton.com.cn/wp-content/uploads/B4010-2302C014-CER_FCCISED.pdf'),
    ('inspur-i24-whitepaper.pdf','https://www.inspur.com/eportal/fileDir/defaultCurSite/resource/cms/2020/08/2020081316525840458.pdf'),
):
    if url in seen: continue
    path=ROOT/name;assert not path.exists()
    result=subprocess.run(['/usr/bin/curl','--silent','--show-error','--fail','--location',
        '--max-time','30','--max-filesize','20000000','--output',str(path),
        '--write-out','%{http_code} %{url_effective}',url],capture_output=True,text=True)
    entry=dict(url=url,attempted_filename=name,method='system curl with default TLS validation',
               checked_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),
               curl_exit=result.returncode,result=result.stdout,error=result.stderr)
    if result.returncode==0:
        body=path.read_bytes();assert body.startswith(b'%PDF-'),name
        entry.update(path=name,sha256=hashlib.sha256(body).hexdigest(),bytes=len(body))
    entries.append(entry);report.write_text(json.dumps(entries,indent=2)+'\n')
    print(json.dumps(entry),flush=True)
