"""One-time bounded original-source captures; checkpoint outcomes, never retry."""
import datetime
import hashlib
import json
from pathlib import Path
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / 'data/raw/odm-models-2026-10-04'
OUT.mkdir(exist_ok=True)
ITEMS = {
    'wiwynn-sv7100g4.html': 'https://www.wiwynn.com/products/21-inch/sv7100g4',
    'wiwynn-sv7100g4.pdf': 'https://www.wiwynn.com/hubfs/Datasheet%20-%20SV7100G4_210917_internal.pdf',
    'hyve-orion-2024.pdf': 'https://hyvesolutions.com/wp-content/uploads/2024/10/HyveOrion_WhitePaper_1024_02.pdf',
    'hyve-datasheets.html': 'https://www.hyvesolutions.com/datasheets',
    'ingrasys-docs.html': 'https://www.ingrasys.com/docs/',
}
report = OUT / 'fetch-report.json'
entries = json.loads(report.read_text()) if report.exists() else []
seen = {entry['url'] for entry in entries}
for name, url in ITEMS.items():
    if url in seen:
        continue
    entry = {'url': url, 'attempted_filename': name,
             'checked_at': datetime.datetime.now(datetime.timezone.utc).isoformat()}
    try:
        request = urllib.request.Request(url, headers={'User-Agent': 'DeviceHunt/0.1 public hardware research'})
        with urllib.request.urlopen(request, timeout=30) as response:
            body = response.read(20_000_001)
            if len(body) > 20_000_000:
                raise ValueError('size exceeds limit')
            content_type = response.headers.get('Content-Type', '')
            if name.endswith('.pdf') and not body.startswith(b'%PDF-'):
                raise ValueError('not PDF bytes: ' + content_type)
            if name.endswith('.html') and 'html' not in content_type:
                raise ValueError('not HTML: ' + content_type)
            with (OUT / name).open('xb') as stream:
                stream.write(body)
            entry.update(status=response.status, path=name, bytes=len(body),
                         sha256=hashlib.sha256(body).hexdigest(), final_url=response.url)
    except (urllib.error.URLError, TimeoutError, ValueError) as error:
        entry.update(error=str(error), status=getattr(error, 'code', None))
    entries.append(entry)
    report.write_text(json.dumps(entries, indent=2) + '\n')
    print(json.dumps(entry), flush=True)
