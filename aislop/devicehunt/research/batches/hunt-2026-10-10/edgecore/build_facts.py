"""Build edgecore-ordering-facts-2026-10-10.json from captured PDFs (run: python3 -I build_facts.py)."""
import json, re, subprocess, hashlib
RAW = '/home/erin/nepeat/aislop/devicehunt/data/raw/hunt-2026-10-10-edgecore/'
PT = '/nix/store/wb8486c5l68f1shsbc217b7m4hbgrw3s-poppler-utils-26.06.0/bin/pdftotext'
DOCS = [
 # id, file, base model(s) heading handled by parse, columns, footer id, 1st-page title
 ('edgecore-ordering-as7712-32x-ds-r03-2016', 'as7712-as7716-32x-onie-ds-r03-20160707.pdf', ['Model Number','CPU Module','PSU','Airflow','Region (power cord)'], r'^\d{4}-32X-O-'),
 ('edgecore-ordering-dcs208-as5812-54x-ds-r15', 'dcs208-as5812-54x-ds-r15.pdf', ['Model Number','Part Number','PSU','Airflow','Region (Power Cord)'], r'^5812-54X-O-'),
 ('edgecore-ordering-dcs203-as7326-56x-ds-r10', 'dcs203-as7326-56x-ds-r10.pdf', ['Model Number','Part Number','PSU','Airflow','Region (Power Cord)'], r'^7326-56X-O-'),
 ('edgecore-ordering-dcs204-as7726-32x-ds-r09', 'dcs204-as7726-32x-ds-r09.pdf', ['Model Number','Part Number','PSU','Airflow','Power Cord'], r'^7726-32X-O-'),
 ('edgecore-ordering-dcs500-as7816-64x-ds-r15', 'dcs500-as7816-64x-ds-r15.pdf', ['Model Number','Part Number','PSU','Airflow','Region (Power Cord)'], r'^7816-64X-O'),
 ('edgecore-ordering-eps202-as4630-54pe-ds-r10', 'eps202-as4630-54pe-ds.pdf', ['Model Number','Part Number','PSU','Airflow','Region (Power Cord)'], r'^AS4630-54PE-O-'),
]
ledger = {e['file']: e for e in json.load(open(RAW + 'capture-ledger.json')) if e.get('file')}
sources, devices = [], []
for sid, fn, cols, pat in DOCS:
    txt = subprocess.run([PT, '-layout', RAW + fn, '-'], capture_output=True, text=True, check=True).stdout
    pages = txt.split('\f')
    pno = next(i for i, p in enumerate(pages, 1) if 'Ordering Information' in p)
    page = pages[pno - 1]
    footer = re.search(r'(EC-DS-\d{4}-\w+)', page).group(1)
    bases = [re.sub(r'\s+', ' ', m.strip()) for m in re.findall(r'Base Model:\s*(.*?)(?=\n\s*Model Number|\n\s*\n)', page, re.S)]
    # per-row base model: nearest preceding Base Model heading
    cur, rows = None, []
    for line in page.split('\n'):
        m = re.match(r'\s*Base Model:\s*(.*)', line)
        if m:
            cur = m.group(1).strip(); continue
        if cur and re.match(r'^\s*Model Number', line): continue
        if re.match(pat, line.strip()):
            cells = [c for c in re.split(r'\s{2,}', line.strip()) if c]
            rows.append((cur, cells))
    # base model heading continues on next line in 2 docs; rebuild from bases list
    order = [b for b in bases]
    assert rows, sid
    url = ledger[fn]['final_url'] if ledger[fn]['final_url'].endswith('.pdf') else ledger[fn]['url']
    sources.append({
      'id': sid, 'url': url, 'tier': 'vendor',
      'license': 'Original Edgecore Networks document rights retained; local factual extraction',
      'snapshot': fn,
      'coverage': ('Ordering Information table, PDF page %d, document id %s. Switch order-code rows transcribed literally by pdftotext -layout (poppler 26.06.0), column headers as printed. '
                   'PSU/fan-tray/accessory/service rows on the same page were hand-keyed from the printed tables (PDF page %d), machine-checked against the extracted text, and verified by independent review; '
                   'they carry brand "Unattributed accessory" with an Edge-Core documentation_vendor affiliation because the datasheet does not make them Edge-Core-branded identities. '
                   'Typographic ligatures from text extraction (fi, fl, ff, ffi, ffl, ft) were normalized to plain letters: the glyph is a rendering, the printed word is e.g. "Airflow". '
                   'Suffix meanings are not defined by this table beyond the literal column text printed for each order code; no cross-model generalisation, region/airflow decoding, or equivalence to catalog AS-prefixed names (printed codes lack the AS prefix where shown without it). '
                   'Document brand printed as Edgecore Networks; no availability, condition, EOL status, or physical-manufacture claim. PDF SHA-256 %s.') % (pno, footer, pno, ledger[fn]['sha256'])})
    bi = 0
    for cur, cells in rows:
        code = cells[0]
        # map base heading: re-find heading text by scan order
        desc = '; '.join('%s: %s' % (h, v) for h, v in zip(cols, cells))
        base_line = None
        devices.append({
          'source': sid, 'key': code, 'brand': 'Edge-Core', 'model': code, 'title': code, 'kind': 'switch',
          'affiliations': [['Edge-Core', 'brand']],
          'facts': [
            ['ordering_description_as_printed', desc],
            ['page_ref', 'PDF page %d (Ordering Information); document id %s' % (pno, footer)],
            ['_base', cur],
          ] + ([['part_number_as_printed', cells[1]]] if cols[1] == 'Part Number' else [])})
# resolve base model strings (headings can wrap across lines; take whitespace-joined heading)
for d in devices:
    sid = d['source']; fn = next(x[1] for x in DOCS if x[0] == sid)
    txt = subprocess.run([PT, '-layout', RAW + fn, '-'], capture_output=True, text=True, check=True).stdout
    page = next(p for p in txt.split('\f') if 'Ordering Information' in p)
    heads = [re.sub(r'\s+', ' ', h.strip()) for h in re.findall(r'Base Model:\s*(.*?)(?=\n\s*Model Number)', page, re.S)]
    fl = d['facts']; i = next(k for k, f in enumerate(fl) if f[0] == '_base'); raw = fl.pop(i)[1]
    h = next(h for h in heads if h.startswith(raw[:25]))
    fl.insert(1, ['base_model_as_printed', h.split(';')[0].strip()])
    fl.insert(2, ['base_model_line_as_printed', h])
exec(open(__file__.replace('build_facts.py','accessories.py')).read())
LIG = {'\ufb00':'ff','\ufb01':'fi','\ufb02':'fl','\ufb03':'ffi','\ufb04':'ffl','\ufb05':'ft','\ufb06':'st'}
def _fix(o):
    if isinstance(o, str):
        for k, v in LIG.items(): o = o.replace(k, v)
        return o
    if isinstance(o, list): return [_fix(x) for x in o]
    if isinstance(o, dict): return {k: _fix(v) for k, v in o.items()}
    return o
sources, devices = _fix(sources), _fix(devices)
json.dump({'sources': sources, 'devices': devices}, open(RAW + 'edgecore-ordering-facts-2026-10-10.json', 'w'), indent=1, ensure_ascii=False)
print(len(sources), len(devices))
