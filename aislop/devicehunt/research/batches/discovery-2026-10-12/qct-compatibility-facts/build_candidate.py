"""One existing QCT family, eight literal CCL rows; --check only reads files."""
import argparse
import hashlib
from html.parser import HTMLParser
import json
from pathlib import Path
import re

HERE=Path(__file__).resolve().parent
ROOT=HERE.parents[3]
DISCOVERY=HERE.parent/'qct-compatibility-discovery'
URL='https://www.qct.io/microsite/ccl/D52BQ-2U.html'
PARENT_URL='https://www.qct.io/product/index/Server/rackmount-server/2U-Rackmount-Server/QuantaGrid-D52BQ-2U'
SOURCE='qct-d52bq-2u-psu-ccl-2026-10-12'
IDENTITY='f65a474568c484317fc5'
HEADERS=['Power Capacity','Type','Efficiency','Dimension(mm)','240VDC','Cold redundancy','QuantaPN','VendorPN','Note']
LIMITS=('Historical QCT Compatible Components List (CCL), printed 2025-06-27, reference only; '
        'does not replace full QCT hardware certification or guarantee third-party component quality/reliability. '
        'Eight literal PSU compatibility-table rows on the existing QCT QuantaGrid D52BQ-2U family; '
        'not an installed inventory, PSU quantity/summed capacity, current certification/EOL/stock, '
        'manufacturer/factory identity, independent efficiency certification, physical-unit fitment, '
        'standalone-operation or safe-wiring claim. 240VDC Yes/No is not an input range; dimensions '
        'retain unlabelled axis order. QuantaPN and VendorPN stay distinct, no new identifiers/aliases '
        'or standalone PSU identities. Regional schedule and EOL footnote remain historical source text. '
        'No bridge to the separate Quanta namespace or other platforms.')
FACT_SCOPE=('Historical reference-only CCL row; not full hardware certification or a third-party quality/reliability guarantee. '
            'Redundancy labels establish no installed quantity/capacity; 240VDC Yes/No is not an input range/wiring rating. '
            'Efficiency and dimension fields remain literal, without certification or axis inference.')


def sha(path):return hashlib.sha256(path.read_bytes()).hexdigest()


class Tables(HTMLParser):
    def __init__(self,text):
        super().__init__(convert_charrefs=True)
        self.tables=[]
        self.t=self.r=self.c=None
        self.feed(text)
    def handle_starttag(self,tag,attrs):
        attrs=[list(a) for a in attrs]
        if tag=='table':
            assert self.t is None
            self.t={'attributes':attrs,'start_line':self.getpos()[0],'rows':[]};self.tables.append(self.t)
        elif tag=='tr' and self.t is not None:
            self.r={'attributes':attrs,'start_line':self.getpos()[0],'cells':[]};self.t['rows'].append(self.r)
        elif tag in ('td','th') and self.r is not None:
            self.c={'tag':tag,'attributes':attrs,'start_line':self.getpos()[0],'text_parts':[]}
        elif self.c is not None:self.c.setdefault('nested_elements',[]).append({'tag':tag,'attributes':attrs})
    def handle_data(self,text):
        if self.c is not None:self.c['text_parts'].append(text)
    def handle_endtag(self,tag):
        if tag in ('td','th') and self.c is not None:
            self.c['text']=' '.join(''.join(self.c['text_parts']).split());self.r['cells'].append(self.c);self.c=None
        elif tag=='tr':self.r=None
        elif tag=='table':self.t=None


class Parent(HTMLParser):
    def __init__(self,text):
        super().__init__(convert_charrefs=True);self.links=[];self.heading=False;self.text=[];self.headings=[];self.feed(text)
    def handle_starttag(self,t,a):
        if t=='a' and dict(a).get('href'):self.links.append(dict(a)['href'])
        if t=='h1':self.heading=True;self.text=[]
    def handle_data(self,text):
        if self.heading:self.text.append(text)
    def handle_endtag(self,t):
        if t=='h1' and self.heading:self.headings.append(' '.join(''.join(self.text).split()));self.heading=False


def serialized(value):return json.dumps(value,ensure_ascii=False,indent=2)+'\n'


def generate():
    snapshot=HERE/'snapshots/qct-d52bq-ccl.html';parent=HERE/'snapshots/parent-product.html'
    assert sha(snapshot)=='9c377013ee3bde686ae7372eb45aeadef00361b1ebe1f81a67bd872b5379d8b8'
    assert sha(parent)=='d5b0fd941c02b83842157ce5e9a4ad8e188fe5abe4bd6e9e07a57fe927061455'
    article=Tables(snapshot.read_bytes().decode('utf-8'))
    anchor=Parent(parent.read_bytes().decode('utf-8'))
    assert URL in anchor.links and 'QuantaGrid D52BQ-2U' in anchor.headings
    assert len(article.tables)==15 and sum(len(t['rows']) for t in article.tables)==892
    table=article.tables[11]
    assert [c['text'] for c in table['rows'][0]['cells']]==['PSU CCL']
    assert [c['text'] for c in table['rows'][1]['cells']]==HEADERS
    selected=table['rows'][2:-1];assert len(selected)==8
    footnote=[c['text'] for c in table['rows'][-1]['cells']]
    assert footnote==['*Grayed-out items: to be EOL soon or already EOL']
    intro=article.tables[0]
    assert [c['text'] for c in intro['rows'][0]['cells']]==['Publish date: 2025-06-27']
    assert [c['text'] for c in intro['rows'][1]['cells']]==['Quanta Cloud Technology']
    disclaimer=intro['rows'][2]['cells'][0]['text'];assert disclaimer.startswith('D52BQ-2U QCT provides the Compatible Components List')
    context=json.loads((HERE/'catalog-context.json').read_text())
    device=context['parent'];assert device['id']==IDENTITY and device['brand']=='QCT' and device['model']=='QuantaGrid D52BQ-2U' and device['revision']==''
    calculated=hashlib.sha256(json.dumps([re.sub(r'[^a-z0-9]','','QCT'.lower()),device['model'].casefold().strip(),'']).encode()).hexdigest()[:20]
    assert calculated==IDENTITY
    assert any(r['source_url']==PARENT_URL for r in context['existing_records'])
    facts=[];evidence=[];mapping=[]
    for row_number,row in enumerate(selected,3):
        values=[c['text'] for c in row['cells']];assert len(values)==9
        fields=dict(zip(HEADERS,values))
        value=('QCT CCL PSU row (D52BQ-2U; publication 2025-06-27): '+
               json.dumps(fields,ensure_ascii=False)+'. '+FACT_SCOPE+' [HTML table 12, row '+str(row_number)+']')
        facts.append(['power_description',value])
        evidence.append(dict(fact_ordinal_zero_based=len(facts)-1,fact_name='power_description',fact_value=value,
            source_URL=URL,source_sha256=sha(snapshot),snapshot='snapshots/qct-d52bq-ccl.html',table_1based=12,row_1based=row_number,
            table_attributes=table['attributes'],header_original_row=table['rows'][1],original_row=row,
            fields_in_header_order=fields,table_original_footnote_row=table['rows'][-1],table_footnote=footnote,
            introduction_original_table=intro,printed_company='Quanta Cloud Technology',printed_system='D52BQ-2U',printed_publish_date='2025-06-27',
            literal_CCL_disclaimer=disclaimer,scope_limitations=LIMITS))
        mapping.append(dict(fact_ordinal_zero_based=len(facts)-1,table_1based=12,row_1based=row_number,
                            QuantaPN=fields['QuantaPN'],VendorPN=fields['VendorPN'],all_nine_cells=fields))
    assert len(facts)==len(set(v for n,v in facts))==8
    ledger=[json.loads(line) for line in (DISCOVERY/'requests.jsonl').read_text().splitlines()]
    capture=next(r for r in ledger if r['url']==URL)
    assert capture['status']==200 and capture['body_sha256']==sha(snapshot)
    candidate=dict(sources=[dict(id=SOURCE,url=URL,tier='vendor',license='Public vendor factual compatibility documentation; original QCT/third-party rights retained.',
        coverage=LIMITS,snapshot='snapshots/qct-d52bq-ccl.html',original_sha256=sha(snapshot),captured_at_utc=capture['finished_at_utc'],
        printed_publish_date='2025-06-27',evidence_dependencies=[dict(path='snapshots/parent-product.html',sha256=sha(parent),purpose='Already imported parent product page; exact identity/href routing anchor only, no new source fact')])],
        devices=[dict(source=SOURCE,key='QuantaGrid D52BQ-2U:PSU-CCL',url=URL,brand='QCT',model='QuantaGrid D52BQ-2U',
            title='QCT QuantaGrid D52BQ-2U',kind=device['kind'],affiliations=[['Quanta Cloud Technology','documentation_vendor']],identifiers=[],facts=facts,
            existing_identity_id=IDENTITY,evidence=evidence,scope_limitations=LIMITS,
            parent_routing=dict(snapshot='snapshots/parent-product.html',sha256=sha(parent),source_URL=PARENT_URL,printed_heading='QuantaGrid D52BQ-2U',literal_CCL_href=URL,existing_parent_record_sources=context['existing_records']))],
        status='staged finite candidate; independent review required; no import',scope_limitations=LIMITS)
    provenance=dict(original_sources=[dict(original='research/batches/discovery-2026-10-12/qct-compatibility-discovery/compatibility.html',copied='snapshots/qct-d52bq-ccl.html',URL=URL,sha256=sha(snapshot),bytes=snapshot.stat().st_size,
        capture_finished_utc=capture['finished_at_utc'],original_headers_sha256=capture['headers_sha256']),
        dict(original='data/raw/vendor-discovery-qct-2026-10-04/d52bq-product.html',copied='snapshots/parent-product.html',URL=PARENT_URL,sha256=sha(parent),bytes=parent.stat().st_size,purpose='Auxiliary identity/href evidence only; not a second candidate source')],
        discovery_handoff_sha256=sha(DISCOVERY/'handoff.md'),discovery_observations_sha256=sha(DISCOVERY/'observations.json'),discovery_ledger_sha256=sha(DISCOVERY/'requests.jsonl'),
        reviewed_discovery='research/batches/discovery-2026-10-12/review/qct-compatibility-discovery-review.md',review_sha256=sha(HERE.parent/'review/qct-compatibility-discovery-review.md'),
        future_manifest_dependencies=[dict(path='candidate.json',purpose='One curated job'),dict(path='snapshots/qct-d52bq-ccl.html',sha256=sha(snapshot),purpose='Curated source snapshot'),
            dict(path='snapshots/parent-product.html',sha256=sha(parent),purpose='Auxiliary routing dependency: explicitly pin in manifest files; curated importer does not auto-enforce this ignored metadata field')],
        routing_namespace=dict(brand='QCT',model='QuantaGrid D52BQ-2U',revision='',device_id=IDENTITY),
        exclusions=['No new identity, standalone PSU, alias, identifier, Quanta namespace bridge or manufacturer/factory/target-family inference.',
                    'No axis order decoding, AC/DC input range, PSU installed count/summed power, pinout/connector/safe standalone wiring or efficiency certification inference.',
                    'No current regulatory/stock/EOL/support conclusion; preserve historical regional note, blank notes, style attributes and footer.',
                    'Other fourteen HTML tables unselected; auxiliary parent capture adds no new factual claims.'])
    return {'candidate.json':candidate,'provenance.json':provenance,'row-mapping.json':mapping}


def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--check',action='store_true');args=parser.parse_args()
    outputs=generate()
    for name,value in outputs.items():
        data=serialized(value)
        if args.check:assert (HERE/name).read_bytes()==data.encode('utf-8'),name
        else:(HERE/name).write_text(data)
    print('CHECK PASS' if args.check else 'STAGED', '1 source / 1 existing QCT record / 8 power_description facts; no imports')
    print('candidate sha256',sha(HERE/'candidate.json'))


if __name__=='__main__':main()
