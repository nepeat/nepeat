"""Reviewed Korean approval mirror fields, with derived models kept distinct."""
import datetime as dt
import json
import re
from pathlib import Path

from .catalog import target_for
from .documents import dependency

METHOD = 'reviewed factual web-tool table extract; not original HTML'
FIELDS = ('Record Type', 'Authorization / Certificate Number', 'Applicant / Company',
          'Device / Product Name', 'Model', 'Derived Models', 'Manufacturer', 'Country', 'Date')


def parse_capture(data):
    if (not isinstance(data, dict) or data.get('schema_version') != 1
            or data.get('capture_method') != METHOD
            or data.get('language') != 'en'
            or not isinstance(data.get('fields'), dict)
            or set(data['fields']) != set(FIELDS)):
        raise ValueError('Unrecognized reviewed RRA capture')
    fields = data['fields']
    if any(not isinstance(v, str) or '\n' in v or '\r' in v or '\x00' in v
           or v != v.strip() for v in fields.values()):
        raise ValueError('Invalid reviewed RRA field')
    certificate = fields['Authorization / Certificate Number']
    if (not re.fullmatch(r'R(?:R|C|S)-[A-Za-z0-9]{3}-[A-Za-z0-9_-]+', certificate)
            or data.get('source_url') != 'https://fccid.io/' + certificate
            or fields['Record Type'] != 'Radio conformity assessment'):
        raise ValueError('Unrecognized RRA certificate source or type')
    for value in (data.get('reviewed_on'), fields['Date']):
        if not isinstance(value, str) or not re.fullmatch(r'\d{4}-\d{2}-\d{2}', value):
            raise ValueError('Invalid RRA date')
        dt.date.fromisoformat(value)
    manufacturer = fields['Manufacturer']
    target = target_for(manufacturer)
    if target is None or not fields['Applicant / Company'] or not fields['Device / Product Name']:
        raise ValueError('RRA capture lacks a reviewed target manufacturer')
    # A reviewed list is required: never guess delimiters in a combined model cell.
    derived = data.get('derived_models')
    if (not isinstance(derived, list) or any(not isinstance(m, str) for m in derived)
            or fields['Derived Models'] != ', '.join(derived)):
        raise ValueError('Derived model list differs from the captured field')
    models = [fields['Model'], *derived]
    if (any(not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9._-]*', m) for m in models)
            or len({m.casefold() for m in models}) != len(models)):
        raise ValueError('Ambiguous or duplicate RRA models')
    return certificate, target, models, fields


def import_bundle(c, path):
    path = Path(path).resolve()
    bundle = json.loads(path.read_text())
    if (bundle.get('schema_version') != 1 or not isinstance(bundle.get('captures'), list)
            or not bundle['captures']):
        raise ValueError('Invalid reviewed RRA bundle')
    ready = []
    for entry in bundle['captures']:
        capture = json.loads(dependency(path.parent, entry))
        ready.append((capture, parse_capture(capture), entry['sha256']))
    if len({item[1][0] for item in ready}) != len(ready):
        raise ValueError('Repeated RRA certificate capture')
    # Validate every capture and dependency before replacing any source records.
    for capture, (certificate, target, models, fields), sha in ready:
        sid = 'rra-reviewed-' + certificate.lower()
        url = capture['source_url']
        c.source(sid, url, 'regulatory_mirror', 'Selected factual fields; mirror source rights retained',
                 METHOD + '. English mirror fields only; Korean text and original RRA record '
                 'were not independently verified. Main and explicitly listed derived models '
                 'remain separate identities, not aliases. Manufacturer/applicant are mirror '
                 'claims, not physical factory verification. Country and displayed date remain '
                 'raw source fields; no present approval validity, FCC ID, operator deployment '
                 'or historical completeness is inferred. Catalog brand is a manufacturer '
                 'namespace, not an asserted printed brand.', path)
        for index, model in enumerate(models):
            c.add(sid, model, url, target, model, target + ' ' + model,
                  fields['Device / Product Name'], capture,
                  [(fields['Manufacturer'], 'registry_manufacturer'),
                   (fields['Applicant / Company'], 'regulatory_applicant')],
                  [('rra_certificate_id', certificate)],
                  [('rra_model_role', 'main' if index == 0 else 'derived'),
                   ('rra_main_model', fields['Model']), ('country_raw', fields['Country']),
                   ('displayed_date_raw', fields['Date']), ('capture_sha256', sha),
                   ('capture_method', METHOD)])
        c.finish(sid, len(models))
    return {'sources': len(ready), 'model_assertions': sum(len(item[1][2]) for item in ready)}
