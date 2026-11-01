"""Fetch only reviewed primary source URLs; cache once, no retries."""
import datetime
import hashlib
import json
import subprocess
from pathlib import Path
ROOT = Path(__file__).resolve().parents[2]
DEST = ROOT / 'data/raw/inventec-qct-2026-10-04'
DEST.mkdir(exist_ok=True)
report = DEST / 'fetch-report.json'
entries = json.loads(report.read_text()) if report.exists() else [dict(
    url='https://www.qct.io/en-US/Download/index/E5-2600-v4-Broadwell-CPU-Upgrade-Package',
    method='web open', status=403, note='Denied on 2026-10-04; do not retry or use alternate delivery.')]
seen = {e['url'] for e in entries}
for name, url in (
    ('inventec-ebg.html', 'https://ebg.inventec.com/en/EBG'),
    ('inventec-products-en.html', 'https://ebg.inventec.com/en/product'),
    ('inventec-products-cn.html', 'https://ebg.inventec.com/cn/product'),
    ('inventec-horsea.html', 'https://ebg.inventec.com/cn/product/%E6%9C%8D%E5%8A%A1%E5%99%A8/2U/Horsea'),
    ('inventec-seadra.html', 'https://ebg.inventec.com/cn/product/%E6%9C%8D%E5%8A%A1%E5%99%A8/2U/Seadra'),
    ('quanta-affiliates-2025.pdf', 'https://www.quantatw.com/quanta/english/about/organize25en.pdf'),
    ('qct-wssd.pdf', 'https://go.qct.io/wp-content/uploads/2020/11/QCT-Window-Server-2019-Datasheet.pdf'),
    ('qct-qxvdi-2020.pdf', 'https://go.qct.io/wp-content/uploads/2020/05/QxStack-QxVDI-OAHC-High-Density-Optimized-SKU-Reference-Architecture-EN.pdf'),
):
    if url in seen:
        continue
    path = DEST / name
    assert not path.exists()
    result = subprocess.run(['/usr/bin/curl', '--silent', '--show-error', '--fail', '--location',
        '--max-time', '30', '--max-filesize', '25000000', '--output', str(path),
        '--write-out', '%{http_code} %{url_effective}', url], capture_output=True, text=True)
    entry = dict(url=url, attempted_filename=name,
        method='system curl with default TLS validation; no retries',
        checked_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),
        curl_exit=result.returncode, result=result.stdout, error=result.stderr)
    if result.returncode == 0:
        body = path.read_bytes()
        entry.update(path=name, sha256=hashlib.sha256(body).hexdigest(), bytes=len(body))
        if name.endswith('.pdf'):
            entry['pdf_header_valid'] = body.startswith(b'%PDF-')
    entries.append(entry)
    report.write_text(json.dumps(entries, indent=2) + '\n')
    print(json.dumps(entry), flush=True)
