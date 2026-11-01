"""Finite source-scoped observations and raw tables from immutable wave 5 captures."""
import datetime as dt
import hashlib
from html.parser import HTMLParser
import json
from pathlib import Path
import re

HERE=Path(__file__).resolve().parent
ROOT=HERE.parents[3]


def sha(path): return hashlib.sha256(path.read_bytes()).hexdigest()


class Article(HTMLParser):
    def __init__(self,text):
        super().__init__(convert_charrefs=True)
        self.active=False
        self.stack=[]
        self.blocks=[]
        self.all=[]
        self.feed(text)

    def handle_starttag(self,tag,attrs):
        attrs=dict(attrs)
        if tag=='article':self.active=True
        if self.active and (tag in ('h1','h2','h3','p','li','tr') or tag=='div' and attrs.get('role')=='note'):
            self.stack.append([tag,[],attrs])

    def handle_data(self,text):
        if self.active:self.all.append(text)
        for block in self.stack:block[1].append(text)

    def handle_endtag(self,tag):
        if self.stack and self.stack[-1][0]==tag:
            block=self.stack.pop()
            text=' '.join(''.join(block[1]).split())
            if text:self.blocks.append(dict(tag=tag,text=text,attrs=block[2]))
        if tag=='article':self.active=False


def main():
    requests=[json.loads(line) for line in (HERE/'requests.jsonl').read_text().splitlines()]
    final={r['body_path']:r for r in requests if r['status']==200}
    files=('oracle7420-specifications.html','as6700-quick-start.pdf','as5710-as5712-ds-r09.pdf')
    sources={name:dict(url=final[name]['url'],captured_at_utc=final[name]['finished_at_utc'],sha256=sha(HERE/name)) for name in files}
    for name in files:assert sources[name]['sha256']==final[name]['body_sha256']
    pdfs={'as6700':json.loads((HERE/'as6700-pages.json').read_text()),'as5712':json.loads((HERE/'as5712-pages.json').read_text())}
    observations=[]

    def pdf(label,page,anchor,end,description,scope,category,occurrence=None):
        lines=pdfs[label]['pages'][page-1]['text'].splitlines()
        selected=[i for i,line in enumerate(lines) if anchor in line]
        assert selected and (occurrence is not None or len(selected)==1),(label,page,anchor,selected)
        start=selected[occurrence or 0]
        finish=start if end is None else next(i for i in range(start,len(lines)) if end in lines[i])
        name='as6700-quick-start.pdf' if label=='as6700' else 'as5710-as5712-ds-r09.pdf'
        observations.append(dict(id=label+'-'+str(len(observations)+1),source=name,pdf_page=page,
            lines_1based=list(range(start+1,finish+2)),raw_lines=lines[start:finish+1],summary=description,
            scope=scope,category=category,visually_reviewed=True,visual_page=label+'-'+str(page)+'.png'))

    joint6700='Printed AS6700-32X / AS6701-32X quick-start document scope; no equivalence between models, variants or auction units.'
    joint5712='Printed AS5710-54X / AS5712-54X family scope; preserve separately named CPU/software and ordering rows.'
    pdf('as6700',1,'AS6700-32X / AS6701-32X',None,'Joint model heading retained.',joint6700,'identity_literal',occurrence=0)
    pdf('as6700',1,'Rack Mounting Kit','screws for attaching','Kit contains two brackets and eight screws.',joint6700,'physical')
    pdf('as6700',1,'Switches with part numbers 6700/6701-32X-D*','user documentation can be found','Literal 6700/6701-32X-D*-******** pattern has switch software pre-loaded; wildcard pattern is not expanded into SKUs.',joint6700+' Historical loaded-software statement, no current image/access/entitlement claim.','software')
    pdf('as6700',1,'Switches with part numbers 6700/6701-32X-O-','software can be found','Literal 6700/6701-32X-O-******** pattern has ONIE installer pre-loaded but no switch software image.',joint6700+' Pattern meaning is source-local, not a general suffix decoder.','software')
    pdf('as6700',1,'All','modules is indicated by labels','All installed PSU/fan tray modules must match F2B or B2F airflow; labels indicate direction.',joint6700,'cooling')
    pdf('as6700',1,'Install one or two universal AC power modules','same matching airflow','Install one or two universal AC modules; up to two PSUs matching installed fan-tray airflow.',joint6700+' No DC configuration facts from the AC installation passage.','power')
    pdf('as6700',1,'Installing the switch in a rack requires two','while the other secures','Two-person rack installation requirement.',joint6700,'physical')
    pdf('as6700',1,'The earth connection must not be removed','unless all supply','Earth connection removal restriction retains all-supply disconnection condition.',joint6700,'service_constraint')
    pdf('as6700',1,'E012014-CS-R01','150200000827A','Literal footer E012014-CS-R01 / 150200000827A, no upload-date inference.',joint6700,'revision_literal')
    pdf('as6700',2,'438.4 x 473 x 43.4 mm',None,'W x D x H: 438.4 x 473 x 43.4 mm (17.26 x 18.62 x 1.71 in.).',joint6700,'physical')
    pdf('as6700',2,'8.382 kg','five fan trays, and two 40G modules','8.382 kg (18.52 lb), with two installed PSUs, five fan trays and two 40G modules; not empty weight.',joint6700,'physical')
    pdf('as6700',2,'Operating: 0° C','Storage: -40° C','Operating 0–40°C; separate storage -40–70°C.',joint6700,'environment')
    pdf('as6700',2,'Operating: 5% to 95%',None,'Operating humidity 5–95%, non-condensing.',joint6700,'environment')
    pdf('as6700',2,'100-240 VAC, 50-60 Hz, 6.0-3.0 A',None,'PSU AC Input Rating: 100-240 VAC, 50-60 Hz, 6.0-3.0 A.',joint6700,'power')
    pdf('as6700',2,'375 Watts maximum, 267 Watts typical',None,'Switch Power Consumption: 375 Watts maximum, 267 Watts typical; not PSU output.',joint6700,'power')
    pdf('as6700',2,'Configure the PC’s serial port','parity, one stop bit','DCSS serial setup: 115200 bps, 8 characters, no parity, one stop bit, 8 data bits, no flow control; preserve the printed repeated 8-character/8-bit wording.',joint6700+' Historical documented configuration; no current software entitlement.','software')
    pdf('as6700',2,'If the network operating system (NOS) installer','located on attached storage','ONIE: network-server installer uses Mgmt RJ-45 with Cat5/5e or better; connection not required for installer on attached storage.',joint6700+' Installer-location condition remains explicit.','software')
    pdf('as6700',2,'Subsequent switch boots','the NOS software','Subsequent boots bypass ONIE and run loaded NOS, per historical guide.',joint6700+' Not a unit-specific boot/access promise.','software')
    pdf('as5712',1,'AS5710-54X, AS5712-54X',None,'Both full printed family names retained.',joint5712,'identity_literal')
    pdf('as5712',1,'Rack mountable in standard','Open Rack Switch Adapter','Standard 19-inch rack; 21-inch Open Rack with Open Rack Switch Adapter.',joint5712+' Source-listed mounting option, not inferred universal fitment.','physical')
    pdf('as5712',1,'Cumulus® Linux® r2.5.1','Pluribus Networks','AS5712 historical compatibility retains Cumulus r2.5.1+, PicOS r2.6+, SnapRoute FlexSwitch and Pluribus ONVL 2.5+ statements as printed.',joint5712+' No current release/license/image/support claim; AS5710 does not inherit AS5712 list.','software')
    pdf('as5712',2,'AS5710-54X: Freescale P2041','Reserve mSATA connector','AS5710 P2041/1.5GHz and memory/storage differ from AS5712 Atom C2538/2.4GHz and memory/storage; raw per-model rows retained.',joint5712,'model_configuration')
    pdf('as5712',2,'PSUs: 2 redundant','Max Power: 282 W','Power block: two redundant/load-sharing/hot-swappable AC or -48VDC; 90–264VAC 50–60Hz, -48 to -72VDC; 6A@100/120VAC, 3A@200/240VAC, 10A@-72VDC; up to 93% AC efficiency; separate 12VDC input option; max 282W without optics.',joint5712+' Polarity/conditions and AC efficiency scope retained; no summed output inference.','power')
    pdf('as5712',2,'Dimensions (WxDxH)','Operating Humidity','Dimensions 44.3 x 47.3 x 4.34 cm; 8.5kg with two PSU modules; 4+1 hot-swappable fans; operating 0–40°C, storage -40–70°C, humidity 5–95% non-condensing.',joint5712,'physical_cooling_environment')
    pdf('as5712',4,'subsidiary of Accton Technology Corporation','data center switches are developed','Generic about-company parent/development/manufacture prose retained as context only, no per-model role/physical factory assertion.',joint5712,'literal_company_context')
    pdf('as5712',4,'© Copyright 2019',None,'Literal copyright 2019; footer EC-DS-0619-09 verified visually; path 2024/01 is not publication date.',joint5712,'revision_literal')

    article=Article((HERE/'oracle7420-specifications.html').read_text())
    html={'method':'stdlib HTMLParser, selected article blocks; whitespace normalized only, raw HTML immutable',
          'blocks':[{**b,'ordinal_zero_based':i} for i,b in enumerate(article.blocks)],'article_text':' '.join(''.join(article.all).split())}
    (HERE/'oracle-html-observations.json').write_text(json.dumps(html,indent=2)+'\n')
    oracle_scope='Printed Sun ZFS Storage 7420 controller specifications only; no M2/registry-code transfer, PSU identity, manufacturer role or physical-unit configuration inference.'
    for token,description,category in [
        ('The 3U chassis','3U chassis form factor.','physical'),
        ('Height 13.3 cm/5.25 in','Table 12-2 height 13.3cm/5.25in and depth 70.6cm/27.8in.','physical'),
        ('Width 43.7 cm/17.19 in','Table 12-2 width 43.7cm/17.19in; weight literal 16.36kg/96lbs is inconsistent and must remain uncorrected.','physical'),
        ('Nominal frequencies: 50/60Hz','Nominal frequency 50/60Hz.','power'),
        ('AC operating range:','100–127VAC for 2 CPUs; 200–240VAC for 2 or 4 CPUs, exact CPU condition retained.','power'),
        ('Maximum current AC RMS:','Maximum AC RMS 12A@100VAC / 12A@200VAC.','power'),
        ('Max power consumption:','Power Dissipation list says Max power consumption: 1800W, qualified by note that listed dissipation numbers are maximum PSU rating, not actual appliance consumption.','power'),
        ('Max heat output:','Max heat output: 6143BTU/hr, same power-dissipation/PSU-rating note applies.','power'),
        ('Volt-Ampere rating:','1837VA@240VAC, 0.98P.F., retain source scope/units.','power')]:
        matches=[b for b in html['blocks'] if token in b['text']]
        selected=next((b for b in matches if b['tag']=='tr'),matches[0])
        observations.append(dict(id='oracle-'+str(len(observations)+1),source='oracle7420-specifications.html',
            locator={'article_block_ordinal_zero_based':selected['ordinal_zero_based'],'tag':selected['tag'],'text_phrase':token},
            raw_block=selected['text'],summary=description,scope=oracle_scope,category=category,visually_reviewed=False,method='original HTML paragraph/table markup review'))
    note='The power dissipation numbers listed are the maximum rated for the power supply and are not a rating of the actual power consumption of the appliance.'
    assert note in html['article_text']
    observations.append(dict(id='oracle-rating-note',source='oracle7420-specifications.html',locator={'article_text_exact_phrase':note},raw_text=note,
        summary='This note qualifies the whole Power Dissipation list; do not rename 1800W as actual consumption.',scope=oracle_scope,category='rating_scope_note',visually_reviewed=False))

    rows=[]
    for page in (3,4):
        lines=pdfs['as5712']['pages'][page-1]['text'].splitlines()
        base=None
        headers=None
        for n,line in enumerate(lines):
            if line.startswith('Base Model:'):base=line
            if line.strip().startswith('Model Number'):
                headers=re.split(r'\s{2,}',line.strip())
            if not re.match(r'^\s*(?:571[02]-54X-|PSU-|FAN-|57xx-|RKIT-|ORSA-|CBL-)',line):continue
            cells=re.split(r'\s{2,}',line.strip())
            kind='family_configuration_order_row' if page==3 else 'PSU_fan_FRU_order_row' if cells[0].startswith(('PSU-','FAN-')) else 'service_mounting_cable_order_row'
            record=dict(source='as5710-as5712-ds-r09.pdf',pdf_page=page,line_1based=n+1,raw_line=line,
                        raw_cells_from_embedded_text=cells,base_model_line_as_printed=base if page==3 else None,
                        table_headers_as_embedded_text=headers,
                        row_scope=kind,visually_reviewed=True,visual_page='as5712-'+str(page)+'.png',
                        scope='Literal source ordering row only; no aliases, SKU expansion, standalone hardware kind/fitment, role or current stock/entitlement decision.')
            if cells[0].startswith('FAN-'):
                record.update(model_text_render_conflict=True,model_as_embedded_text=cells[0],model_visible_in_render=cells[0][1:],
                              conflict_note='Initial F exists in embedded text but is not visible in both 150/240dpi original-page renders. Retain both observations unresolved; do not canonicalize either into a hardware identity.',additional_visual_page='as5712-reread4-4.png')
            rows.append(record)
    assert len(rows)==38 and len([r for r in rows if r['pdf_page']==3])==24
    conflicts=[dict(source='oracle7420-specifications.html',issue='Weight is printed 16.36 kg/96 lbs; units disagree. Preserve literal pair, no correction/conversion.'),
               dict(source='oracle7420-specifications.html',issue='1800W label says Max power consumption but note qualifies dissipation as maximum PSU rating, not actual appliance consumption.'),
               dict(source='data/raw/edgecore/product-as6700-32x.html',issue='Previously saved AS6700 product page vendor_description names AS6701. Joint new guide does not prove equivalence or repair old field.'),
               dict(source='as5710-as5712-ds-r09.pdf',issue='Saved pdfinfo Title metadata reads AS5710_AS5712-54X ONIE DS R06, whereas visible footer EC-DS-0619-09 and published landing call R09. Document designation follows visible footer; metadata remains unresolved historical context, no guessed alternate revision.'),
               dict(source='as5710-as5712-ds-r09.pdf',pdf_page=4,issue='Two fan model rows read FAN-1U-1x1D-F/B in embedded text but AN-1U-1x1D-F/B visually. Both unresolved; no row identity accepted.')]
    result=dict(status='finite discovery only; no curated candidate or import',observed_at_utc=dt.datetime.now(dt.timezone.utc).isoformat(),
                sources=sources,observations=observations,ordering_rows=rows,conflicts=conflicts,
                negative_findings=[
                    dict(source='oracle7420-specifications.html',scope='entire captured specifications article',absent='No M2-named specifications, environmental limits, PSU model/order code, PSU count/redundancy or service part table on this page.'),
                    dict(source='as6700-quick-start.pdf',scope='complete 2-page guide',absent='No full instantiated chassis/PSU/fan order-code table, exact PSU model/output rating, DC-input installation/rating, service replacement timing or current NOS entitlement.'),
                    dict(source='as5710-as5712-ds-r09.pdf',scope='complete 4-page datasheet',absent='No general cross-family suffix decoder, physical factory identity, current firmware/image entitlement or unit-specific configuration. PSU-12V-400-1 description does not state 400W output despite its code.')],
                limits='Historical/source-scoped only; complete sidecars, six distinct PDF pages visually reviewed with seven renders; HTML reviewed from raw markup. No model aliases, roles/manufacture, inferred fitment, current certification/support/stock or candidate decisions.')
    (HERE/'findings.json').write_text(json.dumps(result,indent=2,ensure_ascii=False)+'\n')
    print('observations',len(observations),'ordering rows',len(rows),'conflicts',len(conflicts))


if __name__=='__main__':main()
