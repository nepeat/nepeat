"""Capture a bounded set of observed original Ingrasys links, once per URL."""
import datetime
import hashlib
import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DEST = ROOT / 'data/raw/ingrasys-servers-2026-10-04'
DEST.mkdir(exist_ok=True)
report = DEST / 'fetch-report.json'
entries = json.loads(report.read_text()) if report.exists() else []
seen = {entry['url'] for entry in entries}
for name, url in (
    ('about.html', 'https://www.ingrasys.com/company/about/'),
    ('sv1120-sv1121-sv2121-sv2221.html', 'https://www.ingrasys.com/docs-detail/sv1120-sv1121-sv2121-sv2221/'),
    ('es2000-es2100.html', 'https://www.ingrasys.com/docs-detail/es2000-es2100/'),
    ('amd-epyc.html', 'https://www.ingrasys.com/solutions/amd/amd-epyc-processor-solution/'),
    ('intel-xeon.html', 'https://www.ingrasys.com/solutions/intel/intel-xeon-processor-solution/'),
    ('nvme-of-ebof.html', 'https://www.ingrasys.com/solutions/15/nvme-of_ebof_solution/'),
):
    if url in seen:
        continue
    path = DEST / name
    assert not path.exists(), path
    result = subprocess.run(['/usr/bin/curl', '--silent', '--show-error', '--fail',
        '--location', '--max-time', '30', '--max-filesize', '20000000',
        '--output', str(path), '--write-out', '%{http_code} %{url_effective}', url],
        capture_output=True, text=True)
    entry = dict(url=url, attempted_filename=name,
        method='system curl with default TLS validation; no retries',
        checked_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),
        curl_exit=result.returncode, result=result.stdout, error=result.stderr)
    if result.returncode == 0:
        body = path.read_bytes()
        entry.update(path=name, sha256=hashlib.sha256(body).hexdigest(), bytes=len(body))
    entries.append(entry)
    report.write_text(json.dumps(entries, indent=2) + '\n')
    print(json.dumps(entry), flush=True)
