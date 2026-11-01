"""Reviewed model/applicant fields from cached FCC report-cover excerpts."""
import json
import re
from pathlib import Path

from .catalog import digest, target_for
from .fcc_details import parse_page
from .fcc_index import BASE

SOURCE = 'fcc-accton-report-covers'
COVER = re.compile(r'FCC\s+TEST\s+REPORT\s+REPORT\s+NO\.\s*:\s*(?P<report>\S+)\s+'
                   r'MODEL\s+NO\.\s*:\s*(?P<model>.+?)\s+RECEIVED\s*:', re.I)
APPLICANT = re.compile(r'APPLICANT\s*:\s*(.*?)\s+ADDRESS\s*:', re.I)
MODEL = re.compile(r'[A-Za-z0-9]+(?:[-.][A-Za-z0-9]+)*')


def cover_fields(text, fid):
    # Deliberately restricted to one reviewed report-cover layout. Body tables,
    # test instruments, free-form marketing names and footnotes are not parsed.
    head = text[:1800]
    covers, applicants = list(COVER.finditer(head)), list(APPLICANT.finditer(head))
    if len(covers) != 1 or len(applicants) != 1:
        return None
    cover, applicant = covers[0], applicants[0][1]
    fids = re.findall(r'FCC\s+ID\s*\.?\s*:\s*([A-Z0-9-]+)', head)
    if not fids or set(fids) != {fid} or target_for(applicant) != 'Accton':
        return None
    raw = cover['model']; models = [m.strip() for m in raw.split(',')]
    if (not re.fullmatch(r'RF[A-Za-z0-9-]+', cover['report']) or len(models) != len(set(models))
            or any(not MODEL.fullmatch(m) or not any(ch.isdigit() for ch in m) for m in models)):
        return None
    return {'report_number':cover['report'], 'model_field':raw, 'models':models,
            'applicant':applicant, 'cover_span':cover[0]}


def import_capture(c, path):
    path=Path(path).resolve(); data=json.loads(path.read_text()); root=path.parent
    if data.get('schema_version')!=1 or data.get('capture_format')!='reviewed_fcc_cover_fields' or not data.get('entries'):
        raise ValueError('Expected reviewed FCC cover capture')
    ready=[]; seen=set(); cached={}
    for row in data['entries']:
        fid=row['fcc_id']; ordinal=row['excerpt_ordinal']; entry=row['html']
        if (not re.fullmatch(r'HED[A-Z0-9-]+',fid) or type(ordinal) is not int or ordinal<1
                or row.get('source_url')!=BASE+'/fcc/'+fid):
            raise ValueError('Invalid report-cover identity')
        name=entry['path']
        if not isinstance(name,str) or Path(name).is_absolute():
            raise ValueError('Report capture path escapes snapshot')
        p=(root/name).resolve()
        if root not in p.parents:
            raise ValueError('Report capture path escapes snapshot')
        if name not in cached: cached[name]=p.read_bytes()
        body=cached[name]
        if digest(body)!=entry['sha256']:
            raise ValueError('Report-cover HTML checksum mismatch')
        parsed=parse_page(body,fid,'Accton Technology Corp')
        if ordinal>len(parsed['excerpts']):raise ValueError('Report excerpt missing')
        excerpt=parsed['excerpts'][ordinal-1]
        if excerpt['category']!='Test Report' or digest(excerpt['text'].encode())!=row['excerpt_sha256']:
            raise ValueError('Report excerpt association changed')
        fields=cover_fields(excerpt['text'],fid)
        if fields is None or any(row.get(k)!=fields[k] for k in fields):
            raise ValueError('Reviewed report-cover fields changed or ambiguous')
        key=(fid,fields['report_number'])
        if key in seen:raise ValueError('Duplicate filing/report cover')
        seen.add(key);ready.append((row,excerpt,parsed['description']))
    c.source(SOURCE,BASE,'regulatory_mirror','Factual report-cover fields; original source rights retained',
             'Selected, reviewed simple MODEL NO and APPLICANT fields in cached mirror Test Report excerpts. '
             'Source is HTML mirror text, not an original PDF or complete filing. No manufacturer or factory claim. '
             'Accton identity namespace follows the report applicant, not necessarily retail brand. '
             'Comma-separated model fields remain separate identities, not aliases. Ambiguous spaces, OCR fragments, '
             'OEM subfields, free-form names and test-equipment tables are excluded pending original-document review. '
             'Repeated copies of a filing/report cover contribute one assertion per model.',path)
    count=0
    for row,excerpt,description in ready:
        for model in row['models']:
            c.add(SOURCE,row['fcc_id']+'|'+row['report_number']+'|'+model,row['source_url']+'#document-text',
                  'Accton',model,model,'FCC-listed equipment',row,
                  [(row['applicant'],'regulatory_applicant')], [('fcc_id',row['fcc_id'])],
                  [('report_number',row['report_number']),('source_model_field',row['model_field']),
                   ('equipment_description',description),('mirror_excerpt_ordinal',str(row['excerpt_ordinal'])),
                   ('mirror_excerpt_truncated',str(bool(excerpt['truncated'])).lower()),
                   ('identity_namespace_scope','Accton is the report applicant; retail brand and physical manufacturer are not inferred'),
                   ('evidence_format','Cached HTML mirror excerpt; original PDF unavailable during review')])
            count+=1
    c.finish(SOURCE,len(ready))
    return {'reviewed_report_covers':len(ready),'model_assertions':count}
