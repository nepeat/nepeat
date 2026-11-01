"""Import reviewed factual extracts of selected OCP product catalog fields.

This is deliberately a capture importer, not a complete OCP crawler. Page
identity is the numeric product ID; a stale URL slug is not a model alias.
"""
import datetime as dt
import json
import re
from pathlib import Path
from urllib.parse import urlsplit

SOURCE = 'ocp-products-reviewed'
BRANDS = {'Celestica': 'Celestica', 'Edgecore Networks': 'Edge-Core'}
FACTS = {'cooling_description', 'ports_description', 'power_description',
         'software_installation_description', 'physical_description',
         'source_caveat', 'contribution_url'}


def validate(path):
    data = json.loads(Path(path).read_text())
    if (data.get('schema_version') != 1 or data.get('capture_format') != 'reviewed_web_tool_fields'
            or not isinstance(data.get('products'), list) or not data['products']):
        raise ValueError('Expected a reviewed OCP product capture')
    stamp = dt.datetime.fromisoformat(data['retrieved_at'])
    if stamp.tzinfo is None:
        raise ValueError('OCP capture timestamp must include timezone')
    seen, prepared = set(), []
    for row in data['products']:
        pid = row.get('product_id')
        url = urlsplit(row.get('url', ''))
        if (type(pid) is not int or pid <= 0 or pid in seen or url.scheme != 'https'
                or url.netloc != 'www.opencompute.org' or url.query or url.fragment
                or not re.fullmatch('/products/' + str(pid) + '/[a-z0-9-]+', url.path)):
            raise ValueError('Invalid or duplicate OCP product ID/URL')
        seen.add(pid)
        if row.get('retrieval_method') not in ('web_search_result', 'web_open_result'):
            raise ValueError('OCP capture requires retrieval method')
        heading = row.get('heading_brand')
        if heading not in BRANDS or row.get('target_solution_provider') != heading:
            raise ValueError('OCP heading brand and target solution provider must agree')
        raw = row.get('model_field', '')
        if not isinstance(raw, str):
            raise ValueError('OCP model field must be text')
        if heading == 'Celestica':
            match = re.fullmatch(r'Celestica ((?:DS|ES|EG)\d{4}(?:/(?:DS|ES|EG)\d{4})*) (?:1U|2U) .+', raw)
            models = match[1].split('/') if match else []
        else:
            models = [raw] if re.fullmatch(r'AS\d{4}-[A-Z0-9]+(?:-[A-Z0-9]+)*', raw) else []
        if not models or len(set(models)) != len(models) or row.get('models') != models:
            raise ValueError('OCP reviewed models differ from explicit model field')
        facts = row.get('facts', [])
        if not isinstance(facts, list):
            raise ValueError('OCP facts must be a list')
        for fact in facts:
            if (not isinstance(fact, list) or len(fact) != 2 or fact[0] not in FACTS
                    or not isinstance(fact[1], str) or not fact[1].strip()):
                raise ValueError('Unreviewed OCP fact type')
            if fact[0] == 'contribution_url':
                link = urlsplit(fact[1])
                if link.scheme != 'https' or link.netloc != 'www.opencompute.org' or link.path != '/contributions':
                    raise ValueError('Invalid OCP contribution link')
        prepared.append((row, BRANDS[heading], models))
    return data, prepared


def import_capture(c, path):
    data, prepared = validate(path)  # Validate the entire capture before replacing evidence.
    c.source(SOURCE, 'https://www.opencompute.org/products', 'project',
             'OCP factual catalog fields and reviewed paraphrases; source rights retained',
             'Selected product pages reviewed via web-tool open/search results. Original HTML is not cached; '
             'direct index download returned 403. This is not a complete catalog or fresh upstream verification. '
             'Brand comes from the product heading; solution provider is a separate role, not factory/OEM proof. '
             'Associated contributions do not establish aliases, current support, firmware entitlement or '
             'hyperscaler deployment. Grouped models remain separate; ordering suffixes are preserved.', path)
    count = 0
    for row, brand, models in prepared:
        for model in models:
            facts = row.get('facts', []) + [
                ['ocp_product_id', str(row['product_id'])], ['source_model_field', row['model_field']],
                ['capture_format', data['capture_format']], ['retrieved_at', data['retrieved_at']],
                ['retrieval_method', row['retrieval_method']],
                ['evidence_scope', 'Selected OCP catalog assertion; factory, unit provenance and current support unverified']]
            c.add(SOURCE, str(row['product_id']) + '|' + model, row['url'], brand, model,
                  brand + ' ' + model, 'network gateway' if model.startswith('EG') else 'network switch',
                  row, [(brand, 'brand'), (row['target_solution_provider'], 'solution_provider')],
                  (), facts)
            count += 1
    c.finish(SOURCE, len(prepared))
    return {'product_pages': len(prepared), 'model_assertions': count}
