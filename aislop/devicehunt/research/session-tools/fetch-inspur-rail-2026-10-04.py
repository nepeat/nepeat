"""Single-attempt capture of selected public product sources; preserve outcomes."""
import datetime
import hashlib
import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DEST = ROOT / 'data/raw/inspur-rail-2026-10-04'
DEST.mkdir(exist_ok=True)
report = DEST / 'fetch-report.json'
entries = json.loads(report.read_text()) if report.exists() else []
seen = {entry['url'] for entry in entries}
for name, url in (
    ('inventec-download-page-2.html', 'https://ebg.inventec.com/en/support/download-center?page=2'),
    ('inventec-download-page-3.html', 'https://ebg.inventec.com/en/support/download-center?page=3'),
    ('inspur-incloud-rail-compatibility.pdf', 'https://partner.inspur.com/eportal/fileDir/lcjtww/resource/cms/2024/03/2024032014450874883.pdf'),
    ('inspur-ne5260m5-launch.html', 'https://partner.inspur.com/qudao/2392089/2392123/2025021410101280931/index.html'),
):
    if url in seen:
        continue
    path = DEST / name
    assert not path.exists()
    proc = subprocess.run(['/usr/bin/curl', '--silent', '--show-error', '--fail', '--location',
        '--max-time', '40', '--max-filesize', '25000000', '--output', str(path),
        '--write-out', '%{http_code} %{url_effective}', url], capture_output=True, text=True)
    entry = dict(url=url, attempted_filename=name, method='system curl; default TLS validation; no retries',
        checked_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),
        curl_exit=proc.returncode, result=proc.stdout, error=proc.stderr)
    if proc.returncode == 0:
        body = path.read_bytes()
        entry.update(path=name, sha256=hashlib.sha256(body).hexdigest(), bytes=len(body))
        if name.endswith('.pdf'):
            entry['pdf_header_valid'] = body.startswith(b'%PDF-')
    entries.append(entry)
    report.write_text(json.dumps(entries, indent=2) + '\n')
    print(json.dumps(entry), flush=True)
    if proc.returncode and ('429' in proc.stdout or '429' in proc.stderr):
        break
