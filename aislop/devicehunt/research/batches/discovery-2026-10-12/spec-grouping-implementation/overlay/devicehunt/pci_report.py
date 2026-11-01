"""Decode a supplied numeric lspci machine-readable inventory entirely offline."""
import hashlib
import re

from . import pci

MAX_BYTES = 10_000_000
MAX_RECORDS = 5000
SINGLE_TAGS = {'Slot','Class','Vendor','Device','SVendor','SDevice','PhySlot',
               'Rev','ProgIf','Driver','NUMANode','IOMMUGroup'}
SCOPE = ('User-supplied lspci observations decoded against the local PCI snapshot. '
         'The input is not independently verified and is never imported as catalog evidence. '
         'PCI functions and subsystem names do not identify a chassis or prove its maker, '
         'operator, compatibility, firmware access or marketplace provenance.')


def slot(value):
    match = re.fullmatch(r'(?:(?P<domain>[0-9a-fA-F]{4}):)?(?P<bus>[0-9a-fA-F]{2}):(?P<device>[0-9a-fA-F]{2})\.(?P<function>[0-7])', value)
    if not match or int(match['device'],16) > 31:
        raise ValueError('Invalid PCI slot; expected [dddd:]bb:dd.f')
    return ':'.join([match['domain'] or '0000',match['bus'],match['device']]).lower()+'.'+match['function']


def parse(body):
    if not isinstance(body,bytes) or len(body) > MAX_BYTES:
        raise ValueError('PCI inventory must be at most 10 MB')
    text = body.decode('utf-8-sig')
    records, fields, first_line, seen = [], {}, None, set()

    def flush():
        if not fields:
            return
        missing = {'Slot','Class','Vendor','Device'} - fields.keys()
        if missing:
            raise ValueError('PCI record at line '+str(first_line)+' lacks '+', '.join(sorted(missing)))
        normalized_slot = slot(fields['Slot'][0])
        if normalized_slot in seen:
            raise ValueError('Duplicate PCI slot: '+normalized_slot+'; use one machine per report')
        seen.add(normalized_slot)
        for tag in ['Vendor','Device','SVendor','SDevice','Class','Rev','ProgIf']:
            width = 2 if tag in ('Rev','ProgIf') else 4
            if tag in fields and not re.fullmatch(r'[0-9a-fA-F]{'+str(width)+'}',fields[tag][0]):
                raise ValueError('Non-numeric or invalid '+tag+' at line '+str(first_line)+'; use lspci -D -vmm -n')
        if ('SVendor' in fields) != ('SDevice' in fields):
            raise ValueError('PCI subsystem requires both SVendor and SDevice')
        identifier = fields['Vendor'][0]+':'+fields['Device'][0]
        subsystem = fields['SVendor'][0]+':'+fields['SDevice'][0] if 'SVendor' in fields else None
        class_code = fields['Class'][0]+fields.get('ProgIf',[''])[0]
        records.append({'slot':normalized_slot,'input_line':first_line,'id':pci.pair(identifier),
                        'subsystem_id':pci.pair(subsystem) if subsystem else None,
                        'class_code':class_code.lower(),
                        'input_fields':{k:list(v) for k,v in fields.items()},
                        'uninterpreted_tags':sorted(set(fields)-SINGLE_TAGS-{'Module'})})
        if len(records)>MAX_RECORDS:
            raise ValueError('PCI inventory exceeds 5000 functions')

    for number,line in enumerate(text.splitlines(),1):
        if not line.strip():
            flush();fields={};first_line=None
            continue
        match = re.fullmatch(r'([A-Za-z][A-Za-z0-9_-]*):\t([^\t\x00-\x1f]*)',line)
        if not match or not match[2].strip() or len(match[2])>4096:
            raise ValueError('Invalid PCI inventory line '+str(number)+'; use lspci -D -vmm -n')
        tag,value=match[1],match[2].strip()
        if tag in SINGLE_TAGS and tag in fields:
            raise ValueError('Repeated '+tag+' in PCI record; use -vmm, not -vm')
        if first_line is None:first_line=number
        fields.setdefault(tag,[]).append(value)
        if sum(map(len,fields.values()))>128:
            raise ValueError('PCI record exceeds 128 fields')
    flush()
    if not records:
        raise ValueError('PCI inventory has no records')
    return records


def analyze(db, body):
    records=parse(body)  # Reject the entire malformed input before any lookup.
    own=not db.in_transaction
    if own:db.execute('BEGIN')
    try:
        output=[]
        cache={}
        for record in records:
            key=(record['id'],record['subsystem_id'],record['class_code'])
            if key not in cache:cache[key]=pci.identify(db,*key)
            result=cache[key]
            status=('not_loaded' if result['state']=='not_loaded' else
                    'device_name_found' if result['device'] else
                    'vendor_only' if result['vendor'] else 'unknown_id')
            output.append({**record,'status':status,'decoded':result})
        return {'input_sha256':hashlib.sha256(body).hexdigest(),'format':'lspci -D -vmm -n',
                'scope':SCOPE,'summary':{'functions':len(output),
                **{status:sum(r['status']==status for r in output) for status in
                   ['device_name_found','vendor_only','unknown_id','not_loaded']}},'results':output}
    finally:
        if own:db.rollback()


def table_rows(result):
    def names(rows):return '; '.join(r['name'] for r in rows)
    return [{'slot':r['slot'],'pci_id':r['id'],'status':r['status'],
             'vendor':names(r['decoded']['vendor']),'device':names(r['decoded']['device']),
             'class_code':r['class_code'],'class_names':names(r['decoded']['class']),
             'reported_revision':r['input_fields'].get('Rev',[''])[0],
             'reported_driver':r['input_fields'].get('Driver',[''])[0],
             'subsystem_id':r['subsystem_id'] or '',
             'subsystem_vendor':names(r['decoded']['subsystem_vendor']),
             'subsystem':names(r['decoded']['subsystem'])} for r in result['results']]
