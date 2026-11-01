"""Capture and curate the explicit five-product 2020 Inventec EBG claim once."""
import datetime
import hashlib
import json
import subprocess
from pathlib import Path
from html.parser import HTMLParser
ROOT = Path(__file__).resolve().parents[2]
DATA = ROOT / 'data'
RAW = DATA / 'raw'
url = 'https://ebg.inventec.com/cn/news/%E6%96%B0%E9%97%BB%E4%B8%8E%E5%85%AC%E5%91%8A/2020/37'
path = RAW / 'inventec-dongguan-2020-2026-10-04.html'
fetch = RAW / 'inventec-dongguan-2020-2026-10-04.fetch.json'
assert not path.exists() and not fetch.exists()
r = subprocess.run(['/usr/bin/curl', '--silent', '--show-error', '--fail', '--location',
    '--max-time', '30', '--max-filesize', '3000000', '--output', str(path),
    '--write-out', '%{http_code} %{url_effective}', url], capture_output=True, text=True)
result = dict(url=url, method='system curl default TLS; no retry',
    checked_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),
    curl_exit=r.returncode, result=r.stdout, error=r.stderr)
if r.returncode == 0:
    result.update(path=path.name, sha256=hashlib.sha256(path.read_bytes()).hexdigest())
fetch.write_text(json.dumps(result, indent=2) + '\n')
assert r.returncode == 0, result
class Text(HTMLParser):
    def __init__(self, s):
        super().__init__(); self.parts=[]; self.feed(s)
    def handle_data(self, s): self.parts.append(s)
text = ' '.join(''.join(Text(path.read_text()).parts).split())
assert '这些产品均由英业达企业电脑事业群(EBG)负责研发制造' in text
assert '英华达(IAC)研发制造' in text
assert '04 November. 2020' in text
models = [('Entei', 'storage server'), ('Horsea', 'server platform'), ('Golduck', 'server platform'),
          ('E850G4', 'edge server'), ('FPGA SmartNIC C5020X', 'SmartNIC')]
for model, _ in models:
    assert 'Inventec ' + model in text, model
sid = 'inventec-dongguan-manufacturer-2020'
data = dict(sources=[dict(id=sid, url=url, tier='vendor',
    license='Inventec factual product/manufacturer statement; original rights retained',
    snapshot=str(path.relative_to(DATA)),
    coverage='Original Inventec article dated 2020-11-04, captured 2026-10-04. In the datacenter '
        'hardware paragraph, five exact named products are followed by an explicit statement '
        'that Inventec EBG developed and manufactured them. This is a dated vendor manufacturer '
        'claim; no factory, production duration, customer/operator or unit provenance asserted. '
        'The unnamed custom servers, software offerings, Intel C5000X base platform and separately '
        'IAC-manufactured 5G V2X OBU Box are excluded; no global subsidiary alias added.')], devices=[])
for model, kind in models:
    data['devices'].append(dict(source=sid, key=model, brand='Inventec', model=model,
        title='Inventec ' + model, kind=kind,
        affiliations=[['Inventec', 'brand'], ['Inventec Corporation', 'manufacturer']], facts=[
            ['statement_date', '2020-11-04'], ['manufacturer_unit_as_printed', '英业达企业电脑事业群(EBG)'],
            ['manufacturer_statement_as_printed', '这些产品均由英业达企业电脑事业群(EBG)负责研发制造'],
            ['manufacturer_statement_translation', 'The preceding named products were developed and manufactured by Inventec Enterprise Business Group (EBG).'],
            ['manufacturer_scope', 'Dated vendor claim for this named platform. Article distinguishes EBG from IAC and the Tianjin software subsidiary. No exact physical factory or operator assigned.'],
            ['identity_limit', 'Printed product name retained; no inferred commercial-order-code alias.']]))
with (DATA / 'inventec-manufacturer-facts-2026-10-04.json').open('x') as stream:
    stream.write(json.dumps(data, indent=2, ensure_ascii=False) + '\n')
print(json.dumps(result, indent=2)); print('Prepared five dated manufacturer claims; excluded IAC device and unnamed models.')
