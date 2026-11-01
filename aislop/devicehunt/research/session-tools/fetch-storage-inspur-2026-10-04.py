"""Capture a bounded set of reviewed original URLs once; preserve all outcomes."""
import datetime
import hashlib
import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DEST = ROOT / 'data/raw/storage-inspur-2026-10-04'
DEST.mkdir(exist_ok=True)
report = DEST / 'fetch-report.json'
entries = json.loads(report.read_text()) if report.exists() else []
seen = {entry['url'] for entry in entries}
for name, url in (
    ('inventec-storage.html', 'https://ebg.inventec.com/en/product/Storage'),
    ('inspur-download-index.html', 'https://www.inspur.com/eportal/ui?pageId=2317460'),
    ('inspur-nf5280m5-whitepaper.pdf', 'https://en.inspur.com/eportal/fileDir/defaultCurSite/resource/cms/2020/04/2020040710453910867.pdf'),
    ('inventec-e850g4-brief.pdf', 'https://ebg.inventec.com/en/tool-download/product_files/file/80?note='),
    ('inventec-metagross.html', 'https://ebg.inventec.com/en/product/Storage/2U%2042%20Bay/Metagross'),
    ('inspur-product-names.json', 'https://www.inspur.com/eportal/ui?portal.url=/portlet/type-tree-front!getAllProductName.portlet&pageId=2317460&moduleId=973866bf97a740b5bca6cf179cc2523c'),
    ('inventec-download-index.html', 'https://ebg.inventec.com/en/support/download-center'),
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
