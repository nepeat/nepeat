"""Offline discovery: preserve tables; select only the eight literal PSU rows."""
from pathlib import Path
from html.parser import HTMLParser
import json,hashlib
HERE=Path(__file__).resolve().parent
class Tables(HTMLParser):
 def __init__(self):super().__init__(convert_charrefs=True);self.tables=[];self.t=None;self.r=None;self.c=None;self.stack=[]
 def handle_starttag(self,tag,attrs):
  if tag=='table':
   assert self.t is None,'nested table requires separate review'
   self.t={'attributes':attrs,'start_line':self.getpos()[0],'rows':[]};self.tables.append(self.t)
  elif tag=='tr' and self.t is not None:
   self.r={'attributes':attrs,'start_line':self.getpos()[0],'cells':[]};self.t['rows'].append(self.r)
  elif tag in ('td','th') and self.r is not None:self.c={'tag':tag,'attributes':attrs,'start_line':self.getpos()[0],'text_parts':[]}
  elif self.c is not None:self.c.setdefault('nested_elements',[]).append({'tag':tag,'attributes':attrs})
 def handle_data(self,text):
  if self.c is not None:self.c['text_parts'].append(text)
 def handle_endtag(self,tag):
  if tag in ('td','th') and self.c is not None:
   self.c['text']=' '.join(''.join(self.c['text_parts']).split());self.r['cells'].append(self.c);self.c=None
  elif tag=='tr':self.r=None
  elif tag=='table':self.t=None

def main():
 raw=(HERE/'compatibility.html').read_bytes();p=Tables();p.feed(raw.decode('utf-8'));assert len(p.tables)==15
 table=p.tables[11];rows=table['rows'];assert [c['text'] for c in rows[0]['cells']]==['PSU CCL']
 headers=[c['text'] for c in rows[1]['cells']];assert headers==['Power Capacity','Type','Efficiency','Dimension(mm)','240VDC','Cold redundancy','QuantaPN','VendorPN','Note']
 selected=[]
 for i,r in enumerate(rows[2:-1],3):
  vals=[c['text'] for c in r['cells']];assert len(vals)==len(headers)==9
  selected.append({'table_1based':12,'row_1based':i,'fields':dict(zip(headers,vals)),'original_row':r})
 assert len(selected)==8
 result={'status':'discovery only; no candidate/import','source_url':'https://www.qct.io/microsite/ccl/D52BQ-2U.html','source_sha256':hashlib.sha256(raw).hexdigest(),'printed_system':'D52BQ-2U','printed_company':'Quanta Cloud Technology','printed_publish_date':'2025-06-27','tables':p.tables,'selected_psu_rows':selected,'table_footnote':[c['text'] for c in rows[-1]['cells']], 'limits':['CCL historical compatibility statements only, not proof of current support/stock or auction-unit configuration.','No manufacturer inferred from VendorPN. QuantaPN and VendorPN are separate literal columns, not aliases automatically.','Eight selected PSU rows use form01/style4; source attributes retained without converting table style into current EOL claims.','240VDC Yes/No is the table column only; no AC/input-range, connector/pinout, standalone operation or wiring inference.','Redundant and Cold redundancy Yes remain table labels, not installed quantity or summed capacity.','Regulatory note on FSF048-930G is a historical schedule claim, not current certification status.','Other fourteen tables retained as discovery structure only; no component/OS facts selected or identities created.']}
 for name,body in [('observations.json',result)]:
  with (HERE/name).open('x') as f:json.dump(body,f,indent=2,ensure_ascii=False);f.write('\n')
 print(json.dumps({'tables':len(p.tables),'rows':sum(len(t['rows']) for t in p.tables),'selected_psu_rows':len(selected)}))
if __name__=='__main__':main()
