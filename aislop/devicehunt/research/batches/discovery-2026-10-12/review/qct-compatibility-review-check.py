"""Independent saved HTML/policy review, with two bounded read-only catalog checks."""
import datetime as dt
import hashlib
from html.parser import HTMLParser
import json
from pathlib import Path
import re
import sqlite3
from urllib import robotparser

HERE=Path(__file__).resolve().parent
ROOT=HERE.parents[3]
BATCH=HERE.parent/'qct-compatibility-discovery'


def sha(path):return hashlib.sha256(path.read_bytes()).hexdigest()


class Parser(HTMLParser):
    def __init__(self,body):
        super().__init__(convert_charrefs=True)
        self.tables=[]
        self.table=None
        self.row=None
        self.cell=None
        self.feed(body)

    def handle_starttag(self,tag,attrs):
        attrs=[list(a) for a in attrs]
        if tag=='table':
            assert self.table is None
            self.table={'attributes':attrs,'start_line':self.getpos()[0],'rows':[]}
            self.tables.append(self.table)
        elif tag=='tr' and self.table is not None:
            self.row={'attributes':attrs,'start_line':self.getpos()[0],'cells':[]}
            self.table['rows'].append(self.row)
        elif tag in ('td','th') and self.row is not None:
            self.cell={'tag':tag,'attributes':attrs,'start_line':self.getpos()[0],'text_parts':[]}
        elif self.cell is not None:
            self.cell.setdefault('nested_elements',[]).append({'tag':tag,'attributes':attrs})

    def handle_data(self,text):
        if self.cell is not None:self.cell['text_parts'].append(text)

    def handle_endtag(self,tag):
        if tag in ('td','th') and self.cell is not None:
            self.cell['text']=' '.join(''.join(self.cell['text_parts']).split())
            self.row['cells'].append(self.cell)
            self.cell=None
        elif tag=='tr':self.row=None
        elif tag=='table':self.table=None


def main():
    assert sha(BATCH/'handoff.md')=='896c61de2dd8cbc3593a99bc048fa97224a64fdb3dc6350a72ac8f8665254123'
    sums=[]
    for line in (BATCH/'SHA256SUMS').read_text().splitlines():
        expected,name=line.split('  ',1)
        assert sha(BATCH/name)==expected,name
        sums.append(name)
    plan=json.loads((BATCH/'plan.json').read_text())
    assert sha(ROOT/plan['basis'])==plan['basis_sha256']
    basis=(ROOT/plan['basis']).read_text()
    class Links(HTMLParser):
        def __init__(self):super().__init__();self.links=[]
        def handle_starttag(self,t,a):
            if t=='a' and dict(a).get('href'):self.links.append(dict(a)['href'])
    links=Links();links.feed(basis)
    assert plan['url'] in links.links
    prior=json.loads((ROOT/plan['prior_capture_report']).read_text())
    assert not any(r['url']==plan['url'] for r in prior)
    assert sorted(r['key'] for r in prior if r['status']==404)==['d51b-product','t41s-product']
    requests=[json.loads(line) for line in (BATCH/'requests.jsonl').read_text().splitlines()]
    assert len(requests)==plan['max_http_requests']==2
    assert [r['url'] for r in requests]==['https://www.qct.io/robots.txt',plan['url']]
    policy=robotparser.RobotFileParser()
    policy.parse((BATCH/'robots.txt').read_text().splitlines())
    for row in requests:
        assert row['status']==200 and row['curl_returncode']==0 and row['stderr']==''
        assert row['redirects_followed'] is False and row['retries']==0 and not row['login_marker']
        assert row['argv'][-1]==row['url'] and '-L' not in row['argv'] and '--location' not in row['argv']
        assert row['argv'][row['argv'].index('--retry')+1]=='0'
        body=(BATCH/row['body_path']).read_bytes()
        assert len(body)==row['body_bytes'] and sha(BATCH/row['body_path'])==row['body_sha256']
        assert sha(BATCH/row['headers_path'])==row['headers_sha256']
        head=(BATCH/row['headers_path']).read_text()
        assert re.findall(r'^HTTP/\S+ (\d+)',head,re.M)==['200'] and not re.search(r'^Location:',head,re.M|re.I)
        assert not re.search(rb'<input[^>]+type=["\']?password|<title>[^<]*(?:sign in|log in|login)',body,re.I)
    capture=json.loads((BATCH/'capture-result.json').read_text())
    assert capture['request']==requests[1]
    recorded=capture['policy']
    assert policy.can_fetch(recorded['user_agent'],plan['url']) and recorded['allowed']
    assert sha(BATCH/'robots.txt')==recorded['robots_sha256']
    delay=policy.crawl_delay(recorded['user_agent']) or 0
    assert delay==recorded['delay_seconds']==0
    raw=(BATCH/'compatibility.html').read_bytes().decode('utf-8')
    parsed=Parser(raw)
    observed=json.loads((BATCH/'observations.json').read_text())
    assert parsed.tables==observed['tables']
    assert len(parsed.tables)==15 and sum(len(t['rows']) for t in parsed.tables)==892
    assert sha(BATCH/'compatibility.html')==observed['source_sha256']==requests[1]['body_sha256']
    all_text=' '.join(c['text'] for t in parsed.tables for r in t['rows'] for c in r['cells'])
    for token in ('Quanta Cloud Technology','D52BQ-2U','2025-06-27'):assert token in all_text
    table=parsed.tables[11]
    assert [c['text'] for c in table['rows'][0]['cells']]==['PSU CCL']
    headers=[c['text'] for c in table['rows'][1]['cells']]
    assert headers==['Power Capacity','Type','Efficiency','Dimension(mm)','240VDC','Cold redundancy','QuantaPN','VendorPN','Note']
    selected=table['rows'][2:-1]
    assert len(selected)==len(observed['selected_psu_rows'])==8
    for i,(row,saved) in enumerate(zip(selected,observed['selected_psu_rows']),3):
        assert row==saved['original_row'] and saved['table_1based']==12 and saved['row_1based']==i
        assert len(row['cells'])==9 and saved['fields']==dict(zip(headers,[c['text'] for c in row['cells']]))
        assert row['attributes']==[['class','form01']] and all(c['attributes']==[['class','style4']] for c in row['cells'])
        assert 1<=row['start_line']<=len(raw.split('\n'))
        assert all(1<=c['start_line']<=len(raw.split('\n')) for c in row['cells'])
    assert [c['text'] for c in table['rows'][-1]['cells']]==observed['table_footnote']==['*Grayed-out items: to be EOL soon or already EOL']
    disclaimer=[c['text'] for t in parsed.tables for r in t['rows'] for c in r['cells'] if 'compatib' in c['text'].casefold() and len(c['text'])>150]
    assert disclaimer
    file=ROOT/'data/devices.sqlite'
    before=sha(file)
    db=sqlite3.connect(file.as_uri()+'?mode=ro',uri=True);db.row_factory=sqlite3.Row;db.execute('BEGIN')
    source_matches=[dict(r) for r in db.execute('SELECT id,url FROM sources WHERE url=?',(plan['url'],))]
    parents=[dict(r) for r in db.execute('SELECT id,brand,model,revision FROM devices WHERE model IN (?,?) ORDER BY brand,model',('D52BQ-2U','QuantaGrid D52BQ-2U'))]
    db.rollback();db.close()
    after=sha(file);assert before==after
    result=dict(checked_utc=dt.datetime.now(dt.timezone.utc).isoformat(),verdict='ACCEPT discovery only',handoff_sha256=sha(BATCH/'handoff.md'),
                artifact_hashes_verified=len(sums),source_sha256=sha(BATCH/'compatibility.html'),observations_sha256=sha(BATCH/'observations.json'),
                HTTP=2,statuses={'200':2},policy_sha256=sha(BATCH/'robots.txt'),exact_path_allowed=True,delay_seconds=0,prior_failures_excluded=['d51b-product','t41s-product'],
                plan_basis_sha256=plan['basis_sha256'],tables=15,rows=892,selected_table_1based=12,selected_PSU_rows=8,selected_cells=72,headers=headers,
                PSU_rows=[r['fields'] for r in observed['selected_psu_rows']],table_footnote=observed['table_footnote'],CCL_disclaimer=disclaimer,
                raw_tables_attributes_cells_text_fragments_lines_equal=True,current_catalog_sha256_before=before,current_catalog_sha256_after=after,
                current_source_exact_URL_matches=source_matches,parent_identities=parents,readonly_catalog_query_cases=2,
                scope='review only; no network/import/candidate/code/shared-data mutation; other fourteen tables not selected as evidence')
    (HERE/'qct-compatibility-review-checks.json').write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps({k:v for k,v in result.items() if k not in ('PSU_rows','CCL_disclaimer','parent_identities')},indent=2))


if __name__=='__main__':main()
