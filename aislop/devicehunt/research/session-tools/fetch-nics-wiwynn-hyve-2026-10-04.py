"""Single-attempt capture of selected public product sources; preserve outcomes."""
import datetime
import hashlib
import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DEST = ROOT / 'data/raw/nics-wiwynn-hyve-2026-10-04'
DEST.mkdir(exist_ok=True)
report = DEST / 'fetch-report.json'
entries = json.loads(report.read_text()) if report.exists() else []
seen = {entry['url'] for entry in entries}
for name, url in (
    ('wiwynn-sv7100g2.html','https://www.wiwynn.com/products/21-inch/sv7100g2'),
    ('wiwynn-sv7220g3.html','https://www.wiwynn.com/products/21-inch/sv7220g3'),
    ('wiwynn-st7000g2.html','https://www.wiwynn.com/products/21-inch/st7000g2'),
    ('wiwynn-sv7000g2.html','https://www.wiwynn.com/products/21-inch/sv7000g2'),
    ('wiwynn-sitemap.html','https://www.wiwynn.com/sitemap'),
    ('hyve-ambient-series-e.pdf','https://www.opencompute.org/documents/hyve-solutions-ambient-series-e'),
    ('wiwynn-sv7100g2.pdf','https://www.wiwynn.com/hubfs/Datasheet-SV7100G2_181119.pdf'),
    ('wiwynn-sv7220g3.pdf','https://www.wiwynn.com/hubfs/Datasheet%20-%20SV7220G3_200430_internal-1.pdf'),
    ('wiwynn-st7000g2.pdf','https://www.wiwynn.com/hubfs/Datasheet-ST7000G2_171222-1.pdf'),
    ('hyve-hercules-2026.pdf','https://cdn.prod.website-files.com/697d7e6ffbb0348d6224a85d/69f8ec07a827785533ca84e5_Hercules_RGB_Rev1_050126.pdf'),
    ('hyve-jbof-2026.pdf','https://cdn.prod.website-files.com/697d7e6ffbb0348d6224a85d/69f8ecbeb6ef0bc0d959c6ca_JBOF_RGB_Rev1_050126.pdf'),
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
