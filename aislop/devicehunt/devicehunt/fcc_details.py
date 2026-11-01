"""Cached mirror detail pages and explicitly truncated exhibit-text excerpts."""
import json
import re
from pathlib import Path

from .catalog import digest
from .documents import dependency
from .fcc_index import BASE, Node, document, detail_index_rows

SOURCE = 'fcc-detail-pages'


def parse_page(body, fid, company):
    root = document(body.decode('utf-8'))
    headings = [n.text() for n in root.walk('h1')]
    if len(headings) != 1 or not re.match(re.escape(fid) + r'(?:\s|$)', headings[0]):
        raise ValueError('FCC detail identity differs: ' + fid)
    blocks = [h.parent for h in root.walk('h2') if h.text() == 'Application Details']
    if len(blocks) != 1:
        raise ValueError('Missing or duplicate application-details block')
    fields = {}
    for dl in blocks[0].walk('dl'):
        for item in dl.walk('div'):
            keys, values = list(item.walk('dt')), list(item.walk('dd'))
            if len(keys) == len(values) == 1:
                key, value = keys[0].text(), values[0].text()
                if key in fields:
                    raise ValueError('Duplicate application detail field')
                fields[key] = value
    required = {'Equipment Class', 'Application Purpose', 'Company'}
    if not required <= fields.keys() or not all(fields[k] for k in required) or fields['Company'] != company:
        raise ValueError('Incomplete or mismatched FCC company/details')
    links = []
    for a in root.walk('a'):
        href = a.attrs.get('href', '')
        if not href.startswith('/api/files/'):
            continue
        if not re.fullmatch(r'/api/files/[A-Za-z0-9_./%()&-]+', href) or '..' in href:
            raise ValueError('Unrecognized public exhibit URL')
        parent, category = a.parent, None
        while parent:
            labels = [n.text() for n in parent.children if isinstance(n, Node) and n.tag in ('h2','h3','h4')]
            if len(labels) == 1:
                category = labels[0]; break
            parent = parent.parent
        title = a.attrs.get('title', '')
        if not category or not title.startswith('Open '):
            raise ValueError('Unscoped FCC exhibit link')
        links.append({'category':category,'title':title[5:],'document_url':BASE+href})
    if len({r['document_url'] for r in links}) != len(links):
        raise ValueError('Duplicate FCC exhibit URL')
    sections = [s for s in root.walk('section') if s.attrs.get('id') == 'document-text']
    if len(sections) > 1:
        raise ValueError('Duplicate document-text section')
    excerpts = []
    if sections:
        for block in sections[0].walk('details'):
            labels = list(block.walk('summary'))
            paragraphs = list(block.walk('p'))
            texts = [p.text() for p in paragraphs if 'whitespace-pre-wrap' in p.attrs.get('class','').split()]
            notes = [p.text() for p in paragraphs if 'whitespace-pre-wrap' not in p.attrs.get('class','').split()]
            if (len(labels) != 1 or len(texts) != 1 or not texts[0]
                    or any(n != 'Text truncated - open the document above for the full version.' for n in notes)):
                raise ValueError('Changed FCC excerpt structure or truncation note')
            excerpts.append({'category':labels[0].text(),'text':texts[0],'truncated':bool(notes)})
    return {'description': headings[0][len(fid):].strip(), 'fields':fields, 'links':links, 'excerpts':excerpts}


def import_bundle(c, path):
    path = Path(path).resolve(); data = json.loads(path.read_text())
    if data.get('schema_version') != 1 or data.get('mirror') != BASE or not isinstance(data.get('entries'),list):
        raise ValueError('Unexpected FCC detail bundle')
    index = {r['fcc_id']:r for r in detail_index_rows(c.db)}
    entries = data['entries']
    if not index or len(entries) != len(index) or {e['fcc_id'] for e in entries} != set(index):
        raise ValueError('Detail scope must match the complete imported company-index snapshot')
    ready = []
    for entry in entries:
        fid = entry['fcc_id']; row = index[fid]
        if any(entry.get(k) != row[k] for k in ('grantee_code','grantee_name','source_url')):
            raise ValueError('Detail/index scope mismatch')
        status = entry.get('status')
        if status == 'unavailable':
            if not isinstance(entry.get('issue'), str) or not entry['issue'].strip() or 'page' in entry:
                raise ValueError('Unavailable detail requires a reason and no parsed page')
            parsed = None
        elif status == 'captured':
            parsed = parse_page(dependency(path.parent,entry['page']),fid,row['grantee_name'])
        else:
            raise ValueError('Unknown FCC detail capture status')
        ready.append((entry,parsed))
    c.source(SOURCE,BASE,'regulatory_mirror','Factual mirror metadata and excerpts; document rights retained',
             'One detail-page attempt for every ID in the imported incomplete company-index snapshot. '
             'Not every FCC application for an ID. Display dates and frequency strings stay verbatim in fields_json. '
             'Public exhibit links are not downloaded PDFs. Text excerpts retain explicit truncation flags; '
             'absence of a flag is not proof of full-document coverage. Excerpts have no inferred document ID or page number. '
             'No FCC product-code/model alias, manufacturer or deployment assertion is created.',path)
    c.db.execute('DELETE FROM fcc_detail_pages WHERE source_id=?',(SOURCE,))
    totals={'attempted':len(ready),'captured':0,'unavailable':0,'links':0,'excerpts':0,'truncated_excerpts':0}
    for entry,p in ready:
        fid=entry['fcc_id']
        c.db.execute('INSERT INTO fcc_detail_pages VALUES (?,?,?,?,?,?,?,?,?)',
                     (SOURCE,fid,entry['grantee_code'],entry['grantee_name'],p['description'] if p else '',
                      json.dumps(p['fields'],ensure_ascii=False,sort_keys=True) if p else '{}',
                      entry['source_url'],entry['status'],entry.get('issue','')))
        totals[entry['status']] += 1
        if p:
            for n,r in enumerate(p['links'],1):
                c.db.execute('INSERT INTO fcc_detail_links VALUES (?,?,?,?,?,?)',
                             (SOURCE,fid,n,r['category'],r['title'],r['document_url']))
            for n,r in enumerate(p['excerpts'],1):
                c.db.execute('INSERT INTO fcc_text_excerpts VALUES (?,?,?,?,?,?)',
                             (SOURCE,fid,n,r['category'],r['text'],int(r['truncated'])))
            totals['links']+=len(p['links']); totals['excerpts']+=len(p['excerpts'])
            totals['truncated_excerpts']+=sum(r['truncated'] for r in p['excerpts'])
    c.db.execute('UPDATE sources SET records_seen=? WHERE id=?',(len(ready),SOURCE))
    c.db.commit()
    return totals
