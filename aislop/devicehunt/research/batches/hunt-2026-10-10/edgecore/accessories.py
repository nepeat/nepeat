# executed inside build_facts.py: appends PSU/fan/accessory rows (hand-keyed from the Ordering Information page, verified against extracted text)
def _P(fn):
    t = subprocess.run([PT, '-layout', RAW + fn, '-'], capture_output=True, text=True, check=True).stdout
    return next(p for p in t.split('\f') if 'Ordering Information' in p)
def _n(x): return re.sub(r'\s+', ' ', x)
SID = {x[1]: x[0] for x in DOCS}
# (file, kind, heading, [(code, [(label,value),...])]) ; label None => unlabeled (table headers misaligned/absent)
ACC = [
 ('as7712-as7716-32x-onie-ds-r03-20160707.pdf', 'Power Supply FRU', 'Model Number / CPU Module / PSU / Airflow / Region (power cord)', [
   ('PSU-AC-650A-F', [('PSU','650W AC Power Supply FRU'),('Airflow','port-to-power airflow'),('Region (power cord)','no power cord')]),
   ('PSU-AC-650A-B', [('PSU','650W AC Power Supply FRU'),('Airflow','power-to-port airflow'),('Region (power cord)','no power cord')])]),
 ('as7712-as7716-32x-onie-ds-r03-20160707.pdf', 'Fan Module FRU', 'Model Number / CPU Module / PSU / Airflow / Region (power cord)', [
   ('FAN-1U-1x1C-F', [('PSU','Fan Module FRU'),('Airflow','port-to-power airflow')]),
   ('FAN-1U-1x1C-B', [('PSU','Fan Module FRU'),('Airflow','power-to-port airflow')])]),
 ('as7712-as7716-32x-onie-ds-r03-20160707.pdf', 'ordering item (type not printed)', 'Model Number / Description', [
   ('77xx-32X-SVC-RTF', [('Description','AS77xx-32X series, Annual Hardware Service, Return-to-Factory, after warranty. 3 year warranty included already with the product, maximum 2 additional years per product.')]),
   ('CBL-PWR-US', [('Description','AC Power Cable - US (125V/13A, 1830mm) – only required with spare power supplies')]),
   ('CBL-PWR-EU', [('Description','AC Power Cable - Europe (250V/10A, 1830mm) – only required with spare power supplies')]),
   ('CBL-PWR-UK', [('Description','AC Power Cable - UK (250V/10A, 1830mm) – only required with spare power supplies')]),
   ('CBL-PWR-JP', [('Description','AC Power Cable - Japan – only required with spare power supplies')])]),
 ('dcs208-as5812-54x-ds-r15.pdf', 'Power Supply FRU', 'Model Number / Part Number / PSU / Airflow / Region (Power Cord)', [
   ('CPR-4011-4M11', [('Part Number','F0TZZ5652003A'),('PSU','400 W AC Power Supply FRU (PSU-AC-400-F)'),('Airflow','Front-to-Back Airflow'),('Region (Power Cord)','no power cord')]),
   ('CPR-4011-4M21', [('Part Number','F0TZZ5652002A'),('PSU','400 W AC Power Supply FRU (PSU-AC-400-B)'),('Airflow','Back-to-Front Airflow'),('Region (Power Cord)','no power cord')]),
   ('UM400D01-01', [('Part Number','F0TZZ5654004A'),('PSU','400 W -48 VDC Power Supply FRU (PSU-48V-400-F)'),('Airflow','Front-to-Back Airflow'),('Region (Power Cord)','no power cord')]),
   ('UM400D01', [('Part Number','F0TZZ5654003A'),('PSU','400 W -48 VDC Power Supply FRU (PSU-48V-400-B)'),('Airflow','Back-to-Front Airflow'),('Region (Power Cord)','no power cord')])]),
 ('dcs208-as5812-54x-ds-r15.pdf', 'Fan Tray FRU', 'Model Number / Part Number / PSU / Airflow / Region (Power Cord)', [
   ('FAN-1U-1x1D-F', [('Part Number','F0TZZ5654000A'),('PSU','Fan Tray FRU'),('Airflow','Front-to-Back Airflow')]),
   ('FAN-1U-1x1D-B', [('Part Number','F0TZZ5654001A'),('PSU','Fan Tray FRU'),('Airflow','Back-to-Front Airflow')])]),
 ('dcs208-as5812-54x-ds-r15.pdf', 'ordering item (type not printed)', 'Model Number / Part Number / Description', [
   ('58XX-54X-SVC-RTF', [('Part Number','REWECECWA008Z'),('Description','AS5812-54X Annual Hardware Service, post warranty, Return-to-Factory')]),
   ('RKIT-L-6712', [('Part Number','F0TZZ6632001A'),('Description','Rack mount kit')]),
   ('ORSA-1U', [('Part Number','F0TZZ5654002A'),('Description','Open Rack Switch Adapter, for mounting standard 19 inch form factor 1U switches into 21 inch Open Rack')])]),
 ('dcs203-as7326-56x-ds-r10.pdf', 'Power Supply FRU', 'Model Number / Part Number / PSU / Airflow / Region (Power Cord)', [
   ('YM-2651Y, B01R', [('Part Number','F0TZZ5654019A'),('PSU','650 W AC Power Supply FRU'),('Airflow','Front-to-Back Airflow')]),
   ('YM-2651Y, C01R', [('Part Number','F0TZZ5654020A'),('PSU','650 W AC Power Supply FRU'),('Airflow','Back-to-Front Airflow')])]),
 ('dcs203-as7326-56x-ds-r10.pdf', 'Fan Module FRU', 'Model Number / Part Number / PSU / Airflow / Region (Power Cord)', [
   ('FAN-1U-1x1I-F', [('Part Number','F0TZZ7656000A'),('PSU','Fan Module FRU'),('Airflow','Front-to-Back Airflow')]),
   ('FAN-1U-1x1I-B', [('Part Number','F0TZZ7656001A'),('PSU','Fan Module FRU'),('Airflow','Back-to-Front Airflow')])]),
 ('dcs204-as7726-32x-ds-r09.pdf', 'PSU FRUs', 'PSU FRUs (Power Cord not included): Model Number / Part Number / PSU / Airflow', [
   ('PSU-AC-650B-F', [('Part Number','F0TZZ5654019A'),('PSU','AC'),('Airflow','Front-to-Back')]),
   ('PSU-AC-650B-B', [('Part Number','F0TZZ5654020A'),('PSU','AC'),('Airflow','Back-to-Front')]),
   ('PSU-AC-650C-F', [('Part Number','F0TZZ7656002A'),('PSU','AC'),('Airflow','Front-to-Back')]),
   ('PSU-AC-650C-B', [('Part Number','F0TZZ7656003A'),('PSU','AC'),('Airflow','Back-to-Front')]),
   ('PSU-48V-650-F', [('Part Number','F0TZZ7632004A'),('PSU','48VDC'),('Airflow','Front-to-Back')]),
   ('PSU-48V-650-B', [('Part Number','F0TZZ7632003A'),('PSU','48VDC'),('Airflow','Back-to-Front')])]),
 ('dcs204-as7726-32x-ds-r09.pdf', 'FAN FRUs', 'FAN FRUs: Model Number / Part Number / Airflow', [
   ('FAN-1U-1x1C-F', [('Part Number','F0TAO6632000A'),('Airflow','Front-to-Back')]),
   ('FAN-1U-1x1C-B2', [('Part Number','F0TZZ7632000A'),('Airflow','Back-to-Front')])]),
 ('dcs204-as7726-32x-ds-r09.pdf', 'Accessories', 'Accessories: Model Number / Part Number / Description', [
   ('RKIT-100G-SLIDE', [('Part Number','F0OZZ7632001A'),('Description','Tool-less Rack Mounting Rail Slide Kit (674 mm - 919 mm ±3 mm)')])]),
 # 7816 FRU/FAN tables: printed headers "FRU Model Number | Type | Airflow" but the middle cell is a part-number-shaped string; cells kept unlabeled
 ('dcs500-as7816-64x-ds-r15.pdf', 'FRU', 'FRU Model Number / Type / Airflow (header as printed; middle cell holds a part-number-like string)', [
   ('YM-2851F, C01R', [(None,'F0TAT7664003A'),(None,'AC Front-to-Back')]),
   ('YM-2851F, D01R', [(None,'F0TAT7664001A'),(None,'AC Back-to-Front')]),
   ('DPS-850AB-5 A', [(None,'F0TZZ7664004A'),(None,'AC Front-to-Back')]),
   ('DPS-850AB-6 A', [(None,'F0TZZ7664005A'),(None,'AC Back-to-Front')]),
   ('YM-2851J ER', [(None,'F0TAT7664000A'),(None,'DC Front-to-Back')]),
   ('YM-2851J FR', [(None,'F0TAT7664002A'),(None,'DC Back-to-Front')])]),
 ('dcs500-as7816-64x-ds-r15.pdf', 'FAN', 'FAN Model Number / Type / Airflow (header as printed; middle cell holds a part-number-like string)', [
   ('FAN-1U-1x1G-F', [(None,'F0TZZ7664002A'),(None,'Front-to-Back Airflow')]),
   ('FAN-1U-1x1G-B', [(None,'F0TZZ7664003A'),(None,'Back-to-Front Airflow')])]),
 ('eps202-as4630-54pe-ds.pdf', 'PSU', 'PSU Model Number / Part Number / Product Description', [
   ('YPEB1200AM-1A03P10', [('Part Number','F0TZZ4654011A'),('Product Description','1200 W AC Power Supply (3Y), front-to-rear airflow, 1 Year Warranty (for EPS202/AS4630-54PE, EPS203/AS4630-54NPE). Inlet Connector: IEC 60320 C14.')])]),
 ('eps202-as4630-54pe-ds.pdf', 'FAN', 'FAN Model Number / Part Number / Product Description', [
   ('FAN-1U-1x1M-F', [('Part Number','F0TZZ4654009A'),('Product Description','Fan Module, front-to-rear airflow, 1 Year Warranty (for EPS201/AS4630-54TE, EPS202/AS4630-54PE, EPS203/AS4630-54NPE).')])]),
 ('eps202-as4630-54pe-ds.pdf', 'Accessories', 'Accessories Model Number / Part Number / Product Description', [
   ('RKIT-100G-SLIDE', [('Part Number','F0OZZ7632001A'),('Product Description','Tool-less mounting rail slide kit (For EPS200 series/AS4630-54TE/54PE/54NPE, DCS203/AS7326-56X, DCS204/AS7726-32X, DCS500/AS7816-64X, DCS501/AS7712-32X, DCS510/AS9716-32D, AS7716-32X).')])]),
]
_acc_n = 0
for fn, kind, heading, rows_ in ACC:
    sid = SID[fn]; page = _P(fn); words = set(_n(page).replace('ﬂ','fl').replace('ﬅ','ft').replace('/',' ').replace('-',' ').split(' '))
    pno = next(i for i, p in enumerate(subprocess.run([PT,'-layout',RAW+fn,'-'],capture_output=True,text=True).stdout.split('\f'),1) if 'Ordering Information' in p)
    ref = next(d for d in devices if d['source']==sid)
    footer = [f for f in ref['facts'] if f[0]=='page_ref'][0][1].split('document id ')[1]
    bases = [d for d in devices if d['source']==sid and d['kind']=='switch']
    base = bases[-1]  # heading immediately preceding the FRU/accessory tables
    bm = [f[1] for f in base['facts'] if f[0]=='base_model_as_printed'][0]
    bl = [f[1] for f in base['facts'] if f[0]=='base_model_line_as_printed'][0]
    norm = _n(page).replace('ﬂ','fl').replace('ﬅ','ft')
    for code, cells in rows_:
        assert _n(code) in norm, code
        for _, v in cells:
            assert all(w in words for w in _n(v).replace('/',' ').replace('-',' ').split(' ')), (code, v)
        desc = '; '.join(('%s: %s' % (l, v)) if l else v for l, v in cells)
        desc = 'Model Number: %s; %s' % (code, desc) if cells[0][0] else code + '; ' + desc
        pn = [v for l, v in cells if l == 'Part Number' or (l is None and v.startswith('F0'))]
        facts = [['ordering_description_as_printed', desc],
                 ['listed_with_base_model_as_printed', bm],
                 ['listed_with_base_model_line_as_printed', bl],
                 ['table_heading_as_printed', heading],
                 ['page_ref', 'PDF page %d (Ordering Information, FRU/accessory section); document id %s' % (pno, footer)]]
        if pn: facts.append(['part_number_as_printed', pn[0]])
        if sid.endswith('r15') and 'as7816' in sid:
            facts.append(['grouping_note', 'FRU/FAN tables follow the AS7816-64X-O series table; page also prints an AS7816-64X-OZ series Base Model line. No compatibility with either series is stated beyond this page grouping.'])
        devices.append({'source': sid, 'key': 'ACC:' + code, 'brand': 'Unattributed accessory', 'model': code, 'title': code, 'kind': kind,
                        'affiliations': [['Edge-Core', 'documentation_vendor']], 'facts': facts})
        _acc_n += 1
print('accessory rows', _acc_n)
