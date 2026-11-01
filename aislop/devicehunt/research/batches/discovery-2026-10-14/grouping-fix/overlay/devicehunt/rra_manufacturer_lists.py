"""Reviewed whole manufacturer lists from original Korean approval details.

This is an additional evidence source, not a change to whole-company aliases.
Only the exact reviewed strings below are decomposed. Parenthesized company
relationships, geographic qualifiers and unfamiliar lists remain raw evidence.
"""
import json
from collections import Counter
from pathlib import Path

from . import rra, rra_company_names
from .catalog import digest

SOURCE = 'rra-reviewed-manufacturer-lists'
COMPONENTS = [
    ('ACCTON TECHNOLOGY CORP.', 'ENTERASYS NETWORKS, INC. (ENTERASYS NETWORKS,INC.)'),
    ('ACCTON TECHNOLOGY CORP.', 'ENTERASYS NETWORKS, INC.', 'Enterasys Networks'),
    ('ACCTON TECHNOLOGY CORP.', 'Enterasys Networks (Enterasys Networks)'),
    ('ACCTON TECHNOLOGY CORPORATION', 'JOY TECHNOLOGY (SHEN ZHEN) CO., LTD'),
    ('ARCADYAN TECHNOLOGY CORPORATION', 'Accton Technology Corporation'),
    ('CELESTICA (THAILAND) LIMITED', 'FOXCONN, HONGFUJINPRECISION INDUSTRY(SHENZHEN) CO., LTD'),
    ('CELESTICA(THAILAND) LIMITED', 'FOXCONN, HONGFUJINPRECISION INDUSTRY(SHENZHEN) CO., LTD.'),
    ('CELESTICA (THAILAND) LTD.', 'Celestica(DongGuan-SSL) Technology Ltd.'),
    ('CELESTICA (THAILAND) LTD.', 'Celestica(Dongguan-SSL) Technology limited', 'Flextronics de Mexico S.A.'),
    ('CELESTICA(DONGGUAN-SSL)TECHNOLOGY LTD.', 'SUPER MICRO COMPUTER INC.'),
    ('Celestica de Monterrey S.A. de C.V.', 'Celestica Kladno s.r.o.'),
    ('WISTRON INFOCOMM(ZHONGSHAN) CORPORATION', 'Celestica (Dongguan-SSL) Technology limited'),
    ('Wistron InfoComm (Zhongshan) Corporation', 'Celestica(Dongguan-SSL) Technology limited'),
    ('FLEXTRONICS INDUSTRIAL (ZHUHAI) CO., LTD.', 'Wistron InfoComm (Zhongshan) Corporation',
     'Celestica (Dongguan-SSL) Technology Limited'),
    ('FLEXTRONICS INDUSTRIAL (ZHUHAI) CO., LTD.', 'Wistron InfoComm(Zhongshan) Corporation',
     'Celestica (Dongguan-SSL) Technology Limited'),
    ('Juniper Networks, Inc.', 'Celestica (Suzhou) Technology Co.,Ltd.'),
    ('JUNIPER NETWORKS, INC', 'Celestica (Suzhou) Technology Co.,Ltd.', 'Plexus'),
    ('NETERION CORP', 'Celestica (Thailand) Limited.',
     'FOXCONN, HONGFUJINPRECISION INDUSTRY(SHENZHEN) CO., LTD.'),
    ('FUJITSU IT PRODUCT LIMITED', 'Oracle America,Inc.'),
]
REVIEWED = {','.join(parts): parts for parts in COMPONENTS}
SCOPE = ('Reviewed extraction from existing cached RRA queries, not independent corroboration or '
         'a new retrieval. Selected complete names in explicitly reviewed comma-separated manufacturer cells from '
         'original RRA detail pages. The full field and all query/certificate provenance remain. '
         'These are non-exclusive regulatory manufacturer claims, not sole manufacturer, physical '
         'factory, supplier contract, brand, operator deployment or unit provenance. Unmapped text '
         'and parenthesized relationships are not promoted. Company evidence dependencies apply '
         'only to the associated query. Manufacturer namespace identities do not prove distinct '
         'physical products. Raw dates/statuses do not prove present certification validity.')


def capture_path(root, entry):
    name = entry.get('path') if isinstance(entry, dict) else None
    if (not isinstance(name, str) or not name or Path(name).is_absolute()
            or '..' in Path(name).parts):
        raise ValueError('Invalid manufacturer-list capture dependency path')
    path = (root/name).resolve()
    if root not in path.parents:
        raise ValueError('Manufacturer-list dependency escapes bundle directory')
    if digest(path.read_bytes()) != entry.get('sha256'):
        raise ValueError('Manufacturer-list capture checksum mismatch')
    return path


def prepare(path):
    path = Path(path).resolve(); data = json.loads(path.read_text())
    captures = data.get('captures')
    if (data.get('schema_version') != 1 or data.get('kind') != SOURCE
            or not isinstance(captures, list) or not captures):
        raise ValueError('Unrecognized reviewed RRA manufacturer-list bundle')
    selected = {}; seen_paths = set()
    for capture in captures:
        query_path = capture_path(path.parent, capture)
        if query_path in seen_paths:
            raise ValueError('Duplicate RRA capture dependency')
        seen_paths.add(query_path)
        query, ready = rra.validate(query_path)
        reviewed = rra_company_names.load(query_path.parent, query.get('company_evidence'))
        for _, entry, row in ready:
            parts = REVIEWED.get(row['manufacturer'])
            if not parts:
                continue
            # Only recognized target names are asserted individually. Other
            # list segments are retained as context, not resolved legal entities.
            mapped = [(name, rra.company_target(name, reviewed)) for name in parts]
            targets = {target for _, target in mapped if target}
            for target in sorted(targets):
                names = [name for name, mapped_target in mapped if mapped_target == target]
                company_proof = {name: reviewed[rra.normalize(name)] for name in names
                                 if rra.normalize(name) in reviewed}
                key = (row['certificate_number'], target)
                observation = dict(query=query['query'], capture_sha256=capture['sha256'],
                                   detail_url=entry['url'], detail_sha256=entry['sha256'],
                                   company_evidence=company_proof)
                if key in selected:
                    if selected[key]['row'] != row or selected[key]['names'] != names:
                        raise ValueError('Conflicting manufacturer-list approval observations')
                    selected[key]['observations'].append(observation)
                else:
                    selected[key] = dict(row=row, target=target, names=names,
                                         context_parts=list(parts), observations=[observation])
    claims = []
    for key in sorted(selected):
        item = selected[key]
        for role, model in rra.models(item['row']):
            claims.append(dict(item, model_role=role, model=model))
    summary = dict(approval_target_pairs=len(selected), model_assertions=len(claims),
                   assertions_by_target=dict(sorted(Counter(c['target'] for c in claims).items())))
    if summary != data.get('expected'):
        raise ValueError('Reviewed RRA manufacturer-list counts changed')
    return path, claims, summary


def import_bundle(c, path):
    path, claims, summary = prepare(path)  # All dependencies checked before replacing evidence.
    c.source(SOURCE, rra.FORM, 'regulatory', 'RRA public factual records; source rights retained', SCOPE, path)
    for item in claims:
        row = item['row']; target = item['target']; model = item['model']
        observations = item['observations']; url = observations[0]['detail_url']
        key = row['certificate_number'] + ':' + target + ':' + model
        facts = [('rra_model_role', item['model_role']), ('rra_main_model', row['main_model']),
                 ('rra_manufacturer_as_printed', row['manufacturer']),
                 ('manufacturer_list_context', json.dumps(item['context_parts'], ensure_ascii=False)),
                 ('manufacturer_list_basis', 'reviewed full-cell comma list; individually mapped complete names'),
                 ('country_raw', row['country_raw']), ('approval_date_raw', row['approval_date_raw']),
                 ('other_raw', row['other_raw']), ('evidence_scope', SCOPE)]
        facts += [('manufacturer_list_observation', json.dumps(o, ensure_ascii=False, sort_keys=True))
                  for o in observations]
        c.add(SOURCE, key, url, target, model, target + ' ' + model, row['equipment_name'], item,
              [(name, 'registry_manufacturer') for name in item['names']] +
              [(row['applicant'], 'regulatory_applicant')],
              [('rra_certificate_id', row['certificate_number'])], facts)
        # Apply source-scoped legal-name mappings, never substring normalization.
        # Match through the source key rather than relying on Catalog's ID recipe.
        for name in item['names']:
            c.db.execute('UPDATE affiliations SET target=? WHERE company=? AND role=? AND record_id IN '
                         '(SELECT id FROM records WHERE source_id=? AND source_key=?)',
                         (target, name, 'registry_manufacturer', SOURCE, key))
        # Applicant mapping is independent; no target may inherit a manufacturer role.
        # Re-use only whole-name aliases. Optional manufacturer name evidence is
        # retained in observations, not generalized to unrelated applicant cells.
        c.db.execute('UPDATE affiliations SET target=? WHERE company=? AND role=? AND record_id IN '
                     '(SELECT id FROM records WHERE source_id=? AND source_key=?)',
                     (rra.company_target(row['applicant']), row['applicant'],
                      'regulatory_applicant', SOURCE, key))
    c.finish(SOURCE, summary['approval_target_pairs'])
    return summary
