"""Reviewed Sporton cover fields in already archived FCC mirror excerpts."""
import json
import re
from pathlib import Path

from .catalog import digest, target_for
from .fcc_details import parse_page
from .fcc_index import BASE
from .fcc_cover_models import MODEL

SOURCE = 'fcc-sporton-cover-roles'
COMPANIES = {'HED': 'Accton Technology Corp', 'YZK': 'Edgecore Networks Corporation'}
COVER = re.compile(
    r'FCC RADIO TEST REPORT\s+Applicant[’\']s company\s+(?P<applicant>.+?)\s+'
    r'Applicant Address\s+(?P<applicant_address>.+?)\s+FCC ID\s+(?P<fcc_id>[A-Z0-9-]+)\s+'
    r'Manufacturer[’\']s company\s+(?P<manufacturer>.+?)\s+'
    r'Manufacturer Address\s+(?P<manufacturer_address>.+?)\s+'
    r'Product Name\s+(?P<product>.+?)\s+Brand Name\s+(?P<brand>.+?)\s+'
    r'Model (?:Name|No\.)\s+(?P<model_field>.+?)\s+Test Rule(?: Part\(s\))?\s+', re.I)
SCOPE = ('Reviewed labelled applicant, manufacturer, brand and model fields on Sporton report covers '
         'in cached FCCID.co Test Report excerpts. HTML mirror text, not original PDFs; truncation '
         'and excerpt hashes retained. Manufacturer is the report statement, not an independent '
         'factory audit. Applicant target supplies the identity namespace, not presumed manufacture. '
         'Simple comma-separated model fields remain distinct identities, never aliases. Grouped '
         'brand fields stay uninterpreted; no pairing brands with models by position. Unrecognized '
         'manufacturer names remain unassigned to targets. No grant validity, deployment or unit '
         'provenance inferred. Existing applicant-only cover extraction remains separate.')


def fields(text, fid):
    head = text[:1800]
    if 'SPORTON International Inc.' not in head or head.count('FCC RADIO TEST REPORT') != 1:
        return None
    found = list(COVER.finditer(head))
    if len(found) != 1:
        return None
    result = found[0].groupdict()
    fids = re.findall(r'FCC ID\s*:?\s*([A-Z0-9-]+)', head)
    prefix = next((p for p in COMPANIES if fid.startswith(p)), None)
    if (prefix is None or result['fcc_id'] != fid or set(fids) != {fid} or
            any(ch.isalnum() and not ch.isascii() for ch in result['applicant']) or
            target_for(result['applicant']) != target_for(COMPANIES[prefix])):
        return None
    models = [m.strip() for m in result['model_field'].split(',')]
    if (len({m.casefold() for m in models}) != len(models) or
            any(not MODEL.fullmatch(m) or not any(c.isdigit() for c in m) for m in models)):
        return None
    if any(not v.strip() or len(v) > 500 for v in result.values()):
        return None
    result['models'] = models
    return result


def import_capture(c, path):
    path = Path(path).resolve(); data = json.loads(path.read_text()); root = path.parent
    if (data.get('schema_version') != 1 or data.get('capture_format') != 'reviewed_sporton_cover_roles'
            or not isinstance(data.get('entries'), list) or not data['entries']):
        raise ValueError('Expected reviewed Sporton cover capture')
    ready, seen, cached = [], set(), {}
    for row in data['entries']:
        fid = row['fcc_id']; ordinal = row['excerpt_ordinal']; entry = row['html']
        prefix = next((p for p in COMPANIES if re.fullmatch(p+r'[A-Z0-9-]+', fid)), None)
        if (prefix is None or type(ordinal) is not int or ordinal < 1 or
                row.get('source_url') != BASE+'/fcc/'+fid):
            raise ValueError('Invalid Sporton excerpt identity')
        key = (fid, ordinal)
        if key in seen:
            raise ValueError('Duplicate Sporton excerpt')
        seen.add(key)
        name = entry['path']
        if not isinstance(name, str) or Path(name).is_absolute():
            raise ValueError('Sporton snapshot path escapes capture')
        p = (root/name).resolve()
        if root not in p.parents:
            raise ValueError('Sporton snapshot path escapes capture')
        if name not in cached:
            cached[name] = p.read_bytes()
        body = cached[name]
        if digest(body) != entry['sha256']:
            raise ValueError('Sporton HTML checksum mismatch')
        parsed = parse_page(body, fid, COMPANIES[prefix])
        if ordinal > len(parsed['excerpts']):
            raise ValueError('Sporton excerpt missing')
        excerpt = parsed['excerpts'][ordinal-1]
        if excerpt['category'] != 'Test Report' or digest(excerpt['text'].encode()) != row['excerpt_sha256']:
            raise ValueError('Sporton excerpt association changed')
        values = fields(excerpt['text'], fid)
        if values is None or values != row['fields']:
            raise ValueError('Reviewed Sporton fields changed or ambiguous')
        ready.append((row, excerpt, values))
    c.source(SOURCE, BASE, 'regulatory_mirror', 'Factual cover fields; original document rights retained', SCOPE, path)
    count = 0
    for row, excerpt, values in ready:
        namespace = target_for(values['applicant'])
        affiliations = [(values['applicant'], 'regulatory_applicant'), (values['manufacturer'], 'manufacturer')]
        # Only a single known target brand is assigned. Combined or unfamiliar
        # brand cells remain facts; no model-to-brand pairing is guessed.
        if target_for(values['brand']):
            affiliations.append((values['brand'], 'brand'))
        for model in values['models']:
            key = row['fcc_id']+'|'+str(row['excerpt_ordinal'])+'|'+model
            c.add(SOURCE, key, row['source_url']+'#document-text', namespace, model,
                  namespace+' '+model, 'FCC report-listed equipment', row, affiliations,
                  [('fcc_id', row['fcc_id'])],
                  [('cover_'+k, v) for k,v in values.items() if k != 'models']+
                  [('evidence_scope', SCOPE), ('mirror_excerpt_ordinal', str(row['excerpt_ordinal'])),
                   ('mirror_excerpt_truncated', str(bool(excerpt['truncated'])).lower()),
                   ('mirror_excerpt_sha256', row['excerpt_sha256'])])
            # ASCII target aliases must never erase non-ASCII letters from a
            # printed company name and create a false target relationship.
            for name, role in affiliations:
                if any(ch.isalnum() and not ch.isascii() for ch in name):
                    c.db.execute('UPDATE affiliations SET target=NULL WHERE company=? AND role=? '
                                 'AND record_id IN (SELECT id FROM records WHERE source_id=? AND source_key=?)',
                                 (name, role, SOURCE, key))
            count += 1
    c.finish(SOURCE, len(ready))
    return {'reviewed_covers': len(ready), 'model_assertions': count}
